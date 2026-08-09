use symphra_syntax::ast::{
    Declaration, InstrumentBody, PatternBody, ProjectStatement, SequenceItem, SongStatement,
    StepItem,
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
    assert_eq!((note.duration_numerator, note.duration_denominator), (1, 2));
    let SongStatement::Arrangement { occurrences, .. } = &song.statements[4] else {
        panic!("fifth song statement should be an arrangement");
    };
    assert_eq!(occurrences[0].pattern.text, "melody");
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
    let InstrumentBody::Builtin(kind) = &instrument.body else {
        panic!("instrument should be built in");
    };
    let SongStatement::Arrangement { occurrences, .. } = &song.statements[2] else {
        panic!("third statement should be an arrangement");
    };

    assert_eq!(
        (
            instrument.name.text.as_str(),
            kind.text.as_str(),
            occurrences[0].pattern.text.as_str(),
            occurrences[0]
                .instrument
                .as_ref()
                .map(|instrument| instrument.text.as_str()),
            occurrences[1].instrument.as_ref(),
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
        resolution_numerator,
        resolution_denominator,
        items,
        ..
    } = &pattern.body
    else {
        panic!("pattern should contain steps");
    };

    let indices = items
        .iter()
        .map(|item| match item {
            StepItem::Sample { index, .. } => Some(*index),
            StepItem::Rest { .. } => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        (*resolution_numerator, *resolution_denominator, indices),
        (1, 8, vec![Some(1), None, Some(3)])
    );
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
    assert_eq!(
        (
            chord
                .pitches
                .iter()
                .map(|pitch| pitch.text.as_str())
                .collect::<Vec<_>>(),
            chord.duration_numerator,
            chord.duration_denominator,
            chord.velocity.map(|velocity| velocity.value),
        ),
        (vec!["C4", "E4", "G4"], 1, 2, Some(96))
    );
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
