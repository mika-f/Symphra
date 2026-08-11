use symphra_syntax::ast::{
    ArrangementEntry, Declaration, DurationExpression, EffectKind, InstrumentBody, PatternBody,
    ProjectStatement, RhythmItem, SequenceItem, SongStatement, StepItem,
};
use symphra_syntax::{DiagnosticKind, SourceId, TokenKind, lex, parse};

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
    rest for 1/4
    note G4 for 1/2
  }
  arrangement { melody }
}
"#;

#[test]
fn lexes_units_comments_and_strings() {
    let lexed = lex(SourceId(7), "48khz // comment\n\"song\"");
    assert!(lexed.diagnostics.is_empty());
    assert_eq!(
        lexed
            .tokens
            .iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>(),
        [
            TokenKind::Integer,
            TokenKind::Identifier,
            TokenKind::String,
            TokenKind::Eof
        ]
    );
    assert_eq!(lexed.tokens[0].span.source, SourceId(7));
}

#[test]
fn lexes_sharp_pitches_without_consuming_comments() {
    let lexed = lex(SourceId(0), "C#4 C# comment\nDb4 C-1 C#-1 Cb-1");

    assert_eq!(
        lexed
            .tokens
            .iter()
            .map(|token| (token.kind, token.text.as_str()))
            .collect::<Vec<_>>(),
        [
            (TokenKind::Identifier, "C#4"),
            (TokenKind::Identifier, "C"),
            (TokenKind::Identifier, "Db4"),
            (TokenKind::Identifier, "C-1"),
            (TokenKind::Identifier, "C#-1"),
            (TokenKind::Identifier, "Cb-1"),
            (TokenKind::Eof, ""),
        ]
    );
}

#[test]
fn parses_the_draft_example() {
    let parsed = parse(SourceId(0), EXAMPLE);
    assert_eq!(parsed.diagnostics, []);
    assert_eq!(parsed.file.declarations.len(), 2);

    let Declaration::Project(project) = &parsed.file.declarations[0] else {
        panic!("first declaration should be a project");
    };
    assert!(matches!(
        project.statements[0],
        ProjectStatement::Seed {
            value: 20_260_809,
            ..
        }
    ));
    let ProjectStatement::SampleRate { value, .. } = &project.statements[1] else {
        panic!("second project statement should be sample_rate");
    };
    assert!((value.value.value - 48.0).abs() < f64::EPSILON);
    assert_eq!(value.unit.text, "khz");

    let Declaration::Song(song) = &parsed.file.declarations[1] else {
        panic!("second declaration should be a song");
    };
    assert_eq!(song.name.value, "First Song");
    assert!(matches!(
        song.statements[1],
        SongStatement::Meter {
            numerator: 4,
            denominator: 4,
            ..
        }
    ));
    let SongStatement::Pattern(pattern) = &song.statements[3] else {
        panic!("fourth song statement should be a pattern");
    };
    assert_eq!(pattern.name.text, "melody");
    let PatternBody::Sequence { items, .. } = &pattern.body else {
        panic!("pattern should be a sequence");
    };
    assert_eq!(items.len(), 4);
    let SequenceItem::Note(note) = &items[3] else {
        panic!("fourth sequence item should be a note");
    };
    assert_eq!(note.pitch.text, "G4");
    let Some(DurationExpression::Fraction {
        numerator,
        denominator,
        ..
    }) = note.duration
    else {
        panic!("note duration should be a fraction");
    };
    assert_eq!((numerator, denominator), (1, 2));
    let SongStatement::Arrangement { entries, .. } = &song.statements[4] else {
        panic!("fifth song statement should be an arrangement");
    };
    let ArrangementEntry::Pattern(occurrence) = &entries[0] else {
        panic!("first arrangement entry should be a bare pattern reference");
    };
    assert_eq!(occurrence.pattern.text, "melody");
}

#[test]
fn parses_instrument_assignments_in_arrangements() {
    let parsed = parse(
        SourceId(0),
        r#"song "Instruments" {
  instrument lead = triangle
  pattern melody = sequence {}
  arrangement { melody with lead melody }
}"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Instrument(instrument) = &song.statements[0] else {
        panic!("first statement should be an instrument");
    };
    let InstrumentBody::Oscillator { waveform: kind, .. } = &instrument.body else {
        panic!("instrument should be built in");
    };
    let SongStatement::Arrangement { entries, .. } = &song.statements[2] else {
        panic!("third statement should be an arrangement");
    };
    let ArrangementEntry::Pattern(first) = &entries[0] else {
        panic!("first arrangement entry should be a bare pattern reference");
    };
    let ArrangementEntry::Pattern(second) = &entries[1] else {
        panic!("second arrangement entry should be a bare pattern reference");
    };

    assert_eq!(
        (
            instrument.name.text.as_str(),
            kind.text.as_str(),
            first.pattern.text.as_str(),
            first
                .instrument
                .as_ref()
                .map(|instrument| instrument.text.as_str()),
            second.instrument.as_ref(),
        ),
        ("lead", "triangle", "melody", Some("lead"), None)
    );
}

#[test]
fn parses_sampled_instruments() {
    let parsed = parse(
        SourceId(0),
        r#"song "Sample" {
  instrument piano = sampled { source "samples/piano-c4.wav" root C4 }
}"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Instrument(instrument) = &song.statements[0] else {
        panic!("statement should be an instrument");
    };
    let InstrumentBody::Sampled { source, root, .. } = &instrument.body else {
        panic!("instrument should be sampled");
    };

    assert_eq!(
        (source.value.as_str(), root.text.as_str()),
        ("samples/piano-c4.wav", "C4")
    );
}

