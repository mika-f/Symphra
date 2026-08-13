import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { execFile, spawn, type ChildProcess } from "node:child_process";
import { promisify } from "node:util";
import * as vscode from "vscode";
import {
  LanguageClient,
  type LanguageClientOptions,
  type ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;
let playback: ChildProcess | undefined;
let previewDirectory: string | undefined;
let playbackGeneration = 0;

type FrameRange = readonly [start: number, end: number];
type SectionPreview = { name: string; startFrame: number; endFrame: number };

const LSP_BINARY_NAME = process.platform === "win32" ? "symphra-lsp.exe" : "symphra-lsp";
const CLI_BINARY_NAME = process.platform === "win32" ? "symphra.exe" : "symphra";
const PLAYER_BINARY_NAME = process.platform === "win32" ? "symphra-player.exe" : "symphra-player";
const execFileAsync = promisify(execFile);

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  context.subscriptions.push(
    vscode.commands.registerCommand("symphra.restartServer", () => restartServer()),
    vscode.commands.registerCommand("symphra.renderAndPlay", () => renderAndPlay()),
    vscode.commands.registerCommand("symphra.loopSection", () => loopSection()),
    vscode.commands.registerCommand("symphra.stopPlayback", () => stopPlayback()),
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
  await renderDocumentAndPlay(document);
}

async function loopSection(): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  const document = editor?.document;
  if (!editor || !document || document.languageId !== "symphra" || document.uri.scheme !== "file") {
    void vscode.window.showErrorMessage("Symphra: open a saved .sym file to loop a section.");
    return;
  }
  if (!client) {
    void vscode.window.showErrorMessage("Symphra: the language server is not running.");
    return;
  }

  let section: SectionPreview | null;
  try {
    section = await client.sendRequest<SectionPreview | null>("symphra/sectionPreview", {
      textDocument: { uri: document.uri.toString() },
      position: {
        line: editor.selection.active.line,
        character: editor.selection.active.character,
      },
    });
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
  const range: FrameRange = [section.startFrame, section.endFrame];
  if (!range.every(Number.isSafeInteger) || range[0] < 0 || range[0] >= range[1]) {
    void vscode.window.showErrorMessage("Symphra: the section playback range is invalid.");
    return;
  }
  await renderDocumentAndPlay(document, range, section.name);
}

async function renderDocumentAndPlay(
  document: vscode.TextDocument,
  range?: FrameRange,
  sectionName?: string,
): Promise<void> {
  if (!(await document.save())) {
    return;
  }

  stopPlayback();
  const generation = playbackGeneration;
  const directory = await fs.promises.mkdtemp(path.join(os.tmpdir(), "symphra-preview-"));
  const output = path.join(directory, "preview.wav");
  const command = resolveCliCommand();

  try {
    await vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Notification,
        title: sectionName
          ? `Symphra: rendering section ${sectionName}`
          : "Symphra: rendering preview",
      },
      () => execFileAsync(command, [document.uri.fsPath, output]),
    );
  } catch (error) {
    await fs.promises.rm(directory, { recursive: true, force: true });
    void vscode.window.showErrorMessage(
      `Symphra: render failed ("${command}"). ${commandError(error)}`,
    );
    return;
  }

  if (generation === playbackGeneration) {
    startPlayback(resolvePlayerCommand(), output, directory, range);
  } else {
    void removePreview(directory);
  }
}

function stopPlayback(): void {
  playbackGeneration += 1;
  playback?.kill();
  playback = undefined;
  if (previewDirectory) {
    void removePreview(previewDirectory);
  }
}

function startPlayback(
  command: string,
  output: string,
  directory: string,
  range?: FrameRange,
): void {
  const args = range ? [output, String(range[0]), String(range[1])] : [output];
  const child = spawn(command, args, {
    stdio: ["ignore", "ignore", "pipe"],
    windowsHide: true,
  });
  let stderr = "";
  child.stderr?.setEncoding("utf8");
  child.stderr?.on("data", (chunk: string) => {
    stderr = (stderr + chunk).slice(-8_192);
  });
  playback = child;
  previewDirectory = directory;

  child.once("error", (error) => finishPlayback(child, directory, command, describe(error)));
  child.once("exit", (code) => {
    const error = code && code !== 0 ? stderr.trim() || `exit code ${code}` : undefined;
    finishPlayback(child, directory, command, error);
  });
}

function finishPlayback(
  child: ChildProcess,
  directory: string,
  command: string,
  error: string | undefined,
): void {
  const current = playback === child;
  if (current) {
    playback = undefined;
  }
  void removePreview(directory);
  if (current && error) {
    void vscode.window.showErrorMessage(`Symphra: playback failed ("${command}"). ${error}`);
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
