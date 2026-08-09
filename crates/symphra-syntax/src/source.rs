use crate::{SourceId, SourceSpan};

/// A zero-based location using UTF-16 code units for the column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourcePosition {
    pub line: u32,
    pub utf16_column: u32,
}

/// A half-open source range expressed as UTF-16 positions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceRange {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceText {
    pub id: SourceId,
    pub name: String,
    pub text: String,
}

impl SourceText {
    #[must_use]
    pub fn new(id: SourceId, name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            text: text.into(),
        }
    }

    /// Converts a UTF-8 byte offset to a zero-based UTF-16 position.
    #[must_use]
    pub fn utf16_position(&self, byte_offset: u32) -> Option<SourcePosition> {
        let byte_offset = byte_offset as usize;
        if !self.text.is_char_boundary(byte_offset) {
            return None;
        }

        let prefix = self.text.get(..byte_offset)?;
        let line_start = prefix.rfind('\n').map_or(0, |offset| offset + 1);
        Some(SourcePosition {
            line: u32::try_from(prefix.bytes().filter(|byte| *byte == b'\n').count()).ok()?,
            utf16_column: u32::try_from(self.text[line_start..byte_offset].encode_utf16().count())
                .ok()?,
        })
    }

    /// Converts a zero-based UTF-16 position to a UTF-8 byte offset.
    #[must_use]
    pub fn byte_offset_utf16(&self, position: SourcePosition) -> Option<u32> {
        let line = usize::try_from(position.line).ok()?;
        let line_start = if line == 0 {
            0
        } else {
            self.text
                .match_indices('\n')
                .nth(line - 1)
                .map(|(offset, _)| offset + 1)?
        };
        let line_text = self.text[line_start..]
            .split_once('\n')
            .map_or(&self.text[line_start..], |(line, _)| line);
        let target_column = usize::try_from(position.utf16_column).ok()?;
        let mut utf16_column = 0;

        for (byte_offset, ch) in line_text.char_indices() {
            if utf16_column == target_column {
                return u32::try_from(line_start + byte_offset).ok();
            }
            utf16_column += ch.len_utf16();
            if utf16_column > target_column {
                return None;
            }
        }

        (utf16_column == target_column).then(|| u32::try_from(line_start + line_text.len()).ok())?
    }

    /// Converts a byte-based span for this source to a UTF-16 range.
    #[must_use]
    pub fn utf16_range(&self, span: SourceSpan) -> Option<SourceRange> {
        if span.source != self.id || span.start > span.end {
            return None;
        }

        Some(SourceRange {
            start: self.utf16_position(span.start)?,
            end: self.utf16_position(span.end)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{SourceId, SourcePosition, SourceRange, SourceSpan, SourceText};

    #[test]
    fn converts_utf8_offsets_to_utf16_positions() {
        let source = SourceText::new(SourceId(7), "unicode.sym", "a😀\nβ");

        assert_eq!(
            source.utf16_position(5),
            Some(SourcePosition {
                line: 0,
                utf16_column: 3,
            })
        );
        assert_eq!(
            source.utf16_position(6),
            Some(SourcePosition {
                line: 1,
                utf16_column: 0,
            })
        );
        assert_eq!(source.utf16_position(2), None);
        assert_eq!(source.utf16_position(20), None);
        assert_eq!(
            source.byte_offset_utf16(SourcePosition {
                line: 0,
                utf16_column: 3,
            }),
            Some(5)
        );
        assert_eq!(
            source.byte_offset_utf16(SourcePosition {
                line: 1,
                utf16_column: 1,
            }),
            Some(8)
        );
        assert_eq!(
            source.byte_offset_utf16(SourcePosition {
                line: 0,
                utf16_column: 2,
            }),
            None
        );
    }

    #[test]
    fn converts_only_spans_from_the_same_source() {
        let source = SourceText::new(SourceId(7), "unicode.sym", "a😀\nβ");

        assert_eq!(
            source.utf16_range(SourceSpan::new(SourceId(7), 1..5)),
            Some(SourceRange {
                start: SourcePosition {
                    line: 0,
                    utf16_column: 1,
                },
                end: SourcePosition {
                    line: 0,
                    utf16_column: 3,
                },
            })
        );
        assert_eq!(source.utf16_range(SourceSpan::new(SourceId(8), 1..5)), None);
    }
}
