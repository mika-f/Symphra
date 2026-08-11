# Patterns and rhythms

Patterns describe *what* happens. Rhythms describe *when* hits fall. Tracks
combine them with instruments.

## Rhythms

```symphra
rhythm chord_stabs resolution 1/8 {
  hit rest rest hit rest rest hit rest
  hit rest rest hit rest rest hit hit
}
```

- `resolution` — duration of each cell
- Body — `hit` or `rest` tokens in order
- Referenced by `trigger_with`, or by the sugar `play drum "bd" with kick`

## Sequence patterns

Linear lists of notes, chords, and rests:

```symphra
pattern pad_chords = sequence {
  chord G3 B3 D4 F#4 for 1bar
  chord A3 C#4 E4 G4 for 1bar
  rest for 1/4
  note B4 for 1/8 velocity 96
}
```

### Notes and chords

```symphra
note C4 for 1/4
note D5 for 1/16 velocity 110
chord C4 E4 G4 for 1/2 velocity 90
```

- Pitches are bare tokens (`C4`, `F#3`); **no** `[brackets]` around chord tones
- Duration: fraction `N/M` or meter-aware `N bar`
- Optional `velocity` 0–127 (default when omitted)

### Rests

```symphra
rest for 1/8
rest for 2 bar
```

## Steps patterns

Fixed grid, one item per cell at the given step duration:

```symphra
pattern light_hh = steps 1/4 {
  drum "hh" velocity 38
  drum "hh" velocity 38
  drum "hh" velocity 38
  drum "hh" velocity 38
}
```

Step items include:

| Item | Use |
| --- | --- |
| `rest` | Silence for one cell |
| `note …` / `chord …` | Pitched events (with optional velocity) |
| `sample N [velocity V]` | Index into a `sampler` pack |
| `drum "name" [velocity V]` | Named voice on a `drum_machine` |
| `degree N octave O` | Degree-relative pitch material |
| `choose { … weight W … }` | Deterministic weighted alternative |

### Weighted choice

```symphra
pattern kit = steps 1/8 {
  choose {
    drum "bd" weight 2
    drum "sn" weight 1
  }
}
```

Selection is seeded from `project.seed` and the event identity — same source,
same result.

`choose` alternatives may also use `sample N`, and multi-item
`sequence { … }` alternatives. Multi-sample sequence alternatives cannot be
combined with `trigger_with` (cell count would depend on the roll).

## Repetition

Any rhythm cell, sequence item, or step item can be repeated with `* N`
instead of being written out N times:

```symphra
rhythm bass_pulse resolution 1/16 {
  hit rest * 4 hit hit rest * 3 hit rest * 5
}

pattern light_hh = steps 1/4 { drum "hh" velocity 38 * 4 }
pattern light_rim = steps 1/8 { rest * 7  drum "rim" velocity 64 }
```

Parentheses repeat several items as a unit. The elements are separated by
commas, and the `* N` is required — a group without one would just be its
elements:

```symphra
pattern drop_hh = steps 1/8 {
  (drum "hh" velocity 38, drum "hh" velocity 64) * 4
}
```

Groups nest, and the counts multiply: `(rest * 2, drum "hh") * 3` is nine
cells.

Repetition is pure sugar — it expands to the written-out items before
anything else runs, so `trigger_with`, `repeat`, and `reverse` see exactly
the pattern you would have typed by hand. Two rules keep it predictable:

- `choose { … }` cannot be repeated. Each copy would roll independently, so
  `choose { … } * 4` would read like one repeated decision but not be one.
- A body may not expand to more than 4096 items. Past that, the count is
  almost certainly a typo, and the compiler says so instead of building it.

## Velocity ramps

Anywhere `velocity N` is accepted, `velocity A..B` ramps linearly across the
copies of the repetition that encloses it:

```symphra
pattern crescendo = steps 1/16 {
  drum "cp" velocity 86..93 * 8   // 86 87 88 89 90 91 92 93
}
```

- The first copy is `A` and the last is `B`; values in between are rounded to
  the nearest integer, halves away from zero
- Ramps may descend (`velocity 110..70`)
- Both ends must be 0–127
- A ramp needs a repetition to ramp across; `velocity 70..110` on its own is
  an error
- Inside a group, the ramp follows the enclosing repetition, so
  `(drum "bd" velocity 90..110, drum "cp") * 4` ramps the kick over four
  iterations and leaves the clap alone

## Degree steps

```symphra
degree 0 octave 4
degree 4 octave 5
```

`degree N` is currently a **chromatic semitone offset** from the song tonic
(not a diatonic scale degree index). Confirm this when writing scale-aware
material.

## Inline drum sugar

```symphra
play drum "bd" with kick_pattern
```

Expands at compile time into a one-step-per-rhythm-cell pattern: `hit` → that
drum, otherwise `rest`. Requires a `drum_machine` instrument on the track. Do
not also attach `|> trigger_with` (the rhythm is already supplied via `with`).

## Patterns are song-scoped

Declare patterns at song level. Tracks and layers only **reference** them:

```symphra
// yes
pattern phrase = sequence { note C4 for 1/4 }
track lead role lead {
  instrument tone
  play phrase
}

// no — patterns are not declared inside track / layer bodies
```
