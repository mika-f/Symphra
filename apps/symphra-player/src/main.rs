use std::env;
use std::ffi::OsString;
use std::fs;
use std::ops::Range;
use std::process::ExitCode;
use std::sync::{Arc, mpsc};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, I24, Sample, SampleFormat, SizedSample, Stream, StreamConfig, U24};

fn main() -> ExitCode {
    match run(env::args_os().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: impl IntoIterator<Item = OsString>) -> Result<(), PlayerError> {
    let mut args = args.into_iter();
    let input = args.next().ok_or(PlayerError::Usage)?;
    let audio = decode_wav(&fs::read(input)?)?;
    let range = match (args.next(), args.next(), args.next()) {
        (None, None, None) => 0..audio.frames(),
        (Some(start), Some(end), None) => parse_frame(&start)?..parse_frame(&end)?,
        _ => return Err(PlayerError::Usage),
    };
    if range.start >= range.end || range.end > audio.frames() {
        return Err(PlayerError::InvalidRange {
            start: range.start,
            end: range.end,
            frames: audio.frames(),
        });
    }
    play(audio, range)
}

fn parse_frame(value: &OsString) -> Result<usize, PlayerError> {
    value
        .to_str()
        .and_then(|value| value.parse().ok())
        .ok_or(PlayerError::Usage)
}

fn play(audio: Audio, range: Range<usize>) -> Result<(), PlayerError> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or(PlayerError::NoOutputDevice)?;
    let supported = device.default_output_config()?;
    let format = supported.sample_format();
    let config = supported.into();
    let (errors, stream_errors) = mpsc::channel();
    let stream = match format {
        SampleFormat::I8 => output_stream::<i8>(&device, config, audio, range, errors),
        SampleFormat::I16 => output_stream::<i16>(&device, config, audio, range, errors),
        SampleFormat::I24 => output_stream::<I24>(&device, config, audio, range, errors),
        SampleFormat::I32 => output_stream::<i32>(&device, config, audio, range, errors),
        SampleFormat::I64 => output_stream::<i64>(&device, config, audio, range, errors),
        SampleFormat::U8 => output_stream::<u8>(&device, config, audio, range, errors),
        SampleFormat::U16 => output_stream::<u16>(&device, config, audio, range, errors),
        SampleFormat::U24 => output_stream::<U24>(&device, config, audio, range, errors),
        SampleFormat::U32 => output_stream::<u32>(&device, config, audio, range, errors),
        SampleFormat::U64 => output_stream::<u64>(&device, config, audio, range, errors),
        SampleFormat::F32 => output_stream::<f32>(&device, config, audio, range, errors),
        SampleFormat::F64 => output_stream::<f64>(&device, config, audio, range, errors),
        unsupported => return Err(PlayerError::UnsupportedSampleFormat(unsupported)),
    }?;
    stream.play()?;
    Err(PlayerError::StreamStopped(
        stream_errors
            .recv()
            .unwrap_or_else(|_| "audio stream closed".to_owned()),
    ))
}

fn output_stream<T>(
    device: &Device,
    config: StreamConfig,
    audio: Audio,
    range: Range<usize>,
    errors: mpsc::Sender<String>,
) -> Result<Stream, cpal::Error>
where
    T: Sample + SizedSample + FromSample<f32>,
{
    let mut playback = Playback::new(audio, range, &config);
    device.build_output_stream(
        config,
        move |output: &mut [T], _| playback.write(output),
        move |error| {
            let _ = errors.send(error.to_string());
        },
        None,
    )
}

#[derive(Clone, Debug)]
struct Audio {
    sample_rate_hz: u32,
    channels: u16,
    samples: Arc<[f32]>,
}

impl Audio {
    fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels)
    }
}

struct Playback {
    audio: Audio,
    range: Range<usize>,
    output_channels: usize,
    source_position: f64,
    source_step: f64,
}

