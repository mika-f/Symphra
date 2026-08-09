use super::Identifier;
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
