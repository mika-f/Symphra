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
- `section <name> bars <N> { parallel [exact] { play track <name> ... } }`
  declaring a named, fixed-length, reusable group of declared tracks, and
  `arrangement { play <name> }` sequencing section references back-to-back by
  cumulative `bars` offset (coexisting with the original bare-pattern
  `arrangement { pattern_name }` form for track-less songs);
- `master { limiter { ceiling C } }`: a song-level peak-detect-and-scale
  limiter, applied to the whole summed master buffer after every track
  (and its effects) is mixed, before the renderer's final safety clamp and
  PCM output conversion.

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

### 4. Synth declarations and envelopes

The original instrument syntax is not implemented:

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

The same applies to `synth sine { envelope ... }` and `synth triangle {
envelope ... }`. Current syntax declares the two oscillators directly:

```symphra
instrument lead = sine
instrument soft = triangle
```

The renderer has no supersaw, configurable voice count, detune, spread, or
ADSR envelope. It applies only its fixed edge fade. Implement a configurable
envelope before promising the original synth blocks; supersaw can then reuse
that instrument envelope and the existing oscillator/mixer boundary.

### 5. SoundFont and VST3 instruments

The original SoundFont form is absent:

```symphra
instrument music_box = soundfont {
  preset "gm_music_box"
}
```

There is no SoundFont loader, preset resolver, HIR/score instrument variant, or
renderer integration. Likewise, there is currently no language-level VST3
instrument declaration or offline render path for a VST3 plug-in. These should
remain separate backends even if their declaration syntax eventually shares an
external-instrument shape.

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

### 7. Effects and automation — `delay` and `filter` done; `reverb`/`automate` still gaps

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
Filter` and dispatching to `apply_delay`/`apply_filter` respectively, called
once per effected track from the same loop. The no-effect fast path is
unchanged.

**Still missing:** `reverb` as an effect kind, the general `automate { lfo
... }` parameter-timeline block (which would also let `filter`'s `cutoff`
sweep over time as the original example does), and any effect declared
outside a track (there is no bus/master effect chain — see §10).

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
| `instrument x = synth sine { ... }` | `instrument x = sine` |
| `chord [C4 E4 G4] for 1bar` | `chord C4 E4 G4 for 1bar` — brackets not restored, `1bar` now supported |
| pattern declaration inside a track | pattern declaration at song scope |
| `layer { use x { pattern phrase ... play ... } }` with inline patterns | `layer { use x { play ... } }` — layers supported, but patterns must still be declared at song scope |
| drum velocity such as `0.55` | `drum "name"`/`sample N` step velocity is now supported, as an integer `0..127` (matching note/chord), not a decimal |
| arbitrary-looking pipeline chain | each supported stage may appear once and is normalized into fixed scheduling phases |

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
15. The next milestone, if wanted, is §7's remaining `reverb` effect kind and
    `automate`/LFO (each needs its own DSP/ownership decisions, as `delay`,
    `filter`, and `limiter` did — `automate` would also be what finally lets
    `filter`'s `cutoff` sweep over time, matching the original example);
    then §4 synth envelopes/supersaw (needs a configurable ADSR envelope
    designed first); then §5 SoundFont/VST3 (largest, most
    external-dependency-heavy, last).

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

