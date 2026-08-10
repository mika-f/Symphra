use std::fmt::Write as _;

use symphra_syntax::SourceSpan;
use symphra_syntax::ast::{
    ArrangementEntry, ArrangementOccurrence, ChanceTransformExpression, ChordExpression,
    Declaration, DegreeChoiceAlternative, DurationExpression, EffectDeclaration, EffectFactor,
    EffectKind, Identifier, InstrumentBody, InstrumentDeclaration, LayerUse, MasterDeclaration,
    NoteExpression, PanExpression, PatternBody, PatternDeclaration, PlaySource, PlayStatement,
    ProjectDeclaration, ProjectStatement, QuotedString, RateLiteral, RhythmDeclaration, RhythmItem,
    SampleChoiceAlternative, SampleSelectorExpression, SectionDeclaration, SequenceItem,
    SongDeclaration, SongStatement, SourceFile, SpeedExpression, StepItem, TrackBody,
    TrackDeclaration, VolumeExpression,
};

use crate::printer::Printer;
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
/// rank order after printing. `GroupedWithSeparator` additionally forces a
/// blank line at every rank boundary; see [`Printer::reorder_since`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reorder {
    No,
    Yes,
    YesWithSeparator,
}

pub fn format_source_file(ctx: &mut Ctx<'_>, file: &SourceFile) {
    let items: Vec<&Declaration> = file.declarations.iter().collect();
    let dangling = print_items(
        ctx,
        file.span.start,
        &items,
        declaration_span,
        file.span.end,
        declaration_rank,
        Reorder::YesWithSeparator,
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
        Reorder::Yes => ctx.printer.reorder_since(block_start, ranges, false),
        Reorder::YesWithSeparator => ctx.printer.reorder_since(block_start, ranges, true),
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
        Reorder::Yes,
        print_song_statement,
    );
}

fn song_statement_rank(statement: &SongStatement) -> u8 {
    match statement {
        SongStatement::Tempo { .. } => 0,
        SongStatement::Meter { .. } => 1,
        SongStatement::Key { .. } => 2,
        SongStatement::Instrument(_) => 3,
        SongStatement::Rhythm(_) => 4,
        SongStatement::Pattern(_) => 5,
        SongStatement::Track(_) => 6,
        SongStatement::Section(_) => 7,
        SongStatement::Arrangement { .. } => 8,
        SongStatement::Master(_) => 9,
    }
}

