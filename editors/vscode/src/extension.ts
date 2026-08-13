import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import * as vscode from "vscode";
import {
  LanguageClient,
  type LanguageClientOptions,
  type ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;
let player: vscode.WebviewPanel | undefined;
let previewDirectory: string | undefined;

const LSP_BINARY_NAME = process.platform === "win32" ? "symphra-lsp.exe" : "symphra-lsp";
const CLI_BINARY_NAME = process.platform === "win32" ? "symphra.exe" : "symphra";
const execFileAsync = promisify(execFile);

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  context.subscriptions.push(
    vscode.commands.registerCommand("symphra.restartServer", () => restartServer()),
    vscode.commands.registerCommand("symphra.renderAndPlay", () => renderAndPlay()),
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
  if (!(await document.save())) {
    return;
  }

  stopPlayback();
  const directory = await fs.promises.mkdtemp(path.join(os.tmpdir(), "symphra-preview-"));
  const output = path.join(directory, "preview.wav");
  const command = resolveCliCommand();

  try {
    await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Notification, title: "Symphra: rendering preview" },
      () => execFileAsync(command, [document.uri.fsPath, output]),
    );
  } catch (error) {
    await fs.promises.rm(directory, { recursive: true, force: true });
    void vscode.window.showErrorMessage(
      `Symphra: render failed ("${command}"). ${commandError(error)}`,
    );
    return;
  }

  previewDirectory = directory;
  const outputUri = vscode.Uri.file(output);
  const panel = vscode.window.createWebviewPanel(
    "symphraPlayer",
    `Symphra: ${path.basename(document.uri.fsPath)}`,
    vscode.ViewColumn.Beside,
    { localResourceRoots: [vscode.Uri.file(directory)] },
  );
  player = panel;
  const audioUri = panel.webview.asWebviewUri(outputUri);
  panel.webview.html = playerHtml(audioUri, path.basename(document.uri.fsPath));
  panel.onDidDispose(() => {
    if (player === panel) {
      player = undefined;
    }
    void removePreview(directory);
  });
}

function stopPlayback(): void {
  player?.dispose();
  player = undefined;
  if (previewDirectory) {
    void removePreview(previewDirectory);
  }
}

async function removePreview(directory: string): Promise<void> {
  if (previewDirectory === directory) {
    previewDirectory = undefined;
  }
  try {
    await fs.promises.rm(directory, { recursive: true, force: true });
  } catch {
    // The webview may briefly retain the WAV on Windows; the OS temp directory is disposable.
  }
}

function playerHtml(audioUri: vscode.Uri, fileName: string): string {
  const name = escapeHtml(fileName);
  return `<!doctype html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; media-src ${audioUri.scheme}:; style-src 'unsafe-inline';">
  <title>${name}</title>
</head>
<body>
  <p>${name}</p>
  <audio src="${audioUri}" controls autoplay></audio>
</body>
</html>`;
}

function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, (character) => `&#${character.charCodeAt(0)};`);
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
