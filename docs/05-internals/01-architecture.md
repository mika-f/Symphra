# Architecture

Symphra is a Cargo workspace of small crates. Data flows one way from text to
PCM.

## Pipeline

```text
┌─────────────┐   ┌──────────────────┐   ┌──────────────┐
│ .sym source │ → │ symphra-syntax   │ → │ AST + spans  │
└─────────────┘   │  lexer, parser   │   └──────┬───────┘
                  └──────────────────┘          │
                                               ▼
                  ┌──────────────────┐   ┌──────────────┐
                  │ symphra-compiler │ → │ HIR / checks │
                  └──────────────────┘   └──────┬───────┘
                                               ▼
                  ┌──────────────────┐   ┌──────────────┐
                  │  symphra-score   │ → │ Score IR     │
                  └──────────────────┘   └──────┬───────┘
                                               ▼
   assets ──► ┌──────────────────┐   ┌──────────────────────┐
 samples      │ symphra-render   │ → │ float buffers        │
 soundfonts   │ + symphra-dsp    │   │ (+ track FX, master) │
 vst3         └──────────────────┘   └──────────┬───────────┘
                                               ▼
                  ┌──────────────────┐   ┌──────────────┐
                  │ symphra-export   │ → │ WAV bytes    │
                  └──────────────────┘   └──────────────┘
```

`symphra-engine` is the façade used by the CLI (and tests) to compile source and
render with asset libraries.

## Crates (apps)

| Crate | Role |
| --- | --- |
| `symphra` (`apps/symphra-cli`) | CLI entry: path in → WAV out |
| `symphra-lsp` | Language server over stdio |
| `symphra-formatter` | Source formatter |

## Crates (libraries)

| Crate | Role |
| --- | --- |
| `symphra-syntax` | Lex, parse, AST, diagnostics spans |
| `symphra-fmt` | Formatting printer |
| `symphra-compiler` | AST → HIR, validation, score lowering inputs |
| `symphra-score` | Score data structures |
| `symphra-render` | Offline voice / track / master rendering |
| `symphra-dsp` | Delay, filter, reverb, limiter primitives |
| `symphra-sampler` | WAV decode and sample library helpers |
| `symphra-soundfont` | SoundFont load/synth wrapper (`rustysynth`) |
| `symphra-vst3` | VST3 host integration (`vst3-host`) |
| `symphra-export` | WAV encode |
| `symphra-engine` | Shared compile + render API |
| `symphra-wasm` | WebAssembly-facing surface (experimental) |

## Editors

| Path | Role |
| --- | --- |
| `editors/vscode` | TextMate grammar + LSP client |
| `editors/intellij` | JetBrains plugin |

## Design invariants worth knowing

1. **Determinism** — musical “randomness” is seeded
2. **Track-local FX** — no send bus graph
3. **One effect per track** — keeps mix order obvious
4. **Tests over prose** — when docs and tests disagree, fix the docs
5. **MIT-friendly audio stack** — prefer non-GPL host bindings for VST3

Deeper, historical gap tracking:
[Implementation status](./02-implementation-status.md).