impl Playback {
    fn new(audio: Audio, range: Range<usize>, output: &StreamConfig) -> Self {
        Self {
            source_step: f64::from(audio.sample_rate_hz) / f64::from(output.sample_rate),
            audio,
            range,
            output_channels: usize::from(output.channels),
            source_position: 0.0,
        }
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "a WAV data chunk is at most u32::MAX bytes, so its frame count is exact in f64"
    )]
    fn write<T>(&mut self, output: &mut [T])
    where
        T: Sample + FromSample<f32>,
    {
        for frame in output.chunks_mut(self.output_channels) {
            for (channel, sample) in frame.iter_mut().enumerate() {
                *sample = T::from_sample(self.sample(channel));
            }
            self.source_position += self.source_step;
            let frames = self.frames() as f64;
            if self.source_position >= frames {
                self.source_position %= frames;
            }
        }
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "the finite playback position is maintained within the non-empty WAV frame range"
    )]
    fn sample(&self, output_channel: usize) -> f32 {
        let frame = self.source_position.floor() as usize;
        let next = (frame + 1) % self.frames();
        let fraction = (self.source_position - frame as f64) as f32;
        let current = self.frame_sample(self.range.start + frame, output_channel);
        let following = self.frame_sample(self.range.start + next, output_channel);
        current + (following - current) * fraction
    }

    fn frame_sample(&self, frame: usize, output_channel: usize) -> f32 {
        let source_channels = usize::from(self.audio.channels);
        if self.output_channels == 1 && source_channels == 2 {
            let offset = frame * source_channels;
            return (self.audio.samples[offset] + self.audio.samples[offset + 1]) * 0.5;
        }
        let channel = if source_channels == 1 {
            0
        } else {
            output_channel.min(source_channels - 1)
        };
        self.audio.samples[frame * source_channels + channel]
    }

    fn frames(&self) -> usize {
        self.range.end - self.range.start
    }
}

fn decode_wav(bytes: &[u8]) -> Result<Audio, WavError> {
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(WavError::InvalidHeader);
    }

    let mut format = None;
    let mut data = None;
    let mut cursor = 12;
    while cursor + 8 <= bytes.len() {
        let identifier = &bytes[cursor..cursor + 4];
        let size = usize::try_from(u32_at(bytes, cursor + 4)?).map_err(|_| WavError::TooLarge)?;
        let start = cursor + 8;
        let end = start.checked_add(size).ok_or(WavError::TooLarge)?;
        let payload = bytes.get(start..end).ok_or(WavError::Truncated)?;
        match identifier {
            b"fmt " => format = Some(decode_format(payload)?),
            b"data" => data = Some(payload),
            _ => {}
        }
        cursor = end.checked_add(size % 2).ok_or(WavError::TooLarge)?;
    }

    let (sample_rate_hz, channels) = format.ok_or(WavError::MissingFormat)?;
    let data = data.ok_or(WavError::MissingData)?;
    let frame_bytes = usize::from(channels) * 2;
    if data.is_empty() || !data.len().is_multiple_of(frame_bytes) {
        return Err(WavError::MisalignedData);
    }
    let samples = data
        .chunks_exact(2)
        .map(|sample| f32::from(i16::from_le_bytes([sample[0], sample[1]])) / 32_768.0)
        .collect::<Vec<_>>()
        .into();
    Ok(Audio {
        sample_rate_hz,
        channels,
        samples,
    })
}

