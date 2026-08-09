//! Deterministic offline rendering from score events to interleaved PCM.

use std::num::NonZeroU32;

use symphra_dsp::{Oscillator, Waveform, fade_gain};
use symphra_sampler::{SampleLibrary, SamplePlayer, named_sample_source, packed_sample_source};
use symphra_score::{
    Channels, InstrumentKind, MusicalTime, SampleSelector, Score, Song, TimeError,
};

const MAX_NOTE_GAIN: f32 = 0.2;

#[derive(Clone, Debug, PartialEq)]
pub struct AudioBuffer {
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl AudioBuffer {
    #[must_use]
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    #[error("song index is out of range")]
    SongNotFound,
    #[error("tempo must be finite and greater than zero")]
    InvalidTempo,
    #[error("track gain must be finite and greater than or equal to zero")]
    InvalidTrackGain,
    #[error("track pan must be from -100% to 100%")]
    InvalidTrackPan,
    #[error("sample speed must be finite and greater than zero")]
    InvalidSampleSpeed,
    #[error("sample rate must be greater than zero")]
    InvalidSampleRate,
    #[error("rendered audio is too large")]
    AudioTooLarge,
    #[error("sample `{0}` was not loaded")]
    MissingSample(String),
    #[error("sampler pack `{0}` requires sample selection events")]
    SamplerRequiresSampleEvents(String),
    #[error("drum bank `{0}` requires sample selection events")]
    DrumMachineRequiresSampleEvents(String),
    #[error("sample selection events require a sampler or drum machine instrument")]
    SampleEventsRequireSampler,
    #[error(transparent)]
    Time(#[from] TimeError),
}

/// Renders one song from a score into an interleaved PCM buffer.
///
/// # Errors
///
/// Returns [`RenderError`] for an invalid song index, tempo, musical time, or
/// a buffer too large for the current platform.
pub fn render_song(score: &Score, song_index: usize) -> Result<AudioBuffer, RenderError> {
    render_song_with_samples(score, song_index, &SampleLibrary::default())
}

/// Renders one song using preloaded sample assets.
///
/// # Errors
///
/// Returns [`RenderError`] for an invalid score or a referenced sample that is
/// absent from `sample_library`.
pub fn render_song_with_samples(
    score: &Score,
    song_index: usize,
    sample_library: &SampleLibrary,
) -> Result<AudioBuffer, RenderError> {
    let song = score
        .songs
        .get(song_index)
        .ok_or(RenderError::SongNotFound)?;
    if !song.tempo_bpm.is_finite() || song.tempo_bpm <= 0.0 {
        return Err(RenderError::InvalidTempo);
    }
    if song
        .tracks
        .iter()
        .any(|track| !track.gain.is_finite() || track.gain < 0.0)
    {
        return Err(RenderError::InvalidTrackGain);
    }
    if song.tracks.iter().any(|track| !track.pan.is_valid()) {
        return Err(RenderError::InvalidTrackPan);
    }
    if song
        .tracks
        .iter()
        .flat_map(|track| &track.samples)
        .any(|sample| !sample.speed.is_finite() || sample.speed <= 0.0)
    {
        return Err(RenderError::InvalidSampleSpeed);
    }
    let channels = match score.channels {
        Channels::Mono => 1,
        Channels::Stereo => 2,
    };
    let frames = song_frames(song, score.sample_rate_hz)?;
    let sample_count = frames
        .checked_mul(u64::from(channels))
        .and_then(|count| usize::try_from(count).ok())
        .ok_or(RenderError::AudioTooLarge)?;
    let mut samples = vec![0.0; sample_count];
    render_notes(
        song,
        score.sample_rate_hz,
        channels,
        sample_library,
        &mut samples,
    )?;
    render_samples(
        song,
        score.sample_rate_hz,
        channels,
        sample_library,
        &mut samples,
    )?;
    for sample in &mut samples {
        *sample = sample.clamp(-1.0, 1.0);
    }
    Ok(AudioBuffer {
        sample_rate_hz: score.sample_rate_hz,
        channels,
        samples,
    })
}

fn song_frames(song: &Song, sample_rate_hz: u32) -> Result<u64, RenderError> {
    let note_ends = song
        .tracks
        .iter()
        .flat_map(|track| &track.notes)
        .map(|note| {
            time_to_frame(
                note.start.checked_add(note.duration)?,
                song.tempo_bpm,
                sample_rate_hz,
            )
        })
        .try_fold(0, |latest, end| end.map(|end| latest.max(end)))?;
    song.tracks.iter().try_fold(note_ends, |latest, track| {
        time_to_frame(track.end, song.tempo_bpm, sample_rate_hz).map(|end| latest.max(end))
    })
}

fn render_notes(
    song: &Song,
    sample_rate_hz: u32,
    channels: u16,
    sample_library: &SampleLibrary,
    samples: &mut [f32],
) -> Result<(), RenderError> {
    let Some(sample_rate) = NonZeroU32::new(sample_rate_hz) else {
        return Err(RenderError::InvalidSampleRate);
    };
    let fade_samples = u64::from(sample_rate_hz).div_ceil(200);
    for track in &song.tracks {
        for (event_index, note) in track.notes.iter().enumerate() {
            let start = time_to_frame(note.start, song.tempo_bpm, sample_rate_hz)?;
            let end = time_to_frame(
                note.start.checked_add(note.duration)?,
                song.tempo_bpm,
                sample_rate_hz,
            )?;
            let note_frames = end.saturating_sub(start);
            let (mut voice, instrument_gain) = match &track.instrument {
                InstrumentKind::Sine => (
                    Voice::Oscillator(Oscillator::from_midi(
                        note.midi_pitch,
                        sample_rate,
                        Waveform::Sine,
                    )),
                    MAX_NOTE_GAIN,
                ),
                InstrumentKind::Triangle => (
                    Voice::Oscillator(Oscillator::from_midi(
                        note.midi_pitch,
                        sample_rate,
                        Waveform::Triangle,
                    )),
                    MAX_NOTE_GAIN,
                ),
                InstrumentKind::Sampled { source, root_midi } => (
                    Voice::Sample(SamplePlayer::new(
                        sample_library
                            .get(source)
                            .ok_or_else(|| RenderError::MissingSample(source.clone()))?,
                        sample_rate,
                        *root_midi,
                        note.midi_pitch,
                    )),
                    1.0,
                ),
                InstrumentKind::Sampler { pack } => {
                    return Err(RenderError::SamplerRequiresSampleEvents(pack.clone()));
                }
                InstrumentKind::DrumMachine { bank } => {
                    return Err(RenderError::DrumMachineRequiresSampleEvents(bank.clone()));
                }
            };
            for frame in start..end {
                let Some(sample) = voice.next_sample() else {
                    break;
                };
                let value = sample
                    * fade_gain(frame - start, note_frames, fade_samples)
                    * instrument_gain
                    * track.gain
                    * (f32::from(note.velocity) / 127.0);
                let first_sample = frame
                    .checked_mul(u64::from(channels))
                    .and_then(|offset| usize::try_from(offset).ok())
                    .ok_or(RenderError::AudioTooLarge)?;
                mix_sample(
                    samples,
                    first_sample,
                    channels,
                    value,
                    track.pan.percent(event_index),
                );
            }
        }
    }
    Ok(())
}

fn render_samples(
    song: &Song,
    sample_rate_hz: u32,
    channels: u16,
    sample_library: &SampleLibrary,
    samples: &mut [f32],
) -> Result<(), RenderError> {
    let sample_rate = NonZeroU32::new(sample_rate_hz).ok_or(RenderError::InvalidSampleRate)?;
    let fade_samples = u64::from(sample_rate_hz).div_ceil(200);
    for track in &song.tracks {
        if track.samples.is_empty() {
            continue;
        }
        let container = match &track.instrument {
            InstrumentKind::Sampler { pack } => pack,
            InstrumentKind::DrumMachine { bank } => bank,
            InstrumentKind::Sine | InstrumentKind::Triangle | InstrumentKind::Sampled { .. } => {
                return Err(RenderError::SampleEventsRequireSampler);
            }
        };
        for (event_index, event) in track.samples.iter().enumerate() {
            let source = match &event.selector {
                SampleSelector::Index(index) => packed_sample_source(container, *index),
                SampleSelector::Named(name) => named_sample_source(container, name),
            };
            let mut player = SamplePlayer::new(
                sample_library
                    .get(&source)
                    .ok_or_else(|| RenderError::MissingSample(source.clone()))?,
                sample_rate,
                60,
                60,
            )
            .with_speed(f64::from(event.speed));
            let start = time_to_frame(event.start, song.tempo_bpm, sample_rate_hz)?;
            let end = time_to_frame(
                event.start.checked_add(event.duration)?,
                song.tempo_bpm,
                sample_rate_hz,
            )?;
            let event_frames = end.saturating_sub(start);
            for frame in start..end {
                let Some(sample) = player.next_sample() else {
                    break;
                };
                let value = sample
                    * fade_gain(frame - start, event_frames, fade_samples)
                    * track.gain
                    * (f32::from(event.velocity) / 127.0);
                let first_sample = frame
                    .checked_mul(u64::from(channels))
                    .and_then(|offset| usize::try_from(offset).ok())
                    .ok_or(RenderError::AudioTooLarge)?;
                mix_sample(
                    samples,
                    first_sample,
                    channels,
                    value,
                    track.pan.percent(event_index),
                );
            }
        }
    }
    Ok(())
}

fn mix_sample(samples: &mut [f32], first_sample: usize, channels: u16, value: f32, pan: i8) {
    samples[first_sample] += value
        * if pan > 0 {
            1.0 - f32::from(pan) / 100.0
        } else {
            1.0
        };
    if channels == 2 {
        samples[first_sample + 1] += value
            * if pan < 0 {
                1.0 + f32::from(pan) / 100.0
            } else {
                1.0
            };
    }
}

enum Voice<'a> {
    Oscillator(Oscillator),
    Sample(SamplePlayer<'a>),
}

impl Voice<'_> {
    fn next_sample(&mut self) -> Option<f32> {
        match self {
            Self::Oscillator(oscillator) => Some(oscillator.next_sample()),
            Self::Sample(player) => player.next_sample(),
        }
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "the finite non-negative frame count is range-checked before conversion"
)]
fn time_to_frame(
    time: MusicalTime,
    tempo_bpm: f64,
    sample_rate_hz: u32,
) -> Result<u64, RenderError> {
    let frames =
        time.numerator() as f64 / time.denominator() as f64 * 240.0 * f64::from(sample_rate_hz)
            / tempo_bpm;
    if !frames.is_finite() || frames < 0.0 || frames > u64::MAX as f64 {
        Err(RenderError::AudioTooLarge)
    } else {
        Ok(frames.round() as u64)
    }
}

#[cfg(test)]
mod tests {
    use symphra_score::{
        Channels, EntityId, InstrumentKind, Key, Meter, Mode, MusicalTime, NoteEvent, Pan,
        PitchClass, SampleEvent, SampleSelector, Score, Song, Track,
    };

