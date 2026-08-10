use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use symphra_compiler::compile;
use symphra_syntax::ast::{Declaration, PatternBody, SequenceItem, SongStatement};
use symphra_syntax::{
    SourceId, SourcePosition, SourceSpan, SourceText, Token, TokenKind, lex, parse,
};
use tokio::sync::RwLock;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentFormattingParams, DocumentSymbol, DocumentSymbolParams,
    DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents,
    HoverParams, HoverProviderCapability, InitializeParams, InitializeResult, Location,
    MarkupContent, MarkupKind, OneOf, Position, PositionEncodingKind, Range, ServerCapabilities,
    ServerInfo, SymbolInformation, SymbolKind, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextEdit, Uri,
};
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

#[derive(Debug)]
struct Backend {
    client: Client,
    documents: RwLock<HashMap<Uri, SourceText>>,
    hierarchical_symbols: AtomicBool,
}

impl Backend {
    async fn update(&self, uri: Uri, version: i32, text: String) {
        let source = SourceText::new(SourceId(0), uri.as_str(), text);
        let diagnostics = diagnostics(&source);
        self.documents.write().await.insert(uri.clone(), source);
        self.client
            .publish_diagnostics(uri, diagnostics, Some(version))
            .await;
    }
}

impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let hierarchical_symbols = params
            .capabilities
            .text_document
            .and_then(|capabilities| capabilities.document_symbol)
            .and_then(|capabilities| capabilities.hierarchical_document_symbol_support)
            .unwrap_or(false);
        self.hierarchical_symbols
            .store(hierarchical_symbols, Ordering::Relaxed);

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                position_encoding: Some(PositionEncodingKind::UTF16),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                document_symbol_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions::default()),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: "Symphra".to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
            offset_encoding: None,
        })
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let document = params.text_document;
        self.update(document.uri, document.version, document.text)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let document = params.text_document;
        if let Some(change) = params.content_changes.into_iter().last() {
            self.update(document.uri, document.version, change.text)
                .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents
            .write()
            .await
            .remove(&params.text_document.uri);
        self.client
            .publish_diagnostics(params.text_document.uri, Vec::new(), None)
            .await;
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let documents = self.documents.read().await;
        let Some(source) = documents.get(&params.text_document.uri) else {
            return Ok(None);
        };

        let symbols = document_symbols(source);
        Ok(Some(if self.hierarchical_symbols.load(Ordering::Relaxed) {
            DocumentSymbolResponse::Nested(symbols)
        } else {
            DocumentSymbolResponse::Flat(flatten_document_symbols(
                &params.text_document.uri,
                &symbols,
            ))
        }))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let documents = self.documents.read().await;
        let position = params.text_document_position.position;
        let uri = params.text_document_position.text_document.uri;
        let Some(source) = documents.get(&uri) else {
            return Ok(None);
        };

        Ok(Some(CompletionResponse::Array(completions(
            source, position,
        ))))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let documents = self.documents.read().await;
        let position = params.text_document_position_params.position;
        let uri = params.text_document_position_params.text_document.uri;
        Ok(documents
            .get(&uri)
            .and_then(|source| hover(source, position)))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let documents = self.documents.read().await;
        let position = params.text_document_position_params.position;
        let uri = params.text_document_position_params.text_document.uri;
        Ok(documents.get(&uri).and_then(|source| {
            definition(source, &uri, position).map(GotoDefinitionResponse::Scalar)
        }))
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let documents = self.documents.read().await;
        let uri = params.text_document.uri;
        Ok(documents.get(&uri).map(formatting_edits))
    }
}

/// Replaces the whole document with its canonical form.
///
/// Returns no edits when the source already has lexical or syntax
/// diagnostics: reprinting an AST produced alongside diagnostics is not
/// safe, since parse recovery may have skipped or misparsed tokens. This
/// matches `symphra-fmt::format_source`'s own refusal to format such
/// source, so a syntax error simply leaves the document unformatted rather
/// than surfacing a separate formatting failure to the editor.
fn formatting_edits(source: &SourceText) -> Vec<TextEdit> {
    let Ok(formatted) = symphra_fmt::format_source(&source.text) else {
        return Vec::new();
    };
    if formatted == source.text {
        return Vec::new();
    }
    let Some(range) = lsp_range(source, SourceSpan::new(source.id, 0..source.text.len())) else {
        return Vec::new();
    };
    vec![TextEdit {
        range,
        new_text: formatted,
    }]
}

