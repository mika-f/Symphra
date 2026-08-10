//! Semantic analysis and HIR lowering for Symphra.

use std::collections::HashSet;

use symphra_syntax::SourceSpan;
use symphra_syntax::ast::{
    ArrangementEntry, ChanceTransformExpression, Declaration, DegreeChoiceAlternative,
    DurationExpression, EffectDeclaration, EffectKind, Identifier, InstrumentBody,
    MasterDeclaration, PanExpression, PatternBody, PatternDeclaration, PlaySource, PlayStatement,
    ProjectDeclaration, ProjectStatement, QuotedString, RhythmDeclaration, SampleChoiceAlternative,
    SampleSelectorExpression, SectionDeclaration, SequenceItem, SongDeclaration, SongStatement,
    SourceFile, SpeedExpression, StepItem, TrackBody, TrackDeclaration,
};

use crate::hir::{
    Arrangement, Chance, ChanceTransform, Channels, Chord, ChordNote, DegreeChoice, DelayEffect,
    Duration, Effect, FilterEffect, InstrumentKind, Key, MasterLimiter, Meter, Mode, NodeId, Note,
    Pan, Pattern, PatternOccurrence, PatternStep, PitchClass, Program, Project, Rest, ReverbEffect,
    Rhythm, RhythmItem, SampleChoice, SampleRange, SampleSelector, SampleTrigger, Section,
    SectionOccurrence, Song, Speed, TrackDefinition, WeightedNote, WeightedSampleSequence,
};

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
        let mut patterns = pattern_declarations
            .iter()
            .map(|pattern| self.pattern(pattern, settings.key.as_ref(), settings.meter.as_ref()))
            .collect::<Vec<_>>();
        let tracks = self.build_tracks(
            &track_defs,
            &mut patterns,
            &rhythms,
            &instruments,
            settings.meter.as_ref(),
        );
        let sections = section_defs
            .iter()
            .filter_map(|section| self.section(section, &tracks, settings.meter.as_ref()))
            .collect::<Vec<_>>();
        self.check_arrangement_track_combination(
            arrangement.as_ref(),
            !tracks.is_empty(),
            declaration.span,
        );
        let arrangement = arrangement.and_then(|(entries, span)| {
            self.arrangement(entries, span, &patterns, &instruments, &sections)
        });
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
        meter: Option<&Meter>,
    ) -> Option<Section> {
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
        for name in &declaration.tracks {
            let matches = tracks
                .iter()
                .filter(|track| track.name == name.text)
                .map(|track| track.id)
                .collect::<Vec<_>>();
            if matches.is_empty() {
                self.error("section references an unknown track", name.span);
                any_missing = true;
            } else {
                track_ids.extend(matches);
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

    fn build_tracks(
        &mut self,
        track_defs: &[&TrackDeclaration],
        patterns: &mut Vec<Pattern>,
        rhythms: &[Rhythm],
        instruments: &[(&str, Option<InstrumentKind>)],
        meter: Option<&Meter>,
    ) -> Vec<TrackDefinition> {
        let mut tracks = Vec::with_capacity(track_defs.len());
        for track in track_defs {
            for (definition, synthesized) in
                self.track(track, patterns, rhythms, instruments, meter)
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
        Some(Rhythm {
            id: self.id(),
            name: declaration.name.text.clone(),
            resolution,
            items: declaration
                .items
                .iter()
                .map(|item| match item {
                    symphra_syntax::ast::RhythmItem::Hit { .. } => RhythmItem::Hit,
                    symphra_syntax::ast::RhythmItem::Rest { .. } => RhythmItem::Rest,
                })
                .collect(),
        })
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
    ) -> Vec<(TrackDefinition, Option<Pattern>)> {
        // Resolved once per declaration (not per layer) so an invalid effect
        // is reported once, not once per `use`.
        let effect = self
            .effect(declaration.effect.as_ref(), meter)
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
                        repeat_count,
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

    fn repeat_count(&mut self, play: &PlayStatement) -> Option<u16> {
        match play.repeat {
            Some(repeat) => match u16::try_from(repeat.count) {
                Ok(count) if count > 0 => Some(count),
                _ => {
                    self.error("repeat must be from 1 to 65535", repeat.span);
                    None
                }
            },
            None => Some(1),
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
    /// `effect reverb { ... }`. Delay `feedback` is capped at `0.95` (not the
    /// theoretical stability limit of `1.0`) so a delay's echo tail — which
    /// the renderer must extend the song's audio buffer to fit — always
    /// decays to silence within a bounded number of repeats. Filter `cutoff`
    /// is only checked for being a positive, finite frequency here; checking
    /// it against the Nyquist limit needs the project's sample rate, which is
    /// not in scope during song/track compilation, so the renderer clamps it
    /// defensively at render time instead (mirroring the existing
    /// defensive-revalidation precedent for a hand-constructed
    /// [`MasterLimiter`]). Reverb `size` needs no such cross-namespace check —
    /// it is a plain `0.0..=1.0` factor, not a frequency or duration.
    fn effect(
        &mut self,
        declaration: Option<&EffectDeclaration>,
        meter: Option<&Meter>,
    ) -> Result<Option<Effect>, ()> {
        let Some(declaration) = declaration else {
            return Ok(None);
        };
        match &declaration.kind {
            EffectKind::Delay {
                mix,
                time,
                feedback,
            } => {
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
                let multiplier = match cutoff.unit.text.as_str() {
                    "hz" => 1.0,
                    "khz" => 1_000.0,
                    _ => {
                        self.error(
                            "effect filter cutoff unit must be `hz` or `khz`",
                            cutoff.span,
                        );
                        return Err(());
                    }
                };
                let cutoff_hz = cutoff.value.value * multiplier;
                if !cutoff_hz.is_finite() || cutoff_hz <= 0.0 {
                    self.error("effect filter cutoff must be greater than 0hz", cutoff.span);
                    return Err(());
                }
                if !resonance.value.is_finite() || !(0.0..=1.0).contains(&resonance.value) {
                    self.error(
                        "effect filter resonance must be from 0.0 to 1.0",
                        resonance.span,
                    );
                    return Err(());
                }
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "cutoff_hz is validated finite and positive before conversion"
                )]
                let cutoff_hz = cutoff_hz as f32;
                Ok(Some(Effect::Filter(FilterEffect {
                    cutoff_hz,
                    resonance: resonance.value,
                })))
            }
            EffectKind::Reverb { mix, size } => {
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
                let instrument =
                    reference
                        .instrument
                        .as_ref()
                        .map_or(Some(InstrumentKind::Sine), |reference| {
                            let instrument =
                                instruments.iter().find(|(name, _)| *name == reference.text);
                            if instrument.is_none() {
                                self.error(
                                    "arrangement references an unknown instrument",
                                    reference.span,
                                );
                            }
                            instrument.and_then(|(_, kind)| kind.clone())
                        });
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
            InstrumentBody::Builtin(kind) => match kind.text.as_str() {
                "sine" => Some(InstrumentKind::Sine),
                "triangle" => Some(InstrumentKind::Triangle),
                _ => {
                    self.error(
                        "instrument kind must be `sine`, `triangle`, `sampled`, `sampler`, or `drum_machine`",
                        kind.span,
                    );
                    None
                }
            },
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
        }
    }

    fn pattern(
        &mut self,
        declaration: &PatternDeclaration,
        key: Option<&Key>,
        meter: Option<&Meter>,
    ) -> Pattern {
        let id = self.id();
        let steps = match &declaration.body {
            PatternBody::Sequence { items, .. } => self.sequence_steps(items, meter),
            PatternBody::Steps {
                resolution_numerator,
                resolution_denominator,
                items,
                span,
            } => self.steps(
                *resolution_numerator,
                *resolution_denominator,
                items,
                *span,
                key,
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
        meter: Option<&Meter>,
    ) -> Vec<PatternStep> {
        items
            .iter()
            .filter_map(|item| match item {
                SequenceItem::Note(note) => {
                    let midi_pitch = self.pitch(&note.pitch.text, note.pitch.span);
                    let duration = self.item_duration(&note.duration, meter, note.span, "note");
                    let velocity = self.velocity(note.velocity.as_ref());
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
                    let midi_pitches = chord
                        .pitches
                        .iter()
                        .map(|pitch| self.pitch(&pitch.text, pitch.span))
                        .collect::<Option<Vec<_>>>();
                    let duration = self.item_duration(&chord.duration, meter, chord.span, "chord");
                    let velocity = self.velocity(chord.velocity.as_ref());
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
                    .item_duration(&rest.duration, meter, rest.span, "rest")
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
        numerator: u32,
        denominator: u32,
        items: &[StepItem],
        span: SourceSpan,
        key: Option<&Key>,
    ) -> Vec<PatternStep> {
        let Some(duration) = self.duration(numerator, denominator, span, "step") else {
            return Vec::new();
        };
        items
            .iter()
            .filter_map(|item| match item {
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
                } => self.velocity(velocity.as_ref()).map(|velocity| {
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
                        self.velocity(velocity.as_ref()).map(|velocity| {
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
            })
            .collect()
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

    fn velocity(
        &mut self,
        velocity: Option<&symphra_syntax::ast::VelocityExpression>,
    ) -> Option<u8> {
        let Some(velocity) = velocity else {
            return Some(DEFAULT_VELOCITY);
        };
        if let Some(value) = u8::try_from(velocity.value)
            .ok()
            .filter(|value| *value <= 127)
        {
            Some(value)
        } else {
            self.error("velocity must be from 0 to 127", velocity.span);
            None
        }
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

    fn error(&mut self, message: &str, span: SourceSpan) {
        self.diagnostics.push(CompileDiagnostic {
            message: message.to_owned(),
            span,
        });
    }
}
