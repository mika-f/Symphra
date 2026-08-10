# Pipeline stages

Play pipelines use `|>` between stages. Each stage kind may appear **at most
once** on a play chain. The compiler normalizes stages into a **fixed
scheduling order**; writing them in a different source order does not invent a
new graph topology.

## Stage catalog

| Stage | Syntax (sketch) | Applies to | Role |
| --- | --- | --- | --- |
| Trigger | `trigger_with <rhythm>` | Notes, chords, rests, samples, drums, single-select chooses, degrees | Keep events only on rhythm `hit`s |
| Gate | `gate 85%` | Timed events | Shorten length to % of slot |
| Transpose | `transpose 12 st` | Pitched events | Shift by semitones |
| Gain | `gain 0.2` | Events | Scale amplitude |
| Repeat | `repeat 2` | Whole pattern result | Concatenate N copies |
| Reverse | `reverse` | Whole pattern result | Reverse within local window |
| Pan | `pan center` (and related) | Track events | Static stereo position |
| Alternate pan | `alternate { pan … }` | Track events | Alternate pan values |
| Chance transpose | `chance 40% { transpose 12 }` | Pitched | Seeded subset transpose |
| Chance retrigger | `chance 40% { retrigger 2 }` | Sample/drum | N total attacks, split duration |
| Chance speed | `chance 15% { speed 1.5 }` | Sample/drum | Override playback rate |
| Speed | `speed 1.0` | Sample/drum | Base playback rate |
| Alternate speed | `alternate { speed … }` | Sample/drum | Alternate rates |
| Choose sample | `choose_sample 0..3` | Sampler | Inclusive index overwrite |
| At | `at 2:1 play …` | Whole play | Final absolute bar:beat shift |

## Ordering notes

Conceptual scheduling phases (simplified):

1. Expand pattern events
2. `trigger_with` / gate-style selection
3. Pitch/gain transforms (`transpose`, `gain`, …)
4. `repeat`, then chance (`transpose` / `retrigger`), then `reverse`
5. Sampler `speed` / `alternate { speed }`, then `chance { speed }` (override wins)
6. Pan stages
7. `choose_sample`
8. `at` offset applied last to every event and track end

Because `repeat` and `reverse` assume a local `[0, duration]` window, `at` is
intentionally a **final translation**, not a shifted start for earlier stages.

## Instrument gates

Some stages error at compile time on the wrong instrument kind:

| Stage | Allowed instruments |
| --- | --- |
| `speed`, `alternate { speed }`, `chance { speed }`, `chance { retrigger }` | `sampler`, `drum_machine` |
| `choose_sample` | `sampler` (index selection) |
| `chance { transpose }`, `transpose` | Pitched patterns / instruments |

## Inline drum play

```symphra
play drum "bd" with kick_pattern |> repeat 4
```

The `drum … with …` form is a **play source**, not a pipeline stage. It
synthesizes a pattern from the rhythm; combining it with `trigger_with` is
rejected.

## Effects are not pipeline stages

`effect delay|filter|reverb` and `automate cutoff` are **track body** blocks,
applied in the audio domain after event rendering — not `|>` stages.

## Examples

```symphra
play drop_chords
  |> trigger_with chord_stabs
  |> gate 90%
  |> repeat 2

play lead_line
  |> gate 85%
  |> transpose 12 st
  |> gain 0.2
  |> repeat 2

play kit
  |> chance 40% { retrigger 2 }
  |> chance 15% { speed 1.5 }
  |> reverse

at 3:1 play fill |> repeat 1
```
