use std::fmt::Write as _;

use symphra_syntax::SourceSpan;
use symphra_syntax::ast::{
    ArrangementEntry, ArrangementOccurrence, AutomateDeclaration, ChanceTransformExpression,
    ChordExpression, ChordPitches, Declaration, DegreeChoiceAlternative, DurationExpression,
    EffectDeclaration, EffectFactor, EffectKind, EffectPresetDeclaration, EnvelopeDeclaration,
    Identifier, InstrumentBody, InstrumentDeclaration, LayerUse, LfoDeclaration, MasterDeclaration,
    NoteExpression, NumberLiteral, OctavesExpression, PanExpression, PatternBody,
    PatternDeclaration, PlaySource, PlayStatement, ProjectDeclaration, ProjectStatement,
    QuotedString, RateLiteral, RepeatCount, RepeatGroup, RhythmDeclaration, RhythmItem,
    SampleChoiceAlternative, SampleSelectorExpression, SectionDeclaration, SectionTrack,
    SequenceItem, SongDeclaration, SongStatement, SourceFile, SpeedExpression, StepItem, TrackBody,
    TrackDeclaration, TrackEffect, VelocityExpression, VolumeExpression,
};

use crate::printer::{BlankSeparator, Printer};
use crate::trivia::{CommentCursor, LeadingTrivia};

pub struct Ctx<'a> {
    pub source: &'a str,
    pub cursor: CommentCursor<'a>,
    pub printer: Printer,
}

impl Ctx<'_> {
    fn text(&self, span: SourceSpan) -> &str {
        &self.source[span.range()]
    }
}

/// Whether (and how) a sibling list is stably re-sorted into canonical
/// rank order after printing. Variants that carry a [`BlankSeparator`]
/// additionally force blank lines between items; see
/// [`Printer::reorder_since`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reorder {
    No,
    Yes,
    YesWithSeparator(BlankSeparator),
}

/// Lowest [`song_statement_rank`] value that is a declaration rather than a
/// packed setting (`tempo` / `meter` / `key`). Used to force a blank line
/// before each song-level declaration.
const SONG_DECLARATION_RANK: u8 = 3;

pub fn format_source_file(ctx: &mut Ctx<'_>, file: &SourceFile) {
    let items: Vec<&Declaration> = file.declarations.iter().collect();
    let dangling = print_items(
        ctx,
        file.span.start,
        &items,
        declaration_span,
        file.span.end,
        declaration_rank,
        Reorder::YesWithSeparator(BlankSeparator::OnRankChange),
        print_declaration,
    );
    print_dangling(ctx, &dangling);
}

fn declaration_rank(declaration: &Declaration) -> u8 {
    match declaration {
        Declaration::Project(_) => 0,
        Declaration::Song(_) => 1,
    }
}

fn declaration_span(declaration: &Declaration) -> SourceSpan {
    match declaration {
        Declaration::Project(project) => project.span,
        Declaration::Song(song) => song.span,
    }
}

fn print_declaration(ctx: &mut Ctx<'_>, declaration: &Declaration) {
    match declaration {
        Declaration::Project(project) => print_project(ctx, project),
        Declaration::Song(song) => print_song(ctx, song),
    }
}

