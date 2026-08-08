use crate::SourceSpan;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identifier {
    pub text: String,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuotedString {
    /// Contents after removing quotes and interpreting common escapes.
    pub value: String,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NumberLiteral {
    pub value: f64,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RateLiteral {
    pub value: NumberLiteral,
    pub unit: Identifier,
    pub span: SourceSpan,
}

pub type FrequencyLiteral = RateLiteral;
