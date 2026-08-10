use symphra_score::{
    Channels, EntityId, InstrumentKind, Key, Meter, Mode, MusicalTime, NoteEvent, PitchClass,
    SampleEvent, SampleSelector, Score, Song, TimeError, Track,
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
    if !song.tracks.is_empty() {
        return song
            .tracks
            .iter()
            .map(|track| schedule_declared_track(song, track, seed))
            .collect();
    }
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

fn schedule_declared_track(
    song: &hir::Song,
    track: &hir::TrackDefinition,
    seed: u64,
) -> Result<Track, ScheduleError> {
    let pattern = song
        .patterns
        .iter()
        .find(|pattern| pattern.id == track.pattern)
        .ok_or(ScheduleError::UnknownPattern(track.pattern))?;
    let expanded;
    let pattern = if let Some(rhythm) = track.trigger_with {
        let rhythm = song
            .rhythms
            .iter()
            .find(|candidate| candidate.id == rhythm)
            .ok_or(ScheduleError::UnknownRhythm(rhythm))?;
        expanded = triggered_pattern(pattern, rhythm, track)?;
        &expanded
    } else {
        pattern
    };
    let (mut scheduled, _) = schedule_track(
        pattern,
        MusicalTime::ZERO,
        Some(track.id),
        &track.instrument,
        seed,
    )?;
    scheduled.name.clone_from(&track.name);
    scheduled.gain = track.gain;
    scheduled.pan = score_pan(track.pan);
    if let Some(semitones) = track.transpose_semitones {
        apply_transpose(&mut scheduled, semitones)?;
    }
    if let Some(percent) = track.gate_percent {
        apply_gate(&mut scheduled, percent)?;
    }
    apply_repeat(&mut scheduled, track.repeat_count)?;
    if let Some(chance) = track.chance {
        apply_chance(&mut scheduled, chance, seed)?;
    }
    if let Some(range) = track.choose_sample {
        apply_choose_sample(&mut scheduled, range, seed);
    }
    if track.reverse {
        apply_reverse(&mut scheduled)?;
    }
    apply_speed(&mut scheduled, track.speed);
    if let Some(chance) = track.chance {
        apply_chance_speed(&mut scheduled, chance, seed);
    }
    if let Some(offset) = track.at {
        apply_at(&mut scheduled, musical_time(offset)?)?;
    }
    if let Some(effect) = track.effect {
        scheduled.effect = Some(symphra_score::DelayEffect {
            mix: effect.mix,
            time: musical_time(effect.time)?,
            feedback: effect.feedback,
        });
    }
    Ok(scheduled)
}

/// Shifts every event and the track's end by `offset`, applied last so every
/// earlier stage (`repeat`, `reverse`, ...) keeps operating in the pattern's
/// own `[0, duration]` space and stays correct regardless of where the whole
/// track ultimately lands in the song.
fn apply_at(track: &mut Track, offset: MusicalTime) -> Result<(), TimeError> {
    for note in &mut track.notes {
        note.start = note.start.checked_add(offset)?;
    }
    for sample in &mut track.samples {
        sample.start = sample.start.checked_add(offset)?;
    }
    track.end = track.end.checked_add(offset)?;
    Ok(())
}

fn apply_choose_sample(track: &mut Track, range: hir::SampleRange, seed: u64) {
    let span = u64::from(range.end - range.start) + 1;
    for sample in &mut track.samples {
        let offset = mix(seed ^ sample.id.0) % span;
        let index = u32::try_from(offset).expect("offset is bounded by a u32 span") + range.start;
        sample.selector = SampleSelector::Index(index);
    }
}

fn apply_speed(track: &mut Track, speed: hir::Speed) {
    for (index, sample) in track.samples.iter_mut().enumerate() {
        sample.speed = match speed {
            hir::Speed::Fixed(factor) => factor,
            hir::Speed::Alternate { first, .. } if index % 2 == 0 => first,
            hir::Speed::Alternate { second, .. } => second,
        };
    }
}

fn apply_chance(track: &mut Track, chance: hir::Chance, seed: u64) -> Result<(), ScheduleError> {
    match chance.transform {
        hir::ChanceTransform::Transpose(semitones) => {
            for note in &mut track.notes {
                if chance_selected(seed, note.id, chance.percent) {
                    note.midi_pitch = crate::transposed_pitch(note.midi_pitch, semitones)
                        .ok_or(ScheduleError::TransposedPitchOutOfRange)?;
                }
            }
        }
        hir::ChanceTransform::Retrigger(count) => {
            apply_chance_retrigger(track, count, chance.percent, seed)?;
        }
        hir::ChanceTransform::Speed(_) => {}
    }
    Ok(())
}

/// Applies `chance { speed F }` after the track's base playback speed so the
/// chance-selected events consistently win over the unconditional `speed`/
/// `alternate { speed }` pipeline stage.
fn apply_chance_speed(track: &mut Track, chance: hir::Chance, seed: u64) {
    if let hir::ChanceTransform::Speed(factor) = chance.transform {
        for sample in &mut track.samples {
            if chance_selected(seed, sample.id, chance.percent) {
                sample.speed = factor;
            }
        }
    }
}

fn apply_chance_retrigger(
    track: &mut Track,
    count: u32,
    percent: u8,
    seed: u64,
) -> Result<(), ScheduleError> {
    let track_id = track.id;
    let mut next_event = track
        .notes
        .iter()
        .map(|event| event.id.0 & u64::from(u32::MAX))
        .chain(
            track
                .samples
                .iter()
                .map(|event| event.id.0 & u64::from(u32::MAX)),
        )
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(ScheduleError::RepeatedEventIdOverflow)?;
    let mut retriggered = Vec::with_capacity(track.samples.len());
    for sample in track.samples.drain(..) {
        if !chance_selected(seed, sample.id, percent) {
            retriggered.push(sample);
            continue;
        }
        let attack_duration = MusicalTime::new(
            sample.duration.numerator(),
            sample
                .duration
                .denominator()
                .checked_mul(u64::from(count))
                .ok_or(ScheduleError::RepeatTooLarge)?,
        )?;
        let mut start = sample.start;
        for attack in 0..count {
            let id = if attack == 0 {
                sample.id
            } else {
                repeated_event_id(track_id, &mut next_event)?
            };
            retriggered.push(SampleEvent {
                id,
                start,
                duration: attack_duration,
                selector: sample.selector.clone(),
                velocity: sample.velocity,
                speed: sample.speed,
            });
            start = start.checked_add(attack_duration)?;
        }
    }
    track.samples = retriggered;
    Ok(())
}

fn chance_selected(seed: u64, id: EntityId, percent: u8) -> bool {
    mix(seed ^ id.0) % 100 < u64::from(percent)
}

fn apply_reverse(track: &mut Track) -> Result<(), TimeError> {
    for note in &mut track.notes {
        note.start = track
            .end
            .checked_sub(note.start.checked_add(note.duration)?)?;
    }
    track.notes.reverse();
    for sample in &mut track.samples {
        sample.start = track
            .end
            .checked_sub(sample.start.checked_add(sample.duration)?)?;
    }
    track.samples.reverse();
    Ok(())
}

fn apply_repeat(track: &mut Track, count: u16) -> Result<(), ScheduleError> {
    if count == 0 {
        return Err(ScheduleError::InvalidRepeatCount);
    }
    if count == 1 {
        return Ok(());
    }
    let note_count = track.notes.len();
    let sample_count = track.samples.len();
    let copies = usize::from(count - 1);
    track
        .notes
        .try_reserve(
            note_count
                .checked_mul(copies)
                .ok_or(ScheduleError::RepeatTooLarge)?,
        )
        .map_err(|_| ScheduleError::RepeatTooLarge)?;
    track
        .samples
        .try_reserve(
            sample_count
                .checked_mul(copies)
                .ok_or(ScheduleError::RepeatTooLarge)?,
        )
        .map_err(|_| ScheduleError::RepeatTooLarge)?;

    let mut next_event = track
        .notes
        .iter()
        .map(|event| event.id.0 & u64::from(u32::MAX))
        .chain(
            track
                .samples
                .iter()
                .map(|event| event.id.0 & u64::from(u32::MAX)),
        )
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(ScheduleError::RepeatedEventIdOverflow)?;
    let mut offset = track.end;
    for _ in 1..count {
        for index in 0..note_count {
            let mut event = track.notes[index];
            event.id = repeated_event_id(track.id, &mut next_event)?;
            event.start = event.start.checked_add(offset)?;
            track.notes.push(event);
        }
        for index in 0..sample_count {
            let mut event = track.samples[index].clone();
            event.id = repeated_event_id(track.id, &mut next_event)?;
            event.start = event.start.checked_add(offset)?;
            track.samples.push(event);
        }
        offset = offset.checked_add(track.end)?;
    }
    track.end = offset;
    Ok(())
}

fn repeated_event_id(track: EntityId, next_event: &mut u64) -> Result<EntityId, ScheduleError> {
    if *next_event > u64::from(u32::MAX) {
        return Err(ScheduleError::RepeatedEventIdOverflow);
    }
    let id = EntityId((track.0 << 32) | *next_event);
    *next_event += 1;
    Ok(id)
}

fn apply_transpose(track: &mut Track, semitones: i32) -> Result<(), ScheduleError> {
    for note in &mut track.notes {
        note.midi_pitch = crate::transposed_pitch(note.midi_pitch, semitones)
            .ok_or(ScheduleError::TransposedPitchOutOfRange)?;
    }
    Ok(())
}

fn apply_gate(track: &mut Track, percent: u8) -> Result<(), TimeError> {
    for note in &mut track.notes {
        note.duration = gated_duration(note.duration, percent)?;
    }
    for sample in &mut track.samples {
        sample.duration = gated_duration(sample.duration, percent)?;
    }
    Ok(())
}

fn gated_duration(duration: MusicalTime, percent: u8) -> Result<MusicalTime, TimeError> {
    let numerator = duration
        .numerator()
        .checked_mul(u64::from(percent))
        .ok_or(TimeError::Overflow)?;
    let denominator = duration
        .denominator()
        .checked_mul(100)
        .ok_or(TimeError::Overflow)?;
    MusicalTime::new(numerator, denominator)
}

fn triggered_pattern(
    pattern: &hir::Pattern,
    rhythm: &hir::Rhythm,
    track: &hir::TrackDefinition,
) -> Result<hir::Pattern, ScheduleError> {
    if rhythm.items.is_empty() {
        return Err(ScheduleError::EmptyRhythm(rhythm.id));
    }
    let mut steps = Vec::new();
    let mut rhythm_index = 0usize;
    let mut next_id = 0u32;
    for step in &pattern.steps {
        let duration = step_duration(step)?;
        let cells = crate::rhythm_cell_count(duration, rhythm.resolution)
            .ok_or(ScheduleError::IncompatibleRhythmResolution)?;
        for _ in 0..cells {
            let hit = rhythm.items[rhythm_index % rhythm.items.len()] == hir::RhythmItem::Hit;
            rhythm_index += 1;
            steps.push(triggered_step(step, hit, rhythm.resolution, &mut next_id)?);
        }
    }
    Ok(hir::Pattern {
        id: track.id,
        name: track.name.clone(),
        steps,
    })
}

fn step_duration(step: &hir::PatternStep) -> Result<hir::Duration, ScheduleError> {
    match step {
        hir::PatternStep::Note(note) => Ok(note.duration),
        hir::PatternStep::Chord(chord) => Ok(chord.duration),
        hir::PatternStep::Rest(rest) => Ok(rest.duration),
        hir::PatternStep::Sample(sample) => Ok(sample.duration),
        // Every alternative in a compiled DegreeChoice shares the same
        // step-cell duration, unlike Choice's sample sequences (see below).
        hir::PatternStep::DegreeChoice(choice) => choice
            .alternatives
            .first()
            .map(|alternative| alternative.note.duration)
            .ok_or(ScheduleError::UnsupportedTriggeredStep),
        // A Choice alternative's duration only stays fixed when it selects
        // exactly one sample; a multi-sample sequence's duration depends on
        // which alternative gets picked, which isn't known until the
        // weighted selection runs during scheduling.
        hir::PatternStep::Choice(choice) => {
            if choice
                .alternatives
                .iter()
                .any(|alternative| alternative.samples.len() != 1)
            {
                return Err(ScheduleError::UnsupportedTriggeredStep);
            }
            choice
                .alternatives
                .first()
                .and_then(|alternative| alternative.samples.first())
                .map(|sample| sample.duration)
                .ok_or(ScheduleError::UnsupportedTriggeredStep)
        }
    }
}

fn triggered_step(
    step: &hir::PatternStep,
    hit: bool,
    duration: hir::Duration,
    next_id: &mut u32,
) -> Result<hir::PatternStep, ScheduleError> {
    if !hit || matches!(step, hir::PatternStep::Rest(_)) {
        return Ok(hir::PatternStep::Rest(hir::Rest {
            id: generated_id(next_id)?,
            duration,
        }));
    }
    match step {
        hir::PatternStep::Note(note) => Ok(hir::PatternStep::Note(hir::Note {
            id: generated_id(next_id)?,
            duration,
            ..*note
        })),
        hir::PatternStep::Chord(chord) => Ok(hir::PatternStep::Chord(hir::Chord {
            notes: chord
                .notes
                .iter()
                .map(|note| {
                    Ok(hir::ChordNote {
                        id: generated_id(next_id)?,
                        midi_pitch: note.midi_pitch,
                    })
                })
                .collect::<Result<_, ScheduleError>>()?,
            duration,
            velocity: chord.velocity,
        })),
        hir::PatternStep::Rest(_) => unreachable!("rests are handled above"),
        hir::PatternStep::Sample(sample) => Ok(hir::PatternStep::Sample(hir::SampleTrigger {
            id: generated_id(next_id)?,
            duration,
            selector: sample.selector.clone(),
            velocity: sample.velocity,
        })),
        hir::PatternStep::DegreeChoice(choice) => {
            Ok(hir::PatternStep::DegreeChoice(hir::DegreeChoice {
                id: generated_id(next_id)?,
                alternatives: choice
                    .alternatives
                    .iter()
                    .map(|alternative| {
                        Ok(hir::WeightedNote {
                            note: hir::Note {
                                id: generated_id(next_id)?,
                                duration,
                                ..alternative.note
                            },
                            weight: alternative.weight,
                        })
                    })
                    .collect::<Result<_, ScheduleError>>()?,
            }))
        }
        hir::PatternStep::Choice(choice) => Ok(hir::PatternStep::Choice(hir::SampleChoice {
            id: generated_id(next_id)?,
            alternatives: choice
                .alternatives
                .iter()
                .map(|alternative| {
                    let sample = alternative
                        .samples
                        .first()
                        .ok_or(ScheduleError::UnsupportedTriggeredStep)?;
                    Ok(hir::WeightedSampleSequence {
                        samples: vec![hir::SampleTrigger {
                            id: generated_id(next_id)?,
                            duration,
                            selector: sample.selector.clone(),
                            velocity: sample.velocity,
                        }],
                        weight: alternative.weight,
                    })
                })
                .collect::<Result<_, ScheduleError>>()?,
        })),
    }
}

fn generated_id(next_id: &mut u32) -> Result<hir::NodeId, ScheduleError> {
    let id = hir::NodeId(*next_id);
    *next_id = (*next_id)
        .checked_add(1)
        .ok_or(ScheduleError::TriggeredPatternTooLong)?;
    Ok(id)
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
                        selector: score_sample_selector(&sample.selector),
                        velocity: sample.velocity,
                        speed: 1.0,
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
                selector: score_sample_selector(&sample.selector),
                velocity: sample.velocity,
                speed: 1.0,
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
            instrument: score_instrument(instrument),
            notes,
            samples,
            gain: 1.0,
            pan: symphra_score::Pan::Fixed(0),
            end: cursor,
            effect: None,
        },
        cursor,
    ))
}

