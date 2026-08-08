//! Export rendered audio to standard file formats.

use symphra_render::AudioBuffer;

const WAV_HEADER_SIZE: usize = 44;
const PCM_BITS_PER_SAMPLE: u16 = 16;
const PCM_BYTES_PER_SAMPLE: u16 = PCM_BITS_PER_SAMPLE / 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ExportError {
    #[error("sample rate must be greater than zero")]
    InvalidSampleRate,
    #[error("channel count must be greater than zero")]
    InvalidChannels,
    #[error("sample count must be divisible by the channel count")]
    MisalignedSamples,
    #[error("audio samples must be finite")]
    NonFiniteSample,
    #[error("exported audio is too large for a WAV file")]
    FileTooLarge,
}

/// Encodes an interleaved audio buffer as a PCM 16-bit RIFF/WAVE file.
///
/// # Errors
///
/// Returns [`ExportError`] when the buffer metadata or samples are invalid, or
/// when the result exceeds the RIFF size limit.
pub fn encode_wav(buffer: &AudioBuffer) -> Result<Vec<u8>, ExportError> {
    validate_buffer(buffer)?;

    let block_align = buffer
        .channels
        .checked_mul(PCM_BYTES_PER_SAMPLE)
        .ok_or(ExportError::FileTooLarge)?;
    let byte_rate = buffer
        .sample_rate_hz
        .checked_mul(u32::from(block_align))
        .ok_or(ExportError::FileTooLarge)?;
    let data_size = buffer
        .samples
        .len()
        .checked_mul(usize::from(PCM_BYTES_PER_SAMPLE))
        .and_then(|size| u32::try_from(size).ok())
        .ok_or(ExportError::FileTooLarge)?;
    let riff_size = 36_u32
        .checked_add(data_size)
        .ok_or(ExportError::FileTooLarge)?;
    let capacity = WAV_HEADER_SIZE
        .checked_add(usize::try_from(data_size).map_err(|_| ExportError::FileTooLarge)?)
        .ok_or(ExportError::FileTooLarge)?;

    let mut wav = Vec::with_capacity(capacity);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&buffer.channels.to_le_bytes());
    wav.extend_from_slice(&buffer.sample_rate_hz.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&PCM_BITS_PER_SAMPLE.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    for &sample in &buffer.samples {
        wav.extend_from_slice(&sample_to_i16(sample).to_le_bytes());
    }
    Ok(wav)
}

fn validate_buffer(buffer: &AudioBuffer) -> Result<(), ExportError> {
    if buffer.sample_rate_hz == 0 {
        return Err(ExportError::InvalidSampleRate);
    }
    if buffer.channels == 0 {
        return Err(ExportError::InvalidChannels);
    }
    if !buffer
        .samples
        .len()
        .is_multiple_of(usize::from(buffer.channels))
    {
        return Err(ExportError::MisalignedSamples);
    }
    if buffer.samples.iter().any(|sample| !sample.is_finite()) {
        return Err(ExportError::NonFiniteSample);
    }
    Ok(())
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "the rounded value is clamped to the complete i16 range"
)]
fn sample_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * 32_768.0)
        .round()
        .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_pcm_wav() {
        let wav = encode_wav(&AudioBuffer {
            sample_rate_hz: 8_000,
            channels: 1,
            samples: vec![0.0, 1.0, -1.0],
        })
        .expect("valid audio should encode");

        assert_eq!(wav.len(), 50);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(&wav[40..44], &6_u32.to_le_bytes());
        assert_eq!(&wav[44..], &[0, 0, 255, 127, 0, 128]);
    }

    #[test]
    fn rejects_misaligned_samples() {
        let error = encode_wav(&AudioBuffer {
            sample_rate_hz: 48_000,
            channels: 2,
            samples: vec![0.0],
        })
        .expect_err("incomplete frames should fail");

        assert_eq!(error, ExportError::MisalignedSamples);
    }
}