fn diagnostics(source: &SourceText) -> Vec<Diagnostic> {
    let parsed = parse(source.id, &source.text);
    let diagnostics: Vec<(String, SourceSpan)> = if parsed.diagnostics.is_empty() {
        match compile(&parsed.file) {
            Ok(_) => return Vec::new(),
            Err(diagnostics) => diagnostics
                .into_iter()
                .map(|diagnostic| (diagnostic.message, diagnostic.span))
                .collect(),
        }
    } else {
        parsed
            .diagnostics
            .into_iter()
            .map(|diagnostic| (diagnostic.message, diagnostic.span))
            .collect()
    };

    diagnostics
        .into_iter()
        .filter_map(|(message, span)| lsp_diagnostic(source, message, span))
        .collect()
}

fn lsp_diagnostic(source: &SourceText, message: String, span: SourceSpan) -> Option<Diagnostic> {
    Some(Diagnostic {
        range: lsp_range(source, span)?,
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("symphra".to_owned()),
        message,
        ..Diagnostic::default()
    })
}

fn lsp_range(source: &SourceText, span: SourceSpan) -> Option<Range> {
    let range = source.utf16_range(span)?;
    Some(Range {
        start: Position::new(range.start.line, range.start.utf16_column),
        end: Position::new(range.end.line, range.end.utf16_column),
    })
}

fn document_symbols(source: &SourceText) -> Vec<DocumentSymbol> {
    parse(source.id, &source.text)
        .file
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::Project(project) => symbol(
                source,
                "project".to_owned(),
                SymbolKind::NAMESPACE,
                project.span,
                project.span,
                None,
            ),
            Declaration::Song(song) => {
                let children = song
                    .statements
                    .iter()
                    .filter_map(|statement| match statement {
                        SongStatement::Pattern(pattern) => symbol(
                            source,
                            pattern.name.text.clone(),
                            SymbolKind::FUNCTION,
                            pattern.span,
                            pattern.name.span,
                            None,
                        ),
                        SongStatement::Instrument(instrument) => symbol(
                            source,
                            instrument.name.text.clone(),
                            SymbolKind::OBJECT,
                            instrument.span,
                            instrument.name.span,
                            None,
                        ),
                        SongStatement::Rhythm(rhythm) => symbol(
                            source,
                            rhythm.name.text.clone(),
                            SymbolKind::FUNCTION,
                            rhythm.span,
                            rhythm.name.span,
                            None,
                        ),
                        SongStatement::Track(track) => symbol(
                            source,
                            track.name.text.clone(),
                            SymbolKind::OBJECT,
                            track.span,
                            track.name.span,
                            None,
                        ),
                        _ => None,
                    })
                    .collect();
                symbol(
                    source,
                    song.name.value.clone(),
                    SymbolKind::MODULE,
                    song.span,
                    song.name.span,
                    Some(children),
                )
            }
        })
        .collect()
}

fn flatten_document_symbols(uri: &Uri, symbols: &[DocumentSymbol]) -> Vec<SymbolInformation> {
    fn append(
        output: &mut Vec<SymbolInformation>,
        uri: &Uri,
        symbols: &[DocumentSymbol],
        container_name: Option<&str>,
    ) {
        for symbol in symbols {
            output.push(symbol_information(uri, symbol, container_name));
            if let Some(children) = &symbol.children {
                append(output, uri, children, Some(&symbol.name));
            }
        }
    }

    let mut output = Vec::new();
    append(&mut output, uri, symbols, None);
    output
}

#[expect(deprecated, reason = "the LSP wire type still requires this field")]
fn symbol_information(
    uri: &Uri,
    symbol: &DocumentSymbol,
    container_name: Option<&str>,
) -> SymbolInformation {
    SymbolInformation {
        name: symbol.name.clone(),
        kind: symbol.kind,
        tags: None,
        deprecated: None,
        location: Location::new(uri.clone(), symbol.range),
        container_name: container_name.map(str::to_owned),
    }
}

#[expect(deprecated, reason = "the LSP wire type still requires this field")]
fn symbol(
    source: &SourceText,
    name: String,
    kind: SymbolKind,
    span: SourceSpan,
    selection_span: SourceSpan,
    children: Option<Vec<DocumentSymbol>>,
) -> Option<DocumentSymbol> {
    Some(DocumentSymbol {
        name,
        detail: None,
        kind,
        tags: None,
        deprecated: None,
        range: lsp_range(source, span)?,
        selection_range: lsp_range(source, selection_span)?,
        children,
    })
}

#[derive(Clone, Copy)]
enum CompletionBlock {
    Project,
    Song,
    Rhythm,
    Track,
    Arrangement,
    Sequence,
    Steps,
    Choice,
    ChoiceSequence,
    Sampled,
    Sampler,
    DrumMachine,
    Chance,
    Other,
}

