---
name: symphra
description: Understand, write, explain, review, debug, format, and validate Symphra music source (`.sym`) for offline WAV composition. Use for Symphra language syntax, instruments, patterns, rhythms, tracks, pipelines, effects, sections, arrangements, compiler diagnostics, or questions about what Draft 0.1 supports. Do not use this skill for translating another music language unless the request also requires Symphra language knowledge.
---

# Symphra

Treat Symphra as a declarative language for complete, deterministic, offline-rendered songs. Model a file as render settings plus musical context, sound sources, material, performances, form, and optional master processing.

## Establish the source of truth

When working in the Symphra repository, prefer current sources in this order:

1. Parser and compiler tests under `crates/symphra-syntax/tests` and `crates/symphra-compiler/tests`
2. Language and reference pages under `docs/03-language` and `docs/04-reference`
3. Examples under `examples/draft-0.1`
4. Bundled references in this skill

Use the bundled references when the repository is unavailable or a concise overview is enough. Never invent syntax to fill a gap; state that a feature is unsupported or verify it against the implementation.

## Load only the needed reference

- Read [references/language.md](references/language.md) before creating, editing, explaining, or reviewing `.sym` source.
- Read [references/capabilities.md](references/capabilities.md) when choosing an implementation, answering support questions, or diagnosing an invalid design.

## Follow the authoring workflow

1. Identify whether the request is explanation, review, repair, or composition.
2. Preserve the user's musical intent, existing names, assets, meter, tempo, and arrangement unless asked to change them.
3. For new source, start with `project` and `song`, then add only the declarations needed for audible output.
4. Use a bare-pattern arrangement for a small sketch. Use tracks, sections, and a section arrangement for multi-part songs. Never mix both arrangement forms.
5. Prefer built-in `sine` or `triangle` for self-contained examples. Reference samples, SoundFonts, or VST3 only when the required relative assets are available or explicitly requested.
6. Keep seeded choices deterministic. Do not describe `project.seed` as generative composition; it only fixes explicit probabilistic choices.
7. Check instrument-specific pipeline restrictions and current limitations before presenting source as valid.

## Validate changes

When tools and the repository are available:

1. Format a changed file with `cargo run -p symphra-formatter --locked -- <file.sym>`.
2. Render with `cargo run -p symphra --locked -- <file.sym> <output.wav>` when assets are available and audio output is useful.
3. Use relevant syntax/compiler tests for language implementation changes.

The formatter proves parsing, not all compiler or asset checks. Rendering exercises the complete compile and audio path. If validation cannot run, say exactly which checks were not performed.

## Keep responsibilities narrow

Explain Symphra concepts in Symphra's own terms. Leave cross-language comparisons and translation policies to separate skills; this skill supplies the Symphra-side facts they may depend on.
