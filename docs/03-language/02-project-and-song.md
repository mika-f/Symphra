# Project and song

## `project`

Top-level render configuration. Typically one per file.

```symphra
project {
  seed 20260811
  sample_rate 48khz
  output stereo
}
```

| Field | Meaning |
| --- | --- |
| `seed` | Integer seed for deterministic `chance` / `choose` / `choose_sample` |
| `sample_rate` | Rate literal, e.g. `48khz`, `44100hz` |
| `output` | `mono` or `stereo` |

The seed does not “randomize the composition” in a musical-AI sense; it fixes
the outcomes of explicit probabilistic language features.

## `song`

```symphra
song "Aoharu Signal" {
  tempo 150bpm
  meter 4/4
  key D major

  // … declarations …
}
```

| Field | Meaning |
| --- | --- |
| Name | String title of the piece |
| `tempo` | Beats per minute (`150bpm`) |
| `meter` | Time signature `numerator/denominator` (`4/4`, `6/8`, …) |
| `key` | Tonic pitch class + `major` or `minor` (`D major`, `A minor`) |

### How context is used

- **Tempo and meter** convert musical durations (`1/4`, `1 bar`, delay `time
  1/4`, LFO `cycles/bar`, `at 2:3`) into samples
- **Key** anchors degree-based material; note pitches like `C4` are absolute
- **Name** is metadata for humans/tools; the WAV filename is chosen by the CLI

## Declaration order inside a song

Conceptually:

1. Musical context (`tempo`, `meter`, `key`)
2. Instruments, rhythms, patterns (materials)
3. Tracks (bind materials to sound)
4. Sections (optional structured form)
5. Arrangement (required for audible output)
6. Master (optional)

Forward references are resolved after the whole song is parsed; still, keeping
materials above tracks keeps files readable.

## Units you will see early

| Literal | Examples |
| --- | --- |
| Rate | `48khz`, `150bpm` |
| Duration fraction | `1/4`, `1/16`, `3/8` |
| Bar duration | `1 bar`, `4 bar` |
| Time / envelope | `10ms`, `100ms` |
| Level | `-6 db`, `0.3` (context-dependent) |
| Frequency | `1400hz`, `5.2khz` |
| Pitch | `C4`, `F#3`, `Bb2` |

Full table: [Literals and units](/reference/literals-and-units/).
