use crate::SourceSpan;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Identifier,
    Integer,
    Decimal,
    String,
    Project,
    Song,
    Seed,
    SampleRate,
    Output,
    Tempo,
    Meter,
    Key,
    Pattern,
    Sequence,
    Note,
    For,
    LeftBrace,
    RightBrace,
    Equal,
    Slash,
    Eof,
}

impl TokenKind {
    pub(crate) fn keyword(text: &str) -> Option<Self> {
        Some(match text {
            "project" => Self::Project,
            "song" => Self::Song,
            "seed" => Self::Seed,
            "sample_rate" => Self::SampleRate,
            "output" => Self::Output,
            "tempo" => Self::Tempo,
            "meter" => Self::Meter,
            "key" => Self::Key,
            "pattern" => Self::Pattern,
            "sequence" => Self::Sequence,
            "note" => Self::Note,
            "for" => Self::For,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub text: String,
    pub span: SourceSpan,
}
