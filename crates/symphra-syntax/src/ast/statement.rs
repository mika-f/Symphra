use super::{FrequencyLiteral, Identifier, PatternDeclaration, QuotedString, RateLiteral};
use crate::SourceSpan;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RhythmDeclaration {
    pub name: Identifier,
    pub resolution_numerator: u32,
    pub resolution_denominator: u32,
    pub items: Vec<RhythmItem>,
    pub span: SourceSpan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RhythmItem {
    Hit { span: SourceSpan },
    Rest { span: SourceSpan },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackDeclaration {
    pub name: Identifier,
    pub role: Identifier,
    pub instrument: Identifier,
    pub volume: Option<Box<VolumeExpression>>,
    pub play: PlayStatement,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayStatement {
    pub pattern: Identifier,
    pub trigger_with: Option<Identifier>,
    pub gate: Option<GateExpression>,
    pub transpose: Option<TransposeExpression>,
    pub gain: Option<GainExpression>,
    pub repeat: Option<RepeatExpression>,
    pub reverse: bool,
    pub span: SourceSpan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GateExpression {
    pub percent: u32,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransposeExpression {
    pub semitones: i32,
    pub unit: Identifier,
    pub span: SourceSpan,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GainExpression {
    pub factor: f32,
    pub span: SourceSpan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepeatExpression {
    pub count: u32,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VolumeExpression {
    pub decibels: f32,
    pub unit: Identifier,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InstrumentDeclaration {
    pub name: Identifier,
    pub body: InstrumentBody,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InstrumentBody {
    Builtin(Identifier),
    Sampled {
        source: QuotedString,
        root: Identifier,
        span: SourceSpan,
    },
    Sampler {
        pack: QuotedString,
        span: SourceSpan,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArrangementOccurrence {
    pub pattern: Identifier,
    pub instrument: Option<Identifier>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProjectStatement {
    Seed {
        value: u64,
        span: SourceSpan,
    },
    SampleRate {
        value: FrequencyLiteral,
        span: SourceSpan,
    },
    Output {
        channels: Identifier,
        span: SourceSpan,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum SongStatement {
    Tempo {
        value: RateLiteral,
        span: SourceSpan,
    },
    Meter {
        numerator: u32,
        denominator: u32,
        span: SourceSpan,
    },
    Key {
        tonic: Identifier,
        mode: Identifier,
        span: SourceSpan,
    },
    Instrument(InstrumentDeclaration),
    Rhythm(RhythmDeclaration),
    Track(Box<TrackDeclaration>),
    Arrangement {
        occurrences: Vec<ArrangementOccurrence>,
        span: SourceSpan,
    },
    Pattern(PatternDeclaration),
}
