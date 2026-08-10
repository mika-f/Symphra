//! Small deterministic DSP primitives used by the offline renderer.

use std::f64::consts::TAU;
use std::num::NonZeroU32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waveform {
    Sine,
    Triangle,
    /// A naive (non-band-limited) sawtooth, ramping linearly from `-1.0` to
    /// `1.0` across each cycle before discontinuously resetting — the same
    /// simplification already accepted for [`Waveform::Triangle`]'s `asin`
    /// derivation (no band-limiting there either). Used by
    /// [`SupersawOscillator`], the classic "supersaw" building block.
    Sawtooth,
}

#[derive(Clone, Debug)]
pub struct Oscillator {
    sine: SineOscillator,
    waveform: Waveform,
}

impl Oscillator {
    #[must_use]
    pub fn from_midi(midi_pitch: u8, sample_rate_hz: NonZeroU32, waveform: Waveform) -> Self {
        Self::from_frequency(midi_frequency(midi_pitch), sample_rate_hz, waveform)
    }

    /// Like [`Self::from_midi`], but takes a raw frequency in hertz instead
    /// of a whole MIDI pitch — used by [`SupersawOscillator`] to detune
    /// voices by fractional cents, which a MIDI pitch alone cannot express.
    #[must_use]
    pub fn from_frequency(
        frequency_hz: f64,
        sample_rate_hz: NonZeroU32,
        waveform: Waveform,
    ) -> Self {
        Self {
            sine: SineOscillator::from_frequency(frequency_hz, sample_rate_hz),
            waveform,
        }
    }

    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "phase is bounded to [0, TAU) and audio samples use f32"
    )]
    pub fn next_sample(&mut self) -> f32 {
        let phase = self.sine.phase;
        let sine = self.sine.next_sample();
        match self.waveform {
            Waveform::Sine => sine,
            Waveform::Triangle => sine.asin() * std::f32::consts::FRAC_2_PI,
            Waveform::Sawtooth => (phase / std::f64::consts::PI - 1.0) as f32,
        }
    }
}

/// Converts a MIDI note number to frequency in hertz using A4 = 440 Hz.
#[must_use]
pub fn midi_frequency(midi_pitch: u8) -> f64 {
    440.0 * 2.0_f64.powf((f64::from(midi_pitch) - 69.0) / 12.0)
}

/// A phase-continuous sine oscillator.
#[derive(Clone, Debug)]
pub struct SineOscillator {
    phase: f64,
    phase_step: f64,
}

impl SineOscillator {
    #[must_use]
    pub fn from_midi(midi_pitch: u8, sample_rate_hz: NonZeroU32) -> Self {
        Self::from_frequency(midi_frequency(midi_pitch), sample_rate_hz)
    }

    #[must_use]
    pub fn from_frequency(frequency_hz: f64, sample_rate_hz: NonZeroU32) -> Self {
        Self {
            phase: 0.0,
            phase_step: TAU * frequency_hz / f64::from(sample_rate_hz.get()),
        }
    }

    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "sine output is bounded to [-1, 1] and audio samples use f32"
    )]
    pub fn next_sample(&mut self) -> f32 {
        let sample = self.phase.sin();
        self.phase = (self.phase + self.phase_step).rem_euclid(TAU);
        sample as f32
    }
}

/// Returns a linear attack/release gain with zero-valued edge samples.
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "sample ratios intentionally become floating-point gains"
)]
pub fn fade_gain(sample_index: u64, total_samples: u64, fade_samples: u64) -> f32 {
    if sample_index >= total_samples {
        return 0.0;
    }
    if fade_samples == 0 {
        return 1.0;
    }
    let attack = if sample_index < fade_samples {
        sample_index as f32 / fade_samples as f32
    } else {
        1.0
    };
    let remaining = total_samples - sample_index - 1;
    let release = if remaining < fade_samples {
        remaining as f32 / fade_samples as f32
    } else {
        1.0
    };
    attack.min(release)
}

/// `voices` detuned [`Waveform::Sawtooth`] oscillators mixed together — the
/// classic "supersaw" unison-detune thickening technique. `detune`
/// (`0.0..=1.0`) scales how far apart the voices are spread in pitch, up to
/// +-50 cents at `1.0` (a conventional supersaw detune range); voices are
/// spread evenly across that range. `spread` (`0.0..=1.0`) is a blend
/// control between the center (least-detuned) voice and the outer,
/// most-detuned voices: at `0.0` only the center voice is really audible
/// (thin, near-unison); at `1.0` every voice contributes equally (full
/// thickness). This is a deliberate simplification of the original's
/// "stereo pan spread" reading of `spread` — the renderer's existing
/// per-note pipeline mixes one oscillator voice down to a single scalar
/// sample with one track-level pan, so a true per-voice stereo width would
/// need a second, independently panned signal path; blend fits the
/// existing single-voice-per-instrument render loop unchanged. A single
/// voice is just one plain, non-detuned sawtooth.
#[derive(Clone, Debug)]
pub struct SupersawOscillator {
    voices: Vec<(Oscillator, f32)>,
}

