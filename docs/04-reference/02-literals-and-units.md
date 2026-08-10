# Literals and units

## Numbers

| Form | Example | Notes |
| --- | --- | --- |
| Integer | `4`, `127` | Counts, velocities, seeds, bar numbers |
| Decimal | `0.25`, `1.5` | Mix, feedback, gain, speed, resonance, …

Integers used as velocities must be in `0`…`127`.

## Strings

Double-quoted only:

```symphra
"Aoharu Signal"
"assets/TimGM6mb.sf2"
"Warm Pad"
```

Paths to assets are **relative to the `.sym` file**. Absolute paths are rejected.

## Pitches

```symphra
C4    E4    G4
C#4   Db4   F#3
Bb2   C-1   C#-1
```

- Letter + optional accidental (`#`, `b`) + octave integer (may be negative)
- Used as note/chord tones, `root` on sampled instruments, and song `key` tonic

## Rate and tempo

| Literal | Meaning |
| --- | --- |
| `48khz` / `48000hz` | Sample rate |
| `150bpm` | Tempo |

## Duration

| Form | Meaning |
| --- | --- |
| `1/4`, `3/16` | Fraction of a whole note |
| `1 bar`, `4 bar` | Meter-aware length (`N * numerator/denominator` whole notes) |

Used by notes, rests, rhythm `resolution`, steps grids, delay `time`, and
related musical times.

## Time (wall-clock style)

| Form | Typical use |
| --- | --- |
| `10ms`, `100ms` | Envelope attack / decay / release |

## Level

| Form | Typical use |
| --- | --- |
| `-6 db`, `-0.3 db` | Track `volume`, limiter `ceiling` |
| Bare `0.3` | Sustain level, mix, feedback, resonance, gain (context-specific) |

## Frequency

| Form | Typical use |
| --- | --- |
| `1400hz`, `5.2khz` | Filter cutoff, automation range bounds |

## Ranges and pairs

| Syntax | Example | Use |
| --- | --- | --- |
| `..` | `0..3`, `400hz..9000hz` | Inclusive `choose_sample` / LFO range |
| `:` | `2:3` | `at bar:beat` (1-based) |
| `/` | `4/4`, `1/8` | Meter; duration fractions; `cycles/bar` |
| `%` | `90%`, `40%` | Gate and chance percentages |

## Transpose unit

```symphra
transpose 12st
```

Semitone shifts use the `st` unit token in transpose stages.

## Mode words

Song key mode: `major` | `minor`.

Output mode: `mono` | `stereo`.

LFO waveform identifiers: `sine` | `triangle` (not a separate keyword set for
waveforms in every position — oscillators also use `sine` / `triangle` as body
kinds).
