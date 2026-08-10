//! Deterministic offline rendering from score events to interleaved PCM.

use std::num::NonZeroU32;

use symphra_dsp::{
    Envelope as DspEnvelope, Oscillator, SupersawOscillator, Waveform, apply_delay, apply_filter,
    apply_filter_automated, apply_limiter, apply_reverb, envelope_gain, fade_gain,
    reverb_tail_frames,
};
use symphra_sampler::{SampleLibrary, SamplePlayer, named_sample_source, packed_sample_source};
use symphra_score::{
    Channels, DelayEffect, Effect, Envelope, FilterEffect, InstrumentKind, LfoWaveform,
    MasterLimiter, Meter, MusicalTime, NoteEvent, ReverbEffect, SampleSelector, Score, Song,
    TimeError, Track,
};
use symphra_soundfont::{SoundFontLibrary, SoundFontVoice, find_preset};
use symphra_vst3::{Vst3Library, Vst3Note, render_vst3_track};

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
    #[error("soundfont `{0}` was not loaded")]
    MissingSoundFont(String),
    #[error("soundfont `{font_source}` has no preset named `{preset}`")]
    MissingSoundFontPreset { font_source: String, preset: String },
    #[error("vst3 plugin `{0}` was not loaded")]
    MissingVst3Plugin(String),
    #[error("vst3 plugin `{plugin_source}` failed: {message}")]
    Vst3PluginFailed {
        plugin_source: String,
        message: String,
    },
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
    render_song_with_assets(
        score,
        song_index,
        sample_library,
        &SoundFontLibrary::default(),
        &Vst3Library::default(),
    )
}