impl SupersawOscillator {
    #[must_use]
    pub fn from_midi(
        midi_pitch: u8,
        sample_rate_hz: NonZeroU32,
        voice_count: u32,
        detune: f32,
        spread: f32,
    ) -> Self {
        let voice_count = voice_count.max(1);
        let detune = f64::from(detune.clamp(0.0, 1.0));
        let spread = spread.clamp(0.0, 1.0);
        let max_cents = 50.0 * detune;
        let base_frequency = midi_frequency(midi_pitch);
        let voices = (0..voice_count)
            .map(|index| {
                let cents = if voice_count == 1 {
                    0.0
                } else {
                    let t = f64::from(index) / f64::from(voice_count - 1);
                    t.mul_add(2.0, -1.0) * max_cents
                };
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "distance is a ratio in [0.0, 1.0]"
                )]
                let normalized_distance = if max_cents > 0.0 {
                    (cents.abs() / max_cents) as f32
                } else {
                    0.0
                };
                let weight = (1.0 - spread).mul_add(-normalized_distance, 1.0);
                let frequency = base_frequency * 2.0_f64.powf(cents / 1200.0);
                (
                    Oscillator::from_frequency(frequency, sample_rate_hz, Waveform::Sawtooth),
                    weight,
                )
            })
            .collect();
        Self { voices }
    }

    #[must_use]
    pub fn next_sample(&mut self) -> f32 {
        let mut sum = 0.0f32;
        let mut weight_sum = 0.0f32;
        for (oscillator, weight) in &mut self.voices {
            sum += oscillator.next_sample() * *weight;
            weight_sum += *weight;
        }
        if weight_sum > 0.0 {
            sum / weight_sum
        } else {
            0.0
        }
    }
}

/// A configurable ADSR amplitude envelope, attached to an oscillator-based
/// instrument (`sine`, `triangle`, `synth supersaw`) in place of
/// [`fade_gain`]'s fixed edge fade. All four stage lengths are expressed in
/// frames (already resolved from `ms` at render time, the same boundary
/// `DelayEffect.time` crosses from a musical duration to sample frames).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Envelope {
    pub attack_frames: u64,
    pub decay_frames: u64,
    pub sustain: f32,
    pub release_frames: u64,
}

/// Computes an ADSR gain at `sample_index` within a `total_samples`-long
/// note: amplitude ramps `0.0` to `1.0` over `attack_frames`, then to
/// `sustain` over `decay_frames`, holds at `sustain` until `release_frames`
/// before the note's end, then ramps to `0.0`. Unlike [`fade_gain`]'s
/// symmetric attack/release ramp (which always both peak at `1.0`), release
/// here multiplies whatever level attack/decay left the note at, so a
/// `sustain` below `1.0` still ramps smoothly to silence instead of
/// jumping.
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "sample ratios intentionally become floating-point gains"
)]
pub fn envelope_gain(sample_index: u64, total_samples: u64, envelope: Envelope) -> f32 {
    if sample_index >= total_samples {
        return 0.0;
    }
    let sustain = envelope.sustain.clamp(0.0, 1.0);
    let level = if sample_index < envelope.attack_frames {
        sample_index as f32 / envelope.attack_frames as f32
    } else {
        let decay_index = sample_index - envelope.attack_frames;
        if decay_index < envelope.decay_frames {
            let progress = decay_index as f32 / envelope.decay_frames as f32;
            progress.mul_add(-(1.0 - sustain), 1.0)
        } else {
            sustain
        }
    };
    let remaining = total_samples - sample_index - 1;
    let release = if envelope.release_frames > 0 && remaining < envelope.release_frames {
        remaining as f32 / envelope.release_frames as f32
    } else {
        1.0
    };
    level * release
}

/// Applies an in-place feedback delay (echo) to an interleaved audio buffer,
/// independently per channel.
///
/// `echo[n] = wet[n - delay_frames]`, where `wet[n] = dry[n] + feedback *
/// echo[n]`, so each repeat is `feedback` times quieter than the last.
/// `mix` blends `0.0` (fully dry) to `1.0` (fully wet/echo). `delay_frames`
/// is clamped to at least one frame: a zero delay would make each sample
/// feed back into itself in the same step.
pub fn apply_delay(buffer: &mut [f32], channels: u16, delay_frames: u64, mix: f32, feedback: f32) {
    let channels = usize::from(channels);
    if channels == 0 || buffer.is_empty() {
        return;
    }
    let frames = buffer.len() / channels;
    let delay = usize::try_from(delay_frames.max(1))
        .unwrap_or(usize::MAX)
        .min(frames.max(1));
    for channel in 0..channels {
        let dry: Vec<f32> = (0..frames)
            .map(|frame| buffer[frame * channels + channel])
            .collect();
        let mut wet = vec![0.0f32; frames];
        for frame in 0..frames {
            let fed_back = frame.checked_sub(delay).map_or(0.0, |source| wet[source]);
            wet[frame] = feedback.mul_add(fed_back, dry[frame]);
        }
        for frame in 0..frames {
            let echo = frame.checked_sub(delay).map_or(0.0, |source| wet[source]);
            buffer[frame * channels + channel] = mix.mul_add(echo - dry[frame], dry[frame]);
        }
    }
}

