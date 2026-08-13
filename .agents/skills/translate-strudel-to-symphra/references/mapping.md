# Strudel to Symphra mapping

## Contents

- Time and Mini-Notation
- Pattern and form mapping
- Pitch and sound-source mapping
- Controls, effects, and modulation
- Unsupported behavior and translation checklist

## Time and Mini-Notation

Strudel has cycles, not bars. `setcps(x)` sets cycles per second; `setcpm(x)` sets cycles per minute. Infer beats per cycle from expressions such as `setcps(150 / 60 / 4)` and musical context. That example commonly means 150 BPM with four beats per cycle, allowing one cycle to become one 4/4 bar. Do not generalize this to every source.

Normalize the source pattern before emitting Symphra:

| Strudel | Meaning | Symphra strategy |
| --- | --- | --- |
| `a b c d` | Items share one cycle | Four equal sequence/step cells |
| `~` or `-` | Rest | `rest` |
| `[a b]` | Subdivide one outer cell | `[ ... ]` in `steps`, or explicit shorter durations |
| `<a b c>` / `cat` | One item per cycle | Sequential bar-length items or sections |
| `a,b` / `stack` | Simultaneous patterns | Separate tracks in `parallel`, or chords/layers where appropriate |
| `x*N` / `.fast(N)` | Fit N repetitions into the original span | Expand cells or shorten durations; Symphra `repeat N` is not automatically equivalent |
| `x/N` / `.slow(N)` | Stretch across N cycles | Lengthen explicit durations or form |
| `x!N` | Replicate without speeding the enclosing sequence | Repeat written items during normalization |
| `x@N` | Give an item N units of temporal weight | Convert weights to explicit durations |
| `x?P`, `degradeBy(P)` | Randomly remove events | No direct event-drop stage; choose deterministic rests manually or omit with disclosure |
| `a | b` | Random alternative | `choose` only where the target pattern form supports it; otherwise choose explicitly |
| `x(k,n,o)` | Euclidean rhythm | Expand to `hit`/`rest` or step cells |

Strudel's nested Mini-Notation divides the enclosing time span. Symphra `steps` subdivisions have similar local division, but verify total duration rather than translating brackets mechanically.

## Pattern and form mapping

| Strudel | Symphra |
| --- | --- |
| `note(...)` | `pattern ... = sequence` or pitched `steps` |
| `s(...)` / `sound(...)` | An instrument plus sample/drum patterns and tracks |
| `.struct(pattern)` | A `rhythm` and `play ... |> trigger_with rhythm` |
| `stack(a, b, ...)` | Independent tracks listed in a section's `parallel` block |
| `.layer(f, g)` | A Symphra track `layer` when all branches share the track-level effect |
| `.superimpose(x => x.add(12))` | A layer playing the source plus a transposed, gain-adjusted copy |
| `arrange([N, a], [M, b])` | Sections of converted bar lengths followed by `arrangement { play ... }` |
| `.rev()` | `|> reverse` when the local pattern window matches |
| `.add(12)` on pitch | `|> transpose 12 st` |
| `.clip(0.85)` | Often `|> gate 85%`; verify source usage |

`arrange` repeats a shorter Strudel pattern to fill its assigned cycles. Use a numeric repeat or `repeat fit` in a Symphra section only after proving that the material divides the section evenly.

Masks, conditional transforms, polymeters, and time-varying pattern selection may require writing explicit section variants. Prefer a finite, readable arrangement over simulating a live pattern algebra that Symphra does not have.

## Pitch and sound-source mapping

### Pitch and harmony

- Convert note names to explicit Symphra pitches with octaves.
- Convert comma-polyphony to a chord only when the events truly share onset and duration.
- Resolve `.voicing()` deliberately. Strudel chooses voicings using dictionaries, anchors, and modes; Symphra chord symbols build upward from the written root. Write explicit chord tones when voicing identity matters.
- Resolve `.rootNotes(octave)` to explicit root pitches.
- Convert supported chord qualities to Symphra spellings, for example Strudel `G^7` to Symphra `G3:maj7`. Spell unsupported or extended qualities as explicit notes.

### Sound sources

