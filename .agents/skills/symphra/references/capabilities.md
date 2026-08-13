# Symphra Draft 0.1 capabilities and limits

## Supported

- Deterministic offline WAV composition and rendering
- Song tempo, meter, major/minor key, sample rate, mono/stereo, and RNG seed
- Sine and triangle oscillators with optional ADSR; supersaw with unison controls
- Single-file samples, indexed sample packs, named drum banks, SoundFont, and VST3 instruments
- Notes, chords, rests, degrees, fixed grids, subdivisions, repetition, velocity ramps, and seeded weighted choices
- Derived patterns and chord-pattern arpeggiation
- Track pipelines for rhythm triggering, gate, transpose, gain, repeat, reverse, pan, chance, sample speed, sample choice, and placement
- Layered instruments within a track
- One delay, filter, or reverb effect per track; reusable effect presets
- Track-local LFO automation of filter cutoff
- Section-based song form, per-section track overrides, and a master peak limiter
- Formatter, LSP, CLI diagnostics, and offline rendering

## Unsupported or constrained

- No real-time live-performance host or primary live MIDI workflow
- No square oscillator; use another supported source rather than inventing syntax
- No arbitrary DSP/operator graph, bus/send/sidechain routing, or multi-effect track chain
- No continuous pan or gain LFO
- No automation except track-local filter cutoff
- No patterns declared inside tracks; patterns are song-scoped
- No bracketed chord-tone syntax
- No decimal velocity; use an integer from 0 through 127
- No `synth sine` or `synth triangle`; only supersaw uses `synth`
- No mixed bare-pattern and section arrangement forms
- No absolute sample, SoundFont, or VST3 paths
- No `choose` inside subdivisions or repeated `choose`; multi-item `choose` sequences cannot feed `trigger_with`
- No `repeat fit` outside a section or when the pattern does not evenly divide the section
- No bit-identical expectation for arbitrary VST3 renders across hosts

## Common invalid assumptions

| Invalid assumption | Use instead |
| --- | --- |
| Every sound source is `synth <kind>` | Bare `sine`/`triangle`; `synth supersaw`; dedicated asset instrument forms |
| Chords use `[C4 E4 G4]` | `chord C4 E4 G4` or `chord C4:maj` |
| Effects are pipeline stages | Put one `effect ...` in the track body |
| Any parameter can be automated | Automate only `cutoff` on a track with a filter |
| Pipeline order is a free signal chain | Use supported stages; the compiler normalizes their order |
| A section silently loops short tracks | Use an appropriate numeric repeat or `repeat fit` |
| Asset paths resolve from the shell directory | Resolve them relative to the `.sym` file |

## Validation boundaries

- Formatting checks syntax and canonical layout but not every semantic constraint.
- Full CLI rendering checks parsing, compilation, scheduling, assets, rendering, and WAV export.
- External asset and VST3 failures may be environmental rather than language errors.
- Draft 0.1 still evolves. In a repository checkout, verify disputed behavior against current tests and docs rather than this snapshot.