fn score_instrument(instrument: &hir::InstrumentKind) -> InstrumentKind {
    match instrument {
        hir::InstrumentKind::Sine => InstrumentKind::Sine,
        hir::InstrumentKind::Triangle => InstrumentKind::Triangle,
        hir::InstrumentKind::Sampled { source, root_midi } => InstrumentKind::Sampled {
            source: source.clone(),
            root_midi: *root_midi,
        },
        hir::InstrumentKind::Sampler { pack } => InstrumentKind::Sampler { pack: pack.clone() },
        hir::InstrumentKind::DrumMachine { bank } => {
            InstrumentKind::DrumMachine { bank: bank.clone() }
        }
    }
}

fn score_sample_selector(selector: &hir::SampleSelector) -> SampleSelector {
    match selector {
        hir::SampleSelector::Index(index) => SampleSelector::Index(*index),
        hir::SampleSelector::Named(name) => SampleSelector::Named(name.clone()),
    }
}

fn score_pan(pan: hir::Pan) -> symphra_score::Pan {
    match pan {
        hir::Pan::Fixed(percent) => symphra_score::Pan::Fixed(percent),
        hir::Pan::Alternate {
            left_percent,
            right_percent,
        } => symphra_score::Pan::Alternate {
            left_percent,
            right_percent,
        },
    }
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
    #[error("track references unknown rhythm ID {0:?}")]
    UnknownRhythm(hir::NodeId),
    #[error("trigger_with rhythm must contain at least one item")]
    EmptyRhythm(hir::NodeId),
    #[error("pattern step duration must be divisible by rhythm resolution")]
    IncompatibleRhythmResolution,
    #[error("trigger_with requires every choose alternative to select exactly one sample")]
    UnsupportedTriggeredStep,
    #[error("triggered pattern contains too many events")]
    TriggeredPatternTooLong,
    #[error("transposed pitch must be within the MIDI range 0 to 127")]
    TransposedPitchOutOfRange,
    #[error("repeat produces too many events")]
    RepeatTooLarge,
    #[error("repeat count must be greater than zero")]
    InvalidRepeatCount,
    #[error("repeat exhausts the event ID range")]
    RepeatedEventIdOverflow,
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
