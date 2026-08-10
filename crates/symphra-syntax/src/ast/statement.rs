use super::{
    DurationExpression, FrequencyLiteral, Identifier, PatternDeclaration, QuotedString, RateLiteral,
};
use crate::SourceSpan;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RhythmDeclaration {
    pub name: Identifier,
    pub resolution_numerator: u32,
    pub resolution_denominator: u32,
    pub items: Vec<RhythmItem>,
    pub span: SourceSpan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RhythmItem {
    Hit { span: SourceSpan },
    Rest { span: SourceSpan },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackDeclaration {
    pub name: Identifier,
    pub role: Identifier,
    pub volume: Option<Box<VolumeExpression>>,
    pub body: TrackBody,
    pub effect: Option<EffectDeclaration>,
    pub span: SourceSpan,
}

/// `effect delay { mix M time T feedback F }`, applied to the track's
/// rendered audio (after every layer's events have been synthesized) before
/// it is summed into the master mix. `delay` is the only effect kind
/// implemented so far; `filter`/`reverb` and general `automate` blocks are
/// not part of this slice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectDeclaration {
    pub mix: EffectFactor,
    pub time: DurationExpression,
    pub feedback: EffectFactor,
    pub span: SourceSpan,
}

/// A dimensionless 0.0-to-1.0-ish ratio, such as `mix`/`feedback`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectFactor {
    pub value: f32,
    pub span: SourceSpan,
}

/// A track plays either one instrument directly, or several `use` layers
/// that are each scheduled independently and mixed into the same logical
/// track (sharing the track's `role` and `volume`).
#[derive(Clone, Debug, PartialEq)]
pub enum TrackBody {
    Single {
        instrument: Identifier,
        play: Box<PlayStatement>,
    },
    Layers {
        uses: Vec<LayerUse>,
        span: SourceSpan,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayerUse {
    pub instrument: Identifier,
    pub play: PlayStatement,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayStatement {
    pub at: Option<AtExpression>,
    pub source: PlaySource,
    pub trigger_with: Option<Identifier>,
    pub gate: Option<GateExpression>,
    pub transpose: Option<TransposeExpression>,
    pub gain: Option<GainExpression>,
    pub repeat: Option<RepeatExpression>,
    pub reverse: bool,
    pub pan: Option<PanExpression>,
    pub chance: Option<ChanceExpression>,
    pub speed: Option<SpeedExpression>,
    pub choose_sample: Option<ChooseSampleExpression>,
    pub span: SourceSpan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtExpression {
    pub bar: u32,
    pub beat: u32,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaySource {
    Pattern(Identifier),
    Drum {
        name: QuotedString,
        rhythm: Identifier,
        span: SourceSpan,
    },
}

impl PlaySource {
    #[must_use]
    pub fn span(&self) -> SourceSpan {
        match self {
            Self::Pattern(identifier) => identifier.span,
            Self::Drum { span, .. } => *span,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChooseSampleExpression {
    pub start: u32,
    pub end: u32,
    pub span: SourceSpan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GateExpression {
    pub percent: u32,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransposeExpression {
    pub semitones: i32,
    pub unit: Identifier,
    pub span: SourceSpan,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GainExpression {
    pub factor: f32,
    pub span: SourceSpan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepeatExpression {
    pub count: u32,
    pub span: SourceSpan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanExpression {
    Fixed {
        percent: i32,
        span: SourceSpan,
    },
    Alternate {
        left_percent: u32,
        right_percent: u32,
        span: SourceSpan,
    },
}

impl PanExpression {
    #[must_use]
    pub const fn span(self) -> SourceSpan {
        match self {
            Self::Fixed { span, .. } | Self::Alternate { span, .. } => span,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChanceExpression {
    pub percent: u32,
    pub transform: ChanceTransformExpression,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ChanceTransformExpression {
    Transpose(TransposeExpression),
    Retrigger { count: u32, span: SourceSpan },
    Speed { factor: f32, span: SourceSpan },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpeedExpression {
    Fixed {
        factor: f32,
        span: SourceSpan,
    },
    Alternate {
        first_factor: f32,
        second_factor: f32,
        span: SourceSpan,
    },
}

impl SpeedExpression {
    #[must_use]
    pub const fn span(self) -> SourceSpan {
        match self {
            Self::Fixed { span, .. } | Self::Alternate { span, .. } => span,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VolumeExpression {
    pub decibels: f32,
    pub unit: Identifier,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InstrumentDeclaration {
    pub name: Identifier,
    pub body: InstrumentBody,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InstrumentBody {
    Builtin(Identifier),
    Sampled {
        source: QuotedString,
        root: Identifier,
        span: SourceSpan,
    },
    Sampler {
        pack: QuotedString,
        span: SourceSpan,
    },
    DrumMachine {
        bank: QuotedString,
        span: SourceSpan,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArrangementOccurrence {
    pub pattern: Identifier,
    pub instrument: Option<Identifier>,
    pub span: SourceSpan,
}

/// An `arrangement` entry is either a bare pattern name (with an optional
/// `with <instrument>`, for track-less pattern-only songs), or `play <name>`
/// referencing a declared `section` (for songs with declared tracks).
#[derive(Clone, Debug, PartialEq)]
pub enum ArrangementEntry {
    Pattern(ArrangementOccurrence),
    Play { name: Identifier, span: SourceSpan },
}

impl ArrangementEntry {
    #[must_use]
    pub fn span(&self) -> SourceSpan {
        match self {
            Self::Pattern(occurrence) => occurrence.span,
            Self::Play { span, .. } => *span,
        }
    }
}

/// `section <name> bars <N> { parallel [exact] { play track <name> ... } }`.
/// Declares a named, fixed-length group of declared tracks that can be
/// referenced (and reused) from an `arrangement { play <name> }` entry. The
/// body is always exactly one `parallel` block; `exact` requires every
/// referenced track's scheduled length to equal `bars` precisely.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionDeclaration {
    pub name: Identifier,
    pub bars: u32,
    pub exact: bool,
    pub tracks: Vec<Identifier>,
    pub span: SourceSpan,
}

/// `master { limiter { ceiling C } }`. `limiter` is the only accepted
/// master-processor kind so far (mirroring how `delay` is the only accepted
/// `effect` kind) — flattened directly onto `MasterDeclaration` rather than
/// a separate `LimiterDeclaration` layer. `ceiling` reuses `VolumeExpression`
/// verbatim: identical signed-decibel-with-unit grammar to `volume -6db`.
#[derive(Clone, Debug, PartialEq)]
pub struct MasterDeclaration {
    pub ceiling: VolumeExpression,
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
    Rhythm(RhythmDeclaration),
    Track(Box<TrackDeclaration>),
    Section(SectionDeclaration),
    Master(MasterDeclaration),
    Arrangement {
        entries: Vec<ArrangementEntry>,
        span: SourceSpan,
    },
    Pattern(PatternDeclaration),
}
