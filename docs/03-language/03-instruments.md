# Instruments

An instrument answers: *given a note or hit, what samples do we hear?*

```symphra
instrument name = <kind> …
```

Tracks and arrangements refer to instruments by name.

## Built-in oscillators

```symphra
instrument lead = triangle

instrument pad = sine {
  envelope {
    attack 20ms
    decay 200ms
    sustain 0.5
    release 300ms
  }
}
```

- Kinds: `sine`, `triangle`
- Optional `envelope` block: ADSR with `attack` / `decay` / `release` in time
  units (`ms`) and `sustain` as a 0–1 level
- Without `envelope`, a simple edge fade is used

There is **no** `synth sine` wrapper — only supersaw uses the `synth` keyword.

## Supersaw

```symphra
instrument fb_saw = synth supersaw {
  voices 5
  detune 0.4
  spread 0.7

  envelope {
    attack 10ms
    decay 150ms
    sustain 0.3
    release 100ms
  }
}
```

| Field | Role |
| --- | --- |
| `voices` | Unison count |
| `detune` | Pitch spread across voices |
| `spread` | Blend between center and outer voices (not stereo width) |
| `envelope` | Optional ADSR, same shape as oscillators |

Stereo image still comes from track-level `pan`, not from `spread`.

## Sampled one-shot

```symphra
instrument riser = sampled {
  source "assets/riser.wav"
  root C4
}
```

- `source` — path relative to the `.sym` file
- `root` — pitch that maps to untransposed playback

## Sample pack (`sampler`)

```symphra
instrument kit = sampler {
  pack "assets/one_shots"
}
```

Pattern steps use **indices**: `sample 0`, `sample 3 velocity 100`. Files resolve
as `<pack>/<index>.wav` (implementation convention shared with named drums).

## Drum machine

```symphra
instrument tr909 = drum_machine {
  bank "assets/RolandTR909"
}
```

Pattern steps use **names**: `drum "bd" velocity 115`. Files resolve as
`<bank>/<name>.wav`.

`sampler` and `drum_machine` share the same sample-event pipeline (including
`chance { retrigger | speed }`, `speed`, `reverse`, …).

## SoundFont

```symphra
instrument music_box = soundfont {
  source "assets/TimGM6mb.sf2"
  preset "MusicBox"
}
```

- Requires both `source` (`.sf2` path) and `preset` (preset name inside the bank)
- Rendered offline via a pure-Rust SoundFont engine

## VST3

```symphra
instrument soft_synth = vst3 {
  source "plugins/SomeSynth.vst3"
  preset "Init"
}
```

- `source` required; `preset` optional
- Unlike other instruments, a VST3 instance is **stateful and track-long**
  (one plugin instance per track consuming the note stream)
- Availability depends on host OS/architecture; end-to-end golden WAV tests are
  not provided for arbitrary plugins

## Choosing an instrument kind

| Need | Prefer |
| --- | --- |
| Quick melodic sketch | `triangle` / `sine` |
| Future-bass style stacks | `synth supersaw` |
| One atmospheric sample | `sampled` |
| Indexed multi-samples | `sampler` |
| Named drum voices | `drum_machine` |
| GM / banked pitched instruments | `soundfont` |
| Your existing soft synth | `vst3` |
