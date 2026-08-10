//! VST3 plugin loading and per-track offline rendering.
//!
//! A VST3 instrument plugin is a persistent, stateful object, unlike every
//! other instrument kind in this workspace (which are rendered as one
//! independent [`symphra_dsp`]-style voice per note). Feeding it note
//! events one voice at a time would break chords, voice-stealing, and any
//! cross-note state the plugin keeps — so this crate exposes one whole-track
//! render function ([`render_vst3_track`]) instead of a per-note voice, built
//! on the [`vst3-host`](https://docs.rs/vst3-host) crate (MIT-licensed, zero
//! `unsafe` in its public API).

use std::collections::HashSet;
use std::num::NonZeroU32;

use vst3_host::{AudioBuffers, MidiChannel, MidiClip, MidiEvent, Plugin, Timeline, simple};

/// Block size used for offline rendering. Arbitrary but conventional; this
/// is an offline, faster-than-realtime render, so it only affects how many
/// times `process_audio` is called, not latency.
const BLOCK_SIZE: usize = 512;

/// A cache of VST3 plugin source paths already confirmed loadable by
/// [`validate_plugin`]. Unlike [`symphra_sampler::SampleLibrary`] or
/// `symphra_soundfont::SoundFontLibrary`, this cannot cache the loaded asset
/// itself — a `vst3_host::Plugin` owns an exclusive loaded native module and
/// live COM instance, so it isn't `Clone`/shareable the way decoded sample
/// or `SoundFont` data is. Caching the validated path is what still lets a
/// caller (e.g. the CLI) surface a "plugin not found" error before
/// rendering starts, even though the real, per-track plugin instantiation
/// is unavoidably deferred to [`render_vst3_track`].
#[derive(Clone, Default)]
pub struct Vst3Library {
    sources: HashSet<String>,
}

impl Vst3Library {
    pub fn insert(&mut self, source: impl Into<String>) {
        self.sources.insert(source.into());
    }

    #[must_use]
    pub fn contains(&self, source: &str) -> bool {
        self.sources.contains(source)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Vst3Error {
    #[error("vst3 plugin could not be loaded: {0}")]
    LoadFailed(String),
    #[error("vst3 plugin has no preset named `{0}`")]
    UnknownPreset(String),
    #[error("vst3 plugin processing failed: {0}")]
    ProcessFailed(String),
}

impl From<vst3_host::Error> for Vst3Error {
    fn from(error: vst3_host::Error) -> Self {
        Self::LoadFailed(error.to_string())
    }
}

/// Confirms `source` is a loadable VST3 plugin by loading it once (at a
/// throwaway sample rate/block size) and immediately dropping it.
///
/// This is the "can this asset actually be used" preload step, mirroring
/// `symphra_soundfont::decode_soundfont`'s role: a caller (the CLI) runs
/// this once per unique source before rendering, so a missing or broken
/// plugin surfaces as a clean preload error rather than a mid-render one.
///
/// # Errors
///
/// Returns [`Vst3Error::LoadFailed`] when `source` cannot be loaded as a
/// VST3 plugin.
pub fn validate_plugin(source: &str) -> Result<(), Vst3Error> {
    simple::get_plugin_info(source)
        .map(|_| ())
        .map_err(Vst3Error::from)
}

/// One note-on/note-off pair to feed a VST3 instrument, in already-resolved
/// sample frames — mirrors `symphra_soundfont::SoundFontVoice::new`'s
/// raw-parameter style rather than depending on `symphra-score` types.
#[derive(Clone, Copy, Debug)]
pub struct Vst3Note {
    pub start_frame: u64,
    pub end_frame: u64,
    pub midi_pitch: u8,
    pub velocity: u8,
}

/// Renders one track's worth of note events through a VST3 instrument
/// plugin in a single offline pass, returning interleaved stereo `f32`
/// samples (`2 * total_frames` long).
///
/// # Errors
///
/// Returns [`Vst3Error`] when `source` cannot be loaded, `preset` (if any)
/// does not match a program in the plugin's program list, or the plugin's
/// `process()` call fails.
pub fn render_vst3_track(
    source: &str,
    preset: Option<&str>,
    sample_rate_hz: NonZeroU32,
    total_frames: u64,
    notes: &[Vst3Note],
) -> Result<Vec<f32>, Vst3Error> {
    let total_frames = usize::try_from(total_frames).map_err(|_| {
        Vst3Error::ProcessFailed("render length does not fit this platform's memory".to_string())
    })?;
    let sample_rate = f64::from(sample_rate_hz.get());
    let mut plugin = simple::load_plugin_with_settings(source, sample_rate, BLOCK_SIZE)?;

    if let Some(preset) = preset {
        select_preset(&mut plugin, preset)?;
    }

    plugin.start_processing()?;

    let mut timeline =
        Timeline::new(sample_rate, timeline_bpm(sample_rate)).with_clip(note_clip(notes));

    let mut output = vec![0.0f32; total_frames * 2];
    let mut rendered = 0usize;
    while rendered < total_frames {
        let frames = BLOCK_SIZE.min(total_frames - rendered);
        let mut buffers = AudioBuffers::new(0, 2, frames, sample_rate);
        timeline
            .drive_block(&mut plugin, &mut buffers)
            .map_err(|error| Vst3Error::ProcessFailed(error.to_string()))?;
        for frame in 0..frames {
            output[(rendered + frame) * 2] = buffers.outputs[0][frame];
            output[(rendered + frame) * 2 + 1] = buffers.outputs[1][frame];
        }
        rendered += frames;
    }

    plugin.stop_processing()?;
    Ok(output)
}

/// A tempo chosen so `samples_per_beat == sample_rate * 60 / bpm == 1`: a
/// [`MidiClip`] "beat" position then *is* a frame index, letting
/// already-resolved `start_frame`/`end_frame` values (computed by the
/// caller via the same time-to-frame conversion every other instrument kind
/// uses) feed straight into `MidiClip::with` with no separate tempo/meter
/// conversion in this crate.
fn timeline_bpm(sample_rate: f64) -> f64 {
    sample_rate * 60.0
}

#[expect(
    clippy::cast_precision_loss,
    reason = "frame indices are resolved audio-length values, far below f64's exact-integer range"
)]
fn note_clip(notes: &[Vst3Note]) -> MidiClip {
    let mut clip = MidiClip::new();
    for note in notes {
        clip = clip
            .with(
                note.start_frame as f64,
                MidiEvent::NoteOn {
                    channel: MidiChannel::Ch1,
                    note: note.midi_pitch,
                    velocity: note.velocity,
                },
            )
            .with(
                note.end_frame as f64,
                MidiEvent::NoteOff {
                    channel: MidiChannel::Ch1,
                    note: note.midi_pitch,
                    velocity: 0,
                },
            );
    }
    clip
}

