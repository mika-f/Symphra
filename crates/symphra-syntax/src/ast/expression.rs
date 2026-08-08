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
        notes: Vec<NoteExpression>,
        span: SourceSpan,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct NoteExpression {
    pub pitch: Identifier,
    pub duration_numerator: u32,
    pub duration_denominator: u32,
    pub span: SourceSpan,
}
