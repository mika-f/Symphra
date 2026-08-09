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
    pub patterns: Vec<Pattern>,
    pub arrangement: Option<Arrangement>,
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
    Rest(Rest),
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
