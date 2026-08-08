//! Language-independent, exactly timed musical events.

#[derive(Clone, Debug, PartialEq)]
pub struct Score {
    pub seed: u64,
    pub sample_rate_hz: u32,
    pub channels: Channels,
    pub songs: Vec<Song>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channels {
    Mono,
    Stereo,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    pub id: EntityId,
    pub name: String,
    pub tempo_bpm: f64,
    pub meter: Meter,
    pub key: Key,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EntityId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Meter {
    pub numerator: u32,
    pub denominator: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub tonic: PitchClass,
    pub mode: Mode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PitchClass {
    C,
    D,
    E,
    F,
    G,
    A,
    B,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Major,
    Minor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub id: EntityId,
    pub name: String,
    pub notes: Vec<NoteEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteEvent {
    pub id: EntityId,
    pub start: MusicalTime,
    pub duration: MusicalTime,
    pub midi_pitch: u8,
}

/// An exact fraction of a whole note.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MusicalTime {
    numerator: u64,
    denominator: u64,
}

impl MusicalTime {
    pub const ZERO: Self = Self {
        numerator: 0,
        denominator: 1,
    };

    /// Creates a reduced musical-time fraction.
    ///
    /// # Errors
    ///
    /// Returns [`TimeError::ZeroDenominator`] when `denominator` is zero.
    pub fn new(numerator: u64, denominator: u64) -> Result<Self, TimeError> {
        if denominator == 0 {
            return Err(TimeError::ZeroDenominator);
        }
        let divisor = gcd(numerator, denominator);
        Ok(Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }

    #[must_use]
    pub const fn numerator(self) -> u64 {
        self.numerator
    }

    #[must_use]
    pub const fn denominator(self) -> u64 {
        self.denominator
    }

    /// Adds two exact musical times.
    ///
    /// # Errors
    ///
    /// Returns [`TimeError::Overflow`] when the result exceeds `u64`.
    pub fn checked_add(self, other: Self) -> Result<Self, TimeError> {
        let common = gcd(self.denominator, other.denominator);
        let left = self
            .numerator
            .checked_mul(other.denominator / common)
            .ok_or(TimeError::Overflow)?;
        let right = other
            .numerator
            .checked_mul(self.denominator / common)
            .ok_or(TimeError::Overflow)?;
        let numerator = left.checked_add(right).ok_or(TimeError::Overflow)?;
        let denominator = (self.denominator / common)
            .checked_mul(other.denominator)
            .ok_or(TimeError::Overflow)?;
        Self::new(numerator, denominator)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TimeError {
    #[error("musical-time denominator must not be zero")]
    ZeroDenominator,
    #[error("musical time exceeds the supported range")]
    Overflow,
}

fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}

#[cfg(test)]
mod tests {
    use super::MusicalTime;

    #[test]
    fn checked_add_should_reduce_exact_result() {
        let quarter = MusicalTime::new(1, 4).expect("quarter note should be valid");

        let result = quarter
            .checked_add(quarter)
            .expect("two quarter notes should fit");

        assert_eq!(
            result,
            MusicalTime::new(1, 2).expect("half note should be valid")
        );
    }
}
