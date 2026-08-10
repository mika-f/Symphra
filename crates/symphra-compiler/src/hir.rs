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
    pub sections: Vec<Section>,
    pub arrangement: Option<Arrangement>,
    pub master: Option<MasterLimiter>,
}

/// `master { limiter { ceiling C } }`. `ceiling` is a linear amplitude
/// (already converted from dB), the loudest sample the renderer's
/// peak-detect-and-scale limiter allows through.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MasterLimiter {
    pub ceiling: f32,
}

/// `section <name> bars <N> { parallel [exact] { play track <name> ... } }`.
/// A named, fixed-length (`bars`, a whole-note-fraction `Duration`) group of
/// declared tracks, referenced (and possibly reused) from
/// `arrangement { play <name> }`. `exact` requires every referenced track's
/// scheduled length to equal `bars` precisely, checked at schedule time since
/// a track's true post-pipeline length is not known until scheduling runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub id: NodeId,
    pub name: String,
    pub bars: Duration,
    pub exact: bool,
    pub tracks: Vec<NodeId>,
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
    pub at: Option<Duration>,
    pub effect: Option<Effect>,
}

/// A track's single post-render effect: a feedback delay, a resonant
/// lowpass filter, or a reverb. `delay`, `filter`, and `reverb` are the
/// only kinds implemented so far.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    Delay(DelayEffect),
    Filter(FilterEffect),
    Reverb(ReverbEffect),
}

/// A feedback delay line applied to the track's rendered audio. `time` is a
/// duration (fraction of a whole note), resolved to sample frames using the
/// song's tempo at render time, the same way note/sample durations are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DelayEffect {
    pub mix: f32,
    pub time: Duration,
    pub feedback: f32,
}

/// A resonant lowpass filter applied to the track's rendered audio.
/// `cutoff_hz` is already resolved from the source `hz`/`khz` literal to
/// plain hertz; converting it (together with the render sample rate) into
/// biquad filter coefficients happens in `symphra-dsp`, the same boundary
/// where `time` becomes sample frames for [`DelayEffect`]. When `automation`
/// is present, `cutoff_hz` is superseded by the LFO's swept value at render
/// time — the same "override always wins" pattern `chance { speed F }`
/// already established over the base `speed` stage — rather than being
/// removed from the grammar, so `effect filter { cutoff C ... }` keeps
/// working unchanged whether or not a track also has `automate cutoff`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilterEffect {
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub automation: Option<FilterAutomation>,
}

/// `automate cutoff { lfo <waveform> { range A..B rate N cycles/bar } }`.
/// `cycles_per_bar` is left tempo/meter-agnostic here (like `Duration`) and
/// resolved to an LFO frequency in Hz only at render time, once both the
/// song's tempo and sample rate are known — the same boundary `time`/
/// `cutoff_hz` already cross for [`DelayEffect`]/[`FilterEffect`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilterAutomation {
    pub waveform: LfoWaveform,
    pub range_start_hz: f32,
    pub range_end_hz: f32,
    pub cycles_per_bar: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LfoWaveform {
    Sine,
    Triangle,
}

/// A Schroeder reverberator applied to the track's rendered audio. `size`
/// is the raw `0.0..=1.0` factor; mapping it to comb filter feedback (and
/// picking the fixed comb/allpass delay times) happens in `symphra-dsp`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReverbEffect {
    pub mix: f32,
    pub size: f32,
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

/// A song's arrangement is either a sequence of bare pattern occurrences
/// (track-less, pattern-only songs — the original form), or a sequence of
/// section references (songs with declared tracks + declared sections). The
/// two never mix within one song.
#[derive(Clone, Debug, PartialEq)]
pub enum Arrangement {
    Patterns(Vec<PatternOccurrence>),
    Sections(Vec<SectionOccurrence>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PatternOccurrence {
    pub id: NodeId,
    pub pattern: NodeId,
    pub instrument: InstrumentKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionOccurrence {
    pub id: NodeId,
    pub section: NodeId,
}

/// `envelope { attack Ams decay Dms sustain S release Rms }`, replacing an
/// oscillator instrument's fixed edge fade with a real ADSR amplitude
/// shape. `attack_ms`/`decay_ms`/`release_ms` stay tempo-agnostic
/// milliseconds — like [`DelayEffect`]'s `time`, they are not converted to
/// sample frames until render time, since only the renderer knows the
/// sample rate. `sustain` is a `0.0..=1.0` level, not a duration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Envelope {
    pub attack_ms: f32,
    pub decay_ms: f32,
    pub sustain: f32,
    pub release_ms: f32,
}

/// `sine`/`triangle`, or `synth supersaw { voices N detune D spread S }` —
/// `voices` detuned sawtooth oscillators mixed together. Each accepts an
/// optional `envelope`; `None` keeps the renderer's pre-existing fixed edge
/// fade, so a bare `instrument x = sine` (with no envelope) renders exactly
/// as before this feature existed.
#[derive(Clone, Debug, PartialEq)]
pub enum InstrumentKind {
    Sine {
        envelope: Option<Envelope>,
    },
    Triangle {
        envelope: Option<Envelope>,
    },
    Supersaw {
        voices: u32,
        detune: f32,
        spread: f32,
        envelope: Option<Envelope>,
    },
    Sampled {
        source: String,
        root_midi: u8,
    },
    Sampler {
        pack: String,
    },
    DrumMachine {
        bank: String,
    },
    SoundFont {
        source: String,
        preset: String,
    },
    Vst3 {
        source: String,
        preset: Option<String>,
    },
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
