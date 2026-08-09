use symphra_syntax::Comment;

/// One comment attached to the node that follows it, plus whether the
/// author left a blank line above it.
#[derive(Debug, Clone)]
pub struct AttachedComment {
    pub text: String,
    pub blank_before: bool,
}

/// The comments (if any) immediately preceding a node, and whether the
/// author left a blank line between the last of them (or whatever preceded
/// the node) and the node itself.
#[derive(Debug, Clone, Default)]
pub struct LeadingTrivia {
    pub comments: Vec<AttachedComment>,
    pub blank_before_node: bool,
}

/// Walks the comment list left to right and hands out slices of it as the
/// caller advances through the source, one node at a time, in the same
/// depth-first, left-to-right order the parser produced those nodes in.
///
/// That last part matters: this cursor is a single, monotonically advancing
/// stream shared across the whole file. A caller must fully consume (via
/// [`Self::take_leading`] up to `close_end`) whatever comments fall inside a
/// composite node's own span, as part of printing that node, before its
/// sibling asks the cursor about the gap that follows it. Computing an
/// entire sibling list's trivia in one pass ahead of recursing into any of
/// them is unsound: a comment that is textually inside an earlier sibling's
/// span (say, dangling just before that sibling's own closing brace) would
/// still be sitting unconsumed at the cursor when the gap after that
/// sibling is examined, and byte-range comparisons against it would
/// underflow.
///
/// Byte-offset bounds passed to this cursor do not need to be exact brace or
/// token positions: every construct this formatter prints is written out
/// literally rather than sliced from source, so a loose bound (an
/// enclosing node's own span) only changes which nearby node an unusual
/// comment placement is displaced to, never causes it to be duplicated or
/// dropped.
pub struct CommentCursor<'a> {
    source: &'a str,
    comments: &'a [Comment],
    index: usize,
}

impl<'a> CommentCursor<'a> {
    #[must_use]
    pub fn new(source: &'a str, comments: &'a [Comment]) -> Self {
        Self {
            source,
            comments,
            index: 0,
        }
    }

    /// Consumes every remaining comment that starts before `upto`, computing
    /// each one's blank-line-before flag against `prev_end` as it goes.
    pub fn take_leading(&mut self, upto: u32, mut prev_end: u32) -> LeadingTrivia {
        let mut comments = Vec::new();
        while self.index < self.comments.len() && self.comments[self.index].span.start < upto {
            let comment = &self.comments[self.index];
            comments.push(AttachedComment {
                text: comment.text.trim_end().to_owned(),
                blank_before: self.has_blank_gap(prev_end, comment.span.start),
            });
            prev_end = comment.span.end;
            self.index += 1;
        }
        let blank_before_node = self.has_blank_gap(prev_end, upto);
        LeadingTrivia {
            comments,
            blank_before_node,
        }
    }

    /// If the very next unconsumed comment starts before `limit` with only
    /// whitespace (no newline) between `end` and it, consumes and returns it
    /// as a same-line trailing comment. Otherwise leaves the cursor alone.
    pub fn take_trailing_same_line(&mut self, end: u32, limit: u32) -> Option<(String, u32)> {
        let comment = self.comments.get(self.index)?;
        if comment.span.start < end || comment.span.start >= limit {
            return None;
        }
        let gap = self.gap(end, comment.span.start);
        if gap.contains('\n') {
            return None;
        }
        self.index += 1;
        Some((comment.text.trim_end().to_owned(), comment.span.end))
    }

    /// Slices the source between two offsets, defensively returning an
    /// empty string for an inverted range instead of panicking.
    ///
    /// An inverted range can only reach here for a comment that sits
    /// textually inside a leaf node's own tokens (for example, between
    /// `velocity` and its number) — the rare case this formatter does not
    /// finely track. Falling back to an empty gap just means that comment
    /// is treated as adjacent (no blank line) rather than crashing the
    /// formatter over a placement decision that was already a best-effort
    /// approximation.
    fn gap(&self, start: u32, end: u32) -> &'a str {
        self.source
            .get(start as usize..end as usize)
            .unwrap_or_default()
    }

    fn has_blank_gap(&self, start: u32, end: u32) -> bool {
        self.gap(start, end).matches('\n').count() >= 2
    }
}
