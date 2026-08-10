# Symphra for Visual Studio Code

Syntax highlighting and language server integration for Symphra (`.sym`) files.
The grammar is still evolving (see [`docs/language/draft-0.1.md`](../../docs/language/draft-0.1.md));
this extension tracks the current draft and will need updates as the language grows.

## What it provides

- TextMate grammar aligned with `crates/symphra-syntax` token kinds: keywords
  (including tracks, effects, pipelines, automate/lfo, drums, and master),
  comments, strings, numbers, rate literals (`48khz`, `150bpm`), pitch
  literals (`C4`), and punctuation (`|>`, `..`, `:`, `()`, `,`).
- A language client that launches `symphra-lsp` over stdio and forwards
  diagnostics, document symbols, and completions.

## Building and running

From this directory:

```console
npm install
npm run compile
```

Build the language server from the repository root:

```console
cargo build -p symphra-lsp --locked
```

Then open this `editors/vscode` folder in VS Code and press F5 to launch an
Extension Development Host. Open a workspace containing a `.sym` file (e.g.
[`examples/draft-0.1/001-infinite-sparkle.sym`](../../examples/draft-0.1/001-infinite-sparkle.sym)).

## Locating the language server

By default the extension looks for `target/debug/symphra-lsp` or
`target/release/symphra-lsp` under each workspace folder, then falls back to
`symphra-lsp` on `PATH`. Set `symphra.server.path` to point at a specific
binary if that doesn't find it (e.g. when the extension host's workspace
folder isn't the repository root).

## Settings

- `symphra.server.path`: path to the `symphra-lsp` executable.
- `symphra.trace.server`: `off` | `messages` | `verbose`, for LSP wire tracing
  in the "Symphra Language Server" output channel.

## Commands

- **Symphra: Restart Language Server** (`symphra.restartServer`)