fn song_statement_span(statement: &SongStatement) -> SourceSpan {
    match statement {
        SongStatement::Tempo { span, .. }
        | SongStatement::Meter { span, .. }
        | SongStatement::Key { span, .. }
        | SongStatement::Arrangement { span, .. } => *span,
        SongStatement::Instrument(decl) => decl.span,
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
    let items: Vec<&Identifier> = decl.tracks.iter().collect();
    print_block(
        ctx,
        header,
        decl.span,
        &items,
        |track: &Identifier| track.span,
        |_| 0,
        Reorder::No,
        |ctx, track| {
            ctx.printer
                .line(format!("play track {}", ctx.text(track.span)));
        },
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
                "ceiling {} {}",
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

fn print_instrument(ctx: &mut Ctx<'_>, decl: &InstrumentDeclaration) {
    let name = ctx.text(decl.name.span).to_owned();
    match &decl.body {
        InstrumentBody::Builtin(kind) => {
            ctx.printer
                .line(format!("instrument {name} = {}", ctx.text(kind.span)));
        }
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
    }
}

// --- rhythm ----------------------------------------------------------------

fn rhythm_item_span(item: &RhythmItem) -> SourceSpan {
    match item {
        RhythmItem::Hit { span } | RhythmItem::Rest { span } => *span,
    }
}

fn print_rhythm(ctx: &mut Ctx<'_>, decl: &RhythmDeclaration) {
    let header = format!(
        "rhythm {} resolution {}/{}",
        ctx.text(decl.name.span),
        decl.resolution_numerator,
        decl.resolution_denominator
    );
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
            ctx.printer.line(match item {
                RhythmItem::Hit { .. } => "hit",
                RhythmItem::Rest { .. } => "rest",
            });
        },
    );
}

// --- track -------------------------------------------------------------

enum TrackField<'a> {
    Instrument(&'a Identifier),
    Volume(&'a VolumeExpression),
    Play(&'a PlayStatement),
    Layer(&'a [LayerUse], SourceSpan),
    Effect(&'a EffectDeclaration),
}

fn track_field_span(field: &TrackField<'_>) -> SourceSpan {
    match field {
        TrackField::Instrument(instrument) => instrument.span,
        TrackField::Volume(volume) => volume.span,
        TrackField::Play(play) => play.span,
        TrackField::Layer(_, span) => *span,
        TrackField::Effect(effect) => effect.span,
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
    let items: Vec<&TrackField<'_>> = fields.iter().collect();
    print_block(
        ctx,
        &header,
        decl.span,
        &items,
        |field: &TrackField<'_>| track_field_span(field),
        |_| 0,
        Reorder::No,
        |ctx, field| match field {
            TrackField::Instrument(instrument) => ctx
                .printer
                .line(format!("instrument {}", ctx.text(instrument.span))),
            TrackField::Volume(volume) => ctx.printer.line(format!(
                "volume {} {}",
                volume.decibels,
                ctx.text(volume.unit.span)
            )),
            TrackField::Play(play) => print_play(ctx, play),
            TrackField::Layer(uses, span) => print_layer(ctx, uses, *span),
            TrackField::Effect(effect) => print_effect(ctx, effect),
        },
    );
}

#[derive(Clone, Copy)]
enum EffectField<'a> {
    Mix(EffectFactor),
    Time(DurationExpression),
    Feedback(EffectFactor),
    Cutoff(&'a RateLiteral),
    Resonance(EffectFactor),
}

fn effect_field_span(field: EffectField<'_>) -> SourceSpan {
    match field {
        EffectField::Mix(factor)
        | EffectField::Feedback(factor)
        | EffectField::Resonance(factor) => factor.span,
        EffectField::Time(duration) => duration.span(),
        EffectField::Cutoff(cutoff) => cutoff.span,
    }
}

fn print_effect(ctx: &mut Ctx<'_>, effect: &EffectDeclaration) {
    let (header, items): (&str, Vec<EffectField<'_>>) = match &effect.kind {
        EffectKind::Delay {
            mix,
            time,
            feedback,
        } => (
            "effect delay",
            vec![
                EffectField::Mix(*mix),
                EffectField::Time(*time),
                EffectField::Feedback(*feedback),
            ],
        ),
        EffectKind::Filter { cutoff, resonance } => (
            "effect filter",
            vec![
                EffectField::Cutoff(cutoff),
                EffectField::Resonance(*resonance),
            ],
        ),
    };
    print_block(
        ctx,
        header,
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
            " |> transpose {} {}",
            transpose.semitones,
            ctx.text(transpose.unit.span)
        );
    }
    if let Some(gain) = &play.gain {
        let _ = write!(line, " |> gain {}", gain.factor);
    }
    if let Some(repeat) = &play.repeat {
        let _ = write!(line, " |> repeat {}", repeat.count);
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
                    " transpose {} {}",
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
    }
}

fn step_item_span(item: &StepItem) -> SourceSpan {
    match item {
        StepItem::Degree { span, .. }
        | StepItem::Sample { span, .. }
        | StepItem::Drum { span, .. }
        | StepItem::Rest { span }
        | StepItem::Choose { span, .. }
        | StepItem::ChooseDegrees { span, .. } => *span,
    }
}

fn print_pattern(ctx: &mut Ctx<'_>, decl: &PatternDeclaration) {
    match &decl.body {
        PatternBody::Sequence { items, .. } => {
            let header = format!("pattern {} = sequence", ctx.text(decl.name.span));
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
        PatternBody::Steps {
            resolution_numerator,
            resolution_denominator,
            items,
            ..
        } => {
            let header = format!(
                "pattern {} = steps {}/{}",
                ctx.text(decl.name.span),
                resolution_numerator,
                resolution_denominator
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
    match item {
        SequenceItem::Note(note) => print_note(ctx, note),
        SequenceItem::Chord(chord) => print_chord(ctx, chord),
        SequenceItem::Rest(rest) => ctx
            .printer
            .line(format!("rest for {}", format_duration(&rest.duration))),
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

fn print_note(ctx: &mut Ctx<'_>, note: &NoteExpression) {
    let mut line = format!(
        "note {} for {}",
        ctx.text(note.pitch.span),
        format_duration(&note.duration)
    );
    if let Some(velocity) = &note.velocity {
        let _ = write!(line, " velocity {}", velocity.value);
    }
    ctx.printer.line(line);
}

fn print_chord(ctx: &mut Ctx<'_>, chord: &ChordExpression) {
    let pitches = chord
        .pitches
        .iter()
        .map(|pitch| ctx.text(pitch.span))
        .collect::<Vec<_>>()
        .join(" ");
    let mut line = format!("chord {pitches} for {}", format_duration(&chord.duration));
    if let Some(velocity) = &chord.velocity {
        let _ = write!(line, " velocity {}", velocity.value);
    }
    ctx.printer.line(line);
}

fn print_step_item(ctx: &mut Ctx<'_>, item: &StepItem) {
    match item {
        StepItem::Degree { degree, octave, .. } => {
            ctx.printer.line(format!("degree {degree} octave {octave}"));
        }
        StepItem::Sample {
            index, velocity, ..
        } => {
            let mut line = format!("sample {index}");
            if let Some(velocity) = velocity {
                let _ = write!(line, " velocity {}", velocity.value);
            }
            ctx.printer.line(line);
        }
        StepItem::Drum { name, velocity, .. } => {
            let mut line = format!("drum {}", ctx.text(name.span));
            if let Some(velocity) = velocity {
                let _ = write!(line, " velocity {}", velocity.value);
            }
            ctx.printer.line(line);
        }
        StepItem::Rest { .. } => ctx.printer.line("rest"),
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
