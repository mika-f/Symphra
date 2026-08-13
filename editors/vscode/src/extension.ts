import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { execFile, spawn, type ChildProcess } from "node:child_process";
import { createInterface } from "node:readline";
import { promisify } from "node:util";
import * as vscode from "vscode";
import {
  LanguageClient,
  type LanguageClientOptions,
  type ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;
let player: PreviewPlayer | undefined;
let previewDirectory: string | undefined;
let previewGeneration = 0;
let previewSession: PreviewSession | undefined;
let savingDocument: string | undefined;
let playbackStatus: vscode.StatusBarItem | undefined;
const previewTrackMixes = new Map<string, TrackMix>();

type FrameRange = readonly [start: number, end: number];
type SectionPreview = { name: string; startFrame: number; endFrame: number };
type PreviewSession = { uri: string; sectionName?: string };
type TrackMix = { muted: Set<string>; soloed: Set<string> };
type PreviewResponse = { id: number; error: string | null };
type PendingPreview = { resolve: () => void; reject: (error: Error) => void };

const LSP_BINARY_NAME = process.platform === "win32" ? "symphra-lsp.exe" : "symphra-lsp";
const CLI_BINARY_NAME = process.platform === "win32" ? "symphra.exe" : "symphra";
const PLAYER_BINARY_NAME = process.platform === "win32" ? "symphra-player.exe" : "symphra-player";
const execFileAsync = promisify(execFile);

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  playbackStatus = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
  playbackStatus.command = "symphra.stopPlayback";
  playbackStatus.tooltip = "Stop Symphra preview";
  context.subscriptions.push(
    playbackStatus,
    vscode.commands.registerCommand("symphra.restartServer", () => restartServer()),
    vscode.commands.registerCommand("symphra.renderAndPlay", () => renderAndPlay()),
    vscode.commands.registerCommand(
      "symphra.loopSection",
      (uri?: string, sectionName?: string) => loopSection(uri, sectionName),
    ),
    vscode.commands.registerCommand("symphra.stopPlayback", () => stopPlayback()),
    vscode.commands.registerCommand("symphra.toggleMute", (uri: string, name: string) =>
      toggleTrack(uri, name, "muted"),
    ),
    vscode.commands.registerCommand("symphra.toggleSolo", (uri: string, name: string) =>
      toggleTrack(uri, name, "soloed"),
    ),
    vscode.workspace.onDidSaveTextDocument((document) => {
      if (document.uri.toString() !== savingDocument) {
        void refreshPreview(document);
      }
    }),
    // CodeLens "N references" clicks: server sends LSP positions/locations as plain JSON.
    vscode.commands.registerCommand(
      "symphra.showReferences",
      (
        uri: string,
        position: { line: number; character: number },
        locations: Array<{
          uri: string;
          range: {
            start: { line: number; character: number };
            end: { line: number; character: number };
          };
        }>,
      ) => {
        const vscodeUri = vscode.Uri.parse(uri);
        const vscodePosition = new vscode.Position(position.line, position.character);
        const vscodeLocations = locations.map(
          (location) =>
            new vscode.Location(
              vscode.Uri.parse(location.uri),
              new vscode.Range(
                new vscode.Position(location.range.start.line, location.range.start.character),
                new vscode.Position(location.range.end.line, location.range.end.character),
              ),
            ),
        );
        return vscode.commands.executeCommand(
          "editor.action.showReferences",
          vscodeUri,
          vscodePosition,
          vscodeLocations,
        );
      },
    ),
  );
  await startServer();
}

export async function deactivate(): Promise<void> {
  stopPlayback();
  player?.dispose();
  player = undefined;
  await client?.stop();
}

async function startServer(): Promise<void> {
  const command = resolveServerCommand();
  const serverOptions: ServerOptions = {
    command,
    args: [],
    transport: TransportKind.stdio,
  };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "symphra" }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.sym"),
    },
  };

  client = new LanguageClient("symphra", "Symphra Language Server", serverOptions, clientOptions);

  try {
    await client.start();
    await Promise.all(
      [...previewTrackMixes].map(([uri, mix]) => sendPreviewTrackState(uri, mix)),
    );
  } catch (error) {
    void vscode.window.showErrorMessage(
      `Symphra: failed to start the language server ("${command}"). ` +
        `Build it with "cargo build -p symphra-lsp" or set "symphra.server.path". ${describe(error)}`,
    );
  }
}

