//! Semantic analysis and HIR lowering for Symphra.

use std::collections::HashSet;

use symphra_syntax::SourceSpan;
use symphra_syntax::ast::{
    Declaration, PatternBody, PatternDeclaration, ProjectDeclaration, ProjectStatement,
    SongDeclaration, SongStatement, SourceFile,
};

use crate::hir::{
    Channels, Duration, Key, Meter, Mode, NodeId, Note, Pattern, Program, Project, Song,
};

pub mod hir;

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
        let mut tempo_bpm = None;
        let mut meter = None;
        let mut key = None;
        let mut tempo_seen = false;
        let mut meter_seen = false;
        let mut key_seen = false;
        let mut patterns = Vec::new();
        let mut pattern_names = HashSet::new();

        for statement in &declaration.statements {
            match statement {
                SongStatement::Tempo { value, span } => {
                    if tempo_seen {
                        self.error("tempo is declared more than once", *span);
                    } else {
                        tempo_seen = true;
                        tempo_bpm = self.tempo(value.value.value, &value.unit.text, *span);
                    }
                }
                SongStatement::Meter {
                    numerator,
                    denominator,
                    span,
                } => {
                    if meter_seen {
                        self.error("meter is declared more than once", *span);
                    } else {
                        meter_seen = true;
                        meter = self.meter(*numerator, *denominator, *span);
                    }
                }
                SongStatement::Key { tonic, mode, span } => {
                    if key_seen {
                        self.error("key is declared more than once", *span);
                    } else {
                        key_seen = true;
                        key = self.key(&tonic.text, &mode.text, *span);
                    }
                }
                SongStatement::Pattern(pattern) => {
                    if pattern_names.insert(pattern.name.text.as_str()) {
                        patterns.push(self.pattern(pattern));
                    } else {
                        self.error("pattern name is declared more than once", pattern.name.span);
                    }
                }
            }
        }

        if !tempo_seen {
            self.error("tempo is required", declaration.span);
        }
        if !meter_seen {
            self.error("meter is required", declaration.span);
        }
        if !key_seen {
            self.error("key is required", declaration.span);
        }
        match (tempo_bpm, meter, key) {
            (Some(tempo_bpm), Some(meter), Some(key)) => Some(Song {
                id,
                name: declaration.name.value.clone(),
                tempo_bpm,
                meter,
                key,
                patterns,
            }),
            _ => None,
        }
    }

    fn pattern(&mut self, declaration: &PatternDeclaration) -> Pattern {
        let id = self.id();
        let PatternBody::Sequence { notes, .. } = &declaration.body;
        let notes = notes
            .iter()
            .filter_map(|note| {
                let midi_pitch = self.pitch(&note.pitch.text, note.pitch.span);
                let duration = if note.duration_numerator == 0 || note.duration_denominator == 0 {
                    self.error("note duration must be greater than zero", note.span);
                    None
                } else {
                    Some(Duration {
                        numerator: note.duration_numerator,
                        denominator: note.duration_denominator,
                    })
                };
                let (Some(midi_pitch), Some(duration)) = (midi_pitch, duration) else {
                    return None;
                };
                Some(Note {
                    id: self.id(),
                    midi_pitch,
                    duration,
                })
            })
            .collect();
        Pattern {
            id,
            name: declaration.name.text.clone(),
            notes,
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
        if !matches!(tonic, "A" | "B" | "C" | "D" | "E" | "F" | "G") {
            self.error("key tonic must be a natural note from A to G", span);
            return None;
        }
        let mode = match mode {
            "major" => Mode::Major,
            "minor" => Mode::Minor,
            _ => {
                self.error("key mode must be `major` or `minor`", span);
                return None;
            }
        };
        Some(Key {
            tonic: tonic.to_owned(),
            mode,
        })
    }

    fn pitch(&mut self, pitch: &str, span: SourceSpan) -> Option<u8> {
        let mut chars = pitch.chars();
        let semitone = match chars.next() {
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
        let octave = chars.as_str().parse::<i16>().ok();
        let midi = octave.map(|octave| (octave + 1) * 12 + semitone);
        if let Some(value) = midi
            .and_then(|value| u8::try_from(value).ok())
            .filter(|value| *value <= 127)
        {
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
