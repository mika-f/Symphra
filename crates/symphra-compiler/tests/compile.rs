use symphra_compiler::hir::{
    Arrangement, Channels, Duration, InstrumentKind, Key, Meter, Mode, NodeId, Note, Pattern,
    PatternOccurrence, PatternStep, PitchClass, Program, Project, Song,
};
use symphra_compiler::{ScheduleError, compile, schedule};
use symphra_score::MusicalTime;
use symphra_syntax::{SourceId, parse};

const EXAMPLE: &str = r#"
project {
  seed 20260809
  sample_rate 48khz
  output stereo
}

song "First Song" {
  tempo 150bpm
  meter 4/4
  key C major
  pattern melody = sequence {
    note C4 for 1/4
    note E4 for 1/4
    note G4 for 1/2
  }
}
"#;

#[test]
fn compile_should_lower_valid_source_to_normalized_hir() {
    let parsed = parse(SourceId(0), EXAMPLE);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);

    let program = compile(&parsed.file).expect("example should compile");

    assert_eq!(
        program,
        Program {
            project: Project {
                seed: 20_260_809,
                sample_rate_hz: 48_000,
                channels: Channels::Stereo,
            },
            songs: vec![Song {
                id: NodeId(0),
                name: "First Song".to_owned(),
                tempo_bpm: 150.0,
                meter: Meter {
                    numerator: 4,
                    denominator: 4,
                },
                key: Key {
                    tonic: PitchClass::C,
                    mode: Mode::Major,
                },
                patterns: vec![Pattern {
                    id: NodeId(1),
                    name: "melody".to_owned(),
                    steps: vec![
                        PatternStep::Note(Note {
                            id: NodeId(2),
                            midi_pitch: 60,
                            velocity: 127,
                            duration: Duration {
                                numerator: 1,
                                denominator: 4,
                            },
                        }),
                        PatternStep::Note(Note {
                            id: NodeId(3),
                            midi_pitch: 64,
                            velocity: 127,
                            duration: Duration {
                                numerator: 1,
                                denominator: 4,
                            },
                        }),
                        PatternStep::Note(Note {
                            id: NodeId(4),
                            midi_pitch: 67,
                            velocity: 127,
                            duration: Duration {
                                numerator: 1,
                                denominator: 2,
                            },
                        }),
                    ],
                }],
                arrangement: None,
            }],
        }
    );
}

#[test]
fn compile_should_report_all_invalid_musical_values() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48bpm output surround }
song "Bad" {
  tempo 0bpm
  meter 4/0
  key H dorian
  pattern bad = sequence {
    note H4 for 0/4
    rest for 1/0
    chord C4 H4 for 0/4
    note C-2 for 1/4 velocity 128
  }
}
"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);

    let diagnostics = compile(&parsed.file).expect_err("invalid values should fail");
    let messages = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        messages,
        [
            "sample_rate unit must be `hz` or `khz`",
            "output must be `mono` or `stereo`",
            "tempo must be greater than zero",
            "meter values must be greater than zero",
            "key tonic must be a natural note from A to G",
            "pitch must be a natural note followed by an octave",
            "note duration must be greater than zero",
            "rest duration must be greater than zero",
            "pitch must be a natural note followed by an octave",
            "chord duration must be greater than zero",
            "pitch must be within the MIDI range C-1 to G9",
            "velocity must be from 0 to 127",
        ]
    );
}

#[test]
fn schedule_should_place_sequence_notes_back_to_back() {
    let parsed = parse(SourceId(0), EXAMPLE);
    let program = compile(&parsed.file).expect("example should compile");

    let score = schedule(&program).expect("example times should fit");
    let starts = score.songs[0].tracks[0]
        .notes
        .iter()
        .map(|note| note.start)
        .collect::<Vec<_>>();

    assert_eq!(
        starts,
        [
            MusicalTime::ZERO,
            MusicalTime::new(1, 4).expect("quarter note should be valid"),
            MusicalTime::new(1, 2).expect("half note should be valid"),
        ]
    );
}