/// Computes normalized RBJ Audio EQ Cookbook lowpass biquad coefficients
/// `(b0, b1, b2, a1, a2)`. `resonance` (`0.0` to `1.0`) maps to filter Q:
/// `0.0` gives a gentle Butterworth-like response (`Q ~= 0.7`) and `1.0`
/// approaches a sharp resonant peak (`Q = 10`) just short of
/// self-oscillation. `cutoff_hz` is clamped to `(0, nyquist)` so the filter
/// always stays numerically stable regardless of what the caller passes in.
/// Shared by [`apply_filter`] (coefficients computed once) and
/// [`apply_filter_automated`] (recomputed every frame, since `cutoff_hz`
/// there is swept by an LFO rather than constant).
fn lowpass_biquad_coefficients(
    cutoff_hz: f64,
    sample_rate_hz: u32,
    resonance: f32,
) -> (f64, f64, f64, f64, f64) {
    let nyquist = f64::from(sample_rate_hz) / 2.0;
    let cutoff = cutoff_hz.clamp(1.0, nyquist * 0.999);
    let q = 0.7 + f64::from(resonance.clamp(0.0, 1.0)) * 9.3;
    let w0 = TAU * cutoff / f64::from(sample_rate_hz);
    let (sin_w0, cos_w0) = w0.sin_cos();
    let alpha = sin_w0 / (2.0 * q);
    let a0 = 1.0 + alpha;
    let b1 = 1.0 - cos_w0;
    let b0 = (b1 / 2.0) / a0;
    let b2 = b0;
    let b1 = b1 / a0;
    let a1 = (-2.0 * cos_w0) / a0;
    let a2 = (1.0 - alpha) / a0;
    (b0, b1, b2, a1, a2)
}

/// Applies an in-place resonant lowpass biquad filter to an interleaved
/// audio buffer, independently per channel. This is offline rendering, so
/// coefficients are computed once up front (see
/// [`lowpass_biquad_coefficients`]) and applied as a direct-form-I
/// difference equation over the whole buffer; there is no need for the
/// block-based coefficient smoothing a real-time filter would use. This is
/// the only place that knows the render sample rate, so it is also the only
/// place that can bounds-check `cutoff_hz` against the Nyquist frequency.
pub fn apply_filter(
    buffer: &mut [f32],
    channels: u16,
    sample_rate_hz: u32,
    cutoff_hz: f32,
    resonance: f32,
) {
    let channel_count = usize::from(channels);
    if channel_count == 0 || buffer.is_empty() || sample_rate_hz == 0 {
        return;
    }
    let (b0, b1, b2, a1, a2) =
        lowpass_biquad_coefficients(f64::from(cutoff_hz), sample_rate_hz, resonance);

    for channel in 0..channel_count {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        let mut frame = channel;
        while frame < buffer.len() {
            let x0 = f64::from(buffer[frame]);
            let y0 = b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a resonant filter's output is clamped downstream by the renderer's final safety clamp"
            )]
            {
                buffer[frame] = y0 as f32;
            }
            x2 = x1;
            x1 = x0;
            y2 = y1;
            y1 = y0;
            frame += channel_count;
        }
    }
}

/// Same resonant lowpass biquad as [`apply_filter`], but `cutoff_hz` is
/// swept continuously by an LFO between `range_hz`'s two bounds (in either
/// order) instead of held constant — this is what a "swept filter"
/// (wah-style) effect sounds like. The LFO's trajectory is precomputed once
/// up front (so a stereo buffer sweeps identically on both channels — the
/// same swept cutoff at a given time instant, not independent per-channel
/// phases) and biquad coefficients are recomputed from scratch every frame
/// from [`lowpass_biquad_coefficients`]; because rendering is offline, this
/// is not a performance concern the way recomputing coefficients at audio
/// rate would be for a real-time plugin (which would instead only
/// recompute periodically, to save CPU).
pub fn apply_filter_automated(
    buffer: &mut [f32],
    channels: u16,
    sample_rate_hz: u32,
    resonance: f32,
    waveform: Waveform,
    range_hz: (f32, f32),
    lfo_rate_hz: f32,
) {
    let channel_count = usize::from(channels);
    if channel_count == 0 || buffer.is_empty() || sample_rate_hz == 0 {
        return;
    }
    let frames = buffer.len() / channel_count;
    let low = f64::from(range_hz.0.min(range_hz.1));
    let high = f64::from(range_hz.0.max(range_hz.1));
    let center = f64::midpoint(low, high);
    let half_range = (high - low) / 2.0;
    let phase_step = TAU * f64::from(lfo_rate_hz) / f64::from(sample_rate_hz);
    let mut phase = 0.0f64;
    let cutoffs: Vec<f64> = (0..frames)
        .map(|_| {
            let lfo_value = match waveform {
                Waveform::Sine => phase.sin(),
                Waveform::Triangle => phase.sin().asin() * std::f64::consts::FRAC_2_PI,
                // `automate`'s `lfo` only ever resolves to `Sine`/`Triangle`
                // (see `LfoWaveform` in `symphra-compiler`/`symphra-score`);
                // this arm exists only because `Waveform` is shared with
                // `Oscillator`/`SupersawOscillator`, which do use `Sawtooth`.
                Waveform::Sawtooth => phase / std::f64::consts::PI - 1.0,
            };
            let cutoff = center + half_range * lfo_value;
            phase = (phase + phase_step).rem_euclid(TAU);
            cutoff
        })
        .collect();

    for channel in 0..channel_count {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for (frame, &cutoff) in cutoffs.iter().enumerate() {
            let (b0, b1, b2, a1, a2) =
                lowpass_biquad_coefficients(cutoff, sample_rate_hz, resonance);
            let index = frame * channel_count + channel;
            let x0 = f64::from(buffer[index]);
            let y0 = b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a resonant filter's output is clamped downstream by the renderer's final safety clamp"
            )]
            {
                buffer[index] = y0 as f32;
            }
            x2 = x1;
            x1 = x0;
            y2 = y1;
            y1 = y0;
        }
    }
}

