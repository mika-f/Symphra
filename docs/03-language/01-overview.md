# Language overview

A Symphra source file is a sequence of top-level declarations. In practice you
almost always write one `project` block and one `song` block.

```symphra
project { … }

song "Title" {
  // context
  tempo …
  meter …
  key …

  // materials
  instrument …
  rhythm …
  pattern …

  // performance
  track …

  // form
  section …
  arrangement { … }

  // mix bus
  master { … }
}
```

## Compilation pipeline (conceptual)

```text
.sym source
    → lexer / parser  (symphra-syntax)
    → HIR + checks    (symphra-compiler)
    → score           (symphra-score)
    → render + DSP    (symphra-render, symphra-dsp, assets)
    → WAV             (symphra-export)
```

The CLI wires these steps through `symphra-engine`. Errors are reported with
source spans.

## Comments and whitespace

- Line comments: `// …`
- Layout is free-form; the formatter normalizes spacing
- Identifiers are ASCII-style names used for instruments, patterns, tracks, …
- Strings use double quotes: `"Warm Pad"`, `"assets/kick.wav"`

## Two arrangement styles

**Bare patterns** (small sketches):

```symphra
arrangement { melody with lead harmony }
```

**Sections** (full form; tracks required):

```symphra
section intro bars 4 {
  parallel exact {
    play track arp
    play track pad
  }
}

arrangement {
  play intro
  play drop
}
```

A single song uses one style or the other — not both. See
[Sections and arrangement](./07-sections-and-arrangement.md).

## Guide order

| Page | Topic |
| --- | --- |
| [Project and song](./02-project-and-song.md) | Global settings and musical context |
| [Instruments](./03-instruments.md) | Sound sources |
| [Patterns and rhythms](./04-patterns-and-rhythms.md) | Notes, steps, hits |
| [Tracks and pipelines](./05-tracks-and-pipelines.md) | Performance graph |
| [Effects and automation](./06-effects-and-automation.md) | Delay, filter, reverb, LFO |
| [Sections and arrangement](./07-sections-and-arrangement.md) | Large-scale form |

For token-level detail, use the [Reference](/reference/grammar/) section.
