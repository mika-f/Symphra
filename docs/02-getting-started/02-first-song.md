# Your first song

This walkthrough renders a short phrase to WAV with the CLI.

## 1. Write a file

Create `hello.sym`:

```symphra title="hello.sym"
project {
  seed 20260811
  sample_rate 48khz
  output stereo
}

song "Hello" {
  tempo 120bpm
  meter 4/4
  key C major

  instrument lead = triangle {
    envelope {
      attack 10ms
      decay 80ms
      sustain 0.4
      release 80ms
    }
  }

  pattern phrase = sequence {
    note C4 for 1/4 velocity 100
    note E4 for 1/4 velocity 90
    note G4 for 1/4 velocity 100
    rest for 1/4
  }

  arrangement { phrase with lead }
}
```

What this does:

- **`project`** fixes the RNG seed and audio format
- **`song`** sets tempo, meter, and key
- **`instrument lead`** is a triangle wave with an ADSR envelope
- **`pattern phrase`** is a four-event sequence
- **`arrangement`** plays that pattern once with the lead instrument

## 2. Render

From the repository root (or with `symphra` on your `PATH`):

```console
cargo run -p symphra --locked -- hello.sym hello.wav
```

On success the CLI prints `wrote hello.wav`. Open the file in any audio player.

Asset paths (samples, SoundFonts, VST3s) are resolved **relative to the
`.sym` file**, not your shell’s working directory.

## 3. Grow the piece

Natural next steps:

1. Add a `rhythm` and `|> trigger_with` to chop a longer pattern
2. Declare a second instrument and a `track` with `volume` / `effect`
3. Introduce `section … bars N { parallel exact { play track … } }` and an
   arrangement that sequences sections

Full guide: [Language overview](/language/overview/).  
Showcase: `examples/draft-0.1/001-example.sym`.

## Common failures

| Symptom | Likely cause |
| --- | --- |
| Parse / compile diagnostics | Typo, unknown keyword, or missing required field |
| Sample / SoundFont read error | Wrong relative path from the `.sym` file |
| Absolute asset path rejected | Paths must be relative |
| VST3 load error | Plugin missing, wrong architecture, or host cannot open it |
| Empty or very short WAV | Arrangement never plays a pattern/section, or all rests |
