use symphra_compiler::hir::{
    Arrangement, Channels, Duration, InstrumentKind, Key, Meter, Mode, NodeId, Note, Pattern,
    PatternOccurrence, PatternStep, PitchClass, Program, Project, Rhythm, RhythmItem, Song,
};
use symphra_compiler::{ScheduleError, compile, schedule};
use symphra_score::{Effect, MusicalTime, SampleSelector};
use symphra_syntax::{SourceId, parse};

fn pattern_occurrences(arrangement: &Arrangement) -> &[PatternOccurrence] {
    let Arrangement::Patterns(occurrences) = arrangement else {
        panic!("arrangement should be pattern occurrences");
    };
    occurrences
}

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
                sections: Vec::new(),
                arrangement: None,
                master: None,
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
fn compile_should_reject_chance_above_one_hundred_percent() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Chance" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody |> chance 101% { transpose +12st }
  }
}
"#,
    );
    let diagnostics = compile(&parsed.file).expect_err("out-of-range chance should fail");

    assert_eq!(diagnostics[0].message, "chance must be from 0% to 100%");
}

#[test]
fn schedule_should_apply_certain_chance_transpose() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Chance" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 note D4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody |> chance 100% { transpose +12st }
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("chance transpose should compile");
    let score = schedule(&program).expect("chance transpose should schedule");

    assert_eq!(
        score.songs[0].tracks[0]
            .notes
            .iter()
            .map(|note| note.midi_pitch)
            .collect::<Vec<_>>(),
        [72, 74]
    );
}

#[test]
fn schedule_should_apply_certain_chance_retrigger() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Sampler" {
  tempo 120bpm meter 4/4 key C major
  instrument voice = sampler { pack "numbers" }
  pattern phrase = steps 1/4 { sample 1 }
  track voice role melody {
    instrument voice
    play phrase |> chance 100% { retrigger 2 }
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("chance retrigger should compile");
    let score = schedule(&program).expect("chance retrigger should schedule");

    assert_eq!(
        score.songs[0].tracks[0]
            .samples
            .iter()
            .map(|event| (event.start, event.duration))
            .collect::<Vec<_>>(),
        [
            (
                MusicalTime::ZERO,
                MusicalTime::new(1, 8).expect("eighth note should be valid")
            ),
            (
                MusicalTime::new(1, 8).expect("eighth note should be valid"),
                MusicalTime::new(1, 8).expect("eighth note should be valid")
            ),
        ]
    );
}

#[test]
fn schedule_should_apply_certain_chance_speed_after_base_speed() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Sampler" {
  tempo 120bpm meter 4/4 key C major
  instrument voice = sampler { pack "numbers" }
  pattern phrase = steps 1/8 { sample 1 sample 3 }
  track voice role melody {
    instrument voice
    play phrase |> speed 1.2 |> chance 100% { speed 1.8 }
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("chance speed should compile");
    let score = schedule(&program).expect("chance speed should schedule");

    assert!(
        score.songs[0].tracks[0]
            .samples
            .iter()
            .all(|event| (event.speed - 1.8).abs() < f32::EPSILON)
    );
}

#[test]
fn compile_should_reject_chance_retrigger_below_two() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Sampler" {
  tempo 120bpm meter 4/4 key C major
  instrument voice = sampler { pack "numbers" }
  pattern phrase = steps 1/8 { sample 1 }
  track voice role melody {
    instrument voice
    play phrase |> chance 40% { retrigger 1 }
  }
}
"#,
    );
    let diagnostics = compile(&parsed.file).expect_err("retrigger below two should fail");

    assert_eq!(
        diagnostics[0].message,
        "chance retrigger count must be at least 2"
    );
}

#[test]
fn compile_should_reject_chance_retrigger_for_pitched_instruments() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Lead" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern phrase = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play phrase |> chance 40% { retrigger 2 }
  }
}
"#,
    );
    let diagnostics = compile(&parsed.file).expect_err("pitched retrigger should fail");

    assert_eq!(
        diagnostics[0].message,
        "chance retrigger is only supported for sampler or drum machine tracks"
    );
}

#[test]
fn compile_should_reject_chance_speed_for_pitched_instruments() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Lead" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern phrase = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play phrase |> chance 15% { speed 1.50 }
  }
}
"#,
    );
    let diagnostics = compile(&parsed.file).expect_err("pitched chance speed should fail");

    assert_eq!(
        diagnostics[0].message,
        "chance speed is only supported for sampler or drum machine tracks"
    );
}