#[test]
fn parses_sampler_packs() {
    let parsed = parse(
        SourceId(0),
        r#"song "Sampler" {
  instrument voice_numbers = sampler { pack "numbers" }
}"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Instrument(instrument) = &song.statements[0] else {
        panic!("statement should be an instrument");
    };
    let InstrumentBody::Sampler { pack, .. } = &instrument.body else {
        panic!("instrument should be a sampler");
    };

    assert_eq!(pack.value, "numbers");
}

#[test]
fn parses_drum_machine_instruments() {
    let parsed = parse(
        SourceId(0),
        r#"song "Drums" {
  instrument tr909 = drum_machine { bank "RolandTR909" }
}"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Instrument(instrument) = &song.statements[0] else {
        panic!("statement should be an instrument");
    };
    let InstrumentBody::DrumMachine { bank, .. } = &instrument.body else {
        panic!("instrument should be a drum machine");
    };

    assert_eq!(bank.value, "RolandTR909");
}

#[test]
fn parses_soundfont_instruments() {
    let parsed = parse(
        SourceId(0),
        r#"song "SoundFont" {
  instrument music_box = soundfont { source "instruments/gm.sf2" preset "gm_music_box" }
}"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Instrument(instrument) = &song.statements[0] else {
        panic!("statement should be an instrument");
    };
    let InstrumentBody::SoundFont { source, preset, .. } = &instrument.body else {
        panic!("instrument should be a soundfont");
    };

    assert_eq!(
        (source.value.as_str(), preset.value.as_str()),
        ("instruments/gm.sf2", "gm_music_box")
    );
}

#[test]
fn parses_vst3_instruments_with_preset() {
    let parsed = parse(
        SourceId(0),
        r#"song "Vst3" {
  instrument lead = vst3 { source "instruments/synth.vst3" preset "Warm Pad" }
}"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Instrument(instrument) = &song.statements[0] else {
        panic!("statement should be an instrument");
    };
    let InstrumentBody::Vst3 { source, preset, .. } = &instrument.body else {
        panic!("instrument should be a vst3");
    };

    assert_eq!(source.value.as_str(), "instruments/synth.vst3");
    assert_eq!(
        preset.as_ref().map(|preset| preset.value.as_str()),
        Some("Warm Pad")
    );
}

#[test]
fn parses_vst3_instruments_without_preset() {
    let parsed = parse(
        SourceId(0),
        r#"song "Vst3" {
  instrument lead = vst3 { source "instruments/synth.vst3" }
}"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Instrument(instrument) = &song.statements[0] else {
        panic!("statement should be an instrument");
    };
    let InstrumentBody::Vst3 { source, preset, .. } = &instrument.body else {
        panic!("instrument should be a vst3");
    };

    assert_eq!(source.value.as_str(), "instruments/synth.vst3");
    assert!(preset.is_none());
}

#[test]
fn parses_oscillator_instruments_with_envelope() {
    let parsed = parse(
        SourceId(0),
        r#"song "Envelope" {
  instrument lead = sine { envelope { attack 4ms decay 200ms sustain 0.50 release 150ms } }
}"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Instrument(instrument) = &song.statements[0] else {
        panic!("statement should be an instrument");
    };
    let InstrumentBody::Oscillator {
        waveform,
        envelope: Some(envelope),
        ..
    } = &instrument.body
    else {
        panic!("instrument should be an oscillator with an envelope");
    };

    assert_eq!(waveform.text, "sine");
    assert_eq!(
        (
            envelope.attack.value.value,
            envelope.attack.unit.text.as_str(),
            envelope.decay.value.value,
            envelope.sustain.value,
            envelope.release.value.value,
        ),
        (4.0, "ms", 200.0, 0.50, 150.0)
    );
}

#[test]
fn parses_supersaw_instruments() {
    let parsed = parse(
        SourceId(0),
        r#"song "Supersaw" {
  instrument chord_saw = synth supersaw { voices 5 detune 0.35 spread 0.80 }
}"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Instrument(instrument) = &song.statements[0] else {
        panic!("statement should be an instrument");
    };
    let InstrumentBody::Supersaw {
        voices,
        detune,
        spread,
        envelope,
        ..
    } = &instrument.body
    else {
        panic!("instrument should be a supersaw");
    };

    assert_eq!(
        (*voices, detune.value, spread.value, envelope.is_none()),
        (5, 0.35, 0.80, true)
    );
}

#[test]
fn parses_drum_steps() {
    let parsed = parse(
        SourceId(0),
        r#"song "Drums" {
  pattern kit = steps 1/8 { drum "bd" rest drum "hh" }
}"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Pattern(pattern) = &song.statements[0] else {
        panic!("statement should be a pattern");
    };
    let PatternBody::Steps { items, .. } = &pattern.body else {
        panic!("pattern should contain steps");
    };

    let names = items
        .iter()
        .map(|item| match item {
            StepItem::Drum { name, .. } => Some(name.value.as_str()),
            StepItem::Rest { .. } => None,
            StepItem::Degree { .. } => panic!("unexpected degree"),
            StepItem::Sample { .. } => panic!("unexpected sample"),
            StepItem::Choose { .. } => panic!("unexpected choice"),
            StepItem::ChooseDegrees { .. } => panic!("unexpected degree choice"),
            StepItem::Repeat(_) => panic!("unexpected repetition"),
            StepItem::Subdivide { .. } => panic!("unexpected subdivision"),
        })
        .collect::<Vec<_>>();
    assert_eq!(names, vec![Some("bd"), None, Some("hh")]);
}