async function restartServer(): Promise<void> {
  await client?.stop();
  await startServer();
}

function resolveServerCommand(): string {
  const configured = vscode.workspace.getConfiguration("symphra").get<string>("server.path");
  if (configured) {
    return configured;
  }
  return findWorkspaceBinary(LSP_BINARY_NAME) ?? LSP_BINARY_NAME;
}

function resolveCliCommand(): string {
  const configured = vscode.workspace.getConfiguration("symphra").get<string>("cli.path");
  if (configured) {
    return configured;
  }
  return findWorkspaceBinary(CLI_BINARY_NAME) ?? CLI_BINARY_NAME;
}

function resolvePlayerCommand(): string {
  const configured = vscode.workspace.getConfiguration("symphra").get<string>("player.path");
  if (configured) {
    return configured;
  }
  return findWorkspaceBinary(PLAYER_BINARY_NAME) ?? PLAYER_BINARY_NAME;
}

function findWorkspaceBinary(binaryName: string): string | undefined {
  for (const folder of vscode.workspace.workspaceFolders ?? []) {
    for (const profile of ["debug", "release"]) {
      const candidate = path.join(folder.uri.fsPath, "target", profile, binaryName);
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    }
  }
  return undefined;
}

async function renderAndPlay(): Promise<void> {
  const document = vscode.window.activeTextEditor?.document;
  if (!document || document.languageId !== "symphra" || document.uri.scheme !== "file") {
    void vscode.window.showErrorMessage("Symphra: open a saved .sym file to render it.");
    return;
  }
  if (await renderDocumentAndPlay(document)) {
    setPreviewSession({ uri: document.uri.toString() });
  }
}

async function loopSection(uri?: string, sectionName?: string): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  const document = uri
    ? await vscode.workspace.openTextDocument(vscode.Uri.parse(uri))
    : editor?.document;
  if (!document || document.languageId !== "symphra" || document.uri.scheme !== "file") {
    void vscode.window.showErrorMessage("Symphra: open a saved .sym file to loop a section.");
    return;
  }
  if (!client) {
    void vscode.window.showErrorMessage("Symphra: the language server is not running.");
    return;
  }

  let section: SectionPreview | null;
  try {
    const selector = sectionName
      ? { sectionName }
      : {
          position: {
            line: editor?.selection.active.line ?? 0,
            character: editor?.selection.active.character ?? 0,
          },
        };
    section = await requestSectionPreview(document, selector);
  } catch (error) {
    void vscode.window.showErrorMessage(`Symphra: could not resolve the section. ${describe(error)}`);
    return;
  }
  if (!section) {
    void vscode.window.showErrorMessage(
      "Symphra: place the cursor inside a section used by the arrangement.",
    );
    return;
  }
  const range = sectionFrameRange(section);
  if (!range) {
    void vscode.window.showErrorMessage("Symphra: the section playback range is invalid.");
    return;
  }
  if (await renderDocumentAndPlay(document, range, section.name)) {
    setPreviewSession({ uri: document.uri.toString(), sectionName: section.name });
  }
}

async function renderDocumentAndPlay(
  document: vscode.TextDocument,
  range?: FrameRange,
  sectionName?: string,
  automatic = false,
): Promise<boolean> {
  savingDocument = document.uri.toString();
  try {
    if (!(await document.save())) {
      return false;
    }
  } finally {
    savingDocument = undefined;
  }

  const generation = ++previewGeneration;
  const directory = await fs.promises.mkdtemp(path.join(os.tmpdir(), "symphra-preview-"));
  const output = path.join(directory, "preview.wav");
  const command = resolveCliCommand();

  try {
    await vscode.window.withProgress(
      {
        location: automatic ? vscode.ProgressLocation.Window : vscode.ProgressLocation.Notification,
        title: sectionName
          ? `Symphra: rendering section ${sectionName}`
          : "Symphra: rendering preview",
      },
      () => execFileAsync(command, renderArguments(document, output)),
    );
  } catch (error) {
    await fs.promises.rm(directory, { recursive: true, force: true });
    void vscode.window.showErrorMessage(
      `Symphra: render failed ("${command}"). ${commandError(error)}`,
    );
    return false;
  }

  if (generation === previewGeneration) {
    return startPlayback(resolvePlayerCommand(), output, directory, range);
  } else {
    void removePreview(directory);
    return false;
  }
}

