use super::{Identifier, QuotedString};
use crate::SourceSpan;

#[derive(Clone, Debug, PartialEq)]
pub struct PatternDeclaration {
    pub name: Identifier,
    pub body: PatternBody,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PatternBody {
    Sequence {
        items: Vec<SequenceItem>,
        span: SourceSpan,
    },
    Steps {
        resolution_numerator: u32,
        resolution_denominator: u32,
        items: Vec<StepItem>,
        span: SourceSpan,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepItem {
    Degree {
        degree: u32,
        octave: u32,
        span: SourceSpan,
    },
    Sample {
        index: u32,
        span: SourceSpan,
    },
    Drum {
        name: QuotedString,
        span: SourceSpan,
    },
    Rest {
        span: SourceSpan,
    },
    Choose {
        alternatives: Vec<SampleChoiceAlternative>,
        span: SourceSpan,
    },
    ChooseDegrees {
        alternatives: Vec<DegreeChoiceAlternative>,
        span: SourceSpan,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DegreeChoiceAlternative {
    pub degree: u32,
    pub octave: u32,
    pub weight: u32,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleChoiceAlternative {
    pub selectors: Vec<SampleSelectorExpression>,
    pub weight: u32,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SampleSelectorExpression {
    Index(u32),
    Named(QuotedString),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SequenceItem {
    Note(NoteExpression),
    Chord(ChordExpression),
    Rest(RestExpression),
}

#[derive(Clone, Debug, PartialEq)]
pub struct NoteExpression {
    pub pitch: Identifier,
    pub duration_numerator: u32,
    pub duration_denominator: u32,
    pub velocity: Option<VelocityExpression>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChordExpression {
    pub pitches: Vec<Identifier>,
    pub duration_numerator: u32,
    pub duration_denominator: u32,
    pub velocity: Option<VelocityExpression>,
    pub span: SourceSpan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VelocityExpression {
    pub value: u32,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestExpression {
    pub duration_numerator: u32,
    pub duration_denominator: u32,
    pub span: SourceSpan,
}
