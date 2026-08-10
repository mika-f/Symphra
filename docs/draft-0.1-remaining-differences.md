# Remaining Differences from the Original Draft 0.1

This is a cold-session handoff for continuing implementation of the original
Draft 0.1 example at <https://pastebin.com/uFQMfkcn>. It compares that example
with the repository on 2026-08-10.

The committed baseline was `7d396ae` (`feat(language): support sampler
playback speed`). Since then, two further slices landed:

- `feat(language): support chance retrigger and chance speed` generalizes
  `chance { transpose N }` into a `ChanceTransform` enum (`Transpose`,
  `Retrigger`, `Speed`) shared by syntax, HIR, and scheduling. `chance {
  retrigger N }` (N total attacks, dividing the selected sample event's
  duration evenly, sampler/drum-machine tracks only, N >= 2) and `chance {
  speed F }` (overrides sampler playback speed for the selected percentage,
  applied after the base `speed`/`alternate { speed }` stage so it always
  wins) are implemented end to end, including formatter and LSP support.
- A drum-instrument vertical slice adds `instrument x = drum_machine { bank
  "..." }` and `drum "voice"` pattern steps inside `steps` patterns. Sample
  identity was generalized from a bare `index: u32` into a `SampleSelector`
  enum (`Index(u32) | Named(String)`) reused by both `sampler`/`sample N` and
  `drum_machine`/`drum "name"`, so both instrument kinds share the existing
  score, scheduling, and render pipeline (including `chance`, `repeat`,
  `reverse`, and `speed`) rather than a second playback path. `bank`/`pack`
  resolve to `<container>/<name-or-index>.wav`, matching the existing sampler
  convention.
- Three further slices landed in this session: `sample N`/`drum "name"` step
  items gained an optional `velocity N` (reusing the existing note/chord
  integer-0-to-127 `VelocityExpression`, threaded into the `SampleTrigger`
  field that already existed in HIR but was previously always defaulted);
  note/chord/rest durations gained a meter-aware `N bar` alternative to
  `N/M` (a new `DurationExpression::Fraction | Bars` AST enum, resolved to a
  whole-note fraction at HIR-lowering time via `count * meter.numerator /
  meter.denominator`, mirroring how `at N:M` already resolves against the
  song meter); and tracks gained a `layer { use x { play ... } use y { play
  ... } }` body as an alternative to the single `instrument`/`play` form. See
  below for details on each.
- A fourth slice, in a follow-up continuation of the same session, adds one
  end-to-end audio effect: `effect delay { mix M time T feedback F }` on a
  track, a feedback delay line applied to that track's rendered audio before
  it is summed into the master mix. See §7 below for the ownership decision
  and DSP details.
- A fifth slice, in a new session on 2026-08-10, adds `section <name> bars <N>
  { parallel [exact] { play track <name> ... } }` and a section-referencing
  `arrangement { play <name> }` form, alongside the existing bare-pattern
  arrangement form. See §9 below for the design decisions (section reuse,
  `exact` semantics, and backward compatibility) and implementation details.
- A sixth slice, in a follow-up continuation of the same 2026-08-10 session,
  adds `master { limiter { ceiling C } }`: a song-level, peak-detect-and-scale
  limiter applied to the whole summed master buffer before the renderer's
  final `[-1, 1]` safety clamp and PCM output conversion. See §10 below for
  the ordering/algorithm decisions and implementation details.
- A seventh slice, in a new session on 2026-08-10, adds a second `effect`
  kind: `effect filter { cutoff C resonance R }`, a resonant lowpass biquad
  filter applied to a track's rendered audio the same way `effect delay`
  already was. `EffectDeclaration` was generalized into an `EffectKind::Delay
  | Filter` enum (mirroring the `ChanceTransform` precedent) rather than a
  grammar rework, exactly as §7 had anticipated. See §7 below for the
  cutoff/resonance design decisions and implementation details.
- An eighth slice, in a follow-up continuation of the same session, adds a
  third `effect` kind: `effect reverb { mix M size S }`, a reduced (4 comb, 2
  allpass) Schroeder reverberator applied to a track's rendered audio the
  same way `delay`/`filter` already are. `EffectKind` gained a `Reverb { mix,
  size }` variant — another token-and-arm addition, no grammar rework. See §7
  below for the topology/tuning decisions and implementation details.
- A ninth slice, in a new session on 2026-08-10, adds `automate cutoff { lfo
  <sine|triangle> { range A..B rate N cycles/bar } }`: a track-scoped
  parameter-automation block that sweeps an `effect filter`'s `cutoff`
  between `range`'s bounds via an LFO, instead of holding it at its static
  value. This is the first (and, so far, only) implemented case of the
  original's general `automate { lfo ... }` mechanism. See §7 below for the
  scoping/topology decisions and implementation details.
- A tenth slice, in a new session on 2026-08-10, closes most of §4's gap:
  `instrument x = sine { envelope { attack Ams decay Dms sustain S release
  Rms } }` / `= triangle { envelope { ... } }` add an optional configurable
  ADSR amplitude envelope to the two existing oscillator instrument kinds,
  and a new `instrument x = synth supersaw { voices N detune D spread S
  [envelope { ... }] }` instrument kind adds a detuned-sawtooth-unison
  instrument reusing the same envelope. `sine`/`triangle` keep their bare,
  envelope-less form as a fully backward-compatible default; the `synth`
  keyword is only ever paired with `supersaw`. SoundFont and VST3
  instruments (§5) remain unimplemented. See §4 below for the envelope gain
  formula, the supersaw detune/blend model, and why `sine`/`triangle`
  weren't wrapped in `synth` to match the original verbatim.
- An eleventh slice, in a new session on 2026-08-10, adds
  `instrument x = soundfont { source "..." preset "..." }`: a pitched
  instrument backed by a `.sf2` `SoundFont` file, synthesized via the
  external `rustysynth` crate (a pure-Rust, MIT-licensed `SoundFont`
  synthesizer) rather than a hand-rolled decoder — the first external
  runtime dependency added to the audio pipeline. A new `symphra-soundfont`
  crate wraps it behind the same asset-library shape `symphra-sampler`
  already established (`SoundFontLibrary`, `decode_soundfont`, a
  `SoundFontVoice` that plugs into `symphra-render`'s existing per-note
  `Voice` abstraction). VST3 (the other half of §5) remains unimplemented —
  see §5 below for why they were split and the SoundFont design decisions.
- A twelfth slice, in a new session on 2026-08-10, closes out §5 by adding
  `instrument x = vst3 { source "..." [preset "..."] }`: a pitched
  instrument backed by a live VST3 plug-in instance, the last item on this
  document's continuation-order list. It was originally assumed this would
  need the GPLv3 `vst3-sys` bindings (matching the Steinberg VST3 SDK's own
  historical license), but this slice instead uses `vst3-host` (MIT), an
  independently reimplemented VST3 host built on the `vst3` bindings crate
  (dual MIT/Apache-2.0) rather than any vendored Steinberg SDK source — so
  the whole workspace stays MIT-licensed, with no GPL boundary or feature
  flag needed. A VST3 instrument is architecturally unlike every other
  instrument kind: it is a persistent, stateful plug-in instance rather than
  an independent per-note voice, so it is rendered through a new
  `render_track_vst3` path parallel to (not a new arm inside) the existing
  per-note `render_track_notes`. See §5 below for the full design (why one
  plugin instance per track, the static-pan simplification, and why
  end-to-end audio testing is a permanent — not just currently missing — gap
  for this instrument kind).

This document treats tests and Rust types as authoritative. Some other files in
`docs/` describe an older repository state.

## Implemented Draft surface

The following original ideas already have an implementation, although some use
the adjusted syntax described later:

- project seed, sample rate, and mono/stereo output;
- song tempo, meter, and major/minor key;
- sine and triangle built-in instruments;
- pitched single-file `sampled` instruments and indexed `sampler` packs;
- reusable hit/rest rhythms;
- sequence notes, chords, rests, duration fractions, and MIDI velocity;
- fixed-resolution degree and sample steps;
- deterministic weighted degree choices;
- deterministic weighted sample choices, including named `drum "name"`
  alternatives and sample sequences;
- tracks with one instrument and one played pattern;
- track volume, rhythm triggering, gate, transpose, gain, repeat, reverse, fixed
  pan, and alternating pan;
- deterministic `chance { transpose ... }` for pitched events;
- static sampler `speed`;
- alternating sampler speed;
- deterministic `chance { retrigger N }` and `chance { speed F }` for sampler
  and drum-machine sample events;
- named drum voices via `drum_machine { bank "..." }` and `drum "name"`
  pattern steps (`sampler`/`drum_machine` share one `SampleSelector`-based
  sample event model);
- `|> choose_sample 0..3` deterministic per-event sample index selection for
  sampler tracks;
- rhythmic triggering of sample/degree-choice/single-selection-choose steps,
  and the `play drum "bd" with kick_pattern` inline shorthand;
- `at N:M play ...` explicit bar:beat placement, composing correctly with
  every other pipeline stage;
- sequential pattern arrangement with an optional instrument per occurrence;
- deterministic scheduling, sample playback, stereo panning, and WAV export;
- normalized integer `velocity N` (0 to 127) on `sample N`/`drum "name"`
  step items, matching the existing note/chord convention;
- meter-aware `N bar` note/chord/rest durations alongside `N/M` fractions;
- tracks with several independently scheduled `layer { use x { play ... } }`
  voices mixed into one logical track, sharing the track's `role` and
  `volume` while each layer keeps its own instrument and full play pipeline;
- `effect delay { mix M time T feedback F }` on a track: a feedback delay
  line applied to that track's rendered audio (mix/dry-wet blend, meter-aware
  echo time, feedback-decayed repeats) before it is summed into the master
  mix;
