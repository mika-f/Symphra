//! `SoundFont` (.sf2) loading and deterministic pitched instrument playback.

use std::collections::HashMap;
use std::io::Cursor;
use std::num::NonZeroU32;
use std::sync::Arc;

use rustysynth::{Preset, SoundFont, Synthesizer, SynthesizerSettings};

#[derive(Clone, Default)]
pub struct SoundFontLibrary {
    fonts: HashMap<String, Arc<SoundFont>>,
}

impl SoundFontLibrary {
    pub fn insert(&mut self, source: impl Into<String>, font: Arc<SoundFont>) {
        self.fonts.insert(source.into(), font);
    }

    #[must_use]
    pub fn get(&self, source: &str) -> Option<&Arc<SoundFont>> {
        self.fonts.get(source)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("soundfont file is malformed or unsupported: {0}")]
    InvalidFont(String),
}

/// Decodes a `SoundFont` (.sf2) file from bytes.
///
/// # Errors
///
/// Returns [`DecodeError`] when the bytes are not a valid `SoundFont` file.
pub fn decode_soundfont(bytes: &[u8]) -> Result<SoundFont, DecodeError> {
    let mut cursor = Cursor::new(bytes);
    SoundFont::new(&mut cursor).map_err(|error| DecodeError::InvalidFont(error.to_string()))
}

/// Finds a preset by name (exact match).
#[must_use]
pub fn find_preset<'a>(font: &'a SoundFont, preset_name: &str) -> Option<&'a Preset> {
    font.get_presets()
        .iter()
        .find(|preset| preset.get_name() == preset_name)
}

/// A single-note voice backed by a `SoundFont` preset. Renders mono audio (the
/// synthesizer's stereo output averaged down to one channel, matching every
/// other instrument kind's mono-voice-under-one-track-pan model) one sample
/// at a time via an internal block buffer, refilled from
/// [`rustysynth::Synthesizer::render`] as needed.
pub struct SoundFontVoice {
    synthesizer: Synthesizer,
    left: Vec<f32>,
    right: Vec<f32>,
    cursor: usize,
}

impl SoundFontVoice {
    /// Starts a note immediately (`note_on`) on `bank`/`patch`, selected via
    /// MIDI bank-select (CC0 MSB, CC32 LSB) and program-change messages on
    /// channel 0.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] when the synthesizer cannot be constructed for
    /// `sample_rate_hz`.
    pub fn new(
        font: &Arc<SoundFont>,
        sample_rate_hz: NonZeroU32,
        bank: i32,
        patch: i32,
        midi_pitch: u8,
        velocity: u8,
    ) -> Result<Self, DecodeError> {
        let sample_rate = i32::try_from(sample_rate_hz.get()).unwrap_or(i32::MAX);
        let settings = SynthesizerSettings::new(sample_rate);
        let mut synthesizer = Synthesizer::new(font, &settings)
            .map_err(|error| DecodeError::InvalidFont(error.to_string()))?;
        synthesizer.process_midi_message(0, 0xB0, 0x00, (bank >> 7) & 0x7F);
        synthesizer.process_midi_message(0, 0xB0, 0x20, bank & 0x7F);
        synthesizer.process_midi_message(0, 0xC0, patch & 0x7F, 0);
        synthesizer.note_on(0, i32::from(midi_pitch), i32::from(velocity));
        let block_size = synthesizer.get_block_size();
        Ok(Self {
            synthesizer,
            left: vec![0.0; block_size],
            right: vec![0.0; block_size],
            cursor: block_size,
        })
    }

