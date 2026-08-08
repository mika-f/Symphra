use super::{ProjectStatement, QuotedString, SongStatement};
use crate::{SourceId, SourceSpan};

#[derive(Clone, Debug, PartialEq)]
pub struct SourceFile {
    pub source: SourceId,
    pub declarations: Vec<Declaration>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Declaration {
    Project(ProjectDeclaration),
    Song(SongDeclaration),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectDeclaration {
    pub statements: Vec<ProjectStatement>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SongDeclaration {
    pub name: QuotedString,
    pub statements: Vec<SongStatement>,
    pub span: SourceSpan,
}
