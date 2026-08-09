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
            .map(|pattern| schedule_track(pattern, MusicalTime::ZERO, None).map(|(track, _)| track))
            .collect();
    };
    if arrangement.occurrences.is_empty() {
        return Err(ScheduleError::EmptyArrangement);
    }

    let mut cursor = MusicalTime::ZERO;
    let mut tracks = Vec::with_capacity(arrangement.occurrences.len());
    for (index, occurrence) in arrangement.occurrences.iter().enumerate() {
        if arrangement.occurrences[..index]
            .iter()
            .any(|previous| previous.id == occurrence.id)
        {
            return Err(ScheduleError::DuplicateOccurrence(occurrence.id));
        }
        let pattern = song
            .patterns
            .iter()
            .find(|pattern| pattern.id == occurrence.pattern)
            .ok_or(ScheduleError::UnknownPattern(occurrence.pattern))?;
        let (track, end) = schedule_track(pattern, cursor, Some(occurrence.id))?;
        tracks.push(track);
        cursor = end;
    }
    Ok(tracks)
}

fn schedule_track(
    pattern: &hir::Pattern,
    mut cursor: MusicalTime,
    occurrence: Option<hir::NodeId>,
) -> Result<(Track, MusicalTime), ScheduleError> {
    let mut notes = Vec::new();
    for step in &pattern.steps {
        let written_duration = match step {
            hir::PatternStep::Note(note) => note.duration,
            hir::PatternStep::Chord(chord) => chord.duration,
            hir::PatternStep::Rest(rest) => rest.duration,
        };
        let duration = MusicalTime::new(
            u64::from(written_duration.numerator),
            u64::from(written_duration.denominator),
        )?;
        match step {
            hir::PatternStep::Note(note) => notes.push(note_event(
                note.id,
                note.midi_pitch,
                note.velocity,
                duration,
                cursor,
                occurrence,
            )),
            hir::PatternStep::Chord(chord) => {
                notes.extend(chord.notes.iter().map(|note| {
                    note_event(
                        note.id,
                        note.midi_pitch,
                        chord.velocity,
                        duration,
                        cursor,
                        occurrence,
                    )
                }));
            }
            hir::PatternStep::Rest(_) => {}
        }
        cursor = cursor.checked_add(duration)?;
    }
    Ok((
        Track {
            id: id(occurrence.unwrap_or(pattern.id)),
            name: pattern.name.clone(),
            notes,
            end: cursor,
        },
        cursor,
    ))
}

fn note_event(
    note: hir::NodeId,
    midi_pitch: u8,
    velocity: u8,
    duration: MusicalTime,
    start: MusicalTime,
    occurrence: Option<hir::NodeId>,
) -> NoteEvent {
    NoteEvent {
        id: occurrence.map_or_else(
            || id(note),
            |occurrence| occurrence_note_id(occurrence, note),
        ),
        start,
        duration,
        midi_pitch,
        velocity,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleError {
    #[error("arrangement must contain at least one pattern")]
    EmptyArrangement,
    #[error("arrangement occurrence ID {0:?} is used more than once")]
    DuplicateOccurrence(hir::NodeId),
    #[error("arrangement references unknown pattern ID {0:?}")]
    UnknownPattern(hir::NodeId),
    #[error(transparent)]
    Time(#[from] TimeError),
}

fn id(id: hir::NodeId) -> EntityId {
    EntityId(u64::from(id.0))
}

fn occurrence_note_id(occurrence: hir::NodeId, note: hir::NodeId) -> EntityId {
    EntityId((u64::from(occurrence.0) << 32) | u64::from(note.0))
}
