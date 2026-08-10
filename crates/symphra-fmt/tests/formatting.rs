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
fn formats_track_effect_delay() {
    let input = r#"song "S" {
  track drums role beat {
    instrument tr909
    play kit
    effect delay { mix 0.40 time 1/4 feedback 0.25 }
  }
}
"#;

    let expected = r#"song "S" {
  track drums role beat {
    instrument tr909
    play kit
    effect delay {
      mix 0.4
      time 1/4
      feedback 0.25
    }
  }
}
"#;

    assert_eq!(format(input), expected);
}

#[test]
fn formats_track_effect_filter() {
    let input = r#"song "S" {
  track drums role beat {
    instrument tr909
    play kit
    effect filter { cutoff 2000hz resonance 0.40 }
  }
}
"#;

    let expected = r#"song "S" {
  track drums role beat {
    instrument tr909
    play kit
    effect filter {
      cutoff 2000hz
      resonance 0.4
    }
  }
}
"#;

    assert_eq!(format(input), expected);
}

#[test]
fn formats_track_effect_reverb() {
    let input = r#"song "S" {
  track drums role beat {
    instrument tr909
    play kit
    effect reverb { mix 0.40 size 0.80 }
  }
}
"#;

    let expected = r#"song "S" {
  track drums role beat {
    instrument tr909
    play kit
    effect reverb {
      mix 0.4
      size 0.8
    }
  }
}
"#;

    assert_eq!(format(input), expected);
}

#[test]
fn formats_track_automate_cutoff() {
    let input = r#"song "S" {
  track drums role beat {
    instrument tr909
    play kit
    effect filter { cutoff 600hz resonance 0.40 }
    automate cutoff { lfo sine { range 600hz..2800hz rate 2 cycles/bar } }
  }
}
"#;

    let expected = r#"song "S" {
  track drums role beat {
    instrument tr909
    play kit
    effect filter {
      cutoff 600hz
      resonance 0.4
    }
    automate cutoff {
      lfo sine {
        range 600hz..2800hz
        rate 2 cycles/bar
      }
    }
  }
}
"#;

    assert_eq!(format(input), expected);
}

#[test]
fn formats_oscillator_envelope_and_supersaw_instrument() {
    let input = r#"song "S" {
  instrument lead = sine { envelope { attack 4ms decay 200ms sustain 0.50 release 150ms } }
  instrument chord_saw = synth supersaw { voices 5 detune 0.35 spread 0.80 envelope { attack 4ms decay 200ms sustain 0.50 release 150ms } }
}
"#;

    let expected = r#"song "S" {
  instrument lead = sine {
    envelope {
      attack 4ms
      decay 200ms
      sustain 0.5
      release 150ms
    }
  }
  instrument chord_saw = synth supersaw {
    voices 5
    detune 0.35
    spread 0.8
    envelope {
      attack 4ms
      decay 200ms
      sustain 0.5
      release 150ms
    }
  }
}
"#;

    assert_eq!(format(input), expected);
}

#[test]
fn formats_track_with_layer_uses() {
    let input = r#"song "S" {
  track bass role low {
    volume -3db
    layer { use sub_sine { play bass_roots |> gain 1.0 } use sub_triangle { play bass_roots |> gain 0.6 |> pan -20% } }
  }
}
"#;

    let expected = r#"song "S" {
  track bass role low {
    volume -3 db
    layer {
      use sub_sine {
        play bass_roots |> gain 1
      }
      use sub_triangle {
        play bass_roots |> gain 0.6 |> pan -20%
      }
    }
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
fn formats_at_bar_beat_placement() {
    let input = r#"song "S" {
  track crash role fx {
    instrument tr909
    at 2:1 play drum "cr" with one_hit
  }
}
"#;

    let expected = r#"song "S" {
  track crash role fx {
    instrument tr909
    at 2:1 play drum "cr" with one_hit
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
fn formats_sample_and_drum_step_velocity() {
    let input = r#"song "S" {
  pattern kit = steps 1/8 { drum "bd" velocity 90 sample 2 velocity 40 rest }
}
"#;

    let expected = r#"song "S" {
  pattern kit = steps 1/8 {
    drum "bd" velocity 90
    sample 2 velocity 40
    rest
  }
}
"#;

    assert_eq!(format(input), expected);
}