#[test]
fn schedule_should_advance_over_rests_and_preserve_trailing_silence() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Rests" {
  tempo 120bpm meter 4/4 key C major
  pattern phrase = sequence {
    note C4 for 1/4
    rest for 1/4
    note E4 for 1/4
    rest for 1/4
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("rests should compile");

    let score = schedule(&program).expect("rests should schedule");
    let track = &score.songs[0].tracks[0];
    assert_eq!(
        (track.notes[0].start, track.notes[1].start, track.end),
        (
            MusicalTime::ZERO,
            MusicalTime::new(1, 2).expect("half note should be valid"),
            MusicalTime::new(1, 1).expect("whole note should be valid"),
        )
    );
}

#[test]
fn schedule_should_start_chord_notes_together() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Chords" {
  tempo 120bpm meter 4/4 key C major
  pattern harmony = sequence {
    chord C#4 Eb4 G4 for 1/4 velocity 96
    note C5 for 1/4
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("chord should compile");

    let score = schedule(&program).expect("chord should schedule");
    let notes = &score.songs[0].tracks[0].notes;
    assert_eq!(
        notes
            .iter()
            .map(|note| (note.midi_pitch, note.start, note.velocity))
            .collect::<Vec<_>>(),
        vec![
            (61, MusicalTime::ZERO, 96),
            (63, MusicalTime::ZERO, 96),
            (67, MusicalTime::ZERO, 96),
            (
                72,
                MusicalTime::new(1, 4).expect("quarter note should be valid"),
                127,
            ),
        ]
    );
}

#[test]
fn schedule_should_accept_full_midi_pitch_range() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Range" {
  tempo 120bpm meter 4/4 key C major
  pattern range = sequence { chord C-1 G9 for 1/4 }
}
"#,
    );
    let program = compile(&parsed.file).expect("MIDI boundary pitches should compile");

    let score = schedule(&program).expect("MIDI boundary pitches should schedule");
    assert_eq!(
        score.songs[0].tracks[0]
            .notes
            .iter()
            .map(|note| note.midi_pitch)
            .collect::<Vec<_>>(),
        [0, 127]
    );
}

#[test]
fn schedule_should_place_arranged_patterns_back_to_back() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Arranged" {
  tempo 120bpm meter 4/4 key C major
  pattern intro = sequence { note C4 for 1/4 }
  pattern outro = sequence { note G4 for 1/2 }
  arrangement { outro intro }
}
"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let program = compile(&parsed.file).expect("arrangement should resolve");

    let score = schedule(&program).expect("arrangement times should fit");
    assert_eq!(
        score.songs[0]
            .tracks
            .iter()
            .map(|track| (track.name.as_str(), track.notes[0].start))
            .collect::<Vec<_>>(),
        [
            ("outro", MusicalTime::ZERO),
            (
                "intro",
                MusicalTime::new(1, 2).expect("half note should be valid")
            ),
        ]
    );
}

#[test]
fn compile_should_reject_invalid_arrangement_references() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Invalid arrangement" {
  tempo 120bpm meter 4/4 key C major
  pattern melody = sequence {}
  arrangement { missing melody }
}
"#,
    );
    let diagnostics = compile(&parsed.file).expect_err("invalid arrangement should fail");

    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        ["arrangement references an unknown pattern"]
    );
}

#[test]
fn schedule_should_reject_invalid_arrangements_in_manual_hir() {
    let parsed = parse(SourceId(0), EXAMPLE);
    let mut program = compile(&parsed.file).expect("example should compile");
    program.songs[0].arrangement = Some(Arrangement {
        occurrences: vec![PatternOccurrence {
            id: NodeId(99),
            pattern: NodeId(u32::MAX),
            instrument: InstrumentKind::Sine,
        }],
    });
    let unknown = schedule(&program);
    program.songs[0].arrangement = Some(Arrangement {
        occurrences: vec![
            PatternOccurrence {
                id: NodeId(99),
                pattern: NodeId(1),
                instrument: InstrumentKind::Sine,
            },
            PatternOccurrence {
                id: NodeId(99),
                pattern: NodeId(1),
                instrument: InstrumentKind::Sine,
            },
        ],
    });
    let duplicate = schedule(&program);

    assert_eq!(
        (unknown, duplicate),
        (
            Err(ScheduleError::UnknownPattern(NodeId(u32::MAX))),
            Err(ScheduleError::DuplicateOccurrence(NodeId(99))),
        )
    );
}

