//! Small deterministic DSP primitives used by the offline renderer.

use std::f64::consts::TAU;
use std::num::NonZeroU32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waveform {
    Sine,
    Triangle,
}

#[derive(Clone, Debug)]
pub struct Oscillator {
    sine: SineOscillator,
    waveform: Waveform,
}

impl Oscillator {
    #[must_use]
    pub fn from_midi(midi_pitch: u8, sample_rate_hz: NonZeroU32, waveform: Waveform) -> Self {
        Self {
            sine: SineOscillator::from_midi(midi_pitch, sample_rate_hz),
            waveform,
        }
    }

    #[must_use]
    pub fn next_sample(&mut self) -> f32 {
        let sine = self.sine.next_sample();
        match self.waveform {
            Waveform::Sine => sine,
            Waveform::Triangle => sine.asin() * std::f32::consts::FRAC_2_PI,
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
        Self {
            phase: 0.0,
            phase_step: TAU * midi_frequency(midi_pitch) / f64::from(sample_rate_hz.get()),
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

/// Applies an in-place resonant lowpass biquad filter (RBJ Audio EQ
/// Cookbook coefficients) to an interleaved audio buffer, independently per
/// channel. This is offline rendering, so coefficients are computed once up
/// front and applied as a direct-form-I difference equation over the whole
/// buffer; there is no need for the block-based coefficient smoothing a
/// real-time filter would use.
///
/// `resonance` (`0.0` to `1.0`) maps to filter Q: `0.0` gives a gentle
/// Butterworth-like response (`Q ~= 0.7`) and `1.0` approaches a sharp
/// resonant peak (`Q = 10`) just short of self-oscillation. `cutoff_hz` is
/// clamped to `(0, nyquist)` so the filter always stays numerically stable
/// regardless of what the caller passes in — this is the only place that
/// knows the render sample rate, so it is also the only place that can
/// bounds-check `cutoff_hz` against the Nyquist frequency.
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
    let nyquist = f64::from(sample_rate_hz) / 2.0;
    let cutoff = f64::from(cutoff_hz).clamp(1.0, nyquist * 0.999);
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
        Oscillator, SineOscillator, Waveform, apply_delay, apply_filter, apply_limiter,
        apply_reverb, fade_gain, reverb_tail_frames,
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
