use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use symphra_engine::{EngineError, SourceId, SourceText, render_source};
use symphra_export::{ExportError, encode_wav};

fn main() -> ExitCode {
    match run(env::args_os().skip(1)) {
        Ok(output) => {
            println!("wrote {}", output.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: impl IntoIterator<Item = OsString>) -> Result<PathBuf, CliError> {
    let mut args = args.into_iter();
    let input = PathBuf::from(args.next().ok_or(CliError::Usage)?);
    let output = args
        .next()
        .map_or_else(|| input.with_extension("wav"), PathBuf::from);
    if args.next().is_some() {
        return Err(CliError::Usage);
    }

    let text = fs::read_to_string(&input).map_err(|source| CliError::Read {
        path: input.display().to_string(),
        source,
    })?;
    let wav = source_to_wav(input.display().to_string(), text)?;
    fs::write(&output, wav).map_err(|source| CliError::Write {
        path: output.display().to_string(),
        source,
    })?;
    Ok(output)
}

fn source_to_wav(name: String, text: String) -> Result<Vec<u8>, CliError> {
    let source = SourceText::new(SourceId(0), name, text);
    let audio = render_source(&source, 0).map_err(|error| engine_error(&source, error))?;
    encode_wav(&audio).map_err(CliError::Export)
}

fn engine_error(source: &SourceText, error: EngineError) -> CliError {
    match error {
        EngineError::Syntax(diagnostics) => CliError::Diagnostics(
            diagnostics
                .iter()
                .map(|diagnostic| {
                    format_diagnostic(source, &diagnostic.message, diagnostic.span.start)
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        EngineError::Compile(diagnostics) => CliError::Diagnostics(
            diagnostics
                .iter()
                .map(|diagnostic| {
                    format_diagnostic(source, &diagnostic.message, diagnostic.span.start)
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        error => CliError::Engine(error),
    }
}

fn format_diagnostic(source: &SourceText, message: &str, offset: u32) -> String {
    let offset = usize::try_from(offset)
        .unwrap_or(source.text.len())
        .min(source.text.len());
    let prefix = source.text.get(..offset).unwrap_or(&source.text);
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .chars()
        .count()
        + 1;
    format!("{}:{line}:{column}: error: {message}", source.name)
}

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("usage: symphra <input.sym> [output.wav]")]
    Usage,
    #[error("failed to read `{path}`: {source}")]
    Read {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("{0}")]
    Diagnostics(String),
    #[error(transparent)]
    Engine(EngineError),
    #[error(transparent)]
    Export(ExportError),
    #[error("failed to write `{path}`: {source}")]
    Write {
        path: String,
        #[source]
        source: io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::{SourceId, SourceText, format_diagnostic, source_to_wav};

    #[test]
    fn source_to_wav_should_run_the_complete_offline_pipeline() {
        let wav = source_to_wav(
            "test.sym".to_owned(),
            r#"
project { seed 1 sample_rate 8khz output mono }
song "Test" {
  tempo 120bpm
  meter 4/4
  key C major
  pattern melody = sequence { note A4 for 1/4 }
}
"#
            .to_owned(),
        )
        .expect("valid source should encode");

        assert_eq!(
            (&wav[0..4], &wav[8..12]),
            (&b"RIFF"[..], &b"WAVE"[..])
        );
    }

    #[test]
    fn format_diagnostic_should_count_unicode_columns() {
        let source = SourceText::new(SourceId(0), "bad.sym", "aé\n@");

        let diagnostic = format_diagnostic(&source, "unexpected character", 4);

        assert_eq!(diagnostic, "bad.sym:2:1: error: unexpected character");
    }
}