- `effect filter { cutoff C resonance R }` on a track: a resonant lowpass
  biquad filter applied to that track's rendered audio the same way `effect
  delay` is, mutually exclusive with it (a track has at most one effect);
- `effect reverb { mix M size S }` on a track: a reduced (4 comb filter, 2
  allpass filter) Schroeder reverberator applied to that track's rendered
  audio, mutually exclusive with `delay`/`filter` (still at most one effect
  per track);
- `automate cutoff { lfo <sine|triangle> { range A..B rate N cycles/bar } }`
  on a track that also has `effect filter { ... }`: sweeps that filter's
  `cutoff` between `range`'s bounds via an LFO synced to the song's tempo
  (`N cycles/bar`), overriding the static `cutoff` value the same way
  `chance { speed F }` overrides base `speed`;
- `section <name> bars <N> { parallel [exact] { play track <name> ... } }`
  declaring a named, fixed-length, reusable group of declared tracks, and
  `arrangement { play <name> }` sequencing section references back-to-back by
  cumulative `bars` offset (coexisting with the original bare-pattern
  `arrangement { pattern_name }` form for track-less songs);
- `master { limiter { ceiling C } }`: a song-level peak-detect-and-scale
  limiter, applied to the whole summed master buffer after every track
  (and its effects) is mixed, before the renderer's final safety clamp and
  PCM output conversion;
- `instrument x = sine { envelope { attack Ams decay Dms sustain S release
  Rms } }` / `= triangle { envelope { ... } }`: an optional configurable ADSR
  amplitude envelope on the two oscillator instrument kinds, replacing the
  renderer's fixed edge fade for that instrument; absent `envelope`, the bare
  `instrument x = sine` form renders exactly as before;
- `instrument x = synth supersaw { voices N detune D spread S [envelope {
  ... }] }`: a unison of `voices` detuned sawtooth oscillators (a new
  `Waveform::Sawtooth`), `detune` controlling the pitch spread and `spread`
  controlling the blend between the center and outer voices, sharing the
  same optional `envelope` and the existing note-scheduling pipeline;
- `instrument x = soundfont { source "..." preset "..." }`: a pitched
  instrument backed by a `.sf2` `SoundFont` preset, synthesized offline via
  `rustysynth` and mixed down to mono under the same track-level `Pan` every
  other instrument kind uses;
- `instrument x = vst3 { source "..." [preset "..."] }`: a pitched
  instrument backed by a live VST3 plug-in instance, rendered through one
  persistent plugin per track fed the track's full note-event sequence
  (rather than one independent voice per note, the model every other
  instrument kind uses), via the MIT-licensed `vst3-host` crate.

## Remaining language and runtime gaps

### 1. Chance transforms — done

```symphra
|> chance 40% { retrigger 2 }
|> chance 15% { speed 1.50 }
```

Both forms are implemented via a `ChanceTransform` enum (`Transpose(i32) |
Retrigger(u32) | Speed(f32)`) shared with the existing `chance { transpose N
}`. Resolved decisions:

- `retrigger N` means `N` total attacks (rejected below 2), not `N`
  additional attacks;
- attacks divide the selected event's existing duration evenly, preserving
  the outer pattern slot's total length;
- the first attack keeps the original event's entity ID; later attacks get a
  derived ID from the same allocator `repeat` uses;
- both `retrigger` and `speed` chance transforms are gated to
  `sampler`/`drum_machine` tracks (mirroring the existing `speed` pipeline
  stage's instrument gating); `transpose` remains gated to pitched patterns;
- chance selection still happens after `repeat` and before `reverse` for
  `transpose`/`retrigger` (so repeated copies roll independently and reversed
  order reflects the outcome); `chance { speed F }` is applied *after* the
  base `speed`/`alternate { speed }` stage (which runs after `reverse`) so
  that a chance-selected override always wins instead of being clobbered by
  the unconditional per-track speed.

The existing `mix(seed ^ event.id.0) % 100 < percent` selection mechanism is
reused unchanged for all three variants.

### 2. Drum instruments and events — partially done

```symphra
instrument tr909 = drum_machine {
  bank "RolandTR909"
}

play drum "bd" with kick_pattern
drum "hh" velocity 0.55
at 1:1 play drum "cr"
```

Implemented: `drum_machine { bank "..." }` instrument declarations and `drum
"name"` items inside a `steps` pattern, scheduled, rendered, formatted, and
completed end to end. Sample identity was generalized into a `SampleSelector`
enum (`Index(u32) | Named(String)`) reused by `SampleTrigger`/`SampleEvent`,
so `sampler`/`sample N` and `drum_machine`/`drum "name"` share one score and
scheduling model — no second playback engine. A drum bank resolves the same
way a sampler pack does: `<bank>/<name>.wav` (via
`symphra_sampler::named_sample_source`, parallel to the existing
`packed_sample_source`). `chance { retrigger ... }` and `chance { speed ... }`
already work on `drum_machine` tracks because the instrument gating in
`fn chance`/`fn speed` accepts both `Sampler` and `DrumMachine`.

`trigger_with` (rhythmic step triggering) now also supports `drum "name"` /
`sample N` steps and `choose { degree ... }` (`DegreeChoice`) steps, not just
note/chord/rest — both `step_duration` (schedule.rs) and the compile-time
`validate_trigger` (lib.rs) were extended together, so:

```symphra
rhythm off_beat resolution 1/8 { hit hit rest rest }
pattern kit = steps 1/8 { drum "bd" drum "hh" drum "bd" drum "hh" }
track drums role beat {
  instrument tr909
  play kit |> trigger_with off_beat
}
```

now works end to end. `choose { sample ... }` (`SampleChoice`) steps are also
triggerable now, but only when every alternative selects exactly one sample —
a multi-sample `sequence { ... }` alternative's total duration depends on
which alternative gets picked, which isn't resolved until the weighted
selection runs during scheduling, so it can't feed a fixed rhythm cell count.
This restriction is enforced in both `validate_trigger` (compile time) and
`step_duration`/`triggered_step` (schedule time, defensively). `choose`
blocks also gained `drum "name"` alternatives alongside `sample N` — both as
a single alternative (`drum "bd" weight 1`) and inside `sequence { ... }`
(freely mixable with `sample N`) — via a generalized
`SampleChoiceAlternative.selectors: Vec<SampleSelectorExpression>`
(`Index(u32) | Named(QuotedString)`), the AST-level counterpart of the
HIR/score `SampleSelector` used everywhere else. So:

```symphra
rhythm off_beat resolution 1/8 { hit rest }
pattern kit = steps 1/8 { choose { drum "bd" weight 1 drum "sn" weight 1 } }
track drums role beat {
  instrument tr909
  play kit |> trigger_with off_beat
}
```

now compiles and schedules: each hit deterministically picks one drum voice
(same weighted-choice mechanism as always) and the miss becomes a rest.

`play drum "bd" with kick_pattern` is also implemented, as sugar rather than a
new runtime concept: `PlayStatement` gained a `source: PlaySource` field
(`Pattern(Identifier) | Drum { name, rhythm, span }`) replacing the old bare
`pattern: Identifier`. At HIR-lowering time, `PlaySource::Drum` synthesizes a
fresh `Pattern` with one step per item in the referenced rhythm — a named
drum trigger (`SampleSelector::Named`) for each `hit`, a `Rest` for each
`hit`-less item, each `rhythm.resolution` long — and registers it in the
song's pattern list like any other pattern. No new scheduling code was
needed; the synthesized pattern flows through the exact same pipeline
(`gate`, `repeat`, `reverse`, `pan`, `chance`, `speed`, `choose_sample`) as a
declared one. Requires a `drum_machine` instrument; combining it with
`|> trigger_with` is a compile error (the rhythm is already given via
`with`, so a second one would be ambiguous/redundant).

`at 1:1 play drum "cr"` (explicit bar:beat placement) is also implemented.
`PlayStatement` gained an `at: Option<AtExpression>` prefix field (new `at`
keyword, `N:M` via a new `:` token), 1-indexed like musical convention. At
HIR-lowering time the bar:beat pair is converted to a plain `hir::Duration`
offset using the song's meter — `offset = ((bar-1) * meter.numerator +
(beat-1)) / meter.denominator` whole notes — and validated (`bar`/`beat` >=
1, `beat` <= the meter's numerator). Scheduling applies the offset as a
*final* pass (`apply_at`, adding it to every note/sample start and to
`track.end`) after every other pipeline stage, rather than using it as the
initial cursor. This was a deliberate choice: `repeat` and `reverse` both
implicitly assume a track starts at absolute zero (e.g. `reverse` mirrors
events using `track.end` as the window, and `repeat` reuses `track.end` as
the inter-copy spacing) — shifting the *start* instead of doing a final
translation would have silently corrupted both. Shifting only at the very
end lets every earlier stage keep operating in the pattern's own `[0,
duration]` space and stay correct regardless of where `at` ultimately places
the track.

`sample N`/`drum "name"` step velocity — done. Both step kinds now accept an
optional trailing `velocity N` (`0` to `127`), reusing the same
`VelocityExpression` and parsing helper as note/chord `velocity`, not the
original's decimal `0.55` — see the intentional-differences table. HIR's
`SampleTrigger` already had a `velocity: u8` field (previously always
`DEFAULT_VELOCITY`); the new syntax just threads a real value into it, and
`symphra-render` already scaled sample gain by `velocity / 127`, so no
renderer change was needed. `choose { ... }` alternatives do not accept
`velocity` — that gap was not part of this slice.

### 3. Sample selection pipeline — done

```symphra
|> choose_sample 0..3
```

Implemented as a `choose_sample` pipeline stage (new `..` token) that
overwrites every scheduled sample event's index with a value deterministically
chosen from `mix(seed ^ event.id.0)`, reusing the same selection mechanism as
`chance`. `0..3` is inclusive (four samples), matching the original example.
It runs after `repeat` and `chance` (so retriggered attacks also get their own
independent selection) and is gated to `sampler` tracks only — unlike
`chance`/`speed`, it is not extended to `drum_machine` tracks, since it
operates on the numeric `Index` half of `SampleSelector` and named drum voices
have no ordinal range to pick from. `choose` blocks (compile-time weighted
pattern alternatives) are unrelated and unchanged.

### 4. Synth declarations and envelopes — envelope and supersaw done, SoundFont/VST3 out of scope (see §5)

```symphra
instrument chord_saw = synth supersaw {
  voices 5
  detune 0.35
  spread 0.80
  envelope {
    attack 4ms
    decay 200ms
    sustain 0.50
    release 150ms
  }
}
```

is implemented, along with `instrument lead = sine { envelope { ... } }` /
`= triangle { envelope { ... } }`. Current syntax still declares the bare
oscillators without a `synth` wrapper when no envelope is wanted:

```symphra
instrument lead = sine
instrument soft = triangle
```

**Design: `sine`/`triangle` keep their bare form; `synth` only pairs with
`supersaw`.** The original always wraps every oscillator instrument in
`synth <kind> { ... }`, even a plain `synth sine { envelope ... }`. This
repo's `instrument x = sine` (no `synth`, no braces) was already an
intentional simplification from an earlier session (see the
intentional-differences table). Rather than reopening that decision, `sine`/
`triangle` gained an *optional* trailing `{ envelope { ... } }` — the bare,
brace-less form keeps working unchanged, so every existing `instrument x =
sine` in this document's own examples still parses and renders identically.
`synth` was introduced only as the new supersaw's prefix (`synth supersaw {
... }`), since supersaw has no existing bare form to preserve — this mirrors
how `sampled`/`sampler`/`drum_machine` are also dedicated keyword-and-brace
instrument kinds, just gated behind one extra `synth` keyword the way the
original spells it. `TokenKind::Synth`/`Supersaw`/`Envelope`/`Attack`/
`Decay`/`Sustain`/`Release`/`Voices`/`Detune`/`Spread` are ten new real
keyword tokens (mirroring how `Delay`/`Mix`/`Time`/`Feedback` were added for
`effect`) — `sine`/`triangle` remain plain identifier text, validated at
compile time exactly as before.

**Grammar.** `InstrumentBody::Builtin(Identifier)` became `InstrumentBody::
Oscillator { waveform, envelope: Option<EnvelopeDeclaration>, span }` and a
new `InstrumentBody::Supersaw { voices, voices_span, detune: EffectFactor,
spread: EffectFactor, envelope: Option<EnvelopeDeclaration>, span }`.
`envelope { attack Ams decay Dms sustain S release Rms }` is its own small
grammar production (`fn envelope` in the parser), reused identically by
both instrument kinds: `attack`/`decay`/`release` reuse the `RateLiteral`
grammar already used by `hz`/`khz`/`bpm` (validated as `ms` at compile
time, the same `context`-parameterized-error convention `frequency_hz`
already established for `hz`/`khz`); `sustain` reuses `EffectFactor`, the
same dimensionless `0.0..=1.0` ratio `mix`/`resonance`/`size` already use.
`SongStatement::Instrument` had to become `Instrument(Box<InstrumentDeclaration>)`
(clippy's `large_enum_variant`, since `InstrumentBody` grew once `Supersaw`
carries four value fields plus an optional four-field envelope) — a pure
indirection change with no effect on any existing match/construction site
past updating one parser call site.

**Validation.** `envelope`'s `attack`/`decay`/`release` must be finite and
`>= 0ms` (unlike `effect delay { time }`, a *zero*-length stage is a
legitimate "skip this stage" value — `attack 0ms` means an instant attack —
so, unlike `effect`'s existing zero-duration rejection, these are allowed to
be zero); `sustain` must be finite and in `0.0..=1.0`. `supersaw`'s `voices`
must be at least 1 (rejected below that, no upper bound — consistent with
`repeat`'s uncapped `u32` count elsewhere in this grammar); `detune`/
`spread` are both finite `0.0..=1.0` factors, identical checks to `effect`'s
`mix`/`resonance`/`size`.

**DSP — envelope.** `symphra_dsp::Envelope { attack_frames, decay_frames,
sustain, release_frames }` (already-resolved sample frames, not `ms`) and
`envelope_gain(sample_index, total_samples, envelope) -> f32` are new
primitives. Unlike `fade_gain`'s symmetric attack/release ramp (both sides
independently peak at `1.0`, so `.min()` is enough to combine them),
`sustain` can be below `1.0`, so `envelope_gain` computes an attack-decay-
sustain *level* first (ramping `0.0` to `1.0` over `attack_frames`, then to
`sustain` over `decay_frames`, then holding), and multiplies that level by
a separate `0.0..=1.0` release ramp anchored to the note's *end* (mirroring
`fade_gain`'s "final `release_frames` before the end" window) — release
scales whatever level attack/decay left the note at, rather than always
ramping from `1.0`, so a `sustain` below `1.0` still glides smoothly to
silence instead of jumping. `symphra-render`'s `dsp_envelope`/
`envelope_ms_to_frames` resolve a score-level `Envelope` (milliseconds) to
this frame-based one at the render sample rate — allowing zero frames,
unlike `symphra_dsp`'s own internal `ms_to_frames` (used by `apply_reverb`,
which clamps to at least one frame since a zero comb/allpass delay would
read and write the same sample in one step).

**DSP — supersaw.** `symphra_dsp::Waveform` gained a third `Sawtooth`
variant: a naive (non-band-limited) ramp computed directly from
`SineOscillator`'s existing phase (`phase / PI - 1.0`), the same
simplification precedent `Triangle`'s `asin`-derived shape already
established (no band-limiting there either). `SupersawOscillator::from_midi`
builds `voices` `Oscillator`s of `Waveform::Sawtooth`, each detuned by a
cents offset spread evenly across `+-50 * detune` cents (`50` cents at
`detune 1.0` is a conventional supersaw detune range) and weighted by
`spread` — `next_sample` returns the weighted average. **`spread`
simplification:** the original's `spread` most likely means stereo pan
width (spreading detuned voices left/right), but this renderer's note loop
mixes one oscillator voice down to a single scalar sample under one
track-level `Pan` (`render_track_notes`'s existing `Voice`/`mix_sample`
pipeline, shared by every instrument kind); giving supersaw a genuinely
independent per-voice stereo path would mean a second, divergent render
loop just for this one instrument. Instead `spread` is implemented as a
blend control — `0.0` weights only the least-detuned (center-most) voices,
`1.0` weights every voice equally — which stays entirely inside the
existing single-voice-per-instrument abstraction (`SupersawOscillator`
plugs into `Voice` exactly like `Oscillator` already does) at the cost of
not matching the original's literal stereo-width reading. `render_track_notes`
gained a `Voice::Supersaw` arm alongside `Voice::Oscillator`/`Voice::Sample`;
`render_track_samples`'s sampler/drum-machine-only container match gained a
`Supersaw` arm on the same "not sample-based" side as `Sine`/`Triangle`/
`Sampled`.

**Still missing:** SoundFont and VST3 instruments (§5) — separate,
larger, external-dependency-heavy backends left for their own slice.

### 5. SoundFont and VST3 instruments — both done

```symphra
instrument music_box = soundfont {
  source "instruments/gm.sf2"
  preset "gm_music_box"
}

