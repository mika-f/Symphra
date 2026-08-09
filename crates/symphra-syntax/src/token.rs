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
    Instrument,
    Pattern,
    Arrangement,
    With,
    Sequence,
    Note,
    Chord,
    Rest,
    For,
    Velocity,
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
            "instrument" => Self::Instrument,
            "pattern" => Self::Pattern,
            "arrangement" => Self::Arrangement,
            "with" => Self::With,
            "sequence" => Self::Sequence,
            "note" => Self::Note,
            "chord" => Self::Chord,
            "rest" => Self::Rest,
            "for" => Self::For,
            "velocity" => Self::Velocity,
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
