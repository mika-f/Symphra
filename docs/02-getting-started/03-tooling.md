# Tooling

## CLI (`symphra`)

```console
symphra <input.sym> [output.wav]
```

- Compiles source, loads referenced assets, renders, encodes WAV
- Default output: same path as the input with a `.wav` extension
- Prints `wrote <path>` on success; diagnostics go to stderr on failure

There are no extra flags yet — configuration lives in the source file.

## Formatter (`symphra-formatter`)

Formats Symphra source in a stable layout (pipeline stages, rhythm hit/rest
lines, declaration spacing). Prefer running it before commit when editing
`.sym` files or language tests.

```console
cargo run -p symphra-formatter --locked -- path/to/file.sym
```

(See the formatter crate/tests for the exact stdin/file interface used in CI.)

## Language server (`symphra-lsp`)

Speaks LSP over **stdio**. Capabilities include:

- Lexical, syntax, and compiler diagnostics
- Document symbols
- Keyword and song-local name completion
- Hover (keywords; MIDI note numbers for pitches)
- Go to definition / find references / rename
- Semantic tokens and inlay hints

Build:

```console
cargo build -p symphra-lsp --locked
```

### VS Code

Extension sources: [`editors/vscode`](https://github.com/mika-f/Symphra/tree/main/editors/vscode).

1. `npm install` / `npm run compile` in that folder
2. Build `symphra-lsp` from the monorepo root
3. Open the extension folder and press F5

By default the client looks for `target/debug/symphra-lsp` or
`target/release/symphra-lsp` under workspace folders, then `PATH`. Override with
setting `symphra.server.path`.

### IntelliJ

See [`editors/intellij`](https://github.com/mika-f/Symphra/tree/main/editors/intellij).