instrument lead = vst3 {
  source "instruments/synth.vst3"
  preset "Warm Pad"
}
```

are both implemented (`soundfont`'s `source` field is a repo addition — see
the intentional-differences table; `vst3`'s `preset` is optional, unlike
`soundfont`'s required one — see below).

**Why SoundFont and VST3 stayed split (across sessions, not in the grammar).**
The original groups both under one heading, but they are unrelated
engineering problems: SoundFont is a static sample-and-envelope *asset
format* with several mature pure-Rust parsers already available, while VST3
is a *live plug-in protocol* requiring a host implementation and
license-compliance decisions — an order of magnitude more work with no
shared code between the two. Doing SoundFont first, alone, was a scoping
decision made with the user rather than an oversight; VST3 was picked up in
a later session once wanted (see the recommended continuation order). The
language surface reflects this too: `soundfont` and `vst3` are two entirely
separate `InstrumentBody`/`InstrumentKind` variants with no shared grammar
production, exactly as `sampled`/`sampler`/`drum_machine` already are.

**Dependency decision.** `rustysynth` (MIT-licensed, pure Rust, actively
maintained, no `unsafe` at this crate's call boundary) was chosen over
hand-rolling an SF2 parser and wavetable synthesizer. This is a deliberate
departure from every other instrument/effect in this codebase, which are all
implemented from scratch in `symphra-dsp`/`symphra-sampler` — but the
SoundFont 2 spec is a large, precisely-specified binary format (RIFF-based,
nine interlinked `pdta` sub-chunks, a full generator/modulator/envelope
model) whose correct reimplementation would dwarf every other slice in this
document combined, for a result strictly worse than an existing, tested,
permissively-licensed crate. `rustysynth` is a new dependency of a new
`symphra-soundfont` crate only — no other crate in the workspace depends on
it directly, keeping the "hand-rolled DSP" boundary intact everywhere else.

**Grammar.** `instrument x = soundfont { source "..." preset "..." }` is a
new `InstrumentBody::SoundFont { source, preset, span }` variant, parsed
exactly like `sampled`/`sampler`/`drum_machine` (a dedicated keyword,
`soundfont`, opening a brace-delimited body of required fields). Two new
keyword tokens, `Soundfont` and `Preset`, were added; `source` reuses the
existing `Source` token `sampled` already uses (same field, generalized
meaning — see below).

**Design: `source` is required, unlike the original.** The original's
`soundfont { preset "gm_music_box" }` never names a `.sf2` file at all,
presumably assuming some implicit soundfont catalog the draft doesn't
define. Since a `.sf2` asset has to be loaded from somewhere, this adds
`source` — mirroring `sampled { source "..." root ... }`'s shape exactly,
the same precedent `sampler`/`drum_machine`'s `pack`/`bank` already
established for resolving named assets to files (though those resolve to a
convention-based path, `<container>/<name>.wav`, while `soundfont`'s
`source` names the file directly, since one `.sf2` file can back many
different `preset`-selected patches — there is no equivalent one-name-per-
file convention to build).

**Validation.** `source`/`preset` must both be non-empty strings, checked at
compile time (mirroring `sample source path must not be empty` for
`sampled`). Neither the file's existence nor the preset's presence inside it
can be checked at compile time — like `sampled`/`sampler`/`drum_machine`,
that only happens once the asset is actually loaded, at render time.

**HIR/Score.** `InstrumentKind::SoundFont { source: String, preset: String }`
is a new variant in both `hir` and `symphra-score`, alongside the existing
`Sine`/`Triangle`/`Supersaw`/`Sampled`/`Sampler`/`DrumMachine` — a
pitched instrument like `Sampled`, not a sample-selector-based one like
`Sampler`/`DrumMachine`, so it plugs into every place that already
distinguishes those two families (trigger validation, `at`/`transpose`
gating, etc.) without new gating logic. `Score` gained a
`soundfont_sources()` iterator returning `(source, preset)` pairs, mirroring
`sampled_sources()`/`packed_samples()`'s "asset locations the caller must
preload" contract — unlike `sampled_sources`, the preset name has to travel
alongside the source path, since one file can back several presets.

**`symphra-soundfont` (new crate).** Mirrors `symphra-sampler`'s shape:
`SoundFontLibrary` (a `source -> Arc<rustysynth::SoundFont>` cache, like
`SampleLibrary`), `decode_soundfont(bytes) -> Result<SoundFont, DecodeError>`
(like `decode_wav`), `find_preset(font, name) -> Option<&Preset>` (exact
name match over `SoundFont::get_presets()`), and `SoundFontVoice` (like
`SamplePlayer`) — a single-note voice that starts a `note_on` immediately at
construction (selecting the resolved preset's bank/patch via MIDI
bank-select + program-change messages on channel 0, since `rustysynth`'s
`Synthesizer` is channel/MIDI-message-driven, not a direct
"play this preset" API) and renders mono `f32` samples one at a time,
internally buffering `rustysynth::Synthesizer::render`'s block-based stereo
output (averaging left/right down to mono — see the render-integration note
below) and refilling as the buffer drains.

**Render integration.** `render_track_notes` gained a `soundfont_library`
parameter and a `Voice::SoundFont(Box<SoundFontVoice>)` arm (boxed to keep
`Voice`'s other, much smaller variants from growing — `large_enum_variant`);
resolving a note's instrument to a `Voice` was extracted into a new
`note_voice` helper to stay under clippy's `too_many_lines` threshold once
the `SoundFont` arm was added (a mechanical extraction, not a behavior
change). `render_track_samples`'s sampler/drum-machine-only dispatch gained
`SoundFont` on the "not sample-based, reject" side, same as
`Sine`/`Triangle`/`Supersaw`/`Sampled`. No configured `envelope` (§4)
applies to `soundfont` instruments — the `.sf2` preset already carries its
own attack/decay/sustain/release per the spec, so this instrument kind
simply keeps the renderer's fixed edge fade (like every other instrument
without an explicit `envelope`) rather than doubly enveloping the signal;
similarly, no explicit `note_off` is sent — the note rings for its fixed
`NoteEvent` duration and is cut off by that same edge fade, mirroring how
every other instrument's fixed-duration voice already works, rather than
modeling the preset's own release phase and extending the render buffer for
it (the way `effect delay`/`reverb` tails already do, for a different
reason). **Simplification:** `rustysynth::Synthesizer::render` produces
genuine stereo output (shaped by the `.sf2` preset's own pan/effects
sends), but this renderer's shared per-note pipeline mixes one voice down to
a single scalar sample under one track-level `Pan`
(`render_track_notes`/`mix_sample`, the same pipeline every instrument kind
already goes through) — so `SoundFontVoice` averages left/right to mono
before handing a sample back, discarding the preset's own stereo image
rather than adding a second, divergent stereo-voice render path for this
one instrument kind.

**API surface.** `symphra-render`'s existing `render_song`/
`render_song_with_samples` (and `symphra-engine`'s `render_score`/
`render_source`) keep their exact prior signatures unchanged — a `soundfont`
instrument with no soundfont loaded simply errors with
`RenderError::MissingSoundFont`, the same shape `MissingSample` already
has. A new `render_song_with_assets(score, song_index, sample_library,
soundfont_library)` (and `symphra-engine`'s matching
`render_score_with_assets`) is the richer entry point that actually loads
`soundfont` instruments; `apps/symphra-cli` now calls this one, with a new
`load_soundfonts` alongside the existing `load_samples` (same relative-path
resolution and absolute-path rejection). This additive-only shape was
chosen specifically to avoid a breaking change to two function signatures
with many existing call sites/tests, mirroring how `Song.master` and other
past slices stayed "new, orthogonal, optional" additions rather than
reopening settled contracts.

**Testing.** Unlike every other fixture in this workspace's tests (built as
byte literals directly in Rust, no fixture files — see `fn wav` in
`symphra-sampler`/`symphra-cli`'s own tests), a valid `.sf2` file is a
meaningfully bigger binary format (RIFF `INFO`/`sdta`/`pdta` chunks, the
`pdta` list alone needing `phdr`/`pbag`/`pmod`/`pgen`/`inst`/`ibag`/`imod`/
`igen`/`shdr` sub-chunks with fixed-size binary records terminated by
sentinel records). `symphra-soundfont`'s tests build the smallest buffer
`rustysynth` accepts this same way anyway — one 8-sample mono tone, one
instrument zone (a single `sampleID` generator, relying on the spec's
default full-0..127 key range rather than an explicit `keyRange`
generator), one preset (a single `instrument` generator) — verified against
the real `rustysynth` parser rather than assumed correct. `apps/symphra-cli`
duplicates the same builder for one true end-to-end
"loads a `.sf2` file from disk and renders audible audio through it" test,
mirroring the existing WAV-fixture convention of small, deliberate
duplication over a shared test-only dependency; `symphra-render`'s own
tests stick to the established "error-path only, no real audio" convention
already used for `Sampler`/`DrumMachine` (an unloaded soundfont's
`RenderError::MissingSoundFont`), since `find_preset`/decode correctness
is already covered directly in `symphra-soundfont`.

**Still missing (SoundFont only):** automating any `soundfont` parameter
(not applicable today, since `soundfont` has no automatable parameter of its
own — `.sf2` presets are static assets, not song-declared values like
`effect filter`'s `cutoff`).

#### VST3

```symphra
instrument lead = vst3 {
  source "instruments/synth.vst3"
  preset "Warm Pad"
}
```

is implemented via a new `symphra-vst3` crate wrapping the external
`vst3-host` crate — a real offline VST3 host, not a stub. `preset` is
optional (unlike `soundfont`'s required one): not every plugin exposes a
useful named program list, and absent it the plugin's own default program
applies.

**Dependency decision.** The obvious path — `vst3-sys`, hand-generated
bindings to Steinberg's own VST3 SDK — is GPLv3-licensed, which would have
forced either a license-boundary split (a GPLv3 `symphra-vst3` crate behind
an opt-in feature flag, kept out of the default MIT build) or accepting
GPLv3 for the whole workspace. Neither was needed: `vst3-host` (MIT) is
built on `vst3` (dual MIT/Apache-2.0), an independently reimplemented set of
VST3 COM interface bindings that does not vendor or link against Steinberg's
own SDK source. This keeps the workspace's existing all-MIT licensing intact
with no split, and — like `rustysynth` for SoundFont — `vst3-host` is a
dependency of the new `symphra-vst3` crate only; no other crate in the
workspace depends on it directly. `vst3-host`'s public API also has zero
`unsafe` code (it does the FFI/COM work internally), so this slice needed no
exception to the workspace's `unsafe_code = "deny"` lint.

**Grammar.** `instrument x = vst3 { source "..." [preset "..."] }` is a new
`InstrumentBody::Vst3 { source, preset: Option<QuotedString>, span }`
variant, parsed like `soundfont` (a dedicated `vst3` keyword opening a
brace-delimited body) but with `preset` optional rather than required —
parsed only if the `preset` keyword follows `source`, the same optional-
trailing-field shape `envelope` already uses after an oscillator body. Only
one new keyword token, `Vst3`, was needed; `source`/`preset` reuse the
existing tokens `sampled`/`soundfont` already established.

**Validation.** `source` must be non-empty; `preset`, if present, must also
be non-empty — the same checks `soundfont` uses, `preset` just being
optional here. Neither the plugin's existence nor a named preset's presence
inside it can be checked at compile time, the same "only checked once
actually loaded, at render time" precedent every other asset kind in this
document already established.

**Design: VST3 does not fit the per-note `Voice` model.** Every other
pitched instrument (`Sine`, `Triangle`, `Supersaw`, `Sampled`, `SoundFont`)
is rendered as one independent, fresh `Voice` per `NoteEvent`
(`note_voice`/`render_track_notes`), with the renderer applying its own
fade/envelope, gain, velocity, and per-event pan uniformly on top. A VST3
plug-in is a persistent, *stateful* object: it must receive every note-on
and note-off for a track through one long-lived instance so polyphony,
voice-stealing, and the plugin's own internal effects behave correctly —
instantiating a fresh plugin per note (as the existing model would) would
silently break chords (three simultaneous notes would become three
unrelated plugin instances that can't interact) and any state a plugin
keeps across notes, on top of being far too expensive to actually do per
note. So `vst3` instruments get their own render path,
`render_track_vst3`, called by `render_track` *instead of*
`render_track_notes` (not a new arm inside it) — it builds one plugin
instance for the whole track, feeds it every note in the track as
note-on/note-off pairs, and renders the track's full frame range in a
single pass. Per-note host-side shaping does not apply here — the plugin
owns its own amplitude envelope entirely (the same reasoning `soundfont`
already established for why it ignores the configured `envelope` feature).
`track.gain` still applies as a scalar; `track.pan`, however, is applied as
**one static value for the whole rendered buffer**
(`track.pan.percent(0)`), not alternated per note — once the plugin has
mixed every note into one continuous stream there is no discrete per-note
segment left to alternate across. Unlike every other instrument kind (which
mixes down to mono before panning), the plugin's own genuinely stereo
output is preserved: `pan` is applied as a per-channel gain trim on top of
that stereo signal (the same role a mixing console channel strip's pan
knob plays on an already-stereo channel), only downmixing to mono for a
`Channels::Mono` song.

**The "beat = frame" trick.** `vst3-host`'s `Timeline`/`MidiClip` schedule
MIDI events at *beat* positions and drive a plugin block by block
(`Timeline::drive_block`). Rather than adding a second tempo/meter
conversion path alongside the render pipeline's existing
`time_to_frame`-based one, `symphra-vst3` picks
`bpm = sample_rate_hz * 60` for its internal `Timeline`, which makes
`samples_per_beat == 1` — so a "beat" position *is* a frame index, and the
already-resolved `start_frame`/`end_frame` values `render_track_vst3`
computes via the same `time_to_frame` every other instrument kind uses feed
straight into `MidiClip::with` with no new unit conversion in this crate at
all.

**`symphra-vst3` (new crate).** `Vst3Library` is a cache of *validated
source paths*, not loaded plugins — unlike `Arc<SoundFont>` or decoded
sample data, a `vst3_host::Plugin` owns an exclusive loaded native module
and live COM instance, so it cannot be `Clone`d or shared across tracks the
way every other preloaded asset in this workspace can be. `validate_plugin`
loads a plugin once (via `vst3_host::simple::get_plugin_info`) just to
confirm it's loadable, then drops it — this is what `apps/symphra-cli`'s
new `load_vst3s` calls per unique source before rendering, so a broken
plugin path still surfaces as a clean preload error rather than a
mid-render one, matching `load_soundfonts`'s contract even though the real
plugin instantiation is unavoidably deferred to render time.
`render_vst3_track(source, preset, sample_rate_hz, total_frames, notes)` is
the whole-track render entry point described above; `preset` resolves to a
program index via `Plugin::get_units()`'s exact-name match over each unit's
program list, then `Plugin::select_program`.

**Render integration.** `render_song_with_assets` gained a `vst3_library`
parameter alongside `sample_library`/`soundfont_library` — bundled with them
into a small `AssetLibraries` struct so `render_track` stays under clippy's
`too_many_arguments` threshold now that a track can reference three
independent asset kinds. `render_track` branches on the instrument kind
before calling into notes/samples rendering, exactly the "declared-track-
only" dispatch precedent every other instrument-kind-specific behavior in
this document already follows. `song_frames` needed no change — like
`soundfont`, a `vst3` track has no known decaying tail to extend the buffer
for; it renders exactly the track's existing frame window and is cut off
there, the same "fixed event window" precedent `soundfont` established.

**API surface.** Exactly the additive-only shape `soundfont` established:
`render_song`/`render_song_with_samples` keep their prior signatures
unchanged (defaulting to an empty `Vst3Library`, so an unrendered `vst3`
instrument simply errors with `RenderError::MissingVst3Plugin`, the same
shape `MissingSoundFont` already has); `render_song_with_assets` and
`symphra-engine`'s `render_score_with_assets` gained the new
`vst3_library`/`vst3s` parameter as their one richer entry point.
`apps/symphra-cli` calls it with the new `load_vst3s` alongside
`load_samples`/`load_soundfonts` (same relative-path resolution and
absolute-path rejection).

**Testing — a permanent gap, not a currently-missing one.** Every other
asset-backed instrument in this document (WAV samples, `.sf2` SoundFonts)
is a documented, byte-level file format this workspace's tests can
hand-build a minimal fixture for entirely in Rust — see `fn wav` and
`minimal_soundfont` across `symphra-sampler`/`symphra-soundfont`/
`symphra-cli`'s own tests. A `.vst3` plugin is compiled native code (a
platform dynamic library inside a bundle), not a format with a minimal
valid byte sequence that can be authored by hand — there is no equivalent
fixture to build, in this environment or any other, without an actual
compiled plugin binary. So `symphra-vst3`, `symphra-render`, and
`apps/symphra-cli`'s tests all stay on the "error-path only, no real audio"
side (a nonexistent plugin path rejected by `validate_plugin`/
`render_vst3_track`; `RenderError::MissingVst3Plugin` for an unloaded
`vst3` instrument; an absolute-path rejection in the CLI) — unlike
SoundFont, `apps/symphra-cli` does **not** get a real end-to-end "loads a
file and renders audible audio" test for `vst3`, because there is no
fixture-building path to it. Verifying real VST3 audio requires manually
pointing `symphra-cli` at an actual installed plugin outside this
environment.

**Still missing:** automating any `vst3` parameter (not applicable — a VST3
plugin's own parameters are not exposed as song-declared values the way
`effect filter`'s `cutoff` is), and this document's continuation-order list
has no further numbered phase after this one.

### 6. Layers and per-layer instruments — done

```symphra
track bass role low {
  volume -3db
  layer {
    use sub_sine { play bass_roots |> gain 1.0 }
    use sub_triangle { play bass_roots |> gain 0.6 |> pan -20% }
  }
}
```

A track body is now `instrument X <play>` (unchanged) **or** `[volume]
layer { use A { <play> } use B { <play> } ... }`. `TrackDeclaration` gained a
`body: TrackBody` field (`Single { instrument, play } | Layers { uses,
span }`); `volume` stays track-level and shared, matching "mixed into one
logical track."

The smallest semantic model — an independently scheduled voice mixed into one
logical track — turned out to need no new scheduling, score, or render code:
each `use` clause is exactly as expressive as the existing single-instrument
track body (own instrument, own pattern, and the full existing pipeline —
`trigger_with`, `gate`, `transpose`, `gain`, `repeat`, `reverse`, `pan`,
`chance`, `speed`, `choose_sample`, `at`), so the compiler's `fn track` was
split into a dispatcher (`Single` → one call, `Layers` → one call per `use`)
over an unchanged `fn track_layer` that now takes `(instrument, play)`
instead of reading them off `TrackDeclaration` directly. Each `use` lowers to
its own `hir::TrackDefinition` sharing the track's `name`/`role`; the
render/score pipeline already just sums every track's audio, so multiple
`TrackDefinition`s sharing one declared name mix together "for free" — no
render-side layer concept was needed. Patterns must still be declared at song
scope; the original's inline `pattern phrase`/`pattern hats` declarations
inside tracks remain unsupported (out of scope for this slice — layers only
addressed multi-instrument mixing, not inline pattern declarations).

### 7. Effects and automation — `delay`, `filter`, `reverb`, and one `automate` case done

The original declares effects and LFO-driven automation as blocks of their
own, outside any track:

```symphra
automate filter.lowpass.cutoff {
  lfo sine {
    range 600hz..2800hz
    rate 2cycles/bar
  }
}

