//! Expansion of `* N` repetition sugar.
//!
//! `symphra-syntax` keeps `item * 4` and `(a, b) * 4` in the AST as
//! [`RepeatGroup`]s so the formatter can reprint what the author wrote. Every
//! consumer of pattern and rhythm bodies wants the expanded run instead, so
//! this module flattens a body once, up front, and hands back borrowed items
//! in playing order.
//!
//! The expansion is bounded: a body that would produce more than
//! [`MAX_EXPANDED_ITEMS`] items is rejected rather than materialized, so a
//! typo like `* 4000000` fails with a diagnostic instead of exhausting
//! memory.
//!
//! Each expanded item carries the [`Repetition`] that produced it, because
//! a `velocity A..B` ramp needs to know which copy it is and how many there
//! are in total.

use symphra_syntax::ast::{RepeatGroup, RhythmItem, SequenceItem, StepItem, VelocityExpression};

/// The most items one pattern or rhythm body may expand to.
///
/// 4096 sixteenth notes is 256 bars of 4/4 — far past any single body in
/// practice, and small enough that hitting the cap is always a mistake.
pub const MAX_EXPANDED_ITEMS: usize = 4096;

/// Where one expanded copy sits inside the innermost repetition that
/// produced it. An item written once is [`Repetition::ONCE`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Repetition {
    pub index: u32,
    pub count: u32,
}

impl Repetition {
    /// An item that no repetition encloses.
    pub const ONCE: Self = Self { index: 0, count: 1 };

    /// Resolves a `velocity` at this position: the plain value as written,
    /// or a ramp interpolated linearly from its start to its end across the
    /// repetition's copies.
    ///
    /// Rounds halves away from zero, so a ramp is symmetric whether it
    /// climbs or falls.
    #[must_use]
    pub fn velocity(self, velocity: &VelocityExpression) -> u32 {
        let Some(end) = velocity.ramp_to else {
            return velocity.value;
        };
        if self.count <= 1 {
            return velocity.value;
        }
        let delta = i64::from(end) - i64::from(velocity.value);
        let steps = i64::from(self.count) - 1;
        let scaled = delta * i64::from(self.index);
        let offset = if scaled >= 0 {
            (2 * scaled + steps) / (2 * steps)
        } else {
            -((-2 * scaled + steps) / (2 * steps))
        };
        let value = i64::from(velocity.value) + offset;
        u32::try_from(value).unwrap_or(0)
    }
}

/// One expanded item, and the repetition position it was produced at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Expanded<'a, T> {
    pub item: &'a T,
    pub position: Repetition,
}

/// Expands the repetitions in a `steps` body, or returns `None` if the body
/// exceeds [`MAX_EXPANDED_ITEMS`].
#[must_use]
pub fn step_items(items: &[StepItem]) -> Option<Vec<Expanded<'_, StepItem>>> {
    expand(items, |item| match item {
        StepItem::Repeat(group) => Some(group),
        _ => None,
    })
}

/// Expands the repetitions in a `sequence` body, or returns `None` if the
/// body exceeds [`MAX_EXPANDED_ITEMS`].
#[must_use]
pub fn sequence_items(items: &[SequenceItem]) -> Option<Vec<Expanded<'_, SequenceItem>>> {
    expand(items, |item| match item {
        SequenceItem::Repeat(group) => Some(group),
        _ => None,
    })
}

/// Expands the repetitions in a `rhythm` body, or returns `None` if the body
/// exceeds [`MAX_EXPANDED_ITEMS`].
#[must_use]
pub fn rhythm_items(items: &[RhythmItem]) -> Option<Vec<Expanded<'_, RhythmItem>>> {
    expand(items, |item| match item {
        RhythmItem::Repeat(group) => Some(group),
        _ => None,
    })
}

fn expand<T>(
    items: &[T],
    repeat: fn(&T) -> Option<&RepeatGroup<T>>,
) -> Option<Vec<Expanded<'_, T>>> {
    let mut expanded = Vec::with_capacity(items.len());
    push_all(items, repeat, Repetition::ONCE, &mut expanded).ok()?;
    Some(expanded)
}

fn push_all<'a, T>(
    items: &'a [T],
    repeat: fn(&T) -> Option<&RepeatGroup<T>>,
    position: Repetition,
    expanded: &mut Vec<Expanded<'a, T>>,
) -> Result<(), TooLarge> {
    for item in items {
        if let Some(group) = repeat(item) {
            for index in 0..group.count {
                let position = Repetition {
                    index,
                    count: group.count,
                };
                push_all(&group.items, repeat, position, expanded)?;
            }
        } else {
            if expanded.len() >= MAX_EXPANDED_ITEMS {
                return Err(TooLarge);
            }
            expanded.push(Expanded { item, position });
        }
    }
    Ok(())
}

struct TooLarge;

#[cfg(test)]
mod tests {
    use symphra_syntax::{SourceId, ast::Declaration, ast::PatternBody, ast::SongStatement, parse};

    use super::{MAX_EXPANDED_ITEMS, step_items};

    fn steps_of(body: &str) -> Vec<String> {
        let source = format!("song \"t\" {{ pattern p = steps 1/8 {{ {body} }} }}");
        let parsed = parse(SourceId(0), &source);
        assert_eq!(parsed.diagnostics, Vec::new());
        let Declaration::Song(song) = &parsed.file.declarations[0] else {
            panic!("expected a song");
        };
        let SongStatement::Pattern(pattern) = &song.statements[0] else {
            panic!("expected a pattern");
        };
        let PatternBody::Steps { items, .. } = &pattern.body else {
            panic!("expected steps");
        };
        step_items(items)
            .expect("body is small")
            .iter()
            .map(|expanded| format!("{:?}", expanded.item))
            .collect()
    }

    #[test]
    fn expands_a_repeated_item_in_place() {
        let expanded = steps_of("rest drum \"hh\" * 3 rest");
        assert_eq!(expanded.len(), 5);
        assert!(expanded[0].starts_with("Rest"));
        assert!(expanded[1].starts_with("Drum"));
        assert!(expanded[3].starts_with("Drum"));
        assert!(expanded[4].starts_with("Rest"));
    }

    #[test]
    fn expands_a_group_as_a_unit() {
        let expanded = steps_of("(drum \"hh\", rest) * 2");
        assert_eq!(expanded.len(), 4);
        assert!(expanded[0].starts_with("Drum"));
        assert!(expanded[1].starts_with("Rest"));
        assert!(expanded[2].starts_with("Drum"));
        assert!(expanded[3].starts_with("Rest"));
    }

    #[test]
    fn expands_nested_repetitions_multiplicatively() {
        assert_eq!(steps_of("(rest * 2, drum \"hh\") * 3").len(), 9);
    }

    #[test]
    fn rejects_a_body_larger_than_the_cap() {
        let source = format!(
            "song \"t\" {{ pattern p = steps 1/8 {{ rest * {} }} }}",
            MAX_EXPANDED_ITEMS + 1
        );
        let parsed = parse(SourceId(0), &source);
        let Declaration::Song(song) = &parsed.file.declarations[0] else {
            panic!("expected a song");
        };
        let SongStatement::Pattern(pattern) = &song.statements[0] else {
            panic!("expected a pattern");
        };
        let PatternBody::Steps { items, .. } = &pattern.body else {
            panic!("expected steps");
        };
        assert!(step_items(items).is_none());
    }
}
