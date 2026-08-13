# Tooling

## CLI (`symphra`)

```console
symphra <input.sym> [output.wav] [--mute <track>]... [--solo <track>]...
```

- Compiles source, loads referenced assets, renders, encodes WAV
- Shows live terminal progress for compilation, asset loading, rendering, and WAV encoding
- Reuses unchanged post-effect track audio from `.symphra-cache/` beside the source file
- Renders independent non-VST3 tracks across available CPU cores, then mixes them in source order
- Default output: same path as the input with a `.wav` extension
- `--mute` excludes a named track declaration; `--solo` includes only named
  track declarations. Both flags are repeatable, and mute takes precedence.
- Prints `wrote <path>` on success; diagnostics go to stderr on failure

## Preview player (`symphra-player`)

```console
symphra-player <input.wav>
```

The VS Code extension uses `symphra-player --server` to keep the output device
open between save-to-refresh previews, avoiding player startup latency.

Plays a PCM 16-bit WAV continuously through the default output device. The
process runs until it is terminated. The VS Code extension manages this
process for background loop previews; it is not a real-time Symphra host.

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
2. Build `symphra-lsp` and `symphra-player` from the monorepo root
3. Open the extension folder and press F5

By default the client looks for `target/debug/symphra-lsp` or
`target/release/symphra-lsp` under workspace folders, then `PATH`. Override with
setting `symphra.server.path`.

### IntelliJ

See [`editors/intellij`](https://github.com/mika-f/Symphra/tree/main/editors/intellij).
