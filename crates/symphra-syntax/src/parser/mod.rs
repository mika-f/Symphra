mod literal;

use crate::ast::{
    ArrangementOccurrence, ChordExpression, Declaration, DegreeChoiceAlternative, Identifier,
    InstrumentBody, InstrumentDeclaration, NoteExpression, NumberLiteral, PatternBody,
    PatternDeclaration, PlayStatement, ProjectDeclaration, ProjectStatement, QuotedString,
    RateLiteral, RestExpression, RhythmDeclaration, RhythmItem, SampleChoiceAlternative,
    SequenceItem, SongDeclaration, SongStatement, SourceFile, StepItem, TrackDeclaration,
    VelocityExpression,
};
use crate::{Diagnostic, SourceId, SourceSpan, Token, TokenKind, lex};

const DECLARATION_START: &[TokenKind] = &[TokenKind::Project, TokenKind::Song, TokenKind::Eof];
const PROJECT_STATEMENT_START: &[TokenKind] = &[
    TokenKind::Seed,
    TokenKind::SampleRate,
    TokenKind::Output,
    TokenKind::RightBrace,
    TokenKind::Eof,
];
const SONG_STATEMENT_START: &[TokenKind] = &[
    TokenKind::Tempo,
    TokenKind::Meter,
    TokenKind::Key,
    TokenKind::Instrument,
    TokenKind::Rhythm,
    TokenKind::Track,
    TokenKind::Pattern,
    TokenKind::Arrangement,
    TokenKind::RightBrace,
    TokenKind::Eof,
];
const RHYTHM_ITEM_START: &[TokenKind] = &[
    TokenKind::Hit,
    TokenKind::Rest,
    TokenKind::RightBrace,
    TokenKind::Eof,
];
const SEQUENCE_ITEM_START: &[TokenKind] = &[
    TokenKind::Note,
    TokenKind::Chord,
    TokenKind::Rest,
    TokenKind::RightBrace,
    TokenKind::Eof,
];
const STEP_ITEM_START: &[TokenKind] = &[
    TokenKind::Degree,
    TokenKind::Sample,
    TokenKind::Choose,
    TokenKind::Rest,
    TokenKind::RightBrace,
    TokenKind::Eof,
];

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedSource {
    pub file: SourceFile,
    pub diagnostics: Vec<Diagnostic>,
}

