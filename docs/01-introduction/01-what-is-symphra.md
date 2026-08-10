# What is Symphra?

Symphra is a **programming language for music composition**. A program is a
song: declarations for sound sources, rhythmic and harmonic material, tracks
that play that material, optional effects, and an arrangement that places
everything in time. The primary deliverable is an **offline WAV render**, not a
live performance host.

## Mental model

Think of a `.sym` file as a score plus a small mixer, written as code:

1. **`project`** — render settings (sample rate, mono/stereo, RNG seed)
2. **`song`** — musical context (tempo, meter, key) and everything that follows
3. **Instruments** — how notes and hits become sound
4. **Rhythms and patterns** — *when* and *what* is played
5. **Tracks** — bind instruments to patterns, with volume, role, pipeline, and
   at most one effect
6. **Sections and arrangement** — large-scale form (intro → drop → outro, …)
7. **Master** — song-level processing (today: a peak limiter)

The compiler lowers source into a score; the renderer walks that score and
writes PCM audio. Choices (`chance`, weighted `choose`, `choose_sample`) are
**deterministic** given `project { seed ... }`, so the same file produces the
same WAV.

## Why text?

- **Version control** — diffs, reviews, and branches for musical structure
- **Automation-friendly** — CI can compile and render fixtures
- **LLM-friendly** — structured syntax with a small surface area, designed so
  humans and language models can co-author songs
- **Reproducible** — seed + source → bit-stable offline audio (modulo external
  plugins such as VST3)

## Relationship to DAWs and live coding

Symphra is **not** a digital audio workstation UI, and it is **not** a
live-coding REPL in the style of Tidal/Strudel (though Draft 0.1 was informed by
ideas from that world). You edit source, compile, listen to a render, revise.
Editor tooling (LSP, formatter, syntax highlighting) is part of the workflow;
real-time audio I/O is not the primary product.

## Name and spirit

The project frames composition as a collaboration: a human direction, a
machine-readable score language, and tools that make the loop tight. The
language stays small enough to learn end to end, while still aiming at
**complete pieces** — not only loops or one-shots.