function stopPlayback(): void {
  clearPreviewSession();
  previewGeneration += 1;
  void player?.stop().catch(() => {});
  if (previewDirectory) {
    void removePreview(previewDirectory);
  }
}

function setPreviewSession(session: PreviewSession): void {
  previewSession = session;
  if (!playbackStatus) {
    return;
  }
  const filename = path.basename(vscode.Uri.parse(session.uri).fsPath);
  const mix = previewTrackMixes.get(session.uri);
  const mixStatus = mix
    ? `${mix.muted.size ? ` M${mix.muted.size}` : ""}${mix.soloed.size ? ` S${mix.soloed.size}` : ""}`
    : "";
  playbackStatus.text = `$(debug-stop) ${filename}${session.sectionName ? `: ${session.sectionName}` : ""}${mixStatus}`;
  playbackStatus.show();
}

function clearPreviewSession(): void {
  previewSession = undefined;
  playbackStatus?.hide();
}

async function refreshPreview(document: vscode.TextDocument): Promise<void> {
  const session = previewSession;
  if (!session || session.uri !== document.uri.toString()) {
    return;
  }

  let section: SectionPreview | null = null;
  if (session.sectionName) {
    try {
      section = await requestSectionPreview(document, { sectionName: session.sectionName });
    } catch (error) {
      void vscode.window.showErrorMessage(
        `Symphra: could not refresh section ${session.sectionName}. ${describe(error)}`,
      );
      return;
    }
    if (!section) {
      void vscode.window.showErrorMessage(
        `Symphra: section ${session.sectionName} is no longer in the arrangement.`,
      );
      return;
    }
  }
  if (previewSession !== session) {
    return;
  }

  const range = section ? sectionFrameRange(section) : undefined;
  if (section && !range) {
    void vscode.window.showErrorMessage("Symphra: the section playback range is invalid.");
    return;
  }
  await renderDocumentAndPlay(document, range, section?.name, true);
}

async function toggleTrack(
  uri: string,
  name: string,
  kind: "muted" | "soloed",
): Promise<void> {
  const mix = previewTrackMixes.get(uri) ?? { muted: new Set<string>(), soloed: new Set<string>() };
  previewTrackMixes.set(uri, mix);
  const tracks = mix[kind];
  if (tracks.has(name)) {
    tracks.delete(name);
  } else {
    tracks.add(name);
  }
  try {
    await sendPreviewTrackState(uri, mix);
  } catch (error) {
    void vscode.window.showErrorMessage(`Symphra: could not update track state. ${describe(error)}`);
    return;
  }
  if (previewSession?.uri === uri) {
    setPreviewSession(previewSession);
    await refreshPreview(await vscode.workspace.openTextDocument(vscode.Uri.parse(uri)));
  }
}

function sendPreviewTrackState(uri: string, mix: TrackMix): Promise<void> {
  if (!client) {
    return Promise.reject(new Error("the language server is not running"));
  }
  return client.sendRequest("symphra/setPreviewTrackState", {
    textDocument: { uri },
    muted: [...mix.muted],
    soloed: [...mix.soloed],
  });
}

function renderArguments(document: vscode.TextDocument, output: string): string[] {
  const args = [document.uri.fsPath, output];
  const mix = previewTrackMixes.get(document.uri.toString());
  for (const name of [...(mix?.muted ?? [])].sort()) {
    args.push("--mute", name);
  }
  for (const name of [...(mix?.soloed ?? [])].sort()) {
    args.push("--solo", name);
  }
  return args;
}

