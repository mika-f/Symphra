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

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::{Oscillator, SineOscillator, Waveform, fade_gain};

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
}
