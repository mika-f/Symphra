use symphra_compiler::hir::{
    Arrangement, Channels, Duration, InstrumentKind, Key, Meter, Mode, NodeId, Note, Pattern,
    PatternOccurrence, PatternStep, PitchClass, Program, Project, Rhythm, RhythmItem, Song,
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
                rhythms: Vec::new(),
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
                tracks: Vec::new(),
                arrangement: None,
            }],
        }
    );
}

#[test]
fn compile_should_lower_reusable_rhythms() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Rhythm" {
  tempo 120bpm meter 4/4 key C major
  rhythm pulse resolution 1/8 { hit rest hit }
}
"#,
    );
    let program = compile(&parsed.file).expect("rhythm should compile");

    assert_eq!(
        program.songs[0].rhythms,
        [Rhythm {
            id: NodeId(1),
            name: "pulse".to_owned(),
            resolution: Duration {
                numerator: 1,
                denominator: 8,
            },
            items: vec![RhythmItem::Hit, RhythmItem::Rest, RhythmItem::Hit],
        }]
    );
}

#[test]
fn compile_should_reject_invalid_rhythms() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Rhythm" {
  tempo 120bpm meter 4/4 key C major
  rhythm pulse resolution 0/8 { hit }
  rhythm pulse resolution 1/8 { rest }
}
"#,
    );
    let diagnostics = compile(&parsed.file).expect_err("invalid rhythms should fail");

    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        [
            "rhythm name is declared more than once",
            "rhythm resolution duration must be greater than zero",
        ]
    );
}

#[test]
fn schedule_should_apply_rhythms_and_gate_to_tracks() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Triggered" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  rhythm stabs resolution 1/4 { hit rest hit rest }
  pattern harmony = sequence { chord C4 E4 G4 for 1/1 }
  track chords role harmony {
    instrument lead
    play harmony |> trigger_with stabs |> gate 50%
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("triggered track should compile");
    let score = schedule(&program).expect("triggered track should schedule");
    let track = &score.songs[0].tracks[0];

    assert_eq!(
        (
            track.name.as_str(),
            track.notes.len(),
            track.notes[0].start,
            track.notes[3].start,
            track.notes[0].duration,
            track.end,
        ),
        (
            "chords",
            6,
            MusicalTime::ZERO,
            MusicalTime::new(1, 2).expect("half note should be valid"),
            MusicalTime::new(1, 8).expect("eighth note should be valid"),
            MusicalTime::new(1, 1).expect("whole note should be valid"),
        )
    );
}

#[test]
fn compile_should_reject_gate_above_one_hundred_percent() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Gate" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody |> gate 101%
  }
}
"#,
    );
    let diagnostics = compile(&parsed.file).expect_err("invalid gate should fail");

    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        ["gate must be from 0% to 100%"]
    );
}

#[test]
fn schedule_should_transpose_tracks_without_moving_events() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Transpose" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 note E4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody |> transpose -12st
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("transposed track should compile");
    let score = schedule(&program).expect("transposed track should schedule");
    let track = &score.songs[0].tracks[0];

    assert_eq!(
        (
            track.notes[0].midi_pitch,
            track.notes[1].midi_pitch,
            track.notes[1].start,
            track.end,
        ),
        (
            48,
            52,
            MusicalTime::new(1, 4).expect("quarter note should be valid"),
            MusicalTime::new(1, 2).expect("half note should be valid"),
        )
    );
}

#[test]
fn compile_should_reject_transposed_pitches_outside_midi_range() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Transpose" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C9 for 1/4 }
  track lead role melody {
    instrument lead
    play melody |> transpose +12st
  }
}
"#,
    );
    let diagnostics = compile(&parsed.file).expect_err("out-of-range transpose should fail");

    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        ["transposed pitch must be within the MIDI range 0 to 127"]
    );
}

#[test]
fn schedule_should_preserve_linear_track_gain() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Gain" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody |> gain 0.30
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("gained track should compile");
    let score = schedule(&program).expect("gained track should schedule");

    assert!((score.songs[0].tracks[0].gain - 0.30).abs() < f32::EPSILON);
}

#[test]
fn schedule_should_repeat_tracks_sequentially_with_unique_event_ids() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Repeat" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody |> repeat 3
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("repeated track should compile");
    let score = schedule(&program).expect("repeated track should schedule");
    let track = &score.songs[0].tracks[0];

    assert_eq!(
        (
            track
                .notes
                .iter()
                .map(|note| note.start)
                .collect::<Vec<_>>(),
            track.end,
            track.notes.windows(2).all(|pair| pair[0].id != pair[1].id),
        ),
        (
            vec![
                MusicalTime::ZERO,
                MusicalTime::new(1, 4).expect("quarter note should be valid"),
                MusicalTime::new(1, 2).expect("half note should be valid"),
            ],
            MusicalTime::new(3, 4).expect("three quarters should be valid"),
            true,
        )
    );
}

