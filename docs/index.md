# Symphra documentation

Symphra is a **text-first music language**: you describe a complete piece —
tempo, harmony, instruments, grooves, form — in a `.sym` file, then render it
offline to WAV.

These pages cover purpose, what works today, how to install and run the tools,
and the language itself (guide + grammar reference).

## Start here

1. [What is Symphra?](/introduction/what-is-symphra/) — intent and mental model
2. [Goals and non-goals](/introduction/goals-and-non-goals/) — product boundaries
3. [Capabilities](/introduction/capabilities/) — what you can and cannot do yet
4. [Installation](/getting-started/installation/) — build the CLI and LSP
5. [Your first song](/getting-started/first-song/) — a minimal render path
6. [Language overview](/language/overview/) — how a `.sym` file is structured

## Documentation map

| Section | Contents |
| --- | --- |
| [Introduction](/introduction/what-is-symphra/) | Purpose, goals, capabilities |
| [Getting started](/getting-started/installation/) | Install, first song, editors |
| [Language guide](/language/overview/) | Project, instruments, patterns, tracks, effects, form |
| [Reference](/reference/grammar/) | Grammar sketch, units, pipeline stages |
| [Internals](/internals/architecture/) | Crate layout, implementation status, LSP testing |

## Example

A tiny song you can grow into a full arrangement:

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

  instrument lead = triangle

  pattern phrase = sequence {
    note C4 for 1/4
    note E4 for 1/4
    note G4 for 1/4
    rest for 1/4
  }

  arrangement { phrase with lead }
}
```

```console
cargo run -p symphra --locked -- hello.sym hello.wav
```

For a fuller Showcase (sections, SoundFont, drums, filter automation), see
[`examples/draft-0.1/001-example.sym`](https://github.com/mika-f/Symphra/blob/main/examples/draft-0.1/001-example.sym)
in the repository.