| Strudel source | Symphra strategy |
| --- | --- |
| `sine` | Built-in `sine` |
| `triangle` | Built-in `triangle` |
| `supersaw` | `synth supersaw` |
| `sawtooth` | Approximate with one-voice supersaw or use an available asset/VST3 |
| `square` | Approximate with triangle or use an available asset/VST3 |
| noise such as `white` | Use a relative sampled asset when available |
| GM-style `gm_*` sound | Use an available SoundFont and verified preset name |
| `.bank("Name")` drums | Use a relative drum-machine bank containing matching `<name>.wav` files |
| arbitrary sample catalog sound | Use a verified relative sample/pack asset; Strudel's remote catalog is not implicit in Symphra |

Never manufacture an asset path merely to make the source look complete.

## Controls, effects, and modulation

| Strudel behavior | Symphra strategy | Caveat |
| --- | --- | --- |
| static `.gain(x)` | Track `volume`, play `gain`, or event velocity | Numeric response is not guaranteed to match |
| patterned gain | Velocity pattern/ramp when structurally compatible | Continuous gain LFO is unsupported |
| static `.pan(x)` | Convert Strudel 0..1 to Symphra `pan -100%..100%` with `(x - 0.5) * 200` | Clamp and round deliberately |
| two-position pan | `pan alternate(L%, R%)` when it truly alternates | Continuous pan LFO is unsupported |
| `.attack/.decay/.sustain/.release` | Instrument envelope | Strudel seconds can seed Symphra millisecond values; controls must be static per instrument |
| `.adsr("a:d:s:r")` | Instrument envelope | Convert seconds to time literals and verify sustain range |
| `.lpf(x)` + `.lpq(q)` | Track filter cutoff + resonance | DSP and resonance scales differ |
| sine/triangle cutoff LFO | `automate cutoff` | Convert source cycles to `cycles/bar` using the chosen time model |
| saw/random cutoff modulation | Approximate with triangle/static cutoff or write section variants | Symphra cutoff LFO supports sine/triangle only |
| `.room/.roomsize` | Track reverb `mix`/`size` | Parameter ranges and algorithm differ |
| `.delay/.delaytime` | Track delay `mix`/`time`/`feedback` | Translate timing musically; Strudel send semantics differ |
| reverb plus delay/filter chain | Choose the dominant one, split/duplicate with care, or disclose omission | Symphra permits one effect per track |
| orbit, ducking, compressor, distortion, chorus, phaser, bitcrush | No direct mapping | Simplify or use an external preprocessed asset/VST3 when appropriate |

Strudel parameter signals are often sampled at events, while some effects vary continuously. Inspect the actual control before labeling it an LFO. Symphra only provides continuous track-local automation for filter cutoff.

## Unsupported behavior

Common gaps that must be disclosed:

- live evaluation, interactive state, and open-ended playback
- arbitrary JavaScript callbacks or pattern combinators
- random event removal and many random/time transforms
- polymetric or irrational timing that cannot be represented cleanly with supported durations
- continuous gain/pan modulation and most parameter automation
- multi-effect chains, sends, orbits, sidechain/ducking, and many Strudel DSP effects
- exact synthesis/sample-catalog equivalence without matching local assets
- identical chord voicing without resolving the source voicing

## Translation checklist

Before handoff, verify:

- tempo, meter, and cycle-to-bar interpretation are explicit
- every normalized pattern has the intended total duration
- simultaneous voices became chords, layers, or parallel tracks intentionally
- source instruments map only to real built-ins or existing relative assets
- every `.struct`, `stack`, `superimpose`, and `arrange` has a target representation
- continuous/random behavior is mapped or disclosed
- no track contains more than one effect
- the formatter succeeds
- the full render succeeds when assets are available

## Official Strudel references

Recheck these pages for unfamiliar constructs:

- <https://strudel.cc/understand/cycles/>
- <https://strudel.cc/learn/mini-notation/>
- <https://strudel.cc/learn/factories/>
- <https://strudel.cc/learn/time-modifiers/>
- <https://strudel.cc/learn/accumulation/>
- <https://strudel.cc/learn/tonal/>
- <https://strudel.cc/learn/effects/>
- <https://strudel.cc/learn/random-modifiers/>