#[test]
fn compile_should_reject_non_positive_chance_speed() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Sampler" {
  tempo 120bpm meter 4/4 key C major
  instrument voice = sampler { pack "numbers" }
  pattern phrase = steps 1/8 { sample 1 }
  track voice role melody {
    instrument voice
    play phrase |> chance 15% { speed 0 }
  }
}
"#,
    );
    let diagnostics = compile(&parsed.file).expect_err("zero chance speed should fail");

    assert_eq!(
        diagnostics[0].message,
        "chance speed must be finite and greater than zero"
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
fn schedule_should_resolve_bar_durations_against_the_song_meter() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Bars" {
  tempo 120bpm meter 3/4 key C major
  pattern phrase = sequence {
    note C4 for 1bar
    note E4 for 2bar
    rest for 1bar
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("bar durations should compile");

    let score = schedule(&program).expect("bar durations should schedule");
    let track = &score.songs[0].tracks[0];
    assert_eq!(
        (track.notes[0].start, track.notes[1].start, track.end),
        (
            MusicalTime::ZERO,
            MusicalTime::new(3, 4).expect("one bar of 3/4 should be a valid duration"),
            MusicalTime::new(3, 1).expect("four bars of 3/4 should be a valid duration"),
        )
    );
}

#[test]
fn compile_should_reject_zero_bar_duration() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "ZeroBar" {
  tempo 120bpm meter 4/4 key C major
  pattern phrase = sequence { note C4 for 0bar }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("zero bar duration should fail");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "note duration must be greater than zero"),
        "{diagnostics:#?}"
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
  pattern pitch_range = sequence { chord C-1 G9 for 1/4 }
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
    program.songs[0].arrangement = Some(Arrangement::Patterns(vec![PatternOccurrence {
        id: NodeId(99),
        pattern: NodeId(u32::MAX),
        instrument: InstrumentKind::Sine,
    }]));
    let unknown = schedule(&program);
    program.songs[0].arrangement = Some(Arrangement::Patterns(vec![
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
    ]));
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
                let Arrangement::Patterns(occurrences) = arrangement else {
                    panic!("arrangement should be pattern occurrences");
                };
                occurrences
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
fn schedule_should_mix_independently_scheduled_layers_into_one_track() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Layers" {
  tempo 120bpm meter 4/4 key C major
  instrument sub_sine = sine
  instrument sub_triangle = triangle
  pattern bass_roots = sequence { note C2 for 1/4 }
  track bass role low {
    volume -6db
    layer {
      use sub_sine { play bass_roots |> gain 1.0 }
      use sub_triangle { play bass_roots |> gain 0.5 }
    }
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("layered track should compile");

    let score = schedule(&program).expect("layered track should schedule");
    let tracks = &score.songs[0].tracks;

    assert_eq!(tracks.len(), 2, "each `use` should become its own track");
    assert_eq!(
        tracks
            .iter()
            .map(|track| (track.name.as_str(), track.instrument.clone()))
            .collect::<Vec<_>>(),
        vec![
            ("bass", symphra_score::InstrumentKind::Sine),
            ("bass", symphra_score::InstrumentKind::Triangle),
        ],
        "layers share the declared track name but keep their own instrument"
    );
    // Track-level `volume -6db` (~0.501x) applies uniformly; only the
    // per-layer `gain` differs.
    let volume = 10f32.powf(-6.0 / 20.0);
    assert!((tracks[0].gain - volume).abs() < 1e-6);
    assert!((tracks[1].gain - 0.5 * volume).abs() < 1e-6);
}

#[test]
fn schedule_should_apply_track_effect_delay() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Delay" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect delay { mix 0.40 time 1/4 feedback 0.25 }
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("track effect should compile");

    let score = schedule(&program).expect("track effect should schedule");
    let effect = score.songs[0].tracks[0]
        .effect
        .expect("effect should be scheduled");
    let Effect::Delay(effect) = effect else {
        panic!("effect should be a delay");
    };

    assert!((effect.mix - 0.40).abs() < 1e-6);
    assert!((effect.feedback - 0.25).abs() < 1e-6);
    assert_eq!(
        effect.time,
        MusicalTime::new(1, 4).expect("quarter note should be valid")
    );
}

#[test]
fn schedule_should_apply_track_effect_filter() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Filter" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect filter { cutoff 2000hz resonance 0.40 }
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("track effect should compile");

    let score = schedule(&program).expect("track effect should schedule");
    let effect = score.songs[0].tracks[0]
        .effect
        .expect("effect should be scheduled");
    let Effect::Filter(effect) = effect else {
        panic!("effect should be a filter");
    };

    assert!((effect.cutoff_hz - 2_000.0).abs() < 1e-3);
    assert!((effect.resonance - 0.40).abs() < 1e-6);
}

#[test]
fn schedule_should_apply_track_effect_filter_with_khz_cutoff() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Filter" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect filter { cutoff 2khz resonance 0.0 }
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("track effect should compile");

    let score = schedule(&program).expect("track effect should schedule");
    let effect = score.songs[0].tracks[0]
        .effect
        .expect("effect should be scheduled");
    let Effect::Filter(effect) = effect else {
        panic!("effect should be a filter");
    };

    assert!((effect.cutoff_hz - 2_000.0).abs() < 1e-3);
}

