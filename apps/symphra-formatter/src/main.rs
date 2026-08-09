use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
use symphra_fmt::FormatError;
use symphra_syntax::{SourceId, SourceText};

fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    if args.as_slice() == [OsString::from("-")] {
        return run_stdin();
    }

    match run(args) {
        Ok(Summary {
            reformatted,
            unformatted,
        }) => {
            for path in &reformatted {
                println!("formatted {}", path.display());
            }
            if unformatted.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Formats stdin and writes the result to stdout, for editor integrations
/// that want to format an in-memory buffer without touching disk. Prints
/// diagnostics to stderr and writes nothing to stdout on invalid source, so
/// a caller piping stdout back into a buffer never applies a partial or
/// garbled result.
fn run_stdin() -> ExitCode {
    let mut text = String::new();
    if let Err(error) = io::stdin().read_to_string(&mut text) {
        eprintln!("failed to read stdin: {error}");
        return ExitCode::FAILURE;
    }

    let source = SourceText::new(SourceId(0), "<stdin>", text.clone());
    let formatted = match symphra_fmt::format_source(&text) {
        Ok(formatted) => formatted,
        Err(error) => {
            print_diagnostics(&source, &error);
            return ExitCode::FAILURE;
        }
    };

    let mut stdout = io::stdout();
    if stdout.write_all(formatted.as_bytes()).is_err() {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[derive(Debug, Default)]
struct Summary {
    /// Files rewritten in place (empty in `--check` mode).
    reformatted: Vec<PathBuf>,
    /// Files that are not already in canonical form, or that failed to
    /// parse. Any entry here is reported and makes the run exit non-zero.
    unformatted: Vec<PathBuf>,
}

fn run(args: impl IntoIterator<Item = OsString>) -> Result<Summary, CliError> {
    let mut check = false;
    let mut paths = Vec::new();
    for arg in args {
        if arg == "--check" {
            check = true;
        } else {
            paths.push(PathBuf::from(arg));
        }
    }
    if paths.is_empty() {
        return Err(CliError::Usage);
    }

    let mut summary = Summary::default();
    for path in paths {
        let text = fs::read_to_string(&path).map_err(|source| CliError::Read {
            path: path.display().to_string(),
            source,
        })?;

        let source = SourceText::new(SourceId(0), path.display().to_string(), text.clone());
        let formatted = symphra_fmt::format_source(&text).map_err(|error| {
            print_diagnostics(&source, &error);
            summary.unformatted.push(path.clone());
        });
        let Ok(formatted) = formatted else {
            continue;
        };
        if formatted == text {
            continue;
        }
        if check {
            println!("would reformat {}", path.display());
            summary.unformatted.push(path);
            continue;
        }
        fs::write(&path, formatted).map_err(|source| CliError::Write {
            path: path.display().to_string(),
            source,
        })?;
        summary.reformatted.push(path);
    }
    Ok(summary)
}

fn print_diagnostics(source: &SourceText, error: &FormatError) {
    for diagnostic in error.diagnostics() {
        let group = Level::ERROR.primary_title(&diagnostic.message).element(
            Snippet::source(&source.text)
                .line_start(1)
                .path(&source.name)
                .fold(true)
                .annotation(
                    AnnotationKind::Primary
                        .span(diagnostic.span.range())
                        .label(&diagnostic.message),
                ),
        );
        eprintln!("{}", Renderer::plain().render(&[group]));
    }
}

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("usage: symphra-formatter [--check] <file.sym>...")]
    Usage,
    #[error("failed to read `{path}`: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write `{path}`: {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::run;

    fn write_temp(name: &str, contents: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "symphra-formatter-cli-test-{}-{name}",
            std::process::id()
        ));
        fs::write(&path, contents).expect("temp fixture should be written");
        path
    }

    #[test]
    fn rewrites_a_compact_file_in_place() {
        let path = write_temp(
            "rewrite.sym",
            "project { seed 1 sample_rate 8khz output mono }\n",
        );

        let summary = run([path.clone().into_os_string()]).expect("valid source should format");

        assert_eq!(summary.reformatted.as_slice(), std::slice::from_ref(&path));
        assert!(summary.unformatted.is_empty());
        let rewritten = fs::read_to_string(&path).expect("file should be rewritten");
        assert!(rewritten.contains("project {\n  seed 1\n"));
        fs::remove_file(path).expect("temp fixture should be removed");
    }

    #[test]
    fn leaves_an_already_formatted_file_untouched() {
        let path = write_temp(
            "already.sym",
            "project {\n  seed 1\n  sample_rate 8khz\n  output mono\n}\n",
        );
        let original_contents = fs::read_to_string(&path).unwrap();

        let summary = run([path.clone().into_os_string()]).expect("valid source should format");

        assert!(summary.reformatted.is_empty());
        assert!(summary.unformatted.is_empty());
        assert_eq!(fs::read_to_string(&path).unwrap(), original_contents);
        fs::remove_file(path).expect("temp fixture should be removed");
    }

    #[test]
    fn check_mode_reports_without_writing() {
        let path = write_temp(
            "check.sym",
            "project { seed 1 sample_rate 8khz output mono }\n",
        );
        let original_contents = fs::read_to_string(&path).unwrap();

        let summary = run(["--check".into(), path.clone().into_os_string()])
            .expect("valid source should format");

        assert!(summary.reformatted.is_empty());
        assert_eq!(summary.unformatted.as_slice(), std::slice::from_ref(&path));
        assert_eq!(fs::read_to_string(&path).unwrap(), original_contents);
        fs::remove_file(path).expect("temp fixture should be removed");
    }

    #[test]
    fn reports_diagnostics_for_invalid_source_without_writing() {
        let path = write_temp("invalid.sym", "project {");
        let original_contents = fs::read_to_string(&path).unwrap();

        let summary = run([path.clone().into_os_string()]).expect("run should not fail outright");

        assert_eq!(summary.unformatted.as_slice(), std::slice::from_ref(&path));
        assert_eq!(fs::read_to_string(&path).unwrap(), original_contents);
        fs::remove_file(path).expect("temp fixture should be removed");
    }

    #[test]
    fn requires_at_least_one_path() {
        let error = run([]).expect_err("no arguments should fail");
        assert!(error.to_string().contains("usage"));
    }
}
