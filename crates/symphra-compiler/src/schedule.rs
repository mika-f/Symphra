use symphra_score::{
    Channels, EntityId, Key, Meter, Mode, MusicalTime, NoteEvent, PitchClass, Score, Song,
    TimeError, Track,
};

use crate::hir;

/// Converts validated HIR into exact, sequential score events.
///
/// # Errors
///
/// Returns [`TimeError`] if accumulated musical time exceeds its supported range.
pub fn schedule(program: &hir::Program) -> Result<Score, TimeError> {
    Ok(Score {
        seed: program.project.seed,
        sample_rate_hz: program.project.sample_rate_hz,
        channels: match program.project.channels {
            hir::Channels::Mono => Channels::Mono,
            hir::Channels::Stereo => Channels::Stereo,
        },
        songs: program
            .songs
            .iter()
            .map(schedule_song)
            .collect::<Result<_, _>>()?,
    })
}

fn schedule_song(song: &hir::Song) -> Result<Song, TimeError> {
    Ok(Song {
        id: id(song.id),
        name: song.name.clone(),
        tempo_bpm: song.tempo_bpm,
        meter: Meter {
            numerator: song.meter.numerator,
            denominator: song.meter.denominator,
        },
        key: Key {
            tonic: match song.key.tonic {
                hir::PitchClass::C => PitchClass::C,
                hir::PitchClass::D => PitchClass::D,
                hir::PitchClass::E => PitchClass::E,
                hir::PitchClass::F => PitchClass::F,
                hir::PitchClass::G => PitchClass::G,
                hir::PitchClass::A => PitchClass::A,
                hir::PitchClass::B => PitchClass::B,
            },
            mode: match song.key.mode {
                hir::Mode::Major => Mode::Major,
                hir::Mode::Minor => Mode::Minor,
            },
        },
        tracks: song
            .patterns
            .iter()
            .map(schedule_track)
            .collect::<Result<_, _>>()?,
    })
}

fn schedule_track(pattern: &hir::Pattern) -> Result<Track, TimeError> {
    let mut cursor = MusicalTime::ZERO;
    let mut notes = Vec::with_capacity(pattern.notes.len());
    for note in &pattern.notes {
        let duration = MusicalTime::new(
            u64::from(note.duration.numerator),
            u64::from(note.duration.denominator),
        )?;
        notes.push(NoteEvent {
            id: id(note.id),
            start: cursor,
            duration,
            midi_pitch: note.midi_pitch,
        });
        cursor = cursor.checked_add(duration)?;
    }
    Ok(Track {
        id: id(pattern.id),
        name: pattern.name.clone(),
        notes,
    })
}

fn id(id: hir::NodeId) -> EntityId {
    EntityId(u64::from(id.0))
}