/// Resolves `preset` to a program index by exact-name match over every unit
/// exposed by `IUnitInfo` (`Plugin::get_units`), then selects it. Plugins
/// without `IUnitInfo` support (or without a program list at all) report an
/// empty unit list, which falls through to [`Vst3Error::UnknownPreset`] —
/// like `soundfont`'s `preset`, this can only be verified once the plugin is
/// actually loaded, not at compile time.
fn select_preset(plugin: &mut Plugin, preset: &str) -> Result<(), Vst3Error> {
    for unit in plugin.get_units()? {
        if let Some(index) = unit.programs.iter().position(|name| name == preset) {
            let index =
                i32::try_from(index).map_err(|_| Vst3Error::UnknownPreset(preset.to_string()))?;
            return plugin
                .select_program(unit.id, index)
                .map_err(Vst3Error::from);
        }
    }
    Err(Vst3Error::UnknownPreset(preset.to_string()))
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::{Vst3Library, render_vst3_track, validate_plugin};

    #[test]
    fn validate_plugin_should_reject_a_nonexistent_path() {
        let error = validate_plugin("/nonexistent/path/plugin.vst3")
            .expect_err("a nonexistent path should not validate");

        assert!(error.to_string().contains("could not be loaded"));
    }

    #[test]
    fn render_vst3_track_should_reject_a_nonexistent_path() {
        let sample_rate = NonZeroU32::new(44_100).expect("sample rate should be non-zero");

        let error = render_vst3_track(
            "/nonexistent/path/plugin.vst3",
            None,
            sample_rate,
            1_000,
            &[],
        )
        .expect_err("a nonexistent path should not render");

        assert!(error.to_string().contains("could not be loaded"));
    }

    #[test]
    fn vst3_library_tracks_inserted_sources() {
        let mut library = Vst3Library::default();
        assert!(!library.contains("plugins/synth.vst3"));

        library.insert("plugins/synth.vst3");
        assert!(library.contains("plugins/synth.vst3"));
    }
}
