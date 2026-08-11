//! Semantic analysis and HIR lowering for Symphra.

use std::collections::HashSet;

use symphra_syntax::SourceSpan;
use symphra_syntax::ast::{
    ArrangementEntry, AutomateDeclaration, ChanceTransformExpression, ChordPitches, Declaration,
    DegreeChoiceAlternative, DurationExpression, EffectDeclaration, EffectKind,
    EffectPresetDeclaration, EnvelopeDeclaration, FrequencyLiteral, Identifier, InstrumentBody,
    MasterDeclaration, OctavesExpression, PanExpression, PatternBody, PatternDeclaration,
    PlaySource, PlayStatement, ProjectDeclaration, ProjectStatement, QuotedString, RateLiteral,
    RepeatCount, RepeatExpression, RhythmDeclaration, SampleChoiceAlternative,
    SampleSelectorExpression, SectionDeclaration, SectionTrack, SequenceItem, SongDeclaration,
    SongStatement, SourceFile, SpeedExpression, StepItem, TrackBody, TrackDeclaration, TrackEffect,
    TransposeExpression, VolumeExpression,
};

use crate::expand::Repetition;
use crate::hir::{
    Arrangement, Chance, ChanceTransform, Channels, Chord, ChordNote, DegreeChoice, DelayEffect,
    Duration, Effect, Envelope, FilterAutomation, FilterEffect, InstrumentKind, Key, LfoWaveform,
    MasterLimiter, Meter, Mode, NodeId, Note, Pan, Pattern, PatternOccurrence, PatternStep,
    PitchClass, Program, Project, Repeat, Rest, ReverbEffect, Rhythm, RhythmItem, SampleChoice,
    SampleRange, SampleSelector, SampleTrigger, Section, SectionOccurrence, Song, Speed,
    TrackDefinition, WeightedNote, WeightedSampleSequence,
};

pub mod expand;
pub mod hir;
mod schedule;

pub use schedule::{ScheduleError, schedule};

const DEFAULT_VELOCITY: u8 = 127;

fn rhythm_cell_count(duration: Duration, resolution: Duration) -> Option<u64> {
    let dividend = u64::from(duration.numerator) * u64::from(resolution.denominator);
    let divisor = u64::from(duration.denominator) * u64::from(resolution.numerator);
    if divisor == 0 || dividend % divisor != 0 {
        None
    } else {
        Some(dividend / divisor)
    }
}

/// Splits one grid cell between the `cells` items of a `[ ... ]`
/// subdivision. Prefers dividing the numerator so nested subdivisions do not
/// grow the denominator faster than they have to; `None` when the split
/// cannot be represented (an empty subdivision, or a denominator past
/// `u32`).
fn divide_duration(duration: Duration, cells: usize) -> Option<Duration> {
    let cells = u32::try_from(cells).ok().filter(|cells| *cells > 0)?;
    if duration.numerator.is_multiple_of(cells) {
        return Some(Duration {
            numerator: duration.numerator / cells,
            denominator: duration.denominator,
        });
    }
    Some(Duration {
        numerator: duration.numerator,
        denominator: duration.denominator.checked_mul(cells)?,
    })
}

/// Transposes every pitched event in a pattern step, reporting `false`
/// when any of them would leave the MIDI range.
fn transpose_step(step: &mut PatternStep, semitones: i32) -> bool {
    let pitches: Vec<&mut u8> = match step {
        PatternStep::Note(note) => vec![&mut note.midi_pitch],
        PatternStep::Chord(chord) => chord
            .notes
            .iter_mut()
            .map(|note| &mut note.midi_pitch)
            .collect(),
        PatternStep::DegreeChoice(choice) => choice
            .alternatives
            .iter_mut()
            .map(|alternative| &mut alternative.note.midi_pitch)
            .collect(),
        PatternStep::Sample(_) | PatternStep::Choice(_) | PatternStep::Rest(_) => Vec::new(),
    };
    for pitch in pitches {
        let Some(transposed) = transposed_pitch(*pitch, semitones) else {
            return false;
        };
        *pitch = transposed;
    }
    true
}

/// Semitone offsets above the root for each chord quality a `root:quality`
/// symbol may name.
///
/// Deliberately a closed table rather than a parsed formula: the point of
/// the symbol form is that its expansion is predictable, and a quality
/// nobody has agreed the spelling of is better rejected than guessed.
fn chord_intervals(quality: &str) -> Option<&'static [i32]> {
    Some(match quality {
        "maj" => &[0, 4, 7],
        "m" | "min" => &[0, 3, 7],
        "dim" => &[0, 3, 6],
        "aug" => &[0, 4, 8],
        "sus2" => &[0, 2, 7],
        "sus4" => &[0, 5, 7],
        "6" => &[0, 4, 7, 9],
        "m6" => &[0, 3, 7, 9],
        "add9" => &[0, 4, 7, 14],
        "7" => &[0, 4, 7, 10],
        "maj7" => &[0, 4, 7, 11],
        "m7" => &[0, 3, 7, 10],
        "mmaj7" => &[0, 3, 7, 11],
        "m7b5" => &[0, 3, 6, 10],
        "dim7" => &[0, 3, 6, 9],
        "9" => &[0, 4, 7, 10, 14],
        "maj9" => &[0, 4, 7, 11, 14],
        "m9" => &[0, 3, 7, 10, 14],
        _ => return None,
    })
}

/// What a section needs from the song around it to resolve its track
/// references and per-reference overrides.
struct SectionContext<'a, 'b> {
    track_defs: &'a [&'b TrackDeclaration],
    effect_presets: &'a [&'b EffectPresetDeclaration],
    patterns: &'a [Pattern],
    meter: Option<&'a Meter>,
}

/// A zero-length duration, the identity for [`add_durations`].
const ZERO_DURATION: Duration = Duration {
    numerator: 0,
    denominator: 1,
};

/// One pattern step's length, or `None` for a `choose` whose alternatives
/// are sequences of different lengths — that step's duration is not known
/// until the roll happens during scheduling.
fn step_duration(step: &PatternStep) -> Option<Duration> {
    match step {
        PatternStep::Note(note) => Some(note.duration),
        PatternStep::Chord(chord) => Some(chord.duration),
        PatternStep::Rest(rest) => Some(rest.duration),
        PatternStep::Sample(sample) => Some(sample.duration),
        PatternStep::DegreeChoice(choice) => {
            choice.alternatives.first().map(|first| first.note.duration)
        }
        PatternStep::Choice(choice) => {
            let mut lengths = choice.alternatives.iter().map(|alternative| {
                alternative
                    .samples
                    .iter()
                    .try_fold(ZERO_DURATION, |total, sample| {
                        add_durations(total, sample.duration)
                    })
            });
            let first = lengths.next()??;
            lengths.all(|length| length == Some(first)).then_some(first)
        }
    }
}

/// A pattern's total length, or `None` when any step's length depends on a
/// weighted roll.
fn pattern_duration(pattern: &Pattern) -> Option<Duration> {
    pattern.steps.iter().try_fold(ZERO_DURATION, |total, step| {
        add_durations(total, step_duration(step)?)
    })
}

/// Adds two whole-note fractions, reduced so repeated addition stays inside
/// `u32`.
fn add_durations(left: Duration, right: Duration) -> Option<Duration> {
    if left.denominator == 0 || right.denominator == 0 {
        return None;
    }
    let denominator = u64::from(left.denominator) * u64::from(right.denominator);
    let numerator = u64::from(left.numerator) * u64::from(right.denominator)
        + u64::from(right.numerator) * u64::from(left.denominator);
    let divisor = gcd(numerator, denominator).max(1);
    Some(Duration {
        numerator: u32::try_from(numerator / divisor).ok()?,
        denominator: u32::try_from(denominator / divisor).ok()?,
    })
}

fn gcd(left: u64, right: u64) -> u64 {
    let (mut left, mut right) = (left, right);
    while right != 0 {
        let next = left % right;
        left = right;
        right = next;
    }
    left
}

/// Whether a section's `play track` reference carries any override at all.
fn overrides_anything(reference: &SectionTrack) -> bool {
    reference.volume.is_some() || reference.effect.is_some() || reference.automate.is_some()
}

/// The parts of `arpeggiate <source> { ... }`, bundled so lowering takes a
/// spec rather than five loose parameters.
struct ArpeggioSpec<'a> {
    source: &'a Identifier,
    style: &'a Identifier,
    step: &'a DurationExpression,
    octaves: Option<&'a OctavesExpression>,
    span: SourceSpan,
}

/// The order an [`ArpeggioStyle`] walks a chord's tones in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArpeggioStyle {
    Up,
    Down,
    UpDown,
    DownUp,
    AsWritten,
}

impl ArpeggioStyle {
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "up" => Self::Up,
            "down" => Self::Down,
            "up_down" => Self::UpDown,
            "down_up" => Self::DownUp,
            "as_written" => Self::AsWritten,
            _ => return None,
        })
    }

    /// Pool indices for `count` notes, wrapping at `cap` tones when the
    /// arpeggio was given an `octaves` limit.
    ///
    /// `up_down` climbs to `ceil((count + 2) / 2)` and turns around without
    /// repeating the top or bottom note, which is the shape a hand-written
    /// up-down arpeggio has; `down_up` is that sequence reflected.
    fn indices(self, count: usize, cap: Option<usize>) -> Vec<usize> {
        let raw: Vec<usize> = match self {
            Self::Up | Self::AsWritten => (0..count).collect(),
            Self::Down => (0..count).rev().collect(),
            Self::UpDown | Self::DownUp => {
                let turn = (count + 2).div_ceil(2);
                let mut indices = Vec::with_capacity(count);
                for index in 0..turn.min(count) {
                    indices.push(index);
                }
                let mut index = turn.saturating_sub(2);
                while indices.len() < count {
                    indices.push(index);
                    index = index.saturating_sub(1);
                }
                if self == Self::DownUp {
                    let top = turn.saturating_sub(1);
                    indices = indices.into_iter().map(|index| top - index).collect();
                }
                indices
            }
        };
        match cap {
            Some(cap) if cap > 0 => raw.into_iter().map(|index| index % cap).collect(),
            _ => raw,
        }
    }
}

/// The `index`th tone of a chord's pool: its tones in order, then the same
/// tones an octave up, and so on.
fn pool_pitch(tones: &[u8], index: usize) -> Option<u8> {
    let tone = *tones.get(index % tones.len())?;
    let octave = i32::try_from(index / tones.len()).ok()?;
    transposed_pitch(tone, octave.checked_mul(12)?)
}