function requestSectionPreview(
  document: vscode.TextDocument,
  selector: { position: { line: number; character: number } } | { sectionName: string },
): Promise<SectionPreview | null> {
  if (!client) {
    return Promise.reject(new Error("the language server is not running"));
  }
  return client.sendRequest("symphra/sectionPreview", {
    textDocument: { uri: document.uri.toString() },
    ...selector,
  });
}

function sectionFrameRange(section: SectionPreview): FrameRange | undefined {
  const range: FrameRange = [section.startFrame, section.endFrame];
  return range.every(Number.isSafeInteger) && range[0] >= 0 && range[0] < range[1]
    ? range
    : undefined;
}

async function startPlayback(
  command: string,
  output: string,
  directory: string,
  range?: FrameRange,
): Promise<boolean> {
  try {
    const activePlayer = player ?? PreviewPlayer.start(command);
    player = activePlayer;
    await activePlayer.play(output, range);
    const previousDirectory = previewDirectory;
    previewDirectory = directory;
    if (previousDirectory) {
      void removePreview(previousDirectory);
    }
    return true;
  } catch (error) {
    player?.dispose();
    player = undefined;
    stopPlayback();
    void removePreview(directory);
    void vscode.window.showErrorMessage(`Symphra: playback failed ("${command}"). ${describe(error)}`);
    return false;
  }
}

async function removePreview(directory: string): Promise<void> {
  if (previewDirectory === directory) {
    previewDirectory = undefined;
  }
  try {
    await fs.promises.rm(directory, { recursive: true, force: true });
  } catch {
    // The player may briefly retain the WAV on Windows; the OS temp directory is disposable.
  }
}

class PreviewPlayer {
  private readonly pending = new Map<number, PendingPreview>();
  private nextRequestId = 1;
  private stderr = "";

  constructor(private readonly child: ChildProcess) {
    const output = child.stdout;
    if (!output || !child.stdin) {
      throw new Error("could not create the preview player protocol streams");
    }
    const lines = createInterface({ input: output });
    lines.on("line", (line) => this.handleResponse(line));
    child.stderr?.setEncoding("utf8");
    child.stderr?.on("data", (chunk: string) => {
      this.stderr = (this.stderr + chunk).slice(-8_192);
    });
    child.once("error", (error) => this.fail(describe(error)));
    child.once("exit", (code) => {
      this.fail(this.stderr.trim() || `exit code ${code ?? "unknown"}`);
      if (player === this) {
        player = undefined;
        stopPlayback();
      }
    });
  }

  static start(command: string): PreviewPlayer {
    return new PreviewPlayer(
      spawn(command, ["--server"], {
        stdio: ["pipe", "pipe", "pipe"],
        windowsHide: true,
      }),
    );
  }

  play(path: string, range?: FrameRange): Promise<void> {
    return this.request({
      path,
      ...(range ? { start_frame: range[0], end_frame: range[1] } : {}),
    });
  }

  stop(): Promise<void> {
    return this.request({});
  }

  dispose(): void {
    this.fail("preview player was stopped");
    this.child.kill();
  }

  private request(payload: Omit<Record<string, unknown>, "id">): Promise<void> {
    const id = this.nextRequestId++;
    return new Promise<void>((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.child.stdin?.write(`${JSON.stringify({ id, ...payload })}\n`, (error) => {
        if (error) {
          this.pending.delete(id);
          reject(error);
        }
      });
    });
  }

  private handleResponse(line: string): void {
    let response: PreviewResponse;
    try {
      response = JSON.parse(line) as PreviewResponse;
    } catch {
      return;
    }
    const pending = this.pending.get(response.id);
    if (!pending) {
      return;
    }
    this.pending.delete(response.id);
    if (response.error) {
      pending.reject(new Error(response.error));
    } else {
      pending.resolve();
    }
  }

  private fail(message: string): void {
    for (const pending of this.pending.values()) {
      pending.reject(new Error(message));
    }
    this.pending.clear();
  }
}

function commandError(error: unknown): string {
  if (typeof error === "object" && error && "stderr" in error) {
    const stderr = String(error.stderr).trim();
    if (stderr) {
      return stderr;
    }
  }
  return describe(error);
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
