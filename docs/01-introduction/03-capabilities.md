# Capabilities

A practical map of what Symphra can do **today** (workspace `0.1.0` / Draft 0.1
implementation) and what it deliberately or currently cannot.

## You can

### Structure a full song

- Set `project` seed, sample rate (`48khz`, …), and mono/stereo output
- Set song `tempo`, `meter`, and major/minor `key`
- Declare named instruments, rhythms, patterns, tracks, and sections
- Arrange sections back-to-back, or use a bare pattern arrangement for simpler
  pieces
- Apply a master `limiter { ceiling ... }`

### Make sound

| Kind | How |
| --- | --- |
| Oscillators | `instrument x = sine` / `triangle`, optional `envelope { attack … }` |
| Supersaw | `instrument x = synth supersaw { voices detune spread [envelope] }` |
| One-shot sample | `sampled { source "…" root C4 }` |
| Sample pack | `sampler { pack "…" }` with `sample N` steps |
| Drum machine | `drum_machine { bank "…" }` with `drum "name"` steps |
| SoundFont | `soundfont { source "file.sf2" preset "…" }` |
| VST3 | `vst3 { source "plugin.vst3" [preset "…"] }` |

### Write musical material

- `sequence` notes, chords, rests with `for N/M` or `for N bar` durations
- Velocity `0`–`127` on notes, chords, samples, and drums
- `steps` grids with degrees, samples, drums, and weighted `choose`
- Reusable `rhythm` hit/rest grids at a given `resolution`
- Pitch literals (`C4`, `F#3`, `Db5`, …)

### Shape playback on a track

Pipeline stages (each at most once; applied in a fixed order):

- `trigger_with` — gate pattern events with a rhythm
- `gate` — shorten note lengths by percentage
- `transpose`, `gain`, `repeat`, `reverse`
- `pan` / `alternate { pan … }`
- `chance { transpose | retrigger | speed }` (instrument-gated)
- `speed` / `alternate { speed … }` for samplers and drums
- `choose_sample low..high` for sampler index selection
- `at bar:beat` — final absolute placement

Also: multi-voice `layer { use instrument { play … } … }` on a track.

### Process audio

- One of `effect delay`, `effect filter`, or `effect reverb` per track
- `automate cutoff { lfo sine|triangle { range A..B rate N cycles/bar } }` when
  the track has a filter
- Master peak-detect-and-scale limiter

### Tooling

- CLI: `symphra input.sym [output.wav]`
- Background WAV loop player (`symphra-player`), used by VS Code previews
- Formatter (`symphra-formatter`)
- Language server (`symphra-lsp`): diagnostics, symbols, completion, hover,
  go-to-definition, references, rename, semantic tokens, inlay hints
- VS Code and IntelliJ extensions

## You cannot (yet or by design)

| Limitation | Notes |
| --- | --- |
| Multiple effects on one track | At most one of delay / filter / reverb; no insert chain |
| Bus / send / sidechain routing | Tracks sum into master only |
| Automate parameters other than filter cutoff | Delay mix, reverb size, resonance, envelopes, etc. are static |
| Free-floating / dotted-path automation | Automation is track-scoped: `automate cutoff { … }` |
| Bracketed chord syntax | Write `chord C4 E4 G4`, not `chord [C4 E4 G4]` |
| `synth sine` wrapper | Use bare `sine` / `triangle`; only supersaw uses `synth` |
| Decimal drum velocity | Use integer `velocity 0`–`127`, not `0.55` |
| Patterns declared inside tracks | Patterns are song-scoped; layers only `play` existing patterns |
| Mixed arrangement forms | Bare-pattern arrangement **or** section `play` arrangement — not both in one song |
| Real-time performance host | No primary live MIDI clock / audio I/O product path |
| Square oscillator | Lead tones often approximate with `triangle` |
| Continuous pan/gain LFO | Alternating pan exists; continuous modulation does not |
| Multi-sample `choose` + `trigger_with` | Weighted choice with multi-item sequences cannot feed a fixed rhythm cell count |
| Bit-identical VST3 golden tests | Native plugins are not fixture-friendly; audio may depend on host/OS |

## Intentional syntax differences from early Draft sketches

If you saw an older Pastebin-style draft, translate as follows:

| Sketch | Current language |
| --- | --- |
| `instrument x = synth sine { … }` | `instrument x = sine` or `sine { envelope { … } }` |
| `chord [C4 E4 G4] for 1bar` | `chord C4 E4 G4 for 1bar` |
| Inline patterns inside `layer` | Declare `pattern` at song scope; `layer` only `play`s |
| `automate filter.lowpass.cutoff` | `automate cutoff` on the same track as `effect filter` |
| `soundfont { preset "gm_…" }` only | `soundfont { source "…sf2" preset "…" }` |

Contributor-oriented gap analysis still lives under
[Implementation status](/internals/implementation-status/).
