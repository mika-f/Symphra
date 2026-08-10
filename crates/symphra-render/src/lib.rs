//! Deterministic offline rendering from score events to interleaved PCM.

use std::num::NonZeroU32;

use symphra_dsp::{Oscillator, Waveform, apply_delay, apply_limiter, fade_gain};
use symphra_sampler::{SampleLibrary, SamplePlayer, named_sample_source, packed_sample_source};
use symphra_score::{
    Channels, DelayEffect, InstrumentKind, MasterLimiter, MusicalTime, SampleSelector, Score, Song,
    TimeError, Track,
};

const MAX_NOTE_GAIN: f32 = 0.2;
/// Amplitude below which a delay's decaying echo repeats are considered
/// inaudible; used to bound how far the rendered buffer must extend to fit
/// a delay's tail.
const DELAY_TAIL_EPSILON: f32 = 0.001;

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
    #[error("effect mix must be from 0.0 to 1.0 and feedback from 0.0 to less than 1.0")]
    InvalidEffectParameters,
    #[error("master ceiling must be finite and from 0.0 to 1.0")]
    InvalidMasterCeiling,
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
    if song
        .tracks
        .iter()
        .filter_map(|track| track.effect)
        .any(|effect| !effect_is_valid(&effect))
    {
        return Err(RenderError::InvalidEffectParameters);
    }
    if song.master.is_some_and(|master| !master_is_valid(master)) {
        return Err(RenderError::InvalidMasterCeiling);
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
    for track in &song.tracks {
        if let Some(effect) = track.effect {
            let mut track_samples = vec![0.0; sample_count];
            render_track(
                track,
                song.tempo_bpm,
                score.sample_rate_hz,
                channels,
                sample_library,
                &mut track_samples,
            )?;
            let delay_frames = time_to_frame(effect.time, song.tempo_bpm, score.sample_rate_hz)?;
            apply_delay(
                &mut track_samples,
                channels,
                delay_frames,
                effect.mix,
                effect.feedback,
            );
            for (mixed, dry) in samples.iter_mut().zip(&track_samples) {
                *mixed += dry;
            }
        } else {
            render_track(
                track,
                song.tempo_bpm,
                score.sample_rate_hz,
                channels,
                sample_library,
                &mut samples,
            )?;
        }
    }
    if let Some(master) = &song.master {
        apply_limiter(&mut samples, master.ceiling);
    }
    for sample in &mut samples {
        *sample = sample.clamp(-1.0, 1.0);
    }
    Ok(AudioBuffer {
        sample_rate_hz: score.sample_rate_hz,
        channels,
        samples,
    })
}

fn master_is_valid(master: MasterLimiter) -> bool {
    master.ceiling.is_finite() && (0.0..=1.0).contains(&master.ceiling)
}

fn effect_is_valid(effect: &DelayEffect) -> bool {
    effect.mix.is_finite()
        && (0.0..=1.0).contains(&effect.mix)
        && effect.feedback.is_finite()
        && (0.0..1.0).contains(&effect.feedback)
}

/// The extra frames a track's delay tail needs beyond its natural end, so
/// decaying echo repeats are not truncated. Bounded by [`DELAY_TAIL_EPSILON`]
/// rather than the effect's theoretical (infinite, for `feedback -> 1`)
/// ring-out time.
#[expect(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    reason = "the repeat count is derived from a finite, non-negative logarithm"
)]
fn delay_tail_frames(
    effect: &DelayEffect,
    tempo_bpm: f64,
    sample_rate_hz: u32,
) -> Result<u64, RenderError> {
    let delay_frames = time_to_frame(effect.time, tempo_bpm, sample_rate_hz)?.max(1);
    let repeats = if effect.feedback <= 0.0 {
        1
    } else {
        let repeats = (DELAY_TAIL_EPSILON.ln() / effect.feedback.ln()).ceil();
        if repeats.is_finite() {
            (repeats as u64).max(1)
        } else {
            1
        }
    };
    delay_frames
        .checked_mul(repeats)
        .ok_or(RenderError::AudioTooLarge)
}

fn song_frames(song: &Song, sample_rate_hz: u32) -> Result<u64, RenderError> {
    song.tracks.iter().try_fold(0, |latest, track| {
        let mut end = time_to_frame(track.end, song.tempo_bpm, sample_rate_hz)?;
        for note in &track.notes {
            end = end.max(time_to_frame(
                note.start.checked_add(note.duration)?,
                song.tempo_bpm,
                sample_rate_hz,
            )?);
        }
        for sample in &track.samples {
            end = end.max(time_to_frame(
                sample.start.checked_add(sample.duration)?,
                song.tempo_bpm,
                sample_rate_hz,
            )?);
        }
        if let Some(effect) = track.effect {
            end = end.saturating_add(delay_tail_frames(&effect, song.tempo_bpm, sample_rate_hz)?);
        }
        Ok(latest.max(end))
    })
}