/// Comb filter delay times, expressed in milliseconds so they scale to any
/// sample rate rather than being fixed sample counts. A reduced (4 comb, 2
/// allpass) version of the classic Schroeder reverberator topology Freeverb
/// later popularized; these four are the millisecond-equivalent of
/// Freeverb's first four comb delays (in samples, at its reference 44.1kHz:
/// 1116, 1188, 1277, 1356).
const REVERB_COMB_DELAYS_MS: [f64; 4] = [25.31, 26.94, 28.96, 30.75];
/// Allpass delay times, the millisecond-equivalent of Freeverb's first two
/// allpass delays (in samples, at 44.1kHz: 556, 441).
const REVERB_ALLPASS_DELAYS_MS: [f64; 2] = [12.61, 10.00];
/// Schroeder's original fixed allpass feedback coefficient.
const REVERB_ALLPASS_FEEDBACK: f64 = 0.5;
/// Amplitude below which a reverb's decaying tail is considered inaudible;
/// used by [`reverb_tail_frames`] the same way delay bounds its own tail.
const REVERB_TAIL_EPSILON: f64 = 0.001;

/// Maps `size` (`0.0` to `1.0`) to comb filter feedback (`0.7` to `0.98`),
/// mirroring Freeverb's roomsize-to-feedback mapping. Kept below `1.0` so
/// every comb filter is unconditionally stable.
fn reverb_comb_feedback(size: f32) -> f64 {
    0.7 + f64::from(size.clamp(0.0, 1.0)) * 0.28
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a positive, finite frame count is range-checked before conversion"
)]
fn ms_to_frames(ms: f64, sample_rate_hz: u32) -> usize {
    let frames = (ms / 1000.0 * f64::from(sample_rate_hz)).round();
    if frames.is_finite() && frames >= 1.0 {
        frames as usize
    } else {
        1
    }
}

/// The longest comb filter's decaying tail at `sample_rate_hz` and `size`,
/// in frames — used by the renderer to size its output buffer to fit a
/// reverb's ring-out, the same way delay's tail is bounded. Lives here
/// (not duplicated in `symphra-render`) since the comb delay times
/// themselves are an [`apply_reverb`] implementation detail, not something
/// `mix`/`size` expose to the caller.
#[must_use]
#[expect(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    reason = "the repeat count is derived from a finite, non-negative logarithm"
)]
pub fn reverb_tail_frames(sample_rate_hz: u32, size: f32) -> u64 {
    if sample_rate_hz == 0 {
        return 0;
    }
    let feedback = reverb_comb_feedback(size);
    let longest_delay = REVERB_COMB_DELAYS_MS
        .iter()
        .map(|ms| ms_to_frames(*ms, sample_rate_hz))
        .max()
        .unwrap_or(1) as u64;
    let repeats = if feedback <= 0.0 {
        1
    } else {
        let repeats = (REVERB_TAIL_EPSILON.ln() / feedback.ln()).ceil();
        if repeats.is_finite() {
            (repeats as u64).max(1)
        } else {
            1
        }
    };
    longest_delay.saturating_mul(repeats)
}

