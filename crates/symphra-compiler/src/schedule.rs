use symphra_score::{
    Channels, EntityId, Key, Meter, Mode, MusicalTime, NoteEvent, PitchClass, Score, Song,
    TimeError, Track,
};

use crate::hir;

/// Converts validated HIR into exact, sequential score events.
///
/// # Errors
///
/// Returns [`ScheduleError`] if the HIR contains an invalid pattern reference or
/// accumulated musical time exceeds its supported range.
pub fn schedule(program: &hir::Program) -> Result<Score, ScheduleError> {
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

fn schedule_song(song: &hir::Song) -> Result<Song, ScheduleError> {
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
        tracks: schedule_tracks(song)?,
    })
}

fn schedule_tracks(song: &hir::Song) -> Result<Vec<Track>, ScheduleError> {
    let Some(arrangement) = &song.arrangement else {
        return song
            .patterns
            .iter()
            .map(|pattern| schedule_track(pattern, MusicalTime::ZERO).map(|(track, _)| track))
            .collect();
    };
    if arrangement.patterns.is_empty() {
        return Err(ScheduleError::EmptyArrangement);
    }

    let mut cursor = MusicalTime::ZERO;
    let mut tracks = Vec::with_capacity(arrangement.patterns.len());
    for (index, pattern_id) in arrangement.patterns.iter().enumerate() {
        if arrangement.patterns[..index].contains(pattern_id) {
            return Err(ScheduleError::DuplicatePattern(*pattern_id));
        }
        let pattern = song
            .patterns
            .iter()
            .find(|pattern| pattern.id == *pattern_id)
            .ok_or(ScheduleError::UnknownPattern(*pattern_id))?;
        let (track, end) = schedule_track(pattern, cursor)?;
        tracks.push(track);
        cursor = end;
    }
    Ok(tracks)
}

fn schedule_track(
    pattern: &hir::Pattern,
    mut cursor: MusicalTime,
) -> Result<(Track, MusicalTime), ScheduleError> {
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
    Ok((
        Track {
            id: id(pattern.id),
            name: pattern.name.clone(),
            notes,
        },
        cursor,
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleError {
    #[error("arrangement must contain at least one pattern")]
    EmptyArrangement,
    #[error("pattern ID {0:?} is arranged more than once")]
    DuplicatePattern(hir::NodeId),
    #[error("arrangement references unknown pattern ID {0:?}")]
    UnknownPattern(hir::NodeId),
    #[error(transparent)]
    Time(#[from] TimeError),
}

fn id(id: hir::NodeId) -> EntityId {
    EntityId(u64::from(id.0))
}