fn completions(source: &SourceText, position: Position) -> Vec<CompletionItem> {
    let Some(byte_offset) = source.byte_offset_utf16(SourcePosition {
        line: position.line,
        utf16_column: position.character,
    }) else {
        return Vec::new();
    };
    let byte_offset = byte_offset as usize;
    let prefix = &source.text[..byte_offset];
    let tokens = lex(source.id, prefix).tokens;
    let tokens = &tokens[..tokens.len() - 1];
    let block = completion_block(tokens);
    let line_start = prefix.rfind('\n').map_or(0, |offset| offset + 1);
    let Ok(line_start) = u32::try_from(line_start) else {
        return Vec::new();
    };
    let line_token_start = tokens
        .iter()
        .rposition(|token| token.span.start < line_start || token.kind == TokenKind::LeftBrace)
        .map_or(0, |index| index + 1);
    let line_tokens = &tokens[line_token_start..];

    completion_labels(block, line_tokens)
        .iter()
        .map(|label| CompletionItem {
            label: (*label).to_owned(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Symphra keyword".to_owned()),
            ..CompletionItem::default()
        })
        .collect()
}

fn completion_labels(
    block: Option<CompletionBlock>,
    line_tokens: &[Token],
) -> &'static [&'static str] {
    if matches!(line_tokens.last(), Some(token) if token.kind == TokenKind::Equal) {
        if matches!(line_tokens.first(), Some(token) if token.kind == TokenKind::Instrument) {
            &["sine", "triangle", "sampled", "sampler", "drum_machine"]
        } else {
            &["sequence", "steps"]
        }
    } else if duration_keyword_follows(line_tokens) {
        &["for"]
    } else if matches!(
        line_tokens,
        [
            Token {
                kind: TokenKind::Degree,
                ..
            },
            Token {
                kind: TokenKind::Integer,
                ..
            }
        ]
    ) {
        &["octave"]
    } else if matches!(
        line_tokens,
        [
            Token {
                kind: TokenKind::Rhythm,
                ..
            },
            Token {
                kind: TokenKind::Identifier,
                ..
            }
        ]
    ) {
        &["resolution"]
    } else if matches!(
        line_tokens,
        [
            Token {
                kind: TokenKind::Track,
                ..
            },
            Token {
                kind: TokenKind::Identifier,
                ..
            }
        ]
    ) {
        &["role"]
    } else if matches!(line_tokens.last(), Some(token) if token.kind == TokenKind::Pan) {
        &["alternate"]
    } else if matches!(line_tokens.last(), Some(token) if token.kind == TokenKind::Play) {
        &["drum"]
    } else if matches!(line_tokens.last(), Some(token) if token.kind == TokenKind::PipeGreater) {
        &[
            "trigger_with",
            "gate",
            "transpose",
            "gain",
            "repeat",
            "reverse",
            "speed",
            "alternate",
            "pan",
            "chance",
            "choose_sample",
        ]
    } else if velocity_keyword_follows(line_tokens) {
        &["velocity"]
    } else if matches!(block, Some(CompletionBlock::Arrangement))
        && matches!(line_tokens.last(), Some(token) if token.kind == TokenKind::Identifier)
        && !matches!(line_tokens.get(line_tokens.len().saturating_sub(2)), Some(token) if token.kind == TokenKind::With)
    {
        &["with"]
    } else if completion_statement_start(line_tokens) {
        completion_block_labels(block)
    } else {
        &[]
    }
}

fn completion_block_labels(block: Option<CompletionBlock>) -> &'static [&'static str] {
    match block {
        None => &["project", "song"],
        Some(CompletionBlock::Project) => &["seed", "sample_rate", "output"],
        Some(CompletionBlock::Song) => &[
            "tempo",
            "meter",
            "key",
            "instrument",
            "rhythm",
            "track",
            "pattern",
            "arrangement",
        ],
        Some(CompletionBlock::Sequence) => &["note", "chord", "rest"],
        Some(CompletionBlock::Steps) => &["degree", "sample", "drum", "rest", "choose"],
        Some(CompletionBlock::Choice) => &["degree", "sample", "drum", "sequence"],
        Some(CompletionBlock::ChoiceSequence) => &["sample", "drum"],
        Some(CompletionBlock::Sampled) => &["source", "root"],
        Some(CompletionBlock::Sampler) => &["pack"],
        Some(CompletionBlock::DrumMachine) => &["bank"],
        Some(CompletionBlock::Chance) => &["transpose", "retrigger", "speed"],
        Some(CompletionBlock::Rhythm) => &["hit", "rest"],
        Some(CompletionBlock::Track) => &["instrument", "volume", "play", "at"],
        Some(CompletionBlock::Arrangement | CompletionBlock::Other) => &[],
    }
}

