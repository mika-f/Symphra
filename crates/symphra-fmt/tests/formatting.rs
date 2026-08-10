use symphra_fmt::format_source;

fn format(source: &str) -> String {
    format_source(source).unwrap_or_else(|error| panic!("expected valid source: {error}"))
}

#[test]
fn expands_a_compact_project_and_song_into_canonical_layout() {
    let input = r#"project { seed 1 sample_rate 48khz output stereo }
song "Arranged" {
  tempo 120bpm meter 4/4 key C major
  pattern intro = sequence { note C4 for 1/4 }
  pattern outro = sequence { note G4 for 1/2 }
  arrangement { outro intro }
}
"#;

    let expected = r#"project {
  seed 1
  sample_rate 48khz
  output stereo
}

song "Arranged" {
  tempo 120bpm
  meter 4/4
  key C major
  pattern intro = sequence {
    note C4 for 1/4
  }
  pattern outro = sequence {
    note G4 for 1/2
  }
  arrangement {
    outro
    intro
  }
}
"#;

    assert_eq!(format(input), expected);
}

#[test]
fn reorders_project_and_song_settings_into_canonical_order() {
    let input = r#"song "S" {
  arrangement { p }
  key C major
  pattern p = sequence { note C4 for 1/4 }
  meter 4/4
  tempo 120bpm
}
project { output mono sample_rate 8khz seed 1 }
"#;

    let expected = r#"project {
  seed 1
  sample_rate 8khz
  output mono
}

song "S" {
  tempo 120bpm
  meter 4/4
  key C major
  pattern p = sequence {
    note C4 for 1/4
  }
  arrangement {
    p
  }
}
"#;

    assert_eq!(format(input), expected);
}

#[test]
fn preserves_leading_and_trailing_comments() {
    let input = r#"project {
  # about the seed
  seed 1 // deterministic
  sample_rate 8khz
  output mono
  # dangling before close
}
song "S" {
  tempo 120bpm meter 4/4 key C major
  pattern p = sequence { note C4 for 1/4 }
}
"#;

    let output = format(input);
    assert!(output.contains("  # about the seed\n  seed 1 // deterministic\n"));
    assert!(output.contains("  # dangling before close\n}"));
}

#[test]
fn collapses_more_than_one_blank_line_to_a_single_blank_line() {
    let input = r#"project { seed 1 sample_rate 8khz output mono }
song "S" {
  tempo 120bpm meter 4/4 key C major
  pattern a = sequence { note C4 for 1/4 }



  pattern b = sequence { note D4 for 1/4 }
}
"#;

    let output = format(input);
    assert!(output.contains("}\n\n  pattern b"));
    assert!(!output.contains("\n\n\n"));
}

#[test]
fn normalizes_rate_whitespace_and_redundant_digits() {
    let input = r#"project { seed 007 sample_rate 48 khz output mono }
song "S" {
  tempo 120 bpm meter 04/4 key C major
  pattern p = sequence { note C4 for 1/4 }
}
"#;

    let output = format(input);
    assert!(output.contains("seed 7\n"));
    assert!(output.contains("sample_rate 48khz\n"));
    assert!(output.contains("tempo 120bpm\n"));
    assert!(output.contains("meter 4/4\n"));
}

#[test]
fn formats_instruments_rhythm_track_and_pipeline_stages() {
    let input = r#"project { seed 1 sample_rate 8khz output mono }
song "S" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  instrument piano = sampled { source "piano.wav" root C4 }
  instrument voice = sampler { pack "numbers" }
  rhythm stabs resolution 1/4 { hit rest hit rest }
  pattern harmony = sequence { chord C4 E4 G4 for 1/1 }
  track chords role harmony {
    instrument lead
    volume -6 db
    play harmony |> trigger_with stabs |> gate 80% |> transpose +2 st |> gain 0.8 |> repeat 2 |> reverse |> chance 15% { transpose +12st } |> speed 1.5 |> pan alternate(30%,70%)
  }
}
"#;

    let expected = r#"project {
  seed 1
  sample_rate 8khz
  output mono
}

song "S" {
  tempo 120bpm
  meter 4/4
  key C major
  instrument lead = sine
  instrument piano = sampled {
    source "piano.wav"
    root C4
  }
  instrument voice = sampler {
    pack "numbers"
  }
  rhythm stabs resolution 1/4 {
    hit
    rest
    hit
    rest
  }
  pattern harmony = sequence {
    chord C4 E4 G4 for 1/1
  }
  track chords role harmony {
    instrument lead
    volume -6 db
    play harmony |> trigger_with stabs |> gate 80% |> transpose 2 st |> gain 0.8 |> repeat 2 |> reverse |> chance 15% { transpose 12 st } |> speed 1.5 |> pan alternate(30%, 70%)
  }
}
"#;

    assert_eq!(format(input), expected);
}

#[test]
fn formats_alternating_sampler_speed() {
    let input = r#"song "S" {
  track voice role melody {
    instrument voice
    play phrase |> alternate { speed 1.50 speed 1.80 }
  }
}
"#;

    let expected = r#"song "S" {
  track voice role melody {
    instrument voice
    play phrase |> alternate { speed 1.5 speed 1.8 }
  }
}
"#;

    assert_eq!(format(input), expected);
}

#[test]
fn formats_chance_retrigger() {
    let input = r#"song "S" {
  track voice role melody {
    instrument voice
    play phrase |> chance 40% { retrigger 2 }
  }
}
"#;

    let expected = r#"song "S" {
  track voice role melody {
    instrument voice
    play phrase |> chance 40% { retrigger 2 }
  }
}
"#;

    assert_eq!(format(input), expected);
}

