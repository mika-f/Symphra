# Tracks and pipelines

A **track** binds an instrument (or layers) to one or more play statements and
optional processing.

```symphra
track bass role bass {
  instrument bass_saw
  volume -3 db

  play bass_roots |> trigger_with bass_pulse |> gate 85% |> repeat 2

  effect filter {
    cutoff 500hz
    resonance 0.2
  }
}
```

## Track header fields

| Field | Meaning |
| --- | --- |
| Name | Identifier used by `play track name` inside sections |
| `role` | Free-form label (`bass`, `drums`, `harmony`, …) for organization |
| `instrument` | Default instrument for a single-play track |
| `volume` | Track gain in dB |

## Single instrument vs layers

**Single instrument** — one `instrument` and `play` pipeline:

```symphra
track arp role harmony {
  instrument music_box
  volume -8 db
  play arpeggio
}
```

**Layers** — several independently scheduled voices mixed into one track,
sharing `role` / `volume` / effect, each with its own instrument and pipeline:

```symphra
track lead role lead {
  volume -5 db

  layer {
    use lead_tone {
      play lead_line |> gate 85% |> repeat 2
    }
    use lead_tone {
      play lead_line |> gate 85% |> transpose 12 st |> gain 0.2 |> repeat 2
    }
  }

  effect delay {
    mix 0.25
    time 1/4
    feedback 0.2
  }
}
```

Patterns still must exist at song scope; layers only `play` them.

## Play pipelines

Stages chain with `|>`. Each supported stage appears **at most once** and is
normalized into a fixed scheduling order (not a free operator graph).

```symphra
play drop_chords
  |> trigger_with chord_stabs
  |> gate 90%
  |> repeat 2
```

Common stages:

| Stage | Role |
| --- | --- |
| `trigger_with <rhythm>` | Keep events only where the rhythm has `hit` |
| `gate P%` | Shorten event length to a percentage of the slot |
| `transpose N st` | Shift pitch by N semitones |
| `gain X` | Scale event amplitude |
| `repeat N` | Play the material N times back-to-back |
| `reverse` | Reverse event order in the pattern window |
| `pan left\|right\|center\|N` | Static pan |
| `alternate { pan … }` | Alternating pan values |
| `chance P% { transpose N }` | Seeded subset of pitched events |
| `chance P% { retrigger N }` | Sample/drum only; N total attacks (`N ≥ 2`) |
| `chance P% { speed F }` | Sample/drum only; override playback rate |
| `speed F` | Base sampler/drum playback rate |
| `alternate { speed … }` | Alternating speeds |
| `choose_sample A..B` | Inclusive index range for sampler events |
| `at B:T play …` | Prefix form: place after all other stages |

Full stage reference: [Pipeline stages](/reference/pipeline-stages/).

### `at` placement

```symphra
at 2:1 play fill |> repeat 1
```

Bar and beat are **1-based**. The offset is applied **last**, so `repeat` and
`reverse` still operate in the pattern’s local time window.

## Effects on tracks

At most **one** `effect` block per track. Optional `automate cutoff` when the
effect is a filter. Details:
[Effects and automation](./06-effects-and-automation.md).

## Tracks without sections

For bare-pattern arrangements you may skip `track` entirely and assign
instruments in the arrangement:

```symphra
arrangement { melody with lead pads with warm }
```

Sections require declared tracks and `play track …` inside `parallel` blocks.