effect filter.lowpass { resonance 0.40 }
effect reverb { mix 0.40 size 0.80 }
effect delay { mix 0.40 time 1/4 feedback 0.25 }
```

Current syntax nests one effect inside the track it applies to, since there
is no separate routing/bus concept to resolve a free-floating `effect` block
against:

```symphra
track drums role beat {
  instrument tr909
  play kit
  effect delay { mix 0.40 time 1/4 feedback 0.25 }
}
```

**Ownership decision.** The doc's own guidance was to decide processing order
first: per layer, per track after layer mixing, or master. Since a `layer`
block already lowers each `use` into its own independent `hir::TrackDefinition`
(§6) with no intermediate "layer-mixed" buffer, applying the effect per
underlying track (rather than after a layer-mixing step that doesn't exist)
was the only option that needed no new render concept — and it is provably
equivalent to "after layer mixing" for this effect: a feedback delay is a
linear time-invariant system, so `delay(a) + delay(b) == delay(a + b)` when
both layers share the same delay parameters (which they do, since `effect` is
declared once per `TrackDeclaration` and resolved once in `fn track`, not
once per `use`, specifically so a bad effect is reported once rather than
once per layer). `at` and `chance`'s per-track gating already established this
"declared-track-only" precedent — `effect` follows it too; arrangement-only
(undeclared) tracks have no way to attach an effect, same as gain/pan/chance.

**Grammar.** `effect delay { mix M time T feedback F }`. At the time this was
written, `delay` was the only accepted effect kind —
`TokenKind::Delay`/`Mix`/`Time`/`Feedback` are real keywords (mirroring how
`sampler`/`drum_machine` are dedicated instrument-kind keywords, not compared
as plain identifiers), so accepting `filter`/`reverb` later is a
token-and-arm addition, not a grammar rework. `time` reuses the note/chord/rest
`DurationExpression` (`1/4` or `1bar`) introduced in §8; `mix`/`feedback` are
bare floats via a new `EffectFactor { value: f32, span }`.

**Validation.** `mix` must be finite and in `0.0..=1.0`. `feedback` must be
finite and in `0.0..1.0` for stability, but is additionally capped at `0.95`
(not `1.0`) — the renderer must extend the song's audio buffer to fit the
delay's decaying echo tail, and `0.95` bounds that tail to at most ~135
repeats (`ln(0.001) / ln(0.95)`) regardless of how close to the true
stability limit a user asks for. `time` reuses the existing zero-duration
check (`item_duration`), so `time 0bar` is rejected the same way `for 0bar`
already is.

**DSP.** `symphra_dsp::apply_delay` is a new primitive: `echo[n] =
wet[n - delay_frames]`, where `wet[n] = dry[n] + feedback * echo[n]`, and
`out[n] = dry[n] * (1 - mix) + echo[n] * mix` — a standard feedback delay
line, computed per channel over the whole buffer (this is offline rendering,
not a real-time process, so there is no circular-buffer/streaming constraint).
A zero `delay_frames` is clamped to `1` before use: at `0` the line would read
and write the same sample in the same step.

**Render integration.** `symphra-render`'s two whole-song passes
(`render_notes`/`render_samples`, each looping over every track and mixing
directly into one shared buffer) became one per-track function
(`render_track`, calling new `render_track_notes`/`render_track_samples`)
called once per track from a loop in `render_song_with_samples`. A track with
no effect still renders straight into the master buffer (the original fast
path, unchanged). A track with an effect renders into a same-length scratch
buffer first, gets `apply_delay`'d, and is then summed into the master
buffer. `song_frames` (the function that sizes the master buffer) now also
folds in each effected track's tail length, so echoes are never truncated.

**`effect filter { cutoff C resonance R }` (added in a later session).** The
original pairs `effect filter.lowpass { resonance 0.40 }` with a separate
`automate { lfo ... }` block that sweeps the cutoff over time. Since general
parameter automation is not implemented (see "still missing" below), a filter
with only `resonance` and no cutoff would not be independently usable, so this
adds a static `cutoff` alongside `resonance` rather than waiting on
`automate`. Only a lowpass response is implemented, matching the one filter
type the original example shows — there is no `filter.highpass`/`.bandpass`
variant and no dotted-name grammar; `filter` alone means lowpass, the same way
`delay` alone means feedback delay.

`EffectDeclaration` was generalized from a delay-only struct into `{ kind:
EffectKind, span }`, where `EffectKind` is `Delay { mix, time, feedback } |
Filter { cutoff, resonance }` — the token-and-arm addition §7 anticipated, not
a grammar rework. A track still has at most one `effect` block, so `delay`
and `filter` are mutually exclusive per track (not chainable); this mirrors
how `chance`'s `Transpose | Retrigger | Speed` variants are also one-per-track.
`cutoff` reuses the `hz`/`khz` `RateLiteral`/`FrequencyLiteral` grammar
already used by `sample_rate`; `resonance` reuses `EffectFactor` like
`mix`/`feedback`.

**Validation.** `cutoff`'s unit must be `hz` or `khz` and its resolved hertz
value must be finite and greater than zero; `resonance` must be finite and in
`0.0..=1.0`. Checking `cutoff` against the Nyquist frequency needs the
project's sample rate, which is a separate namespace from song/track
compilation (project and song are compiled independently) — rather than
threading sample rate through track compilation for this one check, the
renderer clamps `cutoff` defensively at render time instead, the same
defensive-revalidation precedent already established for a
hand-constructed `MasterLimiter`.

**DSP.** `symphra_dsp::apply_filter` is a new primitive: a standard RBJ Audio
EQ Cookbook resonant lowpass biquad, computed per channel over the whole
buffer as a direct-form-I difference equation (again offline, so no
real-time coefficient-smoothing concern). `resonance` (`0.0` to `1.0`) maps
linearly to filter Q (`0.7`, approximately Butterworth, to `10`, a sharp
resonant peak short of self-oscillation). `cutoff_hz` is clamped to
`(0, nyquist * 0.999)` inside `apply_filter` itself, since it is the only
place that knows the render sample rate and can bounds-check against it.
Unlike `apply_delay`, a resonant filter has no tail that needs the render
buffer extended — its (theoretically infinite) impulse response decays
within the buffer's own existing length, and any resonant overshoot is left
to the renderer's final `[-1, 1]` safety clamp — so `song_frames` only grows
the buffer for a `Delay` effect, never for `Filter`.

**Render integration.** The delay-only branch in `render_song_with_samples`
became `apply_track_effect`, a small helper matching on `Effect::Delay |
Filter` (later `| Reverb`, see below) and dispatching to
`apply_delay`/`apply_filter`/`apply_reverb` respectively, called once per
effected track from the same loop. The no-effect fast path is unchanged.

**`effect reverb { mix M size S }` (added in a later session).** The
original's `effect reverb { mix 0.40 size 0.80 }` is implemented verbatim —
no design gap to fill the way `filter` needed a static `cutoff` in place of
LFO automation, since `mix`/`size` are exactly what the original shows.

**Topology.** A classic Schroeder reverberator: parallel feedback comb
filters summed and averaged, then run through series allpass filters for
diffusion. Freeverb later popularized this same topology with 8 comb + 4
allpass filters tuned for a specific studio-quality sound; this uses a
reduced 4-comb/2-allpass version (Schroeder's original 1962 design also used
4 comb + 2 allpass) — enough to sound like a genuine reverb without the
extra tuning surface area an 8-comb design would need to justify, in
keeping with how every other effect in this codebase implements one
representative variant rather than a configurable family (`delay` is one
fixed feedback-delay topology, not a pluggable multi-tap/ping-pong choice;
`filter` is a lowpass biquad, not a selectable filter type). Comb/allpass
delay times are the millisecond-equivalent of four of Freeverb's own comb
delays and two of its allpass delays (so tone stays consistent as sample
rate changes), scaled from Freeverb's 44.1kHz reference rather than
invented from scratch.

**Validation.** `mix` reuses the identical `0.0..=1.0` check `delay`'s `mix`
already uses. `size` must be finite and in `0.0..=1.0` — a plain factor, not
a frequency or duration, so (unlike `filter`'s `cutoff`) it needs no
cross-namespace Nyquist-style check against the project's sample rate.

**DSP.** `symphra_dsp::apply_reverb` computes, per channel: four parallel
feedback comb filters (`comb[n] = dry[n] + feedback * comb[n - delay]`,
mirroring `apply_delay`'s recursion but without delay's separate dry/wet mix
stage) summed and averaged, then two series Schroeder allpass filters
(`allpass[n] = -g * input[n] + input[n - delay] + g * allpass[n - delay]`,
fixed `g = 0.5`) for diffusion, then a final `mix` blend against the dry
signal exactly like `apply_delay`'s blend stage. `size` (`0.0` to `1.0`)
maps to comb filter feedback (`0.7` to `0.98`, Freeverb's own
roomsize-to-feedback formula), kept below `1.0` so every comb filter is
unconditionally stable regardless of `size`. Like `apply_filter`, the
recursive math runs in `f64` internally (cast to `f32` only at the final
output) to keep rounding error from compounding across the comb/allpass
chain. `symphra_dsp::reverb_tail_frames(sample_rate_hz, size)` is a second
new public primitive — the comb delay times are an `apply_reverb`
implementation detail, not something `mix`/`size` expose to callers, so the
render-side tail-length estimate (mirroring `delay_tail_frames`'s own
epsilon-bounded formula) lives in `symphra-dsp` too rather than being
duplicated in `symphra-render` against magic numbers it doesn't otherwise
know about.

**Render integration.** `song_frames` grows the render buffer for a
`Reverb` effect's decaying tail the same way it already does for `Delay`
(via `reverb_tail_frames`); `Filter` still needs no tail, since its
response — like `apply_reverb`'s — is left to the renderer's final safety
clamp for any final decaying resonance below the epsilon threshold, but only
`delay`/`reverb` extend the buffer up front since only their known-tail
length can be bounded ahead of render time.

**`automate cutoff { lfo <sine|triangle> { range A..B rate N cycles/bar } }`
(added in a later session).** The original declares `automate` as a
free-floating, song-scoped block naming a dotted parameter path
(`filter.lowpass.cutoff`), independent of any track. Since every effect in
this codebase is declared per track rather than through a bus/routing
concept (see the intentional-differences table), a free-floating `automate`
block has nothing to resolve its dotted path against. This adds `automate
cutoff { ... }` as a second, optional block *inside* the same track that
declares `effect filter { ... }` — mirroring exactly how `effect` itself
already moved from free-floating to track-scoped — so `cutoff` unambiguously
means "this track's filter's cutoff" without needing a dotted path at all.
`cutoff` is the only automatable target implemented, since it is the
original's only worked `automate` example; automating `delay`/`reverb`
parameters, or `filter`'s `resonance`, is a natural but unimplemented
extension (see "still missing" below).

**Validation.** `automate cutoff` requires the same track to declare
`effect filter { ... }` — a compile error names what was found instead
(`no effect block`, `` `effect delay` ``, or `` `effect reverb` ``) when it
doesn't. `lfo`'s waveform is validated as plain identifier text (`"sine"` or
`"triangle"`), the same way `instrument x = sine` already is, not via a
dedicated keyword token. `range`'s two bounds reuse `frequency_hz` — the
same `hz`/`khz`-unit, positive-frequency check `cutoff` itself uses,
factored out of the filter-effect resolver into a shared helper
parameterized by an error-message `context` string. `rate`'s `N` must be finite and greater
than zero (a non-positive rate would either stall the sweep or run it
backwards nonsensically).

**Design: `cutoff` stays static-plus-override, not automate-only.** The
original has no static `cutoff` at all when `automate` drives it — `effect
filter.lowpass { resonance 0.40 }` never mentions `cutoff`. This
implementation's `effect filter { cutoff C resonance R }` still requires a
`cutoff`, even when paired with `automate cutoff`, and the LFO's swept value
simply supersedes it at render time — the same override-always-wins pattern
`chance { speed F }` already established over the base `speed` stage. This
was chosen over making `cutoff` optional (and required only when `automate`
is absent) because it keeps `effect filter`'s own grammar and validation
completely unchanged by this slice — `automate` layers on top as a pure
addition rather than reopening a settled feature's contract.

**`N cycles/bar` stays tempo/meter-agnostic through HIR.** Resolving a
`cycles/bar` rate to an LFO frequency in Hz needs both the song's tempo and
meter, neither of which is in scope where `effect`/`automate` are compiled
(song-level settings and per-track declarations are resolved separately).
Rather than threading tempo through track compilation for this one case,
`hir::FilterAutomation`/`score::FilterAutomation` carry the raw
`cycles_per_bar: f32` unresolved — mirroring exactly how `DelayEffect.time`
stays a tempo-agnostic `Duration` until `symphra-render`'s `time_to_frame`
resolves it. A new `automation_rate_hz` in `symphra-render` performs the
equivalent conversion: one bar is `meter.numerator / meter.denominator`
whole notes, and one whole note lasts `240 / tempo_bpm` seconds (the same
relationship `time_to_frame` uses), so `rate_hz = cycles_per_bar *
meter.denominator * tempo_bpm / (240 * meter.numerator)`.

**DSP.** `apply_filter`'s biquad-coefficient math was factored out into
`lowpass_biquad_coefficients`, shared with a new `apply_filter_automated`.
The latter precomputes the LFO's per-frame cutoff trajectory once up front
(so a stereo buffer sweeps identically on both channels, rather than each
channel running its own independent LFO phase), then recomputes biquad
coefficients from scratch on every single frame from that trajectory —
unlike a real-time plugin, which would only recompute periodically to save
CPU, offline rendering has no such constraint, so recomputing at audio rate
keeps the implementation simple without a correctness trade-off. `triangle`
reuses the same `sin(x).asin() * 2/pi` shaping the existing oscillator
already uses for its own triangle waveform.

**Render integration.** `apply_track_effect`'s `Filter` arm now branches on
`filter.automation`: `None` calls `apply_filter` exactly as before (the
existing static-cutoff behavior, unchanged); `Some` computes `lfo_rate_hz`
via `automation_rate_hz` and calls `apply_filter_automated` instead. Like a
static filter, automation adds no tail — `song_frames` is unaffected.

**Still missing:** automating any parameter other than `filter`'s `cutoff`
(the original's own general dotted-path `automate` mechanism would cover
`delay`/`reverb`/`resonance` too, but only `cutoff` has a worked example to
implement against), and any effect declared outside a track (there is no
bus/master effect chain — see §10).

### 8. Bar durations and bracketed chords — bar durations done, brackets intentionally not restored

The original uses `for 1bar` and bracketed pitches:

```symphra
chord [F3 A3 C4 E4 G4] for 1bar
```

Current syntax accepts either fraction or bar durations, still without
brackets:

```symphra
chord F3 A3 C4 E4 G4 for 1bar
chord F3 A3 C4 E4 G4 for 1/1
```

`note`/`chord`/`rest` durations now accept `N bar` as an alternative to
`N/M` (a new `bar` keyword token; `Nbar`/`N bar` both lex fine since
whitespace is discarded, matching the existing rate-literal convention). The
AST's `duration_numerator`/`duration_denominator` fields became one
`duration: DurationExpression` (`Fraction { numerator, denominator } | Bars {
count }`), since resolving `bar` requires the song's meter, which — like `at
N:M` — is not known until HIR-lowering time. `N bar` normalizes to
`N * meter.numerator / meter.denominator` whole notes there (in 4/4, `1bar`
is `1/1`; in 3/4, `1bar` is `3/4`), sharing the same zero-duration validation
as fractions. Step-pattern `resolution N/M` is unaffected — only note, chord,
and rest durations gained the alternative. Brackets were deliberately omitted
from the current chord syntax and still need not be restored unless
compatibility with the original example is preferred over the simpler form.

### 9. Sections, parallel playback, and arrangement syntax — done

```symphra
section phrase bars 4 {
  parallel exact {
    play track chords
    play track bass
  }
}

