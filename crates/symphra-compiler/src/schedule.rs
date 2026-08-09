use symphra_score::{
    Channels, EntityId, InstrumentKind, Key, Meter, Mode, MusicalTime, NoteEvent, PitchClass,
    SampleEvent, Score, Song, TimeError, Track,
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
            .map(|song| schedule_song(song, program.project.seed))
            .collect::<Result<_, _>>()?,
    })
}

fn schedule_song(song: &hir::Song, seed: u64) -> Result<Song, ScheduleError> {
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
        tracks: schedule_tracks(song, seed)?,
    })
}

fn schedule_tracks(song: &hir::Song, seed: u64) -> Result<Vec<Track>, ScheduleError> {
    let Some(arrangement) = &song.arrangement else {
        return song
            .patterns
            .iter()
            .map(|pattern| {
                schedule_track(
                    pattern,
                    MusicalTime::ZERO,
                    None,
                    &hir::InstrumentKind::Sine,
                    seed,
                )
                .map(|(track, _)| track)
            })
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
        let (track, end) = schedule_track(
            pattern,
            cursor,
            Some(occurrence.id),
            &occurrence.instrument,
            seed,
        )?;
        tracks.push(track);
        cursor = end;
    }
    Ok(tracks)
}

fn schedule_track(
    pattern: &hir::Pattern,
    mut cursor: MusicalTime,
    occurrence: Option<hir::NodeId>,
    instrument: &hir::InstrumentKind,
    seed: u64,
) -> Result<(Track, MusicalTime), ScheduleError> {
    let mut notes = Vec::new();
    let mut samples = Vec::new();
    for step in &pattern.steps {
        let written_duration = match step {
            hir::PatternStep::Note(note) => note.duration,
            hir::PatternStep::Chord(chord) => chord.duration,
            hir::PatternStep::Sample(sample) => sample.duration,
            hir::PatternStep::Choice(choice) => {
                let alternative = choose_weighted(
                    &choice.alternatives,
                    seed,
                    occurrence.unwrap_or(pattern.id),
                    choice.id,
                    |alternative| alternative.weight,
                )?;
                for sample in &alternative.samples {
                    let duration = musical_time(sample.duration)?;
                    samples.push(SampleEvent {
                        id: occurrence.map_or_else(
                            || id(sample.id),
                            |occurrence| occurrence_note_id(occurrence, sample.id),
                        ),
                        start: cursor,
                        duration,
                        index: sample.index,
                        velocity: sample.velocity,
                    });
                    cursor = cursor.checked_add(duration)?;
                }
                continue;
            }
            hir::PatternStep::DegreeChoice(choice) => {
                let (event, duration) =
                    degree_choice_event(choice, seed, pattern.id, cursor, occurrence)?;
                notes.push(event);
                cursor = cursor.checked_add(duration)?;
                continue;
            }
            hir::PatternStep::Rest(rest) => rest.duration,
        };
        let duration = musical_time(written_duration)?;
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
            hir::PatternStep::Sample(sample) => samples.push(SampleEvent {
                id: occurrence.map_or_else(
                    || id(sample.id),
                    |occurrence| occurrence_note_id(occurrence, sample.id),
                ),
                start: cursor,
                duration,
                index: sample.index,
                velocity: sample.velocity,
            }),
            hir::PatternStep::Choice(_) | hir::PatternStep::DegreeChoice(_) => {
                unreachable!("choices are scheduled above")
            }
            hir::PatternStep::Rest(_) => {}
        }
        cursor = cursor.checked_add(duration)?;
    }
    Ok((
        Track {
            id: id(occurrence.unwrap_or(pattern.id)),
            name: pattern.name.clone(),
            instrument: match instrument {
                hir::InstrumentKind::Sine => InstrumentKind::Sine,
                hir::InstrumentKind::Triangle => InstrumentKind::Triangle,
                hir::InstrumentKind::Sampled { source, root_midi } => InstrumentKind::Sampled {
                    source: source.clone(),
                    root_midi: *root_midi,
                },
                hir::InstrumentKind::Sampler { pack } => {
                    InstrumentKind::Sampler { pack: pack.clone() }
                }
            },
            notes,
            samples,
            end: cursor,
        },
        cursor,
    ))
}

fn degree_choice_event(
    choice: &hir::DegreeChoice,
    seed: u64,
    pattern: hir::NodeId,
    cursor: MusicalTime,
    occurrence: Option<hir::NodeId>,
) -> Result<(NoteEvent, MusicalTime), ScheduleError> {
    let alternative = choose_weighted(
        &choice.alternatives,
        seed,
        occurrence.unwrap_or(pattern),
        choice.id,
        |alternative| alternative.weight,
    )?;
    let note = alternative.note;
    let duration = musical_time(note.duration)?;
    Ok((
        note_event(
            note.id,
            note.midi_pitch,
            note.velocity,
            duration,
            cursor,
            occurrence,
        ),
        duration,
    ))
}

fn musical_time(duration: hir::Duration) -> Result<MusicalTime, TimeError> {
    MusicalTime::new(
        u64::from(duration.numerator),
        u64::from(duration.denominator),
    )
}

fn choose_weighted<T>(
    alternatives: &[T],
    seed: u64,
    track: hir::NodeId,
    choice: hir::NodeId,
    weight: impl Fn(&T) -> u32,
) -> Result<&T, ScheduleError> {
    let total = alternatives.iter().try_fold(0u64, |total, alternative| {
        total
            .checked_add(u64::from(weight(alternative)))
            .ok_or(ScheduleError::ChoiceWeightOverflow)
    })?;
    if total == 0 {
        return Err(ScheduleError::EmptyChoice(choice));
    }
    let mut roll = mix(seed ^ (u64::from(track.0) << 32) ^ u64::from(choice.0)) % total;
    alternatives
        .iter()
        .find(|alternative| {
            let alternative_weight = u64::from(weight(alternative));
            if roll < alternative_weight {
                true
            } else {
                roll -= alternative_weight;
                false
            }
        })
        .ok_or(ScheduleError::EmptyChoice(choice))
}

fn mix(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
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
    #[error("sample choice {0:?} has no alternatives")]
    EmptyChoice(hir::NodeId),
    #[error("sample choice weights exceed the supported range")]
    ChoiceWeightOverflow,
    #[error(transparent)]
    Time(#[from] TimeError),
}

fn id(id: hir::NodeId) -> EntityId {
    EntityId(u64::from(id.0))
}

fn occurrence_note_id(occurrence: hir::NodeId, note: hir::NodeId) -> EntityId {
    EntityId((u64::from(occurrence.0) << 32) | u64::from(note.0))
}