#[test]
fn parses_sample_steps() {
    let parsed = parse(
        SourceId(0),
        r#"song "Samples" {
  pattern phrase = steps 1/8 { sample 1 rest sample 3 }
}"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Pattern(pattern) = &song.statements[0] else {
        panic!("statement should be a pattern");
    };
    let PatternBody::Steps {
        resolution, items, ..
    } = &pattern.body
    else {
        panic!("pattern should contain steps");
    };

    let indices = items
        .iter()
        .map(|item| match item {
            StepItem::Sample { index, .. } => Some(*index),
            StepItem::Rest { .. } => None,
            StepItem::Degree { .. } => panic!("unexpected degree"),
            StepItem::Drum { .. } => panic!("unexpected drum"),
            StepItem::Choose { .. } => panic!("unexpected choice"),
            StepItem::ChooseDegrees { .. } => panic!("unexpected degree choice"),
            StepItem::Repeat(_) => panic!("unexpected repetition"),
            StepItem::Subdivide { .. } => panic!("unexpected subdivision"),
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        resolution,
        DurationExpression::Fraction {
            numerator: 1,
            denominator: 8,
            ..
        }
    ));
    assert_eq!(indices, vec![Some(1), None, Some(3)]);
}

#[test]
fn parses_drum_and_sample_step_velocity() {
    let parsed = parse(
        SourceId(0),
        r#"song "Velocity" {
  pattern kit = steps 1/8 { drum "bd" velocity 90 sample 2 velocity 40 rest }
}"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Pattern(pattern) = &song.statements[0] else {
        panic!("statement should be a pattern");
    };
    let PatternBody::Steps { items, .. } = &pattern.body else {
        panic!("pattern should contain steps");
    };

    let velocities = items
        .iter()
        .map(|item| match item {
            StepItem::Drum { velocity, .. } | StepItem::Sample { velocity, .. } => {
                velocity.map(|velocity| velocity.value)
            }
            StepItem::Rest { .. } => None,
            StepItem::Degree { .. } => panic!("unexpected degree"),
            StepItem::Choose { .. } => panic!("unexpected choice"),
            StepItem::ChooseDegrees { .. } => panic!("unexpected degree choice"),
            StepItem::Repeat(_) => panic!("unexpected repetition"),
            StepItem::Subdivide { .. } => panic!("unexpected subdivision"),
        })
        .collect::<Vec<_>>();
    assert_eq!(velocities, vec![Some(90), Some(40), None]);
}

#[test]
fn parses_degree_steps() {
    let parsed = parse(
        SourceId(0),
        r#"song "Degree" {
  pattern phrase = steps 1/8 { degree 2 octave 5 degree 12 octave 5 }
}"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Pattern(pattern) = &song.statements[0] else {
        panic!("statement should be a pattern");
    };
    let PatternBody::Steps { items, .. } = &pattern.body else {
        panic!("pattern should contain steps");
    };

    assert_eq!(
        items
            .iter()
            .map(|item| match item {
                StepItem::Degree { degree, octave, .. } => (*degree, *octave),
                _ => panic!("step should be a degree"),
            })
            .collect::<Vec<_>>(),
        [(2, 5), (12, 5)]
    );
}

#[test]
fn parses_weighted_sample_choices() {
    let parsed = parse(
        SourceId(0),
        r#"song "Choice" {
  pattern phrase = steps 1/8 {
    choose { sample 1 sequence weight 2 { sample 3 sample 5 } }
  }
}"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Pattern(pattern) = &song.statements[0] else {
        panic!("statement should be a pattern");
    };
    let PatternBody::Steps { items, .. } = &pattern.body else {
        panic!("pattern should contain steps");
    };
    let StepItem::Choose { alternatives, .. } = &items[0] else {
        panic!("step should be a choice");
    };

    assert_eq!(
        alternatives
            .iter()
            .map(|alternative| (
                alternative
                    .selectors
                    .iter()
                    .map(|selector| match selector {
                        symphra_syntax::ast::SampleSelectorExpression::Index(index) => *index,
                        symphra_syntax::ast::SampleSelectorExpression::Named(_) =>
                            panic!("unexpected drum selector"),
                    })
                    .collect::<Vec<_>>(),
                alternative.weight
            ))
            .collect::<Vec<_>>(),
        [(vec![1], 1), (vec![3, 5], 2)]
    );
}

#[test]
fn parses_drum_choice_alternatives() {
    let parsed = parse(
        SourceId(0),
        r#"song "Choice" {
  pattern kit = steps 1/8 {
    choose { drum "bd" weight 1 sequence weight 2 { drum "hh" sample 3 } }
  }
}"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Pattern(pattern) = &song.statements[0] else {
        panic!("statement should be a pattern");
    };
    let PatternBody::Steps { items, .. } = &pattern.body else {
        panic!("pattern should contain steps");
    };
    let StepItem::Choose { alternatives, .. } = &items[0] else {
        panic!("step should be a choice");
    };

    let describe = |selector: &symphra_syntax::ast::SampleSelectorExpression| match selector {
        symphra_syntax::ast::SampleSelectorExpression::Index(index) => index.to_string(),
        symphra_syntax::ast::SampleSelectorExpression::Named(name) => name.value.clone(),
    };
    assert_eq!(
        alternatives
            .iter()
            .map(|alternative| (
                alternative
                    .selectors
                    .iter()
                    .map(describe)
                    .collect::<Vec<_>>(),
                alternative.weight
            ))
            .collect::<Vec<_>>(),
        [
            (vec!["bd".to_owned()], 1),
            (vec!["hh".to_owned(), "3".to_owned()], 2)
        ]
    );
}

