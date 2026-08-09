//! Mono PCM 16-bit WAV decoding and deterministic pitched sample playback.

use std::collections::HashMap;
use std::num::NonZeroU32;

#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub sample_rate_hz: NonZeroU32,
    pub samples: Vec<f32>,
}

#[derive(Clone, Debug, Default)]
pub struct SampleLibrary {
    samples: HashMap<String, Sample>,
}

impl SampleLibrary {
    pub fn insert(&mut self, source: impl Into<String>, sample: Sample) {
        self.samples.insert(source.into(), sample);
    }

    #[must_use]
    pub fn get(&self, source: &str) -> Option<&Sample> {
        self.samples.get(source)
    }
}

#[must_use]
pub fn packed_sample_source(pack: &str, index: u32) -> String {
    format!("{pack}/{index}.wav")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("sample is not a RIFF/WAVE file")]
    InvalidContainer,
    #[error("sample contains a truncated WAV chunk")]
    TruncatedChunk,
    #[error("sample is missing a `fmt ` chunk")]
    MissingFormat,
    #[error("sample is missing a `data` chunk")]
    MissingData,
    #[error("sample must be mono PCM 16-bit WAV")]
    UnsupportedFormat,
    #[error("sample rate must be greater than zero")]
    InvalidSampleRate,
    #[error("sample data is not aligned to 16-bit frames")]
    MisalignedData,
}

/// Decodes a mono PCM 16-bit RIFF/WAVE buffer.
///
/// # Errors
///
/// Returns [`DecodeError`] when the container is malformed or uses an
/// unsupported audio format.
pub fn decode_wav(bytes: &[u8]) -> Result<Sample, DecodeError> {
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(DecodeError::InvalidContainer);
    }

    let mut format = None;
    let mut data = None;
    let mut cursor = 12usize;
    while cursor < bytes.len() {
        let header = bytes
            .get(cursor..cursor.saturating_add(8))
            .ok_or(DecodeError::TruncatedChunk)?;
        let size = usize::try_from(u32::from_le_bytes([
            header[4], header[5], header[6], header[7],
        ]))
        .map_err(|_| DecodeError::TruncatedChunk)?;
        let start = cursor.checked_add(8).ok_or(DecodeError::TruncatedChunk)?;
        let end = start.checked_add(size).ok_or(DecodeError::TruncatedChunk)?;
        let chunk = bytes.get(start..end).ok_or(DecodeError::TruncatedChunk)?;
        match &header[..4] {
            b"fmt " => format = Some(chunk),
            b"data" => data = Some(chunk),
            _ => {}
        }
        cursor = end
            .checked_add(size % 2)
            .ok_or(DecodeError::TruncatedChunk)?;
    }

    let format = format.ok_or(DecodeError::MissingFormat)?;
    if format.len() < 16 {
        return Err(DecodeError::TruncatedChunk);
    }
    let audio_format = u16::from_le_bytes([format[0], format[1]]);
    let channels = u16::from_le_bytes([format[2], format[3]]);
    let sample_rate_hz = u32::from_le_bytes([format[4], format[5], format[6], format[7]]);
    let block_align = u16::from_le_bytes([format[12], format[13]]);
    let bits_per_sample = u16::from_le_bytes([format[14], format[15]]);
    if audio_format != 1 || channels != 1 || block_align != 2 || bits_per_sample != 16 {
        return Err(DecodeError::UnsupportedFormat);
    }
    let sample_rate_hz = NonZeroU32::new(sample_rate_hz).ok_or(DecodeError::InvalidSampleRate)?;
    let data = data.ok_or(DecodeError::MissingData)?;
    let frames = data.chunks_exact(2);
    if !frames.remainder().is_empty() {
        return Err(DecodeError::MisalignedData);
    }
    let samples = frames
        .map(|frame| f32::from(i16::from_le_bytes([frame[0], frame[1]])) / 32_768.0)
        .collect();

    Ok(Sample {
        sample_rate_hz,
        samples,
    })
}

#[derive(Debug)]
pub struct SamplePlayer<'a> {
    sample: &'a Sample,
    position: f64,
    step: f64,
}

impl<'a> SamplePlayer<'a> {
    #[must_use]
    pub fn new(
        sample: &'a Sample,
        output_sample_rate_hz: NonZeroU32,
        root_midi: u8,
        midi_pitch: u8,
    ) -> Self {
        let pitch_ratio = 2.0_f64.powf((f64::from(midi_pitch) - f64::from(root_midi)) / 12.0);
        Self {
            sample,
            position: 0.0,
            step: f64::from(sample.sample_rate_hz.get()) / f64::from(output_sample_rate_hz.get())
                * pitch_ratio,
        }
    }

    #[must_use]
    pub fn with_speed(mut self, speed: f64) -> Self {
        self.step *= speed;
        self
    }

    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        reason = "the finite non-negative playback position is bounded by sample length"
    )]
    pub fn next_sample(&mut self) -> Option<f32> {
        let index = self.position.floor() as usize;
        let current = *self.sample.samples.get(index)?;
        let next = self
            .sample
            .samples
            .get(index + 1)
            .copied()
            .unwrap_or(current);
        let fraction = (self.position - index as f64) as f32;
        self.position += self.step;
        Some(current + (next - current) * fraction)
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::{SamplePlayer, decode_wav};

    #[test]
    fn decode_wav_should_read_mono_pcm16() {
        let bytes = wav(&[i16::MIN, 0, i16::MAX], 8_000);

        let sample = decode_wav(&bytes).expect("PCM16 WAV should decode");

        assert_eq!(
            (sample.sample_rate_hz.get(), sample.samples),
            (8_000, vec![-1.0, 0.0, f32::from(i16::MAX) / 32_768.0])
        );
    }

    #[test]
    fn sample_player_should_transpose_and_resample() {
        let sample =
            decode_wav(&wav(&[0, 16_384, 0, -16_384], 4)).expect("PCM16 WAV should decode");
        let mut player = SamplePlayer::new(
            &sample,
            NonZeroU32::new(4).expect("sample rate should be non-zero"),
            60,
            72,
        );

        assert_eq!(
            [
                player.next_sample(),
                player.next_sample(),
                player.next_sample()
            ],
            [Some(0.0), Some(0.0), None]
        );
    }

    #[test]
    fn sample_player_should_apply_playback_speed() {
        let sample =
            decode_wav(&wav(&[0, 8_192, 16_384, 24_576], 4)).expect("PCM16 WAV should decode");
        let mut player = SamplePlayer::new(
            &sample,
            NonZeroU32::new(4).expect("sample rate should be non-zero"),
            60,
            60,
        )
        .with_speed(2.0);

        assert_eq!(
            [
                player.next_sample(),
                player.next_sample(),
                player.next_sample()
            ],
            [Some(0.0), Some(0.5), None]
        );
    }

    fn wav(samples: &[i16], sample_rate_hz: u32) -> Vec<u8> {
        let data_size = u32::try_from(samples.len() * 2).expect("test sample should fit");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&sample_rate_hz.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate_hz * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_size.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }
}
