use std::borrow::Cow;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
use symphra_engine::{
    DecodeError, EngineError, SampleLibrary, SampleSelector, Score, SourceId, SourceSpan,
    SourceText, compile_source, decode_wav, named_sample_source, packed_sample_source,
    render_score,
};
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
    let source_path = PathBuf::from(&name);
    let source = SourceText::new(SourceId(0), name, text);
    let score = compile_source(&source).map_err(|error| engine_error(&source, error))?;
    let samples = load_samples(
        &score,
        source_path.parent().unwrap_or_else(|| Path::new("")),
    )?;
    let audio = render_score(&score, 0, &samples).map_err(|error| engine_error(&source, error))?;
    encode_wav(&audio).map_err(CliError::Export)
}

fn load_samples(score: &Score, base: &Path) -> Result<SampleLibrary, CliError> {
    let mut samples = SampleLibrary::default();
    let sources = score
        .sampled_sources()
        .map(Cow::Borrowed)
        .chain(score.packed_samples().map(|(container, selector)| {
            Cow::Owned(match selector {
                SampleSelector::Index(index) => packed_sample_source(container, *index),
                SampleSelector::Named(name) => named_sample_source(container, name),
            })
        }));
    for source in sources {
        if samples.get(&source).is_some() {
            continue;
        }
        let relative = Path::new(source.as_ref());
        if relative.is_absolute() {
            return Err(CliError::AbsoluteSamplePath(source.into_owned()));
        }
        let path = base.join(relative);
        let bytes = fs::read(&path).map_err(|error| CliError::SampleRead {
            path: path.display().to_string(),
            source: error,
        })?;
        let sample = decode_wav(&bytes).map_err(|error| CliError::SampleDecode {
            path: path.display().to_string(),
            source: error,
        })?;
        samples.insert(source.into_owned(), sample);
    }
    Ok(samples)
}

fn engine_error(source: &SourceText, error: EngineError) -> CliError {
    match error {
        EngineError::Syntax(diagnostics) => CliError::Diagnostics(
            diagnostics
                .iter()
                .map(|diagnostic| render_diagnostic(source, &diagnostic.message, diagnostic.span))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        EngineError::Compile(diagnostics) => CliError::Diagnostics(
            diagnostics
                .iter()
                .map(|diagnostic| render_diagnostic(source, &diagnostic.message, diagnostic.span))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        error => CliError::Engine(error),
    }
}

fn render_diagnostic(source: &SourceText, message: &str, span: SourceSpan) -> String {
    let group = Level::ERROR.primary_title(message).element(
        Snippet::source(&source.text)
            .line_start(1)
            .path(&source.name)
            .fold(true)
            .annotation(AnnotationKind::Primary.span(span.range()).label(message)),
    );
    Renderer::plain().render(&[group])
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
    #[error("sample path must be relative: `{0}`")]
    AbsoluteSamplePath(String),
    #[error("failed to read sample `{path}`: {source}")]
    SampleRead {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to decode sample `{path}`: {source}")]
    SampleDecode {
        path: String,
        #[source]
        source: DecodeError,
    },
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
    use std::fs;

    use super::{SourceId, SourceSpan, SourceText, render_diagnostic, source_to_wav};

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

        assert_eq!((&wav[0..4], &wav[8..12]), (&b"RIFF"[..], &b"WAVE"[..]));
    }

    #[test]
    fn render_diagnostic_should_highlight_unicode_source() {
        let source = SourceText::new(SourceId(0), "bad.sym", "aé\n@");

        let diagnostic = render_diagnostic(
            &source,
            "unexpected character",
            SourceSpan::new(SourceId(0), 4..5),
        );

        assert!(
            diagnostic.contains("bad.sym")
                && diagnostic.contains('@')
                && diagnostic.contains("unexpected character")
        );
    }

    #[test]
    fn source_to_wav_should_render_end_of_file_diagnostics() {
        let error = source_to_wav("bad.sym".to_owned(), "project {".to_owned())
            .expect_err("unclosed project should fail");

        assert!(error.to_string().contains("expected `}` to close project"));
    }

    #[test]
    fn source_to_wav_should_load_samples_relative_to_the_source() {
        let directory =
            std::env::temp_dir().join(format!("symphra-cli-sampled-test-{}", std::process::id()));
        fs::create_dir_all(&directory).expect("test directory should be created");
        fs::write(directory.join("piano.wav"), wav(&[16_384; 4_000], 8_000))
            .expect("test sample should be written");

        let result = source_to_wav(
            directory.join("song.sym").display().to_string(),
            r#"
project { seed 1 sample_rate 8khz output mono }
song "Sample" {
  tempo 120bpm meter 4/4 key C major
  instrument piano = sampled { source "piano.wav" root C4 }
  pattern phrase = sequence { note C4 for 1/4 }
  arrangement { phrase with piano }
}
"#
            .to_owned(),
        );

        fs::remove_file(directory.join("piano.wav")).expect("test sample should be removed");
        fs::remove_dir(directory).expect("test directory should be removed");
        let rendered = result.expect("relative sample should render");
        assert_eq!(
            (&rendered[0..4], &rendered[8..12]),
            (&b"RIFF"[..], &b"WAVE"[..])
        );
    }

    #[test]
    fn source_to_wav_should_load_numbered_pack_samples() {
        let directory =
            std::env::temp_dir().join(format!("symphra-cli-pack-test-{}", std::process::id()));
        let pack = directory.join("numbers");
        fs::create_dir_all(&pack).expect("sample pack directory should be created");
        for index in [1, 3] {
            fs::write(
                pack.join(format!("{index}.wav")),
                wav(&[16_384; 2_000], 8_000),
            )
            .expect("pack sample should be written");
        }

        let result = source_to_wav(
            directory.join("song.sym").display().to_string(),
            r#"
project { seed 1 sample_rate 8khz output mono }
song "Pack" {
  tempo 120bpm meter 4/4 key C major
  instrument voice = sampler { pack "numbers" }
  pattern phrase = steps 1/8 { sample 1 rest sample 3 }
  arrangement { phrase with voice }
}
"#
            .to_owned(),
        );

        for index in [1, 3] {
            fs::remove_file(pack.join(format!("{index}.wav")))
                .expect("pack sample should be removed");
        }
        fs::remove_dir(pack).expect("sample pack directory should be removed");
        fs::remove_dir(directory).expect("test directory should be removed");
        let rendered = result.expect("numbered pack samples should render");
        assert_eq!(
            (&rendered[0..4], &rendered[8..12]),
            (&b"RIFF"[..], &b"WAVE"[..])
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