fn duration_keyword_follows(tokens: &[Token]) -> bool {
    matches!(
        tokens,
        [
            Token {
                kind: TokenKind::Note,
                ..
            },
            Token {
                kind: TokenKind::Identifier,
                ..
            }
        ]
    ) || (tokens.len() >= 3
        && tokens
            .first()
            .is_some_and(|token| token.kind == TokenKind::Chord)
        && tokens[1..]
            .iter()
            .all(|token| token.kind == TokenKind::Identifier))
}

fn velocity_keyword_follows(tokens: &[Token]) -> bool {
    matches!(
        (
            tokens.first(),
            tokens.get(tokens.len().saturating_sub(2)),
            tokens.last()
        ),
        (
            Some(Token {
                kind: TokenKind::Note | TokenKind::Chord,
                ..
            }),
            Some(Token {
                kind: TokenKind::Slash,
                ..
            }),
            Some(Token {
                kind: TokenKind::Integer,
                ..
            })
        )
    ) && tokens.iter().any(|token| token.kind == TokenKind::For)
}

fn completion_block(tokens: &[Token]) -> Option<CompletionBlock> {
    let mut pending = None;
    let mut blocks = Vec::new();
    for token in tokens {
        match token.kind {
            TokenKind::Project => pending = Some(CompletionBlock::Project),
            TokenKind::Song => pending = Some(CompletionBlock::Song),
            TokenKind::Rhythm => pending = Some(CompletionBlock::Rhythm),
            TokenKind::Track => pending = Some(CompletionBlock::Track),
            TokenKind::Arrangement => pending = Some(CompletionBlock::Arrangement),
            TokenKind::Sequence => {
                pending = Some(if matches!(blocks.last(), Some(CompletionBlock::Choice)) {
                    CompletionBlock::ChoiceSequence
                } else {
                    CompletionBlock::Sequence
                });
            }
            TokenKind::Steps => pending = Some(CompletionBlock::Steps),
            TokenKind::Choose => pending = Some(CompletionBlock::Choice),
            TokenKind::Sampled => pending = Some(CompletionBlock::Sampled),
            TokenKind::Sampler => pending = Some(CompletionBlock::Sampler),
            TokenKind::DrumMachine => pending = Some(CompletionBlock::DrumMachine),
            TokenKind::Chance => pending = Some(CompletionBlock::Chance),
            TokenKind::LeftBrace => blocks.push(pending.take().unwrap_or(CompletionBlock::Other)),
            TokenKind::RightBrace => {
                blocks.pop();
            }
            _ => {}
        }
    }
    blocks.last().copied()
}

fn completion_statement_start(tokens: &[Token]) -> bool {
    tokens.is_empty()
        || matches!(
            tokens,
            [Token {
                kind: TokenKind::Identifier
                    | TokenKind::Project
                    | TokenKind::Song
                    | TokenKind::Seed
                    | TokenKind::SampleRate
                    | TokenKind::Output
                    | TokenKind::Tempo
                    | TokenKind::Meter
                    | TokenKind::Key
                    | TokenKind::Instrument
                    | TokenKind::Rhythm
                    | TokenKind::Resolution
                    | TokenKind::Hit
                    | TokenKind::Track
                    | TokenKind::Role
                    | TokenKind::Volume
                    | TokenKind::Play
                    | TokenKind::TriggerWith
                    | TokenKind::Gate
                    | TokenKind::Transpose
                    | TokenKind::Gain
                    | TokenKind::Repeat
                    | TokenKind::Reverse
                    | TokenKind::Speed
                    | TokenKind::Retrigger
                    | TokenKind::Pan
                    | TokenKind::Alternate
                    | TokenKind::Chance
                    | TokenKind::ChooseSample
                    | TokenKind::At
                    | TokenKind::PipeGreater
                    | TokenKind::Percent
                    | TokenKind::LeftParen
                    | TokenKind::RightParen
                    | TokenKind::Comma
                    | TokenKind::Colon
                    | TokenKind::Plus
                    | TokenKind::Minus
                    | TokenKind::Pattern
                    | TokenKind::Arrangement
                    | TokenKind::Degree
                    | TokenKind::Octave
                    | TokenKind::Note
                    | TokenKind::Chord
                    | TokenKind::Rest
                    | TokenKind::Sample
                    | TokenKind::Drum
                    | TokenKind::Choose
                    | TokenKind::Weight
                    | TokenKind::Source
                    | TokenKind::Root
                    | TokenKind::Pack
                    | TokenKind::Bank,
                ..
            }]
        )
}