#[test]
fn parses_weighted_degree_choices() {
    let parsed = parse(
        SourceId(0),
        r#"song "Choice" {
  pattern phrase = steps 1/8 {
    choose {
      degree 12 octave 5 weight 1
      degree 11 octave 5 weight 3
    }
  }
}"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Pattern(pattern) = &song.statements[0] else {
        panic!("statement should be a pattern");
    };
    let PatternBody::Steps { items, .. } = &pattern.body else {
        panic!("pattern should contain steps");
    };
    let StepItem::ChooseDegrees { alternatives, .. } = &items[0] else {
        panic!("step should be a degree choice");
    };

    assert_eq!(
        alternatives
            .iter()
            .map(|alternative| (alternative.degree, alternative.octave, alternative.weight,))
            .collect::<Vec<_>>(),
        [(12, 5, 1), (11, 5, 3)]
    );
}

#[test]
fn parses_reusable_rhythms() {
    let parsed = parse(
        SourceId(0),
        "song \"Rhythm\" { rhythm pulse resolution 1/8 { hit rest hit } }",
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Rhythm(rhythm) = &song.statements[0] else {
        panic!("statement should be a rhythm");
    };

    assert_eq!(
        (
            rhythm.name.text.as_str(),
            rhythm.resolution_numerator,
            rhythm.resolution_denominator,
            rhythm
                .items
                .iter()
                .map(|item| matches!(item, RhythmItem::Hit { .. }))
                .collect::<Vec<_>>(),
        ),
        ("pulse", 1, 8, vec![true, false, true])
    );
}

#[test]
fn parses_tracks_with_rhythm_triggers() {
    let parsed = parse(
        SourceId(0),
        concat!(
            "song \"Track\" { ",
            "track chords role harmony { ",
            "instrument lead volume -5.2db play progression |> trigger_with stabs |> gate 95% ",
            "|> transpose +12st |> gain 0.30 |> repeat 4 |> reverse ",
            "|> chance 15% { transpose +12st } |> speed 1.50 ",
            "|> pan alternate(30%, 70%) } }",
        ),
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Track(track) = &song.statements[0] else {
        panic!("statement should be a track");
    };
    let (instrument, play) = single_layer(&track.body);

    assert_eq!(
        (
            track.name.text.as_str(),
            track.role.text.as_str(),
            instrument.text.as_str(),
            track
                .volume
                .as_ref()
                .map(|volume| (volume.decibels, volume.unit.text.as_str())),
            pattern_source_name(&play.source),
            play.trigger_with.as_ref().map(|name| name.text.as_str()),
            play.gate.map(|gate| gate.percent),
            play.transpose
                .as_ref()
                .map(|transpose| (transpose.semitones, transpose.unit.text.as_str())),
            play.gain.map(|gain| gain.factor),
            play.repeat.map(|repeat| repeat.count),
            play.reverse,
            play.pan,
        ),
        (
            "chords",
            "harmony",
            "lead",
            Some((-5.2, "db")),
            "progression",
            Some("stabs"),
            Some(95),
            Some((12, "st")),
            Some(0.30),
            Some(4),
            true,
            Some(symphra_syntax::ast::PanExpression::Alternate {
                left_percent: 30,
                right_percent: 70,
                span: play.pan.expect("pan should be present").span(),
            })
        )
    );
    assert_eq!(
        (
            play.speed.and_then(|speed| match speed {
                symphra_syntax::ast::SpeedExpression::Fixed { factor, .. } => Some(factor),
                symphra_syntax::ast::SpeedExpression::Alternate { .. } => None,
            }),
            play.chance.as_ref().map(|chance| {
                let symphra_syntax::ast::ChanceTransformExpression::Transpose(transpose) =
                    &chance.transform
                else {
                    panic!("chance transform should be a transpose");
                };
                (
                    chance.percent,
                    transpose.semitones,
                    transpose.unit.text.as_str(),
                )
            }),
        ),
        (Some(1.50), Some((15, 12, "st")))
    );
}

#[test]
fn parses_track_with_layer_uses() {
    let parsed = parse(
        SourceId(0),
        concat!(
            "song \"Track\" { ",
            "track bass role low { ",
            "volume -3db ",
            "layer { ",
            "use sub_sine { play bass_roots |> gain 1.0 } ",
            "use sub_triangle { play bass_roots |> gain 0.6 |> pan -20% } ",
            "} } }",
        ),
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Track(track) = &song.statements[0] else {
        panic!("statement should be a track");
    };
    assert_eq!(
        track
            .volume
            .as_ref()
            .map(|volume| (volume.decibels, volume.unit.text.as_str())),
        Some((-3.0, "db"))
    );
    let symphra_syntax::ast::TrackBody::Layers { uses, .. } = &track.body else {
        panic!("track should be a layer body");
    };
    assert_eq!(
        uses.iter()
            .map(|layer_use| (
                layer_use.instrument.text.as_str(),
                pattern_source_name(&layer_use.play.source),
                layer_use.play.gain.map(|gain| gain.factor),
            ))
            .collect::<Vec<_>>(),
        vec![
            ("sub_sine", "bass_roots", Some(1.0)),
            ("sub_triangle", "bass_roots", Some(0.6)),
        ]
    );
}

#[test]
fn parses_track_effect_delay() {
    let parsed = parse(
        SourceId(0),
        concat!(
            "song \"Track\" { ",
            "track drums role beat { ",
            "instrument tr909 play kit ",
            "effect delay { mix 0.40 time 1/4 feedback 0.25 } ",
            "} }",
        ),
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Track(track) = &song.statements[0] else {
        panic!("statement should be a track");
    };
    let effect = track.effect.as_ref().expect("effect should be present");
    let EffectKind::Delay {
        mix,
        time,
        feedback,
    } = &effect.kind
    else {
        panic!("effect should be a delay");
    };

    assert!((mix.value - 0.40).abs() < f32::EPSILON);
    assert!((feedback.value - 0.25).abs() < f32::EPSILON);
    let DurationExpression::Fraction {
        numerator,
        denominator,
        ..
    } = *time
    else {
        panic!("effect delay time should be a fraction");
    };
    assert_eq!((numerator, denominator), (1, 4));
}

#[test]
fn parses_track_effect_filter() {
    let parsed = parse(
        SourceId(0),
        concat!(
            "song \"Track\" { ",
            "track drums role beat { ",
            "instrument tr909 play kit ",
            "effect filter { cutoff 2000hz resonance 0.40 } ",
            "} }",
        ),
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Track(track) = &song.statements[0] else {
        panic!("statement should be a track");
    };
    let effect = track.effect.as_ref().expect("effect should be present");
    let EffectKind::Filter { cutoff, resonance } = &effect.kind else {
        panic!("effect should be a filter");
    };

    assert!((cutoff.value.value - 2000.0).abs() < f64::EPSILON);
    assert_eq!(cutoff.unit.text, "hz");
    assert!((resonance.value - 0.40).abs() < f32::EPSILON);
}

#[test]
fn parses_track_effect_reverb() {
    let parsed = parse(
        SourceId(0),
        concat!(
            "song \"Track\" { ",
            "track drums role beat { ",
            "instrument tr909 play kit ",
            "effect reverb { mix 0.40 size 0.80 } ",
            "} }",
        ),
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Track(track) = &song.statements[0] else {
        panic!("statement should be a track");
    };
    let effect = track.effect.as_ref().expect("effect should be present");
    let EffectKind::Reverb { mix, size } = &effect.kind else {
        panic!("effect should be a reverb");
    };

    assert!((mix.value - 0.40).abs() < f32::EPSILON);
    assert!((size.value - 0.80).abs() < f32::EPSILON);
}

#[test]
fn parses_track_automate_cutoff() {
    let parsed = parse(
        SourceId(0),
        concat!(
            "song \"Track\" { ",
            "track drums role beat { ",
            "instrument tr909 play kit ",
            "effect filter { cutoff 600hz resonance 0.40 } ",
            "automate cutoff { lfo sine { range 600hz..2800hz rate 2 cycles/bar } } ",
            "} }",
        ),
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Track(track) = &song.statements[0] else {
        panic!("statement should be a track");
    };
    let automate = track.automate.as_ref().expect("automate should be present");

    assert_eq!(automate.lfo.waveform.text, "sine");
    assert!((automate.lfo.range_start.value.value - 600.0).abs() < f64::EPSILON);
    assert_eq!(automate.lfo.range_start.unit.text, "hz");
    assert!((automate.lfo.range_end.value.value - 2_800.0).abs() < f64::EPSILON);
    assert!((automate.lfo.rate.value - 2.0).abs() < f64::EPSILON);
}

/// Unwraps a track's single-instrument body, panicking if it is a `layer`.
fn single_layer(
    body: &symphra_syntax::ast::TrackBody,
) -> (
    &symphra_syntax::ast::Identifier,
    &symphra_syntax::ast::PlayStatement,
) {
    let symphra_syntax::ast::TrackBody::Single { instrument, play } = body else {
        panic!("track should be a single-instrument body");
    };
    (instrument, play)
}

fn pattern_source_name(source: &symphra_syntax::ast::PlaySource) -> &str {
    let symphra_syntax::ast::PlaySource::Pattern(identifier) = source else {
        panic!("play source should be a pattern");
    };
    identifier.text.as_str()
}

#[test]
fn parses_play_drum_with_rhythm_shorthand() {
    let parsed = parse(
        SourceId(0),
        "song \"Track\" { track drums role beat { instrument tr909 play drum \"bd\" with kick_pattern } }",
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Track(track) = &song.statements[0] else {
        panic!("statement should be a track");
    };
    let (_, play) = single_layer(&track.body);
    let symphra_syntax::ast::PlaySource::Drum { name, rhythm, .. } = &play.source else {
        panic!("play source should be a drum shorthand");
    };

    assert_eq!(
        (name.value.as_str(), rhythm.text.as_str()),
        ("bd", "kick_pattern")
    );
}

#[test]
fn parses_at_bar_beat_placement() {
    let parsed = parse(
        SourceId(0),
        "song \"Track\" { track crash role fx { instrument tr909 at 1:1 play drum \"cr\" with one_hit } }",
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Track(track) = &song.statements[0] else {
        panic!("statement should be a track");
    };
    let (_, play) = single_layer(&track.body);
    let at = play.at.expect("at position should be present");

    assert_eq!((at.bar, at.beat), (1, 1));
}

#[test]
fn parses_alternating_sampler_speed() {
    let parsed = parse(
        SourceId(0),
        "song \"Track\" { track voice role melody { instrument voice play phrase |> alternate { speed 1.50 speed 1.80 } } }",
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Track(track) = &song.statements[0] else {
        panic!("statement should be a track");
    };

    let (_, play) = single_layer(&track.body);
    assert!(matches!(
        play.speed,
        Some(symphra_syntax::ast::SpeedExpression::Alternate {
            first_factor: 1.5,
            second_factor: 1.8,
            ..
        })
    ));
}

#[test]
fn parses_chance_retrigger() {
    let parsed = parse(
        SourceId(0),
        "song \"Track\" { track voice role melody { instrument voice play phrase |> chance 40% { retrigger 2 } } }",
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Track(track) = &song.statements[0] else {
        panic!("statement should be a track");
    };
    let (_, play) = single_layer(&track.body);
    let chance = play.chance.as_ref().expect("chance should be present");

    assert_eq!(chance.percent, 40);
    assert!(matches!(
        chance.transform,
        symphra_syntax::ast::ChanceTransformExpression::Retrigger { count: 2, .. }
    ));
}

#[test]
fn parses_chance_speed() {
    let parsed = parse(
        SourceId(0),
        "song \"Track\" { track voice role melody { instrument voice play phrase |> chance 15% { speed 1.50 } } }",
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Track(track) = &song.statements[0] else {
        panic!("statement should be a track");
    };
    let (_, play) = single_layer(&track.body);
    let chance = play.chance.as_ref().expect("chance should be present");

    assert_eq!(chance.percent, 15);
    assert!(matches!(
        chance.transform,
        symphra_syntax::ast::ChanceTransformExpression::Speed { factor, .. } if (factor - 1.5).abs() < f32::EPSILON
    ));
}

#[test]
fn parses_choose_sample_range() {
    let parsed = parse(
        SourceId(0),
        "song \"Track\" { track voice role melody { instrument voice play phrase |> choose_sample 0..3 } }",
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Track(track) = &song.statements[0] else {
        panic!("statement should be a track");
    };
    let (_, play) = single_layer(&track.body);
    let choose_sample = play.choose_sample.expect("choose_sample should be present");

    assert_eq!((choose_sample.start, choose_sample.end), (0, 3));
}

#[test]
fn parses_chord_pitches_and_duration() {
    let parsed = parse(
        SourceId(0),
        "song \"Chord\" { pattern harmony = sequence { chord C4 E4 G4 for 1/2 velocity 96 } }",
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Pattern(pattern) = &song.statements[0] else {
        panic!("statement should be a pattern");
    };
    let PatternBody::Sequence { items, .. } = &pattern.body else {
        panic!("pattern should be a sequence");
    };
    let SequenceItem::Chord(chord) = &items[0] else {
        panic!("sequence item should be a chord");
    };
    let Some(DurationExpression::Fraction {
        numerator,
        denominator,
        ..
    }) = chord.duration
    else {
        panic!("chord duration should be a fraction");
    };
    assert_eq!(
        (
            chord
                .pitches
                .iter()
                .map(|pitch| pitch.text.as_str())
                .collect::<Vec<_>>(),
            numerator,
            denominator,
            chord.velocity.map(|velocity| velocity.value),
        ),
        (vec!["C4", "E4", "G4"], 1, 2, Some(96))
    );
}

#[test]
fn parses_bar_durations_for_notes_chords_and_rests() {
    let parsed = parse(
        SourceId(0),
        r#"song "Bars" {
  pattern phrase = sequence {
    note C4 for 1bar
    chord C4 E4 G4 for 2bar velocity 64
    rest for 1bar
  }
}"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Pattern(pattern) = &song.statements[0] else {
        panic!("statement should be a pattern");
    };
    let PatternBody::Sequence { items, .. } = &pattern.body else {
        panic!("pattern should be a sequence");
    };

    let SequenceItem::Note(note) = &items[0] else {
        panic!("first item should be a note");
    };
    let Some(DurationExpression::Bars { count, .. }) = note.duration else {
        panic!("note duration should be bars");
    };
    assert_eq!(count, 1);

    let SequenceItem::Chord(chord) = &items[1] else {
        panic!("second item should be a chord");
    };
    let Some(DurationExpression::Bars { count, .. }) = chord.duration else {
        panic!("chord duration should be bars");
    };
    assert_eq!(count, 2);

    let SequenceItem::Rest(rest) = &items[2] else {
        panic!("third item should be a rest");
    };
    let Some(DurationExpression::Bars { count, .. }) = rest.duration else {
        panic!("rest duration should be bars");
    };
    assert_eq!(count, 1);
}

#[test]
fn reports_lexical_and_syntax_errors_without_panicking() {
    let parsed = parse(SourceId(0), "@ project { seed nope } song \"unfinished");
    assert!(
        parsed
            .diagnostics
            .iter()
            .any(|error| error.kind == DiagnosticKind::Lexical)
    );
    assert!(
        parsed
            .diagnostics
            .iter()
            .any(|error| error.kind == DiagnosticKind::Syntax)
    );
}

#[test]
fn recovers_at_the_next_statement_or_note() {
    let parsed = parse(
        SourceId(0),
        r#"
project {
  seed nope junk
  sample_rate 48khz
  output stereo
}
song "Recovery" {
  tempo nope junk
  tempo 120bpm
  meter 4/4
  key C major
  pattern melody = sequence {
    note C4 for nope junk
    note E4 for 1/4
  }
}
"#,
    );

    assert_eq!(
        parsed
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        [
            "expected an integer seed",
            "expected a number",
            "expected duration numerator",
        ]
    );
    let Declaration::Project(project) = &parsed.file.declarations[0] else {
        panic!("first declaration should remain a project");
    };
    assert_eq!(project.statements.len(), 2);
    let Declaration::Song(song) = &parsed.file.declarations[1] else {
        panic!("second declaration should remain a song");
    };
    let SongStatement::Pattern(pattern) = &song.statements[3] else {
        panic!("valid pattern should remain in the song");
    };
    let PatternBody::Sequence { items, .. } = &pattern.body else {
        panic!("pattern should be a sequence");
    };
    let SequenceItem::Note(note) = &items[0] else {
        panic!("recovered sequence item should be a note");
    };
    assert_eq!((items.len(), note.pitch.text.as_str()), (1, "E4"));
}

#[test]
fn parses_section_with_exact_parallel_block() {
    let parsed = parse(
        SourceId(0),
        concat!(
            "song \"Sections\" { ",
            "section phrase bars 4 { ",
            "parallel exact { play track chords play track bass } ",
            "} }",
        ),
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Section(section) = &song.statements[0] else {
        panic!("statement should be a section");
    };
    assert_eq!(section.name.text, "phrase");
    assert_eq!(section.bars, 4);
    assert!(section.exact);
    assert_eq!(
        section
            .tracks
            .iter()
            .map(|track| track.text.as_str())
            .collect::<Vec<_>>(),
        vec!["chords", "bass"]
    );
}

#[test]
fn parses_section_without_exact_parallel_block() {
    let parsed = parse(
        SourceId(0),
        "song \"Sections\" { section intro bars 2 { parallel { play track pad } } }",
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Section(section) = &song.statements[0] else {
        panic!("statement should be a section");
    };
    assert!(!section.exact);
    assert_eq!(section.tracks.len(), 1);
}

#[test]
fn parses_arrangement_play_section_entries() {
    let parsed = parse(
        SourceId(0),
        "song \"Sections\" { arrangement { play phrase play phrase } }",
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Arrangement { entries, .. } = &song.statements[0] else {
        panic!("statement should be an arrangement");
    };
    assert_eq!(entries.len(), 2);
    for entry in entries {
        let ArrangementEntry::Play { name, .. } = entry else {
            panic!("entry should be a `play <name>` section reference");
        };
        assert_eq!(name.text, "phrase");
    }
}

#[test]
fn parses_arrangement_legacy_pattern_entries_unchanged() {
    let parsed = parse(
        SourceId(0),
        "song \"Legacy\" { pattern melody = sequence {} arrangement { melody with lead } }",
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Arrangement { entries, .. } = &song.statements[1] else {
        panic!("statement should be an arrangement");
    };
    let ArrangementEntry::Pattern(occurrence) = &entries[0] else {
        panic!("entry should be a bare pattern reference");
    };
    assert_eq!(
        (
            occurrence.pattern.text.as_str(),
            occurrence
                .instrument
                .as_ref()
                .map(|instrument| instrument.text.as_str())
        ),
        ("melody", Some("lead"))
    );
}

#[test]
fn parses_master_limiter_ceiling() {
    let parsed = parse(
        SourceId(0),
        "song \"S\" { master { limiter { ceiling -0.3db } } }",
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Master(master) = &song.statements[0] else {
        panic!("statement should be a master block");
    };
    assert_eq!(
        (master.ceiling.decibels, master.ceiling.unit.text.as_str()),
        (-0.3, "db")
    );
}

/// `* N` repetition sugar is kept in the AST rather than expanded here, so
/// the formatter can reprint what the author wrote; the compiler expands it
/// during lowering.
#[test]
fn parses_a_repeated_step_item() {
    let parsed = parse(
        SourceId(0),
        r#"song "S" { pattern kit = steps 1/8 { drum "hh" velocity 38 * 4 rest } }"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let items = step_items(&parsed);
    assert_eq!(items.len(), 2);
    let StepItem::Repeat(group) = &items[0] else {
        panic!("first item should be a repetition");
    };
    assert_eq!(group.count, 4);
    let [StepItem::Drum { name, velocity, .. }] = group.items.as_slice() else {
        panic!("repetition should hold one drum item");
    };
    assert_eq!(name.value, "hh");
    assert_eq!(velocity.map(|velocity| velocity.value), Some(38));
    assert!(matches!(items[1], StepItem::Rest { .. }));
}

#[test]
fn parses_a_repetition_group_of_step_items() {
    let parsed = parse(
        SourceId(0),
        r#"song "S" { pattern kit = steps 1/8 { (drum "hh", rest) * 2 } }"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let items = step_items(&parsed);
    let [StepItem::Repeat(group)] = items else {
        panic!("body should be one repetition");
    };
    assert_eq!(group.count, 2);
    assert_eq!(group.items.len(), 2);
    assert!(matches!(group.items[0], StepItem::Drum { .. }));
    assert!(matches!(group.items[1], StepItem::Rest { .. }));
}

#[test]
fn parses_nested_repetition_groups() {
    let parsed = parse(
        SourceId(0),
        r#"song "S" { pattern kit = steps 1/8 { (rest * 2, drum "hh") * 3 } }"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let items = step_items(&parsed);
    let [StepItem::Repeat(outer)] = items else {
        panic!("body should be one repetition");
    };
    assert_eq!(outer.count, 3);
    let StepItem::Repeat(inner) = &outer.items[0] else {
        panic!("first element should itself be a repetition");
    };
    assert_eq!(inner.count, 2);
}

#[test]
fn parses_repeated_rhythm_cells() {
    let parsed = parse(
        SourceId(0),
        "song \"S\" { rhythm pulse resolution 1/8 { hit rest * 3 (hit, rest) * 2 } }",
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Rhythm(rhythm) = &song.statements[0] else {
        panic!("statement should be a rhythm");
    };
    assert!(matches!(rhythm.items[0], RhythmItem::Hit { .. }));
    let RhythmItem::Repeat(rests) = &rhythm.items[1] else {
        panic!("second item should be a repetition");
    };
    assert_eq!(rests.count, 3);
    let RhythmItem::Repeat(group) = &rhythm.items[2] else {
        panic!("third item should be a repetition group");
    };
    assert_eq!((group.count, group.items.len()), (2, 2));
}

#[test]
fn parses_a_repeated_sequence_item() {
    let parsed = parse(
        SourceId(0),
        "song \"S\" { pattern melody = sequence { note C4 for 1/4 * 2 } }",
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Pattern(pattern) = &song.statements[0] else {
        panic!("statement should be a pattern");
    };
    let PatternBody::Sequence { items, .. } = &pattern.body else {
        panic!("pattern should contain a sequence");
    };
    let [SequenceItem::Repeat(group)] = items.as_slice() else {
        panic!("body should be one repetition");
    };
    assert_eq!(group.count, 2);
    assert!(matches!(group.items[0], SequenceItem::Note(_)));
}

#[test]
fn rejects_malformed_repetitions() {
    for (source, message) in [
        (
            r#"song "S" { pattern kit = steps 1/8 { drum "hh" * 0 } }"#,
            "repetition count must be at least 1",
        ),
        (
            r#"song "S" { pattern kit = steps 1/8 { drum "hh" * } }"#,
            "expected a repetition count after `*`",
        ),
        (
            r#"song "S" { pattern kit = steps 1/8 { (drum "hh", rest) } }"#,
            "expected `*` after a repetition group",
        ),
        (
            r#"song "S" { pattern kit = steps 1/8 { () * 2 } }"#,
            "a repetition group must contain at least one item",
        ),
        (
            r#"song "S" { pattern kit = steps 1/8 { choose { sample 0 weight 1 } * 2 } }"#,
            "`choose` cannot be repeated with `*`",
        ),
    ] {
        let parsed = parse(SourceId(0), source);
        assert_eq!(
            parsed
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_str())
                .collect::<Vec<_>>(),
            [message],
            "unexpected diagnostics for {source}"
        );
    }
}

fn step_items(parsed: &symphra_syntax::ParsedSource) -> &[StepItem] {
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Pattern(pattern) = &song.statements[0] else {
        panic!("statement should be a pattern");
    };
    let PatternBody::Steps { items, .. } = &pattern.body else {
        panic!("pattern should contain steps");
    };
    items
}

#[test]
fn parses_a_velocity_ramp() {
    let parsed = parse(
        SourceId(0),
        r#"song "S" { pattern kit = steps 1/16 { drum "cp" velocity 70..110 * 4 } }"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let [StepItem::Repeat(group)] = step_items(&parsed) else {
        panic!("body should be one repetition");
    };
    let [StepItem::Drum { velocity, .. }] = group.items.as_slice() else {
        panic!("repetition should hold one drum item");
    };
    let velocity = velocity.expect("drum should carry a velocity");
    assert_eq!((velocity.value, velocity.ramp_to), (70, Some(110)));
}

#[test]
fn rejects_a_velocity_ramp_without_an_end() {
    let parsed = parse(
        SourceId(0),
        r#"song "S" { pattern kit = steps 1/16 { drum "cp" velocity 70.. * 4 } }"#,
    );
    assert_eq!(
        parsed
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        ["expected the end of a velocity ramp"]
    );
}

#[test]
fn parses_a_bar_step_resolution_and_subdivisions() {
    let parsed = parse(
        SourceId(0),
        r#"song "S" { pattern kit = steps 1bar { [ drum "cp" * 2 ] [ drum "bd" [ drum "sn" rest ] ] } }"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Pattern(pattern) = &song.statements[0] else {
        panic!("statement should be a pattern");
    };
    let PatternBody::Steps {
        resolution, items, ..
    } = &pattern.body
    else {
        panic!("pattern should contain steps");
    };
    assert!(matches!(
        resolution,
        DurationExpression::Bars { count: 1, .. }
    ));

    let [
        StepItem::Subdivide { items: first, .. },
        StepItem::Subdivide { items: second, .. },
    ] = items.as_slice()
    else {
        panic!("both cells should be subdivisions");
    };
    assert!(matches!(first.as_slice(), [StepItem::Repeat(_)]));
    assert!(matches!(
        second.as_slice(),
        [StepItem::Drum { .. }, StepItem::Subdivide { .. }]
    ));
}

#[test]
fn rejects_an_empty_subdivision() {
    let parsed = parse(
        SourceId(0),
        r#"song "S" { pattern kit = steps 1/8 { [] } }"#,
    );
    assert_eq!(
        parsed
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        ["a subdivision must contain at least one item"]
    );
}

#[test]
fn parses_a_sequence_step_default_duration() {
    let parsed = parse(
        SourceId(0),
        r#"song "S" { pattern line = sequence step 1/8 { note C4  note D4 for 1/16  rest } }"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    let Declaration::Song(song) = &parsed.file.declarations[0] else {
        panic!("declaration should be a song");
    };
    let SongStatement::Pattern(pattern) = &song.statements[0] else {
        panic!("statement should be a pattern");
    };
    let PatternBody::Sequence { step, items, .. } = &pattern.body else {
        panic!("pattern should contain a sequence");
    };
    assert!(matches!(
        step,
        Some(DurationExpression::Fraction {
            numerator: 1,
            denominator: 8,
            ..
        })
    ));

    let [
        SequenceItem::Note(first),
        SequenceItem::Note(second),
        SequenceItem::Rest(rest),
    ] = items.as_slice()
    else {
        panic!("body should be two notes and a rest");
    };
    assert!(first.duration.is_none());
    assert!(matches!(
        second.duration,
        Some(DurationExpression::Fraction {
            denominator: 16,
            ..
        })
    ));
    assert!(rest.duration.is_none());
}

#[test]
fn rejects_a_missing_duration_without_a_sequence_step() {
    let parsed = parse(
        SourceId(0),
        r#"song "S" { pattern line = sequence { note C4 } }"#,
    );
    assert_eq!(
        parsed
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        ["expected `for` after note pitch"]
    );
}
