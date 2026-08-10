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