    #[must_use]
    pub fn next_sample(&mut self) -> f32 {
        if self.cursor >= self.left.len() {
            self.synthesizer.render(&mut self.left, &mut self.right);
            self.cursor = 0;
        }
        let value = f32::midpoint(self.left[self.cursor], self.right[self.cursor]);
        self.cursor += 1;
        value
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::sync::Arc;

    use super::{DecodeError, SoundFontVoice, decode_soundfont, find_preset};

    #[test]
    fn decode_soundfont_should_reject_non_soundfont_bytes() {
        let error = decode_soundfont(b"not a soundfont").expect_err("garbage bytes should fail");

        assert!(matches!(error, DecodeError::InvalidFont(_)));
    }

    #[test]
    fn decode_soundfont_should_read_a_minimal_font_and_find_its_preset() {
        let font = decode_soundfont(&minimal_soundfont()).expect("minimal font should decode");

        let preset = find_preset(&font, "music_box").expect("preset should be found by name");

        assert_eq!(
            (preset.get_bank_number(), preset.get_patch_number()),
            (0, 0)
        );
        assert!(find_preset(&font, "nonexistent").is_none());
    }

    #[test]
    fn soundfont_voice_should_render_audible_audio() {
        let font = Arc::new(decode_soundfont(&minimal_soundfont()).expect("font should decode"));
        let preset = find_preset(&font, "music_box").expect("preset should be found");
        let (bank, patch) = (preset.get_bank_number(), preset.get_patch_number());
        let sample_rate = NonZeroU32::new(44_100).expect("sample rate should be non-zero");

        let mut voice = SoundFontVoice::new(&font, sample_rate, bank, patch, 60, 100)
            .expect("voice should be constructed");

        let samples: Vec<f32> = (0..2_000).map(|_| voice.next_sample()).collect();
        assert!(samples.iter().any(|sample| sample.abs() > f32::EPSILON));
    }

    /// Builds the smallest `SoundFont` (.sf2) byte buffer `rustysynth`
    /// accepts: one 8-sample mono tone, one instrument zone selecting it
    /// across the full key range (the default when no `keyRange` generator
    /// is present), and one preset (bank 0, patch 0) named `"music_box"`
    /// pointing at that instrument. Mirrors the WAV byte-builder convention
    /// used elsewhere in this workspace's tests (no fixture files) — the
    /// `SoundFont` 2.01 spec's RIFF layout just has more mandatory chunks.
    fn minimal_soundfont() -> Vec<u8> {
        fn name20(text: &str) -> [u8; 20] {
            let mut buffer = [0u8; 20];
            let bytes = text.as_bytes();
            let len = bytes.len().min(20);
            buffer[..len].copy_from_slice(&bytes[..len]);
            buffer
        }

        fn chunk(id: [u8; 4], payload: &[u8]) -> Vec<u8> {
            let mut out = Vec::new();
            out.extend_from_slice(&id);
            let size = u32::try_from(payload.len()).expect("test payload should fit");
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(payload);
            if !payload.len().is_multiple_of(2) {
                out.push(0);
            }
            out
        }

        fn list(list_type: [u8; 4], subchunks: &[Vec<u8>]) -> Vec<u8> {
            let mut payload = Vec::new();
            payload.extend_from_slice(&list_type);
            for subchunk in subchunks {
                payload.extend_from_slice(subchunk);
            }
            chunk(*b"LIST", &payload)
        }

        // One 8-sample tone plus two trailing zero "guard" samples, as the
        // SoundFont spec recommends past a sample's declared end.
        let mut wave = Vec::new();
        for value in [
            8_000i16, -8_000, 8_000, -8_000, 8_000, -8_000, 8_000, -8_000, 0, 0,
        ] {
            wave.extend_from_slice(&value.to_le_bytes());
        }

        // phdr: one preset (bank 0, patch 0, zone 0) plus the required
        // terminal record (`bag_index` one past the last real zone).
        let mut phdr = Vec::new();
        phdr.extend_from_slice(&name20("music_box"));
        phdr.extend_from_slice(&[0u8; 6]); // patch, bank, bag_index all 0
        phdr.extend_from_slice(&[0u8; 12]); // library, genre, morphology
        phdr.extend_from_slice(&name20("EOP"));
        phdr.extend_from_slice(&0u16.to_le_bytes());
        phdr.extend_from_slice(&0u16.to_le_bytes());
        phdr.extend_from_slice(&1u16.to_le_bytes()); // terminal bag_index
        phdr.extend_from_slice(&[0u8; 12]);

        // pbag: one zone (generators 0..1) plus the terminal record.
        let mut pbag = Vec::new();
        pbag.extend_from_slice(&[0u8; 4]); // generator_index 0, modulator_index 0
        pbag.extend_from_slice(&1u16.to_le_bytes()); // terminal generator_index
        pbag.extend_from_slice(&0u16.to_le_bytes());

        // pgen: a single `instrument` generator selecting instrument 0,
        // plus the terminal record (content unused, only counted).
        let mut pgen = Vec::new();
        pgen.extend_from_slice(&41u16.to_le_bytes()); // GeneratorType::Instrument
        pgen.extend_from_slice(&0u16.to_le_bytes());
        pgen.extend_from_slice(&[0u8; 4]);

        // inst: one instrument (zone 0) plus the terminal record.
        let mut inst = Vec::new();
        inst.extend_from_slice(&name20("music_box_inst"));
        inst.extend_from_slice(&0u16.to_le_bytes());
        inst.extend_from_slice(&name20("EOI"));
        inst.extend_from_slice(&1u16.to_le_bytes());

        // ibag: one zone (generators 0..1) plus the terminal record.
        let mut ibag = Vec::new();
        ibag.extend_from_slice(&[0u8; 4]);
        ibag.extend_from_slice(&1u16.to_le_bytes());
        ibag.extend_from_slice(&0u16.to_le_bytes());

        // igen: a single `sampleID` generator selecting sample 0, plus the
        // terminal record. No `keyRange` generator, so the default (the
        // full 0..127 key range) applies.
        let mut igen = Vec::new();
        igen.extend_from_slice(&53u16.to_le_bytes()); // GeneratorType::SampleId
        igen.extend_from_slice(&0u16.to_le_bytes());
        igen.extend_from_slice(&[0u8; 4]);

        // shdr: one mono sample (8 real frames of the 10-frame buffer)
        // plus the terminal record.
        let mut shdr = Vec::new();
        shdr.extend_from_slice(&name20("tone"));
        shdr.extend_from_slice(&0i32.to_le_bytes()); // start
        shdr.extend_from_slice(&8i32.to_le_bytes()); // end
        shdr.extend_from_slice(&0i32.to_le_bytes()); // start_loop
        shdr.extend_from_slice(&0i32.to_le_bytes()); // end_loop
        shdr.extend_from_slice(&44_100i32.to_le_bytes()); // sample_rate
        shdr.push(60); // original_pitch
        shdr.push(0); // pitch_correction
        shdr.extend_from_slice(&0u16.to_le_bytes()); // link
        shdr.extend_from_slice(&1u16.to_le_bytes()); // sample_type: mono
        shdr.extend_from_slice(&name20("EOS"));
        shdr.extend_from_slice(&[0u8; 26]);

        let info = list(*b"INFO", &[]);
        let sdta = list(*b"sdta", &[chunk(*b"smpl", &wave)]);
        let pdta = list(
            *b"pdta",
            &[
                chunk(*b"phdr", &phdr),
                chunk(*b"pbag", &pbag),
                chunk(*b"pmod", &[]),
                chunk(*b"pgen", &pgen),
                chunk(*b"inst", &inst),
                chunk(*b"ibag", &ibag),
                chunk(*b"imod", &[]),
                chunk(*b"igen", &igen),
                chunk(*b"shdr", &shdr),
            ],
        );

        let mut riff_payload = Vec::new();
        riff_payload.extend_from_slice(b"sfbk");
        riff_payload.extend_from_slice(&info);
        riff_payload.extend_from_slice(&sdta);
        riff_payload.extend_from_slice(&pdta);

        chunk(*b"RIFF", &riff_payload)
    }
}