    use super::{RenderError, render_song};

    #[test]
    fn render_song_should_be_deterministic_and_interleaved() {
        let score = score(InstrumentKind::Sine);

        let first = render_song(&score, 0).expect("score should render");
        let second = render_song(&score, 0).expect("score should render again");

        assert_eq!(
            (first.frames(), first.channels, first.samples),
            (8, 2, second.samples)
        );
    }

    #[test]
    fn render_song_should_reject_note_events_for_sampler_packs() {
        let error = render_song(
            &score(InstrumentKind::Sampler {
                pack: "numbers".to_owned(),
            }),
            0,
        )
        .expect_err("sampler packs require sample selection events");

        assert_eq!(
            error,
            RenderError::SamplerRequiresSampleEvents("numbers".to_owned())
        );
    }

    #[test]
    fn render_song_should_reject_note_events_for_drum_machines() {
        let error = render_song(
            &score(InstrumentKind::DrumMachine {
                bank: "RolandTR909".to_owned(),
            }),
            0,
        )
        .expect_err("drum machines require sample selection events");

        assert_eq!(
            error,
            RenderError::DrumMachineRequiresSampleEvents("RolandTR909".to_owned())
        );
    }

    #[test]
    fn render_song_should_reject_invalid_sample_speed() {
        let mut score = score(InstrumentKind::Sampler {
            pack: "numbers".to_owned(),
        });
        score.songs[0].tracks[0].notes.clear();
        score.songs[0].tracks[0].samples.push(SampleEvent {
            id: EntityId(2),
            start: MusicalTime::ZERO,
            duration: MusicalTime::new(1, 4).expect("quarter note should be valid"),
            selector: SampleSelector::Index(0),
            velocity: 127,
            speed: 0.0,
        });

        let error = render_song(&score, 0).expect_err("zero sample speed should fail");

        assert_eq!(error, RenderError::InvalidSampleSpeed);
    }