#[test]
fn formats_chance_speed() {
    let input = r#"song "S" {
  track voice role melody {
    instrument voice
    play phrase |> chance 15% { speed 1.50 }
  }
}
"#;

    let expected = r#"song "S" {
  track voice role melody {
    instrument voice
    play phrase |> chance 15% { speed 1.5 }
  }
}
"#;

    assert_eq!(format(input), expected);
}

#[test]
fn formats_choose_sample_range() {
    let input = r#"song "S" {
  track voice role melody {
    instrument voice
    play phrase |> choose_sample 0..3
  }
}
"#;

    let expected = r#"song "S" {
  track voice role melody {
    instrument voice
    play phrase |> choose_sample 0..3
  }
}
"#;

    assert_eq!(format(input), expected);
}

#[test]
fn formats_play_drum_with_rhythm_shorthand() {
    let input = r#"song "S" {
  track drums role beat {
    instrument tr909
    play drum "bd" with kick_pattern
  }
}
"#;

    let expected = r#"song "S" {
  track drums role beat {
    instrument tr909
    play drum "bd" with kick_pattern
  }
}
"#;

    assert_eq!(format(input), expected);
}

#[test]
fn formats_drum_machine_instrument_and_drum_steps() {
    let input = r#"song "S" {
  instrument tr909 = drum_machine { bank "RolandTR909" }
  pattern kit = steps 1/8 { drum "bd" rest drum "hh" }
}
"#;

    let expected = r#"song "S" {
  instrument tr909 = drum_machine {
    bank "RolandTR909"
  }
  pattern kit = steps 1/8 {
    drum "bd"
    rest
    drum "hh"
  }
}
"#;

    assert_eq!(format(input), expected);
}

#[test]
fn formats_steps_patterns_with_degree_and_sample_choices() {
    let input = r#"project { seed 1 sample_rate 8khz output mono }
song "S" {
  tempo 120bpm meter 4/4 key C major
  pattern degrees = steps 1/8 {
    degree 0 octave 4
    rest
    choose {
      degree 12 octave 4 weight 1
      degree 11 octave 4
    }
  }
  pattern samples = steps 1/8 {
    sample 1
    choose {
      sample 2 weight 1
      sequence { sample 3 }
      sequence weight 2 { sample 4 sample 5 }
    }
  }
}
"#;

    let expected = r#"project {
  seed 1
  sample_rate 8khz
  output mono
}

song "S" {
  tempo 120bpm
  meter 4/4
  key C major
  pattern degrees = steps 1/8 {
    degree 0 octave 4
    rest
    choose {
      degree 12 octave 4 weight 1
      degree 11 octave 4 weight 1
    }
  }
  pattern samples = steps 1/8 {
    sample 1
    choose {
      sample 2 weight 1
      sample 3 weight 1
      sequence weight 2 {
        sample 4
        sample 5
      }
    }
  }
}
"#;

    assert_eq!(format(input), expected);
}

#[test]
fn formats_arrangement_occurrences_with_instrument_overrides() {
    let input = r#"project { seed 1 sample_rate 8khz output mono }
song "S" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern phrase = sequence { note C4 for 1/4 }
  arrangement { phrase with lead phrase }
}
"#;

    let output = format(input);
    assert!(output.contains("  arrangement {\n    phrase with lead\n    phrase\n  }\n"));
}

#[test]
fn collapses_empty_bodies_to_a_single_line() {
    let input = "project {}\nsong \"S\" {\n  tempo 120bpm meter 4/4 key C major\n  pattern p = sequence {}\n}\n";

    let output = format(input);
    assert!(output.contains("project {}\n"));
    assert!(output.contains("sequence {}\n"));
}

#[test]
fn refuses_to_format_source_with_syntax_diagnostics() {
    let error = format_source("project {").expect_err("unclosed project should fail");
    assert!(!error.diagnostics().is_empty());
}

#[test]
fn is_idempotent_across_every_grammar_construct() {
    let sources = [
        r#"project { seed 1 sample_rate 48khz output stereo }
song "First Song" {
  tempo 150bpm
  meter 4/4
  key C major

  pattern melody = sequence {
    note C4 for 1/4
    note E4 for 1/4
    rest for 1/4
    chord C#4 Eb4 G4 for 1/4 velocity 96
    note G4 for 1/2
  }
  arrangement { melody }
}
"#,
        r#"project { seed 1 sample_rate 8khz output mono }
song "S" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  instrument piano = sampled { source "piano.wav" root C4 }
  instrument voice = sampler { pack "numbers" }
  rhythm stabs resolution 1/4 { hit rest hit rest }
  pattern harmony = sequence { chord C4 E4 G4 for 1/1 }
  track chords role harmony {
    instrument lead
    volume -6 db
    play harmony |> trigger_with stabs |> gate 80% |> transpose -3 st |> gain 1.25
  }
  pattern degrees = steps 1/8 {
    degree 0 octave 4
    choose { degree 12 octave 4 weight 1 degree 11 octave 4 weight 3 }
  }
  pattern samples = steps 1/8 {
    choose { sample 1 weight 1 sequence weight 2 { sample 2 sample 3 } }
  }
  arrangement { harmony with lead }
}
"#,
        "project {\n  # header comment\n  seed 1 // trailing\n  sample_rate 8khz\n  output mono\n}\n\nsong \"S\" {\n  tempo 120bpm\n  meter 4/4\n  key C major\n  pattern p = sequence {\n    note C4 for 1/4\n  }\n}\n",
    ];

    for source in sources {
        let once = format(source);
        let twice = format(&once);
        assert_eq!(
            once, twice,
            "formatting should be idempotent for:\n{source}"
        );
    }
}