#[test]
fn schedule_should_apply_track_effect_reverb() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Reverb" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect reverb { mix 0.40 size 0.80 }
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("track effect should compile");

    let score = schedule(&program).expect("track effect should schedule");
    let effect = score.songs[0].tracks[0]
        .effect
        .expect("effect should be scheduled");
    let Effect::Reverb(effect) = effect else {
        panic!("effect should be a reverb");
    };

    assert!((effect.mix - 0.40).abs() < 1e-6);
    assert!((effect.size - 0.80).abs() < 1e-6);
}

#[test]
fn schedule_should_apply_filter_automation() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Automate" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect filter { cutoff 600hz resonance 0.40 }
    automate cutoff { lfo sine { range 600hz..2800hz rate 2 cycles/bar } }
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("track automate should compile");

    let score = schedule(&program).expect("track automate should schedule");
    let effect = score.songs[0].tracks[0]
        .effect
        .expect("effect should be scheduled");
    let Effect::Filter(effect) = effect else {
        panic!("effect should be a filter");
    };
    let automation = effect.automation.expect("automation should be scheduled");

    assert_eq!(automation.waveform, symphra_score::LfoWaveform::Sine);
    assert!((automation.range_start_hz - 600.0).abs() < 1e-3);
    assert!((automation.range_end_hz - 2_800.0).abs() < 1e-3);
    assert!((automation.cycles_per_bar - 2.0).abs() < 1e-6);
}

#[test]
fn compile_should_reject_out_of_range_effect_mix() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "InvalidMix" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect delay { mix 1.50 time 1/4 feedback 0.25 }
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("out-of-range mix should fail");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "effect mix must be from 0.0 to 1.0"),
        "{diagnostics:#?}"
    );
}

#[test]
fn compile_should_reject_out_of_range_effect_feedback() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "InvalidFeedback" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect delay { mix 0.40 time 1/4 feedback 1.0 }
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("out-of-range feedback should fail");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "effect feedback must be from 0.0 to 0.95"),
        "{diagnostics:#?}"
    );
}

#[test]
fn compile_should_reject_zero_effect_delay_time() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "ZeroDelay" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect delay { mix 0.40 time 0bar feedback 0.25 }
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("zero delay time should fail");

    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.message == "effect delay duration must be greater than zero"
        }),
        "{diagnostics:#?}"
    );
}

#[test]
fn compile_should_reject_out_of_range_effect_filter_resonance() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "InvalidResonance" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect filter { cutoff 2000hz resonance 1.50 }
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("out-of-range resonance should fail");

    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.message == "effect filter resonance must be from 0.0 to 1.0"
        }),
        "{diagnostics:#?}"
    );
}

#[test]
fn compile_should_reject_zero_effect_filter_cutoff() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "ZeroCutoff" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect filter { cutoff 0hz resonance 0.40 }
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("zero cutoff should fail");

    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.message == "effect filter cutoff must be greater than 0hz"
        }),
        "{diagnostics:#?}"
    );
}

#[test]
fn compile_should_reject_invalid_effect_filter_cutoff_unit() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "BadUnit" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect filter { cutoff 2000db resonance 0.40 }
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("invalid cutoff unit should fail");

    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.message == "effect filter cutoff unit must be `hz` or `khz`"
        }),
        "{diagnostics:#?}"
    );
}

#[test]
fn compile_should_reject_automate_without_effect() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "NoEffect" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    automate cutoff { lfo sine { range 600hz..2800hz rate 2 cycles/bar } }
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("automate without effect should fail");

    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("automate cutoff requires `effect filter`")),
        "{diagnostics:#?}"
    );
}

#[test]
fn compile_should_reject_automate_with_effect_delay() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "WrongEffect" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect delay { mix 0.40 time 1/4 feedback 0.25 }
    automate cutoff { lfo sine { range 600hz..2800hz rate 2 cycles/bar } }
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("automate with a delay effect should fail");

    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("automate cutoff requires `effect filter`")),
        "{diagnostics:#?}"
    );
}

#[test]
fn compile_should_reject_invalid_lfo_waveform() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "BadWaveform" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect filter { cutoff 600hz resonance 0.40 }
    automate cutoff { lfo square { range 600hz..2800hz rate 2 cycles/bar } }
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("invalid lfo waveform should fail");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "lfo waveform must be `sine` or `triangle`"),
        "{diagnostics:#?}"
    );
}

#[test]
fn compile_should_reject_invalid_automate_range_unit() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "BadRangeUnit" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect filter { cutoff 600hz resonance 0.40 }
    automate cutoff { lfo sine { range 600db..2800hz rate 2 cycles/bar } }
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("invalid range unit should fail");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "automate range unit must be `hz` or `khz`"),
        "{diagnostics:#?}"
    );
}