/// Prints one brace-delimited construct: a header (everything up to and
/// including the opening brace), a body of comment-and-blank-line-aware
/// items, and a closing brace. Collapses to `header {}` when the body has
/// neither items nor dangling comments.
///
/// `block_span` bounds the search for comments around `items`; it need not
/// be the exact brace positions (see [`crate::trivia::CommentCursor`]).
#[allow(clippy::too_many_arguments)]
fn print_block<T: Copy>(
    ctx: &mut Ctx<'_>,
    header: &str,
    block_span: SourceSpan,
    items: &[T],
    span_of: impl Fn(T) -> SourceSpan,
    rank_of: impl Fn(T) -> u8,
    reorder: Reorder,
    print_item: impl FnMut(&mut Ctx<'_>, T),
) {
    if items.is_empty() {
        let dangling = ctx.cursor.take_leading(block_span.end, block_span.start);
        if dangling.comments.is_empty() {
            ctx.printer.line(format!("{header} {{}}"));
            return;
        }
        ctx.printer.line(format!("{header} {{"));
        ctx.printer.indent();
        print_dangling(ctx, &dangling);
        ctx.printer.dedent();
        ctx.printer.line("}");
        return;
    }
    ctx.printer.line(format!("{header} {{"));
    ctx.printer.indent();
    let dangling = print_items(
        ctx,
        block_span.start,
        items,
        span_of,
        block_span.end,
        rank_of,
        reorder,
        print_item,
    );
    print_dangling(ctx, &dangling);
    ctx.printer.dedent();
    ctx.printer.line("}");
}

/// Prints a full set of siblings, attaching each one's leading and
/// same-line-trailing comments and printing (recursing into) the item
/// itself, all interleaved in strict source order so the shared
/// [`crate::trivia::CommentCursor`] only ever advances monotonically.
/// Returns the trailing trivia left over after the last item.
///
/// When `reorder` is true, the already-printed lines for each item are
/// stably rearranged into rank order afterward (see
/// [`Printer::reorder_since`]); comments travel with the item they were
/// attached to, and each item's own author-authored blank line above it is
/// preserved regardless of where it ends up.
#[allow(clippy::too_many_arguments)]
fn print_items<T: Copy>(
    ctx: &mut Ctx<'_>,
    open_end: u32,
    items: &[T],
    span_of: impl Fn(T) -> SourceSpan,
    close_end: u32,
    rank_of: impl Fn(T) -> u8,
    reorder: Reorder,
    mut print_item: impl FnMut(&mut Ctx<'_>, T),
) -> LeadingTrivia {
    let block_start = ctx.printer.mark();
    let mut prev_end = open_end;
    let mut ranges = Vec::with_capacity(items.len());
    for (index, &item) in items.iter().enumerate() {
        let span = span_of(item);
        let leading = ctx.cursor.take_leading(span.start, prev_end);
        let item_start = ctx.printer.mark();
        for comment in &leading.comments {
            if comment.blank_before {
                ctx.printer.blank();
            }
            ctx.printer.line(&comment.text);
        }
        if leading.blank_before_node {
            ctx.printer.blank();
        }
        print_item(ctx, item);
        let next_limit = items
            .get(index + 1)
            .map_or(close_end, |&next| span_of(next).start);
        let trailing = ctx.cursor.take_trailing_same_line(span.end, next_limit);
        if let Some((text, _)) = &trailing {
            ctx.printer.append_to_last(&format!(" {text}"));
        }
        prev_end = trailing.map_or(span.end, |(_, end)| end);
        ranges.push((rank_of(item), item_start..ctx.printer.mark()));
    }
    match reorder {
        Reorder::No => {}
        Reorder::Yes => ctx
            .printer
            .reorder_since(block_start, ranges, BlankSeparator::None),
        Reorder::YesWithSeparator(separator) => {
            ctx.printer.reorder_since(block_start, ranges, separator);
        }
    }
    ctx.cursor.take_leading(close_end, prev_end)
}

fn print_dangling(ctx: &mut Ctx<'_>, dangling: &LeadingTrivia) {
    for comment in &dangling.comments {
        if comment.blank_before {
            ctx.printer.blank();
        }
        ctx.printer.line(&comment.text);
    }
}

fn rate_text(ctx: &Ctx<'_>, rate: &RateLiteral) -> String {
    format!("{}{}", ctx.text(rate.value.span), ctx.text(rate.unit.span))
}

// --- project ---------------------------------------------------------

fn print_project(ctx: &mut Ctx<'_>, decl: &ProjectDeclaration) {
    let items: Vec<&ProjectStatement> = decl.statements.iter().collect();
    print_block(
        ctx,
        "project",
        decl.span,
        &items,
        project_statement_span,
        project_statement_rank,
        Reorder::Yes,
        print_project_statement,
    );
}

fn project_statement_rank(statement: &ProjectStatement) -> u8 {
    match statement {
        ProjectStatement::Seed { .. } => 0,
        ProjectStatement::SampleRate { .. } => 1,
        ProjectStatement::Output { .. } => 2,
    }
}

fn project_statement_span(statement: &ProjectStatement) -> SourceSpan {
    match statement {
        ProjectStatement::Seed { span, .. }
        | ProjectStatement::SampleRate { span, .. }
        | ProjectStatement::Output { span, .. } => *span,
    }
}

fn print_project_statement(ctx: &mut Ctx<'_>, statement: &ProjectStatement) {
    match statement {
        ProjectStatement::Seed { value, .. } => ctx.printer.line(format!("seed {value}")),
        ProjectStatement::SampleRate { value, .. } => {
            ctx.printer
                .line(format!("sample_rate {}", rate_text(ctx, value)));
        }
        ProjectStatement::Output { channels, .. } => {
            ctx.printer
                .line(format!("output {}", ctx.text(channels.span)));
        }
    }
}

// --- song --------------------------------------------------------------

fn print_song(ctx: &mut Ctx<'_>, decl: &SongDeclaration) {
    let header = format!("song {}", ctx.text(decl.name.span));
    let items: Vec<&SongStatement> = decl.statements.iter().collect();
    print_block(
        ctx,
        &header,
        decl.span,
        &items,
        song_statement_span,
        song_statement_rank,
        Reorder::YesWithSeparator(BlankSeparator::BeforeRankAtLeast(SONG_DECLARATION_RANK)),
        print_song_statement,
    );
}

fn song_statement_rank(statement: &SongStatement) -> u8 {
    match statement {
        SongStatement::Tempo { .. } => 0,
        SongStatement::Meter { .. } => 1,
        SongStatement::Key { .. } => 2,
        SongStatement::Instrument(_) => 3,
        SongStatement::EffectPreset(_) => 4,
        SongStatement::Rhythm(_) => 5,
        SongStatement::Pattern(_) => 6,
        SongStatement::Track(_) => 7,
        SongStatement::Section(_) => 8,
        SongStatement::Arrangement { .. } => 9,
        SongStatement::Master(_) => 10,
    }
}

fn song_statement_span(statement: &SongStatement) -> SourceSpan {
    match statement {
        SongStatement::Tempo { span, .. }
        | SongStatement::Meter { span, .. }
        | SongStatement::Key { span, .. }
        | SongStatement::Arrangement { span, .. } => *span,
        SongStatement::Instrument(decl) => decl.span,
        SongStatement::EffectPreset(decl) => decl.span,
        SongStatement::Rhythm(decl) => decl.span,
        SongStatement::Track(decl) => decl.span,
        SongStatement::Section(decl) => decl.span,
        SongStatement::Master(decl) => decl.span,
        SongStatement::Pattern(decl) => decl.span,
    }
}

fn print_song_statement(ctx: &mut Ctx<'_>, statement: &SongStatement) {
    match statement {
        SongStatement::Tempo { value, .. } => {
            ctx.printer.line(format!("tempo {}", rate_text(ctx, value)));
        }
        SongStatement::Meter {
            numerator,
            denominator,
            ..
        } => ctx.printer.line(format!("meter {numerator}/{denominator}")),
        SongStatement::Key { tonic, mode, .. } => ctx.printer.line(format!(
            "key {} {}",
            ctx.text(tonic.span),
            ctx.text(mode.span)
        )),
        SongStatement::Instrument(decl) => print_instrument(ctx, decl),
        SongStatement::EffectPreset(decl) => print_effect_preset(ctx, decl),
        SongStatement::Rhythm(decl) => print_rhythm(ctx, decl),
        SongStatement::Track(decl) => print_track(ctx, decl),
        SongStatement::Section(decl) => print_section(ctx, decl),
        SongStatement::Master(decl) => print_master(ctx, decl),
        SongStatement::Arrangement { entries, span } => {
            let items: Vec<&ArrangementEntry> = entries.iter().collect();
            print_block(
                ctx,
                "arrangement",
                *span,
                &items,
                |entry: &ArrangementEntry| entry.span(),
                |_| 0,
                Reorder::No,
                print_arrangement_entry,
            );
        }
        SongStatement::Pattern(decl) => print_pattern(ctx, decl),
    }
}

fn print_arrangement_entry(ctx: &mut Ctx<'_>, entry: &ArrangementEntry) {
    match entry {
        ArrangementEntry::Pattern(occurrence) => print_arrangement_occurrence(ctx, occurrence),
        ArrangementEntry::Play { name, .. } => {
            ctx.printer.line(format!("play {}", ctx.text(name.span)));
        }
    }
}

fn print_arrangement_occurrence(ctx: &mut Ctx<'_>, occurrence: &ArrangementOccurrence) {
    let mut line = ctx.text(occurrence.pattern.span).to_owned();
    if let Some(instrument) = &occurrence.instrument {
        line.push_str(" with ");
        line.push_str(ctx.text(instrument.span));
    }
    ctx.printer.line(line);
}

// --- section -------------------------------------------------------------

fn print_section(ctx: &mut Ctx<'_>, decl: &SectionDeclaration) {
    let header = format!("section {} bars {}", ctx.text(decl.name.span), decl.bars);
    let items: Vec<&SectionDeclaration> = vec![decl];
    print_block(
        ctx,
        &header,
        decl.span,
        &items,
        |decl: &SectionDeclaration| decl.span,
        |_| 0,
        Reorder::No,
        print_section_parallel,
    );
}

fn print_section_parallel(ctx: &mut Ctx<'_>, decl: &SectionDeclaration) {
    let header = if decl.exact {
        "parallel exact"
    } else {
        "parallel"
    };
    let items: Vec<&SectionTrack> = decl.tracks.iter().collect();
    // `SectionDeclaration` only records the whole section span, not the
    // inner `parallel` block. Using the section span as the parallel body's
    // open bound makes the first `play track` see the newlines after the
    // section `{` *and* the parallel `{` as a blank gap. Start at the
    // `parallel` keyword instead so only the gap inside the parallel body
    // counts.
    let block_span = SourceSpan {
        source: decl.span.source,
        start: parallel_keyword_start(ctx.source, decl),
        end: decl.span.end,
    };
    print_block(
        ctx,
        header,
        block_span,
        &items,
        |track: &SectionTrack| track.span,
        |_| 0,
        Reorder::No,
        print_section_track,
    );
}

/// Byte offset of the `parallel` keyword inside a section, or the section
/// start if it cannot be found (should not happen for a valid parse).
fn parallel_keyword_start(source: &str, section: &SectionDeclaration) -> u32 {
    let text = &source[section.span.range()];
    let Some(brace) = text.find('{') else {
        return section.span.start;
    };
    let Some(rel) = text[brace..].find("parallel") else {
        return section.span.start;
    };
    section.span.start + u32::try_from(brace + rel).unwrap_or(0)
}

/// Prints `play track <name>`, with its override block when it has one.
///
/// A short override — only a volume and a preset reference, with no comments
/// inside — stays on the reference's own line, the way `rhythm` bodies stay
/// compact. It reads as a modifier on the reference rather than a block, and
/// keeping it inline is what makes overriding shorter than declaring a second
/// track in the first place.
fn print_section_track(ctx: &mut Ctx<'_>, track: &SectionTrack) {
    let header = format!("play track {}", ctx.text(track.name.span));
    if track.volume.is_none() && track.effect.is_none() && track.automate.is_none() {
        ctx.printer.line(header);
        return;
    }
    let inline = track.automate.is_none()
        && !matches!(track.effect, Some(TrackEffect::Inline(_)))
        && !ctx.cursor.has_comment_before(track.span.end);
    if inline {
        let mut line = header;
        line.push_str(" {");
        if let Some(volume) = &track.volume {
            let _ = write!(
                line,
                " volume {}{}",
                volume.decibels,
                ctx.text(volume.unit.span)
            );
        }
        if let Some(TrackEffect::Preset(name)) = &track.effect {
            let _ = write!(line, "  effect {}", ctx.text(name.span));
        }
        line.push_str(" }");
        let _ = ctx.cursor.take_leading(track.span.end, track.span.start);
        ctx.printer.line(line);
        return;
    }
    let mut items = Vec::new();
    if let Some(volume) = &track.volume {
        items.push(TrackField::Volume(volume));
    }
    if let Some(effect) = &track.effect {
        items.push(TrackField::Effect(effect));
    }
    if let Some(automate) = &track.automate {
        items.push(TrackField::Automate(automate));
    }
    let items: Vec<&TrackField<'_>> = items.iter().collect();
    print_block(
        ctx,
        &header,
        track.span,
        &items,
        |field: &TrackField<'_>| track_field_span(field),
        |_| 0,
        Reorder::No,
        print_track_field,
    );
}

// --- master ----------------------------------------------------------------

fn print_master(ctx: &mut Ctx<'_>, decl: &MasterDeclaration) {
    let items: Vec<&MasterDeclaration> = vec![decl];
    print_block(
        ctx,
        "master",
        decl.span,
        &items,
        |decl: &MasterDeclaration| decl.span,
        |_| 0,
        Reorder::No,
        print_master_limiter,
    );
}

fn print_master_limiter(ctx: &mut Ctx<'_>, decl: &MasterDeclaration) {
    let items: Vec<&VolumeExpression> = vec![&decl.ceiling];
    print_block(
        ctx,
        "limiter",
        decl.span,
        &items,
        |ceiling: &VolumeExpression| ceiling.span,
        |_| 0,
        Reorder::No,
        |ctx, ceiling| {
            ctx.printer.line(format!(
                "ceiling {}{}",
                ceiling.decibels,
                ctx.text(ceiling.unit.span)
            ));
        },
    );
}

// --- instrument ----------------------------------------------------------

enum SampledField<'a> {
    Source(&'a QuotedString),
    Root(&'a Identifier),
}

fn sampled_field_span(field: &SampledField<'_>) -> SourceSpan {
    match field {
        SampledField::Source(source) => source.span,
        SampledField::Root(root) => root.span,
    }
}

#[derive(Clone, Copy)]
enum EnvelopeField<'a> {
    Attack(&'a RateLiteral),
    Decay(&'a RateLiteral),
    Sustain(&'a EffectFactor),
    Release(&'a RateLiteral),
}

fn envelope_field_span(field: EnvelopeField<'_>) -> SourceSpan {
    match field {
        EnvelopeField::Attack(rate) | EnvelopeField::Decay(rate) | EnvelopeField::Release(rate) => {
            rate.span
        }
        EnvelopeField::Sustain(factor) => factor.span,
    }
}

fn print_envelope(ctx: &mut Ctx<'_>, envelope: &EnvelopeDeclaration) {
    let items = [
        EnvelopeField::Attack(&envelope.attack),
        EnvelopeField::Decay(&envelope.decay),
        EnvelopeField::Sustain(&envelope.sustain),
        EnvelopeField::Release(&envelope.release),
    ];
    print_block(
        ctx,
        "envelope",
        envelope.span,
        &items,
        envelope_field_span,
        |_| 0,
        Reorder::No,
        |ctx, field| match field {
            EnvelopeField::Attack(rate) => {
                ctx.printer.line(format!("attack {}", rate_text(ctx, rate)));
            }
            EnvelopeField::Decay(rate) => {
                ctx.printer.line(format!("decay {}", rate_text(ctx, rate)));
            }
            EnvelopeField::Sustain(factor) => {
                ctx.printer.line(format!("sustain {}", factor.value));
            }
            EnvelopeField::Release(rate) => {
                ctx.printer
                    .line(format!("release {}", rate_text(ctx, rate)));
            }
        },
    );
}

#[derive(Clone, Copy)]
enum SupersawField<'a> {
    Voices(u32, SourceSpan),
    Detune(&'a EffectFactor),
    Spread(&'a EffectFactor),
    Envelope(&'a EnvelopeDeclaration),
}

fn supersaw_field_span(field: SupersawField<'_>) -> SourceSpan {
    match field {
        SupersawField::Voices(_, span) => span,
        SupersawField::Detune(factor) | SupersawField::Spread(factor) => factor.span,
        SupersawField::Envelope(envelope) => envelope.span,
    }
}

fn print_oscillator_instrument(
    ctx: &mut Ctx<'_>,
    name: &str,
    waveform: &Identifier,
    envelope: Option<&EnvelopeDeclaration>,
    span: SourceSpan,
) {
    let Some(envelope) = envelope else {
        ctx.printer
            .line(format!("instrument {name} = {}", ctx.text(waveform.span)));
        return;
    };
    let header = format!("instrument {name} = {}", ctx.text(waveform.span));
    let items: Vec<&EnvelopeDeclaration> = vec![envelope];
    print_block(
        ctx,
        &header,
        span,
        &items,
        |envelope: &EnvelopeDeclaration| envelope.span,
        |_| 0,
        Reorder::No,
        print_envelope,
    );
}

fn print_supersaw_instrument(
    ctx: &mut Ctx<'_>,
    name: &str,
    voices: (u32, SourceSpan),
    detune: &EffectFactor,
    spread: &EffectFactor,
    envelope: Option<&EnvelopeDeclaration>,
    span: SourceSpan,
) {
    let header = format!("instrument {name} = synth supersaw");
    let mut items = vec![
        SupersawField::Voices(voices.0, voices.1),
        SupersawField::Detune(detune),
        SupersawField::Spread(spread),
    ];
    if let Some(envelope) = envelope {
        items.push(SupersawField::Envelope(envelope));
    }
    print_block(
        ctx,
        &header,
        span,
        &items,
        supersaw_field_span,
        |_| 0,
        Reorder::No,
        |ctx, field| match field {
            SupersawField::Voices(voices, _) => {
                ctx.printer.line(format!("voices {voices}"));
            }
            SupersawField::Detune(factor) => {
                ctx.printer.line(format!("detune {}", factor.value));
            }
            SupersawField::Spread(factor) => {
                ctx.printer.line(format!("spread {}", factor.value));
            }
            SupersawField::Envelope(envelope) => print_envelope(ctx, envelope),
        },
    );
}

fn print_instrument(ctx: &mut Ctx<'_>, decl: &InstrumentDeclaration) {
    let name = ctx.text(decl.name.span).to_owned();
    match &decl.body {
        InstrumentBody::Oscillator {
            waveform, envelope, ..
        } => print_oscillator_instrument(ctx, &name, waveform, envelope.as_ref(), decl.span),
        InstrumentBody::Supersaw {
            voices,
            voices_span,
            detune,
            spread,
            envelope,
            ..
        } => print_supersaw_instrument(
            ctx,
            &name,
            (*voices, *voices_span),
            detune,
            spread,
            envelope.as_ref(),
            decl.span,
        ),
        InstrumentBody::Sampled { source, root, .. } => {
            let header = format!("instrument {name} = sampled");
            let fields = [SampledField::Source(source), SampledField::Root(root)];
            let items: Vec<&SampledField<'_>> = fields.iter().collect();
            print_block(
                ctx,
                &header,
                decl.span,
                &items,
                |field: &SampledField<'_>| sampled_field_span(field),
                |_| 0,
                Reorder::No,
                |ctx, field| match field {
                    SampledField::Source(source) => ctx
                        .printer
                        .line(format!("source {}", ctx.text(source.span))),
                    SampledField::Root(root) => {
                        ctx.printer.line(format!("root {}", ctx.text(root.span)));
                    }
                },
            );
        }
        InstrumentBody::Sampler { pack, .. } => {
            let header = format!("instrument {name} = sampler");
            let items = [pack];
            print_block(
                ctx,
                &header,
                decl.span,
                &items,
                |pack: &QuotedString| pack.span,
                |_| 0,
                Reorder::No,
                |ctx, pack| ctx.printer.line(format!("pack {}", ctx.text(pack.span))),
            );
        }
        InstrumentBody::DrumMachine { bank, .. } => {
            let header = format!("instrument {name} = drum_machine");
            let items = [bank];
            print_block(
                ctx,
                &header,
                decl.span,
                &items,
                |bank: &QuotedString| bank.span,
                |_| 0,
                Reorder::No,
                |ctx, bank| ctx.printer.line(format!("bank {}", ctx.text(bank.span))),
            );
        }
        InstrumentBody::SoundFont { source, preset, .. } => {
            let header = format!("instrument {name} = soundfont");
            let fields = [
                SoundFontField::Source(source),
                SoundFontField::Preset(preset),
            ];
            let items: Vec<&SoundFontField<'_>> = fields.iter().collect();
            print_block(
                ctx,
                &header,
                decl.span,
                &items,
                |field: &SoundFontField<'_>| soundfont_field_span(field),
                |_| 0,
                Reorder::No,
                |ctx, field| match field {
                    SoundFontField::Source(source) => ctx
                        .printer
                        .line(format!("source {}", ctx.text(source.span))),
                    SoundFontField::Preset(preset) => ctx
                        .printer
                        .line(format!("preset {}", ctx.text(preset.span))),
                },
            );
        }
        InstrumentBody::Vst3 { source, preset, .. } => {
            print_vst3_instrument(ctx, &name, source, preset.as_ref(), decl.span);
        }
    }
}

fn print_vst3_instrument(
    ctx: &mut Ctx<'_>,
    name: &str,
    source: &QuotedString,
    preset: Option<&QuotedString>,
    span: SourceSpan,
) {
    let header = format!("instrument {name} = vst3");
    let mut items = vec![Vst3Field::Source(source)];
    if let Some(preset) = preset {
        items.push(Vst3Field::Preset(preset));
    }
    print_block(
        ctx,
        &header,
        span,
        &items,
        vst3_field_span,
        |_| 0,
        Reorder::No,
        |ctx, field| match field {
            Vst3Field::Source(source) => ctx
                .printer
                .line(format!("source {}", ctx.text(source.span))),
            Vst3Field::Preset(preset) => ctx
                .printer
                .line(format!("preset {}", ctx.text(preset.span))),
        },
    );
}

enum SoundFontField<'a> {
    Source(&'a QuotedString),
    Preset(&'a QuotedString),
}

fn soundfont_field_span(field: &SoundFontField<'_>) -> SourceSpan {
    match field {
        SoundFontField::Source(source) | SoundFontField::Preset(source) => source.span,
    }
}

#[derive(Clone, Copy)]
enum Vst3Field<'a> {
    Source(&'a QuotedString),
    Preset(&'a QuotedString),
}

fn vst3_field_span(field: Vst3Field<'_>) -> SourceSpan {
    match field {
        Vst3Field::Source(source) | Vst3Field::Preset(source) => source.span,
    }
}

// --- rhythm ----------------------------------------------------------------

fn rhythm_item_span(item: &RhythmItem) -> SourceSpan {
    match item {
        RhythmItem::Hit { span } | RhythmItem::Rest { span } => *span,
        RhythmItem::Repeat(group) => group.span,
    }
}

fn rhythm_item_text(item: &RhythmItem) -> String {
    match item {
        RhythmItem::Hit { .. } => "hit".to_owned(),
        RhythmItem::Rest { .. } => "rest".to_owned(),
        RhythmItem::Repeat(group) => format_repeat(group, rhythm_item_text),
    }
}

/// Reprints a `* N` repetition: `hit * 4` for a single item, and
/// `(hit, rest) * 4` for a group.
///
/// A one-element group written as `(hit) * 4` normalizes to the ungrouped
/// spelling; the parentheses carry no meaning there, and the AST does not
/// record whether the author typed them.
fn format_repeat<T>(group: &RepeatGroup<T>, element: impl Fn(&T) -> String) -> String {
    let count = group.count;
    if let [only] = group.items.as_slice() {
        return format!("{} * {count}", element(only));
    }
    let items = group
        .items
        .iter()
        .map(element)
        .collect::<Vec<_>>()
        .join(", ");
    format!("({items}) * {count}")
}

/// Prints a rhythm body.
///
/// Cells that share a source line stay on one printed line (`hit rest * 2`),
/// so bar-shaped multi-line rhythms keep their line breaks:
///
/// ```text
/// rhythm chord_stabs resolution 1/8 {
///   hit rest * 2 hit rest * 2 hit rest
///   hit rest * 2 hit rest * 2 hit hit
/// }
/// ```
///
/// Compact one-liners stay compact only when the author wrote the braces
/// inline too: `rhythm pulse resolution 1/8 { hit rest hit }`. A body that
/// was written with a newline after `{` keeps the multi-line brace form even
/// when every cell sits on a single content line.
///
/// If any comment sits inside the rhythm span, fall back to one cell per
/// printed line so comments can still be reattached.
fn print_rhythm(ctx: &mut Ctx<'_>, decl: &RhythmDeclaration) {
    let header = format!(
        "rhythm {} resolution {}/{}",
        ctx.text(decl.name.span),
        decl.resolution_numerator,
        decl.resolution_denominator
    );

    if decl.items.is_empty() {
        let dangling = ctx.cursor.take_leading(decl.span.end, decl.span.start);
        if dangling.comments.is_empty() {
            ctx.printer.line(format!("{header} {{}}"));
            return;
        }
        ctx.printer.line(format!("{header} {{"));
        ctx.printer.indent();
        print_dangling(ctx, &dangling);
        ctx.printer.dedent();
        ctx.printer.line("}");
        return;
    }

    if ctx.cursor.has_comment_before(decl.span.end) {
        let items: Vec<&RhythmItem> = decl.items.iter().collect();
        print_block(
            ctx,
            &header,
            decl.span,
            &items,
            |item: &RhythmItem| rhythm_item_span(item),
            |_| 0,
            Reorder::No,
            |ctx, item| {
                ctx.printer.line(rhythm_item_text(item));
            },
        );
        return;
    }

    // Advance the cursor past every cell so later siblings do not see
    // leftover trivia, then reprint from the source line groups.
    let mut prev_end = decl.span.start;
    for (index, item) in decl.items.iter().enumerate() {
        let span = rhythm_item_span(item);
        let _ = ctx.cursor.take_leading(span.start, prev_end);
        let next_limit = decl
            .items
            .get(index + 1)
            .map_or(decl.span.end, |next| rhythm_item_span(next).start);
        let trailing = ctx.cursor.take_trailing_same_line(span.end, next_limit);
        prev_end = trailing.map_or(span.end, |(_, end)| end);
    }
    let _ = ctx.cursor.take_leading(decl.span.end, prev_end);

    let runs = rhythm_line_runs(ctx.source, &decl.items);
    // Compact only when the author wrote `{ … }` on a single source line.
    // Wrapped braces (`{\n  hit rest\n}`) keep the multi-line form even for
    // a single content line — that layout is intentional, not noise.
    if !rhythm_braces_are_multiline(ctx.source, decl.span) {
        let body = runs
            .into_iter()
            .flatten()
            .map(rhythm_item_text)
            .collect::<Vec<_>>()
            .join(" ");
        ctx.printer.line(format!("{header} {{ {body} }}"));
        return;
    }

    ctx.printer.line(format!("{header} {{"));
    ctx.printer.indent();
    for run in runs {
        let line = run
            .iter()
            .map(|item| rhythm_item_text(item))
            .collect::<Vec<_>>()
            .join(" ");
        ctx.printer.line(line);
    }
    ctx.printer.dedent();
    ctx.printer.line("}");
}

/// True when the rhythm's `{ … }` body contains a newline in the source.
fn rhythm_braces_are_multiline(source: &str, span: SourceSpan) -> bool {
    let text = &source[span.range()];
    let Some(open) = text.find('{') else {
        return false;
    };
    let Some(close) = text.rfind('}') else {
        return false;
    };
    text[open..=close].contains('\n')
}

/// Groups consecutive rhythm cells that share a source line. Authors use
/// line breaks to mark bar boundaries (`hit rest * 2 …` per bar); those
/// breaks have to survive formatting.
fn rhythm_line_runs<'a>(source: &str, items: &'a [RhythmItem]) -> Vec<Vec<&'a RhythmItem>> {
    let mut runs: Vec<Vec<&RhythmItem>> = Vec::new();
    let mut current_line = None;
    for item in items {
        let line = source_line_at(source, rhythm_item_span(item).start);
        match current_line {
            Some(previous) if previous == line => {
                runs.last_mut()
                    .expect("run started with current_line")
                    .push(item);
            }
            _ => {
                current_line = Some(line);
                runs.push(vec![item]);
            }
        }
    }
    runs
}

/// Zero-based source line of the byte offset `offset`.
fn source_line_at(source: &str, offset: u32) -> usize {
    let end = (offset as usize).min(source.len());
    source[..end].bytes().filter(|&b| b == b'\n').count()
}

// --- track -------------------------------------------------------------

enum TrackField<'a> {
    Instrument(&'a Identifier),
    Volume(&'a VolumeExpression),
    Play(&'a PlayStatement),
    Layer(&'a [LayerUse], SourceSpan),
    Effect(&'a TrackEffect),
    Automate(&'a AutomateDeclaration),
}

fn track_field_span(field: &TrackField<'_>) -> SourceSpan {
    match field {
        TrackField::Instrument(instrument) => instrument.span,
        TrackField::Volume(volume) => volume.span,
        TrackField::Play(play) => play.span,
        TrackField::Layer(_, span) => *span,
        TrackField::Effect(effect) => effect.span(),
        TrackField::Automate(automate) => automate.span,
    }
}

fn print_track(ctx: &mut Ctx<'_>, decl: &TrackDeclaration) {
    let header = format!(
        "track {} role {}",
        ctx.text(decl.name.span),
        ctx.text(decl.role.span)
    );
    let mut fields = Vec::new();
    match &decl.body {
        TrackBody::Single { instrument, play } => {
            fields.push(TrackField::Instrument(instrument));
            if let Some(volume) = &decl.volume {
                fields.push(TrackField::Volume(volume));
            }
            fields.push(TrackField::Play(play));
        }
        TrackBody::Layers { uses, span } => {
            if let Some(volume) = &decl.volume {
                fields.push(TrackField::Volume(volume));
            }
            fields.push(TrackField::Layer(uses, *span));
        }
    }
    if let Some(effect) = &decl.effect {
        fields.push(TrackField::Effect(effect));
    }
    if let Some(automate) = &decl.automate {
        fields.push(TrackField::Automate(automate));
    }
    let items: Vec<&TrackField<'_>> = fields.iter().collect();
    print_block(
        ctx,
        &header,
        decl.span,
        &items,
        |field: &TrackField<'_>| track_field_span(field),
        |_| 0,
        Reorder::No,
        print_track_field,
    );
}

fn print_track_field(ctx: &mut Ctx<'_>, field: &TrackField<'_>) {
    match field {
        TrackField::Instrument(instrument) => ctx
            .printer
            .line(format!("instrument {}", ctx.text(instrument.span))),
        TrackField::Volume(volume) => ctx.printer.line(format!(
            "volume {}{}",
            volume.decibels,
            ctx.text(volume.unit.span)
        )),
        TrackField::Play(play) => print_play(ctx, play),
        TrackField::Layer(uses, span) => print_layer(ctx, uses, *span),
        TrackField::Effect(effect) => match effect {
            TrackEffect::Inline(effect) => print_effect(ctx, "effect", effect),
            TrackEffect::Preset(name) => {
                ctx.printer.line(format!("effect {}", ctx.text(name.span)));
            }
        },
        TrackField::Automate(automate) => print_automate(ctx, automate),
    }
}

#[derive(Clone, Copy)]
enum EffectField<'a> {
    Mix(EffectFactor),
    Time(DurationExpression),
    Feedback(EffectFactor),
    Cutoff(&'a RateLiteral),
    Resonance(EffectFactor),
    Size(EffectFactor),
}

fn effect_field_span(field: EffectField<'_>) -> SourceSpan {
    match field {
        EffectField::Mix(factor)
        | EffectField::Feedback(factor)
        | EffectField::Resonance(factor)
        | EffectField::Size(factor) => factor.span,
        EffectField::Time(duration) => duration.span(),
        EffectField::Cutoff(cutoff) => cutoff.span,
    }
}

/// Prints a song-level `effect <name> = <kind> { ... }` preset.
fn print_effect_preset(ctx: &mut Ctx<'_>, decl: &EffectPresetDeclaration) {
    let header = format!("effect {} =", ctx.text(decl.name.span));
    print_effect(ctx, &header, &decl.effect);
}

/// Prints an effect block under `lead`, which is `effect` for a track's own
/// block and `effect <name> =` for a song-level preset.
fn print_effect(ctx: &mut Ctx<'_>, lead: &str, effect: &EffectDeclaration) {
    let (kind, items): (&str, Vec<EffectField<'_>>) = match &effect.kind {
        EffectKind::Delay {
            mix,
            time,
            feedback,
        } => (
            "delay",
            vec![
                EffectField::Mix(*mix),
                EffectField::Time(*time),
                EffectField::Feedback(*feedback),
            ],
        ),
        EffectKind::Filter { cutoff, resonance } => (
            "filter",
            vec![
                EffectField::Cutoff(cutoff),
                EffectField::Resonance(*resonance),
            ],
        ),
        EffectKind::Reverb { mix, size } => (
            "reverb",
            vec![EffectField::Mix(*mix), EffectField::Size(*size)],
        ),
    };
    let header = format!("{lead} {kind}");
    print_block(
        ctx,
        &header,
        effect.span,
        &items,
        effect_field_span,
        |_| 0,
        Reorder::No,
        |ctx, field| match field {
            EffectField::Mix(factor) => ctx.printer.line(format!("mix {}", factor.value)),
            EffectField::Time(duration) => {
                ctx.printer
                    .line(format!("time {}", format_duration(&duration)));
            }
            EffectField::Feedback(factor) => {
                ctx.printer.line(format!("feedback {}", factor.value));
            }
            EffectField::Cutoff(cutoff) => {
                ctx.printer
                    .line(format!("cutoff {}", rate_text(ctx, cutoff)));
            }
            EffectField::Resonance(factor) => {
                ctx.printer.line(format!("resonance {}", factor.value));
            }
            EffectField::Size(factor) => {
                ctx.printer.line(format!("size {}", factor.value));
            }
        },
    );
}

fn print_automate(ctx: &mut Ctx<'_>, decl: &AutomateDeclaration) {
    let items: Vec<&AutomateDeclaration> = vec![decl];
    print_block(
        ctx,
        "automate cutoff",
        decl.span,
        &items,
        |decl: &AutomateDeclaration| decl.span,
        |_| 0,
        Reorder::No,
        print_lfo,
    );
}

#[derive(Clone, Copy)]
enum LfoField<'a> {
    Range {
        start: &'a RateLiteral,
        end: &'a RateLiteral,
        span: SourceSpan,
    },
    Rate(&'a NumberLiteral),
}

fn lfo_field_span(field: LfoField<'_>) -> SourceSpan {
    match field {
        LfoField::Range { span, .. } => span,
        LfoField::Rate(rate) => rate.span,
    }
}

fn print_lfo(ctx: &mut Ctx<'_>, decl: &AutomateDeclaration) {
    let lfo: &LfoDeclaration = &decl.lfo;
    let header = format!("lfo {}", ctx.text(lfo.waveform.span));
    let items = [
        LfoField::Range {
            start: &lfo.range_start,
            end: &lfo.range_end,
            span: lfo.range_start.span.cover(lfo.range_end.span),
        },
        LfoField::Rate(&lfo.rate),
    ];
    print_block(
        ctx,
        &header,
        lfo.span,
        &items,
        lfo_field_span,
        |_| 0,
        Reorder::No,
        |ctx, field| match field {
            LfoField::Range { start, end, .. } => {
                ctx.printer.line(format!(
                    "range {}..{}",
                    rate_text(ctx, start),
                    rate_text(ctx, end)
                ));
            }
            LfoField::Rate(rate) => {
                ctx.printer.line(format!("rate {} cycles/bar", rate.value));
            }
        },
    );
}

fn print_layer(ctx: &mut Ctx<'_>, uses: &[LayerUse], span: SourceSpan) {
    let items: Vec<&LayerUse> = uses.iter().collect();
    print_block(
        ctx,
        "layer",
        span,
        &items,
        |layer_use: &LayerUse| layer_use.span,
        |_| 0,
        Reorder::No,
        print_layer_use,
    );
}

fn print_layer_use(ctx: &mut Ctx<'_>, layer_use: &LayerUse) {
    let header = format!("use {}", ctx.text(layer_use.instrument.span));
    let items: Vec<&PlayStatement> = vec![&layer_use.play];
    print_block(
        ctx,
        &header,
        layer_use.span,
        &items,
        |play: &PlayStatement| play.span,
        |_| 0,
        Reorder::No,
        print_play,
    );
}

fn print_play(ctx: &mut Ctx<'_>, play: &PlayStatement) {
    let mut line = match &play.at {
        Some(at) => format!("at {}:{} ", at.bar, at.beat),
        None => String::new(),
    };
    match &play.source {
        PlaySource::Pattern(identifier) => {
            let _ = write!(line, "play {}", ctx.text(identifier.span));
        }
        PlaySource::Drum { name, rhythm, .. } => {
            let _ = write!(
                line,
                "play drum {} with {}",
                ctx.text(name.span),
                ctx.text(rhythm.span)
            );
        }
    }
    if let Some(trigger_with) = &play.trigger_with {
        line.push_str(" |> trigger_with ");
        line.push_str(ctx.text(trigger_with.span));
    }
    if let Some(gate) = &play.gate {
        let _ = write!(line, " |> gate {}%", gate.percent);
    }
    if let Some(transpose) = &play.transpose {
        let _ = write!(
            line,
            " |> transpose {}{}",
            transpose.semitones,
            ctx.text(transpose.unit.span)
        );
    }
    if let Some(gain) = &play.gain {
        let _ = write!(line, " |> gain {}", gain.factor);
    }
    if let Some(repeat) = &play.repeat {
        let _ = write!(line, " |> repeat {}", repeat_text(repeat.count));
    }
    if play.reverse {
        line.push_str(" |> reverse");
    }
    if let Some(chance) = &play.chance {
        let _ = write!(line, " |> chance {}% {{", chance.percent);
        match &chance.transform {
            ChanceTransformExpression::Transpose(transpose) => {
                let _ = write!(
                    line,
                    " transpose {}{}",
                    transpose.semitones,
                    ctx.text(transpose.unit.span)
                );
            }
            ChanceTransformExpression::Retrigger { count, .. } => {
                let _ = write!(line, " retrigger {count}");
            }
            ChanceTransformExpression::Speed { factor, .. } => {
                let _ = write!(line, " speed {factor}");
            }
        }
        line.push_str(" }");
    }
    if let Some(speed) = play.speed {
        match speed {
            SpeedExpression::Fixed { factor, .. } => {
                let _ = write!(line, " |> speed {factor}");
            }
            SpeedExpression::Alternate {
                first_factor,
                second_factor,
                ..
            } => {
                let _ = write!(
                    line,
                    " |> alternate {{ speed {first_factor} speed {second_factor} }}"
                );
            }
        }
    }
    if let Some(pan) = play.pan {
        match pan {
            PanExpression::Fixed { percent, .. } => {
                let _ = write!(line, " |> pan {percent}%");
            }
            PanExpression::Alternate {
                left_percent,
                right_percent,
                ..
            } => {
                let _ = write!(line, " |> pan alternate({left_percent}%, {right_percent}%)");
            }
        }
    }
    if let Some(choose_sample) = &play.choose_sample {
        let _ = write!(
            line,
            " |> choose_sample {}..{}",
            choose_sample.start, choose_sample.end
        );
    }
    ctx.printer.line(line);
}

// --- pattern -------------------------------------------------------------

fn sequence_item_span(item: &SequenceItem) -> SourceSpan {
    match item {
        SequenceItem::Note(note) => note.span,
        SequenceItem::Chord(chord) => chord.span,
        SequenceItem::Rest(rest) => rest.span,
        SequenceItem::Repeat(group) => group.span,
    }
}

fn step_item_span(item: &StepItem) -> SourceSpan {
    match item {
        StepItem::Degree { span, .. }
        | StepItem::Sample { span, .. }
        | StepItem::Drum { span, .. }
        | StepItem::Rest { span }
        | StepItem::Choose { span, .. }
        | StepItem::ChooseDegrees { span, .. }
        | StepItem::Subdivide { span, .. } => *span,
        StepItem::Repeat(group) => group.span,
    }
}

#[derive(Clone, Copy)]
enum ArpeggiateField<'a> {
    Style(&'a Identifier),
    Step(&'a DurationExpression),
    Octaves(&'a OctavesExpression),
}

fn arpeggiate_field_span(field: ArpeggiateField<'_>) -> SourceSpan {
    match field {
        ArpeggiateField::Style(style) => style.span,
        ArpeggiateField::Step(step) => step.span(),
        ArpeggiateField::Octaves(octaves) => octaves.span,
    }
}

/// Prints `pattern <name> = arpeggiate <source> { ... }`, split out of
/// [`print_pattern`] so that function stays readable as pattern bodies grow.
fn print_arpeggiate(
    ctx: &mut Ctx<'_>,
    decl: &PatternDeclaration,
    source: &Identifier,
    fields: (&Identifier, &DurationExpression, Option<&OctavesExpression>),
    span: SourceSpan,
) {
    let (style, step, octaves) = fields;
    let header = format!(
        "pattern {} = arpeggiate {}",
        ctx.text(decl.name.span),
        ctx.text(source.span)
    );
    let mut items = vec![ArpeggiateField::Style(style), ArpeggiateField::Step(step)];
    if let Some(octaves) = octaves {
        items.push(ArpeggiateField::Octaves(octaves));
    }
    print_block(
        ctx,
        &header,
        span,
        &items,
        arpeggiate_field_span,
        |_| 0,
        Reorder::No,
        |ctx, field| match field {
            ArpeggiateField::Style(style) => {
                ctx.printer.line(format!("style {}", ctx.text(style.span)));
            }
            ArpeggiateField::Step(step) => {
                ctx.printer.line(format!("step {}", format_duration(step)));
            }
            ArpeggiateField::Octaves(octaves) => {
                ctx.printer.line(format!("octaves {}", octaves.count));
            }
        },
    );
}

fn print_pattern(ctx: &mut Ctx<'_>, decl: &PatternDeclaration) {
    match &decl.body {
        PatternBody::Sequence { step, items, .. } => {
            let step = step.map_or_else(String::new, |step| {
                format!(" step {}", format_duration(&step))
            });
            let header = format!("pattern {} = sequence{step}", ctx.text(decl.name.span));
            let items: Vec<&SequenceItem> = items.iter().collect();
            print_block(
                ctx,
                &header,
                decl.span,
                &items,
                sequence_item_span,
                |_| 0,
                Reorder::No,
                print_sequence_item,
            );
        }
        PatternBody::Arpeggiate {
            source,
            style,
            step,
            octaves,
            span,
        } => print_arpeggiate(ctx, decl, source, (style, step, octaves.as_ref()), *span),
        PatternBody::Derived {
            source,
            transpose,
            repeat,
            reverse,
            ..
        } => {
            let mut line = format!(
                "pattern {} = {}",
                ctx.text(decl.name.span),
                ctx.text(source.span)
            );
            if let Some(transpose) = transpose {
                let _ = write!(
                    line,
                    " |> transpose {}{}",
                    transpose.semitones,
                    ctx.text(transpose.unit.span)
                );
            }
            if let Some(repeat) = repeat {
                let _ = write!(line, " |> repeat {}", repeat_text(repeat.count));
            }
            if *reverse {
                line.push_str(" |> reverse");
            }
            ctx.printer.line(line);
        }
        PatternBody::Steps {
            resolution, items, ..
        } => {
            let header = format!(
                "pattern {} = steps {}",
                ctx.text(decl.name.span),
                format_duration(resolution)
            );
            let items: Vec<&StepItem> = items.iter().collect();
            print_block(
                ctx,
                &header,
                decl.span,
                &items,
                step_item_span,
                |_| 0,
                Reorder::No,
                print_step_item,
            );
        }
    }
}

fn print_sequence_item(ctx: &mut Ctx<'_>, item: &SequenceItem) {
    let line = sequence_item_text(ctx, item);
    ctx.printer.line(line);
}

fn sequence_item_text(ctx: &Ctx<'_>, item: &SequenceItem) -> String {
    match item {
        SequenceItem::Note(note) => note_text(ctx, note),
        SequenceItem::Chord(chord) => chord_text(ctx, chord),
        SequenceItem::Rest(rest) => format!("rest{}", format_for(rest.duration.as_ref())),
        SequenceItem::Repeat(group) => format_repeat(group, |item| sequence_item_text(ctx, item)),
    }
}

/// `velocity 90`, or the ramp form `velocity 70..110`.
fn velocity_text(velocity: &VelocityExpression) -> String {
    match velocity.ramp_to {
        Some(end) => format!("velocity {}..{end}", velocity.value),
        None => format!("velocity {}", velocity.value),
    }
}

/// ` for <duration>`, or nothing when the item takes its sequence's `step`.
fn format_for(duration: Option<&DurationExpression>) -> String {
    duration.map_or_else(String::new, |duration| {
        format!(" for {}", format_duration(duration))
    })
}

/// `repeat 2`'s count, or the `fit` that stands in for one.
fn repeat_text(count: RepeatCount) -> String {
    match count {
        RepeatCount::Fixed(count) => count.to_string(),
        RepeatCount::Fit => "fit".to_owned(),
    }
}

fn format_duration(duration: &DurationExpression) -> String {
    match *duration {
        DurationExpression::Fraction {
            numerator,
            denominator,
            ..
        } => format!("{numerator}/{denominator}"),
        DurationExpression::Bars { count, .. } => format!("{count}bar"),
    }
}

fn note_text(ctx: &Ctx<'_>, note: &NoteExpression) -> String {
    let mut line = format!(
        "note {}{}",
        ctx.text(note.pitch.span),
        format_for(note.duration.as_ref())
    );
    if let Some(velocity) = &note.velocity {
        let _ = write!(line, " {}", velocity_text(velocity));
    }
    line
}

fn chord_text(ctx: &Ctx<'_>, chord: &ChordExpression) -> String {
    let pitches = match &chord.pitches {
        ChordPitches::Explicit(pitches) => pitches
            .iter()
            .map(|pitch| ctx.text(pitch.span))
            .collect::<Vec<_>>()
            .join(" "),
        ChordPitches::Symbol { root, quality } => {
            format!("{}:{}", ctx.text(root.span), ctx.text(quality.span))
        }
    };
    let mut line = format!("chord {pitches}{}", format_for(chord.duration.as_ref()));
    if let Some(velocity) = &chord.velocity {
        let _ = write!(line, " {}", velocity_text(velocity));
    }
    line
}

/// Renders every step item that fits on one line. `choose` blocks are the
/// exception, and the parser rejects them inside a repetition for exactly
/// that reason, so a repetition never has to render one inline.
fn step_item_text(ctx: &Ctx<'_>, item: &StepItem) -> String {
    match item {
        StepItem::Degree { degree, octave, .. } => format!("degree {degree} octave {octave}"),
        StepItem::Sample {
            index, velocity, ..
        } => {
            let mut line = format!("sample {index}");
            if let Some(velocity) = velocity {
                let _ = write!(line, " {}", velocity_text(velocity));
            }
            line
        }
        StepItem::Drum { name, velocity, .. } => {
            let mut line = format!("drum {}", ctx.text(name.span));
            if let Some(velocity) = velocity {
                let _ = write!(line, " {}", velocity_text(velocity));
            }
            line
        }
        StepItem::Rest { .. } => "rest".to_owned(),
        StepItem::Repeat(group) => format_repeat(group, |item| step_item_text(ctx, item)),
        StepItem::Subdivide { items, .. } => {
            let items = items
                .iter()
                .map(|item| step_item_text(ctx, item))
                .collect::<Vec<_>>()
                .join(" ");
            format!("[{items}]")
        }
        StepItem::Choose { .. } | StepItem::ChooseDegrees { .. } => {
            unreachable!("choose is not repeatable, and is printed as a block")
        }
    }
}

fn print_step_item(ctx: &mut Ctx<'_>, item: &StepItem) {
    match item {
        StepItem::Degree { .. }
        | StepItem::Sample { .. }
        | StepItem::Drum { .. }
        | StepItem::Rest { .. }
        | StepItem::Repeat(_)
        | StepItem::Subdivide { .. } => {
            let line = step_item_text(ctx, item);
            ctx.printer.line(line);
        }
        StepItem::Choose { alternatives, span } => {
            let items: Vec<&SampleChoiceAlternative> = alternatives.iter().collect();
            print_block(
                ctx,
                "choose",
                *span,
                &items,
                |alternative: &SampleChoiceAlternative| alternative.span,
                |_| 0,
                Reorder::No,
                print_sample_choice_alternative,
            );
        }
        StepItem::ChooseDegrees { alternatives, span } => {
            let items: Vec<&DegreeChoiceAlternative> = alternatives.iter().collect();
            print_block(
                ctx,
                "choose",
                *span,
                &items,
                |alternative: &DegreeChoiceAlternative| alternative.span,
                |_| 0,
                Reorder::No,
                print_degree_choice_alternative,
            );
        }
    }
}

fn print_degree_choice_alternative(ctx: &mut Ctx<'_>, alternative: &DegreeChoiceAlternative) {
    ctx.printer.line(format!(
        "degree {} octave {} weight {}",
        alternative.degree, alternative.octave, alternative.weight
    ));
}

/// A single sample index collapses to the short `sample N weight W` form
/// regardless of whether it was originally written that way or as a
/// one-element `sequence { sample N }`: the two spellings are equivalent,
/// and the AST does not distinguish which one the author used, so the
/// formatter always normalizes to the shorter form.
fn print_sample_choice_alternative(ctx: &mut Ctx<'_>, alternative: &SampleChoiceAlternative) {
    if let [selector] = &alternative.selectors[..] {
        let text = selector_text(ctx, selector);
        ctx.printer
            .line(format!("{text} weight {}", alternative.weight));
        return;
    }
    ctx.printer
        .line(format!("sequence weight {} {{", alternative.weight));
    ctx.printer.indent();
    for selector in &alternative.selectors {
        let text = selector_text(ctx, selector);
        ctx.printer.line(text);
    }
    ctx.printer.dedent();
    ctx.printer.line("}");
}

fn selector_text(ctx: &mut Ctx<'_>, selector: &SampleSelectorExpression) -> String {
    match selector {
        SampleSelectorExpression::Index(index) => format!("sample {index}"),
        SampleSelectorExpression::Named(name) => format!("drum {}", ctx.text(name.span)),
    }
}
