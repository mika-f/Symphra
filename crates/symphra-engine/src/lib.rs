//! Source-to-audio orchestration for Symphra.

use symphra_compiler::{CompileDiagnostic, ScheduleError, compile, schedule};
use symphra_render::{RenderError, render_song};
use symphra_syntax::{Diagnostic, ParsedSource, parse};

pub use symphra_render::AudioBuffer;
pub use symphra_syntax::{SourceId, SourceSpan, SourceText};

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
    let ParsedSource { file, diagnostics } = parse(source.id, &source.text);
    if !diagnostics.is_empty() {
        return Err(EngineError::Syntax(diagnostics));
    }
    let program = compile(&file).map_err(EngineError::Compile)?;
    let score = schedule(&program)?;
    render_song(&score, song_index).map_err(EngineError::Render)
}

#[cfg(test)]
mod tests {
    use super::{EngineError, SourceId, SourceText, render_source};

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
  arrangement { second first }
}
"#,
        );

        let audio = render_source(&source, 0).expect("arranged source should render");

        assert_eq!(audio.frames(), 8_000);
    }
}
