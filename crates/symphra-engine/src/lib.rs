//! Source-to-audio orchestration for Symphra.

use symphra_compiler::{CompileDiagnostic, ScheduleError, compile, schedule};
use symphra_render::{
    RenderError, render_song_with_assets, render_song_with_assets_with_cache,
    render_song_with_assets_with_cache_selected, render_song_with_assets_with_progress,
    render_song_with_samples,
};
use symphra_syntax::{Diagnostic, ParsedSource, parse};

pub use symphra_render::{AudioBuffer, TrackRenderCache, TrackSelection};
pub use symphra_sampler::{
    DecodeError, Sample, SampleLibrary, decode_wav, named_sample_source, packed_sample_source,
};
pub use symphra_score::{InstrumentKind, SampleSelector, Score, Track};
pub use symphra_soundfont::{
    DecodeError as SoundFontDecodeError, SoundFontLibrary, decode_soundfont,
};
pub use symphra_syntax::{SourceId, SourceSpan, SourceText};
pub use symphra_vst3::{Vst3Error, Vst3Library, validate_plugin};

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum EngineError {
    #[error("source contains syntax errors")]
    Syntax(Vec<Diagnostic>),
    #[error("source contains semantic errors")]
    Compile(Vec<CompileDiagnostic>),
    #[error(transparent)]
    Schedule(#[from] ScheduleError),
    #[error(transparent)]
    Render(#[from] RenderError),
}

/// Compiles and renders one song from a Symphra source buffer.
///
/// # Errors
///
/// Returns structured syntax or compile diagnostics before scheduling and
/// rendering errors. No later stage runs after an earlier stage fails.
pub fn render_source(source: &SourceText, song_index: usize) -> Result<AudioBuffer, EngineError> {
    let score = compile_source(source)?;
    render_score(&score, song_index, &SampleLibrary::default())
}

/// Compiles source into a score without loading or rendering audio assets.
///
/// # Errors
///
/// Returns structured syntax, compile, or scheduling errors.
pub fn compile_source(source: &SourceText) -> Result<Score, EngineError> {
    let ParsedSource { file, diagnostics } = parse(source.id, &source.text);
    if !diagnostics.is_empty() {
        return Err(EngineError::Syntax(diagnostics));
    }
    let program = compile(&file).map_err(EngineError::Compile)?;
    schedule(&program).map_err(EngineError::Schedule)
}

/// Renders a compiled score using preloaded sample assets.
///
/// # Errors
///
/// Returns [`EngineError::Render`] when the score or sample library cannot be
/// rendered.
pub fn render_score(
    score: &Score,
    song_index: usize,
    samples: &SampleLibrary,
) -> Result<AudioBuffer, EngineError> {
    render_song_with_samples(score, song_index, samples).map_err(EngineError::Render)
}

/// Renders a compiled score using preloaded sample, `SoundFont`, and VST3
/// assets.
///
/// # Errors
///
/// Returns [`EngineError::Render`] when the score, sample library,
/// soundfont library, or vst3 library cannot be rendered.
pub fn render_score_with_assets(
    score: &Score,
    song_index: usize,
    samples: &SampleLibrary,
    soundfonts: &SoundFontLibrary,
    vst3s: &Vst3Library,
) -> Result<AudioBuffer, EngineError> {
    render_song_with_assets(score, song_index, samples, soundfonts, vst3s)
        .map_err(EngineError::Render)
}

/// Renders a compiled score and reports each completed track through
/// `progress`.
///
/// # Errors
///
/// Returns [`EngineError::Render`] when the score or a required asset is
/// invalid or unavailable.
pub fn render_score_with_assets_with_progress(
    score: &Score,
    song_index: usize,
    samples: &SampleLibrary,
    soundfonts: &SoundFontLibrary,
    vst3s: &Vst3Library,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<AudioBuffer, EngineError> {
    render_song_with_assets_with_progress(score, song_index, samples, soundfonts, vst3s, progress)
        .map_err(EngineError::Render)
}

/// Renders a compiled score while reusing post-effect track audio supplied by
/// `cache`.
///
/// # Errors
///
/// Returns [`EngineError::Render`] when the score or a required asset is
/// invalid or unavailable.
pub fn render_score_with_assets_with_cache(
    score: &Score,
    song_index: usize,
    samples: &SampleLibrary,
    soundfonts: &SoundFontLibrary,
    vst3s: &Vst3Library,
    cache: &mut dyn TrackRenderCache,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<AudioBuffer, EngineError> {
    render_song_with_assets_with_cache(
        score,
        song_index,
        samples,
        soundfonts,
        vst3s,
        Some(cache),
        progress,
    )
    .map_err(EngineError::Render)
}

/// Renders selected track declarations while reusing post-effect track audio
/// supplied by `cache`. The output keeps the full song duration.
///
/// # Errors
///
/// Returns [`EngineError::Render`] when the score or a required asset is
/// invalid or unavailable.
#[expect(
    clippy::too_many_arguments,
    reason = "the renderer accepts three asset libraries plus selection, cache, and progress"
)]
pub fn render_score_with_assets_with_cache_selected(
    score: &Score,
    song_index: usize,
    samples: &SampleLibrary,
    soundfonts: &SoundFontLibrary,
    vst3s: &Vst3Library,
    selection: &TrackSelection,
    cache: &mut dyn TrackRenderCache,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<AudioBuffer, EngineError> {
    render_song_with_assets_with_cache_selected(
        score,
        song_index,
        samples,
        soundfonts,
        vst3s,
        selection,
        Some(cache),
        progress,
    )
    .map_err(EngineError::Render)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::{
        EngineError, Sample, SampleLibrary, SourceId, SourceText, compile_source,
        named_sample_source, packed_sample_source, render_score, render_source,
    };

    const SOURCE: &str = r#"
project {
  seed 1
  sample_rate 8khz
  output mono
}
song "Test" {
  tempo 120bpm
  meter 4/4
  key C major
  pattern melody = sequence {
    note A4 for 1/4
  }
}
"#;

    #[test]
    fn render_source_should_run_the_complete_audio_pipeline() {
        let source = SourceText::new(SourceId(0), "test.sym", SOURCE);

        let audio = render_source(&source, 0).expect("valid source should render");

        assert_eq!(
            (audio.sample_rate_hz, audio.channels, audio.frames()),
            (8_000, 1, 4_000)
        );
    }

    #[test]
    fn render_source_should_preserve_syntax_diagnostics() {
        let source = SourceText::new(SourceId(7), "bad.sym", "project { @ }");

        let error = render_source(&source, 0).expect_err("invalid syntax should fail");

        let EngineError::Syntax(diagnostics) = error else {
            panic!("syntax errors should remain distinguishable");
        };
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.span.source == SourceId(7))
        );
    }

    #[test]
    fn render_source_should_follow_explicit_arrangement() {
        let source = SourceText::new(
            SourceId(0),
            "arranged.sym",
            r#"
project { seed 1 sample_rate 8khz output mono }
song "Arranged" {
  tempo 120bpm meter 4/4 key C major
  pattern first = sequence { note C4 for 1/4 }
  pattern second = sequence { note G4 for 1/4 }
  arrangement { second first second }
}
"#,
        );

        let audio = render_source(&source, 0).expect("arranged source should render");

        assert_eq!(audio.frames(), 12_000);
    }

    #[test]
    fn render_source_should_preserve_trailing_rest() {
        let source = SourceText::new(
            SourceId(0),
            "rest.sym",
            r#"
project { seed 1 sample_rate 8khz output mono }
song "Rest" {
  tempo 120bpm meter 4/4 key C major
  pattern phrase = sequence { note C4 for 1/4 rest for 1/4 }
}
"#,
        );

        let audio = render_source(&source, 0).expect("rest should render");

        assert_eq!(
            (
                audio.frames(),
                audio.samples[4_000..].iter().all(|sample| *sample == 0.0)
            ),
            (8_000, true)
        );
    }

    #[test]
    fn render_source_should_mix_chord_pitches() {
        let chord = SourceText::new(
            SourceId(0),
            "chord.sym",
            r#"
project { seed 1 sample_rate 8khz output mono }
song "Chord" {
  tempo 120bpm meter 4/4 key C major
  pattern harmony = sequence { chord C4 E4 G4 for 1/4 }
}
"#,
        );
        let note = SourceText::new(
            SourceId(1),
            "note.sym",
            r#"
project { seed 1 sample_rate 8khz output mono }
song "Note" {
  tempo 120bpm meter 4/4 key C major
  pattern melody = sequence { note C4 for 1/4 }
}
"#,
        );

        let chord_audio = render_source(&chord, 0).expect("chord should render");
        let note_audio = render_source(&note, 0).expect("note should render");

        assert_eq!(chord_audio.frames(), 4_000);
        assert_ne!(chord_audio.samples, note_audio.samples);
    }

    #[test]
    fn render_source_should_apply_velocity() {
        let source = SourceText::new(
            SourceId(0),
            "velocity.sym",
            r#"
project { seed 1 sample_rate 8khz output mono }
song "Velocity" {
  tempo 120bpm meter 4/4 key C major
  pattern silent = sequence { note C4 for 1/4 velocity 0 }
}
"#,
        );

        let audio = render_source(&source, 0).expect("velocity should render");

        assert_eq!(
            (
                audio.frames(),
                audio.samples.iter().all(|sample| *sample == 0.0)
            ),
            (4_000, true)
        );
    }

    #[test]
    fn render_source_should_apply_arrangement_instruments() {
        let source = SourceText::new(
            SourceId(0),
            "instrument.sym",
            r#"
project { seed 1 sample_rate 8khz output mono }
song "Instrument" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = triangle
  pattern phrase = sequence { note A4 for 1/4 }
  arrangement { phrase with lead phrase }
}
"#,
        );

        let audio = render_source(&source, 0).expect("instrument should render");

        assert_ne!(audio.samples[..4_000], audio.samples[4_000..]);
    }

    #[test]
    fn render_score_should_play_preloaded_samples() {
        let source = SourceText::new(
            SourceId(0),
            "sample.sym",
            r#"
project { seed 1 sample_rate 8khz output mono }
song "Sample" {
  tempo 120bpm meter 4/4 key C major
  instrument piano = sampled {
    source "samples/piano-c4.wav"
    root C4
  }
  pattern phrase = sequence { note C4 for 1/4 }
  arrangement { phrase with piano }
}
"#,
        );
        let score = compile_source(&source).expect("sample instrument should compile");
        let mut samples = SampleLibrary::default();
        samples.insert(
            "samples/piano-c4.wav",
            Sample {
                sample_rate_hz: NonZeroU32::new(8_000).expect("sample rate should be non-zero"),
                samples: vec![0.5; 4_000],
            },
        );

        let audio = render_score(&score, 0, &samples).expect("loaded sample should render");

        assert_eq!(audio.frames(), 4_000);
        assert!(audio.samples[100..3_900].iter().any(|sample| *sample > 0.0));
    }

    #[test]
    fn render_score_should_play_samples_from_packs() {
        let source = SourceText::new(
            SourceId(0),
            "pack.sym",
            r#"
project { seed 1 sample_rate 8khz output mono }
song "Pack" {
  tempo 120bpm meter 4/4 key C major
  instrument voice = sampler { pack "numbers" }
  pattern phrase = steps 1/8 { sample 1 rest sample 3 }
  arrangement { phrase with voice }
}
"#,
        );
        let score = compile_source(&source).expect("sample pack should compile");
        let mut samples = SampleLibrary::default();
        for index in [1, 3] {
            samples.insert(
                packed_sample_source("numbers", index),
                Sample {
                    sample_rate_hz: NonZeroU32::new(8_000).expect("sample rate should be non-zero"),
                    samples: vec![0.5; 2_000],
                },
            );
        }

        let audio = render_score(&score, 0, &samples).expect("sample pack should render");

        assert_eq!(
            (
                audio.frames(),
                audio.samples[2_000..4_000]
                    .iter()
                    .all(|sample| *sample == 0.0)
            ),
            (6_000, true)
        );
    }

    #[test]
    fn render_score_should_play_samples_from_drum_banks() {
        let source = SourceText::new(
            SourceId(0),
            "drums.sym",
            r#"
project { seed 1 sample_rate 8khz output mono }
song "Drums" {
  tempo 120bpm meter 4/4 key C major
  instrument tr909 = drum_machine { bank "RolandTR909" }
  pattern kit = steps 1/8 { drum "bd" rest drum "hh" }
  arrangement { kit with tr909 }
}
"#,
        );
        let score = compile_source(&source).expect("drum bank should compile");
        let mut samples = SampleLibrary::default();
        for name in ["bd", "hh"] {
            samples.insert(
                named_sample_source("RolandTR909", name),
                Sample {
                    sample_rate_hz: NonZeroU32::new(8_000).expect("sample rate should be non-zero"),
                    samples: vec![0.5; 2_000],
                },
            );
        }

        let audio = render_score(&score, 0, &samples).expect("drum bank should render");

        assert_eq!(
            (
                audio.frames(),
                audio.samples[2_000..4_000]
                    .iter()
                    .all(|sample| *sample == 0.0)
            ),
            (6_000, true)
        );
    }
}
