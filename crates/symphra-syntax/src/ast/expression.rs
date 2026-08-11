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
        /// `sequence step 1/8 { ... }`: the duration items take when they
        /// omit `for`. Absent, every item states its own duration.
        step: Option<DurationExpression>,
        items: Vec<SequenceItem>,
        span: SourceSpan,
    },
    Steps {
        resolution: DurationExpression,
        items: Vec<StepItem>,
        span: SourceSpan,
    },
}

/// A repeated item: `item * N`, or a parenthesised group `(a, b) * N` whose
/// elements repeat as a unit.
///
/// Repetition is sugar — it lowers to `count` copies of `items`, in order —
/// but it is kept in the AST rather than expanded by the parser so that
/// `symphra-fmt` can reprint what the author wrote instead of the expansion.
/// `count` is always at least 1, and `items` always holds at least one item;
/// both are enforced by the parser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepeatGroup<T> {
    pub items: Vec<T>,
    pub count: u32,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepItem {
    /// `drum "hh" velocity 38 * 4`, or `(rest, drum "cp") * 2`. Block-shaped
    /// items (`choose`) are not repeatable — see the parser.
    Repeat(RepeatGroup<StepItem>),
    /// `[ a b c ]`: the items share one grid cell, splitting it evenly, so
    /// each lasts `resolution / 3` here. Subdivisions nest, and each level
    /// divides the cell it sits in.
    Subdivide {
        items: Vec<StepItem>,
        span: SourceSpan,
    },
    Degree {
        degree: u32,
        octave: u32,
        span: SourceSpan,
    },
    Sample {
        index: u32,
        velocity: Option<VelocityExpression>,
        span: SourceSpan,
    },
    Drum {
        name: QuotedString,
        velocity: Option<VelocityExpression>,
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
    /// `note C4 for 1/8 * 4`, or `(note C4 for 1/8, rest for 1/8) * 2`.
    Repeat(RepeatGroup<SequenceItem>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct NoteExpression {
    pub pitch: Identifier,
    /// `None` when the item omits `for` and takes the sequence's `step`.
    pub duration: Option<DurationExpression>,
    pub velocity: Option<VelocityExpression>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChordExpression {
    pub pitches: Vec<Identifier>,
    /// `None` when the item omits `for` and takes the sequence's `step`.
    pub duration: Option<DurationExpression>,
    pub velocity: Option<VelocityExpression>,
    pub span: SourceSpan,
}

/// `velocity 90`, or the ramp form `velocity 70..110`.
///
/// A ramp interpolates linearly across the copies of the repetition that
/// encloses it, so it only means anything under a `* N`; the compiler
/// rejects one that stands alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VelocityExpression {
    pub value: u32,
    pub ramp_to: Option<u32>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestExpression {
    /// `None` when the item omits `for` and takes the sequence's `step`.
    pub duration: Option<DurationExpression>,
    pub span: SourceSpan,
}

/// A note, chord, or rest duration: either an explicit fraction of a whole
/// note, or a meter-relative bar count resolved during HIR lowering once the
/// song's meter is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DurationExpression {
    Fraction {
        numerator: u32,
        denominator: u32,
        span: SourceSpan,
    },
    Bars {
        count: u32,
        span: SourceSpan,
    },
}

impl DurationExpression {
    #[must_use]
    pub fn span(&self) -> SourceSpan {
        match self {
            Self::Fraction { span, .. } | Self::Bars { span, .. } => *span,
        }
    }
}
