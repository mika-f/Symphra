mod literal;

use crate::ast::{
    ArrangementEntry, ArrangementOccurrence, AtExpression, ChanceExpression,
    ChanceTransformExpression, ChooseSampleExpression, ChordExpression, Declaration,
    DegreeChoiceAlternative, DurationExpression, EffectDeclaration, EffectFactor, GainExpression,
    GateExpression, Identifier, InstrumentBody, InstrumentDeclaration, LayerUse, MasterDeclaration,
    NoteExpression, NumberLiteral, PanExpression, PatternBody, PatternDeclaration, PlaySource,
    PlayStatement, ProjectDeclaration, ProjectStatement, QuotedString, RateLiteral,
    RepeatExpression, RestExpression, RhythmDeclaration, RhythmItem, SampleChoiceAlternative,
    SampleSelectorExpression, SectionDeclaration, SequenceItem, SongDeclaration, SongStatement,
    SourceFile, SpeedExpression, StepItem, TrackBody, TrackDeclaration, TransposeExpression,
    VelocityExpression, VolumeExpression,
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
    TokenKind::Section,
    TokenKind::Master,
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
    TokenKind::Drum,
    TokenKind::Choose,
    TokenKind::Rest,
    TokenKind::RightBrace,
    TokenKind::Eof,
];
const LAYER_USE_START: &[TokenKind] = &[TokenKind::Use, TokenKind::RightBrace, TokenKind::Eof];
const PARALLEL_PLAY_START: &[TokenKind] = &[TokenKind::Play, TokenKind::RightBrace, TokenKind::Eof];

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
                TokenKind::Track => self.track().map(Box::new).map(SongStatement::Track),
                TokenKind::Section => self.section(),
                TokenKind::Master => self.master().map(SongStatement::Master),
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
        } else if self.at(TokenKind::DrumMachine) {
            let drum_machine_start = self.bump().span;
            self.required(TokenKind::LeftBrace, "expected `{` after `drum_machine`")?;
            self.required(
                TokenKind::Bank,
                "expected `bank` in drum machine instrument",
            )?;
            let bank = self.string("expected a quoted drum bank name")?;
            let end = self.required(
                TokenKind::RightBrace,
                "expected `}` to close drum machine instrument",
            )?;
            InstrumentBody::DrumMachine {
                bank,
                span: drum_machine_start.cover(end.span),
            }
        } else {
            InstrumentBody::Builtin(self.identifier("expected an instrument kind")?)
        };
        let span = match &body {
            InstrumentBody::Builtin(kind) => start.cover(kind.span),
            InstrumentBody::Sampled { span, .. }
            | InstrumentBody::Sampler { span, .. }
            | InstrumentBody::DrumMachine { span, .. } => start.cover(*span),
        };
        Some(InstrumentDeclaration { name, body, span })
    }

    fn arrangement(&mut self) -> Option<SongStatement> {
        let start = self.bump().span;
        self.required(TokenKind::LeftBrace, "expected `{` after `arrangement`")?;
        let mut entries = Vec::new();
        while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
            if self.at(TokenKind::Play) {
                let play_start = self.bump().span;
                let name = self.identifier("expected a section name after `play`")?;
                entries.push(ArrangementEntry::Play {
                    span: play_start.cover(name.span),
                    name,
                });
            } else if self.at(TokenKind::Identifier) {
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
                entries.push(ArrangementEntry::Pattern(ArrangementOccurrence {
                    pattern,
                    instrument,
                    span,
                }));
            } else {
                self.error("expected a pattern name or `play <name>` in arrangement");
                while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
                    self.bump();
                }
            }
        }
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close arrangement")?
            .span;
        Some(SongStatement::Arrangement {
            entries,
            span: start.cover(end),
        })
    }

    /// `section <name> bars <N> { parallel [exact] { play track <name> ... } }`.
    /// The body is always exactly one `parallel` block.
    fn section(&mut self) -> Option<SongStatement> {
        let start = self.bump().span;
        let name = self.identifier("expected a section name")?;
        self.required(TokenKind::Bars, "expected `bars` after section name")?;
        let bars_token = self.required(TokenKind::Integer, "expected a bar count after `bars`")?;
        let bars = self.parse_u32(&bars_token)?;
        self.required(TokenKind::LeftBrace, "expected `{` after section header")?;
        let (exact, tracks) = self.parallel_block()?;
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close section")?
            .span;
        Some(SongStatement::Section(SectionDeclaration {
            name,
            bars,
            exact,
            tracks,
            span: start.cover(end),
        }))
    }

    fn parallel_block(&mut self) -> Option<(bool, Vec<Identifier>)> {
        self.required(TokenKind::Parallel, "expected `parallel` in section")?;
        let exact = if self.at(TokenKind::Exact) {
            self.bump();
            true
        } else {
            false
        };
        self.required(TokenKind::LeftBrace, "expected `{` after `parallel`")?;
        let mut tracks = Vec::new();
        while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
            if self.at(TokenKind::Play) {
                self.bump();
                self.required(TokenKind::Track, "expected `track` after `play`")?;
                let track = self.identifier("expected a track name after `play track`")?;
                tracks.push(track);
            } else {
                self.error("expected `play track <name>` in parallel block");
                self.recover_to(PARALLEL_PLAY_START);
            }
        }
        self.required(TokenKind::RightBrace, "expected `}` to close parallel")?;
        Some((exact, tracks))
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
        let (volume, body) = if self.at(TokenKind::Instrument) {
            self.bump();
            let instrument = self.identifier("expected an instrument name")?;
            let volume = if self.at(TokenKind::Volume) {
                Some(Box::new(self.volume()?))
            } else {
                None
            };
            let play = Box::new(self.play()?);
            (volume, TrackBody::Single { instrument, play })
        } else {
            let volume = if self.at(TokenKind::Volume) {
                Some(Box::new(self.volume()?))
            } else {
                None
            };
            (volume, self.layer_body()?)
        };
        let effect = if self.at(TokenKind::Effect) {
            Some(self.effect()?)
        } else {
            None
        };
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close track")?
            .span;
        Some(TrackDeclaration {
            name,
            role,
            volume,
            body,
            effect,
            span: start.cover(end),
        })
    }

    /// `effect delay { mix M time T feedback F }`, applied to the whole
    /// track (every layer) after its events are rendered to audio. `delay`
    /// is the only effect kind accepted today.
    fn effect(&mut self) -> Option<EffectDeclaration> {
        let start = self.bump().span;
        self.required(TokenKind::Delay, "expected `delay` after `effect`")?;
        self.required(TokenKind::LeftBrace, "expected `{` after `effect delay`")?;
        let mix = self.effect_factor(TokenKind::Mix, "expected `mix` in effect")?;
        self.required(TokenKind::Time, "expected `time` after `mix`")?;
        let time = self.duration_expr("effect delay")?;
        let feedback =
            self.effect_factor(TokenKind::Feedback, "expected `feedback` after `time`")?;
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close effect")?
            .span;
        Some(EffectDeclaration {
            mix,
            time,
            feedback,
            span: start.cover(end),
        })
    }

    fn effect_factor(&mut self, keyword: TokenKind, message: &str) -> Option<EffectFactor> {
        let start = self.required(keyword, message)?.span;
        let value = self.required_any(
            &[TokenKind::Integer, TokenKind::Decimal],
            "expected a number",
        )?;
        let Ok(value_number) = value.text.parse::<f32>() else {
            self.diagnostics
                .push(Diagnostic::syntax("value is out of range", value.span));
            return None;
        };
        Some(EffectFactor {
            value: value_number,
            span: start.cover(value.span),
        })
    }

    /// Several independently scheduled `use` layers, mixed into the same
    /// logical track. Each layer reuses the full `play` pipeline grammar
    /// (`trigger_with`, `gate`, `gain`, `pan`, `chance`, ...), so layers are
    /// as expressive as a single-instrument track.
    fn layer_body(&mut self) -> Option<TrackBody> {
        let start = self
            .required(
                TokenKind::Layer,
                "expected `instrument` or `layer` in track",
            )?
            .span;
        self.required(TokenKind::LeftBrace, "expected `{` after `layer`")?;
        let mut uses = Vec::new();
        while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
            if let Some(layer_use) = self.layer_use() {
                uses.push(layer_use);
            } else {
                self.recover_to(LAYER_USE_START);
            }
        }
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close layer")?
            .span;
        Some(TrackBody::Layers {
            uses,
            span: start.cover(end),
        })
    }

    fn layer_use(&mut self) -> Option<LayerUse> {
        let start = self
            .required(TokenKind::Use, "expected `use` in layer")?
            .span;
        let instrument = self.identifier("expected an instrument name after `use`")?;
        self.required(TokenKind::LeftBrace, "expected `{` after `use` instrument")?;
        let play = self.play()?;
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close `use`")?
            .span;
        Some(LayerUse {
            instrument,
            play,
            span: start.cover(end),
        })
    }

    fn volume(&mut self) -> Option<VolumeExpression> {
        let start = self.bump().span;
        let sign = if self.at(TokenKind::Minus) {
            self.bump();
            -1.0
        } else {
            if self.at(TokenKind::Plus) {
                self.bump();
            }
            1.0
        };
        let value = self.required_any(
            &[TokenKind::Integer, TokenKind::Decimal],
            "expected a number after `volume`",
        )?;
        let Ok(decibels) = value.text.parse::<f32>() else {
            self.diagnostics
                .push(Diagnostic::syntax("volume is out of range", value.span));
            return None;
        };
        let unit = self.identifier("expected `db` after volume")?;
        Some(VolumeExpression {
            decibels: sign * decibels,
            span: start.cover(unit.span),
            unit,
        })
    }

    /// `master { limiter { ceiling C } }`. `limiter` is the only accepted
    /// master-processor kind so far.
    fn master(&mut self) -> Option<MasterDeclaration> {
        let start = self.bump().span;
        self.required(TokenKind::LeftBrace, "expected `{` after `master`")?;
        self.required(TokenKind::Limiter, "expected `limiter` in master")?;
        self.required(TokenKind::LeftBrace, "expected `{` after `limiter`")?;
        let ceiling = self.ceiling()?;
        self.required(TokenKind::RightBrace, "expected `}` to close limiter")?;
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close master")?
            .span;
        Some(MasterDeclaration {
            ceiling,
            span: start.cover(end),
        })
    }

    fn ceiling(&mut self) -> Option<VolumeExpression> {
        let start = self
            .required(TokenKind::Ceiling, "expected `ceiling` in limiter")?
            .span;
        let sign = if self.at(TokenKind::Minus) {
            self.bump();
            -1.0
        } else {
            if self.at(TokenKind::Plus) {
                self.bump();
            }
            1.0
        };
        let value = self.required_any(
            &[TokenKind::Integer, TokenKind::Decimal],
            "expected a number after `ceiling`",
        )?;
        let Ok(decibels) = value.text.parse::<f32>() else {
            self.diagnostics
                .push(Diagnostic::syntax("ceiling is out of range", value.span));
            return None;
        };
        let unit = self.identifier("expected `db` after ceiling")?;
        Some(VolumeExpression {
            decibels: sign * decibels,
            span: start.cover(unit.span),
            unit,
        })
    }

    fn play(&mut self) -> Option<PlayStatement> {
        let at = if self.at(TokenKind::At) {
            Some(self.at_expression()?)
        } else {
            None
        };
        let keyword_span = self
            .required(TokenKind::Play, "expected `play` in track")?
            .span;
        let play_start = at.as_ref().map_or(keyword_span, |at| at.span);
        let source = if self.at(TokenKind::Drum) {
            self.play_drum_source()?
        } else {
            PlaySource::Pattern(self.identifier("expected a pattern name after `play`")?)
        };
        let mut trigger_with = None;
        let mut gate = None;
        let mut transpose = None;
        let mut gain = None;
        let mut repeat = None;
        let mut reverse = false;
        let mut pan = None;
        let mut chance = None;
        let mut speed = None;
        let mut choose_sample = None;
        let mut play_end = source.span();
        while self.at(TokenKind::PipeGreater) {
            self.bump();
            match self.current().kind {
                TokenKind::TriggerWith => play_end = self.trigger_with(&mut trigger_with)?,
                TokenKind::Gate => play_end = self.gate(&mut gate)?,
                TokenKind::Transpose => play_end = self.play_transpose(&mut transpose)?,
                TokenKind::Gain => play_end = self.play_gain(&mut gain)?,
                TokenKind::Repeat => play_end = self.play_repeat(&mut repeat)?,
                TokenKind::Reverse => {
                    let span = self.reverse(&mut reverse);
                    play_end = span;
                }
                TokenKind::Pan => play_end = self.play_pan(&mut pan)?,
                TokenKind::Chance => play_end = self.chance(&mut chance)?,
                TokenKind::Speed => play_end = self.speed(&mut speed)?,
                TokenKind::Alternate => play_end = self.alternate_speed(&mut speed)?,
                TokenKind::ChooseSample => {
                    play_end = self.choose_sample(&mut choose_sample)?;
                }
                _ => {
                    self.error(
                        "expected `trigger_with`, `gate`, `transpose`, `gain`, `repeat`, `reverse`, `pan`, `chance`, `speed`, `alternate`, or `choose_sample` after `|>`",
                    );
                    return None;
                }
            }
        }
        Some(PlayStatement {
            at,
            source,
            trigger_with,
            gate,
            transpose,
            gain,
            repeat,
            reverse,
            pan,
            chance,
            speed,
            choose_sample,
            span: play_start.cover(play_end),
        })
    }

    fn at_expression(&mut self) -> Option<AtExpression> {
        let start = self.bump().span;
        let bar_token = self.required(TokenKind::Integer, "expected a bar number after `at`")?;
        self.required(TokenKind::Colon, "expected `:` in `at` position")?;
        let beat_token = self.required(TokenKind::Integer, "expected a beat number after `:`")?;
        let bar = self.parse_u32(&bar_token)?;
        let beat = self.parse_u32(&beat_token)?;
        Some(AtExpression {
            bar,
            beat,
            span: start.cover(beat_token.span),
        })
    }

    fn play_transpose(
        &mut self,
        transpose: &mut Option<TransposeExpression>,
    ) -> Option<SourceSpan> {
        let expression = self.transpose()?;
        let span = expression.span;
        if transpose.replace(expression).is_some() {
            self.diagnostics.push(Diagnostic::syntax(
                "`transpose` may only appear once in a play pipeline",
                span,
            ));
        }
        Some(span)
    }

    fn play_gain(&mut self, gain: &mut Option<GainExpression>) -> Option<SourceSpan> {
        let expression = self.gain()?;
        if gain.replace(expression).is_some() {
            self.diagnostics.push(Diagnostic::syntax(
                "`gain` may only appear once in a play pipeline",
                expression.span,
            ));
        }
        Some(expression.span)
    }

    fn play_repeat(&mut self, repeat: &mut Option<RepeatExpression>) -> Option<SourceSpan> {
        let expression = self.repeat()?;
        if repeat.replace(expression).is_some() {
            self.diagnostics.push(Diagnostic::syntax(
                "`repeat` may only appear once in a play pipeline",
                expression.span,
            ));
        }
        Some(expression.span)
    }

    fn play_pan(&mut self, pan: &mut Option<PanExpression>) -> Option<SourceSpan> {
        let expression = self.pan()?;
        if pan.replace(expression).is_some() {
            self.diagnostics.push(Diagnostic::syntax(
                "`pan` may only appear once in a play pipeline",
                expression.span(),
            ));
        }
        Some(expression.span())
    }

    fn play_drum_source(&mut self) -> Option<PlaySource> {
        let drum_start = self.bump().span;
        let name = self.string("expected a quoted drum voice name after `drum`")?;
        self.required(TokenKind::With, "expected `with` after drum voice name")?;
        let rhythm = self.identifier("expected a rhythm name after `with`")?;
        let span = drum_start.cover(rhythm.span);
        Some(PlaySource::Drum { name, rhythm, span })
    }

    fn speed(&mut self, speed: &mut Option<SpeedExpression>) -> Option<SourceSpan> {
        let (factor, span) = self.speed_factor()?;
        if speed
            .replace(SpeedExpression::Fixed { factor, span })
            .is_some()
        {
            self.diagnostics.push(Diagnostic::syntax(
                "playback speed may only appear once in a play pipeline",
                span,
            ));
        }
        Some(span)
    }

    fn alternate_speed(&mut self, speed: &mut Option<SpeedExpression>) -> Option<SourceSpan> {
        let start = self.bump().span;
        self.required(TokenKind::LeftBrace, "expected `{` after `alternate`")?;
        let (first_factor, _) = self.speed_factor()?;
        let (second_factor, _) = self.speed_factor()?;
        let end = self
            .required(TokenKind::RightBrace, "expected `}` after alternate block")?
            .span;
        let span = start.cover(end);
        if speed
            .replace(SpeedExpression::Alternate {
                first_factor,
                second_factor,
                span,
            })
            .is_some()
        {
            self.diagnostics.push(Diagnostic::syntax(
                "playback speed may only appear once in a play pipeline",
                span,
            ));
        }
        Some(span)
    }

    fn speed_factor(&mut self) -> Option<(f32, SourceSpan)> {
        let start = self
            .required(TokenKind::Speed, "expected `speed` in alternate block")?
            .span;
        let value = self.required_any(
            &[TokenKind::Integer, TokenKind::Decimal],
            "expected a number after `speed`",
        )?;
        let Ok(factor) = value.text.parse::<f32>() else {
            self.diagnostics
                .push(Diagnostic::syntax("speed is out of range", value.span));
            return None;
        };
        let span = start.cover(value.span);
        Some((factor, span))
    }

    fn chance(&mut self, chance: &mut Option<ChanceExpression>) -> Option<SourceSpan> {
        let start = self.bump().span;
        let percent = self.unsigned_percentage("expected a percentage after `chance`")?;
        self.required(TokenKind::LeftBrace, "expected `{` after chance percentage")?;
        let transform = match self.current().kind {
            TokenKind::Transpose => ChanceTransformExpression::Transpose(self.transpose()?),
            TokenKind::Retrigger => {
                let (count, span) = self.retrigger_count()?;
                ChanceTransformExpression::Retrigger { count, span }
            }
            TokenKind::Speed => {
                let (factor, span) = self.speed_factor()?;
                ChanceTransformExpression::Speed { factor, span }
            }
            _ => {
                self.error("expected `transpose`, `retrigger`, or `speed` in chance block");
                return None;
            }
        };
        let end = self
            .required(TokenKind::RightBrace, "expected `}` after chance block")?
            .span;
        let span = start.cover(end);
        let expression = ChanceExpression {
            percent,
            transform,
            span,
        };
        if chance.replace(expression).is_some() {
            self.diagnostics.push(Diagnostic::syntax(
                "`chance` may only appear once in a play pipeline",
                span,
            ));
        }
        Some(span)
    }

    fn choose_sample(
        &mut self,
        choose_sample: &mut Option<ChooseSampleExpression>,
    ) -> Option<SourceSpan> {
        let start_span = self.bump().span;
        let start_token = self.required(
            TokenKind::Integer,
            "expected a sample index after `choose_sample`",
        )?;
        let start = self.parse_u32(&start_token)?;
        self.required(TokenKind::DotDot, "expected `..` in choose_sample range")?;
        let end_token = self.required(TokenKind::Integer, "expected a sample index after `..`")?;
        let end = self.parse_u32(&end_token)?;
        let span = start_span.cover(end_token.span);
        let expression = ChooseSampleExpression { start, end, span };
        if choose_sample.replace(expression).is_some() {
            self.diagnostics.push(Diagnostic::syntax(
                "`choose_sample` may only appear once in a play pipeline",
                span,
            ));
        }
        Some(span)
    }

    fn retrigger_count(&mut self) -> Option<(u32, SourceSpan)> {
        let start = self.bump().span;
        let value = self.required(
            TokenKind::Integer,
            "expected an attack count after `retrigger`",
        )?;
        let count = self.parse_u32(&value)?;
        Some((count, start.cover(value.span)))
    }

    fn trigger_with(&mut self, trigger_with: &mut Option<Identifier>) -> Option<SourceSpan> {
        self.bump();
        let reference = self.identifier("expected a rhythm name after `trigger_with`")?;
        let span = reference.span;
        if trigger_with.replace(reference).is_some() {
            self.diagnostics.push(Diagnostic::syntax(
                "`trigger_with` may only appear once in a play pipeline",
                span,
            ));
        }
        Some(span)
    }

    fn gate(&mut self, gate: &mut Option<GateExpression>) -> Option<SourceSpan> {
        let start = self.bump().span;
        let value = self.required(TokenKind::Integer, "expected a percentage after `gate`")?;
        let percent = self.parse_u32(&value)?;
        let end = self
            .required(TokenKind::Percent, "expected `%` after gate percentage")?
            .span;
        let expression = GateExpression {
            percent,
            span: start.cover(end),
        };
        if gate.replace(expression).is_some() {
            self.diagnostics.push(Diagnostic::syntax(
                "`gate` may only appear once in a play pipeline",
                expression.span,
            ));
        }
        Some(expression.span)
    }

    fn pan(&mut self) -> Option<PanExpression> {
        let start = self.bump().span;
        if self.at(TokenKind::Alternate) {
            return self.alternate_pan(start);
        }
        let sign = if self.at(TokenKind::Minus) {
            self.bump();
            -1
        } else {
            if self.at(TokenKind::Plus) {
                self.bump();
            }
            1
        };
        let value = self.required(TokenKind::Integer, "expected a percentage after `pan`")?;
        let magnitude = self.parse_u32(&value)?;
        let Ok(magnitude) = i32::try_from(magnitude) else {
            self.diagnostics
                .push(Diagnostic::syntax("pan is out of range", value.span));
            return None;
        };
        let end = self
            .required(TokenKind::Percent, "expected `%` after pan percentage")?
            .span;
        Some(PanExpression::Fixed {
            percent: sign * magnitude,
            span: start.cover(end),
        })
    }

    fn alternate_pan(&mut self, start: SourceSpan) -> Option<PanExpression> {
        self.bump();
        self.required(TokenKind::LeftParen, "expected `(` after `alternate`")?;
        let left_percent = self.unsigned_percentage("expected left pan percentage")?;
        self.required(TokenKind::Comma, "expected `,` between pan percentages")?;
        let right_percent = self.unsigned_percentage("expected right pan percentage")?;
        let end = self
            .required(TokenKind::RightParen, "expected `)` after pan percentages")?
            .span;
        Some(PanExpression::Alternate {
            left_percent,
            right_percent,
            span: start.cover(end),
        })
    }

    fn unsigned_percentage(&mut self, message: &str) -> Option<u32> {
        let value = self.required(TokenKind::Integer, message)?;
        let percent = self.parse_u32(&value)?;
        self.required(TokenKind::Percent, "expected `%` after percentage")?;
        Some(percent)
    }

    fn reverse(&mut self, seen: &mut bool) -> SourceSpan {
        let span = self.bump().span;
        if std::mem::replace(seen, true) {
            self.diagnostics.push(Diagnostic::syntax(
                "`reverse` may only appear once in a play pipeline",
                span,
            ));
        }
        span
    }

    fn transpose(&mut self) -> Option<TransposeExpression> {
        let start = self.bump().span;
        let sign = if self.at(TokenKind::Minus) {
            self.bump();
            -1
        } else {
            if self.at(TokenKind::Plus) {
                self.bump();
            }
            1
        };
        let value = self.required(
            TokenKind::Integer,
            "expected a semitone count after `transpose`",
        )?;
        let magnitude = self.parse_u32(&value)?;
        let Ok(magnitude) = i32::try_from(magnitude) else {
            self.diagnostics.push(Diagnostic::syntax(
                "semitone count is out of range",
                value.span,
            ));
            return None;
        };
        let unit = self.identifier("expected `st` after semitone count")?;
        Some(TransposeExpression {
            semitones: sign * magnitude,
            span: start.cover(unit.span),
            unit,
        })
    }

    fn gain(&mut self) -> Option<GainExpression> {
        let start = self.bump().span;
        let value = self.required_any(
            &[TokenKind::Integer, TokenKind::Decimal],
            "expected a number after `gain`",
        )?;
        let Ok(factor) = value.text.parse::<f32>() else {
            self.diagnostics
                .push(Diagnostic::syntax("gain is out of range", value.span));
            return None;
        };
        Some(GainExpression {
            factor,
            span: start.cover(value.span),
        })
    }

    fn repeat(&mut self) -> Option<RepeatExpression> {
        let start = self.bump().span;
        let value = self.required(TokenKind::Integer, "expected a count after `repeat`")?;
        Some(RepeatExpression {
            count: self.parse_u32(&value)?,
            span: start.cover(value.span),
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
                            self.parse_u32(&index).map(|index_value| {
                                let velocity = self.velocity();
                                let end = velocity.map_or(index.span, |velocity| velocity.span);
                                StepItem::Sample {
                                    index: index_value,
                                    velocity,
                                    span: sample.cover(end),
                                }
                            })
                        })
                }
                TokenKind::Drum => {
                    let drum = self.bump().span;
                    self.string("expected a quoted drum voice name")
                        .map(|name| {
                            let velocity = self.velocity();
                            let end = velocity.map_or(name.span, |velocity| velocity.span);
                            StepItem::Drum {
                                span: drum.cover(end),
                                name,
                                velocity,
                            }
                        })
                }
                TokenKind::Rest => Some(StepItem::Rest {
                    span: self.bump().span,
                }),
                TokenKind::Choose => self.choice(),
                _ => {
                    self.error("expected a degree, sample, drum, rest, or choose in steps");
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
            let (selectors, default_weight_span) = match self.current().kind {
                TokenKind::Sample => {
                    self.bump();
                    let index = self.required(TokenKind::Integer, "expected sample index")?;
                    let span = index.span;
                    (
                        vec![SampleSelectorExpression::Index(self.parse_u32(&index)?)],
                        span,
                    )
                }
                TokenKind::Drum => {
                    self.bump();
                    let name = self.string("expected a quoted drum voice name")?;
                    let span = name.span;
                    (vec![SampleSelectorExpression::Named(name)], span)
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
                    let mut selectors = Vec::new();
                    while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
                        match self.current().kind {
                            TokenKind::Sample => {
                                self.bump();
                                let index =
                                    self.required(TokenKind::Integer, "expected sample index")?;
                                selectors
                                    .push(SampleSelectorExpression::Index(self.parse_u32(&index)?));
                            }
                            TokenKind::Drum => {
                                self.bump();
                                let name = self.string("expected a quoted drum voice name")?;
                                selectors.push(SampleSelectorExpression::Named(name));
                            }
                            _ => {
                                self.error("expected `sample` or `drum` in choice sequence");
                                return None;
                            }
                        }
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
                        selectors,
                        weight: self.parse_u32(&weight)?,
                        span: alternative_start.cover(end),
                    });
                    continue;
                }
                _ => {
                    self.error("expected `sample`, `drum`, or `sequence` in choose");
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
                selectors,
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
        let duration = self.duration_expr("note")?;
        let velocity = self.velocity();
        let end = velocity.map_or(duration.span(), |velocity| velocity.span);
        Some(NoteExpression {
            pitch,
            duration,
            velocity,
            span: start.cover(end),
        })
    }

    fn rest(&mut self) -> Option<RestExpression> {
        let start = self.bump().span;
        self.required(TokenKind::For, "expected `for` after `rest`")?;
        let duration = self.duration_expr("rest")?;
        Some(RestExpression {
            span: start.cover(duration.span()),
            duration,
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
        let duration = self.duration_expr("chord")?;
        let velocity = self.velocity();
        let end = velocity.map_or(duration.span(), |velocity| velocity.span);
        Some(ChordExpression {
            pitches,
            duration,
            velocity,
            span: start.cover(end),
        })
    }

    /// Parses a note/chord/rest duration: either `N/M`, a fraction of a
    /// whole note, or `Nbar`, resolved against the song's meter during HIR
    /// lowering (mirroring how rate literals accept optional whitespace
    /// between the number and its unit).
    fn duration_expr(&mut self, context: &str) -> Option<DurationExpression> {
        let numerator_token = self.required(TokenKind::Integer, "expected duration numerator")?;
        if self.at(TokenKind::Bar) {
            let bar_token = self.bump();
            return Some(DurationExpression::Bars {
                count: self.parse_u32(&numerator_token)?,
                span: numerator_token.span.cover(bar_token.span),
            });
        }
        self.required(
            TokenKind::Slash,
            &format!("expected `/` in {context} duration"),
        )?;
        let denominator_token =
            self.required(TokenKind::Integer, "expected duration denominator")?;
        Some(DurationExpression::Fraction {
            numerator: self.parse_u32(&numerator_token)?,
            denominator: self.parse_u32(&denominator_token)?,
            span: numerator_token.span.cover(denominator_token.span),
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
