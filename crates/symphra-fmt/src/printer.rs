const INDENT_UNIT: &str = "  ";

/// A line-oriented output buffer with indentation tracking.
///
/// Every emitted construct is a whole line; callers never build partial
/// lines across calls except via [`Printer::append_to_last`], which is used
/// only to attach a same-line trailing comment.
#[derive(Debug, Default)]
pub struct Printer {
    indent: usize,
    lines: Vec<String>,
}

impl Printer {
    pub fn indent(&mut self) {
        self.indent += 1;
    }

    pub fn dedent(&mut self) {
        self.indent = self.indent.saturating_sub(1);
    }

    pub fn line(&mut self, text: impl AsRef<str>) {
        let text = text.as_ref();
        if text.is_empty() {
            self.lines.push(String::new());
        } else {
            self.lines
                .push(format!("{}{text}", INDENT_UNIT.repeat(self.indent)));
        }
    }

    /// Emits a blank separator line, unless the buffer is empty or already
    /// ends with one. This keeps blank-line handling idempotent: callers can
    /// request a blank line from several places without ever producing two
    /// in a row.
    pub fn blank(&mut self) {
        if self.lines.last().is_some_and(|line| !line.is_empty()) {
            self.lines.push(String::new());
        }
    }

    pub fn append_to_last(&mut self, suffix: &str) {
        if let Some(last) = self.lines.last_mut() {
            last.push_str(suffix);
        }
    }

    /// Returns the current line count, to be paired with a later call as a
    /// `start..mark()` range identifying everything printed in between.
    #[must_use]
    pub fn mark(&self) -> usize {
        self.lines.len()
    }

    /// Reorders the line ranges printed since `start` (which together must
    /// exactly cover `start..self.mark()`) into `rank` order, stably
    /// preserving each range's internal lines and its position relative to
    /// same-rank ranges.
    ///
    /// This reorders already-printed text rather than reordering AST nodes
    /// before printing them, because printing (and the comment attachment
    /// alongside it) must happen in strict source order; see
    /// [`crate::trivia::CommentCursor`].
    ///
    /// When `force_separator` is set, a blank line is inserted at every
    /// point where the rank changes. Callers use this for a coarse
    /// grouping (for example, `project` versus `song` at the top level)
    /// where a separator is always wanted; it is off for the finer-grained
    /// statement lists inside a `project`/`song` body, where forcing a
    /// blank between every differently named setting would fight the
    /// "preserve the author's own blank lines" rule instead of serving it.
    pub fn reorder_since(
        &mut self,
        start: usize,
        mut ranges: Vec<(u8, std::ops::Range<usize>)>,
        force_separator: bool,
    ) {
        ranges.sort_by_key(|(rank, _)| *rank);
        let mut reordered = Vec::with_capacity(self.lines.len().saturating_sub(start));
        let mut prev_rank = None;
        for (rank, range) in ranges {
            let needs_separator =
                force_separator && prev_rank.is_some_and(|previous| previous != rank);
            let already_blank = reordered.last().is_some_and(String::is_empty)
                || self.lines[range.clone()]
                    .first()
                    .is_some_and(String::is_empty);
            if needs_separator && !already_blank {
                reordered.push(String::new());
            }
            reordered.extend_from_slice(&self.lines[range]);
            prev_rank = Some(rank);
        }
        self.lines.splice(start.., reordered);
    }

    #[must_use]
    pub fn finish(mut self) -> String {
        while self.lines.last().is_some_and(String::is_empty) {
            self.lines.pop();
        }
        let mut output = self.lines.join("\n");
        output.push('\n');
        output
    }
}