#[test]
fn compile_should_reject_zero_automate_range() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "ZeroRange" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect filter { cutoff 600hz resonance 0.40 }
    automate cutoff { lfo sine { range 0hz..2800hz rate 2 cycles/bar } }
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("zero range bound should fail");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "automate range must be greater than 0hz"),
        "{diagnostics:#?}"
    );
}

#[test]
fn compile_should_reject_zero_automate_rate() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "ZeroRate" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect filter { cutoff 600hz resonance 0.40 }
    automate cutoff { lfo sine { range 600hz..2800hz rate 0 cycles/bar } }
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("zero rate should fail");

    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.message == "automate rate must be greater than zero cycles per bar"
        }),
        "{diagnostics:#?}"
    );
}

#[test]
fn compile_should_reject_out_of_range_effect_reverb_mix() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "InvalidReverbMix" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect reverb { mix 1.50 size 0.80 }
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("out-of-range mix should fail");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "effect mix must be from 0.0 to 1.0"),
        "{diagnostics:#?}"
    );
}

#[test]
fn compile_should_reject_out_of_range_effect_reverb_size() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "InvalidReverbSize" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect reverb { mix 0.40 size 1.50 }
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("out-of-range size should fail");

    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.message == "effect reverb size must be from 0.0 to 1.0"
        }),
        "{diagnostics:#?}"
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
            "instrument kind must be `sine`, `triangle`, `sampled`, `sampler`, or `drum_machine`",
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
            .and_then(|arrangement| pattern_occurrences(arrangement).first())
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
            .and_then(|arrangement| pattern_occurrences(arrangement).first())
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
            .map(|event| (sample_index(&event.selector), event.start))
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

fn sample_index(selector: &SampleSelector) -> u32 {
    let SampleSelector::Index(index) = selector else {
        panic!("sample event should use an index selector");
    };
    *index
}

fn sample_name(selector: &SampleSelector) -> &str {
    let SampleSelector::Named(name) = selector else {
        panic!("sample event should use a named selector");
    };
    name
}

#[test]
fn schedule_should_create_sample_events_from_drum_steps() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Drums" {
  tempo 120bpm meter 4/4 key C major
  instrument tr909 = drum_machine { bank "RolandTR909" }
  pattern kit = steps 1/8 { drum "bd" rest drum "hh" }
  arrangement { kit with tr909 }
}
"#,
    );
    let program = compile(&parsed.file).expect("drum steps should compile");

    let score = schedule(&program).expect("drum steps should schedule");

    assert_eq!(
        score.songs[0].tracks[0]
            .samples
            .iter()
            .map(|event| (sample_name(&event.selector), event.start))
            .collect::<Vec<_>>(),
        vec![
            ("bd", MusicalTime::ZERO),
            (
                "hh",
                MusicalTime::new(1, 4).expect("quarter note should be valid")
            ),
        ]
    );
}

#[test]
fn schedule_should_apply_explicit_sample_and_drum_step_velocity() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "StepVelocity" {
  tempo 120bpm meter 4/4 key C major
  instrument tr909 = drum_machine { bank "RolandTR909" }
  pattern kit = steps 1/8 { drum "bd" velocity 90 drum "hh" }
  arrangement { kit with tr909 }
}
"#,
    );
    let program = compile(&parsed.file).expect("drum step velocity should compile");

    let score = schedule(&program).expect("drum step velocity should schedule");

    assert_eq!(
        score.songs[0].tracks[0]
            .samples
            .iter()
            .map(|event| (sample_name(&event.selector), event.velocity))
            .collect::<Vec<_>>(),
        vec![("bd", 90), ("hh", 127)]
    );
}

#[test]
fn compile_should_reject_out_of_range_sample_step_velocity() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "InvalidVelocity" {
  tempo 120bpm meter 4/4 key C major
  instrument voice_numbers = sampler { pack "numbers" }
  pattern phrase = steps 1/8 { sample 1 velocity 200 }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("out-of-range velocity should fail");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "velocity must be from 0 to 127"),
        "{diagnostics:#?}"
    );
}

#[test]
fn schedule_should_place_track_at_bar_beat() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "At" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    at 2:1 play melody
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("at placement should compile");

    let score = schedule(&program).expect("at placement should schedule");

    assert_eq!(
        score.songs[0].tracks[0].notes[0].start,
        MusicalTime::new(1, 1).expect("whole note should be valid")
    );
}

#[test]
fn schedule_should_apply_at_after_repeat_and_reverse() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "At" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 note D4 for 1/4 }
  track lead role melody {
    instrument lead
    at 1:2 play melody |> repeat 2 |> reverse
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("at with repeat/reverse should compile");

    let score = schedule(&program).expect("at with repeat/reverse should schedule");

    assert_eq!(
        score.songs[0].tracks[0]
            .notes
            .iter()
            .map(|note| (note.midi_pitch, note.start))
            .collect::<Vec<_>>(),
        [
            (
                62,
                MusicalTime::new(1, 4).expect("quarter note should be valid")
            ),
            (
                60,
                MusicalTime::new(1, 2).expect("half note should be valid")
            ),
            (
                62,
                MusicalTime::new(3, 4).expect("three quarter notes should be valid")
            ),
            (
                60,
                MusicalTime::new(1, 1).expect("whole note should be valid")
            ),
        ]
    );
}

