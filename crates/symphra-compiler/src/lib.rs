//! Semantic analysis and HIR lowering for Symphra.

use std::collections::HashSet;

use symphra_syntax::SourceSpan;
use symphra_syntax::ast::{
    ArrangementOccurrence, Declaration, DegreeChoiceAlternative, Identifier, InstrumentBody,
    PanExpression, PatternBody, PatternDeclaration, ProjectDeclaration, ProjectStatement,
    RhythmDeclaration, SequenceItem, SongDeclaration, SongStatement, SourceFile, StepItem,
    TrackDeclaration,
};

use crate::hir::{
    Arrangement, Channels, Chord, ChordNote, DegreeChoice, Duration, InstrumentKind, Key, Meter,
    Mode, NodeId, Note, Pan, Pattern, PatternOccurrence, PatternStep, PitchClass, Program, Project,
    Rest, Rhythm, RhythmItem, SampleChoice, SampleTrigger, Song, TrackDefinition, WeightedNote,
    WeightedSampleSequence,
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

    fn song(&mut self, declaration: &SongDeclaration) -> Option<Song> {
        let id = self.id();
        let mut settings = SongSettings::default();
        let mut pattern_declarations = Vec::new();
        let mut pattern_names = HashSet::new();
        let mut rhythm_defs = Vec::new();
        let mut rhythm_names = HashSet::new();
        let mut track_defs = Vec::new();
        let mut track_names = HashSet::new();
        let mut instruments = Vec::new();
        let mut instrument_names = HashSet::new();
        let mut arrangement = None;

        for statement in &declaration.statements {
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
                        track_defs.push(track);
                    }
                }
                SongStatement::Arrangement { occurrences, span } => {
                    if arrangement.replace((occurrences, *span)).is_some() {
                        self.error("arrangement is declared more than once", *span);
                    }
                }
                SongStatement::Pattern(pattern) => {
                    if self.declare_name(&mut pattern_names, &pattern.name, "pattern") {
                        pattern_declarations.push(pattern);
                    }
                }
            }
        }

        if !settings.tempo_seen {
            self.error("tempo is required", declaration.span);
        }
        if !settings.meter_seen {
            self.error("meter is required", declaration.span);
        }
        if !settings.key_seen {
            self.error("key is required", declaration.span);
        }
        let rhythms = rhythm_defs
            .iter()
            .filter_map(|rhythm| self.rhythm(rhythm))
            .collect::<Vec<_>>();
        let patterns = pattern_declarations
            .iter()
            .map(|pattern| self.pattern(pattern, settings.key.as_ref()))
            .collect::<Vec<_>>();
        let tracks = track_defs
            .iter()
            .filter_map(|track| self.track(track, &patterns, &rhythms, &instruments))
            .collect::<Vec<_>>();
        if !tracks.is_empty() && arrangement.is_some() {
            self.error(
                "track declarations cannot be combined with a pattern arrangement",
                declaration.span,
            );
        }
        let arrangement = arrangement.and_then(|(references, span)| {
            self.arrangement(references, span, &patterns, &instruments)
        });
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
                arrangement,
            }),
            _ => None,
        }
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

    fn track(
        &mut self,
        declaration: &TrackDeclaration,
        patterns: &[Pattern],
        rhythms: &[Rhythm],
        instruments: &[(&str, Option<InstrumentKind>)],
    ) -> Option<TrackDefinition> {
        let pattern = patterns
            .iter()
            .find(|pattern| pattern.name == declaration.play.pattern.text);
        if pattern.is_none() {
            self.error(
                "track references an unknown pattern",
                declaration.play.pattern.span,
            );
        }
        let instrument = instruments
            .iter()
            .find(|(name, _)| *name == declaration.instrument.text)
            .and_then(|(_, instrument)| instrument.clone());
        if instrument.is_none() {
            self.error(
                "track references an unknown instrument",
                declaration.instrument.span,
            );
        }
        let rhythm = declaration
            .play
            .trigger_with
            .as_ref()
            .and_then(|reference| {
                let rhythm = rhythms.iter().find(|rhythm| rhythm.name == reference.text);
                if rhythm.is_none() {
                    self.error("trigger_with references an unknown rhythm", reference.span);
                }
                rhythm
            });
        if let (Some(pattern), Some(rhythm), Some(reference)) =
            (pattern, rhythm, declaration.play.trigger_with.as_ref())
        {
            self.validate_trigger(pattern, rhythm, reference.span);
        }
        let gate_percent = match declaration.play.gate {
            Some(gate) => match u8::try_from(gate.percent) {
                Ok(percent) if percent <= 100 => Some(Some(percent)),
                _ => {
                    self.error("gate must be from 0% to 100%", gate.span);
                    None
                }
            },
            None => Some(None),
        };
        let transpose_semitones = match declaration.play.transpose.as_ref() {
            Some(transpose) if transpose.unit.text == "st" => Some(Some(transpose.semitones)),
            Some(transpose) => {
                self.error("transpose unit must be `st`", transpose.unit.span);
                None
            }
            None => Some(None),
        };
        let gain = self.track_gain(declaration);
        let repeat_count = self.repeat_count(declaration);
        let pan = self.pan(declaration);
        if let (Some(pattern), Some(Some(semitones)), Some(transpose)) = (
            pattern,
            transpose_semitones,
            declaration.play.transpose.as_ref(),
        ) {
            self.validate_transpose(pattern, semitones, transpose.span);
        }
        pattern
            .zip(instrument)
            .zip(gate_percent)
            .zip(transpose_semitones)
            .zip(gain)
            .zip(repeat_count)
            .zip(pan)
            .map(
                |(
                    (
                        ((((pattern, instrument), gate_percent), transpose_semitones), gain),
                        repeat_count,
                    ),
                    pan,
                )| {
                    TrackDefinition {
                        id: self.id(),
                        name: declaration.name.text.clone(),
                        role: declaration.role.text.clone(),
                        instrument,
                        pattern: pattern.id,
                        trigger_with: rhythm.map(|rhythm| rhythm.id),
                        gate_percent,
                        transpose_semitones,
                        gain,
                        repeat_count,
                        reverse: declaration.play.reverse,
                        pan,
                    }
                },
            )
    }

    fn repeat_count(&mut self, declaration: &TrackDeclaration) -> Option<u16> {
        match declaration.play.repeat {
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

    fn pan(&mut self, declaration: &TrackDeclaration) -> Option<Pan> {
        match declaration.play.pan {
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

    fn track_gain(&mut self, declaration: &TrackDeclaration) -> Option<f32> {
        let pipeline = match declaration.play.gain {
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
                declaration.play.span,
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
                PatternStep::Sample(_) | PatternStep::Choice(_) | PatternStep::DegreeChoice(_) => {
                    self.error(
                        "trigger_with currently supports note, chord, and rest patterns",
                        span,
                    );
                    return;
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
        references: &[ArrangementOccurrence],
        span: SourceSpan,
        patterns: &[Pattern],
        instruments: &[(&str, Option<InstrumentKind>)],
    ) -> Option<Arrangement> {
        if references.is_empty() {
            self.error("arrangement must contain at least one pattern", span);
            return None;
        }
        let occurrences = references
            .iter()
            .filter_map(|reference| {
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
        Some(Arrangement { occurrences })
    }

    fn instrument_kind(&mut self, body: &InstrumentBody) -> Option<InstrumentKind> {
        match body {
            InstrumentBody::Builtin(kind) => match kind.text.as_str() {
                "sine" => Some(InstrumentKind::Sine),
                "triangle" => Some(InstrumentKind::Triangle),
                _ => {
                    self.error(
                        "instrument kind must be `sine`, `triangle`, `sampled`, or `sampler`",
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
        }
    }

    fn pattern(&mut self, declaration: &PatternDeclaration, key: Option<&Key>) -> Pattern {
        let id = self.id();
        let steps = match &declaration.body {
            PatternBody::Sequence { items, .. } => self.sequence_steps(items),
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

    fn sequence_steps(&mut self, items: &[SequenceItem]) -> Vec<PatternStep> {
        items
            .iter()
            .filter_map(|item| match item {
                SequenceItem::Note(note) => {
                    let midi_pitch = self.pitch(&note.pitch.text, note.pitch.span);
                    let duration = self.duration(
                        note.duration_numerator,
                        note.duration_denominator,
                        note.span,
                        "note",
                    );
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
                    let duration = self.duration(
                        chord.duration_numerator,
                        chord.duration_denominator,
                        chord.span,
                        "chord",
                    );
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
                    .duration(
                        rest.duration_numerator,
                        rest.duration_denominator,
                        rest.span,
                        "rest",
                    )
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
                StepItem::Sample { index, .. } => Some(PatternStep::Sample(SampleTrigger {
                    id: self.id(),
                    index: *index,
                    duration,
                    velocity: DEFAULT_VELOCITY,
                })),
                StepItem::Rest { .. } => Some(PatternStep::Rest(Rest {
                    id: self.id(),
                    duration,
                })),
                StepItem::Choose { alternatives, span } => {
                    if alternatives.is_empty() {
                        self.error("choose must contain at least one sample", *span);
                    }
                    let alternatives = alternatives
                        .iter()
                        .filter_map(|alternative| {
                            if alternative.indices.is_empty() {
                                self.error(
                                    "choice sequence must contain at least one sample",
                                    alternative.span,
                                );
                                return None;
                            }
                            if alternative.weight == 0 {
                                self.error(
                                    "choice weight must be greater than zero",
                                    alternative.span,
                                );
                                None
                            } else {
                                Some(WeightedSampleSequence {
                                    samples: alternative
                                        .indices
                                        .iter()
                                        .map(|index| SampleTrigger {
                                            id: self.id(),
                                            index: *index,
                                            duration,
                                            velocity: DEFAULT_VELOCITY,
                                        })
                                        .collect(),
                                    weight: alternative.weight,
                                })
                            }
                        })
                        .collect();
                    Some(PatternStep::Choice(SampleChoice {
                        id: self.id(),
                        alternatives,
                    }))
                }
                StepItem::ChooseDegrees { alternatives, span } => Some(PatternStep::DegreeChoice(
                    self.degree_choice(alternatives, *span, duration, key),
                )),
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