#[must_use]
pub fn parse(source: SourceId, input: &str) -> ParsedSource {
    let lexed = lex(source, input);
    let mut parser = Parser {
        tokens: lexed.tokens,
        cursor: 0,
        diagnostics: lexed.diagnostics,
    };
    let declarations = parser.source_file();
    ParsedSource {
        file: SourceFile {
            source,
            declarations,
            span: SourceSpan::new(source, 0..input.len()),
        },
        diagnostics: parser.diagnostics,
    }
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    fn source_file(&mut self) -> Vec<Declaration> {
        let mut declarations = Vec::new();
        while !self.at(TokenKind::Eof) {
            let declaration = match self.current().kind {
                TokenKind::Project => self.project().map(Declaration::Project),
                TokenKind::Song => self.song().map(Declaration::Song),
                _ => {
                    self.error("expected `project` or `song`");
                    None
                }
            };
            if let Some(declaration) = declaration {
                declarations.push(declaration);
            } else {
                self.recover_to(DECLARATION_START);
            }
        }
        declarations
    }

    fn project(&mut self) -> Option<ProjectDeclaration> {
        let start = self.bump().span;
        self.required(TokenKind::LeftBrace, "expected `{` after `project`")?;
        let mut statements = Vec::new();
        while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
            let statement = match self.current().kind {
                TokenKind::Seed => self.seed(),
                TokenKind::SampleRate => self.sample_rate(),
                TokenKind::Output => self.output(),
                _ => {
                    self.error("expected a project setting");
                    None
                }
            };
            if let Some(statement) = statement {
                statements.push(statement);
            } else {
                self.recover_to(PROJECT_STATEMENT_START);
            }
        }
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close project")?
            .span;
        Some(ProjectDeclaration {
            statements,
            span: start.cover(end),
        })
    }

    fn seed(&mut self) -> Option<ProjectStatement> {
        let start = self.bump().span;
        let token = self.required(TokenKind::Integer, "expected an integer seed")?;
        let value = self.parse_u64(&token)?;
        Some(ProjectStatement::Seed {
            value,
            span: start.cover(token.span),
        })
    }

    fn sample_rate(&mut self) -> Option<ProjectStatement> {
        let start = self.bump().span;
        let value = self.rate()?;
        let span = start.cover(value.span);
        Some(ProjectStatement::SampleRate { value, span })
    }

    fn output(&mut self) -> Option<ProjectStatement> {
        let start = self.bump().span;
        let channels = self.identifier("expected an output layout")?;
        let span = start.cover(channels.span);
        Some(ProjectStatement::Output { channels, span })
    }

    fn song(&mut self) -> Option<SongDeclaration> {
        let start = self.bump().span;
        let name = self.string("expected a quoted song name")?;
        self.required(TokenKind::LeftBrace, "expected `{` after song name")?;
        let mut statements = Vec::new();
        while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
            let statement = match self.current().kind {
                TokenKind::Tempo => self.tempo(),
                TokenKind::Meter => self.meter(),
                TokenKind::Key => self.key(),
                TokenKind::Instrument => self.instrument().map(SongStatement::Instrument),
                TokenKind::Rhythm => self.rhythm().map(SongStatement::Rhythm),
                TokenKind::Track => self.track().map(SongStatement::Track),
                TokenKind::Arrangement => self.arrangement(),
                TokenKind::Pattern => self.pattern().map(SongStatement::Pattern),
                _ => {
                    self.error("expected a song setting or pattern");
                    None
                }
            };
            if let Some(statement) = statement {
                statements.push(statement);
            } else {
                self.recover_to(SONG_STATEMENT_START);
            }
        }
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close song")?
            .span;
        Some(SongDeclaration {
            name,
            statements,
            span: start.cover(end),
        })
    }

    fn tempo(&mut self) -> Option<SongStatement> {
        let start = self.bump().span;
        let value = self.rate()?;
        let span = start.cover(value.span);
        Some(SongStatement::Tempo { value, span })
    }

    fn meter(&mut self) -> Option<SongStatement> {
        let start = self.bump().span;
        let numerator_token = self.required(TokenKind::Integer, "expected meter numerator")?;
        self.required(TokenKind::Slash, "expected `/` in meter")?;
        let denominator_token = self.required(TokenKind::Integer, "expected meter denominator")?;
        let numerator = self.parse_u32(&numerator_token)?;
        let denominator = self.parse_u32(&denominator_token)?;
        Some(SongStatement::Meter {
            numerator,
            denominator,
            span: start.cover(denominator_token.span),
        })
    }

    fn key(&mut self) -> Option<SongStatement> {
        let start = self.bump().span;
        let tonic = self.identifier("expected key tonic")?;
        let mode = self.identifier("expected key mode")?;
        let span = start.cover(mode.span);
        Some(SongStatement::Key { tonic, mode, span })
    }

    fn instrument(&mut self) -> Option<InstrumentDeclaration> {
        let start = self.bump().span;
        let name = self.identifier("expected an instrument name")?;
        self.required(TokenKind::Equal, "expected `=` after instrument name")?;
        let body = if self.at(TokenKind::Sampled) {
            let sample_start = self.bump().span;
            self.required(TokenKind::LeftBrace, "expected `{` after `sampled`")?;
            self.required(TokenKind::Source, "expected `source` in sampled instrument")?;
            let source = self.string("expected a quoted sample source path")?;
            self.required(TokenKind::Root, "expected `root` after sample source")?;
            let root = self.identifier("expected a root pitch")?;
            let end = self.required(
                TokenKind::RightBrace,
                "expected `}` to close sampled instrument",
            )?;
            InstrumentBody::Sampled {
                source,
                root,
                span: sample_start.cover(end.span),
            }
        } else if self.at(TokenKind::Sampler) {
            let sampler_start = self.bump().span;
            self.required(TokenKind::LeftBrace, "expected `{` after `sampler`")?;
            self.required(TokenKind::Pack, "expected `pack` in sampler instrument")?;
            let pack = self.string("expected a quoted sample pack name")?;
            let end = self.required(
                TokenKind::RightBrace,
                "expected `}` to close sampler instrument",
            )?;
            InstrumentBody::Sampler {
                pack,
                span: sampler_start.cover(end.span),
            }
        } else {
            InstrumentBody::Builtin(self.identifier("expected an instrument kind")?)
        };
        let span = match &body {
            InstrumentBody::Builtin(kind) => start.cover(kind.span),
            InstrumentBody::Sampled { span, .. } | InstrumentBody::Sampler { span, .. } => {
                start.cover(*span)
            }
        };
        Some(InstrumentDeclaration { name, body, span })
    }

    fn arrangement(&mut self) -> Option<SongStatement> {
        let start = self.bump().span;
        self.required(TokenKind::LeftBrace, "expected `{` after `arrangement`")?;
        let mut occurrences = Vec::new();
        while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
            if self.at(TokenKind::Identifier) {
                let pattern = self.identifier("expected a pattern name in arrangement")?;
                let instrument = if self.at(TokenKind::With) {
                    self.bump();
                    Some(self.identifier("expected an instrument name after `with`")?)
                } else {
                    None
                };
                let span = instrument.as_ref().map_or(pattern.span, |instrument| {
                    pattern.span.cover(instrument.span)
                });
                occurrences.push(ArrangementOccurrence {
                    pattern,
                    instrument,
                    span,
                });
            } else {
                self.error("expected a pattern name in arrangement");
                while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
                    self.bump();
                }
            }
        }
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close arrangement")?
            .span;
        Some(SongStatement::Arrangement {
            occurrences,
            span: start.cover(end),
        })
    }

    fn rhythm(&mut self) -> Option<RhythmDeclaration> {
        let start = self.bump().span;
        let name = self.identifier("expected a rhythm name")?;
        self.required(
            TokenKind::Resolution,
            "expected `resolution` after rhythm name",
        )?;
        let numerator = self.required(TokenKind::Integer, "expected resolution numerator")?;
        self.required(TokenKind::Slash, "expected `/` in rhythm resolution")?;
        let denominator = self.required(TokenKind::Integer, "expected resolution denominator")?;
        self.required(TokenKind::LeftBrace, "expected `{` after rhythm resolution")?;
        let mut items = Vec::new();
        while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
            let item = match self.current().kind {
                TokenKind::Hit => Some(RhythmItem::Hit {
                    span: self.bump().span,
                }),
                TokenKind::Rest => Some(RhythmItem::Rest {
                    span: self.bump().span,
                }),
                _ => {
                    self.error("expected `hit` or `rest` in rhythm");
                    None
                }
            };
            if let Some(item) = item {
                items.push(item);
            } else {
                self.recover_to(RHYTHM_ITEM_START);
            }
        }
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close rhythm")?
            .span;
        Some(RhythmDeclaration {
            name,
            resolution_numerator: self.parse_u32(&numerator)?,
            resolution_denominator: self.parse_u32(&denominator)?,
            items,
            span: start.cover(end),
        })
    }

    fn track(&mut self) -> Option<TrackDeclaration> {
        let start = self.bump().span;
        let name = self.identifier("expected a track name")?;
        self.required(TokenKind::Role, "expected `role` after track name")?;
        let role = self.identifier("expected a track role")?;
        self.required(TokenKind::LeftBrace, "expected `{` after track role")?;
        self.required(TokenKind::Instrument, "expected `instrument` in track")?;
        let instrument = self.identifier("expected an instrument name")?;
        let play_start = self
            .required(TokenKind::Play, "expected `play` in track")?
            .span;
        let pattern = self.identifier("expected a pattern name after `play`")?;
        let trigger_with = if self.at(TokenKind::PipeGreater) {
            self.bump();
            self.required(TokenKind::TriggerWith, "expected `trigger_with` after `|>`")?;
            Some(self.identifier("expected a rhythm name after `trigger_with`")?)
        } else {
            None
        };
        let play_span = trigger_with
            .as_ref()
            .map_or(pattern.span, |rhythm| pattern.span.cover(rhythm.span));
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close track")?
            .span;
        Some(TrackDeclaration {
            name,
            role,
            instrument,
            play: PlayStatement {
                pattern,
                trigger_with,
                span: play_start.cover(play_span),
            },
            span: start.cover(end),
        })
    }

    fn pattern(&mut self) -> Option<PatternDeclaration> {
        let start = self.bump().span;
        let name = self.identifier("expected pattern name")?;
        self.required(TokenKind::Equal, "expected `=` after pattern name")?;
        if self.at(TokenKind::Steps) {
            return self.steps_pattern(start, name);
        }
        self.required(
            TokenKind::Sequence,
            "expected `sequence` or `steps` pattern body",
        )?;
        let body_start = self
            .required(TokenKind::LeftBrace, "expected `{` after `sequence`")?
            .span;
        let mut items = Vec::new();
        while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
            let item = match self.current().kind {
                TokenKind::Note => self.note().map(SequenceItem::Note),
                TokenKind::Chord => self.chord().map(SequenceItem::Chord),
                TokenKind::Rest => self.rest().map(SequenceItem::Rest),
                _ => {
                    self.error("expected a note, chord, or rest in sequence");
                    None
                }
            };
            if let Some(item) = item {
                items.push(item);
            } else {
                self.recover_to(SEQUENCE_ITEM_START);
            }
        }
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close sequence")?
            .span;
        Some(PatternDeclaration {
            name,
            body: PatternBody::Sequence {
                items,
                span: body_start.cover(end),
            },
            span: start.cover(end),
        })
    }

    fn steps_pattern(&mut self, start: SourceSpan, name: Identifier) -> Option<PatternDeclaration> {
        self.bump();
        let numerator = self.required(TokenKind::Integer, "expected step resolution numerator")?;
        self.required(TokenKind::Slash, "expected `/` in step resolution")?;
        let denominator =
            self.required(TokenKind::Integer, "expected step resolution denominator")?;
        let body_start = self
            .required(TokenKind::LeftBrace, "expected `{` after step resolution")?
            .span;
        let mut items = Vec::new();
        while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
            let item = match self.current().kind {
                TokenKind::Degree => self.degree_step(),
                TokenKind::Sample => {
                    let sample = self.bump().span;
                    self.required(TokenKind::Integer, "expected sample index")
                        .and_then(|index| {
                            self.parse_u32(&index).map(|index_value| StepItem::Sample {
                                index: index_value,
                                span: sample.cover(index.span),
                            })
                        })
                }
                TokenKind::Rest => Some(StepItem::Rest {
                    span: self.bump().span,
                }),
                TokenKind::Choose => self.choice(),
                _ => {
                    self.error("expected a degree, sample, rest, or choose in steps");
                    None
                }
            };
            if let Some(item) = item {
                items.push(item);
            } else {
                self.recover_to(STEP_ITEM_START);
            }
        }
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close steps")?
            .span;
        Some(PatternDeclaration {
            name,
            body: PatternBody::Steps {
                resolution_numerator: self.parse_u32(&numerator)?,
                resolution_denominator: self.parse_u32(&denominator)?,
                items,
                span: body_start.cover(end),
            },
            span: start.cover(end),
        })
    }

    fn degree_step(&mut self) -> Option<StepItem> {
        let start = self.bump().span;
        let degree = self.required(TokenKind::Integer, "expected degree offset")?;
        self.required(TokenKind::Octave, "expected `octave` after degree")?;
        let octave = self.required(TokenKind::Integer, "expected octave number")?;
        Some(StepItem::Degree {
            degree: self.parse_u32(&degree)?,
            octave: self.parse_u32(&octave)?,
            span: start.cover(octave.span),
        })
    }

    fn choice(&mut self) -> Option<StepItem> {
        let start = self.bump().span;
        self.required(TokenKind::LeftBrace, "expected `{` after `choose`")?;
        if self.at(TokenKind::Degree) {
            return self.degree_choice(start);
        }
        let mut alternatives = Vec::new();
        while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
            let alternative_start = self.current().span;
            let (indices, default_weight_span) = match self.current().kind {
                TokenKind::Sample => {
                    self.bump();
                    let index = self.required(TokenKind::Integer, "expected sample index")?;
                    (vec![self.parse_u32(&index)?], index.span)
                }
                TokenKind::Sequence => {
                    self.bump();
                    let weight = if self.at(TokenKind::Weight) {
                        self.bump();
                        Some(self.required(TokenKind::Integer, "expected choice weight")?)
                    } else {
                        None
                    };
                    self.required(TokenKind::LeftBrace, "expected `{` after choice sequence")?;
                    let mut indices = Vec::new();
                    while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
                        self.required(TokenKind::Sample, "expected `sample` in choice sequence")?;
                        let index = self.required(TokenKind::Integer, "expected sample index")?;
                        indices.push(self.parse_u32(&index)?);
                    }
                    let end = self
                        .required(
                            TokenKind::RightBrace,
                            "expected `}` to close choice sequence",
                        )?
                        .span;
                    let weight = weight.unwrap_or(Token {
                        kind: TokenKind::Integer,
                        text: "1".to_owned(),
                        span: end,
                    });
                    alternatives.push(SampleChoiceAlternative {
                        indices,
                        weight: self.parse_u32(&weight)?,
                        span: alternative_start.cover(end),
                    });
                    continue;
                }
                _ => {
                    self.error("expected `sample` or `sequence` in choose");
                    return None;
                }
            };
            let weight = if self.at(TokenKind::Weight) {
                self.bump();
                self.required(TokenKind::Integer, "expected choice weight")?
            } else {
                Token {
                    kind: TokenKind::Integer,
                    text: "1".to_owned(),
                    span: default_weight_span,
                }
            };
            alternatives.push(SampleChoiceAlternative {
                indices,
                weight: self.parse_u32(&weight)?,
                span: alternative_start.cover(weight.span),
            });
        }
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close choose")?
            .span;
        Some(StepItem::Choose {
            alternatives,
            span: start.cover(end),
        })
    }

    fn degree_choice(&mut self, start: SourceSpan) -> Option<StepItem> {
        let mut alternatives = Vec::new();
        while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
            let alternative_start =
                self.required(TokenKind::Degree, "expected `degree` in choose")?;
            let degree = self.required(TokenKind::Integer, "expected degree offset")?;
            self.required(TokenKind::Octave, "expected `octave` after degree")?;
            let octave = self.required(TokenKind::Integer, "expected octave number")?;
            let weight = if self.at(TokenKind::Weight) {
                self.bump();
                self.required(TokenKind::Integer, "expected choice weight")?
            } else {
                Token {
                    kind: TokenKind::Integer,
                    text: "1".to_owned(),
                    span: octave.span,
                }
            };
            alternatives.push(DegreeChoiceAlternative {
                degree: self.parse_u32(&degree)?,
                octave: self.parse_u32(&octave)?,
                weight: self.parse_u32(&weight)?,
                span: alternative_start.span.cover(weight.span),
            });
        }
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close choose")?
            .span;
        Some(StepItem::ChooseDegrees {
            alternatives,
            span: start.cover(end),
        })
    }

    fn note(&mut self) -> Option<NoteExpression> {
        let start = self.bump().span;
        let pitch = self.identifier("expected note pitch")?;
        self.required(TokenKind::For, "expected `for` after note pitch")?;
        let numerator_token = self.required(TokenKind::Integer, "expected duration numerator")?;
        self.required(TokenKind::Slash, "expected `/` in note duration")?;
        let denominator_token =
            self.required(TokenKind::Integer, "expected duration denominator")?;
        let velocity = self.velocity();
        let end = velocity.map_or(denominator_token.span, |velocity| velocity.span);
        Some(NoteExpression {
            pitch,
            duration_numerator: self.parse_u32(&numerator_token)?,
            duration_denominator: self.parse_u32(&denominator_token)?,
            velocity,
            span: start.cover(end),
        })
    }

    fn rest(&mut self) -> Option<RestExpression> {
        let start = self.bump().span;
        self.required(TokenKind::For, "expected `for` after `rest`")?;
        let numerator_token = self.required(TokenKind::Integer, "expected duration numerator")?;
        self.required(TokenKind::Slash, "expected `/` in rest duration")?;
        let denominator_token =
            self.required(TokenKind::Integer, "expected duration denominator")?;
        Some(RestExpression {
            duration_numerator: self.parse_u32(&numerator_token)?,
            duration_denominator: self.parse_u32(&denominator_token)?,
            span: start.cover(denominator_token.span),
        })
    }

    fn chord(&mut self) -> Option<ChordExpression> {
        let start = self.bump().span;
        let mut pitches = vec![self.identifier("expected first chord pitch")?];
        pitches.push(self.identifier("expected second chord pitch")?);
        while self.at(TokenKind::Identifier) {
            pitches.push(self.identifier("expected chord pitch")?);
        }
        self.required(TokenKind::For, "expected `for` after chord pitches")?;
        let numerator_token = self.required(TokenKind::Integer, "expected duration numerator")?;
        self.required(TokenKind::Slash, "expected `/` in chord duration")?;
        let denominator_token =
            self.required(TokenKind::Integer, "expected duration denominator")?;
        let velocity = self.velocity();
        let end = velocity.map_or(denominator_token.span, |velocity| velocity.span);
        Some(ChordExpression {
            pitches,
            duration_numerator: self.parse_u32(&numerator_token)?,
            duration_denominator: self.parse_u32(&denominator_token)?,
            velocity,
            span: start.cover(end),
        })
    }

    fn velocity(&mut self) -> Option<VelocityExpression> {
        if !self.at(TokenKind::Velocity) {
            return None;
        }
        let start = self.bump().span;
        let value = self.required(TokenKind::Integer, "expected velocity from 0 to 127")?;
        Some(VelocityExpression {
            value: self.parse_u32(&value)?,
            span: start.cover(value.span),
        })
    }

    fn rate(&mut self) -> Option<RateLiteral> {
        let number = self.required_any(
            &[TokenKind::Integer, TokenKind::Decimal],
            "expected a number",
        )?;
        let unit = self.identifier("expected a unit immediately after the number")?;
        let Ok(value) = number.text.parse::<f64>() else {
            self.diagnostics
                .push(Diagnostic::syntax("number is out of range", number.span));
            return None;
        };
        let span = number.span.cover(unit.span);
        Some(RateLiteral {
            value: NumberLiteral {
                value,
                span: number.span,
            },
            unit,
            span,
        })
    }

    fn identifier(&mut self, message: &str) -> Option<Identifier> {
        let token = self.required(TokenKind::Identifier, message)?;
        Some(Identifier {
            text: token.text,
            span: token.span,
        })
    }

    fn string(&mut self, message: &str) -> Option<QuotedString> {
        let token = self.required(TokenKind::String, message)?;
        let inner = token
            .text
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or("");
        let value = inner
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
            .replace("\\n", "\n");
        Some(QuotedString {
            value,
            span: token.span,
        })
    }

    fn parse_u64(&mut self, token: &Token) -> Option<u64> {
        token
            .text
            .parse()
            .map_err(|_| {
                self.diagnostics
                    .push(Diagnostic::syntax("integer is out of range", token.span));
            })
            .ok()
    }

    fn parse_u32(&mut self, token: &Token) -> Option<u32> {
        token
            .text
            .parse()
            .map_err(|_| {
                self.diagnostics
                    .push(Diagnostic::syntax("integer is out of range", token.span));
            })
            .ok()
    }

    fn required(&mut self, kind: TokenKind, message: &str) -> Option<Token> {
        self.required_any(&[kind], message)
    }

    fn required_any(&mut self, kinds: &[TokenKind], message: &str) -> Option<Token> {
        if self.at_any(kinds) {
            Some(self.bump())
        } else {
            self.error(message);
            None
        }
    }

    fn error(&mut self, message: &str) {
        self.diagnostics
            .push(Diagnostic::syntax(message, self.current().span));
    }

    fn recover_to(&mut self, kinds: &[TokenKind]) {
        while !self.at_any(kinds) && !self.at(TokenKind::Eof) {
            self.bump();
        }
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.current().kind == kind
    }

    fn at_any(&self, kinds: &[TokenKind]) -> bool {
        kinds.contains(&self.current().kind)
    }

    fn current(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn bump(&mut self) -> Token {
        let token = self.current().clone();
        if token.kind != TokenKind::Eof {
            self.cursor += 1;
        }
        token
    }
}