#[test]
fn compile_should_reject_zero_bar_or_beat() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "At" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    at 0:1 play melody
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("zero bar should fail");

    assert_eq!(
        diagnostics[0].message,
        "`at` bar and beat are 1-indexed and must be at least 1"
    );
}

#[test]
fn compile_should_reject_beat_beyond_meter() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "At" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    at 1:5 play melody
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("beat beyond meter should fail");

    assert_eq!(
        diagnostics[0].message,
        "`at` beat must not exceed the song's meter numerator"
    );
}

#[test]
fn schedule_should_play_drum_with_rhythm_shorthand() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Drums" {
  tempo 120bpm meter 4/4 key C major
  instrument tr909 = drum_machine { bank "RolandTR909" }
  rhythm kick_pattern resolution 1/8 { hit rest hit rest }
  track drums role beat {
    instrument tr909
    play drum "bd" with kick_pattern
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("play drum with should compile");

    let score = schedule(&program).expect("play drum with should schedule");

    assert_eq!(
        score.songs[0].tracks[0]
            .samples
            .iter()
            .map(|event| (sample_name(&event.selector), event.start))
            .collect::<Vec<_>>(),
        vec![
            ("bd", MusicalTime::ZERO),
            (
                "bd",
                MusicalTime::new(1, 4).expect("quarter note should be valid")
            ),
        ]
    );
}

#[test]
fn compile_should_reject_empty_drum_voice_in_play_shorthand() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Drums" {
  tempo 120bpm meter 4/4 key C major
  instrument tr909 = drum_machine { bank "RolandTR909" }
  rhythm kick_pattern resolution 1/8 { hit }
  track drums role beat {
    instrument tr909
    play drum "" with kick_pattern
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("empty drum voice name should fail");

    assert_eq!(diagnostics[0].message, "drum voice name must not be empty");
}

#[test]
fn compile_should_reject_unknown_rhythm_in_play_drum_shorthand() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Drums" {
  tempo 120bpm meter 4/4 key C major
  instrument tr909 = drum_machine { bank "RolandTR909" }
  track drums role beat {
    instrument tr909
    play drum "bd" with missing_rhythm
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("unknown rhythm should fail");

    assert_eq!(
        diagnostics[0].message,
        "play drum with references an unknown rhythm"
    );
}

#[test]
fn compile_should_reject_play_drum_shorthand_for_non_drum_machine_instrument() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Drums" {
  tempo 120bpm meter 4/4 key C major
  instrument voice = sampler { pack "numbers" }
  rhythm kick_pattern resolution 1/8 { hit }
  track drums role beat {
    instrument voice
    play drum "bd" with kick_pattern
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("non-drum-machine instrument should fail");

    assert_eq!(
        diagnostics[0].message,
        "play drum requires a drum machine instrument"
    );
}

#[test]
fn compile_should_reject_play_drum_shorthand_combined_with_trigger_with() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Drums" {
  tempo 120bpm meter 4/4 key C major
  instrument tr909 = drum_machine { bank "RolandTR909" }
  rhythm kick_pattern resolution 1/8 { hit }
  rhythm other resolution 1/8 { hit rest }
  track drums role beat {
    instrument tr909
    play drum "bd" with kick_pattern |> trigger_with other
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("combined trigger_with should fail");

    assert_eq!(
        diagnostics[0].message,
        "`trigger_with` cannot be combined with `play drum ... with ...`"
    );
}

#[test]
fn compile_should_reject_play_drum_shorthand_with_empty_rhythm() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Drums" {
  tempo 120bpm meter 4/4 key C major
  instrument tr909 = drum_machine { bank "RolandTR909" }
  rhythm empty resolution 1/8 { }
  track drums role beat {
    instrument tr909
    play drum "bd" with empty
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("empty rhythm should fail");

    assert_eq!(
        diagnostics[0].message,
        "play drum with rhythm must contain at least one item"
    );
}

#[test]
fn schedule_should_trigger_drum_steps_with_rhythm() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Drums" {
  tempo 120bpm meter 4/4 key C major
  instrument tr909 = drum_machine { bank "RolandTR909" }
  rhythm off_beat resolution 1/8 { hit hit rest rest }
  pattern kit = steps 1/8 { drum "bd" drum "hh" drum "bd" drum "hh" }
  track drums role beat {
    instrument tr909
    play kit |> trigger_with off_beat
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("triggered drum steps should compile");

    let score = schedule(&program).expect("triggered drum steps should schedule");

    assert_eq!(
        score.songs[0].tracks[0]
            .samples
            .iter()
            .map(|event| (sample_name(&event.selector), event.start))
            .collect::<Vec<_>>(),
        vec![
            ("bd", MusicalTime::ZERO),
            (
                "hh",
                MusicalTime::new(1, 8).expect("eighth note should be valid")
            ),
        ]
    );
}