fn hover(source: &SourceText, position: Position) -> Option<Hover> {
    let offset = source.byte_offset_utf16(SourcePosition {
        line: position.line,
        utf16_column: position.character,
    })?;
    let token = lex(source.id, &source.text)
        .tokens
        .into_iter()
        .find(|token| token.span.start <= offset && offset < token.span.end)?;
    let description = keyword_description(token.kind)
        .map(str::to_owned)
        .or_else(|| pitch_description(source, token.span))?;
    let range = lsp_range(source, token.span)?;

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("`{}` — {description}", token.text),
        }),
        range: Some(range),
    })
}

fn definition(source: &SourceText, uri: &Uri, position: Position) -> Option<Location> {
    let offset = source.byte_offset_utf16(SourcePosition {
        line: position.line,
        utf16_column: position.character,
    })?;
    let parsed = parse(source.id, &source.text);

    for declaration in &parsed.file.declarations {
        let Declaration::Song(song) = declaration else {
            continue;
        };
        let occurrence = song.statements.iter().find_map(|statement| {
            let SongStatement::Arrangement { occurrences, .. } = statement else {
                return None;
            };
            occurrences
                .iter()
                .find(|occurrence| occurrence.span.start <= offset && offset < occurrence.span.end)
        });
        let Some(occurrence) = occurrence else {
            continue;
        };
        if occurrence.pattern.span.start <= offset && offset < occurrence.pattern.span.end {
            let pattern = song.statements.iter().find_map(|statement| {
                let SongStatement::Pattern(pattern) = statement else {
                    return None;
                };
                (pattern.name.text == occurrence.pattern.text).then_some(pattern)
            })?;
            return Some(Location::new(
                uri.clone(),
                lsp_range(source, pattern.name.span)?,
            ));
        }
        if let Some(reference) = occurrence
            .instrument
            .as_ref()
            .filter(|instrument| instrument.span.start <= offset && offset < instrument.span.end)
        {
            let instrument = song.statements.iter().find_map(|statement| {
                let SongStatement::Instrument(instrument) = statement else {
                    return None;
                };
                (instrument.name.text == reference.text).then_some(instrument)
            })?;
            return Some(Location::new(
                uri.clone(),
                lsp_range(source, instrument.name.span)?,
            ));
        }
    }
    None
}