#[test]
fn compile_should_reject_zero_repeats() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Repeat" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody |> repeat 0
  }
}
"#,
    );
    let diagnostics = compile(&parsed.file).expect_err("zero repeats should fail");

    assert_eq!(diagnostics[0].message, "repeat must be from 1 to 65535");
}

#[test]
fn compile_should_reject_out_of_range_pan() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output stereo }
song "Pan" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody |> pan -101%
  }
}
"#,
    );
    let diagnostics = compile(&parsed.file).expect_err("out-of-range pan should fail");

    assert_eq!(diagnostics[0].message, "pan must be from -100% to 100%");
}

#[test]
fn compile_should_reject_out_of_range_alternating_pan() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output stereo }
song "Pan" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody |> pan alternate(30%, 101%)
  }
}
"#,
    );
    let diagnostics = compile(&parsed.file).expect_err("out-of-range alternate pan should fail");

    assert_eq!(
        diagnostics[0].message,
        "alternate pan values must be from 0% to 100%"
    );
}

#[test]
fn schedule_should_preserve_track_pan() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output stereo }
song "Pan" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody |> pan +55%
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("panned track should compile");
    let score = schedule(&program).expect("panned track should schedule");

    assert_eq!(score.songs[0].tracks[0].pan, symphra_score::Pan::Fixed(55));
}

#[test]
fn schedule_should_preserve_alternating_track_pan() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output stereo }
song "Pan" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 note D4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody |> pan alternate(30%, 70%)
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("alternating pan should compile");
    let score = schedule(&program).expect("alternating pan should schedule");

    assert_eq!(
        score.songs[0].tracks[0].pan,
        symphra_score::Pan::Alternate {
            left_percent: 30,
            right_percent: 70,
        }
    );
}

#[test]
fn schedule_should_reverse_events_within_the_track_duration() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Reverse" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence {
    note C4 for 1/4
    rest for 1/4
    note D4 for 1/2
  }
  track lead role melody {
    instrument lead
    play melody |> reverse
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("reversed track should compile");
    let score = schedule(&program).expect("reversed track should schedule");

    assert_eq!(
        score.songs[0].tracks[0]
            .notes
            .iter()
            .map(|note| (note.midi_pitch, note.start, note.duration))
            .collect::<Vec<_>>(),
        [
            (
                62,
                MusicalTime::ZERO,
                MusicalTime::new(1, 2).expect("half note should be valid"),
            ),
            (
                60,
                MusicalTime::new(3, 4).expect("three quarters should be valid"),
                MusicalTime::new(1, 4).expect("quarter note should be valid"),
            ),
        ]
    );
}

#[test]
fn schedule_should_convert_track_volume_from_decibels() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Volume" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    volume -6db
    play melody
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("track volume should compile");
    let score = schedule(&program).expect("track volume should schedule");
    let expected = 10.0_f32.powf(-6.0 / 20.0);

    assert!((score.songs[0].tracks[0].gain - expected).abs() < f32::EPSILON);
}

#[test]
fn compile_should_reject_non_decibel_track_volume() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Volume" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    volume -6hz
    play melody
  }
}
"#,
    );
    let diagnostics = compile(&parsed.file).expect_err("non-decibel volume should fail");

    assert_eq!(diagnostics[0].message, "volume unit must be `db`");
}

#[test]
fn compile_should_reject_incompatible_rhythm_triggers() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Triggered" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  rhythm pulse resolution 1/3 { hit }
  pattern melody = sequence { note C4 for 1/4 }
  track lead role harmony {
    instrument lead
    play melody |> trigger_with pulse
  }
}
"#,
    );
    let diagnostics = compile(&parsed.file).expect_err("incompatible trigger should fail");

    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        ["pattern step duration must be divisible by rhythm resolution"]
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
fn schedule_should_create_sample_events_from_steps() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Sampler" {
  tempo 120bpm meter 4/4 key C major
  instrument voice_numbers = sampler { pack "numbers" }
  pattern phrase = steps 1/8 { sample 1 rest sample 3 }
  arrangement { phrase with voice_numbers }
}
"#,
    );
    let program = compile(&parsed.file).expect("sample steps should compile");

    let score = schedule(&program).expect("sample steps should schedule");

    assert_eq!(
        score.songs[0].tracks[0]
            .samples
            .iter()
            .map(|event| (event.index, event.start))
            .collect::<Vec<_>>(),
        vec![
            (1, MusicalTime::ZERO),
            (
                3,
                MusicalTime::new(1, 4).expect("quarter note should be valid")
            ),
        ]
    );
}

