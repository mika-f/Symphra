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
    Sample,
    Choose,
    Weight,
    Sampled,
    Sampler,
    Source,
    Root,
    Pack,
    Pattern,
    Arrangement,
    With,
    Sequence,
    Steps,
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
            "sample" => Self::Sample,
            "choose" => Self::Choose,
            "weight" => Self::Weight,
            "sampled" => Self::Sampled,
            "sampler" => Self::Sampler,
            "source" => Self::Source,
            "root" => Self::Root,
            "pack" => Self::Pack,
            "pattern" => Self::Pattern,
            "arrangement" => Self::Arrangement,
            "with" => Self::With,
            "sequence" => Self::Sequence,
            "steps" => Self::Steps,
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