#[test]
fn schedule_should_trigger_degree_choice_steps_with_rhythm() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Melody" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  rhythm off_beat resolution 1/8 { hit rest }
  pattern phrase = steps 1/8 {
    choose { degree 0 octave 4 weight 1 }
    choose { degree 4 octave 4 weight 1 }
  }
  track lead role melody {
    instrument lead
    play phrase |> trigger_with off_beat
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("triggered degree choice should compile");

    let score = schedule(&program).expect("triggered degree choice should schedule");

    assert_eq!(
        score.songs[0].tracks[0]
            .notes
            .iter()
            .map(|note| (note.midi_pitch, note.start))
            .collect::<Vec<_>>(),
        [(60, MusicalTime::ZERO)]
    );
}

#[test]
fn schedule_should_trigger_single_sample_choice_steps_with_rhythm() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 20260809 sample_rate 8khz output mono }
song "Sampler" {
  tempo 120bpm meter 4/4 key C major
  instrument voice = sampler { pack "numbers" }
  rhythm off_beat resolution 1/8 { hit rest }
  pattern phrase = steps 1/8 { choose { sample 1 weight 1 sample 2 weight 1 } choose { sample 3 weight 1 sample 4 weight 1 } }
  track voice role melody {
    instrument voice
    play phrase |> trigger_with off_beat
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("triggered single-sample choice should compile");

    let score = schedule(&program).expect("triggered single-sample choice should schedule");

    let samples = &score.songs[0].tracks[0].samples;
    assert_eq!(samples.len(), 1);
    assert!(matches!(sample_index(&samples[0].selector), 1 | 2));
}

#[test]
fn schedule_should_trigger_single_drum_choice_steps_with_rhythm() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Drums" {
  tempo 120bpm meter 4/4 key C major
  instrument tr909 = drum_machine { bank "RolandTR909" }
  rhythm off_beat resolution 1/8 { hit rest }
  pattern kit = steps 1/8 { choose { drum "bd" weight 1 drum "sn" weight 1 } choose { drum "hh" weight 1 } }
  track drums role beat {
    instrument tr909
    play kit |> trigger_with off_beat
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("triggered single-drum choice should compile");

    let score = schedule(&program).expect("triggered single-drum choice should schedule");

    let samples = &score.songs[0].tracks[0].samples;
    assert_eq!(samples.len(), 1);
    assert!(matches!(sample_name(&samples[0].selector), "bd" | "sn"));
}

#[test]
fn compile_should_reject_triggered_multi_sample_choice_sequence() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Sampler" {
  tempo 120bpm meter 4/4 key C major
  instrument voice = sampler { pack "numbers" }
  rhythm off_beat resolution 1/8 { hit rest }
  pattern phrase = steps 1/8 { choose { sequence weight 1 { sample 1 sample 2 } } }
  track voice role melody {
    instrument voice
    play phrase |> trigger_with off_beat
  }
}
"#,
    );

    let diagnostics =
        compile(&parsed.file).expect_err("triggered multi-sample sequence should fail");

    assert_eq!(
        diagnostics[0].message,
        "trigger_with requires every choose alternative to select exactly one sample"
    );
}

#[test]
fn compile_should_reject_empty_drum_bank_name() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Empty bank" {
  tempo 120bpm meter 4/4 key C major
  instrument tr909 = drum_machine { bank "" }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("empty bank name should fail");

    assert_eq!(diagnostics[0].message, "drum bank name must not be empty");
}

#[test]
fn compile_should_reject_empty_drum_voice_name() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 48khz output stereo }
song "Empty voice" {
  tempo 120bpm meter 4/4 key C major
  pattern kit = steps 1/8 { drum "" }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("empty voice name should fail");

    assert_eq!(diagnostics[0].message, "drum voice name must not be empty");
}

#[test]
fn schedule_should_apply_sampler_playback_speed() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Sampler" {
  tempo 120bpm meter 4/4 key C major
  instrument voice = sampler { pack "numbers" }
  pattern phrase = steps 1/8 { sample 1 sample 3 }
  track voice role melody {
    instrument voice
    play phrase |> speed 1.5
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("sampler speed should compile");

    let score = schedule(&program).expect("sampler speed should schedule");

    assert!(
        score.songs[0].tracks[0]
            .samples
            .iter()
            .all(|event| (event.speed - 1.5).abs() < f32::EPSILON)
    );
}