/// Renders one song using preloaded sample, `SoundFont`, and VST3 assets.
///
/// # Errors
///
/// Returns [`RenderError`] for an invalid score, a referenced sample that is
/// absent from `sample_library`, a referenced SoundFont/preset that is
/// absent from `soundfont_library`, or a referenced VST3 plugin that is
/// absent from `vst3_library` (or fails to load/process).
pub fn render_song_with_assets(
    score: &Score,
    song_index: usize,
    sample_library: &SampleLibrary,
    soundfont_library: &SoundFontLibrary,
    vst3_library: &Vst3Library,
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
    let libraries = AssetLibraries {
        samples: sample_library,
        soundfonts: soundfont_library,
        vst3: vst3_library,
    };
    let mut samples = vec![0.0; sample_count];
    for track in &song.tracks {
        if let Some(effect) = track.effect {
            let mut track_samples = vec![0.0; sample_count];
            render_track(
                track,
                song.tempo_bpm,
                score.sample_rate_hz,
                channels,
                &libraries,
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
                &libraries,
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

/// The preloaded, caller-supplied assets a track's instrument may need,
/// bundled into one struct so `render_track` stays under clippy's
/// `too_many_arguments` threshold now that a track can reference three
/// independent asset kinds.
struct AssetLibraries<'a> {
    samples: &'a SampleLibrary,
    soundfonts: &'a SoundFontLibrary,
    vst3: &'a Vst3Library,
}

fn render_track(
    track: &Track,
    tempo_bpm: f64,
    sample_rate_hz: u32,
    channels: u16,
    libraries: &AssetLibraries<'_>,
    samples: &mut [f32],
) -> Result<(), RenderError> {
    if matches!(track.instrument, InstrumentKind::Vst3 { .. }) {
        render_track_vst3(
            track,
            tempo_bpm,
            sample_rate_hz,
            channels,
            libraries.vst3,
            samples,
        )?;
    } else {
        render_track_notes(
            track,
            tempo_bpm,
            sample_rate_hz,
            channels,
            libraries.samples,
            libraries.soundfonts,
            samples,
        )?;
    }
    render_track_samples(
        track,
        tempo_bpm,
        sample_rate_hz,
        channels,
        libraries.samples,
        samples,
    )
}

/// Renders a `vst3`-instrument track through one persistent plugin instance
/// for the whole track, rather than one independent [`Voice`] per note (see
/// [`symphra_vst3`]'s module docs for why). `track.gain` still applies as a
/// scalar; `track.pan` is applied as **one static value for the whole
/// rendered buffer** (`track.pan.percent(0)`) rather than alternated per
/// note — once the plugin has mixed every note into one continuous stream
/// there is no discrete per-note segment left to alternate across.
fn render_track_vst3(
    track: &Track,
    tempo_bpm: f64,
    sample_rate_hz: u32,
    channels: u16,
    vst3_library: &Vst3Library,
    samples: &mut [f32],
) -> Result<(), RenderError> {
    let InstrumentKind::Vst3 { source, preset } = &track.instrument else {
        return Ok(());
    };
    let sample_rate = NonZeroU32::new(sample_rate_hz).ok_or(RenderError::InvalidSampleRate)?;
    if !vst3_library.contains(source) {
        return Err(RenderError::MissingVst3Plugin(source.clone()));
    }

    let total_frames = time_to_frame(track.end, tempo_bpm, sample_rate_hz)?;
    let notes = track
        .notes
        .iter()
        .map(|note| {
            Ok(Vst3Note {
                start_frame: time_to_frame(note.start, tempo_bpm, sample_rate_hz)?,
                end_frame: time_to_frame(
                    note.start.checked_add(note.duration)?,
                    tempo_bpm,
                    sample_rate_hz,
                )?,
                midi_pitch: note.midi_pitch,
                velocity: note.velocity,
            })
        })
        .collect::<Result<Vec<_>, RenderError>>()?;

    let rendered = render_vst3_track(source, preset.as_deref(), sample_rate, total_frames, &notes)
        .map_err(|error| RenderError::Vst3PluginFailed {
            plugin_source: source.clone(),
            message: error.to_string(),
        })?;

    // The plugin already renders a genuinely stereo signal, unlike every
    // other instrument kind's mono-under-one-pan model — so `pan` is
    // applied here as a per-channel gain trim on top of that stereo output
    // (the same role a mixing console channel strip's pan knob plays on an
    // already-stereo channel), not as `mix_sample`'s "spread a mono source
    // across stereo" behavior.
    let pan = track.pan.percent(0);
    let left_trim = if pan > 0 {
        1.0 - f32::from(pan) / 100.0
    } else {
        1.0
    };
    let right_trim = if pan < 0 {
        1.0 + f32::from(pan) / 100.0
    } else {
        1.0
    };
    for frame in 0..total_frames {
        let frame_index = usize::try_from(frame).map_err(|_| RenderError::AudioTooLarge)?;
        let left = rendered[frame_index * 2] * track.gain;
        let right = rendered[frame_index * 2 + 1] * track.gain;
        let first_sample = frame
            .checked_mul(u64::from(channels))
            .and_then(|offset| usize::try_from(offset).ok())
            .ok_or(RenderError::AudioTooLarge)?;
        if channels == 2 {
            samples[first_sample] += left * left_trim;
            samples[first_sample + 1] += right * right_trim;
        } else {
            samples[first_sample] += f32::midpoint(left, right) * left_trim;
        }
    }
    Ok(())
}

fn render_track_notes(
    track: &Track,
    tempo_bpm: f64,
    sample_rate_hz: u32,
    channels: u16,
    sample_library: &SampleLibrary,
    soundfont_library: &SoundFontLibrary,
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
        let (mut voice, instrument_gain, envelope) = note_voice(
            &track.instrument,
            note,
            sample_rate,
            sample_library,
            soundfont_library,
        )?;
        let dsp_envelope = envelope.map(|envelope| dsp_envelope(envelope, sample_rate_hz));
        for frame in start..end {
            let Some(sample) = voice.next_sample() else {
                break;
            };
            let amplitude_gain = dsp_envelope.map_or_else(
                || fade_gain(frame - start, note_frames, fade_samples),
                |envelope| envelope_gain(frame - start, note_frames, envelope),
            );
            let value = sample
                * amplitude_gain
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

/// Builds the [`Voice`] (and its instrument-level gain/envelope) for one
/// note event, dispatching on the track's instrument kind. Split out of
/// [`render_track_notes`] to stay under clippy's `too_many_lines`
/// threshold — a mechanical extraction, not a behavior change.
fn note_voice<'a>(
    instrument: &InstrumentKind,
    note: &NoteEvent,
    sample_rate: NonZeroU32,
    sample_library: &'a SampleLibrary,
    soundfont_library: &SoundFontLibrary,
) -> Result<(Voice<'a>, f32, Option<Envelope>), RenderError> {
    match instrument {
        InstrumentKind::Sine { envelope } => Ok((
            Voice::Oscillator(Oscillator::from_midi(
                note.midi_pitch,
                sample_rate,
                Waveform::Sine,
            )),
            MAX_NOTE_GAIN,
            *envelope,
        )),
        InstrumentKind::Triangle { envelope } => Ok((
            Voice::Oscillator(Oscillator::from_midi(
                note.midi_pitch,
                sample_rate,
                Waveform::Triangle,
            )),
            MAX_NOTE_GAIN,
            *envelope,
        )),
        InstrumentKind::Supersaw {
            voices,
            detune,
            spread,
            envelope,
        } => Ok((
            Voice::Supersaw(SupersawOscillator::from_midi(
                note.midi_pitch,
                sample_rate,
                *voices,
                *detune,
                *spread,
            )),
            MAX_NOTE_GAIN,
            *envelope,
        )),
        InstrumentKind::Sampled { source, root_midi } => Ok((
            Voice::Sample(SamplePlayer::new(
                sample_library
                    .get(source)
                    .ok_or_else(|| RenderError::MissingSample(source.clone()))?,
                sample_rate,
                *root_midi,
                note.midi_pitch,
            )),
            1.0,
            None,
        )),
        InstrumentKind::Sampler { pack } => {
            Err(RenderError::SamplerRequiresSampleEvents(pack.clone()))
        }
        InstrumentKind::DrumMachine { bank } => {
            Err(RenderError::DrumMachineRequiresSampleEvents(bank.clone()))
        }
        InstrumentKind::SoundFont { source, preset } => {
            let font = soundfont_library
                .get(source)
                .ok_or_else(|| RenderError::MissingSoundFont(source.clone()))?;
            let (bank, patch) = find_preset(font, preset)
                .map(|preset| (preset.get_bank_number(), preset.get_patch_number()))
                .ok_or_else(|| RenderError::MissingSoundFontPreset {
                    font_source: source.clone(),
                    preset: preset.clone(),
                })?;
            let voice = SoundFontVoice::new(
                font,
                sample_rate,
                bank,
                patch,
                note.midi_pitch,
                note.velocity,
            )
            .map_err(|_| RenderError::MissingSoundFont(source.clone()))?;
            Ok((Voice::SoundFont(Box::new(voice)), 1.0, None))
        }
        InstrumentKind::Vst3 { .. } => {
            unreachable!("render_track routes Vst3 instruments to render_track_vst3, never here")
        }
    }
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
        InstrumentKind::Sine { .. }
        | InstrumentKind::Triangle { .. }
        | InstrumentKind::Supersaw { .. }
        | InstrumentKind::Sampled { .. }
        | InstrumentKind::SoundFont { .. }
        | InstrumentKind::Vst3 { .. } => {
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

/// Converts a score-level [`Envelope`] (milliseconds) into a [`DspEnvelope`]
/// (sample frames) for the render sample rate.
fn dsp_envelope(envelope: Envelope, sample_rate_hz: u32) -> DspEnvelope {
    DspEnvelope {
        attack_frames: envelope_ms_to_frames(envelope.attack_ms, sample_rate_hz),
        decay_frames: envelope_ms_to_frames(envelope.decay_ms, sample_rate_hz),
        sustain: envelope.sustain,
        release_frames: envelope_ms_to_frames(envelope.release_ms, sample_rate_hz),
    }
}

/// Unlike `symphra_dsp`'s own `ms_to_frames` (used by `apply_reverb`, which
/// clamps to at least one frame since a zero comb/allpass delay would
/// self-read), a zero-length envelope stage is a legitimate "skip this
/// stage" value — e.g. `attack 0ms` means an instant attack — so this
/// allows zero.
#[expect(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    reason = "ms is validated finite and non-negative at compile time"
)]
fn envelope_ms_to_frames(ms: f32, sample_rate_hz: u32) -> u64 {
    let frames = (f64::from(ms) / 1000.0 * f64::from(sample_rate_hz)).round();
    if frames.is_finite() && frames > 0.0 {
        frames as u64
    } else {
        0
    }
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
    Supersaw(SupersawOscillator),
    Sample(SamplePlayer<'a>),
    SoundFont(Box<SoundFontVoice>),
}

impl Voice<'_> {
    fn next_sample(&mut self) -> Option<f32> {
        match self {
            Self::Oscillator(oscillator) => Some(oscillator.next_sample()),
            Self::Supersaw(supersaw) => Some(supersaw.next_sample()),
            Self::Sample(player) => player.next_sample(),
            Self::SoundFont(voice) => Some(voice.next_sample()),
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
        Channels, EntityId, Envelope, InstrumentKind, Key, MasterLimiter, Meter, Mode, MusicalTime,
        NoteEvent, Pan, PitchClass, SampleEvent, SampleSelector, Score, Song, Track,
    };

    use super::{RenderError, render_song};

    #[test]
    fn render_song_should_be_deterministic_and_interleaved() {
        let score = score(InstrumentKind::Sine { envelope: None });

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
    fn render_song_should_reject_notes_for_an_unloaded_soundfont() {
        let error = render_song(
            &score(InstrumentKind::SoundFont {
                source: "instruments/gm.sf2".to_owned(),
                preset: "gm_music_box".to_owned(),
            }),
            0,
        )
        .expect_err("an unloaded soundfont should be rejected");

        assert_eq!(
            error,
            RenderError::MissingSoundFont("instruments/gm.sf2".to_owned())
        );
    }

    #[test]
    fn render_song_should_reject_notes_for_an_unloaded_vst3_plugin() {
        let error = render_song(
            &score(InstrumentKind::Vst3 {
                source: "instruments/synth.vst3".to_owned(),
                preset: None,
            }),
            0,
        )
        .expect_err("an unloaded vst3 plugin should be rejected");

        assert_eq!(
            error,
            RenderError::MissingVst3Plugin("instruments/synth.vst3".to_owned())
        );
    }

    #[test]
    fn render_song_should_apply_track_delay_effect() {
        let mut with_effect = score(InstrumentKind::Sine { envelope: None });
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
        let mut invalid = score(InstrumentKind::Sine { envelope: None });
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
        let mut with_effect = score(InstrumentKind::Sine { envelope: None });
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
        let mut invalid = score(InstrumentKind::Sine { envelope: None });
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
        let mut with_automation = score(InstrumentKind::Sine { envelope: None });
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
        let mut with_effect = score(InstrumentKind::Sine { envelope: None });
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
        let mut invalid = score(InstrumentKind::Sine { envelope: None });
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
        let mut loud = score(InstrumentKind::Sine { envelope: None });
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
        let quiet = score(InstrumentKind::Sine { envelope: None });
        let mut with_master = quiet.clone();
        with_master.songs[0].master = Some(MasterLimiter { ceiling: 0.9 });

        let without = render_song(&quiet, 0).expect("quiet score should render");
        let with = render_song(&with_master, 0).expect("quiet score with limiter should render");

        assert_eq!(without.samples, with.samples);
    }

    #[test]
    fn render_song_should_reject_out_of_range_master_ceiling() {
        let mut invalid = score(InstrumentKind::Sine { envelope: None });
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
        let mut full_score = score(InstrumentKind::Sine { envelope: None });
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
        let mut centered_score = score(InstrumentKind::Sine { envelope: None });
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
        let mut alternating_score = score(InstrumentKind::Sine { envelope: None });
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

    #[test]
    fn render_song_should_apply_a_configured_envelope_instead_of_the_fixed_fade() {
        let with_fixed_fade = score(InstrumentKind::Sine { envelope: None });
        let with_envelope = score(InstrumentKind::Sine {
            envelope: Some(Envelope {
                attack_ms: 250.0,
                decay_ms: 250.0,
                sustain: 0.5,
                release_ms: 250.0,
            }),
        });

        let fixed_fade = render_song(&with_fixed_fade, 0).expect("fixed-fade score should render");
        let enveloped = render_song(&with_envelope, 0).expect("enveloped score should render");

        assert_eq!(fixed_fade.samples.len(), enveloped.samples.len());
        assert_ne!(
            fixed_fade.samples, enveloped.samples,
            "a configured envelope should shape the note differently than the fixed edge fade"
        );
    }

    #[test]
    fn render_song_should_render_a_supersaw_instrument() {
        let score = score(InstrumentKind::Supersaw {
            voices: 5,
            detune: 0.4,
            spread: 0.6,
            envelope: None,
        });

        let rendered = render_song(&score, 0).expect("supersaw score should render");

        assert!(
            rendered
                .samples
                .iter()
                .any(|sample| sample.abs() > f32::EPSILON),
            "a supersaw note should produce audible output"
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