fn decode_format(bytes: &[u8]) -> Result<(u32, u16), WavError> {
    if u16_at(bytes, 0)? != 1 || u16_at(bytes, 14)? != 16 {
        return Err(WavError::UnsupportedFormat);
    }
    let channels = u16_at(bytes, 2)?;
    let sample_rate_hz = u32_at(bytes, 4)?;
    let block_align = u16_at(bytes, 12)?;
    if channels == 0 || sample_rate_hz == 0 || channels.checked_mul(2) != Some(block_align) {
        return Err(WavError::UnsupportedFormat);
    }
    Ok((sample_rate_hz, channels))
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, WavError> {
    let value = bytes.get(offset..offset + 2).ok_or(WavError::Truncated)?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, WavError> {
    let value = bytes.get(offset..offset + 4).ok_or(WavError::Truncated)?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

#[derive(Debug, thiserror::Error)]
enum PlayerError {
    #[error("usage: symphra-player <input.wav> [<start-frame> <end-frame>]")]
    Usage,
    #[error("frame range {start}..{end} is outside the WAV's {frames} frames")]
    InvalidRange {
        start: usize,
        end: usize,
        frames: usize,
    },
    #[error("failed to read WAV: {0}")]
    Read(#[from] std::io::Error),
    #[error(transparent)]
    Wav(#[from] WavError),
    #[error("no default audio output device is available")]
    NoOutputDevice,
    #[error("audio output failed: {0}")]
    Audio(#[from] cpal::Error),
    #[error("unsupported output sample format: {0}")]
    UnsupportedSampleFormat(SampleFormat),
    #[error("audio stream stopped: {0}")]
    StreamStopped(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
enum WavError {
    #[error("input is not a RIFF/WAVE file")]
    InvalidHeader,
    #[error("WAV is truncated")]
    Truncated,
    #[error("WAV is too large")]
    TooLarge,
    #[error("WAV has no format chunk")]
    MissingFormat,
    #[error("WAV has no data chunk")]
    MissingData,
    #[error("only PCM 16-bit WAV is supported")]
    UnsupportedFormat,
    #[error("WAV data must contain complete, non-empty frames")]
    MisalignedData,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_wav_should_read_interleaved_pcm16() {
        let mut wav = Vec::from(&b"RIFF\x28\0\0\0WAVEfmt \x10\0\0\0\x01\0\x02\0\x40\x1f\0\0\0\x7d\0\0\x04\0\x10\0data\x04\0\0\0"[..]);
        wav.extend_from_slice(&[0, 0, 255, 127]);

        let audio = decode_wav(&wav).expect("valid stereo PCM16 should decode");

        assert_eq!(
            (audio.sample_rate_hz, audio.channels, audio.samples.as_ref()),
            (8_000, 2, &[0.0, 32_767.0 / 32_768.0][..])
        );
    }

    #[test]
    fn decode_wav_should_reject_incomplete_frames() {
        let mut wav = Vec::from(&b"RIFF\x27\0\0\0WAVEfmt \x10\0\0\0\x01\0\x02\0\x40\x1f\0\0\0\x7d\0\0\x04\0\x10\0data\x03\0\0\0"[..]);
        wav.extend_from_slice(&[0, 0, 0]);

        let error = decode_wav(&wav).expect_err("partial stereo frame should fail");

        assert_eq!(error, WavError::MisalignedData);
    }

    #[test]
    fn playback_should_loop_at_the_end_of_the_audio() {
        let audio = Audio {
            sample_rate_hz: 2,
            channels: 1,
            samples: Arc::from([0.0, 1.0]),
        };
        let mut playback = Playback::new(
            audio,
            0..2,
            &StreamConfig {
                channels: 1,
                sample_rate: 2,
                buffer_size: cpal::BufferSize::Default,
            },
        );
        let mut output = [0.0_f32; 4];

        playback.write(&mut output);

        assert!(
            output
                .iter()
                .zip([0.0, 1.0, 0.0, 1.0])
                .all(|(actual, expected)| (actual - expected).abs() < f32::EPSILON),
            "looped output differs: {output:?}"
        );
    }

    #[test]
    fn playback_should_loop_only_the_selected_frames() {
        let audio = Audio {
            sample_rate_hz: 4,
            channels: 1,
            samples: Arc::from([0.0, 0.25, 0.5, 0.75]),
        };
        let mut playback = Playback::new(
            audio,
            1..3,
            &StreamConfig {
                channels: 1,
                sample_rate: 4,
                buffer_size: cpal::BufferSize::Default,
            },
        );
        let mut output = [0.0_f32; 4];

        playback.write(&mut output);

        assert!(
            output
                .iter()
                .zip([0.25, 0.5, 0.25, 0.5])
                .all(|(actual, expected)| (actual - expected).abs() < f32::EPSILON),
            "section-looped output differs: {output:?}"
        );
    }
}
