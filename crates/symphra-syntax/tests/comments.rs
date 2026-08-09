use symphra_syntax::{SourceId, lex};

#[test]
fn captures_shell_style_comments_as_trivia() {
    let lexed = lex(SourceId(0), "seed 1 # a comment\noutput mono");

    assert_eq!(lexed.comments.len(), 1);
    assert_eq!(lexed.comments[0].text, "# a comment");
    assert_eq!(lexed.comments[0].span.range(), 7..18);
}

#[test]
fn captures_slash_style_comments_as_trivia() {
    let lexed = lex(SourceId(0), "seed 1 // a comment\noutput mono");

    assert_eq!(lexed.comments.len(), 1);
    assert_eq!(lexed.comments[0].text, "// a comment");
}

#[test]
fn does_not_mistake_a_sharp_pitch_for_a_comment() {
    let lexed = lex(SourceId(0), "chord C#4 Eb4 G4 for 1/4 # trailing");

    assert_eq!(lexed.comments.len(), 1);
    assert_eq!(lexed.comments[0].text, "# trailing");
}

#[test]
fn captures_multiple_comments_in_source_order() {
    let lexed = lex(
        SourceId(0),
        "# first\nseed 1 // second\noutput mono\n# third",
    );

    assert_eq!(
        lexed
            .comments
            .iter()
            .map(|comment| comment.text.as_str())
            .collect::<Vec<_>>(),
        ["# first", "// second", "# third"]
    );
}
