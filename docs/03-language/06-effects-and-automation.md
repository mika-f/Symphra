# Effects and automation

Effects are **track-scoped**: they process that track’s rendered audio before
it is summed into the master bus. A track may declare **at most one** effect.

## Delay

```symphra
effect delay {
  mix 0.25
  time 1/4
  feedback 0.2
}
```

| Field | Meaning |
| --- | --- |
| `mix` | Dry/wet blend (0–1) |
| `time` | Echo time — musical duration (e.g. `1/4`) resolved with tempo/meter |
| `feedback` | Feedback amount for repeats |

The renderer extends the buffer for the delay tail.

## Filter

```symphra
effect filter {
  cutoff 1400hz
  resonance 0.55
}
```

Resonant low-pass biquad.

| Field | Meaning |
| --- | --- |
| `cutoff` | Static cutoff frequency (`hz` / `khz`); required even when automated |
| `resonance` | Resonance amount |

### Automating cutoff

```symphra
effect filter {
  cutoff 400hz
  resonance 0.15
}

automate cutoff {
  lfo triangle {
    range 400hz..9000hz
    rate 0.25 cycles/bar
  }
}
```

Rules:

- Must sit on the **same track** as `effect filter`
- Waveforms: `sine`, `triangle`
- `range A..B` — inclusive frequency bounds
- `rate N cycles/bar` — tempo-synced LFO rate
- The LFO **overrides** the static `cutoff` at render time

No other parameters are automatable yet (not delay mix, reverb size, resonance,
envelope stages, …).

## Reverb

```symphra
effect reverb {
  mix 0.5
  size 0.7
}
```

Reduced Schroeder reverberator (comb + allpass network).

| Field | Meaning |
| --- | --- |
| `mix` | Dry/wet |
| `size` | Spatial size / decay character |

## Mutual exclusion

These are invalid on one track:

```symphra
// pick one only
effect delay { … }
effect filter { … }
```

Combine colors by **splitting tracks** (e.g. dry close mics vs wet bus as a
second track), not by stacking inserts.

## Master limiter

Song-level, after all tracks sum:

```symphra
master {
  limiter {
    ceiling -0.3 db
  }
}
```

Peak-detect-and-scale gain reduction (not a hard clip masquerading as a
limiter), applied before the renderer’s final safety clamp and PCM conversion.

There is **no** master effect chain beyond this limiter, and no track-send into
master inserts.
