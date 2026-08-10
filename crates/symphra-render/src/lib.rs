//! Deterministic offline rendering from score events to interleaved PCM.

use std::num::NonZeroU32;

use symphra_dsp::{
    Oscillator, Waveform, apply_delay, apply_filter, apply_filter_automated, apply_limiter,
    apply_reverb, fade_gain, reverb_tail_frames,
};
use symphra_sampler::{SampleLibrary, SamplePlayer, named_sample_source, packed_sample_source};
use symphra_score::{
    Channels, DelayEffect, Effect, FilterEffect, InstrumentKind, LfoWaveform, MasterLimiter, Meter,
    MusicalTime, ReverbEffect, SampleSelector, Score, Song, TimeError, Track,
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
    #[error(
        "effect parameters must be in range (delay: mix 0.0 to 1.0, feedback 0.0 to less than 1.0; filter: cutoff greater than 0hz, resonance 0.0 to 1.0; reverb: mix 0.0 to 1.0, size 0.0 to 1.0)"
    )]
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
            apply_track_effect(
                effect,
                &mut track_samples,
                channels,
                song.tempo_bpm,
                song.meter,
                score.sample_rate_hz,
            )?;
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

/// Renders `effect` into `track_samples` in place, dispatching to the DSP
/// primitive for its kind.
fn apply_track_effect(
    effect: Effect,
    track_samples: &mut [f32],
    channels: u16,
    tempo_bpm: f64,
    meter: Meter,
    sample_rate_hz: u32,
) -> Result<(), RenderError> {
    match effect {
        Effect::Delay(delay) => {
            let delay_frames = time_to_frame(delay.time, tempo_bpm, sample_rate_hz)?;
            apply_delay(
                track_samples,
                channels,
                delay_frames,
                delay.mix,
                delay.feedback,
            );
        }
        Effect::Filter(filter) => match filter.automation {
            None => {
                apply_filter(
                    track_samples,
                    channels,
                    sample_rate_hz,
                    filter.cutoff_hz,
                    filter.resonance,
                );
            }
            Some(automation) => {
                let waveform = match automation.waveform {
                    LfoWaveform::Sine => Waveform::Sine,
                    LfoWaveform::Triangle => Waveform::Triangle,
                };
                let lfo_rate_hz = automation_rate_hz(automation.cycles_per_bar, tempo_bpm, meter);
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "lfo_rate_hz is derived from a positive, finite cycles_per_bar"
                )]
                let lfo_rate_hz = lfo_rate_hz as f32;
                apply_filter_automated(
                    track_samples,
                    channels,
                    sample_rate_hz,
                    filter.resonance,
                    waveform,
                    (automation.range_start_hz, automation.range_end_hz),
                    lfo_rate_hz,
                );
            }
        },
        Effect::Reverb(reverb) => {
            apply_reverb(
                track_samples,
                channels,
                sample_rate_hz,
                reverb.mix,
                reverb.size,
            );
        }
    }
    Ok(())
}

fn effect_is_valid(effect: &Effect) -> bool {
    match effect {
        Effect::Delay(delay) => delay_effect_is_valid(delay),
        Effect::Filter(filter) => filter_effect_is_valid(*filter),
        Effect::Reverb(reverb) => reverb_effect_is_valid(*reverb),
    }
}

fn delay_effect_is_valid(effect: &DelayEffect) -> bool {
    effect.mix.is_finite()
        && (0.0..=1.0).contains(&effect.mix)
        && effect.feedback.is_finite()
        && (0.0..1.0).contains(&effect.feedback)
}

fn filter_effect_is_valid(effect: FilterEffect) -> bool {
    effect.cutoff_hz.is_finite()
        && effect.cutoff_hz > 0.0
        && effect.resonance.is_finite()
        && (0.0..=1.0).contains(&effect.resonance)
}

