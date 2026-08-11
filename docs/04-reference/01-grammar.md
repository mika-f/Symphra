# Grammar reference

This is a **readable sketch** of the Draft 0.1 surface, not a formal EBNF
guaranteed by the parser tests. When in doubt, `crates/symphra-syntax` and the
compiler tests are authoritative.

## Lexical structure

| Kind | Examples |
| --- | --- |
| Keyword | `project`, `song`, `track`, `play`, … |
| Identifier | `bass_saw`, `intro` |
| Integer | `4`, `127`, `20260811` |
| Decimal | `0.55`, `1.5` |
| String | `"Aoharu Signal"`, `"assets/x.wav"` |
| Pitch | `C4`, `F#3`, `Bb2`, `C-1` |
| Rate / unit literals | `48khz`, `150bpm`, `10ms`, `-6db`, `1400hz` |
| Operators | `=`, `|>`, `..`, `:`, `/`, `*`, `%`, `{`, `}`, `(`, `)`, `,` |
| Comment | `// line comment` |

## Top level

```text
File        = { Declaration }
Declaration = Project | Song

Project = "project" "{" { ProjectStmt } "}"
ProjectStmt =
    "seed" Integer
  | "sample_rate" Rate
  | "output" ("mono" | "stereo")

Song = "song" String "{" { SongStmt } "}"
```

## Song body (summary)

```text
SongStmt =
    "tempo" Rate
  | "meter" Integer "/" Integer
  | "key" PitchClass Mode
  | Instrument
  | Rhythm
  | Pattern
  | Track
  | Section
  | Arrangement
  | Master

Mode = "major" | "minor"
```

## Instruments

```text
Instrument =
  "instrument" Ident "=" InstrumentBody

InstrumentBody =
    "sine" [EnvelopeBlock]
  | "triangle" [EnvelopeBlock]
  | "synth" "supersaw" "{" SupersawFields "}"
  | "sampled" "{" "source" String "root" Pitch "}"
  | "sampler" "{" "pack" String "}"
  | "drum_machine" "{" "bank" String "}"
  | "soundfont" "{" "source" String "preset" String "}"
  | "vst3" "{" "source" String ["preset" String] "}"

EnvelopeBlock = "{" "envelope" "{"
    "attack" Time "decay" Time "sustain" Number "release" Time
  "}" "}"
```

(`sine` / `triangle` may also appear as bare identifiers without a block.)

## Rhythms and patterns

```text
Rhythm =
  "rhythm" Ident "resolution" Duration "{" { RhythmItem } "}"

RhythmItem = ("hit" | "rest" | Group) [Repeat]

Pattern =
  "pattern" Ident "=" PatternBody

PatternBody =
    "sequence" "{" { SequenceItem } "}"
  | "steps" Duration "{" { StepItem } "}"

SequenceItem = SequenceAtom [Repeat]

SequenceAtom =
    "note" Pitch "for" Duration ["velocity" Integer]
  | "chord" Pitch { Pitch } "for" Duration ["velocity" Integer]
  | "rest" "for" Duration
  | Group

StepItem =
    "choose" "{" ChooseAlt { ChooseAlt } "}"     // not repeatable
  | StepAtom [Repeat]

StepAtom =
    "rest"
  | Sequence-like note/chord forms (as accepted by steps)
  | "sample" Integer ["velocity" Integer]
  | "drum" String ["velocity" Integer]
  | "degree" Integer "octave" Integer
  | Group

Group  = "(" Item { "," Item } ")"               // Repeat is mandatory
Repeat = "*" Integer                             // Integer >= 1

ChooseAlt =
  (SampleOrDrum | "sequence" "{" … "}") "weight" Integer

Duration =
    Integer "/" Integer
  | Integer "bar"
```

## Tracks

```text
Track =
  "track" Ident "role" Ident "{" TrackBody "}"

TrackBody =
    ["instrument" Ident]
    ["volume" Level]
    ( PlayStmt | LayerBlock )
    [Effect]
    [AutomateCutoff]

LayerBlock =
  "layer" "{" { "use" Ident "{" PlayStmt "}" } "}"

PlayStmt =
  ["at" Integer ":" Integer]
  "play" PlaySource { "|>" PipelineStage }

PlaySource =
    Ident
  | "drum" String "with" Ident
```

Pipeline stages are listed in [Pipeline stages](./03-pipeline-stages.md).

## Effects, sections, master

```text
Effect =
    "effect" "delay" "{" "mix" Number "time" Duration "feedback" Number "}"
  | "effect" "filter" "{" "cutoff" Freq "resonance" Number "}"
  | "effect" "reverb" "{" "mix" Number "size" Number "}"

AutomateCutoff =
  "automate" "cutoff" "{"
    "lfo" ("sine" | "triangle") "{"
      "range" Freq ".." Freq
      "rate" Number "cycles" "/" "bar"
    "}"
  "}"

Section =
  "section" Ident "bars" Integer "{"
    "parallel" ["exact"] "{" { "play" "track" Ident } "}"
  "}"

Arrangement =
  "arrangement" "{" { ArrEntry } "}"

ArrEntry =
    Ident ["with" Ident]           // bare pattern form
  | "play" Ident                   // section form

Master =
  "master" "{" "limiter" "{" "ceiling" Level "}" "}"
```

## Keywords (non-exhaustive)

`project` `song` `seed` `sample_rate` `output` `tempo` `meter` `key`
`instrument` `sampled` `sampler` `drum_machine` `soundfont` `vst3` `preset`
`source` `root` `pack` `bank` `drum` `sample` `rhythm` `resolution` `hit`
`track` `role` `volume` `layer` `use` `play` `trigger_with` `gate` `transpose`
`gain` `repeat` `reverse` `pan` `alternate` `chance` `speed` `retrigger`
`choose_sample` `at` `pattern` `arrangement` `with` `sequence` `steps`
`degree` `octave` `note` `chord` `rest` `for` `velocity` `bar` `effect`
`delay` `mix` `time` `feedback` `filter` `cutoff` `resonance` `reverb` `size`
`automate` `lfo` `range` `rate` `cycles` `section` `bars` `parallel` `exact`
`master` `limiter` `ceiling` `synth` `supersaw` `envelope` `attack` `decay`
`sustain` `release` `voices` `detune` `spread` `choose` `weight`