#[test]
fn schedule_should_alternate_sampler_playback_speed() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Sampler" {
  tempo 120bpm meter 4/4 key C major
  instrument voice = sampler { pack "numbers" }
  pattern phrase = steps 1/8 { sample 1 sample 2 sample 3 }
  track voice role melody {
    instrument voice
    play phrase |> alternate { speed 1.5 speed 1.8 }
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("alternating speed should compile");

    let score = schedule(&program).expect("alternating speed should schedule");
    let speeds = score.songs[0].tracks[0]
        .samples
        .iter()
        .map(|event| event.speed);

    assert!(
        speeds
            .zip([1.5, 1.8, 1.5])
            .all(|(actual, expected)| (actual - expected).abs() < f32::EPSILON)
    );
}

#[test]
fn schedule_should_choose_sample_within_range() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 20260809 sample_rate 8khz output mono }
song "Sampler" {
  tempo 120bpm meter 4/4 key C major
  instrument voice = sampler { pack "numbers" }
  pattern phrase = steps 1/8 { sample 0 sample 0 sample 0 sample 0 }
  track voice role melody {
    instrument voice
    play phrase |> choose_sample 0..3
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("choose_sample should compile");

    let first = schedule(&program).expect("choose_sample should schedule");
    let second = schedule(&program).expect("choose_sample should schedule again");

    let first_indices = first.songs[0].tracks[0]
        .samples
        .iter()
        .map(|sample| sample_index(&sample.selector))
        .collect::<Vec<_>>();
    let second_indices = second.songs[0].tracks[0]
        .samples
        .iter()
        .map(|sample| sample_index(&sample.selector))
        .collect::<Vec<_>>();

    assert_eq!(first_indices, second_indices);
    assert!(first_indices.iter().all(|index| (0..=3).contains(index)));
}

#[test]
fn compile_should_reject_backwards_choose_sample_range() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Sampler" {
  tempo 120bpm meter 4/4 key C major
  instrument voice = sampler { pack "numbers" }
  pattern phrase = steps 1/8 { sample 0 }
  track voice role melody {
    instrument voice
    play phrase |> choose_sample 3..0
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("backwards range should fail");

    assert_eq!(
        diagnostics[0].message,
        "choose_sample range must not be empty"
    );
}

#[test]
fn compile_should_reject_choose_sample_for_pitched_instruments() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Lead" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern phrase = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play phrase |> choose_sample 0..3
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("pitched choose_sample should fail");

    assert_eq!(
        diagnostics[0].message,
        "choose_sample is only supported for sampler tracks"
    );
}

#[test]
fn compile_should_reject_speed_for_pitched_instruments() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Lead" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern phrase = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play phrase |> speed 1.5
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("pitched speed should fail");

    assert_eq!(
        diagnostics[0].message,
        "speed is only supported for sampler or drum machine tracks"
    );
}

#[test]
fn compile_should_reject_non_positive_speed() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Sampler" {
  tempo 120bpm meter 4/4 key C major
  instrument voice = sampler { pack "numbers" }
  pattern phrase = steps 1/8 { sample 1 }
  track voice role melody {
    instrument voice
    play phrase |> speed 0
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("zero speed should fail");

    assert_eq!(
        diagnostics[0].message,
        "speed must be finite and greater than zero"
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
        .map(|track| sample_index(&track.samples[0].selector))
        .collect::<Vec<_>>();
    let second_indices = second.songs[0]
        .tracks
        .iter()
        .map(|track| sample_index(&track.samples[0].selector))
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
            .map(|sample| (sample_index(&sample.selector), sample.start))
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

const SECTIONS_EXAMPLE: &str = r#"
project { seed 1 sample_rate 8khz output mono }
song "Sections" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern chords = sequence { note C4 for 1bar }
  pattern bassline = sequence { note C2 for 1bar }
  track chords role harmony { instrument lead play chords }
  track bass role low { instrument lead play bassline }
  section phrase bars 1 {
    parallel exact {
      play track chords
      play track bass
    }
  }
  arrangement { play phrase play phrase }
}
"#;

#[test]
fn schedule_should_place_sectioned_tracks_at_cumulative_bar_offsets() {
    let parsed = parse(SourceId(0), SECTIONS_EXAMPLE);
    let program = compile(&parsed.file).expect("sections example should compile");

    let score = schedule(&program).expect("sections example should schedule");
    let tracks = &score.songs[0].tracks;
    let one_bar = MusicalTime::new(1, 1).expect("one bar in 4/4 should be a whole note");

    assert_eq!(
        tracks
            .iter()
            .map(|track| track.notes[0].start)
            .collect::<Vec<_>>(),
        [MusicalTime::ZERO, MusicalTime::ZERO, one_bar, one_bar]
    );
}

#[test]
fn schedule_should_namespace_entity_ids_per_reused_section_occurrence() {
    let parsed = parse(SourceId(0), SECTIONS_EXAMPLE);
    let program = compile(&parsed.file).expect("sections example should compile");

    let score = schedule(&program).expect("sections example should schedule");
    let tracks = &score.songs[0].tracks;

    // tracks[0]/[2] are both the "chords" track, once per `play phrase`
    // occurrence; a section referenced twice must get independent entity IDs
    // (and track IDs) per occurrence, not a verbatim duplicate.
    assert_ne!(tracks[0].id, tracks[2].id);
    assert_ne!(tracks[0].notes[0].id, tracks[2].notes[0].id);
    assert_ne!(tracks[1].id, tracks[3].id);
}

#[test]
fn schedule_should_reject_exact_parallel_length_mismatch() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Sections" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern chords = sequence { note C4 for 2bar }
  track chords role harmony { instrument lead play chords }
  section phrase bars 1 {
    parallel exact {
      play track chords
    }
  }
  arrangement { play phrase }
}
"#,
    );
    let program = compile(&parsed.file).expect("sections example should compile");

    let result = schedule(&program);

    assert!(matches!(
        result,
        Err(ScheduleError::SectionTrackLengthMismatch { .. })
    ));
}

