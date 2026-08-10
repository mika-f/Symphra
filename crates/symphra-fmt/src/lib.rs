//! An opinionated formatter for Symphra source.
//!
//! [`format_source`] reprints a source file into one canonical layout:
//! two-space indentation, one item per line inside every block body (except
//! `rhythm` bodies, which keep `hit`/`rest` on a single compact line),
//! `project`/`song` settings reordered into a fixed canonical order, a blank
//! line before each song-level declaration (`instrument`, `pattern`,
//! `track`, …), and at most one blank line preserved anywhere the author
//! left one. It refuses to format source with lexical or syntax diagnostics,
//! matching `symphra-syntax`'s own guidance not to trust an AST produced
//! alongside diagnostics.
//!
//! Every identifier, number, and string literal is copied verbatim from the
//! original source by byte span rather than reconstructed from its parsed
//! value, so formatting never risks silently changing a literal's meaning
//! (for example by round-tripping a decimal through `f64`). Comments, which
//! `symphra-syntax`'s lexer otherwise discards as insignificant whitespace,
//! are captured separately and reattached to the nearest following item.

mod format;
mod printer;
mod trivia;

use symphra_syntax::{Diagnostic, SourceId, lex, parse};

use crate::format::{Ctx, format_source_file};
use crate::printer::Printer;
use crate::trivia::CommentCursor;

/// Source could not be formatted because lexing or parsing reported
/// diagnostics. Formatting an AST produced alongside diagnostics is not
/// safe: parse recovery may have skipped or misparsed tokens, so reprinting
/// it could silently change the source's meaning.
#[derive(Debug, thiserror::Error)]
#[error("source has {} diagnostic(s) and cannot be formatted", .0.len())]
pub struct FormatError(pub Vec<Diagnostic>);

impl FormatError {
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.0
    }
}

/// Formats one Symphra source file, returning the canonical text.
///
/// # Errors
///
/// Returns [`FormatError`] if `source` has any lexical or syntax
/// diagnostics.
pub fn format_source(source: &str) -> Result<String, FormatError> {
    let source_id = SourceId(0);
    let lexed = lex(source_id, source);
    if !lexed.diagnostics.is_empty() {
        return Err(FormatError(lexed.diagnostics));
    }
    let parsed = parse(source_id, source);
    if !parsed.diagnostics.is_empty() {
        return Err(FormatError(parsed.diagnostics));
    }

    let mut ctx = Ctx {
        source,
        cursor: CommentCursor::new(source, &lexed.comments),
        printer: Printer::default(),
    };
    format_source_file(&mut ctx, &parsed.file);
    Ok(ctx.printer.finish())
}

/// Formats `source` and reports whether the result differs from it.
///
/// Intended for `--check`-style tooling: `Ok(true)` means `source` is
/// already in canonical form.
///
/// # Errors
///
/// Returns [`FormatError`] under the same conditions as [`format_source`].
pub fn is_formatted(source: &str) -> Result<bool, FormatError> {
    Ok(format_source(source)? == source)
}
