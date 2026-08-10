# Symphra

**Symphra** is a programming language for composing complete musical works as
text, then rendering them offline to WAV.

Write songs as structured source (`.sym`): instruments, patterns, tracks,
effects, sections, and arrangement. Compile and render with a single CLI. The
project is also a story about humans and language models making music together.

## Status

Symphra is early (workspace version `0.1.0`). The Draft 0.1 language surface is
largely implemented end to end — syntax, compiler, offline renderer, formatter,
and LSP — but the grammar and tooling still evolve. Prefer [`docs/`](docs/) over
ad-hoc examples when something looks out of date.

## Features

- **Declarative songs** — `project`, `song`, instruments, rhythms, patterns,
  tracks, sections, and arrangement in one file
- **Sound sources** — sine / triangle oscillators (optional ADSR), supersaw,
  single-file samples, sample packs, drum machines, SoundFont (`.sf2`), VST3
- **Composition tools** — sequences, fixed-resolution steps, degrees, weighted
  choices, reusable hit/rest rhythms, layered tracks
- **Play pipelines** — `trigger_with`, `gate`, `transpose`, `gain`, `repeat`,
  `reverse`, `pan` / `alternate`, `chance`, sampler `speed`, `choose_sample`,
  `at bar:beat`
- **Effects** — one track effect at a time (`delay`, `filter`, `reverb`), plus
  LFO automation of filter cutoff; master limiter
- **Deterministic rendering** — seeded choices and chance; offline WAV export
- **Editor support** — `symphra-lsp`, VS Code and IntelliJ extensions, formatter

## Quick start

### Requirements

- Rust **1.88+** (edition 2024)

### Build

```console
cargo build --workspace --locked
```

The CLI binary is `symphra` (`apps/symphra-cli`).

### Render a song

```console
cargo run -p symphra --locked -- path/to/song.sym path/to/out.wav
```

If you omit the output path, Symphra writes `song.wav` next to the input.

Try the Draft 0.1 showcase:

```console
cargo run -p symphra --locked -- examples/draft-0.1/001-example.sym
```

### Language server and formatter

```console
cargo build -p symphra-lsp --locked
cargo build -p symphra-formatter --locked
```

- VS Code: open [`editors/vscode`](editors/vscode) and press F5 (see that
  folder’s README)
- IntelliJ: see [`editors/intellij`](editors/intellij)

## Documentation

| Topic | Path |
| --- | --- |
| Purpose and goals | [`docs/01-introduction/`](docs/01-introduction/) |
| Install and first song | [`docs/02-getting-started/`](docs/02-getting-started/) |
| Language guide | [`docs/03-language/`](docs/03-language/) |
| Grammar and pipeline reference | [`docs/04-reference/`](docs/04-reference/) |
| Architecture and contributor notes | [`docs/05-internals/`](docs/05-internals/) |

## Repository layout

```text
apps/           # CLI, LSP, formatter binaries
crates/         # syntax, compiler, score, render, DSP, assets, engine
docs/           # Language and project documentation (English)
editors/        # VS Code and IntelliJ extensions
examples/       # sample .sym projects and assets
tests/          # language and rendering fixtures
xtask/          # workspace maintenance tasks
```

## What works (and what does not)

**Works today:** offline composition and WAV export; deterministic scheduling;
the instrument and effect set above; track-scoped pipelines and sections;
master peak limiter; editor diagnostics and navigation via LSP.

**Not goals (for now):** real-time DAW hosting of Symphra itself, multi-effect
chains per track, bus routing, general parameter automation beyond filter
cutoff, live MIDI I/O as a primary workflow, or a stable language standard.

Details: [Capabilities](docs/01-introduction/03-capabilities.md).

## License

[MIT](LICENSE) © Kanon Mochizuki ([@6jz](https://twitter.com/6jz)).