fn transposed_pitch(midi_pitch: u8, semitones: i32) -> Option<u8> {
    i32::from(midi_pitch)
        .checked_add(semitones)
        .and_then(|pitch| u8::try_from(pitch).ok())
        .filter(|pitch| *pitch <= 127)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileDiagnostic {
    pub message: String,
    pub span: SourceSpan,
}

/// Validates a parsed source file and lowers it to HIR.
///
/// All semantic errors found in one pass are returned together.
///
/// # Errors
///
/// Returns diagnostics when required declarations are missing, names are
/// duplicated, or musical values and units are invalid.
pub fn compile(file: &SourceFile) -> Result<Program, Vec<CompileDiagnostic>> {
    Compiler::default().compile(file)
}

#[derive(Default)]
struct Compiler {
    diagnostics: Vec<CompileDiagnostic>,
    next_id: u32,
}

#[derive(Default)]
struct SongSettings {
    tempo_bpm: Option<f64>,
    meter: Option<Meter>,
    key: Option<Key>,
    tempo_seen: bool,
    meter_seen: bool,
    key_seen: bool,
}

/// The result of classifying a song's statements into their declaration
/// kinds (one bucket per `SongStatement` variant), with duplicate names
/// already rejected via `declare_name`. Kept as its own struct so `fn song`
/// stays under clippy's line-count lint instead of inlining the whole
/// classification loop.
struct SongStatements<'a> {
    settings: SongSettings,
    effect_presets: Vec<&'a EffectPresetDeclaration>,
    pattern_declarations: Vec<&'a PatternDeclaration>,
    rhythm_defs: Vec<&'a RhythmDeclaration>,
    track_defs: Vec<&'a TrackDeclaration>,
    section_defs: Vec<&'a SectionDeclaration>,
    instruments: Vec<(&'a str, Option<InstrumentKind>)>,
    arrangement: Option<(&'a Vec<ArrangementEntry>, SourceSpan)>,
    master: Option<&'a MasterDeclaration>,
}

impl Compiler {
    fn compile(mut self, file: &SourceFile) -> Result<Program, Vec<CompileDiagnostic>> {
        let mut project = None;
        let mut project_seen = false;
        let mut songs = Vec::new();
        let mut song_names = HashSet::new();

        for declaration in &file.declarations {
            match declaration {
                Declaration::Project(declaration) => {
                    if project_seen {
                        self.error("project is declared more than once", declaration.span);
                    } else {
                        project_seen = true;
                        project = self.project(declaration);
                    }
                }
                Declaration::Song(declaration) => {
                    if !song_names.insert(declaration.name.value.as_str()) {
                        self.error(
                            "song name is declared more than once",
                            declaration.name.span,
                        );
                    } else if let Some(song) = self.song(declaration) {
                        songs.push(song);
                    }
                }
            }
        }

        if !project_seen {
            self.error("project declaration is required", file.span);
        }

        match (project, self.diagnostics.is_empty()) {
            (Some(project), true) => Ok(Program { project, songs }),
            _ => Err(self.diagnostics),
        }
    }

    fn project(&mut self, declaration: &ProjectDeclaration) -> Option<Project> {
        let mut seed = None;
        let mut sample_rate_hz = None;
        let mut channels = None;
        let mut sample_rate_seen = false;
        let mut output_seen = false;

        for statement in &declaration.statements {
            match statement {
                ProjectStatement::Seed { value, span } => {
                    if seed.replace(*value).is_some() {
                        self.error("seed is declared more than once", *span);
                    }
                }
                ProjectStatement::SampleRate { value, span } => {
                    if sample_rate_seen {
                        self.error("sample_rate is declared more than once", *span);
                    } else {
                        sample_rate_seen = true;
                        sample_rate_hz =
                            self.sample_rate(value.value.value, &value.unit.text, *span);
                    }
                }
                ProjectStatement::Output {
                    channels: value,
                    span,
                } => {
                    if output_seen {
                        self.error("output is declared more than once", *span);
                    } else {
                        output_seen = true;
                        channels = match value.text.as_str() {
                            "mono" => Some(Channels::Mono),
                            "stereo" => Some(Channels::Stereo),
                            _ => {
                                self.error("output must be `mono` or `stereo`", value.span);
                                None
                            }
                        };
                    }
                }
            }
        }

        if seed.is_none() {
            self.error("seed is required", declaration.span);
        }
        if !sample_rate_seen {
            self.error("sample_rate is required", declaration.span);
        }
        if !output_seen {
            self.error("output is required", declaration.span);
        }
        match (seed, sample_rate_hz, channels) {
            (Some(seed), Some(sample_rate_hz), Some(channels)) => Some(Project {
                seed,
                sample_rate_hz,
                channels,
            }),
            _ => None,
        }
    }

    fn collect_song_statements<'a>(
        &mut self,
        statements: &'a [SongStatement],
    ) -> SongStatements<'a> {
        let mut settings = SongSettings::default();
        let mut effect_presets = Vec::new();
        let mut effect_preset_names = HashSet::new();
        let mut pattern_declarations = Vec::new();
        let mut pattern_names = HashSet::new();
        let mut rhythm_defs = Vec::new();
        let mut rhythm_names = HashSet::new();
        let mut track_defs = Vec::new();
        let mut track_names = HashSet::new();
        let mut section_defs = Vec::new();
        let mut section_names = HashSet::new();
        let mut instruments = Vec::new();
        let mut instrument_names = HashSet::new();
        let mut arrangement = None;
        let mut master = None;

        for statement in statements {
            match statement {
                SongStatement::Tempo { .. }
                | SongStatement::Meter { .. }
                | SongStatement::Key { .. } => self.song_setting(statement, &mut settings),
                SongStatement::Instrument(instrument) => {
                    if self.declare_name(&mut instrument_names, &instrument.name, "instrument") {
                        instruments.push((
                            instrument.name.text.as_str(),
                            self.instrument_kind(&instrument.body),
                        ));
                    }
                }
                SongStatement::EffectPreset(preset) => {
                    if self.declare_name(&mut effect_preset_names, &preset.name, "effect preset") {
                        effect_presets.push(preset.as_ref());
                    }
                }
                SongStatement::Rhythm(rhythm) => {
                    if self.declare_name(&mut rhythm_names, &rhythm.name, "rhythm") {
                        rhythm_defs.push(rhythm);
                    }
                }
                SongStatement::Track(track) => {
                    if self.declare_name(&mut track_names, &track.name, "track") {
                        track_defs.push(track.as_ref());
                    }
                }
                SongStatement::Section(section) => {
                    if self.declare_name(&mut section_names, &section.name, "section") {
                        section_defs.push(section);
                    }
                }
                SongStatement::Arrangement { entries, span } => {
                    if arrangement.replace((entries, *span)).is_some() {
                        self.error("arrangement is declared more than once", *span);
                    }
                }
                SongStatement::Master(declaration) => {
                    if master.replace(declaration).is_some() {
                        self.error("master is declared more than once", declaration.span);
                    }
                }
                SongStatement::Pattern(pattern) => {
                    if self.declare_name(&mut pattern_names, &pattern.name, "pattern") {
                        pattern_declarations.push(pattern);
                    }
                }
            }
        }

        SongStatements {
            settings,
            effect_presets,
            pattern_declarations,
            rhythm_defs,
            track_defs,
            section_defs,
            instruments,
            arrangement,
            master,
        }
    }

    fn song(&mut self, declaration: &SongDeclaration) -> Option<Song> {
        let id = self.id();
        let SongStatements {
            settings,
            effect_presets,
            pattern_declarations,
            rhythm_defs,
            track_defs,
            section_defs,
            instruments,
            arrangement,
            master: master_decl,
        } = self.collect_song_statements(&declaration.statements);

        self.require_song_settings(&settings, declaration.span);
        let rhythms = rhythm_defs
            .iter()
            .filter_map(|rhythm| self.rhythm(rhythm))
            .collect::<Vec<_>>();
        // Lowered in declaration order, and each one can see the patterns
        // before it: that is what makes `pattern b = a |> ...` resolvable
        // without a second pass, and cyclic by construction impossible.
        let mut patterns: Vec<Pattern> = Vec::with_capacity(pattern_declarations.len());
        for declaration in &pattern_declarations {
            let pattern = self.pattern(
                declaration,
                settings.key.as_ref(),
                settings.meter.as_ref(),
                &patterns,
            );
            patterns.push(pattern);
        }
        let tracks = self.build_tracks(
            &track_defs,
            &mut patterns,
            &rhythms,
            &instruments,
            settings.meter.as_ref(),
            &effect_presets,
        );
        let mut tracks = tracks;
        let mut variants = Vec::new();
        let section_context = SectionContext {
            track_defs: &track_defs,
            effect_presets: &effect_presets,
            patterns: &patterns,
            meter: settings.meter.as_ref(),
        };
        // Sections resolve their overrides against the declared tracks only;
        // the variants they synthesize are appended afterwards so a later
        // section never matches an earlier one's copy by name.
        let sections = section_defs
            .iter()
            .filter_map(|section| self.section(section, &tracks, &mut variants, &section_context))
            .collect::<Vec<_>>();
        tracks.extend(variants);
        self.check_arrangement_track_combination(
            arrangement.as_ref(),
            !tracks.is_empty(),
            declaration.span,
        );
        let arrangement = arrangement.and_then(|(entries, span)| {
            self.arrangement(entries, span, &patterns, &instruments, &sections)
        });
        // Every `repeat fit` reachable from an arrangement was resolved into
        // a count while its section was lowered. One left over belongs to a
        // track no section plays, so there is nothing for it to fill.
        if !matches!(arrangement, Some(Arrangement::Sections(_))) {
            for declaration in &track_defs {
                if tracks
                    .iter()
                    .any(|track| track.name == declaration.name.text && track.repeat == Repeat::Fit)
                {
                    self.error(
                        "`repeat fit` needs the track to be played by a section",
                        declaration.span,
                    );
                }
            }
        }
        let master = master_decl.and_then(|declaration| self.master(declaration));
        match (settings.tempo_bpm, settings.meter, settings.key) {
            (Some(tempo_bpm), Some(meter), Some(key)) => Some(Song {
                id,
                name: declaration.name.value.clone(),
                tempo_bpm,
                meter,
                key,
                rhythms,
                patterns,
                tracks,
                sections,
                arrangement,
                master,
            }),
            _ => None,
        }
    }

    /// Lowers `master { limiter { ceiling C } }`: `ceiling` must carry a
    /// `db` unit (mirroring `fn track_gain`'s `volume` check), be finite,
    /// and be at most `0.0` dB — a limiter that permits amplification above
    /// 0 dBFS defeats its purpose, unlike track `volume` which legitimately
    /// allows positive dB boosts. Converts to linear amplitude via the same
    /// `10^(db / 20)` formula `track_gain` already uses.
    fn master(&mut self, declaration: &MasterDeclaration) -> Option<MasterLimiter> {
        let ceiling = &declaration.ceiling;
        if ceiling.unit.text != "db" {
            self.error("ceiling unit must be `db`", ceiling.unit.span);
            return None;
        }
        if !ceiling.decibels.is_finite() {
            self.error("ceiling must be finite", ceiling.span);
            return None;
        }
        if ceiling.decibels > 0.0 {
            self.error("ceiling must be at most 0db", ceiling.span);
            return None;
        }
        Some(MasterLimiter {
            ceiling: 10.0_f32.powf(ceiling.decibels / 20.0),
        })
    }

    fn require_song_settings(&mut self, settings: &SongSettings, span: SourceSpan) {
        if !settings.tempo_seen {
            self.error("tempo is required", span);
        }
        if !settings.meter_seen {
            self.error("meter is required", span);
        }
        if !settings.key_seen {
            self.error("key is required", span);
        }
    }

    /// Declared tracks and a *pattern* arrangement (the original,
    /// track-less form) are mutually exclusive; declared tracks and a
    /// *section* arrangement (`play <name>`) require each other, since a
    /// section's body always references declared tracks by name.
    fn check_arrangement_track_combination(
        &mut self,
        arrangement: Option<&(&Vec<ArrangementEntry>, SourceSpan)>,
        tracks_declared: bool,
        span: SourceSpan,
    ) {
        let Some((entries, _)) = arrangement else {
            return;
        };
        let is_play = entries
            .iter()
            .any(|entry| matches!(entry, ArrangementEntry::Play { .. }));
        if is_play && !tracks_declared {
            self.error("an arrangement of sections requires declared tracks", span);
        } else if !is_play && tracks_declared {
            self.error(
                "track declarations cannot be combined with a pattern arrangement",
                span,
            );
        }
    }

    /// Lowers `section <name> bars <N> { parallel [exact] { play track ... } }`.
    /// `bars` resolves to a whole-note `Duration` using the same formula as
    /// `N bar` note/chord/rest durations (`count * meter.numerator /
    /// meter.denominator`). Each `play track X` name is resolved against the
    /// already-lowered `tracks` list; a track declared as `layer { use ... }`
    /// contributes every one of its layers (all `TrackDefinition`s sharing
    /// that name), so referencing a layered track places every layer.
    fn section(
        &mut self,
        declaration: &SectionDeclaration,
        tracks: &[TrackDefinition],
        variants: &mut Vec<TrackDefinition>,
        context: &SectionContext<'_, '_>,
    ) -> Option<Section> {
        let meter = context.meter;
        let bars = meter.and_then(|meter| {
            let Some(numerator) = declaration.bars.checked_mul(meter.numerator) else {
                self.error("section bars is out of range", declaration.span);
                return None;
            };
            self.duration(
                numerator,
                meter.denominator,
                declaration.span,
                "section bars",
            )
        });
        let mut any_missing = false;
        let mut track_ids = Vec::new();
        if declaration.tracks.is_empty() {
            self.error(
                "section must reference at least one track",
                declaration.span,
            );
            any_missing = true;
        }
        for reference in &declaration.tracks {
            let matches = tracks
                .iter()
                .filter(|track| track.name == reference.name.text)
                .cloned()
                .collect::<Vec<_>>();
            if matches.is_empty() {
                self.error("section references an unknown track", reference.name.span);
                any_missing = true;
                continue;
            }
            let needs_variant = overrides_anything(reference)
                || matches.iter().any(|track| track.repeat == Repeat::Fit);
            // A failed override has already been reported; falling back to
            // the declaration's own tracks keeps the section itself intact,
            // so the arrangement referencing it is not reported as broken
            // too.
            let overridden = if needs_variant {
                bars.and_then(|bars| self.overridden_tracks(reference, &matches, context, bars))
            } else {
                None
            };
            match overridden {
                Some(overridden) => {
                    track_ids.extend(overridden.iter().map(|track| track.id));
                    variants.extend(overridden);
                }
                None => track_ids.extend(matches.iter().map(|track| track.id)),
            }
        }
        if any_missing {
            return None;
        }
        Some(Section {
            id: self.id(),
            name: declaration.name.text.clone(),
            bars: bars?,
            exact: declaration.exact,
            tracks: track_ids,
        })
    }

    /// Builds the per-section variants of an overridden track: a copy of
    /// each matching track definition (a layered track lowers to one per
    /// layer) with this section's volume, effect, and automation in place of
    /// the declaration's.
    ///
    /// A variant is a separate track with its own id, exactly as a
    /// hand-written duplicate declaration would be. That matters for
    /// `chance`, whose rolls are seeded from the track identity — the same
    /// as today, where a track played in two sections already rolls
    /// differently in each.
    fn overridden_tracks(
        &mut self,
        reference: &SectionTrack,
        matches: &[TrackDefinition],
        context: &SectionContext<'_, '_>,
        bars: Duration,
    ) -> Option<Vec<TrackDefinition>> {
        let declaration = context
            .track_defs
            .iter()
            .find(|track| track.name.text == reference.name.text)?;
        let scale = self.volume_scale(reference.volume.as_ref(), declaration)?;
        let effect = if reference.effect.is_some() || reference.automate.is_some() {
            let effect_declaration =
                self.resolve_effect(reference.effect.as_ref(), context.effect_presets)?;
            Some(
                self.effect(
                    effect_declaration,
                    reference.automate.as_ref(),
                    context.meter,
                )
                .ok()?,
            )
        } else {
            None
        };
        let mut variants = Vec::with_capacity(matches.len());
        for track in matches {
            let repeat = match track.repeat {
                Repeat::Fixed(count) => Repeat::Fixed(count),
                Repeat::Fit => Repeat::Fixed(self.fit_count(track, bars, context, reference)?),
            };
            variants.push(TrackDefinition {
                id: self.id(),
                gain: track.gain * scale,
                effect: effect.unwrap_or(track.effect),
                repeat,
                ..track.clone()
            });
        }
        Some(variants)
    }

    /// Resolves `repeat fit` for one section: how many times the track's
    /// pattern fits into the section's length.
    fn fit_count(
        &mut self,
        track: &TrackDefinition,
        bars: Duration,
        context: &SectionContext<'_, '_>,
        reference: &SectionTrack,
    ) -> Option<u16> {
        let pattern = context
            .patterns
            .iter()
            .find(|pattern| pattern.id == track.pattern)?;
        let Some(length) = pattern_duration(pattern) else {
            self.error(
                "`repeat fit` needs a pattern whose length is fixed",
                reference.span,
            );
            return None;
        };
        let count = rhythm_cell_count(bars, length).and_then(|count| u16::try_from(count).ok());
        match count {
            Some(count) if count > 0 => Some(count),
            _ => {
                self.error(
                    "`repeat fit` needs the pattern to divide the section's length evenly",
                    reference.span,
                );
                None
            }
        }
    }

    /// How much a section's `volume` override changes a track's gain: the
    /// new volume divided by the declaration's, so the play pipeline's own
    /// `gain` stage stays multiplied in.
    fn volume_scale(
        &mut self,
        volume: Option<&VolumeExpression>,
        declaration: &TrackDeclaration,
    ) -> Option<f32> {
        let Some(volume) = volume else {
            return Some(1.0);
        };
        let overridden = self.volume_amplitude(volume)?;
        let declared = match declaration.volume.as_deref() {
            Some(declared) => self.volume_amplitude(declared)?,
            None => 1.0,
        };
        Some(overridden / declared)
    }

    fn volume_amplitude(&mut self, volume: &VolumeExpression) -> Option<f32> {
        if volume.unit.text != "db" {
            self.error("volume unit must be `db`", volume.unit.span);
            return None;
        }
        if !volume.decibels.is_finite() {
            self.error("volume must be finite", volume.span);
            return None;
        }
        Some(10.0_f32.powf(volume.decibels / 20.0))
    }

    /// Resolves a track's `effect` to the block it names, following a
    /// song-level preset reference.
    #[expect(
        clippy::option_option,
        reason = "the outer None is a resolution failure, the inner one an absent effect"
    )]
    fn resolve_effect<'a>(
        &mut self,
        effect: Option<&'a TrackEffect>,
        effect_presets: &[&'a EffectPresetDeclaration],
    ) -> Option<Option<&'a EffectDeclaration>> {
        match effect {
            None => Some(None),
            Some(TrackEffect::Inline(effect)) => Some(Some(effect)),
            Some(TrackEffect::Preset(name)) => {
                let Some(found) = effect_presets
                    .iter()
                    .find(|preset| preset.name.text == name.text)
                else {
                    self.error("track references an unknown effect preset", name.span);
                    return None;
                };
                Some(Some(&found.effect))
            }
        }
    }

    fn build_tracks(
        &mut self,
        track_defs: &[&TrackDeclaration],
        patterns: &mut Vec<Pattern>,
        rhythms: &[Rhythm],
        instruments: &[(&str, Option<InstrumentKind>)],
        meter: Option<&Meter>,
        effect_presets: &[&EffectPresetDeclaration],
    ) -> Vec<TrackDefinition> {
        let mut tracks = Vec::with_capacity(track_defs.len());
        for track in track_defs {
            for (definition, synthesized) in
                self.track(track, patterns, rhythms, instruments, meter, effect_presets)
            {
                if let Some(pattern) = synthesized {
                    patterns.push(pattern);
                }
                tracks.push(definition);
            }
        }
        tracks
    }

    fn song_setting(&mut self, statement: &SongStatement, settings: &mut SongSettings) {
        match statement {
            SongStatement::Tempo { value, span } => {
                if settings.tempo_seen {
                    self.error("tempo is declared more than once", *span);
                } else {
                    settings.tempo_seen = true;
                    settings.tempo_bpm = self.tempo(value.value.value, &value.unit.text, *span);
                }
            }
            SongStatement::Meter {
                numerator,
                denominator,
                span,
            } => {
                if settings.meter_seen {
                    self.error("meter is declared more than once", *span);
                } else {
                    settings.meter_seen = true;
                    settings.meter = self.meter(*numerator, *denominator, *span);
                }
            }
            SongStatement::Key { tonic, mode, span } => {
                if settings.key_seen {
                    self.error("key is declared more than once", *span);
                } else {
                    settings.key_seen = true;
                    settings.key = self.key(&tonic.text, &mode.text, *span);
                }
            }
            _ => unreachable!("called only for song settings"),
        }
    }

    fn declare_name<'a>(
        &mut self,
        names: &mut HashSet<&'a str>,
        name: &'a Identifier,
        declaration: &str,
    ) -> bool {
        if names.insert(&name.text) {
            true
        } else {
            self.error(
                &format!("{declaration} name is declared more than once"),
                name.span,
            );
            false
        }
    }

    fn rhythm(&mut self, declaration: &RhythmDeclaration) -> Option<Rhythm> {
        let resolution = self.duration(
            declaration.resolution_numerator,
            declaration.resolution_denominator,
            declaration.span,
            "rhythm resolution",
        )?;
        let items = self.expanded(expand::rhythm_items(&declaration.items), declaration.span)?;
        Some(Rhythm {
            id: self.id(),
            name: declaration.name.text.clone(),
            resolution,
            items: items
                .into_iter()
                .map(|expanded| match expanded.item {
                    symphra_syntax::ast::RhythmItem::Hit { .. } => RhythmItem::Hit,
                    symphra_syntax::ast::RhythmItem::Rest { .. } => RhythmItem::Rest,
                    symphra_syntax::ast::RhythmItem::Repeat(_) => {
                        unreachable!("repetitions are expanded before lowering")
                    }
                })
                .collect(),
        })
    }

    /// Reports the one way expansion can fail — a body whose `* N`
    /// repetitions multiply out past [`expand::MAX_EXPANDED_ITEMS`] — and
    /// otherwise hands back the expanded items.
    fn expanded<T>(&mut self, expanded: Option<Vec<T>>, span: SourceSpan) -> Option<Vec<T>> {
        if expanded.is_none() {
            self.error(
                &format!(
                    "repetitions expand to more than {} items",
                    expand::MAX_EXPANDED_ITEMS
                ),
                span,
            );
        }
        expanded
    }

    /// Lowers a track declaration into one `TrackDefinition` per layer: the
    /// single `instrument`/`play` form lowers to exactly one, while a
    /// `layer { use ... }` form lowers each `use` independently (its own
    /// instrument, pattern, and full pipeline), sharing only the track's
    /// `name`, `role`, and `volume`. The score/render pipeline never
    /// distinguishes the two forms: every layer becomes an ordinary track
    /// that gets mixed together, which is what "mixed into one logical
    /// track" means in practice.
    fn track(
        &mut self,
        declaration: &TrackDeclaration,
        patterns: &[Pattern],
        rhythms: &[Rhythm],
        instruments: &[(&str, Option<InstrumentKind>)],
        meter: Option<&Meter>,
        effect_presets: &[&EffectPresetDeclaration],
    ) -> Vec<(TrackDefinition, Option<Pattern>)> {
        // Resolved once per declaration (not per layer) so an invalid effect
        // is reported once, not once per `use`.
        let effect_declaration = self
            .resolve_effect(declaration.effect.as_ref(), effect_presets)
            .flatten();
        let effect = self
            .effect(effect_declaration, declaration.automate.as_ref(), meter)
            .ok()
            .flatten();
        match &declaration.body {
            TrackBody::Single { instrument, play } => self
                .track_layer(
                    declaration,
                    instrument,
                    play,
                    patterns,
                    rhythms,
                    instruments,
                    meter,
                    effect,
                )
                .into_iter()
                .collect(),
            TrackBody::Layers { uses, .. } => uses
                .iter()
                .filter_map(|layer_use| {
                    self.track_layer(
                        declaration,
                        &layer_use.instrument,
                        &layer_use.play,
                        patterns,
                        rhythms,
                        instruments,
                        meter,
                        effect,
                    )
                })
                .collect(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn track_layer(
        &mut self,
        declaration: &TrackDeclaration,
        instrument_ref: &Identifier,
        play: &PlayStatement,
        patterns: &[Pattern],
        rhythms: &[Rhythm],
        instruments: &[(&str, Option<InstrumentKind>)],
        meter: Option<&Meter>,
        effect: Option<Effect>,
    ) -> Option<(TrackDefinition, Option<Pattern>)> {
        let instrument = instruments
            .iter()
            .find(|(name, _)| *name == instrument_ref.text)
            .and_then(|(_, instrument)| instrument.clone());
        if instrument.is_none() {
            self.error(
                "track references an unknown instrument",
                instrument_ref.span,
            );
        }
        let (pattern, synthesized, trigger_with) =
            self.play_source(declaration, play, patterns, rhythms, instrument.as_ref());
        let gate_percent = match play.gate {
            Some(gate) => match u8::try_from(gate.percent) {
                Ok(percent) if percent <= 100 => Some(Some(percent)),
                _ => {
                    self.error("gate must be from 0% to 100%", gate.span);
                    None
                }
            },
            None => Some(None),
        };
        let transpose_semitones = match play.transpose.as_ref() {
            Some(transpose) if transpose.unit.text == "st" => Some(Some(transpose.semitones)),
            Some(transpose) => {
                self.error("transpose unit must be `st`", transpose.unit.span);
                None
            }
            None => Some(None),
        };
        let gain = self.track_gain(declaration, play);
        let repeat_count = self.repeat_count(play);
        let pan = self.pan(play);
        let chance = self
            .chance(play, pattern.as_ref(), instrument.as_ref())
            .ok();
        let speed = self.speed(play, instrument.as_ref());
        let choose_sample = self.choose_sample(play, instrument.as_ref()).ok();
        let at = self.at_offset(play, meter).ok();
        if let (Some(pattern), Some(Some(semitones)), Some(transpose)) = (
            pattern.as_ref(),
            transpose_semitones,
            play.transpose.as_ref(),
        ) {
            self.validate_transpose(pattern, semitones, transpose.span);
        }
        pattern
            .zip(instrument)
            .zip(gate_percent)
            .zip(transpose_semitones)
            .zip(gain)
            .zip(repeat_count)
            .zip(pan.zip(chance).zip(speed).zip(choose_sample).zip(at))
            .map(
                |(
                    (
                        ((((pattern, instrument), gate_percent), transpose_semitones), gain),
                        repeat_count,
                    ),
                    ((((pan, chance), speed), choose_sample), at),
                )| {
                    let definition = TrackDefinition {
                        id: self.id(),
                        name: declaration.name.text.clone(),
                        role: declaration.role.text.clone(),
                        instrument,
                        pattern: pattern.id,
                        trigger_with,
                        gate_percent,
                        transpose_semitones,
                        gain,
                        repeat: repeat_count,
                        reverse: play.reverse,
                        pan,
                        chance,
                        speed,
                        choose_sample,
                        at,
                        effect,
                    };
                    (definition, synthesized)
                },
            )
    }

    fn at_offset(
        &mut self,
        play: &PlayStatement,
        meter: Option<&Meter>,
    ) -> Result<Option<Duration>, ()> {
        let Some(expression) = play.at else {
            return Ok(None);
        };
        if expression.bar == 0 || expression.beat == 0 {
            self.error(
                "`at` bar and beat are 1-indexed and must be at least 1",
                expression.span,
            );
            return Err(());
        }
        let Some(meter) = meter else {
            return Err(());
        };
        if expression.beat > meter.numerator {
            self.error(
                "`at` beat must not exceed the song's meter numerator",
                expression.span,
            );
            return Err(());
        }
        let numerator = u64::from(expression.bar - 1) * u64::from(meter.numerator)
            + u64::from(expression.beat - 1);
        Ok(Some(Duration {
            numerator: u32::try_from(numerator).map_err(|_| {
                self.error("`at` position is out of range", expression.span);
            })?,
            denominator: meter.denominator,
        }))
    }

    /// Resolves a track's `play` source into the pattern it should schedule.
    ///
    /// `PlaySource::Pattern` looks up a declared pattern by name, exactly as
    /// before. `PlaySource::Drum` is sugar: it synthesizes a fresh pattern
    /// with one step per rhythm item (a named drum trigger for `hit`, a rest
    /// otherwise) so the rest of the pipeline never needs to know the
    /// difference. The synthesized pattern is returned separately so the
    /// caller can register it in the song's pattern list.
    fn play_source(
        &mut self,
        declaration: &TrackDeclaration,
        play: &PlayStatement,
        patterns: &[Pattern],
        rhythms: &[Rhythm],
        instrument: Option<&InstrumentKind>,
    ) -> (Option<Pattern>, Option<Pattern>, Option<NodeId>) {
        match &play.source {
            PlaySource::Pattern(identifier) => {
                let pattern = patterns
                    .iter()
                    .find(|pattern| pattern.name == identifier.text)
                    .cloned();
                if pattern.is_none() {
                    self.error("track references an unknown pattern", identifier.span);
                }
                let rhythm = self.resolve_trigger_with(play, pattern.as_ref(), rhythms);
                (pattern, None, rhythm.map(|rhythm| rhythm.id))
            }
            PlaySource::Drum { name, rhythm, span } => {
                let pattern = self.drum_play_pattern(
                    declaration,
                    play,
                    name,
                    rhythm,
                    *span,
                    rhythms,
                    instrument,
                );
                match pattern {
                    Some(pattern) => (Some(pattern.clone()), Some(pattern), None),
                    None => (None, None, None),
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn drum_play_pattern(
        &mut self,
        declaration: &TrackDeclaration,
        play: &PlayStatement,
        name: &QuotedString,
        rhythm: &Identifier,
        span: SourceSpan,
        rhythms: &[Rhythm],
        instrument: Option<&InstrumentKind>,
    ) -> Option<Pattern> {
        if play.trigger_with.is_some() {
            self.error(
                "`trigger_with` cannot be combined with `play drum ... with ...`",
                span,
            );
            return None;
        }
        if name.value.is_empty() {
            self.error("drum voice name must not be empty", name.span);
            return None;
        }
        if instrument
            .is_some_and(|instrument| !matches!(instrument, InstrumentKind::DrumMachine { .. }))
        {
            self.error("play drum requires a drum machine instrument", span);
            return None;
        }
        let Some(found_rhythm) = rhythms
            .iter()
            .find(|candidate| candidate.name == rhythm.text)
        else {
            self.error("play drum with references an unknown rhythm", rhythm.span);
            return None;
        };
        if found_rhythm.items.is_empty() {
            self.error("play drum with rhythm must contain at least one item", span);
            return None;
        }
        let steps = found_rhythm
            .items
            .iter()
            .map(|item| match item {
                RhythmItem::Hit => PatternStep::Sample(SampleTrigger {
                    id: self.id(),
                    selector: SampleSelector::Named(name.value.clone()),
                    duration: found_rhythm.resolution,
                    velocity: DEFAULT_VELOCITY,
                }),
                RhythmItem::Rest => PatternStep::Rest(Rest {
                    id: self.id(),
                    duration: found_rhythm.resolution,
                }),
            })
            .collect();
        Some(Pattern {
            id: self.id(),
            name: format!("{}::drum", declaration.name.text),
            steps,
        })
    }

    fn resolve_trigger_with<'a>(
        &mut self,
        play: &PlayStatement,
        pattern: Option<&Pattern>,
        rhythms: &'a [Rhythm],
    ) -> Option<&'a Rhythm> {
        let rhythm = play.trigger_with.as_ref().and_then(|reference| {
            let rhythm = rhythms.iter().find(|rhythm| rhythm.name == reference.text);
            if rhythm.is_none() {
                self.error("trigger_with references an unknown rhythm", reference.span);
            }
            rhythm
        });
        if let (Some(pattern), Some(rhythm), Some(reference)) =
            (pattern, rhythm, play.trigger_with.as_ref())
        {
            self.validate_trigger(pattern, rhythm, reference.span);
        }
        rhythm
    }

    fn choose_sample(
        &mut self,
        play: &PlayStatement,
        instrument: Option<&InstrumentKind>,
    ) -> Result<Option<SampleRange>, ()> {
        let Some(expression) = play.choose_sample else {
            return Ok(None);
        };
        if expression.start > expression.end {
            self.error("choose_sample range must not be empty", expression.span);
            return Err(());
        }
        if instrument
            .is_some_and(|instrument| !matches!(instrument, InstrumentKind::Sampler { .. }))
        {
            self.error(
                "choose_sample is only supported for sampler tracks",
                expression.span,
            );
            return Err(());
        }
        Ok(Some(SampleRange {
            start: expression.start,
            end: expression.end,
        }))
    }

    fn speed(
        &mut self,
        play: &PlayStatement,
        instrument: Option<&InstrumentKind>,
    ) -> Option<Speed> {
        let Some(speed) = play.speed else {
            return Some(Speed::Fixed(1.0));
        };
        let span = speed.span();
        let (speed, factors) = match speed {
            SpeedExpression::Fixed { factor, .. } => (Speed::Fixed(factor), [factor, factor]),
            SpeedExpression::Alternate {
                first_factor,
                second_factor,
                ..
            } => (
                Speed::Alternate {
                    first: first_factor,
                    second: second_factor,
                },
                [first_factor, second_factor],
            ),
        };
        if factors
            .iter()
            .any(|factor| !factor.is_finite() || *factor <= 0.0)
        {
            self.error("speed must be finite and greater than zero", span);
            return None;
        }
        if instrument.is_some_and(|instrument| {
            !matches!(
                instrument,
                InstrumentKind::Sampler { .. } | InstrumentKind::DrumMachine { .. }
            )
        }) {
            self.error(
                "speed is only supported for sampler or drum machine tracks",
                span,
            );
            return None;
        }
        Some(speed)
    }

    fn repeat_count(&mut self, play: &PlayStatement) -> Option<Repeat> {
        match play.repeat {
            Some(repeat) => match repeat.count {
                RepeatCount::Fit => Some(Repeat::Fit),
                RepeatCount::Fixed(count) => match u16::try_from(count) {
                    Ok(count) if count > 0 => Some(Repeat::Fixed(count)),
                    _ => {
                        self.error("repeat must be from 1 to 65535", repeat.span);
                        None
                    }
                },
            },
            None => Some(Repeat::Fixed(1)),
        }
    }

    fn pan(&mut self, play: &PlayStatement) -> Option<Pan> {
        match play.pan {
            Some(PanExpression::Fixed { percent, span }) => match i8::try_from(percent) {
                Ok(percent) if (-100..=100).contains(&percent) => Some(Pan::Fixed(percent)),
                _ => {
                    self.error("pan must be from -100% to 100%", span);
                    None
                }
            },
            Some(PanExpression::Alternate {
                left_percent,
                right_percent,
                span,
            }) => {
                let (Ok(left_percent), Ok(right_percent)) =
                    (i8::try_from(left_percent), i8::try_from(right_percent))
                else {
                    self.error("alternate pan values must be from 0% to 100%", span);
                    return None;
                };
                if left_percent > 100 || right_percent > 100 {
                    self.error("alternate pan values must be from 0% to 100%", span);
                    return None;
                }
                Some(Pan::Alternate {
                    left_percent,
                    right_percent,
                })
            }
            None => Some(Pan::Fixed(0)),
        }
    }

    fn chance(
        &mut self,
        play: &PlayStatement,
        pattern: Option<&Pattern>,
        instrument: Option<&InstrumentKind>,
    ) -> Result<Option<Chance>, SourceSpan> {
        let Some(expression) = play.chance.as_ref() else {
            return Ok(None);
        };
        let Ok(percent) = u8::try_from(expression.percent) else {
            self.error("chance must be from 0% to 100%", expression.span);
            return Err(expression.span);
        };
        if percent > 100 {
            self.error("chance must be from 0% to 100%", expression.span);
            return Err(expression.span);
        }
        let sampler_only = |compiler: &mut Self, span: SourceSpan, what: &str| {
            if instrument.is_some_and(|instrument| {
                !matches!(
                    instrument,
                    InstrumentKind::Sampler { .. } | InstrumentKind::DrumMachine { .. }
                )
            }) {
                compiler.error(
                    &format!("chance {what} is only supported for sampler or drum machine tracks"),
                    span,
                );
                return Err(span);
            }
            Ok(())
        };
        let transform = match &expression.transform {
            ChanceTransformExpression::Transpose(transpose) => {
                if transpose.unit.text != "st" {
                    self.error("chance transpose unit must be `st`", transpose.unit.span);
                    return Err(transpose.unit.span);
                }
                ChanceTransform::Transpose(transpose.semitones)
            }
            ChanceTransformExpression::Retrigger { count, span } => {
                sampler_only(self, *span, "retrigger")?;
                if *count < 2 {
                    self.error("chance retrigger count must be at least 2", *span);
                    return Err(*span);
                }
                ChanceTransform::Retrigger(*count)
            }
            ChanceTransformExpression::Speed { factor, span } => {
                sampler_only(self, *span, "speed")?;
                if !factor.is_finite() || *factor <= 0.0 {
                    self.error("chance speed must be finite and greater than zero", *span);
                    return Err(*span);
                }
                ChanceTransform::Speed(*factor)
            }
        };
        let chance = Chance { percent, transform };
        if let (ChanceTransform::Transpose(semitones), Some(pattern)) = (chance.transform, pattern)
        {
            self.validate_transpose(pattern, semitones, expression.span);
        }
        Ok(Some(chance))
    }

    /// Resolves a track's `effect delay { ... }`, `effect filter { ... }`, or
    /// `effect reverb { ... }`, together with an optional `automate cutoff {
    /// ... }` (which only combines with `effect filter`). Delay `feedback`
    /// is capped at `0.95` (not the theoretical stability limit of `1.0`) so
    /// a delay's echo tail — which the renderer must extend the song's audio
    /// buffer to fit — always decays to silence within a bounded number of
    /// repeats. Filter `cutoff` (and `automate`'s `range`) is only checked
    /// for being a positive, finite frequency here; checking it against the
    /// Nyquist limit needs the project's sample rate, which is not in scope
    /// during song/track compilation, so the renderer clamps it defensively
    /// at render time instead (mirroring the existing defensive-revalidation
    /// precedent for a hand-constructed [`MasterLimiter`]). Reverb `size`
    /// needs no such cross-namespace check — it is a plain `0.0..=1.0`
    /// factor, not a frequency or duration.
    fn effect(
        &mut self,
        effect_declaration: Option<&EffectDeclaration>,
        automate_declaration: Option<&AutomateDeclaration>,
        meter: Option<&Meter>,
    ) -> Result<Option<Effect>, ()> {
        let automation = self.filter_automation(automate_declaration)?;
        let Some(declaration) = effect_declaration else {
            if automation.is_some() {
                self.automate_requires_filter_error(automate_declaration, "no `effect` block");
                return Err(());
            }
            return Ok(None);
        };
        match &declaration.kind {
            EffectKind::Delay {
                mix,
                time,
                feedback,
            } => {
                if automation.is_some() {
                    self.automate_requires_filter_error(automate_declaration, "`effect delay`");
                    return Err(());
                }
                if !mix.value.is_finite() || !(0.0..=1.0).contains(&mix.value) {
                    self.error("effect mix must be from 0.0 to 1.0", mix.span);
                    return Err(());
                }
                if !feedback.value.is_finite() || !(0.0..=0.95).contains(&feedback.value) {
                    self.error("effect feedback must be from 0.0 to 0.95", feedback.span);
                    return Err(());
                }
                let time = self
                    .item_duration(time, meter, declaration.span, "effect delay")
                    .ok_or(())?;
                Ok(Some(Effect::Delay(DelayEffect {
                    mix: mix.value,
                    time,
                    feedback: feedback.value,
                })))
            }
            EffectKind::Filter { cutoff, resonance } => {
                let cutoff_hz = self.frequency_hz(cutoff, "effect filter cutoff")?;
                if !resonance.value.is_finite() || !(0.0..=1.0).contains(&resonance.value) {
                    self.error(
                        "effect filter resonance must be from 0.0 to 1.0",
                        resonance.span,
                    );
                    return Err(());
                }
                Ok(Some(Effect::Filter(FilterEffect {
                    cutoff_hz,
                    resonance: resonance.value,
                    automation,
                })))
            }
            EffectKind::Reverb { mix, size } => {
                if automation.is_some() {
                    self.automate_requires_filter_error(automate_declaration, "`effect reverb`");
                    return Err(());
                }
                if !mix.value.is_finite() || !(0.0..=1.0).contains(&mix.value) {
                    self.error("effect mix must be from 0.0 to 1.0", mix.span);
                    return Err(());
                }
                if !size.value.is_finite() || !(0.0..=1.0).contains(&size.value) {
                    self.error("effect reverb size must be from 0.0 to 1.0", size.span);
                    return Err(());
                }
                Ok(Some(Effect::Reverb(ReverbEffect {
                    mix: mix.value,
                    size: size.value,
                })))
            }
        }
    }

    fn automate_requires_filter_error(
        &mut self,
        automate_declaration: Option<&AutomateDeclaration>,
        found: &str,
    ) {
        let span = automate_declaration
            .expect("automate_requires_filter_error is only called when automation is Some")
            .span;
        self.error(
            &format!("automate cutoff requires `effect filter` on the same track, found {found}"),
            span,
        );
    }

    /// Resolves a positive, finite `hz`/`khz` [`FrequencyLiteral`] to plain
    /// hertz. Shared by `effect filter { cutoff ... }` and `automate cutoff
    /// { lfo { range ... } }`, which both accept the same literal grammar;
    /// `context` (such as `"effect filter cutoff"`) names the field being
    /// resolved in error messages.
    fn frequency_hz(&mut self, literal: &FrequencyLiteral, context: &str) -> Result<f32, ()> {
        let multiplier = match literal.unit.text.as_str() {
            "hz" => 1.0,
            "khz" => 1_000.0,
            _ => {
                self.error(
                    &format!("{context} unit must be `hz` or `khz`"),
                    literal.span,
                );
                return Err(());
            }
        };
        let hz = literal.value.value * multiplier;
        if !hz.is_finite() || hz <= 0.0 {
            self.error(&format!("{context} must be greater than 0hz"), literal.span);
            return Err(());
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "hz is validated finite and positive before conversion"
        )]
        let hz = hz as f32;
        Ok(hz)
    }

    /// Resolves a track's `automate cutoff { lfo <waveform> { range A..B
    /// rate N cycles/bar } }`. `waveform` is validated the same way
    /// `instrument x = sine` is (plain identifier text, not a keyword
    /// token). `rate`'s `N cycles/bar` is kept tempo/meter-agnostic here —
    /// resolving it to an LFO frequency in Hz needs the song's tempo, which
    /// (like [`DelayEffect`]'s `time`) is not available until render time.
    fn filter_automation(
        &mut self,
        declaration: Option<&AutomateDeclaration>,
    ) -> Result<Option<FilterAutomation>, ()> {
        let Some(declaration) = declaration else {
            return Ok(None);
        };
        let lfo = &declaration.lfo;
        let waveform = match lfo.waveform.text.as_str() {
            "sine" => LfoWaveform::Sine,
            "triangle" => LfoWaveform::Triangle,
            _ => {
                self.error(
                    "lfo waveform must be `sine` or `triangle`",
                    lfo.waveform.span,
                );
                return Err(());
            }
        };
        let range_start_hz = self.frequency_hz(&lfo.range_start, "automate range")?;
        let range_end_hz = self.frequency_hz(&lfo.range_end, "automate range")?;
        if !lfo.rate.value.is_finite() || lfo.rate.value <= 0.0 {
            self.error(
                "automate rate must be greater than zero cycles per bar",
                lfo.rate.span,
            );
            return Err(());
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "lfo.rate.value is validated finite and positive before conversion"
        )]
        let cycles_per_bar = lfo.rate.value as f32;
        Ok(Some(FilterAutomation {
            waveform,
            range_start_hz,
            range_end_hz,
            cycles_per_bar,
        }))
    }

    fn track_gain(&mut self, declaration: &TrackDeclaration, play: &PlayStatement) -> Option<f32> {
        let pipeline = match play.gain {
            Some(gain) if gain.factor.is_finite() && gain.factor >= 0.0 => gain.factor,
            Some(gain) => {
                self.error(
                    "gain must be finite and greater than or equal to zero",
                    gain.span,
                );
                return None;
            }
            None => 1.0,
        };
        let volume = match declaration.volume.as_deref() {
            Some(volume) if volume.unit.text != "db" => {
                self.error("volume unit must be `db`", volume.unit.span);
                return None;
            }
            Some(volume) if volume.decibels.is_finite() => 10.0_f32.powf(volume.decibels / 20.0),
            Some(volume) => {
                self.error("volume must be finite", volume.span);
                return None;
            }
            None => 1.0,
        };
        let combined = pipeline * volume;
        if combined.is_finite() {
            Some(combined)
        } else {
            self.error(
                "combined track gain is outside the supported range",
                play.span,
            );
            None
        }
    }

    fn validate_transpose(&mut self, pattern: &Pattern, semitones: i32, span: SourceSpan) {
        for step in &pattern.steps {
            let valid = match step {
                PatternStep::Note(note) => transposed_pitch(note.midi_pitch, semitones).is_some(),
                PatternStep::Chord(chord) => chord
                    .notes
                    .iter()
                    .all(|note| transposed_pitch(note.midi_pitch, semitones).is_some()),
                PatternStep::DegreeChoice(choice) => {
                    choice.alternatives.iter().all(|alternative| {
                        transposed_pitch(alternative.note.midi_pitch, semitones).is_some()
                    })
                }
                PatternStep::Rest(_) => true,
                PatternStep::Sample(_) | PatternStep::Choice(_) => {
                    self.error("transpose supports only pitched patterns", span);
                    return;
                }
            };
            if !valid {
                self.error(
                    "transposed pitch must be within the MIDI range 0 to 127",
                    span,
                );
                return;
            }
        }
    }

    fn validate_trigger(&mut self, pattern: &Pattern, rhythm: &Rhythm, span: SourceSpan) {
        if rhythm.items.is_empty() {
            self.error("trigger_with rhythm must contain at least one item", span);
            return;
        }
        for step in &pattern.steps {
            let duration = match step {
                PatternStep::Note(note) => note.duration,
                PatternStep::Chord(chord) => chord.duration,
                PatternStep::Rest(rest) => rest.duration,
                PatternStep::Sample(sample) => sample.duration,
                // Every alternative in a compiled DegreeChoice shares the same
                // step-cell duration, so any one of them stands in for the
                // step's duration.
                PatternStep::DegreeChoice(choice) => match choice.alternatives.first() {
                    Some(alternative) => alternative.note.duration,
                    None => continue,
                },
                // A Choice alternative's duration only stays fixed when it
                // selects exactly one sample; a multi-sample sequence's
                // duration depends on which alternative gets picked, which
                // isn't known until the weighted selection runs during
                // scheduling.
                PatternStep::Choice(choice) => {
                    if choice
                        .alternatives
                        .iter()
                        .any(|alternative| alternative.samples.len() != 1)
                    {
                        self.error(
                            "trigger_with requires every choose alternative to select exactly one sample",
                            span,
                        );
                        return;
                    }
                    match choice
                        .alternatives
                        .first()
                        .and_then(|alternative| alternative.samples.first())
                    {
                        Some(sample) => sample.duration,
                        None => continue,
                    }
                }
            };
            if rhythm_cell_count(duration, rhythm.resolution).is_none() {
                self.error(
                    "pattern step duration must be divisible by rhythm resolution",
                    span,
                );
                return;
            }
        }
    }

    fn arrangement(
        &mut self,
        entries: &[ArrangementEntry],
        span: SourceSpan,
        patterns: &[Pattern],
        instruments: &[(&str, Option<InstrumentKind>)],
        sections: &[Section],
    ) -> Option<Arrangement> {
        if entries.is_empty() {
            self.error("arrangement must contain at least one entry", span);
            return None;
        }
        let has_play = entries
            .iter()
            .any(|entry| matches!(entry, ArrangementEntry::Play { .. }));
        let has_pattern = entries
            .iter()
            .any(|entry| matches!(entry, ArrangementEntry::Pattern(_)));
        if has_play && has_pattern {
            self.error(
                "arrangement cannot mix `play <section>` entries with pattern entries",
                span,
            );
            return None;
        }
        if has_play {
            let occurrences = entries
                .iter()
                .filter_map(|entry| {
                    let ArrangementEntry::Play { name, .. } = entry else {
                        unreachable!("checked above: every entry is Play")
                    };
                    let section = sections.iter().find(|section| section.name == name.text);
                    if section.is_none() {
                        self.error("arrangement references an unknown section", name.span);
                    }
                    section.map(|section| SectionOccurrence {
                        id: self.id(),
                        section: section.id,
                    })
                })
                .collect();
            return Some(Arrangement::Sections(occurrences));
        }
        let occurrences = entries
            .iter()
            .filter_map(|entry| {
                let ArrangementEntry::Pattern(reference) = entry else {
                    unreachable!("checked above: every entry is Pattern")
                };
                let pattern = patterns
                    .iter()
                    .find(|pattern| pattern.name == reference.pattern.text);
                if pattern.is_none() {
                    self.error(
                        "arrangement references an unknown pattern",
                        reference.pattern.span,
                    );
                }
                let instrument = reference.instrument.as_ref().map_or(
                    Some(InstrumentKind::Sine { envelope: None }),
                    |reference| {
                        let instrument =
                            instruments.iter().find(|(name, _)| *name == reference.text);
                        if instrument.is_none() {
                            self.error(
                                "arrangement references an unknown instrument",
                                reference.span,
                            );
                        }
                        instrument.and_then(|(_, kind)| kind.clone())
                    },
                );
                pattern
                    .zip(instrument)
                    .map(|(pattern, instrument)| PatternOccurrence {
                        id: self.id(),
                        pattern: pattern.id,
                        instrument,
                    })
            })
            .collect();
        Some(Arrangement::Patterns(occurrences))
    }

    fn instrument_kind(&mut self, body: &InstrumentBody) -> Option<InstrumentKind> {
        match body {
            InstrumentBody::Oscillator {
                waveform, envelope, ..
            } => {
                let envelope = match envelope {
                    None => None,
                    Some(declaration) => Some(self.envelope(declaration)?),
                };
                match waveform.text.as_str() {
                    "sine" => Some(InstrumentKind::Sine { envelope }),
                    "triangle" => Some(InstrumentKind::Triangle { envelope }),
                    _ => {
                        self.error(
                            "instrument kind must be `sine`, `triangle`, `sampled`, `sampler`, or `drum_machine`",
                            waveform.span,
                        );
                        None
                    }
                }
            }
            InstrumentBody::Supersaw {
                voices,
                voices_span,
                detune,
                spread,
                envelope,
                ..
            } => {
                if *voices == 0 {
                    self.error("supersaw voices must be at least 1", *voices_span);
                    return None;
                }
                if !detune.value.is_finite() || !(0.0..=1.0).contains(&detune.value) {
                    self.error("supersaw detune must be from 0.0 to 1.0", detune.span);
                    return None;
                }
                if !spread.value.is_finite() || !(0.0..=1.0).contains(&spread.value) {
                    self.error("supersaw spread must be from 0.0 to 1.0", spread.span);
                    return None;
                }
                let envelope = match envelope {
                    None => None,
                    Some(declaration) => Some(self.envelope(declaration)?),
                };
                Some(InstrumentKind::Supersaw {
                    voices: *voices,
                    detune: detune.value,
                    spread: spread.value,
                    envelope,
                })
            }
            InstrumentBody::Sampled { source, root, .. } => {
                if source.value.is_empty() {
                    self.error("sample source path must not be empty", source.span);
                    None
                } else {
                    self.pitch(&root.text, root.span)
                        .map(|root_midi| InstrumentKind::Sampled {
                            source: source.value.clone(),
                            root_midi,
                        })
                }
            }
            InstrumentBody::Sampler { pack, .. } => {
                if pack.value.is_empty() {
                    self.error("sample pack name must not be empty", pack.span);
                    None
                } else {
                    Some(InstrumentKind::Sampler {
                        pack: pack.value.clone(),
                    })
                }
            }
            InstrumentBody::DrumMachine { bank, .. } => {
                if bank.value.is_empty() {
                    self.error("drum bank name must not be empty", bank.span);
                    None
                } else {
                    Some(InstrumentKind::DrumMachine {
                        bank: bank.value.clone(),
                    })
                }
            }
            InstrumentBody::SoundFont { source, preset, .. } => {
                self.soundfont_instrument_kind(source, preset)
            }
            InstrumentBody::Vst3 { source, preset, .. } => {
                self.vst3_instrument_kind(source, preset.as_ref())
            }
        }
    }

    /// `source`/`preset` must both be non-empty. Split out of
    /// [`Self::instrument_kind`] to stay under clippy's `too_many_lines`
    /// threshold — a mechanical extraction, not a behavior change.
    fn soundfont_instrument_kind(
        &mut self,
        source: &QuotedString,
        preset: &QuotedString,
    ) -> Option<InstrumentKind> {
        if source.value.is_empty() {
            self.error("soundfont source path must not be empty", source.span);
            return None;
        }
        if preset.value.is_empty() {
            self.error("soundfont preset name must not be empty", preset.span);
            return None;
        }
        Some(InstrumentKind::SoundFont {
            source: source.value.clone(),
            preset: preset.value.clone(),
        })
    }

    /// `source` must be non-empty; `preset`, if present, must also be
    /// non-empty. Split out of [`Self::instrument_kind`] to stay under
    /// clippy's `too_many_lines` threshold — a mechanical extraction, not a
    /// behavior change.
    fn vst3_instrument_kind(
        &mut self,
        source: &QuotedString,
        preset: Option<&QuotedString>,
    ) -> Option<InstrumentKind> {
        if source.value.is_empty() {
            self.error("vst3 source path must not be empty", source.span);
            return None;
        }
        if let Some(preset) = preset
            && preset.value.is_empty()
        {
            self.error("vst3 preset name must not be empty", preset.span);
            return None;
        }
        Some(InstrumentKind::Vst3 {
            source: source.value.clone(),
            preset: preset.map(|preset| preset.value.clone()),
        })
    }

    /// `attack`/`decay`/`release` must carry an `ms` unit and be finite,
    /// non-negative durations; `sustain` must be finite and in `0.0..=1.0`.
    fn envelope(&mut self, declaration: &EnvelopeDeclaration) -> Option<Envelope> {
        let attack_ms = self.envelope_ms(&declaration.attack, "envelope attack")?;
        let decay_ms = self.envelope_ms(&declaration.decay, "envelope decay")?;
        if !declaration.sustain.value.is_finite()
            || !(0.0..=1.0).contains(&declaration.sustain.value)
        {
            self.error(
                "envelope sustain must be from 0.0 to 1.0",
                declaration.sustain.span,
            );
            return None;
        }
        let release_ms = self.envelope_ms(&declaration.release, "envelope release")?;
        Some(Envelope {
            attack_ms,
            decay_ms,
            sustain: declaration.sustain.value,
            release_ms,
        })
    }

    /// Resolves a finite, non-negative `ms` [`RateLiteral`] to plain
    /// milliseconds. `context` (such as `"envelope attack"`) names the field
    /// being resolved in error messages, the same convention
    /// [`Compiler::frequency_hz`] uses for `hz`/`khz`.
    fn envelope_ms(&mut self, literal: &RateLiteral, context: &str) -> Option<f32> {
        if literal.unit.text != "ms" {
            self.error(&format!("{context} unit must be `ms`"), literal.span);
            return None;
        }
        let ms = literal.value.value;
        if !ms.is_finite() || ms < 0.0 {
            self.error(
                &format!("{context} must be finite and greater than or equal to 0ms"),
                literal.span,
            );
            return None;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "ms is validated finite and non-negative before conversion"
        )]
        let ms = ms as f32;
        Some(ms)
    }

    fn pattern(
        &mut self,
        declaration: &PatternDeclaration,
        key: Option<&Key>,
        meter: Option<&Meter>,
        declared: &[Pattern],
    ) -> Pattern {
        let id = self.id();
        let steps = match &declaration.body {
            PatternBody::Sequence { step, items, span } => {
                self.sequence_steps(items, step.as_ref(), *span, meter)
            }
            PatternBody::Steps {
                resolution,
                items,
                span,
            } => self.steps(resolution, items, *span, key, meter),
            PatternBody::Arpeggiate {
                source,
                style,
                step,
                octaves,
                span,
            } => self.arpeggiated_steps(
                &ArpeggioSpec {
                    source,
                    style,
                    step,
                    octaves: octaves.as_ref(),
                    span: *span,
                },
                meter,
                declared,
            ),
            PatternBody::Derived {
                source,
                transpose,
                repeat,
                reverse,
                span,
            } => self.derived_steps(
                source,
                transpose.as_ref(),
                repeat.as_ref(),
                *reverse,
                *span,
                declared,
            ),
        };
        Pattern {
            id,
            name: declaration.name.text.clone(),
            steps,
        }
    }

    fn sequence_steps(
        &mut self,
        items: &[SequenceItem],
        step: Option<&DurationExpression>,
        span: SourceSpan,
        meter: Option<&Meter>,
    ) -> Vec<PatternStep> {
        let Some(items) = self.expanded(expand::sequence_items(items), span) else {
            return Vec::new();
        };
        items
            .into_iter()
            .filter_map(|expanded| match expanded.item {
                SequenceItem::Repeat(_) => {
                    unreachable!("repetitions are expanded before lowering")
                }
                SequenceItem::Note(note) => {
                    let midi_pitch = self.pitch(&note.pitch.text, note.pitch.span);
                    let duration = self.sequence_duration(
                        note.duration.as_ref(),
                        step,
                        meter,
                        note.span,
                        "note",
                    );
                    let velocity = self.velocity(note.velocity.as_ref(), expanded.position);
                    let (Some(midi_pitch), Some(duration), Some(velocity)) =
                        (midi_pitch, duration, velocity)
                    else {
                        return None;
                    };
                    Some(PatternStep::Note(Note {
                        id: self.id(),
                        midi_pitch,
                        duration,
                        velocity,
                    }))
                }
                SequenceItem::Chord(chord) => {
                    let midi_pitches = self.chord_pitches(&chord.pitches);
                    let duration = self.sequence_duration(
                        chord.duration.as_ref(),
                        step,
                        meter,
                        chord.span,
                        "chord",
                    );
                    let velocity = self.velocity(chord.velocity.as_ref(), expanded.position);
                    let (Some(midi_pitches), Some(duration), Some(velocity)) =
                        (midi_pitches, duration, velocity)
                    else {
                        return None;
                    };
                    Some(PatternStep::Chord(Chord {
                        notes: midi_pitches
                            .into_iter()
                            .map(|midi_pitch| ChordNote {
                                id: self.id(),
                                midi_pitch,
                            })
                            .collect(),
                        duration,
                        velocity,
                    }))
                }
                SequenceItem::Rest(rest) => self
                    .sequence_duration(rest.duration.as_ref(), step, meter, rest.span, "rest")
                    .map(|duration| {
                        PatternStep::Rest(Rest {
                            id: self.id(),
                            duration,
                        })
                    }),
            })
            .collect()
    }

    fn steps(
        &mut self,
        resolution: &DurationExpression,
        items: &[StepItem],
        span: SourceSpan,
        key: Option<&Key>,
        meter: Option<&Meter>,
    ) -> Vec<PatternStep> {
        let Some(duration) = self.item_duration(resolution, meter, span, "step") else {
            return Vec::new();
        };
        // Counted once for the whole body, subdivisions included, so a
        // nested blow-up is rejected before any of it is built.
        if expand::step_event_count(items).is_none() {
            self.error(
                &format!(
                    "repetitions expand to more than {} items",
                    expand::MAX_EXPANDED_ITEMS
                ),
                span,
            );
            return Vec::new();
        }
        self.step_events(items, duration, span, key)
    }

    /// Lowers `pattern <name> = arpeggiate <source> { ... }`: every chord in
    /// the source pattern becomes a run of single notes on a `step` grid,
    /// while notes and rests pass through unchanged.
    fn arpeggiated_steps(
        &mut self,
        spec: &ArpeggioSpec<'_>,
        meter: Option<&Meter>,
        declared: &[Pattern],
    ) -> Vec<PatternStep> {
        let ArpeggioSpec {
            source,
            style,
            step,
            octaves,
            span,
        } = *spec;
        let Some(found) = declared.iter().find(|pattern| pattern.name == source.text) else {
            self.error(
                "arpeggiate references a pattern that is not declared above it",
                source.span,
            );
            return Vec::new();
        };
        let Some(style) = ArpeggioStyle::parse(&style.text) else {
            self.error(
                "arpeggio style must be `up`, `down`, `up_down`, `down_up`, or `as_written`",
                style.span,
            );
            return Vec::new();
        };
        let Some(step) = self.item_duration(step, meter, span, "step") else {
            return Vec::new();
        };
        let octaves = match octaves {
            Some(octaves) if octaves.count == 0 => {
                self.error("octaves must be at least 1", octaves.span);
                return Vec::new();
            }
            Some(octaves) => Some(octaves.count as usize),
            None => None,
        };

        let source_steps = found.steps.clone();
        let mut steps = Vec::with_capacity(source_steps.len());
        for source_step in source_steps {
            match source_step {
                PatternStep::Chord(chord) => {
                    let Some(count) = rhythm_cell_count(chord.duration, step) else {
                        self.error(
                            "arpeggiate step must divide every chord's duration evenly",
                            span,
                        );
                        return Vec::new();
                    };
                    let Ok(count) = usize::try_from(count) else {
                        self.error("arpeggiate produces too many notes", span);
                        return Vec::new();
                    };
                    let mut tones = chord
                        .notes
                        .iter()
                        .map(|note| note.midi_pitch)
                        .collect::<Vec<_>>();
                    if style != ArpeggioStyle::AsWritten {
                        tones.sort_unstable();
                    }
                    for index in style.indices(count, octaves.map(|octaves| octaves * tones.len()))
                    {
                        let Some(pitch) = pool_pitch(&tones, index) else {
                            self.error("arpeggio reaches past the MIDI range 0 to 127", span);
                            return Vec::new();
                        };
                        steps.push(PatternStep::Note(Note {
                            id: self.id(),
                            midi_pitch: pitch,
                            duration: step,
                            velocity: chord.velocity,
                        }));
                    }
                }
                PatternStep::Sample(_) | PatternStep::Choice(_) => {
                    self.error("arpeggiate needs a pitched pattern", source.span);
                    return Vec::new();
                }
                other => steps.push(self.renumbered(other)),
            }
            if steps.len() > expand::MAX_EXPANDED_ITEMS {
                self.error("arpeggiate produces too many notes", span);
                return Vec::new();
            }
        }
        steps
    }

    /// Lowers `pattern <name> = <source> |> ...`: the source pattern's
    /// steps, copied with fresh node ids (so seeded `choose` rolls stay
    /// distinct between the two patterns), then transformed.
    ///
    /// Stage order matches the play pipeline's: transpose, then repeat,
    /// then reverse.
    fn derived_steps(
        &mut self,
        source: &Identifier,
        transpose: Option<&TransposeExpression>,
        repeat: Option<&RepeatExpression>,
        reverse: bool,
        span: SourceSpan,
        declared: &[Pattern],
    ) -> Vec<PatternStep> {
        let Some(found) = declared.iter().find(|pattern| pattern.name == source.text) else {
            self.error(
                "pattern derivation references a pattern that is not declared above it",
                source.span,
            );
            return Vec::new();
        };
        let mut steps = found
            .steps
            .clone()
            .into_iter()
            .map(|step| self.renumbered(step))
            .collect::<Vec<_>>();

        if let Some(transpose) = transpose {
            for step in &mut steps {
                if !transpose_step(step, transpose.semitones) {
                    self.error(
                        "transposed pitch must be within the MIDI range 0 to 127",
                        transpose.span,
                    );
                    return Vec::new();
                }
            }
        }
        if let Some(repeat) = repeat {
            let RepeatCount::Fixed(count) = repeat.count else {
                unreachable!("the parser rejects `repeat fit` on a pattern derivation")
            };
            if count == 0 {
                self.error("repeat count must be greater than zero", repeat.span);
                return Vec::new();
            }
            let once = steps.clone();
            for _ in 1..count {
                let copy = once
                    .iter()
                    .map(|step| self.renumbered(step.clone()))
                    .collect::<Vec<_>>();
                steps.extend(copy);
            }
            if steps.len() > expand::MAX_EXPANDED_ITEMS {
                self.error(
                    &format!(
                        "repetitions expand to more than {} items",
                        expand::MAX_EXPANDED_ITEMS
                    ),
                    span,
                );
                return Vec::new();
            }
        }
        if reverse {
            steps.reverse();
        }
        steps
    }

    /// Gives a copied step (and everything inside it) fresh node ids.
    fn renumbered(&mut self, step: PatternStep) -> PatternStep {
        match step {
            PatternStep::Note(note) => PatternStep::Note(Note {
                id: self.id(),
                ..note
            }),
            PatternStep::Chord(chord) => PatternStep::Chord(Chord {
                notes: chord
                    .notes
                    .into_iter()
                    .map(|note| ChordNote {
                        id: self.id(),
                        ..note
                    })
                    .collect(),
                ..chord
            }),
            PatternStep::Sample(sample) => PatternStep::Sample(SampleTrigger {
                id: self.id(),
                ..sample
            }),
            PatternStep::Rest(rest) => PatternStep::Rest(Rest {
                id: self.id(),
                ..rest
            }),
            PatternStep::Choice(choice) => PatternStep::Choice(SampleChoice {
                id: self.id(),
                alternatives: choice
                    .alternatives
                    .into_iter()
                    .map(|alternative| WeightedSampleSequence {
                        samples: alternative
                            .samples
                            .into_iter()
                            .map(|sample| SampleTrigger {
                                id: self.id(),
                                ..sample
                            })
                            .collect(),
                        ..alternative
                    })
                    .collect(),
            }),
            PatternStep::DegreeChoice(choice) => PatternStep::DegreeChoice(DegreeChoice {
                id: self.id(),
                alternatives: choice
                    .alternatives
                    .into_iter()
                    .map(|alternative| WeightedNote {
                        note: Note {
                            id: self.id(),
                            ..alternative.note
                        },
                        ..alternative
                    })
                    .collect(),
            }),
        }
    }

    /// Lowers one level of a `steps` body at `duration` per cell, recursing
    /// into `[ ... ]` subdivisions with the cell duration split between
    /// their items.
    fn step_events(
        &mut self,
        items: &[StepItem],
        duration: Duration,
        span: SourceSpan,
        key: Option<&Key>,
    ) -> Vec<PatternStep> {
        let Some(items) = self.expanded(expand::step_items(items), span) else {
            return Vec::new();
        };
        let mut steps = Vec::with_capacity(items.len());
        for expanded in items {
            if let StepItem::Subdivide {
                items: nested,
                span: nested_span,
            } = expanded.item
            {
                let cells = expand::step_items(nested).map_or(0, |cells| cells.len());
                let Some(divided) = divide_duration(duration, cells) else {
                    self.error("subdivision splits a step too finely", *nested_span);
                    continue;
                };
                steps.extend(self.step_events(nested, divided, *nested_span, key));
                continue;
            }
            let step = match expanded.item {
                StepItem::Repeat(_) | StepItem::Subdivide { .. } => {
                    unreachable!("repetitions are expanded, and subdivisions handled above")
                }
                StepItem::Degree {
                    degree,
                    octave,
                    span,
                } => key
                    .and_then(|key| self.degree_pitch(key.tonic, *degree, *octave, *span))
                    .map(|midi_pitch| {
                        PatternStep::Note(Note {
                            id: self.id(),
                            midi_pitch,
                            duration,
                            velocity: DEFAULT_VELOCITY,
                        })
                    }),
                StepItem::Sample {
                    index, velocity, ..
                } => self
                    .velocity(velocity.as_ref(), expanded.position)
                    .map(|velocity| {
                        PatternStep::Sample(SampleTrigger {
                            id: self.id(),
                            selector: SampleSelector::Index(*index),
                            duration,
                            velocity,
                        })
                    }),
                StepItem::Drum {
                    name,
                    velocity,
                    span,
                } => {
                    if name.value.is_empty() {
                        self.error("drum voice name must not be empty", *span);
                        None
                    } else {
                        self.velocity(velocity.as_ref(), expanded.position)
                            .map(|velocity| {
                                PatternStep::Sample(SampleTrigger {
                                    id: self.id(),
                                    selector: SampleSelector::Named(name.value.clone()),
                                    duration,
                                    velocity,
                                })
                            })
                    }
                }
                StepItem::Rest { .. } => Some(PatternStep::Rest(Rest {
                    id: self.id(),
                    duration,
                })),
                StepItem::Choose { alternatives, span } => {
                    if alternatives.is_empty() {
                        self.error("choose must contain at least one sample", *span);
                    }
                    Some(PatternStep::Choice(SampleChoice {
                        id: self.id(),
                        alternatives: self.sample_choice_alternatives(alternatives, duration),
                    }))
                }
                StepItem::ChooseDegrees { alternatives, span } => Some(PatternStep::DegreeChoice(
                    self.degree_choice(alternatives, *span, duration, key),
                )),
            };
            steps.extend(step);
        }
        steps
    }

    fn sample_choice_alternatives(
        &mut self,
        alternatives: &[SampleChoiceAlternative],
        duration: Duration,
    ) -> Vec<WeightedSampleSequence> {
        alternatives
            .iter()
            .filter_map(|alternative| {
                if alternative.selectors.is_empty() {
                    self.error(
                        "choice sequence must contain at least one sample",
                        alternative.span,
                    );
                    return None;
                }
                if alternative.weight == 0 {
                    self.error("choice weight must be greater than zero", alternative.span);
                    return None;
                }
                if alternative.selectors.iter().any(|selector| {
                    matches!(
                        selector,
                        SampleSelectorExpression::Named(name) if name.value.is_empty()
                    )
                }) {
                    self.error("drum voice name must not be empty", alternative.span);
                    return None;
                }
                Some(WeightedSampleSequence {
                    samples: alternative
                        .selectors
                        .iter()
                        .map(|selector| SampleTrigger {
                            id: self.id(),
                            selector: match selector {
                                SampleSelectorExpression::Index(index) => {
                                    SampleSelector::Index(*index)
                                }
                                SampleSelectorExpression::Named(name) => {
                                    SampleSelector::Named(name.value.clone())
                                }
                            },
                            duration,
                            velocity: DEFAULT_VELOCITY,
                        })
                        .collect(),
                    weight: alternative.weight,
                })
            })
            .collect()
    }

    fn degree_choice(
        &mut self,
        alternatives: &[DegreeChoiceAlternative],
        span: SourceSpan,
        duration: Duration,
        key: Option<&Key>,
    ) -> DegreeChoice {
        if alternatives.is_empty() {
            self.error("choose must contain at least one degree", span);
        }
        let alternatives = alternatives
            .iter()
            .filter_map(|alternative| {
                if alternative.weight == 0 {
                    self.error("choice weight must be greater than zero", alternative.span);
                    return None;
                }
                key.and_then(|key| {
                    self.degree_pitch(
                        key.tonic,
                        alternative.degree,
                        alternative.octave,
                        alternative.span,
                    )
                })
                .map(|midi_pitch| WeightedNote {
                    note: Note {
                        id: self.id(),
                        midi_pitch,
                        duration,
                        velocity: DEFAULT_VELOCITY,
                    },
                    weight: alternative.weight,
                })
            })
            .collect();
        DegreeChoice {
            id: self.id(),
            alternatives,
        }
    }

    fn degree_pitch(
        &mut self,
        tonic: PitchClass,
        degree: u32,
        octave: u32,
        span: SourceSpan,
    ) -> Option<u8> {
        let tonic = match tonic {
            PitchClass::C => 0,
            PitchClass::D => 2,
            PitchClass::E => 4,
            PitchClass::F => 5,
            PitchClass::G => 7,
            PitchClass::A => 9,
            PitchClass::B => 11,
        };
        let midi = octave
            .checked_add(1)
            .and_then(|octave| octave.checked_mul(12))
            .and_then(|midi| midi.checked_add(tonic))
            .and_then(|midi| midi.checked_add(degree));
        if let Some(midi) = midi
            .and_then(|midi| u8::try_from(midi).ok())
            .filter(|midi| *midi <= 127)
        {
            Some(midi)
        } else {
            self.error(
                "degree and octave must resolve to a MIDI pitch from 0 to 127",
                span,
            );
            None
        }
    }

    fn duration(
        &mut self,
        numerator: u32,
        denominator: u32,
        span: SourceSpan,
        item: &str,
    ) -> Option<Duration> {
        if numerator == 0 || denominator == 0 {
            self.error(&format!("{item} duration must be greater than zero"), span);
            None
        } else {
            Some(Duration {
                numerator,
                denominator,
            })
        }
    }

    /// Resolves a note/chord/rest duration expression, converting `N bar`
    /// into a whole-note fraction using the song's meter: `N` bars is
    /// `N * meter.numerator` beats, each `1 / meter.denominator` of a whole
    /// note. A missing meter is already reported by `song` as "meter is
    /// required", so this returns `None` silently rather than duplicating
    /// that diagnostic (mirroring `at_offset`).
    fn item_duration(
        &mut self,
        expr: &DurationExpression,
        meter: Option<&Meter>,
        span: SourceSpan,
        item: &str,
    ) -> Option<Duration> {
        let (numerator, denominator) = match *expr {
            DurationExpression::Fraction {
                numerator,
                denominator,
                ..
            } => (numerator, denominator),
            DurationExpression::Bars { count, span } => {
                let meter = meter?;
                let Some(numerator) = count.checked_mul(meter.numerator) else {
                    self.error(&format!("{item} duration is out of range"), span);
                    return None;
                };
                (numerator, meter.denominator)
            }
        };
        self.duration(numerator, denominator, span, item)
    }

    /// Resolves a chord's notes: the pitches as written, or the pitches a
    /// `root:quality` symbol names.
    fn chord_pitches(&mut self, pitches: &ChordPitches) -> Option<Vec<u8>> {
        match pitches {
            ChordPitches::Explicit(pitches) => pitches
                .iter()
                .map(|pitch| self.pitch(&pitch.text, pitch.span))
                .collect(),
            ChordPitches::Symbol { root, quality } => {
                let root_pitch = self.pitch(&root.text, root.span)?;
                let Some(intervals) = chord_intervals(&quality.text) else {
                    self.error("unknown chord quality", quality.span);
                    return None;
                };
                intervals
                    .iter()
                    .map(|interval| {
                        let pitch = transposed_pitch(root_pitch, *interval);
                        if pitch.is_none() {
                            self.error(
                                "chord reaches past the MIDI range 0 to 127",
                                root.span.cover(quality.span),
                            );
                        }
                        pitch
                    })
                    .collect()
            }
        }
    }

    /// Resolves a sequence item's duration: its own `for <duration>`, or
    /// the `sequence step <duration>` default when it omitted one. An item
    /// with neither is rejected — the parser normally catches this, but a
    /// recovered parse can still reach here.
    fn sequence_duration(
        &mut self,
        duration: Option<&DurationExpression>,
        step: Option<&DurationExpression>,
        meter: Option<&Meter>,
        span: SourceSpan,
        item: &str,
    ) -> Option<Duration> {
        let Some(duration) = duration.or(step) else {
            self.error(
                &format!("{item} needs a duration, or a `step` on its sequence"),
                span,
            );
            return None;
        };
        self.item_duration(duration, meter, span, item)
    }

    /// Resolves an item's velocity at `position`, the place it sits in the
    /// repetition that produced it. A plain `velocity N` ignores the
    /// position; a `velocity A..B` ramp interpolates across the repetition's
    /// copies, and needs one to ramp across.
    fn velocity(
        &mut self,
        velocity: Option<&symphra_syntax::ast::VelocityExpression>,
        position: Repetition,
    ) -> Option<u8> {
        let Some(velocity) = velocity else {
            return Some(DEFAULT_VELOCITY);
        };
        if velocity.value > 127 || velocity.ramp_to.is_some_and(|end| end > 127) {
            self.error("velocity must be from 0 to 127", velocity.span);
            return None;
        }
        if velocity.ramp_to.is_some() && position.count <= 1 {
            self.error(
                "a velocity ramp needs a `* N` repetition to ramp across",
                velocity.span,
            );
            return None;
        }
        u8::try_from(position.velocity(velocity)).ok()
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the range and integer value are validated before conversion"
    )]
    fn sample_rate(&mut self, value: f64, unit: &str, span: SourceSpan) -> Option<u32> {
        let multiplier = match unit {
            "hz" => 1.0,
            "khz" => 1_000.0,
            _ => {
                self.error("sample_rate unit must be `hz` or `khz`", span);
                return None;
            }
        };
        let hertz = value * multiplier;
        if !hertz.is_finite() || hertz <= 0.0 || hertz.fract() != 0.0 || hertz > f64::from(u32::MAX)
        {
            self.error("sample_rate must be a positive whole number of hertz", span);
            None
        } else {
            Some(hertz as u32)
        }
    }

    fn tempo(&mut self, value: f64, unit: &str, span: SourceSpan) -> Option<f64> {
        if unit != "bpm" {
            self.error("tempo unit must be `bpm`", span);
            None
        } else if !value.is_finite() || value <= 0.0 {
            self.error("tempo must be greater than zero", span);
            None
        } else {
            Some(value)
        }
    }

    fn meter(&mut self, numerator: u32, denominator: u32, span: SourceSpan) -> Option<Meter> {
        if numerator == 0 || denominator == 0 {
            self.error("meter values must be greater than zero", span);
            None
        } else {
            Some(Meter {
                numerator,
                denominator,
            })
        }
    }

    fn key(&mut self, tonic: &str, mode: &str, span: SourceSpan) -> Option<Key> {
        let tonic = match tonic {
            "C" => PitchClass::C,
            "D" => PitchClass::D,
            "E" => PitchClass::E,
            "F" => PitchClass::F,
            "G" => PitchClass::G,
            "A" => PitchClass::A,
            "B" => PitchClass::B,
            _ => {
                self.error("key tonic must be a natural note from A to G", span);
                return None;
            }
        };
        let mode = match mode {
            "major" => Mode::Major,
            "minor" => Mode::Minor,
            _ => {
                self.error("key mode must be `major` or `minor`", span);
                return None;
            }
        };
        Some(Key { tonic, mode })
    }

    fn pitch(&mut self, pitch: &str, span: SourceSpan) -> Option<u8> {
        let mut chars = pitch.chars();
        let mut semitone = match chars.next() {
            Some('C') => 0,
            Some('D') => 2,
            Some('E') => 4,
            Some('F') => 5,
            Some('G') => 7,
            Some('A') => 9,
            Some('B') => 11,
            _ => {
                self.error("pitch must be a natural note followed by an octave", span);
                return None;
            }
        };
        match chars.clone().next() {
            Some('#') => {
                semitone += 1;
                chars.next();
            }
            Some('b') => {
                semitone -= 1;
                chars.next();
            }
            _ => {}
        }
        let Ok(octave) = chars.as_str().parse::<i16>() else {
            self.error(
                "pitch must be a note letter, optional `#` or `b`, and an octave",
                span,
            );
            return None;
        };
        let midi = (octave + 1) * 12 + semitone;
        if let Some(value) = u8::try_from(midi).ok().filter(|value| *value <= 127) {
            Some(value)
        } else {
            self.error("pitch must be within the MIDI range C-1 to G9", span);
            None
        }
    }

    fn id(&mut self) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Records a diagnostic, ignoring one that repeats a message already
    /// reported at the same span.
    ///
    /// Repetition sugar lowers one written item into several, so a mistake
    /// in `drum "cp" velocity 100..200 * 4` would otherwise be reported four
    /// times at the same place.
    fn error(&mut self, message: &str, span: SourceSpan) {
        let duplicate = self
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.span == span && diagnostic.message == message);
        if duplicate {
            return;
        }
        self.diagnostics.push(CompileDiagnostic {
            message: message.to_owned(),
            span,
        });
    }
}