fn reverb_effect_is_valid(effect: ReverbEffect) -> bool {
    effect.mix.is_finite()
        && (0.0..=1.0).contains(&effect.mix)
        && effect.size.is_finite()
        && (0.0..=1.0).contains(&effect.size)
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
        match track.effect {
            Some(Effect::Delay(delay)) => {
                end =
                    end.saturating_add(delay_tail_frames(&delay, song.tempo_bpm, sample_rate_hz)?);
            }
            Some(Effect::Reverb(reverb)) => {
                end = end.saturating_add(reverb_tail_frames(sample_rate_hz, reverb.size));
            }
            Some(Effect::Filter(_)) | None => {}
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

/// Converts `automate`'s `N cycles/bar` rate into an LFO frequency in Hz.
/// One bar is `meter.numerator / meter.denominator` whole notes, and one
/// whole note lasts `240 / tempo_bpm` seconds (the same relationship
/// [`time_to_frame`] uses for note/sample durations), so a bar lasts `240 *
/// meter.numerator / (meter.denominator * tempo_bpm)` seconds; the LFO
/// frequency is `cycles_per_bar` divided by that bar duration.
fn automation_rate_hz(cycles_per_bar: f32, tempo_bpm: f64, meter: Meter) -> f64 {
    f64::from(cycles_per_bar) * f64::from(meter.denominator) * tempo_bpm
        / (240.0 * f64::from(meter.numerator))
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

        with_effect.songs[0].tracks[0].effect =
            Some(symphra_score::Effect::Delay(symphra_score::DelayEffect {
                mix: 1.0,
                time: MusicalTime::new(1, 4).expect("quarter note should be valid"),
                feedback: 0.0,
            }));
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
        invalid.songs[0].tracks[0].effect =
            Some(symphra_score::Effect::Delay(symphra_score::DelayEffect {
                mix: 1.0,
                time: MusicalTime::new(1, 4).expect("quarter note should be valid"),
                feedback: 1.0,
            }));

        let error = render_song(&invalid, 0).expect_err("feedback of 1.0 should be rejected");

        assert_eq!(error, RenderError::InvalidEffectParameters);
    }

    #[test]
    fn render_song_should_apply_track_filter_effect() {
        let mut with_effect = score(InstrumentKind::Sine);
        with_effect.sample_rate_hz = 8_000;
        let dry = render_song(&with_effect, 0).expect("dry score should render");

        with_effect.songs[0].tracks[0].effect =
            Some(symphra_score::Effect::Filter(symphra_score::FilterEffect {
                cutoff_hz: 200.0,
                resonance: 0.0,
                automation: None,
            }));
        let filtered = render_song(&with_effect, 0).expect("filtered score should render");

        assert_eq!(
            filtered.samples.len(),
            dry.samples.len(),
            "a filter has no echo tail, unlike delay, so it should not extend render length"
        );
        assert_ne!(
            filtered.samples, dry.samples,
            "the lowpass filter should audibly change the rendered signal"
        );
    }

    #[test]
    fn render_song_should_reject_out_of_range_effect_filter_parameters() {
        let mut invalid = score(InstrumentKind::Sine);
        invalid.songs[0].tracks[0].effect =
            Some(symphra_score::Effect::Filter(symphra_score::FilterEffect {
                cutoff_hz: 0.0,
                resonance: 0.0,
                automation: None,
            }));

        let error = render_song(&invalid, 0).expect_err("zero cutoff should be rejected");

        assert_eq!(error, RenderError::InvalidEffectParameters);
    }

    #[test]
    fn render_song_should_apply_filter_automation() {
        let mut with_automation = score(InstrumentKind::Sine);
        with_automation.sample_rate_hz = 8_000;
        with_automation.songs[0].tracks[0].end =
            MusicalTime::new(2, 1).expect("two whole notes should be valid");
        with_automation.songs[0].tracks[0].notes[0].duration =
            MusicalTime::new(2, 1).expect("two whole notes should be valid");

        let mut with_static_filter = with_automation.clone();
        with_static_filter.songs[0].tracks[0].effect =
            Some(symphra_score::Effect::Filter(symphra_score::FilterEffect {
                cutoff_hz: 1_700.0,
                resonance: 0.0,
                automation: None,
            }));
        let static_filtered =
            render_song(&with_static_filter, 0).expect("statically filtered score should render");

        with_automation.songs[0].tracks[0].effect =
            Some(symphra_score::Effect::Filter(symphra_score::FilterEffect {
                cutoff_hz: 1_700.0,
                resonance: 0.0,
                automation: Some(symphra_score::FilterAutomation {
                    waveform: symphra_score::LfoWaveform::Sine,
                    range_start_hz: 400.0,
                    range_end_hz: 3_000.0,
                    cycles_per_bar: 4.0,
                }),
            }));
        let automated =
            render_song(&with_automation, 0).expect("automated filter score should render");

        assert_eq!(
            automated.samples.len(),
            static_filtered.samples.len(),
            "automation sweeps an existing filter, it does not add its own tail"
        );
        assert_ne!(
            automated.samples, static_filtered.samples,
            "a swept cutoff should audibly differ from holding cutoff at its static value"
        );
    }

    #[test]
    fn render_song_should_apply_track_reverb_effect() {
        let mut with_effect = score(InstrumentKind::Sine);
        with_effect.sample_rate_hz = 8_000;
        let dry = render_song(&with_effect, 0).expect("dry score should render");

        with_effect.songs[0].tracks[0].effect =
            Some(symphra_score::Effect::Reverb(symphra_score::ReverbEffect {
                mix: 1.0,
                size: 0.9,
            }));
        let wet = render_song(&with_effect, 0).expect("wet score should render");

        assert!(
            wet.samples.len() > dry.samples.len(),
            "the reverb's decaying tail should extend the render length"
        );
        assert!(
            wet.samples[dry.samples.len()..]
                .iter()
                .any(|sample| sample.abs() > f32::EPSILON),
            "reverberated energy should appear in the extended tail"
        );
    }

    #[test]
    fn render_song_should_reject_out_of_range_effect_reverb_parameters() {
        let mut invalid = score(InstrumentKind::Sine);
        invalid.songs[0].tracks[0].effect =
            Some(symphra_score::Effect::Reverb(symphra_score::ReverbEffect {
                mix: 1.5,
                size: 0.5,
            }));

        let error = render_song(&invalid, 0).expect_err("mix above 1.0 should be rejected");

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