fn pitch_description(source: &SourceText, span: SourceSpan) -> Option<String> {
    let parsed = parse(source.id, &source.text);
    let program = parsed
        .diagnostics
        .is_empty()
        .then(|| compile(&parsed.file).ok())??;
    let songs = parsed.file.declarations.iter().filter_map(|declaration| {
        let Declaration::Song(song) = declaration else {
            return None;
        };
        Some(song)
    });

    for (source_song, song) in songs.zip(&program.songs) {
        let patterns = source_song
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SongStatement::Pattern(pattern) => Some(pattern),
                _ => None,
            });
        for (source_pattern, pattern) in patterns.zip(&song.patterns) {
            let PatternBody::Sequence { items, .. } = &source_pattern.body else {
                continue;
            };
            for (item, step) in items.iter().zip(&pattern.steps) {
                match (item, step) {
                    (
                        SequenceItem::Note(source),
                        symphra_compiler::hir::PatternStep::Note(note),
                    ) => {
                        if source.pitch.span == span {
                            return Some(format!("MIDI note {}.", note.midi_pitch));
                        }
                    }
                    (
                        SequenceItem::Chord(source),
                        symphra_compiler::hir::PatternStep::Chord(chord),
                    ) => {
                        for (source, note) in source.pitches.iter().zip(&chord.notes) {
                            if source.span == span {
                                return Some(format!("MIDI note {}.", note.midi_pitch));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    None
}

const fn keyword_description(kind: TokenKind) -> Option<&'static str> {
    Some(match kind {
        TokenKind::Project => "starts the project-wide settings block.",
        TokenKind::Song => "starts a named song block.",
        TokenKind::Seed => "sets the deterministic project seed.",
        TokenKind::SampleRate => "sets the output sample rate, such as `48khz`.",
        TokenKind::Output => "sets the output channel layout (`mono` or `stereo`).",
        TokenKind::Tempo => "sets song tempo in beats per minute, such as `120bpm`.",
        TokenKind::Meter => "sets the song time signature, such as `4/4`.",
        TokenKind::Key => "sets the song tonic and mode, such as `C major`.",
        TokenKind::Instrument => "declares a named instrument.",
        TokenKind::Rhythm => "declares a reusable hit-and-rest rhythm.",
        TokenKind::Resolution => "sets the duration of each rhythm or step item.",
        TokenKind::Hit => "marks an active position in a rhythm.",
        TokenKind::Track => "declares a named playback track.",
        TokenKind::Role => "describes a track's musical role.",
        TokenKind::Volume => "sets track volume in decibels, such as `-5.2db`.",
        TokenKind::Play => "selects the pattern played by a track.",
        TokenKind::TriggerWith => "applies a reusable rhythm to a played pattern.",
        TokenKind::Gate => "scales sounding duration without moving later steps.",
        TokenKind::Transpose => "moves pitched events by a number of semitones.",
        TokenKind::Gain => "scales a played pattern's linear amplitude.",
        TokenKind::Repeat => "repeats a played pattern a fixed number of times.",
        TokenKind::Reverse => "mirrors a played pattern across its end time.",
        TokenKind::Speed => "changes sampler playback speed without moving events.",
        TokenKind::Retrigger => "splits a chance-selected sample into evenly spaced attacks.",
        TokenKind::Pan => "places a track from `-100%` left to `100%` right.",
        TokenKind::Alternate => "alternates successive pan positions or sampler speeds.",
        TokenKind::Chance => "applies a transform to a deterministic percentage of events.",
        TokenKind::ChooseSample => {
            "deterministically picks a new sample index per event from a range."
        }
        TokenKind::At => "places a track's play statement at an absolute `bar:beat` position.",
        TokenKind::Pattern => "declares a named musical pattern.",
        TokenKind::Arrangement => "orders named patterns for sequential playback.",
        TokenKind::With => {
            "assigns an instrument to an arrangement occurrence, or a rhythm to a `play drum` shorthand."
        }
        TokenKind::Sequence => "plays pattern notes one after another.",
        TokenKind::Steps => "plays fixed-resolution steps in source order.",
        TokenKind::Degree => "adds a pitch offset from the song key tonic.",
        TokenKind::Octave => "sets the base octave for a degree step.",
        TokenKind::Note => "adds a written pitch to a sequence.",
        TokenKind::Chord => "adds pitches that start and end together.",
        TokenKind::Rest => "marks silence without producing sound.",
        TokenKind::For => "introduces a duration, such as `1/4`.",
        TokenKind::Velocity => "sets note intensity from 0 to 127.",
        TokenKind::Sample => "selects a sample from the current sampler pack.",
        TokenKind::Choose => "selects one sample alternative deterministically.",
        TokenKind::Weight => "sets a relative choice weight.",
        TokenKind::Sampled => "declares a pitched instrument backed by one WAV file.",
        TokenKind::Sampler => "declares an instrument backed by a sample pack.",
        TokenKind::DrumMachine => "declares an instrument backed by named drum voices.",
        TokenKind::Source => "sets the WAV file used by a sampled instrument.",
        TokenKind::Root => "sets the sample's original pitch, such as `C4`.",
        TokenKind::Pack => "sets the sample pack used by a sampler instrument.",
        TokenKind::Bank => "sets the drum bank used by a drum machine instrument.",
        TokenKind::Drum => "selects a named voice from the current drum bank.",
        _ => return None,
    })
}

#[tokio::main]
async fn main() {
    let (service, socket) = LspService::new(|client| Backend {
        client,
        documents: RwLock::new(HashMap::new()),
        hierarchical_symbols: AtomicBool::new(false),
    });
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;
}

#[cfg(test)]
mod tests {
    use super::{
        SourceId, SourceText, completions, definition, diagnostics, document_symbols,
        flatten_document_symbols, formatting_edits, hover,
    };
    use tower_lsp_server::ls_types::{DiagnosticSeverity, Position, Range, SymbolKind, Uri};

    #[test]
    fn reports_syntax_diagnostics_with_utf16_ranges() {
        let source = SourceText::new(SourceId(0), "test.sym", "😀\nproject { seed nope }");

        let diagnostics = diagnostics(&source);

        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(
            diagnostics[0].range,
            Range::new(Position::new(0, 0), Position::new(0, 2))
        );
        assert_eq!(diagnostics[0].source.as_deref(), Some("symphra"));
    }

    #[test]
    fn reports_semantic_diagnostics_after_successful_parsing() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            "project { seed 1 sample_rate 48khz }",
        );

        let diagnostics = diagnostics(&source);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "output is required");
    }

    #[test]
    fn builds_hierarchical_and_flat_document_symbols() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "project { seed 1 sample_rate 48khz output stereo }\n",
                "song \"Test\" { tempo 120bpm meter 4/4 key C major ",
                "pattern melody = sequence {} }",
            ),
        );

        let symbols = document_symbols(&source);

        assert_eq!((symbols.len(), symbols[0].name.as_str()), (2, "project"));
        assert_eq!(
            (symbols[1].name.as_str(), symbols[1].kind),
            ("Test", SymbolKind::MODULE)
        );
        let children = symbols[1]
            .children
            .as_ref()
            .expect("song should have children");
        assert_eq!(
            (children[0].name.as_str(), children[0].kind),
            ("melody", SymbolKind::FUNCTION)
        );

        let uri = "file:///test.sym".parse::<Uri>().expect("URI should parse");
        let flat = flatten_document_symbols(&uri, &symbols);
        assert_eq!(
            (flat.len(), flat[2].container_name.as_deref()),
            (3, Some("Test"))
        );
    }

    #[test]
    fn completes_keywords_for_the_current_grammar_context() {
        fn labels(source: &str, line: u32, character: u32) -> Vec<String> {
            completions(
                &SourceText::new(SourceId(0), "test.sym", source),
                Position::new(line, character),
            )
            .into_iter()
            .map(|item| item.label)
            .collect()
        }

        assert_eq!(labels("so", 0, 2), ["project", "song"]);
        assert_eq!(
            labels("project {\n  sam", 1, 5),
            ["seed", "sample_rate", "output"]
        );
        assert_eq!(
            labels("song \"Test\" {\n  pat", 1, 5),
            [
                "tempo",
                "meter",
                "key",
                "instrument",
                "rhythm",
                "track",
                "pattern",
                "arrangement"
            ]
        );
        assert_eq!(
            labels("song \"Test\" {\narrangement {\n  melody", 2, 8),
            ["with"]
        );
        assert_eq!(
            labels("song \"Test\" {\npattern p = sequence {\n  no", 2, 4),
            ["note", "chord", "rest"]
        );
        assert_eq!(
            labels("song \"Test\" {\n  pattern p = ", 1, 14),
            ["sequence", "steps"]
        );
        assert_eq!(
            labels("song \"Test\" {\npattern p = steps 1/8 {\n  ", 2, 2),
            ["degree", "sample", "drum", "rest", "choose"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\npattern p = steps 1/8 {\n  choose {\n    ",
                3,
                4
            ),
            ["degree", "sample", "drum", "sequence"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\npattern p = steps 1/8 {\n  choose {\n    sequence weight 1 {\n      ",
                4,
                6
            ),
            ["sample", "drum"]
        );
        assert_eq!(
            labels("song \"Test\" {\npattern p = sequence {\n  note C4 ", 2, 10),
            ["for"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\npattern p = sequence {\n  chord C4 E4 ",
                2,
                14
            ),
            ["for"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\npattern p = sequence {\n  note C4 for 1/4 ",
                2,
                18
            ),
            ["velocity"]
        );
        assert!(labels("😀", 0, 1).is_empty());
    }

    #[test]
    fn completes_instrument_body_keywords() {
        let labels = |source: &str, line, character| {
            completions(
                &SourceText::new(SourceId(0), "test.sym", source),
                Position::new(line, character),
            )
            .into_iter()
            .map(|item| item.label)
            .collect::<Vec<_>>()
        };

        assert_eq!(
            labels("song \"Test\" {\n  instrument lead = ", 1, 20),
            ["sine", "triangle", "sampled", "sampler", "drum_machine"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\n  instrument piano = sampled {\n    ",
                2,
                4
            ),
            ["source", "root"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\n  instrument voice = sampler {\n    ",
                2,
                4
            ),
            ["pack"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\n  instrument tr909 = drum_machine {\n    ",
                2,
                4
            ),
            ["bank"]
        );
    }

    #[test]
    fn completes_rhythm_keywords() {
        let labels = |source: &str, line, character| {
            completions(
                &SourceText::new(SourceId(0), "test.sym", source),
                Position::new(line, character),
            )
            .into_iter()
            .map(|item| item.label)
            .collect::<Vec<_>>()
        };

        assert_eq!(
            labels("song \"Test\" {\n  rhythm pulse ", 1, 15),
            ["resolution"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\n  rhythm pulse resolution 1/8 {\n    ",
                2,
                4
            ),
            ["hit", "rest"]
        );
    }

    #[test]
    fn completes_track_and_trigger_keywords() {
        let labels = |source: &str, line, character| {
            completions(
                &SourceText::new(SourceId(0), "test.sym", source),
                Position::new(line, character),
            )
            .into_iter()
            .map(|item| item.label)
            .collect::<Vec<_>>()
        };

        assert_eq!(labels("song \"Test\" {\n  track lead ", 1, 13), ["role"]);
        assert_eq!(
            labels("song \"Test\" {\ntrack lead role harmony {\n  ", 2, 2),
            ["instrument", "volume", "play", "at"]
        );
        assert_eq!(
            labels("song \"Test\" {\ntrack lead role harmony {\n  play ", 2, 7),
            ["drum"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\ntrack lead role harmony {\n  play melody |> ",
                2,
                17
            ),
            [
                "trigger_with",
                "gate",
                "transpose",
                "gain",
                "repeat",
                "reverse",
                "speed",
                "alternate",
                "pan",
                "chance",
                "choose_sample"
            ]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\ntrack lead role harmony {\n  play melody |> pan ",
                2,
                21
            ),
            ["alternate"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\ntrack lead role harmony {\n  play melody |> chance 40% { ",
                2,
                30
            ),
            ["transpose", "retrigger", "speed"]
        );
    }

    #[test]
    fn completes_octave_after_a_degree_value() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            "song \"Test\" {\npattern p = steps 1/8 {\n  degree 2 ",
        );
        let labels = completions(&source, Position::new(2, 11))
            .into_iter()
            .map(|item| item.label)
            .collect::<Vec<_>>();

        assert_eq!(labels, ["octave"]);
    }

    #[test]
    fn hovers_documented_keywords_at_utf16_positions() {
        let source = SourceText::new(SourceId(0), "test.sym", "😀 song \"Test\" {}");
        let result = hover(&source, Position::new(0, 4)).expect("song should have hover help");

        let super::HoverContents::Markup(contents) = result.contents else {
            panic!("hover should use markup content");
        };
        assert_eq!(contents.value, "`song` — starts a named song block.");
        assert_eq!(
            result.range,
            Some(Range::new(Position::new(0, 3), Position::new(0, 7)))
        );
        assert!(hover(&source, Position::new(0, 1)).is_none());
    }

    #[test]
    fn hovers_compiled_pitch_values() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "project { seed 1 sample_rate 48khz output stereo }\n",
                "song \"Test\" { tempo 120bpm meter 4/4 key C major\n",
                "pattern melody = sequence { note C-1 for 1/4 } }",
            ),
        );
        let result = hover(&source, Position::new(2, 34)).expect("C-1 should have pitch help");

        let super::HoverContents::Markup(contents) = result.contents else {
            panic!("hover should use markup content");
        };
        assert_eq!(contents.value, "`C-1` — MIDI note 0.");

        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "project { seed 1 sample_rate 48khz output stereo }\n",
                "song \"Test\" { tempo 120bpm meter 4/4 key C major\n",
                "pattern harmony = sequence { chord C#4 Eb4 G4 for 1/4 } }",
            ),
        );
        let result = hover(&source, Position::new(2, 35)).expect("C#4 should have pitch help");
        let super::HoverContents::Markup(contents) = result.contents else {
            panic!("hover should use markup content");
        };
        assert_eq!(contents.value, "`C#4` — MIDI note 61.");
    }

    #[test]
    fn finds_pattern_definitions_from_arrangements() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "song \"First\" {\n",
                "  pattern melody = sequence {}\n",
                "}\n",
                "song \"Second\" {\n",
                "  pattern melody = sequence {}\n",
                "  arrangement { melody }\n",
                "}",
            ),
        );
        let uri = "file:///test.sym".parse::<Uri>().expect("URI should parse");

        let location = definition(&source, &uri, Position::new(5, 17))
            .expect("arrangement reference should resolve");

        assert_eq!(location.uri, uri);
        assert_eq!(
            location.range,
            Range::new(Position::new(4, 10), Position::new(4, 16))
        );
        assert!(definition(&source, &uri, Position::new(1, 10)).is_none());
    }

    #[test]
    fn finds_instrument_definitions_from_arrangements() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "song \"Test\" {\n",
                "  instrument lead = triangle\n",
                "  pattern melody = sequence {}\n",
                "  arrangement { melody with lead }\n",
                "}",
            ),
        );
        let uri = "file:///test.sym".parse::<Uri>().expect("URI should parse");

        let location = definition(&source, &uri, Position::new(3, 30))
            .expect("instrument reference should resolve");

        assert_eq!(
            location.range,
            Range::new(Position::new(1, 13), Position::new(1, 17))
        );
    }

    #[test]
    fn formats_a_document_into_canonical_layout() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            "project { seed 1 sample_rate 8khz output mono }\n",
        );

        let edits = formatting_edits(&source);

        assert_eq!(edits.len(), 1);
        assert_eq!(
            edits[0].range,
            Range::new(Position::new(0, 0), Position::new(1, 0))
        );
        assert_eq!(
            edits[0].new_text,
            "project {\n  seed 1\n  sample_rate 8khz\n  output mono\n}\n"
        );
    }

    #[test]
    fn returns_no_edits_for_an_already_formatted_document() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            "project {\n  seed 1\n  sample_rate 8khz\n  output mono\n}\n",
        );

        assert!(formatting_edits(&source).is_empty());
    }

    #[test]
    fn returns_no_edits_for_a_document_with_syntax_errors() {
        let source = SourceText::new(SourceId(0), "test.sym", "project {");

        assert!(formatting_edits(&source).is_empty());
    }
}
