import * as fs from "node:fs";
import * as path from "node:path";
import * as vscode from "vscode";
import {
  LanguageClient,
  type LanguageClientOptions,
  type ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

const BINARY_NAME = process.platform === "win32" ? "symphra-lsp.exe" : "symphra-lsp";

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  context.subscriptions.push(
    vscode.commands.registerCommand("symphra.restartServer", () => restartServer()),
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
  return findWorkspaceBinary() ?? BINARY_NAME;
}

function findWorkspaceBinary(): string | undefined {
  for (const folder of vscode.workspace.workspaceFolders ?? []) {
    for (const profile of ["debug", "release"]) {
      const candidate = path.join(folder.uri.fsPath, "target", profile, BINARY_NAME);
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    }
  }
  return undefined;
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