#[test]
fn schedule_should_resolve_degree_steps_from_the_song_key() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Degrees" {
  tempo 120bpm meter 4/4
  pattern phrase = steps 1/8 {
    degree 0 octave 4
    degree 2 octave 4
    degree 12 octave 4
  }
  key D major
  arrangement { phrase }
}
"#,
    );
    let program = compile(&parsed.file).expect("degree steps should compile");
    let score = schedule(&program).expect("degree steps should schedule");

    assert_eq!(
        score.songs[0].tracks[0]
            .notes
            .iter()
            .map(|event| (event.midi_pitch, event.start))
            .collect::<Vec<_>>(),
        [
            (62, MusicalTime::ZERO),
            (
                64,
                MusicalTime::new(1, 8).expect("eighth note should be valid")
            ),
            (
                74,
                MusicalTime::new(1, 4).expect("quarter note should be valid")
            ),
        ]
    );
}

#[test]
fn compile_should_reject_degree_steps_outside_the_midi_range() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Invalid degree" {
  tempo 120bpm meter 4/4 key C major
  pattern phrase = steps 1/8 { degree 12 octave 10 }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("invalid degree should fail");

    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        ["degree and octave must resolve to a MIDI pitch from 0 to 127"]
    );
}

#[test]
fn schedule_should_choose_weighted_samples_deterministically() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 20260809 sample_rate 8khz output mono }
song "Choice" {
  tempo 120bpm meter 4/4 key C major
  instrument voice = sampler { pack "numbers" }
  pattern phrase = steps 1/8 {
    choose { sample 1 sample 3 weight 3 }
  }
  arrangement { phrase with voice phrase with voice }
}
"#,
    );
    let program = compile(&parsed.file).expect("weighted choices should compile");
    let first = schedule(&program).expect("weighted choices should schedule");
    let second = schedule(&program).expect("weighted choices should schedule again");
    let first_indices = first.songs[0]
        .tracks
        .iter()
        .map(|track| track.samples[0].index)
        .collect::<Vec<_>>();
    let second_indices = second.songs[0]
        .tracks
        .iter()
        .map(|track| track.samples[0].index)
        .collect::<Vec<_>>();

    assert_eq!(
        (
            first_indices.as_slice(),
            first_indices.iter().all(|index| [1, 3].contains(index))
        ),
        (second_indices.as_slice(), true)
    );
}

#[test]
fn schedule_should_choose_weighted_degrees_deterministically() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 20260809 sample_rate 8khz output mono }
song "Degree choice" {
  tempo 120bpm meter 4/4 key C major
  pattern phrase = steps 1/8 {
    choose {
      degree 12 octave 4 weight 1
      degree 11 octave 4 weight 3
    }
  }
  arrangement { phrase phrase }
}
"#,
    );
    let program = compile(&parsed.file).expect("weighted degrees should compile");
    let first = schedule(&program).expect("weighted degrees should schedule");
    let second = schedule(&program).expect("weighted degrees should schedule again");
    let first_pitches = first.songs[0]
        .tracks
        .iter()
        .map(|track| track.notes[0].midi_pitch)
        .collect::<Vec<_>>();
    let second_pitches = second.songs[0]
        .tracks
        .iter()
        .map(|track| track.notes[0].midi_pitch)
        .collect::<Vec<_>>();

    assert_eq!(
        (
            first_pitches.as_slice(),
            first_pitches.iter().all(|pitch| [71, 72].contains(pitch))
        ),
        (second_pitches.as_slice(), true)
    );
}

#[test]
fn schedule_should_expand_a_chosen_sample_sequence() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Choice sequence" {
  tempo 120bpm meter 4/4 key C major
  instrument voice = sampler { pack "numbers" }
  pattern phrase = steps 1/8 {
    choose { sequence weight 1 { sample 4 sample 7 } }
  }
  arrangement { phrase with voice }
}
"#,
    );
    let program = compile(&parsed.file).expect("choice sequences should compile");
    let score = schedule(&program).expect("choice sequences should schedule");
    let track = &score.songs[0].tracks[0];

    assert_eq!(
        track
            .samples
            .iter()
            .map(|sample| (sample.index, sample.start))
            .collect::<Vec<_>>(),
        [
            (4, MusicalTime::ZERO),
            (
                7,
                MusicalTime::new(1, 8).expect("eighth note should be valid")
            ),
        ]
    );
    assert_eq!(
        track.end,
        MusicalTime::new(1, 4).expect("quarter note should be valid")
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
fn compile_should_reject_invalid_sample_choices() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Invalid choices" {
  tempo 120bpm meter 4/4 key C major
  pattern phrase = steps 1/8 {
    choose {}
    choose { sample 1 weight 0 }
    choose { sequence weight 1 {} }
    choose { degree 0 octave 4 weight 0 }
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("invalid choices should fail");

    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        [
            "choose must contain at least one sample",
            "choice weight must be greater than zero",
            "choice sequence must contain at least one sample",
            "choice weight must be greater than zero",
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
