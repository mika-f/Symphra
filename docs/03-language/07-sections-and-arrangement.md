# Sections and arrangement

Form turns materials into a piece: what plays, for how long, and in what order.

## Bare pattern arrangement

Best for short sketches without tracks:

```symphra
instrument lead = triangle
pattern melody = sequence {
  note C4 for 1/4
  note E4 for 1/4
  note G4 for 1/2
}

arrangement {
  melody with lead
  melody with lead
}
```

Each entry is a pattern name, optionally `with <instrument>`. Entries play
**sequentially**.

## Section-based form

For multi-track songs, declare tracks first, then sections, then an arrangement
that references sections.

```symphra
section intro bars 4 {
  parallel exact {
    play track arp
    play track pad
    play track light_hh
  }
}

section drop bars 8 {
  parallel exact {
    play track bass
    play track lead
    play track drop_hh
  }
}

arrangement {
  play intro
  play drop
  play intro
}
```

### Section fields

| Piece | Meaning |
| --- | --- |
| Name | Identifier for arrangement `play <name>` |
| `bars N` | Fixed length of this section in bars |
| `parallel` | Simultaneous track activation |
| `exact` | Optional: every listed track must last **exactly** `bars` |

Without `exact`, tracks may be shorter than the section window (remaining time
is silence for that track). With `exact`, a length mismatch is a schedule-time
error.

### Per-section overrides

A `play track` reference may override what that track's declaration says, for
that section only:

```symphra
section outro bars 4 {
  parallel exact {
    play track pad
    play track vox       { volume -14 db }
    play track light_rim { volume -7 db  effect plate }
  }
}
```

`volume`, `effect`, and `automate cutoff` may be overridden. An override
replaces the declaration's value rather than adding to it, and the play
pipeline's own `gain` stage still applies on top of the new volume.

This is what a second track declaration used to be for. The one visible
difference from writing that second declaration by hand is that `chance`
rolls are seeded from track identity, so an overridden reference rolls like
the separate track it stands in for — the same as today, where a track
played in two sections already rolls differently in each.

### Filling a section with `repeat fit`

`repeat N` on a play usually just means "as many times as this section is
long". `repeat fit` says that directly:

```symphra
track light_hh role drums {
  instrument tr909
  volume -12 db
  play light_hh |> repeat fit    // 4 in a 4-bar section, 8 in an 8-bar one
}
```

`fit` resolves per section reference to `section bars ÷ pattern bars`. The
pattern must divide the section evenly, its length must not depend on a
`choose` roll, and the track must be played by a section — `fit` has nothing
to fill otherwise.

### Reuse

The same section may be played multiple times in the arrangement. Each
occurrence is an independent copy in time (namespacing does not collide).

### Compatibility rule

Do **not** mix bare pattern entries and section `play` entries in one
`arrangement`. Section form also expects track declarations; bare pattern form
is for track-less songs.

## End-to-end sketch

```symphra
project {
  seed 1
  sample_rate 48khz
  output stereo
}

song "Sketch" {
  tempo 120bpm
  meter 4/4
  key C major

  instrument tone = triangle
  instrument kit = drum_machine { bank "drums" }

  pattern line = sequence {
    note C4 for 1/4
    note E4 for 1/4
    note G4 for 1/4
    note C5 for 1/4
  }

  pattern kick = steps 1/4 {
    drum "bd" velocity 110
    rest
    drum "bd" velocity 100
    rest
  }

  track lead role lead {
    instrument tone
    volume -6 db
    play line |> repeat 4
  }

  track drums role drums {
    instrument kit
    volume -4 db
    play kick |> repeat 4
  }

  section body bars 4 {
    parallel exact {
      play track lead
      play track drums
    }
  }

  arrangement {
    play body
    play body
  }

  master {
    limiter { ceiling -1 db }
  }
}
```

## Master

Optional, but recommended for louder multi-track material:

```symphra
master {
  limiter {
    ceiling -0.3 db
  }
}
```

See [Effects and automation](./06-effects-and-automation.md#master-limiter).
