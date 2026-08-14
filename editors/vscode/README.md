# Symphra for Visual Studio Code

Syntax highlighting and language server integration for Symphra (`.sym`) files.
The grammar is still evolving (see the [language guide](../../docs/03-language/01-overview.md)
and [grammar reference](../../docs/04-reference/01-grammar.md)); this extension
tracks the current draft and will need updates as the language grows.

## What it provides

- TextMate grammar aligned with `crates/symphra-syntax` token kinds: keywords
  (including tracks, effects, pipelines, automate/lfo, drums, master,
  `arpeggiate` / `style` / `octaves`, `step` / `fit`, and `vst3`), comments,
  strings, numbers, rate literals (`48khz`, `150bpm`), pitch literals (`C4`),
  and punctuation (`|>`, `..`, `:`, `*`, `[]`, `()`, `,`).
- A language client that launches `symphra-lsp` over stdio and forwards
  diagnostics, document symbols, completions, semantic tokens, and inlay hints.
- **Mute** and **Solo** CodeLens controls above each track declaration, plus a
  **▶ Loop section** CodeLens above each section declaration and **▶ From
  here** above each section-based arrangement entry.
- A status bar item showing the current file and section; click it to stop playback.
  It also shows the last render duration and track-cache hits, such as
  `184ms · cache 3/4`.
- A BPM-synchronized `▶ bar.beat` cursor on the active arrangement entry and
  a subtle whole-line highlight over the section currently being played.
- Playback highlighting for sequence `note`, `chord`, and `rest` items plus
  rhythm `hit` / `rest` cells, including derived and arpeggiated patterns and
  reversed playback.
- **Render and Play**, **Loop Section at Cursor**, and **Stop Playback** commands
  backed by the `symphra` CLI and a persistent background `symphra-player` process. The
  player loops either the full render or the arranged occurrence of the section
  containing the cursor, without opening a player panel. Saving the active
  preview document re-renders and replaces the loop; **Stop Playback** disables
  this automatic refresh. Clicking Mute or Solo during playback also refreshes
  the current loop automatically.

## Building and running

From this directory:

```console
npm install
npm run compile
```

Build the language server and preview player from the repository root:

```console
cargo build -p symphra -p symphra-lsp -p symphra-player --locked
```

Then open this `editors/vscode` folder in VS Code and press F5 to launch an
Extension Development Host. Open a workspace containing a `.sym` file (e.g.
[`examples/draft-0.1/001-example.sym`](../../examples/draft-0.1/001-example.sym)).

## Locating the language server

By default the extension looks for `target/debug/symphra-lsp` or
`target/release/symphra-lsp` under each workspace folder, then falls back to
`symphra-lsp` on `PATH`. Set `symphra.server.path` to point at a specific
binary if that doesn't find it (e.g. when the extension host's workspace
folder isn't the repository root).

## Settings

- `symphra.server.path`: path to the `symphra-lsp` executable.
- `symphra.cli.path`: path to the `symphra` executable used for preview rendering.
- `symphra.player.path`: path to the `symphra-player` executable used for background playback.
- `symphra.trace.server`: `off` | `messages` | `verbose`, for LSP wire tracing
  in the "Symphra Language Server" output channel.

## Commands

- **Symphra: Restart Language Server** (`symphra.restartServer`)
- **Symphra: Render and Play** (`symphra.renderAndPlay`)
- **Symphra: Loop Section at Cursor** (`symphra.loopSection`)
- **Symphra: Play Arrangement from Here** (`symphra.playFromHere`)
- **Symphra: Stop Playback** (`symphra.stopPlayback`)

After starting either preview command, save the same `.sym` document to refresh
the background loop automatically. A section preview follows the named section
even when tempo, meter, or earlier arrangement entries change.