/// Applies an in-place Schroeder reverberator to an interleaved audio
/// buffer, independently per channel: four parallel feedback comb filters
/// (`comb[n] = dry[n] + feedback * comb[n - delay]`) are summed and
/// averaged, then run through two series allpass filters
/// (`allpass[n] = -g * input[n] + input[n - delay] + g * allpass[n -
/// delay]`) for diffusion. This is offline rendering, so — like
/// [`apply_delay`] and [`apply_filter`] — there is no real-time streaming
/// constraint; each stage is computed as a full-buffer pass.
///
/// `mix` blends `0.0` (fully dry) to `1.0` (fully wet/reverberated). `size`
/// (`0.0` to `1.0`) controls how long the reverb tail rings out, via
/// [`reverb_comb_feedback`].
pub fn apply_reverb(buffer: &mut [f32], channels: u16, sample_rate_hz: u32, mix: f32, size: f32) {
    let channel_count = usize::from(channels);
    if channel_count == 0 || buffer.is_empty() || sample_rate_hz == 0 {
        return;
    }
    let frames = buffer.len() / channel_count;
    let comb_feedback = reverb_comb_feedback(size);
    let comb_delays: Vec<usize> = REVERB_COMB_DELAYS_MS
        .iter()
        .map(|ms| ms_to_frames(*ms, sample_rate_hz))
        .collect();
    let allpass_delays: Vec<usize> = REVERB_ALLPASS_DELAYS_MS
        .iter()
        .map(|ms| ms_to_frames(*ms, sample_rate_hz))
        .collect();
    #[expect(clippy::cast_precision_loss, reason = "comb_delays.len() is always 4")]
    let comb_count = comb_delays.len() as f64;

    for channel in 0..channel_count {
        let dry: Vec<f64> = (0..frames)
            .map(|frame| f64::from(buffer[frame * channel_count + channel]))
            .collect();

        let mut wet = vec![0.0f64; frames];
        for &delay in &comb_delays {
            let mut comb = vec![0.0f64; frames];
            for frame in 0..frames {
                let fed_back = frame.checked_sub(delay).map_or(0.0, |source| comb[source]);
                comb[frame] = comb_feedback.mul_add(fed_back, dry[frame]);
            }
            for (mixed, comb_sample) in wet.iter_mut().zip(&comb) {
                *mixed += comb_sample / comb_count;
            }
        }
        for &delay in &allpass_delays {
            let input = wet.clone();
            for frame in 0..frames {
                let delayed_input = frame.checked_sub(delay).map_or(0.0, |source| input[source]);
                let fed_back = frame.checked_sub(delay).map_or(0.0, |source| wet[source]);
                wet[frame] = (-REVERB_ALLPASS_FEEDBACK).mul_add(input[frame], delayed_input)
                    + REVERB_ALLPASS_FEEDBACK * fed_back;
            }
        }

        for frame in 0..frames {
            let out = f64::from(mix).mul_add(wet[frame] - dry[frame], dry[frame]);
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a reverb's output is clamped downstream by the renderer's final safety clamp"
            )]
            {
                buffer[frame * channel_count + channel] = out as f32;
            }
        }
    }
}