#[test]
fn schedule_should_allow_non_exact_parallel_length_mismatch() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Sections" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern chords = sequence { note C4 for 2bar }
  pattern bassline = sequence { note C2 for 1bar }
  track chords role harmony { instrument lead play chords }
  track bass role low { instrument lead play bassline }
  section short bars 1 {
    parallel {
      play track chords
    }
  }
  section next bars 1 {
    parallel exact {
      play track bass
    }
  }
  arrangement { play short play next }
}
"#,
    );
    let program = compile(&parsed.file).expect("sections example should compile");

    let score = schedule(&program).expect("non-exact overflow should still schedule");

    // The overlong "chords" track is allowed to spill past its own
    // section's 1-bar window; the next section's offset is still only the
    // cumulative `bars` value (1 bar), not the actual scheduled length.
    assert_eq!(
        score.songs[0].tracks[1].notes[0].start,
        MusicalTime::new(1, 1).expect("one bar in 4/4 should be a whole note")
    );
}

#[test]
fn compile_should_reject_arrangement_play_referencing_unknown_section() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Sections" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern chords = sequence { note C4 for 1bar }
  track chords role harmony { instrument lead play chords }
  section phrase bars 1 {
    parallel exact { play track chords }
  }
  arrangement { play missing }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("unknown section should fail");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "arrangement references an unknown section")
    );
}

#[test]
fn compile_should_reject_section_referencing_unknown_track() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Sections" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern chords = sequence { note C4 for 1bar }
  track chords role harmony { instrument lead play chords }
  section phrase bars 1 {
    parallel exact { play track missing }
  }
  arrangement { play phrase }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("unknown track should fail");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "section references an unknown track")
    );
}

#[test]
fn compile_should_reject_zero_bar_section() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Sections" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern chords = sequence { note C4 for 1bar }
  track chords role harmony { instrument lead play chords }
  section phrase bars 0 {
    parallel exact { play track chords }
  }
  arrangement { play phrase }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("zero-bar section should fail");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message
                == "section bars duration must be greater than zero")
    );
}

#[test]
fn compile_should_reject_declared_tracks_combined_with_legacy_pattern_arrangement() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Sections" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern chords = sequence { note C4 for 1bar }
  track chords role harmony { instrument lead play chords }
  arrangement { chords }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("mixed arrangement forms should fail");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.message == "track declarations cannot be combined with a pattern arrangement"
    }));
}

#[test]
fn compile_should_reject_arrangement_of_sections_without_declared_tracks() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Sections" {
  tempo 120bpm meter 4/4 key C major
  arrangement { play phrase }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("sectionless arrangement should fail");

    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.message
            == "an arrangement of sections requires declared tracks")
    );
}

#[test]
fn compile_should_lower_master_limiter_ceiling_to_linear_amplitude() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Master" {
  tempo 120bpm meter 4/4 key C major
  pattern melody = sequence { note C4 for 1/4 }
  arrangement { melody }
  master {
    limiter { ceiling -0.3db }
  }
}
"#,
    );
    let program = compile(&parsed.file).expect("master limiter should compile");

    let score = schedule(&program).expect("master limiter should schedule");

    let ceiling = score.songs[0]
        .master
        .expect("master should be scheduled")
        .ceiling;
    assert!((ceiling - 10.0_f32.powf(-0.3 / 20.0)).abs() < 1e-6);
}

#[test]
fn compile_should_reject_master_ceiling_with_non_decibel_unit() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Master" {
  tempo 120bpm meter 4/4 key C major
  master {
    limiter { ceiling -0.3percent }
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("non-db ceiling unit should fail");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "ceiling unit must be `db`")
    );
}

#[test]
fn compile_should_reject_positive_master_ceiling() {
    let parsed = parse(
        SourceId(0),
        r#"
project { seed 1 sample_rate 8khz output mono }
song "Master" {
  tempo 120bpm meter 4/4 key C major
  master {
    limiter { ceiling 0.5db }
  }
}
"#,
    );

    let diagnostics = compile(&parsed.file).expect_err("positive ceiling should fail");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "ceiling must be at most 0db")
    );
}
