use std::ops::Range;

/// Identifies one source buffer within a compilation session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceId(pub u32);

/// A half-open UTF-8 byte range in a source buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    pub source: SourceId,
    pub start: u32,
    pub end: u32,
}

impl SourceSpan {
    /// Creates a source span from a half-open UTF-8 byte range.
    ///
    /// # Panics
    ///
    /// Panics if either range boundary exceeds the 32-bit source offset limit.
    #[must_use]
    pub fn new(source: SourceId, range: Range<usize>) -> Self {
        Self {
            source,
            start: u32::try_from(range.start).expect("source exceeds 4 GiB"),
            end: u32::try_from(range.end).expect("source exceeds 4 GiB"),
        }
    }

    #[must_use]
    pub const fn empty(source: SourceId, offset: u32) -> Self {
        Self {
            source,
            start: offset,
            end: offset,
        }
    }

    #[must_use]
    pub const fn cover(self, other: Self) -> Self {
        debug_assert!(self.source.0 == other.source.0);
        Self {
            source: self.source,
            start: if self.start < other.start {
                self.start
            } else {
                other.start
            },
            end: if self.end > other.end {
                self.end
            } else {
                other.end
            },
        }
    }

    #[must_use]
    pub fn range(self) -> Range<usize> {
        self.start as usize..self.end as usize
    }
}