#[test]
fn formats_bar_durations() {
    let input = r#"song "S" {
  pattern phrase = sequence { note C4 for 1bar chord C4 E4 for 2bar velocity 90 rest for 1bar }
}
"#;

    let expected = r#"song "S" {
  pattern phrase = sequence {
    note C4 for 1bar
    chord C4 E4 for 2bar velocity 90
    rest for 1bar
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
fn formats_section_with_exact_parallel_block() {
    let input = r#"song "S" {
  section phrase bars 4 { parallel exact { play track chords play track bass } }
}
"#;

    let expected = r#"song "S" {
  section phrase bars 4 {
    parallel exact {
      play track chords
      play track bass
    }
  }
}
"#;

    assert_eq!(format(input), expected);
}

#[test]
fn formats_section_without_exact_parallel_block() {
    let input = r#"song "S" {
  section intro bars 2 { parallel { play track pad } }
}
"#;

    let expected = r#"song "S" {
  section intro bars 2 {
    parallel {
      play track pad
    }
  }
}
"#;

    assert_eq!(format(input), expected);
}

#[test]
fn formats_arrangement_play_section_entries() {
    let input = r#"song "S" {
  arrangement { play phrase play phrase }
}
"#;

    let expected = r#"song "S" {
  arrangement {
    play phrase
    play phrase
  }
}
"#;

    assert_eq!(format(input), expected);
}

#[test]
fn formats_master_limiter() {
    let input = r#"song "S" {
  master { limiter { ceiling -0.3db } }
}
"#;

    let expected = r#"song "S" {
  master {
    limiter {
      ceiling -0.3 db
    }
  }
}
"#;

    assert_eq!(format(input), expected);
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

/// Split out of `is_idempotent_across_every_grammar_construct` (and further
/// split from `idempotency_sources_effects` below) so that test stays under
/// clippy's `too_many_lines` threshold as new grammar constructs get their
/// own idempotency source appended.
fn idempotency_sources_core() -> [&'static str; 4] {
    [
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
    note A4 for 1bar
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
    sample 4 velocity 90
    choose { sample 1 weight 1 sequence weight 2 { sample 2 sample 3 } }
  }
  arrangement { harmony with lead }
}
"#,
        "project {\n  # header comment\n  seed 1 // trailing\n  sample_rate 8khz\n  output mono\n}\n\nsong \"S\" {\n  tempo 120bpm\n  meter 4/4\n  key C major\n  pattern p = sequence {\n    note C4 for 1/4\n  }\n}\n",
        r#"project { seed 1 sample_rate 8khz output mono }
song "Sections" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern chords = sequence { note C4 for 1bar }
  pattern bassline = sequence { note C2 for 1bar }
  track chords role harmony {
    instrument lead
    play chords
  }
  track bass role low {
    instrument lead
    play bassline
  }
  section phrase bars 1 {
    parallel exact {
      play track chords
      play track bass
    }
  }
  arrangement {
    play phrase
    play phrase
  }
  master {
    limiter {
      ceiling -0.3db
    }
  }
}
"#,
    ]
}

/// Effect-related idempotency sources, split out of
/// `idempotency_sources_core` to stay under clippy's `too_many_lines`.
fn idempotency_sources_effects() -> [&'static str; 4] {
    [
        r#"project { seed 1 sample_rate 8khz output mono }
song "FilterEffect" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect filter { cutoff 2000hz resonance 0.4 }
  }
}
"#,
        r#"project { seed 1 sample_rate 8khz output mono }
song "ReverbEffect" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect reverb { mix 0.4 size 0.8 }
  }
}
"#,
        r#"project { seed 1 sample_rate 8khz output mono }
song "FilterAutomation" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine
  pattern melody = sequence { note C4 for 1/4 }
  track lead role melody {
    instrument lead
    play melody
    effect filter { cutoff 600hz resonance 0.4 }
    automate cutoff { lfo sine { range 600hz..2800hz rate 2 cycles/bar } }
  }
}
"#,
        r#"project { seed 1 sample_rate 8khz output mono }
song "SupersawEnvelope" {
  tempo 120bpm meter 4/4 key C major
  instrument lead = sine { envelope { attack 4ms decay 200ms sustain 0.5 release 150ms } }
  instrument chord_saw = synth supersaw { voices 5 detune 0.35 spread 0.8 envelope { attack 4ms decay 200ms sustain 0.5 release 150ms } }
  pattern chords = sequence { chord C4 E4 G4 for 1/1 }
  track lead role melody {
    instrument chord_saw
    play chords
  }
}
"#,
    ]
}

#[test]
fn is_idempotent_across_every_grammar_construct() {
    for source in idempotency_sources_core()
        .into_iter()
        .chain(idempotency_sources_effects())
    {
        let once = format(source);
        let twice = format(&once);
        assert_eq!(
            once, twice,
            "formatting should be idempotent for:\n{source}"
        );
    }
}
