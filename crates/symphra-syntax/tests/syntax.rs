use symphra_syntax::ast::{Declaration, PatternBody, ProjectStatement, SongStatement};
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
    note G4 for 1/2
  }
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
    assert_eq!(value.value.value, 48.0);
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
    let PatternBody::Sequence { notes, .. } = &pattern.body;
    assert_eq!(notes.len(), 3);
    assert_eq!(notes[2].pitch.text, "G4");
    assert_eq!(
        (notes[2].duration_numerator, notes[2].duration_denominator),
        (1, 2)
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
    let PatternBody::Sequence { notes, .. } = &pattern.body;
    assert_eq!((notes.len(), notes[0].pitch.text.as_str()), (1, "E4"));
}