#[test]
fn schedule_should_apply_arrangement_instruments_with_sine_default() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Instruments" {
  tempo 120bpm meter 4/4 key C major
  arrangement { phrase with lead phrase }
  instrument lead = triangle
  pattern phrase = sequence { note C4 for 1/4 }
}
"#,
    );
    let program = compile(&parsed.file).expect("instruments should compile");

    let score = schedule(&program).expect("instruments should schedule");

    assert_eq!(
        (
            program.songs[0].arrangement.as_ref().map(|arrangement| {
                arrangement
                    .occurrences
                    .iter()
                    .map(|occurrence| occurrence.instrument.clone())
                    .collect::<Vec<_>>()
            }),
            score.songs[0]
                .tracks
                .iter()
                .map(|track| track.instrument.clone())
                .collect::<Vec<_>>(),
        ),
        (
            Some(vec![InstrumentKind::Triangle, InstrumentKind::Sine]),
            vec![
                symphra_score::InstrumentKind::Triangle,
                symphra_score::InstrumentKind::Sine,
            ],
        )
    );
}

#[test]
fn compile_should_reject_invalid_instruments() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Invalid instruments" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = square
  instrument lead = sine
  pattern phrase = sequence {}
  arrangement { phrase with missing }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("invalid instruments should fail");

    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        [
            "instrument kind must be `sine`, `triangle`, `sampled`, or `sampler`",
            "instrument name is declared more than once",
            "arrangement references an unknown instrument",
        ]
    );
}

#[test]
fn compile_should_lower_sampled_instruments() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Sample" {
  tempo 120bpm meter 4/4 key C major
  arrangement { phrase with piano }
  instrument piano = sampled { source "samples/piano-c4.wav" root C4 }
  pattern phrase = sequence { note C4 for 1/4 }
}
"#,
    );

    let program = compile(&parsed.file).expect("sampled instrument should compile");

    assert_eq!(
        program.songs[0]
            .arrangement
            .as_ref()
            .and_then(|arrangement| arrangement.occurrences.first())
            .map(|occurrence| &occurrence.instrument),
        Some(&InstrumentKind::Sampled {
            source: "samples/piano-c4.wav".to_owned(),
            root_midi: 60,
        })
    );
}

#[test]
fn compile_should_lower_sampler_packs() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Sampler" {
  tempo 120bpm meter 4/4 key C major
  instrument voice_numbers = sampler { pack "numbers" }
  pattern phrase = sequence { rest for 1/4 }
  arrangement { phrase with voice_numbers }
}
"#,
    );

    let program = compile(&parsed.file).expect("sampler instrument should compile");

    assert_eq!(
        program.songs[0]
            .arrangement
            .as_ref()
            .and_then(|arrangement| arrangement.occurrences.first())
            .map(|occurrence| &occurrence.instrument),
        Some(&InstrumentKind::Sampler {
            pack: "numbers".to_owned(),
        })
    );
}

#[test]
fn compile_should_reject_empty_sample_asset_names() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Empty samples" {
  tempo 120bpm meter 4/4 key C major
  instrument piano = sampled { source "" root C4 }
  instrument voice = sampler { pack "" }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("empty asset names should fail");

    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        [
            "sample source path must not be empty",
            "sample pack name must not be empty",
        ]
    );
}

#[test]
fn schedule_should_give_repeated_patterns_unique_occurrence_ids() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Repeated" {
  tempo 120bpm meter 4/4 key C major
  pattern phrase = sequence { note C4 for 1/4 }
  arrangement { phrase phrase }
}
"#,
    );
    let program = compile(&parsed.file).expect("repeated pattern should compile");

    let score = schedule(&program).expect("repeated pattern should schedule");
    let tracks = &score.songs[0].tracks;
    assert_eq!(
        (
            tracks[0].notes[0].start,
            tracks[1].notes[0].start,
            tracks[0].id != tracks[1].id,
            tracks[0].notes[0].id != tracks[1].notes[0].id,
        ),
        (
            MusicalTime::ZERO,
            MusicalTime::new(1, 4).expect("quarter note should be valid"),
            true,
            true,
        )
    );
}