fn render_track(
    track: &Track,
    tempo_bpm: f64,
    sample_rate_hz: u32,
    channels: u16,
    sample_library: &SampleLibrary,
    samples: &mut [f32],
) -> Result<(), RenderError> {
    render_track_notes(
        track,
        tempo_bpm,
        sample_rate_hz,
        channels,
        sample_library,
        samples,
    )?;
    render_track_samples(
        track,
        tempo_bpm,
        sample_rate_hz,
        channels,
        sample_library,
        samples,
    )
}

fn render_track_notes(
    track: &Track,
    tempo_bpm: f64,
    sample_rate_hz: u32,
    channels: u16,
    sample_library: &SampleLibrary,
    samples: &mut [f32],
) -> Result<(), RenderError> {
    let Some(sample_rate) = NonZeroU32::new(sample_rate_hz) else {
        return Err(RenderError::InvalidSampleRate);
    };
    let fade_samples = u64::from(sample_rate_hz).div_ceil(200);
    for (event_index, note) in track.notes.iter().enumerate() {
        let start = time_to_frame(note.start, tempo_bpm, sample_rate_hz)?;
        let end = time_to_frame(
            note.start.checked_add(note.duration)?,
            tempo_bpm,
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
    Ok(())
}

fn render_track_samples(
    track: &Track,
    tempo_bpm: f64,
    sample_rate_hz: u32,
    channels: u16,
    sample_library: &SampleLibrary,
    samples: &mut [f32],
) -> Result<(), RenderError> {
    if track.samples.is_empty() {
        return Ok(());
    }
    let sample_rate = NonZeroU32::new(sample_rate_hz).ok_or(RenderError::InvalidSampleRate)?;
    let fade_samples = u64::from(sample_rate_hz).div_ceil(200);
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
        let start = time_to_frame(event.start, tempo_bpm, sample_rate_hz)?;
        let end = time_to_frame(
            event.start.checked_add(event.duration)?,
            tempo_bpm,
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
        Channels, EntityId, InstrumentKind, Key, MasterLimiter, Meter, Mode, MusicalTime,
        NoteEvent, Pan, PitchClass, SampleEvent, SampleSelector, Score, Song, Track,
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
    fn render_song_should_apply_track_delay_effect() {
        let mut with_effect = score(InstrumentKind::Sine);
        with_effect.sample_rate_hz = 1_000;
        let dry = render_song(&with_effect, 0).expect("dry score should render");

        with_effect.songs[0].tracks[0].effect = Some(symphra_score::DelayEffect {
            mix: 1.0,
            time: MusicalTime::new(1, 4).expect("quarter note should be valid"),
            feedback: 0.0,
        });
        let wet = render_song(&with_effect, 0).expect("wet score should render");

        assert!(
            wet.samples.len() > dry.samples.len(),
            "the delay's echo tail should extend the render length"
        );
        assert!(
            wet.samples[dry.samples.len()..]
                .iter()
                .any(|sample| sample.abs() > f32::EPSILON),
            "the delayed echo should appear in the extended tail"
        );
    }

    #[test]
    fn render_song_should_reject_out_of_range_effect_parameters() {
        let mut invalid = score(InstrumentKind::Sine);
        invalid.songs[0].tracks[0].effect = Some(symphra_score::DelayEffect {
            mix: 1.0,
            time: MusicalTime::new(1, 4).expect("quarter note should be valid"),
            feedback: 1.0,
        });

        let error = render_song(&invalid, 0).expect_err("feedback of 1.0 should be rejected");

        assert_eq!(error, RenderError::InvalidEffectParameters);
    }

    #[test]
    fn render_song_should_apply_master_limiter_when_peak_exceeds_ceiling() {
        let mut loud = score(InstrumentKind::Sine);
        loud.songs[0].tracks[0].gain = 10.0;
        let ceiling = 0.5;
        loud.songs[0].master = Some(MasterLimiter { ceiling });

        let limited = render_song(&loud, 0).expect("loud score with limiter should render");

        let peak = limited
            .samples
            .iter()
            .fold(0.0f32, |peak, &sample| peak.max(sample.abs()));
        assert!(
            peak <= ceiling + 1e-4,
            "peak {peak} should not exceed ceiling {ceiling}"
        );
    }

    #[test]
    fn render_song_should_leave_audio_unchanged_when_peak_is_within_ceiling() {
        let quiet = score(InstrumentKind::Sine);
        let mut with_master = quiet.clone();
        with_master.songs[0].master = Some(MasterLimiter { ceiling: 0.9 });

        let without = render_song(&quiet, 0).expect("quiet score should render");
        let with = render_song(&with_master, 0).expect("quiet score with limiter should render");

        assert_eq!(without.samples, with.samples);
    }

    #[test]
    fn render_song_should_reject_out_of_range_master_ceiling() {
        let mut invalid = score(InstrumentKind::Sine);
        invalid.songs[0].master = Some(MasterLimiter { ceiling: 1.5 });

        let error = render_song(&invalid, 0).expect_err("ceiling above 1.0 should be rejected");

        assert_eq!(error, RenderError::InvalidMasterCeiling);
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
                    effect: None,
                }],
                master: None,
            }],
        }
    }
}