    #[test]
    fn render_song_should_apply_track_gain_without_velocity_quantization() {
        let mut full_score = score(InstrumentKind::Sine);
        full_score.sample_rate_hz = 8_000;
        let full = render_song(&full_score, 0).expect("full-gain score should render");
        full_score.songs[0].tracks[0].gain = 0.3;
        let gained = render_song(&full_score, 0).expect("gained score should render");

        assert!(
            full.samples
                .iter()
                .zip(gained.samples)
                .all(|(full, gained)| { (gained - full * 0.3).abs() < f32::EPSILON })
        );
    }

    #[test]
    fn render_song_should_pan_stereo_tracks_linearly() {
        let mut centered_score = score(InstrumentKind::Sine);
        let centered = render_song(&centered_score, 0).expect("centered score should render");
        centered_score.songs[0].tracks[0].pan = Pan::Fixed(-100);
        let left = render_song(&centered_score, 0).expect("left-panned score should render");

        assert!(
            centered
                .samples
                .chunks_exact(2)
                .zip(left.samples.chunks_exact(2))
                .all(|(centered, left)| {
                    (centered[0] - left[0]).abs() < f32::EPSILON && left[1].abs() < f32::EPSILON
                })
        );
    }

    #[test]
    fn render_song_should_alternate_event_pan_from_left_to_right() {
        let mut alternating_score = score(InstrumentKind::Sine);
        alternating_score.sample_rate_hz = 8_000;
        alternating_score.songs[0].tracks[0].notes.push(NoteEvent {
            id: EntityId(3),
            start: MusicalTime::new(1, 4).expect("quarter note should be valid"),
            duration: MusicalTime::new(1, 4).expect("quarter note should be valid"),
            midi_pitch: 69,
            velocity: 127,
        });
        alternating_score.songs[0].tracks[0].end =
            MusicalTime::new(1, 2).expect("half note should be valid");
        alternating_score.songs[0].tracks[0].pan = Pan::Alternate {
            left_percent: 100,
            right_percent: 100,
        };

        let rendered = render_song(&alternating_score, 0).expect("alternating score should render");
        let (first, second) = rendered.samples.split_at(rendered.samples.len() / 2);

        assert!(
            first
                .chunks_exact(2)
                .any(|frame| frame[0].abs() > f32::EPSILON)
                && first
                    .chunks_exact(2)
                    .all(|frame| frame[1].abs() < f32::EPSILON)
                && second
                    .chunks_exact(2)
                    .all(|frame| frame[0].abs() < f32::EPSILON)
                && second
                    .chunks_exact(2)
                    .any(|frame| frame[1].abs() > f32::EPSILON)
        );
    }

    fn score(instrument: InstrumentKind) -> Score {
        Score {
            seed: 1,
            sample_rate_hz: 8,
            channels: Channels::Stereo,
            songs: vec![Song {
                id: EntityId(0),
                name: "test".to_owned(),
                tempo_bpm: 60.0,
                meter: Meter {
                    numerator: 4,
                    denominator: 4,
                },
                key: Key {
                    tonic: PitchClass::A,
                    mode: Mode::Minor,
                },
                tracks: vec![Track {
                    id: EntityId(1),
                    name: "tone".to_owned(),
                    instrument,
                    notes: vec![NoteEvent {
                        id: EntityId(2),
                        start: MusicalTime::ZERO,
                        duration: MusicalTime::new(1, 4).expect("quarter note should be valid"),
                        midi_pitch: 69,
                        velocity: 127,
                    }],
                    samples: Vec::new(),
                    gain: 1.0,
                    pan: Pan::Fixed(0),
                    end: MusicalTime::new(1, 4).expect("quarter note should be valid"),
                }],
            }],
        }
    }
}
