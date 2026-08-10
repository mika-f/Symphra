# Goals and non-goals

## Goals

### Complete works as source

Express a full piece in one language: harmony, groove, texture, form, and a
usable export path. Draft 0.1 targets structured songs (intro / build / drop /
outro style forms), not only eight-bar loops.

### Offline, deterministic rendering

Rendering is a batch process from source (and local assets) to WAV. Randomness
is seeded so experiments and tests stay reproducible.

### Honest audio, small language

Prefer a compact grammar with clear ownership (track-scoped effects, fixed
pipeline phases) over a sprawling DAW-in-text. Each language feature should
have a path through syntax → HIR → score → audio.

### Tooling that matches a programming language

Parser diagnostics, a formatter, an LSP (symbols, completion, rename,
semantic tokens), and editor extensions are first-class — not afterthoughts.

### Asset-backed instruments

Support both built-in synthesis and external assets people already have:
WAV one-shots and packs, SoundFont banks, and VST3 plug-ins (with the
constraints that live plugins imply).

## Non-goals (current phase)

| Non-goal | Rationale |
| --- | --- |
| Real-time DAW hosting of Symphra itself | Product is compile → render, not a live plugin host for the language runtime |
| Full bus routing / multi-send mixers | Tracks sum to a master; one effect per track keeps ordering simple |
| General free-floating automation graphs | Only track-scoped filter-cutoff LFO is implemented |
| Live MIDI performance I/O as the main workflow | MIDI appears as pitch/velocity concepts inside the offline pipeline |
| Stable 1.0 language standard | Syntax still evolves with the Draft 0.1 surface |
| GUI piano-roll editor | Text + LSP is the authoring surface |

## Design preferences

When two designs conflict, Symphra tends to choose:

1. **Track-local ownership** over song-global dotted paths (effects and
   `automate` live on the track that owns the sound)
2. **One effect per track** over arbitrary insert chains
3. **Fixed pipeline stage order** over free-form operator graphs
4. **Integer MIDI-style velocity (0–127)** over ad-hoc float gains on events
5. **MIT-compatible dependencies** for the audio stack where practical

See also [Capabilities](./03-capabilities.md) for a concrete can / cannot list.