arrangement {
  play phrase
}
```

is implemented exactly as written above. The original's bare-pattern
`arrangement { intro chorus with piano }` form is unchanged and still works
for track-less, pattern-only songs; the two forms cannot be mixed in one
song (a song with declared tracks may only use a section-referencing
arrangement, and vice versa).

**Resolved semantics** (decided via user Q&A this session, since the doc
previously flagged all four as open questions):

- **Section duration.** `bars N` resolves to a whole-note `Duration` using
  the same formula as `N bar` note/chord/rest durations
  (`count * meter.numerator / meter.denominator`); `bars 0` is rejected the
  same way other zero durations are.
- **Simultaneous track activation.** A section body is exactly one
  `parallel [exact] { play track <name> ... }` block (matching the original
  example precisely — no bare sequential `play track X` items, no nested or
  multiple `parallel` blocks per section, since the doc never specified that
  grammar). Every `play track X` inside it schedules that track (and, if it
  is a `layer`-bodied track, every one of its layers) independently through
  the existing full per-track pipeline, then all of them are shifted by the
  same absolute offset — the section's position in the arrangement's
  cumulative `bars` total.
- **`exact`.** When present, every track in the `parallel exact` block must,
  after its full pipeline runs (before the section's own offset is added),
  last exactly `bars N` — a schedule-time `ScheduleError::
  SectionTrackLengthMismatch` otherwise. Without `exact`, a track may be
  shorter or longer than `bars N`; the next section's offset is still only
  the cumulative `bars` total (not the actual scheduled length), so an
  overlong track simply plays into the next section's window unclamped.
- **Section reuse.** The same `section` name may appear more than once in an
  `arrangement`; each `play <name>` entry is an independent occurrence
  placed at its own cumulative offset. Each occurrence gets its own
  namespaced entity-ID salt (`section_track_occurrence_id`, combining the
  arrangement entry's own fresh ID with the track's ID) fed into
  `schedule_track`'s `occurrence` parameter in place of the track's bare ID
  — mirroring how the pre-existing bare-pattern arrangement path already
  namespaces by each occurrence's own fresh ID rather than the pattern's ID
  — so `chance`/`choose_sample`/`retrigger` roll independently per
  occurrence instead of producing byte-identical repeats.

**Implementation shape.** `SongStatement::Section` (name, `bars: u32`,
`exact: bool`, referenced track names) is a new song-level declaration,
parsed and lowered alongside `track`/`pattern`. `arrangement` entries became
an `ArrangementEntry` enum (`Pattern(ArrangementOccurrence) | Play { name }`)
so the parser accepts both forms; `hir::Arrangement` similarly became
`Patterns(Vec<PatternOccurrence>) | Sections(Vec<SectionOccurrence>)`. The
compile-time exclusivity check ("track declarations cannot be combined with
a pattern arrangement") was relaxed to only fire for the *pattern* form —
declared tracks plus a *section* arrangement is now the expected, required
combination, since sections cannot exist without tracks to reference.
Scheduling adds `schedule_sectioned_tracks`, a small variant of the existing
sequential-cursor arrangement loop that accumulates `bars`-derived
`MusicalTime` instead of each scheduled track's own end, and reuses the
existing `apply_at` pass verbatim as the per-track absolute-offset shift (the
same "final, additive shift after every other pipeline stage" reasoning `at
N:M` already established). No new Score or render-level concept was needed —
once scheduled, a sectioned track is indistinguishable from any other
`Track` with an absolute offset baked into its events, exactly like `at`.

### 10. Master processing — one processor (`limiter`) done

```symphra
master {
  limiter { ceiling -0.3db }
}
```

is implemented exactly as written above. `limiter` is the only accepted
master-processor kind so far (mirroring how `delay` is the only accepted
`effect` kind) — a future `filter`/`reverb`-style expansion at song scope
would be a token-and-arm addition, not a grammar rework.

**Ordering** (resolved, was previously an open question): the limiter runs
after every track (and its own per-track `effect`) is rendered and summed
into the master buffer, and before the renderer's final `[-1, 1]` safety
clamp / PCM output conversion — the same point in `render_song_with_samples`
where the doc's own pre-existing clamp already lived.

**Algorithm** (resolved via user confirmation, since a plain `.clamp(-1.0,
1.0)` is explicitly not limiter behavior): peak-detect and whole-buffer gain
reduction. Because rendering is offline (not streaming), the whole master
buffer can be scanned up front for its peak absolute sample; if that peak
exceeds `ceiling`, every sample is uniformly scaled by `ceiling / peak` so
the loudest sample lands exactly at `ceiling`. This preserves the buffer's
relative dynamics/waveform shape — unlike clipping, no sample is
independently distorted — which is what actually distinguishes a limiter
from a clamp. A no-op when the peak is already at or below `ceiling`. The
renderer's original `.clamp(-1.0, 1.0)` still runs unconditionally
afterward as a final safety net (a no-op once the limiter has run, since its
output peak is bounded by `ceiling <= 1.0`; unchanged behavior when no
`master` block is declared).

**Implementation shape.** `ceiling` reuses the existing signed-decibel
`VolumeExpression` grammar verbatim (identical to track `volume -6db`) via a
new `fn ceiling` parser mirroring `fn volume`. HIR lowering
(`fn master`) converts dB to linear amplitude with the same
`10^(db / 20)` formula `track_gain`'s `volume` handling already uses, and
additionally rejects `ceiling > 0db` — a limiter that permits amplification
above 0 dBFS would defeat its purpose, unlike track `volume` which
legitimately allows positive dB boosts. `hir::Song.master` /
`symphra_score::Song.master: Option<MasterLimiter>` are new, orthogonal,
optional fields — no interaction with tracks, sections, layers, or
arrangement. The DSP primitive is `symphra_dsp::apply_limiter(buffer: &mut
[f32], ceiling: f32)`, channel-agnostic (peak detection and gain reduction
are per-sample scalar operations, unlike `apply_delay` which needs
per-channel echo history). `symphra-render` re-validates a hand-constructed
`MasterLimiter`'s ceiling defensively at render time
(`RenderError::InvalidMasterCeiling`), mirroring the existing `effect`
validation precedent, on top of the compile-time `> 0db` rejection.

## Intentional or current syntax differences

These are not necessarily backlog items, but the original Pastebin source will
not compile without translation:

| Original Draft 0.1 | Current implementation |
| --- | --- |
| `instrument x = synth sine { ... }` | `instrument x = sine`, optionally `instrument x = sine { envelope { ... } } }` — envelope supported, `synth` wrapper not (only `synth supersaw` uses `synth`) |
| `chord [C4 E4 G4] for 1bar` | `chord C4 E4 G4 for 1bar` — brackets not restored, `1bar` now supported |
| pattern declaration inside a track | pattern declaration at song scope |
| `layer { use x { pattern phrase ... play ... } }` with inline patterns | `layer { use x { play ... } }` — layers supported, but patterns must still be declared at song scope |
| drum velocity such as `0.55` | `drum "name"`/`sample N` step velocity is now supported, as an integer `0..127` (matching note/chord), not a decimal |
| arbitrary-looking pipeline chain | each supported stage may appear once and is normalized into fixed scheduling phases |
| `automate filter.lowpass.cutoff { ... }` — free-floating, dotted parameter path | `automate cutoff { ... }` nested inside the same `track` that declares `effect filter { ... }` — no dotted path, since the track scoping already resolves which filter |
| `effect filter.lowpass { resonance 0.40 }` with no `cutoff` (driven entirely by `automate`) | `effect filter { cutoff C resonance R }` still requires `cutoff`; `automate cutoff` overrides it at render time rather than replacing it |
| `soundfont { preset "gm_music_box" }` — no file named | `soundfont { source "..." preset "..." }` — `source` is required, mirroring `sampled { source ... root ... }` |

`degree N octave O` currently treats `N` as a chromatic semitone offset from
the song tonic, not a diatonic scale index. Confirm that this matches the
original intent before expanding degree-based harmony.

## Recommended continuation order

1. ~~Review and commit the alternating sampler speed slice.~~ Done
   (`bcb4ec2`).
2. ~~Generalize the chance transform representation and implement sampler
   `chance { retrigger 2 }`.~~ Done.
3. ~~Add `chance { speed 1.50 }` using the same deterministic event
   selection.~~ Done.
4. ~~Decide the named-drum asset contract, then implement `drum_machine`, a
   single named drum event, and its renderer path end to end.~~ Done
   (`SampleSelector` generalization; `drum_machine`/`drum "name"` share the
   sampler pipeline).
5. ~~Add rhythmic drum/sample triggering, and the `play drum "bd" with
   kick_pattern` inline shorthand.~~ Done — `trigger_with` supports `Sample`,
   `DegreeChoice`, and single-selection `Choice` steps, and `play drum "..."
   with <rhythm>` is sugar resolved entirely during HIR lowering.
6. ~~Add `choose_sample 0..3` using deterministic per-event selection, and
   extend `choose { ... }` to accept `drum "name"` alternatives alongside
   `sample N`.~~ Done — both `sample N` and `drum "name"` alternatives are
   freely mixable, single- or multi-item, and single-selection `choose`
   blocks now combine with `trigger_with` too.
7. ~~Add `at N:M play ...` explicit bar:beat placement.~~ Done — applied as
   a final absolute shift after every other pipeline stage so `repeat` and
   `reverse` keep working correctly regardless of where the track lands.
8. ~~Decide whether the next composition milestone needs layers, sections, or
   effects; implement only the chosen vertical slice.~~ Done — layers chosen
   and implemented (`layer { use x { play ... } }`); sections/arrangement and
   effects remain unimplemented (see §7, §9).
9. ~~Add `sample N`/`drum "name"` step `velocity N` and `N bar` note/chord/rest
   durations~~ Done — both landed alongside the layers slice (small,
   decision-free items pulled forward ahead of §8's milestone choice).
10. ~~Decide effect ownership/processing order, then add one end-to-end
    effect.~~ Done — effects are declared per track (applied to that track's
    rendered audio before the master sum; provably equivalent to "after layer
    mixing" for a linear effect like delay, so no separate layer-mixing stage
    was needed); `effect delay { mix M time T feedback F }` is the one
    implemented effect kind (see §7).
11. Leave SoundFont, VST3, supersaw, `filter`/`reverb` effect kinds, general
    `automate`/LFO automation, master processing, and bracketed chord syntax
    until their runtime/backend contracts are concrete, or until
    compatibility with the original example is specifically wanted over the
    current simpler forms.
12. ~~Decide section duration, simultaneous track activation, `exact`, and
    section reuse semantics, then implement sections/arrangement (§9).~~
    Done — section reuse is allowed (each `play <name>` arrangement entry is
    an independent, distinctly-namespaced occurrence), `exact` requires
    every track in the `parallel exact` block to last precisely the
    section's `bars` (schedule-time error otherwise, checked before the
    section's own offset is added), and the original bare-pattern
    `arrangement { pattern_name }` form coexists unchanged alongside the new
    section-referencing `arrangement { play <name> }` form.
13. ~~Decide master processing's ordering (relative to track effects and
    output conversion) and algorithm, then implement `master { limiter {
    ceiling C } }`.~~ Done — the limiter runs after track summation and
    before the renderer's final safety clamp / PCM output conversion, using
    peak-detect-and-scale gain reduction (not clipping), confirmed via user
    Q&A since the doc had explicitly called out plain clamping as not being
    limiter behavior.
14. ~~Add a second `effect` kind (`filter`).~~ Done, in a new session on
    2026-08-10 — `effect filter { cutoff C resonance R }` is a resonant
    lowpass biquad (`symphra_dsp::apply_filter`), mutually exclusive with
    `effect delay` per track (a track still has at most one effect).
    `EffectDeclaration` generalized into `EffectKind::Delay | Filter` exactly
    as §7 had anticipated. `cutoff` is a static `hz`/`khz` value rather than
    LFO-automated, since general `automate` is not implemented yet; see §7.
15. ~~Add a third `effect` kind (`reverb`).~~ Done, in a follow-up
    continuation of the same 2026-08-10 session — `effect reverb { mix M
    size S }` is a reduced (4 comb, 2 allpass) Schroeder reverberator
    (`symphra_dsp::apply_reverb`), mutually exclusive with `delay`/`filter`
    per track (a track still has at most one effect). `EffectKind` gained a
    `Reverb { mix, size }` variant, another token-and-arm addition. Unlike
    `filter`, no design gap needed filling — the original's `mix`/`size`
    parameters are exactly what this implements.
16. ~~Add `automate { lfo ... }` parameter automation, at least for
    `filter`'s `cutoff`.~~ Done, in a new session on 2026-08-10 — `automate
    cutoff { lfo <sine|triangle> { range A..B rate N cycles/bar } }` is
    nested inside the same track as `effect filter { ... }` (no dotted
    parameter path — track scoping already resolves which filter), and
    overrides the filter's static `cutoff` at render time, the same
    override-always-wins pattern `chance { speed F }` already established.
    `cutoff` is the only automatable target, matching the original's one
    worked `automate` example; `N cycles/bar` stays tempo/meter-agnostic
    through HIR/score and is resolved to Hz only in `symphra-render`, the
    same boundary `DelayEffect.time` already crosses.
17. ~~Design a configurable ADSR envelope, then add `synth supersaw` and
    `envelope` blocks (§4).~~ Done, in a new session on 2026-08-10 —
    `instrument x = sine { envelope { attack Ams decay Dms sustain S
    release Rms } }` / `= triangle { envelope { ... } }` add an optional
    ADSR envelope to the two existing oscillators (bare `sine`/`triangle`,
    with no `synth` wrapper, still work unchanged as the envelope-less
    default); `instrument x = synth supersaw { voices N detune D spread S
    [envelope { ... }] }` adds a new detuned-sawtooth-unison instrument
    kind reusing the same envelope. See §4 for the envelope gain formula
    and the supersaw detune/blend design (`spread` is a blend control, not
    literal stereo pan width — the renderer's existing per-note pipeline
    mixes every instrument down to one scalar sample under one track-level
    pan).
18. ~~Decide the SoundFont/VST3 split and dependency approach, then implement
    SoundFont (§5).~~ Done, in a new session on 2026-08-10 — user Q&A chose
    SoundFont first (VST3 deferred, given its much larger scope and GPLv3/
    proprietary dual-licensed SDK). `instrument x = soundfont { source "..."
    preset "..." }` is implemented via a new `symphra-soundfont` crate
    wrapping the external `rustysynth` crate (MIT-licensed, pure Rust) —
    this codebase's first external audio-processing dependency, chosen
    because a from-scratch SoundFont 2 parser/synthesizer would dwarf every
    other slice in this document combined. `source` is a repo addition (the
    original never names a `.sf2` file), mirroring `sampled`'s `source`
    field.
19. ~~Add VST3 instruments.~~ Done, in a new session on 2026-08-10 —
    `instrument x = vst3 { source "..." [preset "..."] }` is implemented via
    a new `symphra-vst3` crate wrapping the external `vst3-host` crate
    (MIT-licensed, built on the independently-reimplemented `vst3` bindings
    rather than Steinberg's own GPLv3 SDK bindings, so the workspace stays
    all-MIT with no license-boundary split). Unlike every other instrument
    kind, a VST3 plug-in is a persistent, stateful object rather than an
    independent per-note voice, so it renders through a new
    `render_track_vst3` path (one plugin instance per track, fed the whole
    track's note sequence) parallel to — not inside — the existing per-note
    `render_track_notes`. See §5 for the full design, including why
    end-to-end audio testing is a permanent gap for this instrument kind
    (a `.vst3` plugin is compiled native code, unlike every other asset kind
    in this document, none of which has a hand-buildable byte-level
    fixture). This was the last item on this document's continuation-order
    list — automating parameters other than `filter`'s `cutoff`
    (delay/reverb mix, resonance, envelope stages, supersaw detune/spread,
    etc.) remains the one open, unscheduled extension, pickable up at any
    point once wanted (see §7's "still missing" note).

## Verification notes

For the chance-retrigger/speed and drum-instrument slices, the full workspace
builds and `cargo test --workspace` passes (the `symphra-lsp` binary may be
locked by a running editor/language-client process on Windows — verify it with
`cargo test -p symphra-lsp --target-dir <alternate-dir>` if so). Strict Clippy
(`-D warnings`) passes for every crate. Workspace-wide `cargo fmt --all --
--check` is blocked only by the pre-existing, unrelated formatting difference
in `apps/symphra-formatter/tests/stdin.rs`.

The same holds for the step-velocity, bar-duration, layers, and effect-delay
slices added in that session: `cargo build --workspace`, `cargo test
--workspace` (with `symphra-lsp` run separately via `--target-dir` as above),
and `cargo clippy --workspace --all-targets -- -D warnings` all pass; `cargo
fmt --all -- --check` is clean except for the same pre-existing `stdin.rs`
difference.

The same also holds for the sections/arrangement slice added in the
2026-08-10 session: `cargo build --workspace --all-targets`, `cargo test
--workspace` (with `symphra-lsp` run separately via `cargo test -p
symphra-lsp --target-dir <alternate-dir>`), and `cargo clippy --workspace
--all-targets -- -D warnings` all pass — 9 new compiler tests
(`crates/symphra-compiler/tests/compile.rs`), 5 new parser tests
(`crates/symphra-syntax/tests/syntax.rs`), and 3 new formatter round-trip
tests plus one new idempotency source
(`crates/symphra-fmt/tests/formatting.rs`) were added. `cargo fmt --all --
--check` is clean except for the same two pre-existing, unrelated
differences noted above (`apps/symphra-formatter/tests/stdin.rs` and the
empty `crates/symphra-syntax/src/parser/literal.rs` stub).

The same also holds for the master-limiter slice added in a follow-up
continuation of the 2026-08-10 session: `cargo build --workspace
--all-targets`, `cargo test --workspace` (with `symphra-lsp` run separately
via `cargo test -p symphra-lsp --target-dir <alternate-dir>`), and `cargo
clippy --workspace --all-targets -- -D warnings` all pass — 4 new DSP unit
tests (`crates/symphra-dsp/src/lib.rs`), 1 new parser test
(`crates/symphra-syntax/tests/syntax.rs`), 3 new compiler tests
(`crates/symphra-compiler/tests/compile.rs`), 3 new render tests
(`crates/symphra-render/src/lib.rs`), and 1 new formatter round-trip test
plus one idempotency source addition
(`crates/symphra-fmt/tests/formatting.rs`) were added. `fn song` in
`crates/symphra-compiler/src/lib.rs` was refactored to extract a
`collect_song_statements`/`SongStatements` helper partway through this
slice, to stay under clippy's `too_many_lines` threshold after adding
`master` handling — a mechanical extraction, not a behavior change. `cargo
fmt --all -- --check` is clean except for the same two pre-existing,
unrelated differences.

The same also holds for the `effect filter` slice added in a new session on
2026-08-10: `cargo build --workspace --all-targets`, `cargo test --workspace`
(with `symphra-lsp` run separately via `cargo test -p symphra-lsp
--target-dir <alternate-dir>`), and `cargo clippy --workspace --all-targets
-- -D warnings` all pass — 4 new DSP unit tests
(`crates/symphra-dsp/src/lib.rs`), 1 new parser test
(`crates/symphra-syntax/tests/syntax.rs`), 5 new compiler tests
(`crates/symphra-compiler/tests/compile.rs`), 2 new render tests
(`crates/symphra-render/src/lib.rs`), and 1 new formatter round-trip test
plus one idempotency source addition (`crates/symphra-fmt/tests/formatting.rs`)
were added. `render_song_with_samples` in `crates/symphra-render/src/lib.rs`
was refactored to extract an `apply_track_effect` helper partway through this
slice, to stay under clippy's `too_many_lines` threshold after adding the
`Effect::Filter` branch — a mechanical extraction, not a behavior change.
`cargo fmt --all -- --check` is clean except for the same two pre-existing,
unrelated differences.

The same also holds for the `effect reverb` slice added in a follow-up
continuation of the same 2026-08-10 session: `cargo build --workspace
--all-targets`, `cargo test --workspace` (with `symphra-lsp` run separately
via `cargo test -p symphra-lsp --target-dir <alternate-dir>`), and `cargo
clippy --workspace --all-targets -- -D warnings` all pass — 7 new DSP unit
tests (`crates/symphra-dsp/src/lib.rs`), 1 new parser test
(`crates/symphra-syntax/tests/syntax.rs`), 3 new compiler tests
(`crates/symphra-compiler/tests/compile.rs`), 2 new render tests
(`crates/symphra-render/src/lib.rs`), and 1 new formatter round-trip test
plus one idempotency source addition (`crates/symphra-fmt/tests/formatting.rs`)
were added. Two mechanical test-only refactors were needed to stay under
clippy's `too_many_lines` threshold after the new assertions were added:
`is_idempotent_across_every_grammar_construct` in
`crates/symphra-fmt/tests/formatting.rs` had its `sources` array extracted
into a standalone `idempotency_sources()` helper, and
`completes_track_and_trigger_keywords` in `apps/symphra-lsp/src/main.rs` had
its `effect`-body completion assertions split into a new
`completes_effect_body_keywords` test — neither changes what is asserted.
`cargo fmt --all -- --check` is clean except for the same two pre-existing,
unrelated differences.

The same also holds for the `automate cutoff` slice added in a new session
on 2026-08-10: `cargo build --workspace --all-targets`, `cargo test
--workspace` (with `symphra-lsp` run separately via `cargo test -p
symphra-lsp --target-dir <alternate-dir>`), and `cargo clippy --workspace
--all-targets -- -D warnings` all pass — 4 new DSP unit tests
(`crates/symphra-dsp/src/lib.rs`, covering `apply_filter_automated`), 1 new
parser test (`crates/symphra-syntax/tests/syntax.rs`), 7 new compiler tests
(`crates/symphra-compiler/tests/compile.rs`), 1 new render test
(`crates/symphra-render/src/lib.rs`), and 1 new formatter round-trip test
plus one idempotency source addition
(`crates/symphra-fmt/tests/formatting.rs`) were added. Adding `range` as a
new reserved keyword broke one pre-existing compiler test that happened to
name its pattern `range`
(`schedule_should_accept_full_midi_pitch_range`) — renamed to
`pitch_range`, the same kind of identifier collision every new keyword in
this language risks, not a logic bug. Two more mechanical test-only
refactors were needed to stay under clippy's `too_many_lines` threshold:
`idempotency_sources` in `crates/symphra-fmt/tests/formatting.rs` was split
further into `idempotency_sources_core`/`idempotency_sources_effects`, and
`keyword_description` in `apps/symphra-lsp/src/main.rs` was split into
`keyword_description_declarations`/`keyword_description_playback`; neither
changes behavior. `cargo fmt --all -- --check` is clean except for the same
two pre-existing, unrelated differences.

The same also holds for the envelope/supersaw slice added in a new session
on 2026-08-10: `cargo build --workspace --all-targets`, `cargo test
--workspace` (with `symphra-lsp` run separately via `cargo test -p
symphra-lsp --target-dir <alternate-dir>`), and `cargo clippy --workspace
--all-targets -- -D warnings` all pass — 7 new DSP unit tests
(`crates/symphra-dsp/src/lib.rs`, covering `envelope_gain`, the new
`Waveform::Sawtooth`, and `SupersawOscillator`), 2 new parser tests
(`crates/symphra-syntax/tests/syntax.rs`), 5 new compiler tests
(`crates/symphra-compiler/tests/compile.rs`), 2 new render tests
(`crates/symphra-render/src/lib.rs`), 1 new formatter round-trip test plus
one idempotency source addition (`crates/symphra-fmt/tests/formatting.rs`),
and 1 new LSP completion test (`apps/symphra-lsp/src/main.rs`) were added.
Three mechanical, behavior-preserving changes were needed to satisfy strict
clippy after the new variants/fields landed: `SongStatement::Instrument`
became `Instrument(Box<InstrumentDeclaration>)` (`large_enum_variant`, since
`InstrumentBody::Supersaw` grew the enum's largest variant well past
`Pattern`'s, the previous runner-up); `fn instrument` in
`crates/symphra-syntax/src/parser/mod.rs` was split into
`supersaw_instrument_body`/`oscillator_instrument_body` helpers
(`too_many_lines`); and `fn print_instrument` in
`crates/symphra-fmt/src/format.rs` was similarly split into
`print_oscillator_instrument`/`print_supersaw_instrument`
(`too_many_lines`, then `too_many_arguments` once split, fixed by bundling
`voices`/`voices_span` into one tuple parameter). `cargo fmt --all --
--check` is clean except for the same two pre-existing, unrelated
differences.

The same also holds for the SoundFont slice added in a new session on
2026-08-10: `cargo build --workspace --all-targets`, `cargo test
--workspace` (with `symphra-lsp` run separately via `cargo test -p
symphra-lsp --target-dir <alternate-dir>`), and `cargo clippy --workspace
--all-targets -- -D warnings` all pass — a new `symphra-soundfont` crate
with 3 unit tests (decode rejection, decode-and-find-preset against a
hand-built minimal `.sf2` fixture, and real audio rendering through a
`SoundFontVoice`), 1 new parser test
(`crates/symphra-syntax/tests/syntax.rs`), 3 new compiler tests
(`crates/symphra-compiler/tests/compile.rs`), 1 new render error-path test
(`crates/symphra-render/src/lib.rs`), 1 new formatter round-trip test
(`crates/symphra-fmt/tests/formatting.rs`, folded into the existing combined
core idempotency source rather than a new one), 2 new LSP tests
(completions and the updated instrument-body-keywords assertion in
`apps/symphra-lsp/src/main.rs`), and 2 new `apps/symphra-cli` tests (a real
end-to-end "loads a `.sf2` file and renders audible audio" test, duplicating
the same minimal-fixture builder used in `symphra-soundfont`'s own tests,
plus an absolute-soundfont-path rejection test) were added. Two mechanical,
behavior-preserving changes were needed to satisfy strict clippy after the
new variant/parameter landed: `Voice::SoundFont` was boxed
(`large_enum_variant`, since `SoundFontVoice` — holding a whole
`rustysynth::Synthesizer` — is far larger than `Voice`'s other variants),
and the note-to-`Voice` instrument dispatch in `render_track_notes` was
extracted into a new `note_voice` helper (`too_many_lines`, after adding the
`SoundFont` arm). `cargo fmt --all -- --check` is clean except for the same
two pre-existing, unrelated differences.

The same also holds for the VST3 slice added in a new session on
2026-08-10: `cargo build --workspace --all-targets`, `cargo test
--workspace` (with `symphra-lsp` run separately via `cargo test -p
symphra-lsp --target-dir <alternate-dir>`), and `cargo clippy --workspace
--all-targets -- -D warnings` all pass — a new `symphra-vst3` crate with 3
unit tests (nonexistent-plugin-path rejection for both `validate_plugin` and
`render_vst3_track`, and `Vst3Library` insert/contains), 2 new parser tests
(`crates/symphra-syntax/tests/syntax.rs`, with and without `preset`), 4 new
compiler tests (`crates/symphra-compiler/tests/compile.rs`: lowering with
and without `preset`, empty-source rejection, empty-preset rejection), 1
new render error-path test (`crates/symphra-render/src/lib.rs`:
`MissingVst3Plugin` for an unloaded plugin — no real plugin needed, the same
"error-path only" convention `MissingSoundFont`'s own test already uses), 2
new formatter round-trip tests plus one idempotency source line addition
(`crates/symphra-fmt/tests/formatting.rs`), 1 new LSP completion assertion
(`apps/symphra-lsp/src/main.rs`, extending the existing
`completes_instrument_body_keywords` test rather than a new test function),
and 1 new `apps/symphra-cli` test (an absolute-vst3-path rejection,
mirroring the existing absolute-soundfont-path one) were added. Unlike
SoundFont, there is no real end-to-end "loads a file and renders audible
audio" test anywhere in this slice — see "Testing" under §5's VST3
subsection for why that gap is permanent rather than closable. Four
mechanical, behavior-preserving changes were needed to satisfy strict
clippy after the new variant/parameter landed: `render_track` in
`crates/symphra-render/src/lib.rs` had its `sample_library`/
`soundfont_library`/new `vst3_library` parameters bundled into a small
`AssetLibraries` struct (`too_many_arguments`, once a track could reference
three independent asset kinds); `fn instrument` in
`crates/symphra-syntax/src/parser/mod.rs` gained a `vst3_instrument_body`
helper, `fn instrument_kind` in `crates/symphra-compiler/src/lib.rs` gained
a `vst3_instrument_kind` helper, and `fn print_instrument` in
`crates/symphra-fmt/src/format.rs` gained a `print_vst3_instrument` helper
— all three the same `too_many_lines` extraction precedent every prior
instrument-kind addition in this document already established. `cargo fmt
--all -- --check` is now fully clean, including the two pre-existing,
unrelated differences this document previously called out
(`apps/symphra-formatter/tests/stdin.rs` and the empty
`crates/symphra-syntax/src/parser/literal.rs` stub) — running `cargo fmt
--all` for this slice happened to resolve both as a side effect.

