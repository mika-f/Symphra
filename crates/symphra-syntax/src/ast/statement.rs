use super::{FrequencyLiteral, Identifier, PatternDeclaration, RateLiteral};
use crate::SourceSpan;

#[derive(Clone, Debug, PartialEq)]
pub struct InstrumentDeclaration {
    pub name: Identifier,
    pub kind: Identifier,
    pub span: SourceSpan,
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
    Arrangement {
        occurrences: Vec<ArrangementOccurrence>,
        span: SourceSpan,
    },
    Pattern(PatternDeclaration),
}
