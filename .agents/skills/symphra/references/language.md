# Symphra Draft 0.1 language

## Contents

- Mental model and minimal song
- Project, song, and instruments
- Rhythms and patterns
- Tracks and play pipelines
- Effects and automation
- Arrangement forms and master processing

## Mental model

A `.sym` file describes an offline render:

```text
project -> song context -> instruments -> rhythms/patterns
        -> tracks -> sections -> arrangement -> master -> WAV
```

Comments start with `//`. Strings use double quotes. Asset paths are relative to the `.sym` file; absolute asset paths are rejected.

## Minimal complete song

```symphra
project {
  seed 1
  sample_rate 48khz
  output stereo
}

song "Hello" {
  tempo 120bpm
  meter 4/4
  key C major

  instrument lead = triangle

  pattern phrase = sequence {
    note C4 for 1/4 velocity 100
    note E4 for 1/4 velocity 90
    note G4 for 1/4 velocity 100
    rest for 1/4
  }

  arrangement {
    phrase with lead
  }
}
```

## Project and song

```symphra
project { seed 20260811 sample_rate 48khz output stereo }
song "Title" { tempo 150bpm meter 4/4 key D major }
```

- Output is `mono` or `stereo`.
- Key mode is `major` or `minor`.
- Common units: `48khz`, `150bpm`, `10ms`, `1/8`, `2 bar`, `-6 db`, `1400hz`, `90%`, and `12 st`.
- Pitches are bare tokens such as `C4`, `F#3`, `Bb2`, or `C-1`.

## Instruments

```symphra
instrument tone = sine
instrument lead = triangle {
  envelope { attack 10ms decay 80ms sustain 0.4 release 100ms }
}
instrument saw = synth supersaw {
  voices 5 detune 0.4 spread 0.7
  envelope { attack 10ms decay 150ms sustain 0.3 release 100ms }
}
instrument hit = sampled { source "assets/hit.wav" root C4 }
instrument pack = sampler { pack "assets/one_shots" }
instrument drums = drum_machine { bank "assets/drums" }
instrument keys = soundfont { source "assets/keys.sf2" preset "Piano" }
instrument plugin = vst3 { source "plugins/Synth.vst3" preset "Init" }
```

Only supersaw uses the `synth` keyword. VST3 `preset` is optional; SoundFont `preset` is required.

## Rhythms and patterns

Use a rhythm for reusable hit/rest timing:

```symphra
rhythm stabs resolution 1/8 { hit rest rest hit rest rest hit rest }
```

Use `sequence` for explicitly timed notes, chords, and rests:

```symphra
pattern chords = sequence {
  chord G3 B3 D4 F#4 for 1bar velocity 90
  chord A3:7 for 1bar
  rest for 1/4
}
```

Chord tones have no brackets. Supported symbol forms include `maj`, `m`, `min`, `dim`, `aug`, `sus2`, `sus4`, `6`, `m6`, `7`, `maj7`, `m7`, `mmaj7`, `m7b5`, `dim7`, `9`, `maj9`, `m9`, and `add9`.

Give a sequence a default item duration with `step`:

```symphra
pattern melody = sequence step 1/8 { note C4 note E4 rest note G4 for 1/16 }
```

Without `step`, every sequence item needs `for`.

Use `steps` for a fixed grid:

```symphra
pattern beat = steps 1/8 {
  drum "bd" velocity 110
  rest
  sample 2 velocity 90
  degree 5 octave 4
}
```

Repeat an item or comma-separated group with `* N`:

```symphra
pattern hats = steps 1/8 {
  (drum "hh" velocity 40, drum "hh" velocity 70) * 4
}
```

Use `[ ... ]` to subdivide one step evenly. Use `velocity A..B * N` for a ramp across repeated copies. Expanded bodies are limited to 4096 items.

Weighted choices are deterministic from `project.seed`:

```symphra
choose { drum "bd" weight 2 drum "sn" weight 1 }
```

Derive or arpeggiate material:

```symphra
pattern raised = chords |> transpose 12 st |> reverse
pattern arp = arpeggiate chords { style up_down step 1/8 octaves 1 }
```

Derivation accepts only `transpose`, numeric `repeat`, and `reverse`; its source must be declared above it.

## Tracks and play pipelines

```symphra
track lead_track role lead {
  instrument lead
  volume -6 db
  play melody |> gate 85% |> transpose 12 st |> gain 0.8 |> repeat fit
  effect delay { mix 0.2 time 1/8 feedback 0.15 }
}
```

Pipeline stages include:

- `trigger_with rhythm`
- `gate N%`
- `transpose N st`
- `gain N`
- `repeat N` or section-only `repeat fit`
- `reverse`
- `pan ...` or `alternate { pan ... }`
- `chance N% { transpose N | retrigger N | speed N }`
- `speed N` or `alternate { speed ... }`
- `choose_sample A..B`
- `at bar:beat` before `play`

Each stage kind may occur at most once. The compiler applies a fixed semantic order; source order does not define an arbitrary signal graph. `speed` and retrigger stages require sampler/drum playback; `choose_sample` requires a sampler.

Use `layer` when one track needs multiple instrument voices:

```symphra
track doubled role lead {
  volume -6 db
  layer {
    use lead { play melody }
    use lead { play melody |> transpose 12 st |> gain 0.2 }
  }
}
```

## Effects and automation

Declare an inline effect or a reusable song-level preset:

```symphra
effect room = reverb { mix 0.3 size 0.6 }

track filtered role harmony {
  instrument lead
  play chords
  effect filter { cutoff 1400hz resonance 0.4 }
  automate cutoff {
    lfo triangle { range 400hz..5000hz rate 2 cycles/bar }
  }
}
```

Available effects are delay, resonant low-pass filter, and reverb. A track owns at most one effect. Only filter cutoff supports automation, and the automation must share the filter's track.

## Arrangement forms

Use bare patterns for a small track-less sketch; entries play sequentially:

```symphra
arrangement { melody with lead chords with pad }
```

Use sections for multi-track form:

```symphra
section intro bars 4 {
  parallel exact {
    play track lead_track
    play track drums_track { volume -10 db }
  }
}

arrangement { play intro play intro }
```

`exact` requires each listed track to fill the declared section length. `repeat fit` repeats a pattern to fill the section and requires the pattern length to divide the section evenly. Section track references may override `volume`, `effect`, and cutoff automation.

Do not mix bare pattern entries and `play section` entries in one arrangement.

## Master

```symphra
master { limiter { ceiling -0.3 db } }
```

The limiter is the only master processor in Draft 0.1.
