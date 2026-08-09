//! Language-independent representation produced by the compiler.

/// A deterministic identifier assigned in source order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub project: Project,
    pub songs: Vec<Song>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub seed: u64,
    pub sample_rate_hz: u32,
    pub channels: Channels,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channels {
    Mono,
    Stereo,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    pub id: NodeId,
    pub name: String,
    pub tempo_bpm: f64,
    pub meter: Meter,
    pub key: Key,
    pub rhythms: Vec<Rhythm>,
    pub patterns: Vec<Pattern>,
    pub tracks: Vec<TrackDefinition>,
    pub arrangement: Option<Arrangement>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackDefinition {
    pub id: NodeId,
    pub name: String,
    pub role: String,
    pub instrument: InstrumentKind,
    pub pattern: NodeId,
    pub trigger_with: Option<NodeId>,
    pub gate_percent: Option<u8>,
    pub transpose_semitones: Option<i32>,
    pub gain: f32,
    pub repeat_count: u16,
    pub reverse: bool,
    pub pan: Pan,
    pub chance: Option<Chance>,
    pub speed: Speed,
    pub choose_sample: Option<SampleRange>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chance {
    pub percent: u8,
    pub transform: ChanceTransform,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChanceTransform {
    Transpose(i32),
    Retrigger(u32),
    Speed(f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pan {
    Fixed(i8),
    Alternate { left_percent: i8, right_percent: i8 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Speed {
    Fixed(f32),
    Alternate { first: f32, second: f32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rhythm {
    pub id: NodeId,
    pub name: String,
    pub resolution: Duration,
    pub items: Vec<RhythmItem>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RhythmItem {
    Hit,
    Rest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arrangement {
    pub occurrences: Vec<PatternOccurrence>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternOccurrence {
    pub id: NodeId,
    pub pattern: NodeId,
    pub instrument: InstrumentKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstrumentKind {
    Sine,
    Triangle,
    Sampled { source: String, root_midi: u8 },
    Sampler { pack: String },
    DrumMachine { bank: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Meter {
    pub numerator: u32,
    pub denominator: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub tonic: PitchClass,
    pub mode: Mode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PitchClass {
    C,
    D,
    E,
    F,
    G,
    A,
    B,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Major,
    Minor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    pub id: NodeId,
    pub name: String,
    pub steps: Vec<PatternStep>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternStep {
    Note(Note),
    Chord(Chord),
    Sample(SampleTrigger),
    Choice(SampleChoice),
    DegreeChoice(DegreeChoice),
    Rest(Rest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegreeChoice {
    pub id: NodeId,
    pub alternatives: Vec<WeightedNote>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeightedNote {
    pub note: Note,
    pub weight: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleChoice {
    pub id: NodeId,
    pub alternatives: Vec<WeightedSampleSequence>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeightedSampleSequence {
    pub samples: Vec<SampleTrigger>,
    pub weight: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleTrigger {
    pub id: NodeId,
    pub selector: SampleSelector,
    pub duration: Duration,
    pub velocity: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SampleSelector {
    Index(u32),
    Named(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Note {
    pub id: NodeId,
    pub midi_pitch: u8,
    pub duration: Duration,
    pub velocity: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chord {
    pub notes: Vec<ChordNote>,
    pub duration: Duration,
    pub velocity: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChordNote {
    pub id: NodeId,
    pub midi_pitch: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rest {
    pub id: NodeId,
    pub duration: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Duration {
    pub numerator: u32,
    pub denominator: u32,
}