/// Scans `buffer` for its peak absolute sample value and, if it exceeds
/// `ceiling`, uniformly scales every sample by `ceiling / peak` so the
/// loudest sample lands exactly at `ceiling`. Unlike clipping, this
/// preserves the buffer's relative dynamics/waveform shape — every sample
/// is scaled by the same factor, not independently clamped. A no-op when
/// the peak is already at or below `ceiling` (including a silent buffer).
pub fn apply_limiter(buffer: &mut [f32], ceiling: f32) {
    let peak = buffer
        .iter()
        .fold(0.0f32, |peak, &sample| peak.max(sample.abs()));
    if peak <= ceiling {
        return;
    }
    let gain = ceiling / peak;
    for sample in buffer {
        *sample *= gain;
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::{
        Envelope, Oscillator, SineOscillator, SupersawOscillator, Waveform, apply_delay,
        apply_filter, apply_filter_automated, apply_limiter, apply_reverb, envelope_gain,
        fade_gain, reverb_tail_frames,
    };

    #[test]
    fn sine_oscillator_should_complete_one_cycle_in_four_samples() {
        let mut oscillator = SineOscillator::from_midi(
            69,
            NonZeroU32::new(1_760).expect("sample rate should be non-zero"),
        );

        let samples = std::array::from_fn::<_, 4, _>(|_| oscillator.next_sample().round());

        assert!(
            samples
                .iter()
                .zip([0.0, 1.0, 0.0, -1.0])
                .all(|(actual, expected)| (actual - expected).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn oscillator_should_render_selected_waveform() {
        let sample_rate = NonZeroU32::new(3_520).expect("sample rate should be non-zero");
        let mut sine = Oscillator::from_midi(69, sample_rate, Waveform::Sine);
        let mut triangle = Oscillator::from_midi(69, sample_rate, Waveform::Triangle);

        let _ = sine.next_sample();
        let _ = triangle.next_sample();

        assert!((sine.next_sample() - triangle.next_sample()).abs() > 0.1);
    }

    #[test]
    fn sawtooth_oscillator_should_ramp_linearly_across_one_cycle() {
        let sample_rate = NonZeroU32::new(4).expect("sample rate should be non-zero");
        let mut saw = Oscillator::from_frequency(1.0, sample_rate, Waveform::Sawtooth);

        let samples = std::array::from_fn::<_, 4, _>(|_| saw.next_sample());

        assert!(
            samples
                .iter()
                .zip([-1.0, -0.5, 0.0, 0.5])
                .all(|(actual, expected): (&f32, f32)| (actual - expected).abs() < 1e-4),
            "{samples:?}"
        );
    }

    #[test]
    fn supersaw_with_one_voice_should_match_a_plain_sawtooth() {
        let sample_rate = NonZeroU32::new(48_000).expect("sample rate should be non-zero");
        let mut supersaw = SupersawOscillator::from_midi(69, sample_rate, 1, 0.5, 1.0);
        let mut plain = Oscillator::from_midi(69, sample_rate, Waveform::Sawtooth);

        for _ in 0..8 {
            assert!((supersaw.next_sample() - plain.next_sample()).abs() < 1e-4);
        }
    }

    #[test]
    fn supersaw_with_zero_detune_should_match_a_plain_sawtooth() {
        let sample_rate = NonZeroU32::new(48_000).expect("sample rate should be non-zero");
        let mut supersaw = SupersawOscillator::from_midi(69, sample_rate, 5, 0.0, 1.0);
        let mut plain = Oscillator::from_midi(69, sample_rate, Waveform::Sawtooth);

        for _ in 0..8 {
            assert!((supersaw.next_sample() - plain.next_sample()).abs() < 1e-4);
        }
    }

    #[test]
    fn supersaw_with_more_voices_should_differ_from_a_plain_sawtooth() {
        let sample_rate = NonZeroU32::new(48_000).expect("sample rate should be non-zero");
        let mut supersaw = SupersawOscillator::from_midi(69, sample_rate, 5, 0.5, 1.0);
        let mut plain = Oscillator::from_midi(69, sample_rate, Waveform::Sawtooth);

        let differs =
            (0..2_000).any(|_| (supersaw.next_sample() - plain.next_sample()).abs() > 1e-3);
        assert!(differs);
    }

    #[test]
    fn envelope_gain_should_ramp_through_attack_decay_sustain_and_release() {
        let envelope = Envelope {
            attack_frames: 2,
            decay_frames: 2,
            sustain: 0.5,
            release_frames: 2,
        };

        let gains: Vec<f32> = (0..8)
            .map(|index| envelope_gain(index, 8, envelope))
            .collect();

        assert!(
            gains
                .iter()
                .zip([0.0, 0.5, 1.0, 0.75, 0.5, 0.5, 0.25, 0.0])
                .all(|(actual, expected): (&f32, f32)| (actual - expected).abs() < 1e-4),
            "{gains:?}"
        );
    }

    #[test]
    fn envelope_gain_should_be_zero_past_the_note_end() {
        let envelope = Envelope {
            attack_frames: 1,
            decay_frames: 1,
            sustain: 0.5,
            release_frames: 1,
        };

        assert!(envelope_gain(4, 4, envelope).abs() < f32::EPSILON);
    }

    #[test]
    fn envelope_gain_with_zero_attack_should_start_at_full_level() {
        let envelope = Envelope {
            attack_frames: 0,
            decay_frames: 0,
            sustain: 1.0,
            release_frames: 0,
        };

        assert!((envelope_gain(0, 4, envelope) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn fade_gain_should_silence_both_note_edges() {
        let gains = std::array::from_fn::<_, 5, _>(|index| fade_gain(index as u64, 5, 2));

        assert!(
            gains
                .iter()
                .zip([0.0, 0.5, 1.0, 0.5, 0.0])
                .all(|(actual, expected)| (actual - expected).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn apply_delay_should_place_a_single_echo_at_the_delay_offset() {
        let mut buffer = vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0];

        apply_delay(&mut buffer, 1, 2, 1.0, 0.0);

        assert!(
            buffer
                .iter()
                .zip([0.0, 0.0, 1.0, 0.0, 0.0, 0.0])
                .all(|(actual, expected): (&f32, f32)| (actual - expected).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn apply_delay_should_decay_repeats_by_the_feedback_factor() {
        let mut buffer = vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];

        apply_delay(&mut buffer, 1, 2, 1.0, 0.5);

        assert!(
            buffer
                .iter()
                .zip([0.0, 0.0, 1.0, 0.0, 0.5, 0.0, 0.25, 0.0])
                .all(|(actual, expected): (&f32, f32)| (actual - expected).abs() < 1e-6)
        );
    }

    #[test]
    fn apply_delay_should_blend_dry_and_wet_by_mix() {
        let mut buffer = vec![1.0, 1.0];

        apply_delay(&mut buffer, 1, 1, 0.5, 0.0);

        assert!(
            buffer
                .iter()
                .zip([0.5, 1.0])
                .all(|(actual, expected): (&f32, f32)| (actual - expected).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn apply_delay_should_process_each_channel_independently() {
        let mut buffer = vec![1.0, 2.0, 0.0, 0.0, 0.0, 0.0];

        apply_delay(&mut buffer, 2, 1, 1.0, 0.0);

        assert!(
            buffer
                .iter()
                .zip([0.0, 0.0, 1.0, 2.0, 0.0, 0.0])
                .all(|(actual, expected): (&f32, f32)| (actual - expected).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn apply_delay_should_clamp_a_zero_delay_to_one_frame_without_panicking() {
        let mut buffer = vec![1.0, 0.0];

        apply_delay(&mut buffer, 1, 0, 1.0, 0.0);

        assert!(
            buffer
                .iter()
                .zip([0.0, 1.0])
                .all(|(actual, expected): (&f32, f32)| (actual - expected).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn apply_filter_should_pass_a_dc_signal_through_near_unity_gain() {
        let mut buffer = vec![1.0f32; 200];

        apply_filter(&mut buffer, 1, 48_000, 1_000.0, 0.0);

        let settled = &buffer[180..];
        assert!(
            settled.iter().all(|sample| (sample - 1.0).abs() < 0.05),
            "{settled:?}"
        );
    }

    #[test]
    fn apply_filter_should_attenuate_content_well_above_cutoff() {
        let mut buffer: Vec<f32> = (0..100)
            .map(|frame| if frame % 2 == 0 { 1.0 } else { -1.0 })
            .collect();

        apply_filter(&mut buffer, 1, 48_000, 200.0, 0.0);

        let settled = &buffer[80..];
        assert!(
            settled.iter().all(|sample| sample.abs() < 0.2),
            "{settled:?}"
        );
    }

    #[test]
    fn apply_filter_should_process_each_channel_independently() {
        let frames = 2_000;
        let mut buffer: Vec<f32> = (0..frames)
            .flat_map(|frame| [1.0, if frame % 2 == 0 { 1.0 } else { -1.0 }])
            .collect();

        apply_filter(&mut buffer, 2, 48_000, 200.0, 0.0);

        let dc_channel: Vec<f32> = buffer
            .iter()
            .skip((frames - 50) * 2)
            .step_by(2)
            .copied()
            .collect();
        let nyquist_channel: Vec<f32> = buffer
            .iter()
            .skip((frames - 50) * 2 + 1)
            .step_by(2)
            .copied()
            .collect();
        assert!(
            dc_channel.iter().all(|sample| (sample - 1.0).abs() < 0.05),
            "{dc_channel:?}"
        );
        assert!(
            nyquist_channel.iter().all(|sample| sample.abs() < 0.05),
            "{nyquist_channel:?}"
        );
    }

    #[test]
    fn apply_filter_should_be_a_no_op_for_zero_channels_or_sample_rate() {
        let mut buffer = vec![1.0, 2.0, 3.0];

        apply_filter(&mut buffer, 0, 48_000, 1_000.0, 0.0);
        assert_eq!(buffer, vec![1.0, 2.0, 3.0]);

        apply_filter(&mut buffer, 1, 0, 1_000.0, 0.0);
        assert_eq!(buffer, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn apply_filter_automated_should_match_apply_filter_when_the_lfo_range_is_flat() {
        let dry: Vec<f32> = (0..500)
            .map(|frame| if frame % 7 == 0 { 1.0 } else { -0.3 })
            .collect();

        let mut automated = dry.clone();
        apply_filter_automated(
            &mut automated,
            1,
            48_000,
            0.3,
            Waveform::Sine,
            (1_000.0, 1_000.0),
            4.0,
        );
        let mut constant = dry.clone();
        apply_filter(&mut constant, 1, 48_000, 1_000.0, 0.3);

        assert!(
            automated
                .iter()
                .zip(&constant)
                .all(|(a, c)| (a - c).abs() < 1e-4),
            "{automated:?}\n{constant:?}"
        );
    }

    #[test]
    fn apply_filter_automated_should_differ_from_a_static_filter_over_a_nontrivial_range() {
        let dry: Vec<f32> = (0..2_000)
            .map(|frame| if frame % 3 == 0 { 1.0 } else { -0.5 })
            .collect();

        let mut automated = dry.clone();
        apply_filter_automated(
            &mut automated,
            1,
            48_000,
            0.0,
            Waveform::Sine,
            (100.0, 8_000.0),
            10.0,
        );
        let mut constant = dry.clone();
        apply_filter(&mut constant, 1, 48_000, 4_050.0, 0.0);

        assert!(
            automated
                .iter()
                .zip(&constant)
                .any(|(a, c)| (a - c).abs() > 1e-3)
        );
    }

    #[test]
    fn apply_filter_automated_should_leave_a_silent_channel_untouched() {
        let frames = 500;
        let mut buffer: Vec<f32> = (0..frames)
            .flat_map(|frame| [if frame % 5 == 0 { 1.0 } else { -0.4 }, 0.0])
            .collect();

        apply_filter_automated(
            &mut buffer,
            2,
            48_000,
            0.5,
            Waveform::Triangle,
            (200.0, 6_000.0),
            3.0,
        );

        let silent_channel: Vec<f32> = buffer.iter().skip(1).step_by(2).copied().collect();
        assert!(silent_channel.iter().all(|&sample| sample == 0.0));
    }

    #[test]
    fn apply_filter_automated_should_be_a_no_op_for_zero_channels_or_sample_rate() {
        let mut buffer = vec![1.0, 2.0, 3.0];

        apply_filter_automated(
            &mut buffer,
            0,
            48_000,
            0.0,
            Waveform::Sine,
            (100.0, 2_000.0),
            1.0,
        );
        assert_eq!(buffer, vec![1.0, 2.0, 3.0]);

        apply_filter_automated(
            &mut buffer,
            1,
            0,
            0.0,
            Waveform::Sine,
            (100.0, 2_000.0),
            1.0,
        );
        assert_eq!(buffer, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn apply_reverb_at_zero_mix_should_leave_dry_signal_unchanged() {
        let original = vec![1.0f32, -0.5, 0.25, 0.0, -0.75];
        let mut buffer = original.clone();

        apply_reverb(&mut buffer, 1, 48_000, 0.0, 0.5);

        assert_eq!(buffer, original);
    }

    #[test]
    fn apply_reverb_at_full_mix_should_differ_from_the_dry_impulse() {
        let mut buffer = vec![0.0f32; 4_000];
        buffer[0] = 1.0;

        apply_reverb(&mut buffer, 1, 48_000, 1.0, 0.5);

        assert!(buffer.iter().any(|sample| sample.abs() > f32::EPSILON));
        assert!((buffer[0] - 1.0).abs() > f32::EPSILON);
    }

    #[test]
    fn apply_reverb_should_leave_a_silent_channel_untouched() {
        let frames = 4_000;
        let mut buffer: Vec<f32> = (0..frames)
            .flat_map(|frame| [if frame == 0 { 1.0 } else { 0.0 }, 0.0])
            .collect();

        apply_reverb(&mut buffer, 2, 48_000, 1.0, 0.9);

        let silent_channel: Vec<f32> = buffer.iter().skip(1).step_by(2).copied().collect();
        assert!(silent_channel.iter().all(|&sample| sample == 0.0));
    }

    #[test]
    fn apply_reverb_larger_size_should_retain_more_tail_energy() {
        let frames = 20_000;
        let impulse = || {
            let mut buffer = vec![0.0f32; frames];
            buffer[0] = 1.0;
            buffer
        };

        let mut short = impulse();
        apply_reverb(&mut short, 1, 48_000, 1.0, 0.0);
        let mut long = impulse();
        apply_reverb(&mut long, 1, 48_000, 1.0, 1.0);

        let tail_energy = |buffer: &[f32]| {
            buffer[10_000..]
                .iter()
                .map(|sample| sample.abs())
                .sum::<f32>()
        };
        assert!(
            tail_energy(&long) > tail_energy(&short),
            "size 1.0 tail energy {} should exceed size 0.0 tail energy {}",
            tail_energy(&long),
            tail_energy(&short)
        );
    }

    #[test]
    fn apply_reverb_should_be_a_no_op_for_zero_channels_or_sample_rate() {
        let mut buffer = vec![1.0, 2.0, 3.0];

        apply_reverb(&mut buffer, 0, 48_000, 1.0, 0.5);
        assert_eq!(buffer, vec![1.0, 2.0, 3.0]);

        apply_reverb(&mut buffer, 1, 0, 1.0, 0.5);
        assert_eq!(buffer, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn reverb_tail_frames_should_grow_with_size() {
        assert!(reverb_tail_frames(48_000, 1.0) > reverb_tail_frames(48_000, 0.0));
    }

    #[test]
    fn reverb_tail_frames_should_be_zero_for_zero_sample_rate() {
        assert_eq!(reverb_tail_frames(0, 0.5), 0);
    }

    #[test]
    fn apply_limiter_should_leave_audio_under_ceiling_untouched() {
        let mut buffer = vec![0.2, -0.3, 0.4];

        apply_limiter(&mut buffer, 0.9);

        assert_eq!(buffer, vec![0.2, -0.3, 0.4]);
    }

    #[test]
    fn apply_limiter_should_scale_the_peak_down_to_exactly_the_ceiling() {
        let mut buffer = vec![0.5, -1.0, 0.25];

        apply_limiter(&mut buffer, 0.5);

        let peak = buffer
            .iter()
            .fold(0.0f32, |peak, &sample| peak.max(sample.abs()));
        assert!((peak - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn apply_limiter_should_preserve_relative_dynamics_between_samples() {
        let mut buffer = vec![0.5, -2.0];

        apply_limiter(&mut buffer, 1.0);

        // Both samples are scaled by the same 0.5 gain (1.0 / 2.0), so the
        // 4x ratio between them is preserved rather than one being clipped
        // flat while the other is untouched.
        assert!((buffer[0] - 0.25).abs() < f32::EPSILON);
        assert!((buffer[1] - -1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn apply_limiter_should_be_a_no_op_on_silence() {
        let mut buffer = vec![0.0, 0.0, 0.0];

        apply_limiter(&mut buffer, 0.5);

        assert_eq!(buffer, vec![0.0, 0.0, 0.0]);
    }
}
