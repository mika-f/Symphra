use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use symphra_compiler::compile;
use symphra_syntax::ast::{
    ArrangementEntry, Declaration, Identifier, PatternBody, PlaySource, PlayStatement,
    SequenceItem, SongDeclaration, SongStatement, TrackBody,
};
use symphra_syntax::{
    SourceId, SourcePosition, SourceSpan, SourceText, Token, TokenKind, lex, parse,
};
use tokio::sync::RwLock;
use tower_lsp_server::jsonrpc::{Error, Result};
use tower_lsp_server::ls_types::{
    CodeLens, CodeLensOptions, CodeLensParams, Command, CompletionItem, CompletionItemKind,
    CompletionOptions, CompletionParams, CompletionResponse, Diagnostic, DiagnosticSeverity,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentFormattingParams, DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, Location, MarkupContent,
    MarkupKind, OneOf, Position, PositionEncodingKind, PrepareRenameResponse, Range,
    ReferenceParams, RenameOptions, RenameParams, ServerCapabilities, ServerInfo,
    SymbolInformation, SymbolKind, TextDocumentPositionParams, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextEdit, Uri, WorkDoneProgressOptions, WorkspaceEdit,
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
                references_provider: Some(OneOf::Left(true)),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(false),
                }),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                })),
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

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let documents = self.documents.read().await;
        let position = params.text_document_position.position;
        let uri = params.text_document_position.text_document.uri;
        let include_declaration = params.context.include_declaration;
        Ok(documents
            .get(&uri)
            .map(|source| references(source, &uri, position, include_declaration)))
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let documents = self.documents.read().await;
        let uri = params.text_document.uri;
        Ok(documents.get(&uri).map(|source| code_lenses(source, &uri)))
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let documents = self.documents.read().await;
        let uri = params.text_document.uri;
        let position = params.position;
        Ok(documents
            .get(&uri)
            .and_then(|source| prepare_rename(source, position)))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let documents = self.documents.read().await;
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let Some(source) = documents.get(&uri) else {
            return Ok(None);
        };
        rename(source, &uri, position, &params.new_name)
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
                        SongStatement::Section(section) => symbol(
                            source,
                            section.name.text.clone(),
                            SymbolKind::OBJECT,
                            section.span,
                            section.name.span,
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
    Layer,
    Use,
    Effect,
    Filter,
    Reverb,
    Automate,
    Lfo,
    Section,
    Parallel,
    Master,
    Limiter,
    Arrangement,
    Sequence,
    Steps,
    Choice,
    ChoiceSequence,
    Sampled,
    Sampler,
    DrumMachine,
    SoundFont,
    Vst3,
    Chance,
    Oscillator,
    Supersaw,
    Envelope,
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
            &[
                "sine",
                "triangle",
                "synth",
                "sampled",
                "sampler",
                "drum_machine",
                "soundfont",
                "vst3",
            ]
        } else {
            &["sequence", "steps"]
        }
    } else if duration_keyword_follows(line_tokens) {
        &["for"]
    } else if let Some(labels) = two_token_follow_up_labels(line_tokens) {
        labels
    } else if matches!(line_tokens.last(), Some(token) if token.kind == TokenKind::Pan) {
        &["alternate"]
    } else if matches!(line_tokens.last(), Some(token) if token.kind == TokenKind::Synth) {
        &["supersaw"]
    } else if matches!(line_tokens.last(), Some(token) if token.kind == TokenKind::Effect) {
        &["delay", "filter", "reverb"]
    } else if matches!(line_tokens.last(), Some(token) if token.kind == TokenKind::Automate) {
        &["cutoff"]
    } else if matches!(line_tokens.last(), Some(token) if token.kind == TokenKind::Lfo) {
        &["sine", "triangle"]
    } else if matches!(line_tokens.last(), Some(token) if token.kind == TokenKind::Master) {
        &["limiter"]
    } else if matches!(line_tokens.last(), Some(token) if token.kind == TokenKind::Parallel) {
        &["exact"]
    } else if matches!(line_tokens.last(), Some(token) if token.kind == TokenKind::Play) {
        if matches!(block, Some(CompletionBlock::Parallel)) {
            &["track"]
        } else {
            &["drum"]
        }
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
        && !matches!(line_tokens.first(), Some(token) if token.kind == TokenKind::Play)
        && !matches!(line_tokens.get(line_tokens.len().saturating_sub(2)), Some(token) if token.kind == TokenKind::With)
    {
        &["with"]
    } else if completion_statement_start(line_tokens) {
        completion_block_labels(block)
    } else {
        &[]
    }
}

/// Keyword suggested right after a two-token `<keyword> <name>` header, such
/// as `track lead` (suggest `role`) or `section phrase` (suggest `bars`).
fn two_token_follow_up_labels(line_tokens: &[Token]) -> Option<&'static [&'static str]> {
    match line_tokens {
        [
            Token {
                kind: TokenKind::Degree,
                ..
            },
            Token {
                kind: TokenKind::Integer,
                ..
            },
        ] => Some(&["octave"]),
        [
            Token {
                kind: TokenKind::Rhythm,
                ..
            },
            Token {
                kind: TokenKind::Identifier,
                ..
            },
        ] => Some(&["resolution"]),
        [
            Token {
                kind: TokenKind::Track,
                ..
            },
            Token {
                kind: TokenKind::Identifier,
                ..
            },
        ] => Some(&["role"]),
        [
            Token {
                kind: TokenKind::Section,
                ..
            },
            Token {
                kind: TokenKind::Identifier,
                ..
            },
        ] => Some(&["bars"]),
        _ => None,
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
            "section",
            "pattern",
            "arrangement",
            "master",
        ],
        Some(CompletionBlock::Sequence) => &["note", "chord", "rest"],
        Some(CompletionBlock::Steps) => &["degree", "sample", "drum", "rest", "choose"],
        Some(CompletionBlock::Choice) => &["degree", "sample", "drum", "sequence"],
        Some(CompletionBlock::ChoiceSequence) => &["sample", "drum"],
        Some(CompletionBlock::Sampled) => &["source", "root"],
        Some(CompletionBlock::Sampler) => &["pack"],
        Some(CompletionBlock::DrumMachine) => &["bank"],
        Some(CompletionBlock::SoundFont | CompletionBlock::Vst3) => &["source", "preset"],
        Some(CompletionBlock::Chance) => &["transpose", "retrigger", "speed"],
        Some(CompletionBlock::Oscillator) => &["envelope"],
        Some(CompletionBlock::Supersaw) => &["voices", "detune", "spread", "envelope"],
        Some(CompletionBlock::Envelope) => &["attack", "decay", "sustain", "release"],
        Some(CompletionBlock::Rhythm) => &["hit", "rest"],
        Some(CompletionBlock::Track) => &[
            "instrument",
            "volume",
            "play",
            "layer",
            "effect",
            "automate",
            "at",
        ],
        Some(CompletionBlock::Layer) => &["use"],
        Some(CompletionBlock::Use) => &["play", "at"],
        Some(CompletionBlock::Effect) => &["mix", "time", "feedback"],
        Some(CompletionBlock::Filter) => &["cutoff", "resonance"],
        Some(CompletionBlock::Reverb) => &["mix", "size"],
        Some(CompletionBlock::Automate) => &["lfo"],
        Some(CompletionBlock::Lfo) => &["range", "rate"],
        Some(CompletionBlock::Section) => &["parallel"],
        Some(CompletionBlock::Parallel) => &["play"],
        Some(CompletionBlock::Master) => &["limiter"],
        Some(CompletionBlock::Limiter) => &["ceiling"],
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
    let note_or_chord_duration = matches!(
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
    ) && tokens.iter().any(|token| token.kind == TokenKind::For);
    let sample_or_drum_step = matches!(
        (tokens.first(), tokens.last()),
        (
            Some(Token {
                kind: TokenKind::Sample,
                ..
            }),
            Some(Token {
                kind: TokenKind::Integer,
                ..
            })
        ) | (
            Some(Token {
                kind: TokenKind::Drum,
                ..
            }),
            Some(Token {
                kind: TokenKind::String,
                ..
            })
        )
    ) && tokens.len() == 2;
    note_or_chord_duration || sample_or_drum_step
}

fn completion_block(tokens: &[Token]) -> Option<CompletionBlock> {
    let mut pending = None;
    let mut previous = None;
    let mut blocks = Vec::new();
    for token in tokens {
        match token.kind {
            TokenKind::Project => pending = Some(CompletionBlock::Project),
            TokenKind::Song => pending = Some(CompletionBlock::Song),
            TokenKind::Rhythm => pending = Some(CompletionBlock::Rhythm),
            TokenKind::Track => pending = Some(CompletionBlock::Track),
            TokenKind::Layer => pending = Some(CompletionBlock::Layer),
            TokenKind::Use => pending = Some(CompletionBlock::Use),
            TokenKind::Delay => pending = Some(CompletionBlock::Effect),
            TokenKind::Filter => pending = Some(CompletionBlock::Filter),
            TokenKind::Reverb => pending = Some(CompletionBlock::Reverb),
            TokenKind::Automate => pending = Some(CompletionBlock::Automate),
            TokenKind::Lfo => pending = Some(CompletionBlock::Lfo),
            TokenKind::Section => pending = Some(CompletionBlock::Section),
            TokenKind::Parallel => pending = Some(CompletionBlock::Parallel),
            TokenKind::Master => pending = Some(CompletionBlock::Master),
            TokenKind::Limiter => pending = Some(CompletionBlock::Limiter),
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
            TokenKind::Soundfont => pending = Some(CompletionBlock::SoundFont),
            TokenKind::Vst3 => pending = Some(CompletionBlock::Vst3),
            TokenKind::Chance => pending = Some(CompletionBlock::Chance),
            TokenKind::Supersaw => pending = Some(CompletionBlock::Supersaw),
            TokenKind::Envelope => pending = Some(CompletionBlock::Envelope),
            // `instrument x = sine { ... }` / `= triangle { ... }`: the
            // bare waveform is a plain identifier (not a dedicated keyword
            // token, like `sampled`/`sampler`/`drum_machine` are), so it is
            // recognized by position instead — `=` immediately followed by
            // an identifier is unique to this grammar production (every
            // other `<keyword> =` in the language is followed by a
            // dedicated keyword token, not a bare identifier).
            TokenKind::Identifier if previous == Some(TokenKind::Equal) => {
                pending = Some(CompletionBlock::Oscillator);
            }
            TokenKind::LeftBrace => blocks.push(pending.take().unwrap_or(CompletionBlock::Other)),
            TokenKind::RightBrace => {
                blocks.pop();
            }
            _ => {}
        }
        previous = Some(token.kind);
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
                    | TokenKind::Layer
                    | TokenKind::Use
                    | TokenKind::Effect
                    | TokenKind::Delay
                    | TokenKind::Mix
                    | TokenKind::Time
                    | TokenKind::Feedback
                    | TokenKind::Filter
                    | TokenKind::Cutoff
                    | TokenKind::Resonance
                    | TokenKind::Reverb
                    | TokenKind::Size
                    | TokenKind::Automate
                    | TokenKind::Lfo
                    | TokenKind::Range
                    | TokenKind::Rate
                    | TokenKind::Cycles
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
                    | TokenKind::Bank
                    | TokenKind::Preset
                    | TokenKind::Section
                    | TokenKind::Bars
                    | TokenKind::Parallel
                    | TokenKind::Exact
                    | TokenKind::Master
                    | TokenKind::Limiter
                    | TokenKind::Ceiling
                    | TokenKind::Synth
                    | TokenKind::Supersaw
                    | TokenKind::Envelope
                    | TokenKind::Attack
                    | TokenKind::Decay
                    | TokenKind::Sustain
                    | TokenKind::Release
                    | TokenKind::Voices
                    | TokenKind::Detune
                    | TokenKind::Spread,
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

/// Kind of a named song-local declaration that definition/references understand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NamedKind {
    Pattern,
    Instrument,
    Rhythm,
    Track,
    Section,
}

fn definition(source: &SourceText, uri: &Uri, position: Position) -> Option<Location> {
    let offset = source.byte_offset_utf16(SourcePosition {
        line: position.line,
        utf16_column: position.character,
    })?;
    let parsed = parse(source.id, &source.text);
    let span = parsed.file.declarations.iter().find_map(|declaration| {
        let Declaration::Song(song) = declaration else {
            return None;
        };
        // Go-to-definition only from reference sites, never from the declaration itself.
        let (kind, name) = name_reference_at(song, offset)?;
        declaration_span(song, kind, name)
    })?;
    Some(Location::new(uri.clone(), lsp_range(source, span)?))
}

fn references(
    source: &SourceText,
    uri: &Uri,
    position: Position,
    include_declaration: bool,
) -> Vec<Location> {
    let Some(offset) = source.byte_offset_utf16(SourcePosition {
        line: position.line,
        utf16_column: position.character,
    }) else {
        return Vec::new();
    };
    let parsed = parse(source.id, &source.text);
    let Some((declaration, ref_spans)) = parsed.file.declarations.iter().find_map(|declaration| {
        let Declaration::Song(song) = declaration else {
            return None;
        };
        let (kind, name, declaration_span) = named_symbol_at(song, offset)?;
        Some((declaration_span, reference_spans(song, kind, name)))
    }) else {
        return Vec::new();
    };

    let mut locations = Vec::new();
    if include_declaration
        && let Some(range) = lsp_range(source, declaration)
    {
        locations.push(Location::new(uri.clone(), range));
    }
    for span in ref_spans {
        if let Some(range) = lsp_range(source, span) {
            locations.push(Location::new(uri.clone(), range));
        }
    }
    locations
}

fn code_lenses(source: &SourceText, uri: &Uri) -> Vec<CodeLens> {
    let parsed = parse(source.id, &source.text);
    let mut lenses = Vec::new();
    for declaration in &parsed.file.declarations {
        let Declaration::Song(song) = declaration else {
            continue;
        };
        for (kind, name) in named_declarations(song) {
            let Some(range) = lsp_range(source, name.span) else {
                continue;
            };
            let reference_locations: Vec<Location> = reference_spans(song, kind, &name.text)
                .into_iter()
                .filter_map(|span| {
                    lsp_range(source, span).map(|range| Location::new(uri.clone(), range))
                })
                .collect();
            let count = reference_locations.len();
            let title = if count == 1 {
                "1 reference".to_owned()
            } else {
                format!("{count} references")
            };
            let arguments = Some(vec![
                serde_json::Value::String(uri.as_str().to_owned()),
                serde_json::to_value(range.start).unwrap_or(serde_json::Value::Null),
                serde_json::to_value(reference_locations).unwrap_or(serde_json::Value::Null),
            ]);
            lenses.push(CodeLens {
                range,
                command: Some(Command {
                    title,
                    command: "symphra.showReferences".to_owned(),
                    arguments,
                }),
                data: None,
            });
        }
    }
    lenses
}

fn prepare_rename(source: &SourceText, position: Position) -> Option<PrepareRenameResponse> {
    let (range, placeholder) = rename_target(source, position)?;
    Some(PrepareRenameResponse::RangeWithPlaceholder { range, placeholder })
}

fn rename(
    source: &SourceText,
    uri: &Uri,
    position: Position,
    new_name: &str,
) -> Result<Option<WorkspaceEdit>> {
    if !is_valid_identifier_name(new_name) {
        return Err(Error::invalid_params(format!(
            "`{new_name}` is not a valid Symphra identifier"
        )));
    }

    let Some(offset) = source.byte_offset_utf16(SourcePosition {
        line: position.line,
        utf16_column: position.character,
    }) else {
        return Ok(None);
    };
    let parsed = parse(source.id, &source.text);
    // Rename is song-scoped, matching definition/references.
    let Some((kind, old_name, declaration, song)) =
        parsed.file.declarations.iter().find_map(|declaration| {
            let Declaration::Song(song) = declaration else {
                return None;
            };
            let (kind, name, _occurrence) = rename_target_in_song(song, offset)?;
            let declaration = declaration_span(song, kind, name)?;
            Some((kind, name, declaration, song))
        })
    else {
        return Ok(None);
    };

    if old_name == new_name {
        return Ok(Some(WorkspaceEdit::new(HashMap::new())));
    }

    if declaration_span(song, kind, new_name).is_some() {
        return Err(Error::invalid_params(format!(
            "a {} named `{new_name}` already exists in this song",
            kind.label()
        )));
    }

    let mut spans = vec![declaration];
    spans.extend(reference_spans(song, kind, old_name));

    let mut edits = Vec::with_capacity(spans.len());
    for span in spans {
        let Some(range) = lsp_range(source, span) else {
            continue;
        };
        edits.push(TextEdit {
            range,
            new_text: new_name.to_owned(),
        });
    }
    // Clients apply edits best when ordered from later offsets to earlier ones.
    edits.sort_by(|left, right| {
        right
            .range
            .start
            .line
            .cmp(&left.range.start.line)
            .then(right.range.start.character.cmp(&left.range.start.character))
    });

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);
    Ok(Some(WorkspaceEdit::new(changes)))
}

/// Identifier range under the cursor plus the current name (for prepareRename UI).
fn rename_target(source: &SourceText, position: Position) -> Option<(Range, String)> {
    let offset = source.byte_offset_utf16(SourcePosition {
        line: position.line,
        utf16_column: position.character,
    })?;
    let parsed = parse(source.id, &source.text);
    let (occurrence, name) = parsed.file.declarations.iter().find_map(|declaration| {
        let Declaration::Song(song) = declaration else {
            return None;
        };
        let (_kind, name, occurrence) = rename_target_in_song(song, offset)?;
        Some((occurrence, name.to_owned()))
    })?;
    Some((lsp_range(source, occurrence)?, name))
}

/// Declaration or resolved reference under `offset` → `(kind, name, occurrence span)`.
fn rename_target_in_song(
    song: &SongDeclaration,
    offset: u32,
) -> Option<(NamedKind, &str, SourceSpan)> {
    for (kind, name) in named_declarations(song) {
        if span_contains(name.span, offset) {
            return Some((kind, name.text.as_str(), name.span));
        }
    }
    let mut matched: Option<(NamedKind, String, SourceSpan)> = None;
    visit_name_references(song, |kind, identifier| {
        if matched.is_none()
            && span_contains(identifier.span, offset)
            && declaration_span(song, kind, &identifier.text).is_some()
        {
            matched = Some((kind, identifier.text.clone(), identifier.span));
        }
    });
    let (kind, name, occurrence) = matched?;
    named_declarations(song).find_map(|(declaration_kind, identifier)| {
        (declaration_kind == kind && identifier.text == name)
            .then_some((kind, identifier.text.as_str(), occurrence))
    })
}

/// A rename target must lex as a single non-keyword identifier covering the whole string.
fn is_valid_identifier_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let tokens = lex(SourceId(0), name).tokens;
    matches!(
        tokens.as_slice(),
        [
            Token {
                kind: TokenKind::Identifier,
                text,
                ..
            },
            Token {
                kind: TokenKind::Eof,
                ..
            }
        ] if text == name
    )
}

impl NamedKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Pattern => "pattern",
            Self::Instrument => "instrument",
            Self::Rhythm => "rhythm",
            Self::Track => "track",
            Self::Section => "section",
        }
    }
}

/// Declaration or resolved reference under `offset` → `(kind, name, declaration span)`.
fn named_symbol_at(
    song: &SongDeclaration,
    offset: u32,
) -> Option<(NamedKind, &str, SourceSpan)> {
    for (kind, name) in named_declarations(song) {
        if span_contains(name.span, offset) {
            return Some((kind, name.text.as_str(), name.span));
        }
    }
    let (kind, name) = name_reference_at(song, offset)?;
    let declaration = declaration_span(song, kind, name)?;
    Some((kind, name, declaration))
}

fn declaration_span(song: &SongDeclaration, kind: NamedKind, name: &str) -> Option<SourceSpan> {
    named_declarations(song).find_map(|(declaration_kind, identifier)| {
        (declaration_kind == kind && identifier.text == name).then_some(identifier.span)
    })
}

fn named_declarations(song: &SongDeclaration) -> impl Iterator<Item = (NamedKind, &Identifier)> {
    song.statements.iter().filter_map(|statement| match statement {
        SongStatement::Pattern(pattern) => Some((NamedKind::Pattern, &pattern.name)),
        SongStatement::Instrument(instrument) => Some((NamedKind::Instrument, &instrument.name)),
        SongStatement::Rhythm(rhythm) => Some((NamedKind::Rhythm, &rhythm.name)),
        SongStatement::Track(track) => Some((NamedKind::Track, &track.name)),
        SongStatement::Section(section) => Some((NamedKind::Section, &section.name)),
        _ => None,
    })
}

/// Reference site under `offset` → `(kind, referenced name)`, only when a matching declaration exists.
fn name_reference_at(song: &SongDeclaration, offset: u32) -> Option<(NamedKind, &str)> {
    let mut matched: Option<(NamedKind, String)> = None;
    visit_name_references(song, |kind, identifier| {
        if matched.is_none()
            && span_contains(identifier.span, offset)
            && declaration_span(song, kind, &identifier.text).is_some()
        {
            matched = Some((kind, identifier.text.clone()));
        }
    });
    let (kind, name) = matched?;
    named_declarations(song).find_map(|(declaration_kind, identifier)| {
        (declaration_kind == kind && identifier.text == name)
            .then_some((kind, identifier.text.as_str()))
    })
}

fn reference_spans(song: &SongDeclaration, kind: NamedKind, name: &str) -> Vec<SourceSpan> {
    let mut spans = Vec::new();
    visit_name_references(song, |reference_kind, identifier| {
        if reference_kind == kind && identifier.text == name {
            spans.push(identifier.span);
        }
    });
    spans
}

fn visit_name_references(song: &SongDeclaration, mut visit: impl FnMut(NamedKind, &Identifier)) {
    for statement in &song.statements {
        match statement {
            SongStatement::Arrangement { entries, .. } => {
                for entry in entries {
                    match entry {
                        ArrangementEntry::Pattern(occurrence) => {
                            visit(NamedKind::Pattern, &occurrence.pattern);
                            if let Some(instrument) = &occurrence.instrument {
                                visit(NamedKind::Instrument, instrument);
                            }
                        }
                        ArrangementEntry::Play { name, .. } => {
                            visit(NamedKind::Section, name);
                        }
                    }
                }
            }
            SongStatement::Section(section) => {
                for track in &section.tracks {
                    visit(NamedKind::Track, track);
                }
            }
            SongStatement::Track(track) => match &track.body {
                TrackBody::Single { instrument, play } => {
                    visit(NamedKind::Instrument, instrument);
                    visit_play_name_references(play, &mut visit);
                }
                TrackBody::Layers { uses, .. } => {
                    for layer in uses {
                        visit(NamedKind::Instrument, &layer.instrument);
                        visit_play_name_references(&layer.play, &mut visit);
                    }
                }
            },
            _ => {}
        }
    }
}

fn visit_play_name_references(
    play: &PlayStatement,
    visit: &mut impl FnMut(NamedKind, &Identifier),
) {
    match &play.source {
        PlaySource::Pattern(name) => visit(NamedKind::Pattern, name),
        PlaySource::Drum { rhythm, .. } => visit(NamedKind::Rhythm, rhythm),
    }
    if let Some(rhythm) = &play.trigger_with {
        visit(NamedKind::Rhythm, rhythm);
    }
}

const fn span_contains(span: SourceSpan, offset: u32) -> bool {
    span.start <= offset && offset < span.end
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

/// Delegates to two halves so each stays under clippy's `too_many_lines`
/// threshold as new keywords are documented.
const fn keyword_description(kind: TokenKind) -> Option<&'static str> {
    if let Some(description) = keyword_description_declarations(kind) {
        return Some(description);
    }
    keyword_description_playback(kind)
}

const fn keyword_description_declarations(kind: TokenKind) -> Option<&'static str> {
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
        TokenKind::Layer => {
            "declares several independently scheduled `use` layers, mixed into one track."
        }
        TokenKind::Use => "declares one layer's instrument and play pipeline inside `layer`.",
        TokenKind::Effect => "applies an audio effect to the track's rendered output.",
        TokenKind::Delay => "a feedback delay (echo).",
        TokenKind::Mix => "blends dry (`0.0`) and delayed/wet (`1.0`) signal in an effect.",
        TokenKind::Time => "sets a delay effect's echo time, such as `1/4` or `1bar`.",
        TokenKind::Feedback => {
            "sets how much of a delay's echo feeds back into itself, `0.0` to `0.95`."
        }
        TokenKind::Filter => "a resonant lowpass filter.",
        TokenKind::Cutoff => "sets a filter effect's lowpass cutoff frequency, such as `2000hz`.",
        TokenKind::Resonance => {
            "sets a filter effect's resonance/Q, `0.0` (gentle) to `1.0` (sharp peak)."
        }
        TokenKind::Reverb => "a Schroeder reverberator (comb and allpass filters).",
        TokenKind::Size => {
            "sets a reverb effect's room size, `0.0` (short tail) to `1.0` (long tail)."
        }
        TokenKind::Automate => {
            "sweeps a track's `effect filter` cutoff over time instead of holding it fixed."
        }
        TokenKind::Lfo => "a low-frequency oscillator driving an `automate` target.",
        TokenKind::Range => "sets an `lfo`'s sweep bounds, such as `600hz..2800hz`.",
        TokenKind::Rate => "sets an `lfo`'s speed, such as `2 cycles/bar`.",
        TokenKind::Cycles => "used in `rate N cycles/bar` to set an `lfo`'s speed.",
        TokenKind::Synth => "introduces a synthesizer instrument kind, such as `supersaw`.",
        TokenKind::Supersaw => "a unison of detuned sawtooth oscillators.",
        TokenKind::Voices => "sets a `supersaw`'s oscillator count, such as `voices 5`.",
        TokenKind::Detune => "sets a `supersaw`'s pitch spread, `0.0` to `1.0`.",
        TokenKind::Spread => "sets a `supersaw`'s voice blend, `0.0` (thin) to `1.0` (thick).",
        TokenKind::Envelope => "replaces an oscillator's fixed edge fade with an ADSR shape.",
        TokenKind::Attack => "sets an envelope's ramp-up time, such as `4ms`.",
        TokenKind::Decay => "sets an envelope's ramp-down-to-sustain time, such as `200ms`.",
        TokenKind::Sustain => "sets an envelope's held level, `0.0` to `1.0`.",
        TokenKind::Release => "sets an envelope's ramp-to-silence time, such as `150ms`.",
        _ => return None,
    })
}

const fn keyword_description_playback(kind: TokenKind) -> Option<&'static str> {
    Some(match kind {
        TokenKind::Play => {
            "selects the pattern played by a track, or references a track/section elsewhere."
        }
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
        TokenKind::Arrangement => {
            "orders named patterns, or `play`ed sections, for sequential playback."
        }
        TokenKind::With => {
            "assigns an instrument to an arrangement occurrence, or a rhythm to a `play drum` shorthand."
        }
        TokenKind::Section => "declares a named, fixed-length (`bars`) group of declared tracks.",
        TokenKind::Bars => "sets a section's length in bars, such as `bars 4`.",
        TokenKind::Parallel => "starts a section's group of simultaneously placed tracks.",
        TokenKind::Exact => {
            "requires every track in a `parallel` block to last exactly the section's `bars`."
        }
        TokenKind::Master => "starts the song's master processing chain, applied after mixing.",
        TokenKind::Limiter => {
            "scales the master buffer down so its peak does not exceed `ceiling`."
        }
        TokenKind::Ceiling => "sets a limiter's maximum output level, such as `-0.3db`.",
        TokenKind::Sequence => "plays pattern notes one after another.",
        TokenKind::Steps => "plays fixed-resolution steps in source order.",
        TokenKind::Degree => "adds a pitch offset from the song key tonic.",
        TokenKind::Octave => "sets the base octave for a degree step.",
        TokenKind::Note => "adds a written pitch to a sequence.",
        TokenKind::Chord => "adds pitches that start and end together.",
        TokenKind::Rest => "marks silence without producing sound.",
        TokenKind::For => "introduces a duration, such as `1/4` or `1bar`.",
        TokenKind::Velocity => "sets note intensity from 0 to 127.",
        TokenKind::Bar => {
            "expresses a duration as a whole number of the song's meter, such as `1bar`."
        }
        TokenKind::Sample => "selects a sample from the current sampler pack.",
        TokenKind::Choose => "selects one sample alternative deterministically.",
        TokenKind::Weight => "sets a relative choice weight.",
        TokenKind::Sampled => "declares a pitched instrument backed by one WAV file.",
        TokenKind::Sampler => "declares an instrument backed by a sample pack.",
        TokenKind::DrumMachine => "declares an instrument backed by named drum voices.",
        TokenKind::Soundfont => "declares a pitched instrument backed by a SoundFont preset.",
        TokenKind::Vst3 => "declares a pitched instrument backed by a live VST3 plug-in.",
        TokenKind::Source => {
            "sets the WAV, SoundFont, or VST3 plugin file used by a sampled/soundfont/vst3 instrument."
        }
        TokenKind::Root => "sets the sample's original pitch, such as `C4`.",
        TokenKind::Pack => "sets the sample pack used by a sampler instrument.",
        TokenKind::Bank => "sets the drum bank used by a drum machine instrument.",
        TokenKind::Preset => "sets the preset/program name used by a soundfont or vst3 instrument.",
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
        SourceId, SourceText, code_lenses, completions, definition, diagnostics, document_symbols,
        flatten_document_symbols, formatting_edits, hover, prepare_rename, references, rename,
    };
    use tower_lsp_server::ls_types::{
        DiagnosticSeverity, Position, PrepareRenameResponse, Range, SymbolKind, Uri,
    };

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
                "section",
                "pattern",
                "arrangement",
                "master"
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
        assert_eq!(
            labels(
                "song \"Test\" {\npattern p = steps 1/8 {\n  sample 2 ",
                2,
                11
            ),
            ["velocity"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\npattern p = steps 1/8 {\n  drum \"bd\" ",
                2,
                12
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
            [
                "sine",
                "triangle",
                "synth",
                "sampled",
                "sampler",
                "drum_machine",
                "soundfont",
                "vst3"
            ]
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
        assert_eq!(
            labels(
                "song \"Test\" {\n  instrument music_box = soundfont {\n    ",
                2,
                4
            ),
            ["source", "preset"]
        );
        assert_eq!(
            labels("song \"Test\" {\n  instrument lead = vst3 {\n    ", 2, 4),
            ["source", "preset"]
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
            [
                "instrument",
                "volume",
                "play",
                "layer",
                "effect",
                "automate",
                "at"
            ]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\ntrack bass role low {\n  layer {\n    ",
                3,
                4
            ),
            ["use"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\ntrack bass role low {\n  layer {\n    use sub_sine {\n      ",
                4,
                6
            ),
            ["play", "at"]
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
    fn completes_effect_body_keywords() {
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
            labels(
                "song \"Test\" {\ntrack lead role harmony {\n  effect ",
                2,
                9
            ),
            ["delay", "filter", "reverb"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\ntrack lead role harmony {\n  effect delay {\n    ",
                3,
                4
            ),
            ["mix", "time", "feedback"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\ntrack lead role harmony {\n  effect filter {\n    ",
                3,
                4
            ),
            ["cutoff", "resonance"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\ntrack lead role harmony {\n  effect reverb {\n    ",
                3,
                4
            ),
            ["mix", "size"]
        );
    }

    #[test]
    fn completes_oscillator_and_supersaw_envelope_keywords() {
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
            labels("song \"Test\" {\ninstrument lead = synth ", 1, 24),
            ["supersaw"]
        );
        assert_eq!(
            labels("song \"Test\" {\ninstrument lead = sine {\n  ", 2, 2),
            ["envelope"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\ninstrument lead = synth supersaw {\n  ",
                2,
                2
            ),
            ["voices", "detune", "spread", "envelope"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\ninstrument lead = sine {\n  envelope {\n    ",
                3,
                4
            ),
            ["attack", "decay", "sustain", "release"]
        );
    }

    #[test]
    fn completes_automate_body_keywords() {
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
            labels(
                "song \"Test\" {\ntrack lead role harmony {\n  automate ",
                2,
                11
            ),
            ["cutoff"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\ntrack lead role harmony {\n  automate cutoff {\n    ",
                3,
                4
            ),
            ["lfo"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\ntrack lead role harmony {\n  automate cutoff { lfo ",
                2,
                24
            ),
            ["sine", "triangle"]
        );
        assert_eq!(
            labels(
                "song \"Test\" {\ntrack lead role harmony {\n  automate cutoff { lfo sine {\n    ",
                3,
                4
            ),
            ["range", "rate"]
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
    fn finds_rhythm_definitions_from_trigger_with() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "song \"First\" {\n",
                "  rhythm stabs resolution 1/8 { hit rest }\n",
                "}\n",
                "song \"Second\" {\n",
                "  rhythm stabs resolution 1/8 { hit rest }\n",
                "  rhythm pulse resolution 1/8 { hit hit }\n",
                "  pattern melody = sequence {}\n",
                "  instrument lead = triangle\n",
                "  track chords role harmony {\n",
                "    instrument lead\n",
                "    play melody |> trigger_with stabs\n",
                "  }\n",
                "  track bass role low {\n",
                "    layer {\n",
                "      use lead { play melody |> trigger_with pulse }\n",
                "    }\n",
                "  }\n",
                "}\n",
            ),
        );
        let uri = "file:///test.sym".parse::<Uri>().expect("URI should parse");

        // `trigger_with stabs` inside a single-body track.
        // `    play melody |> trigger_with stabs` → stabs starts at column 32
        let single = definition(&source, &uri, Position::new(10, 32))
            .expect("trigger_with rhythm should resolve");
        assert_eq!(single.uri, uri);
        assert_eq!(
            single.range,
            Range::new(Position::new(4, 9), Position::new(4, 14))
        );

        // `trigger_with pulse` inside a layered track.
        // `      use lead { play melody |> trigger_with pulse }` → pulse at column 45
        let layered = definition(&source, &uri, Position::new(14, 45))
            .expect("layer trigger_with rhythm should resolve");
        assert_eq!(
            layered.range,
            Range::new(Position::new(5, 9), Position::new(5, 14))
        );

        // Same rhythm name in another song must not be chosen from a declaration site.
        assert!(definition(&source, &uri, Position::new(1, 9)).is_none());

        // Unresolved rhythm names return no location.
        let missing = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "song \"Test\" {\n",
                "  pattern melody = sequence {}\n",
                "  instrument lead = triangle\n",
                "  track chords role harmony {\n",
                "    instrument lead\n",
                "    play melody |> trigger_with missing\n",
                "  }\n",
                "}\n",
            ),
        );
        assert!(definition(&missing, &uri, Position::new(5, 32)).is_none());
    }

    #[test]
    fn finds_pattern_definitions_from_track_play() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "song \"First\" {\n",
                "  pattern melody = sequence {}\n",
                "}\n",
                "song \"Second\" {\n",
                "  pattern melody = sequence {}\n",
                "  pattern bassline = sequence {}\n",
                "  instrument lead = triangle\n",
                "  track chords role harmony {\n",
                "    instrument lead\n",
                "    play melody\n",
                "  }\n",
                "  track bass role low {\n",
                "    layer {\n",
                "      use lead { play bassline }\n",
                "    }\n",
                "  }\n",
                "}\n",
            ),
        );
        let uri = "file:///test.sym".parse::<Uri>().expect("URI should parse");

        // `play melody` inside a single-body track.
        let single = definition(&source, &uri, Position::new(9, 9))
            .expect("track play pattern should resolve");
        assert_eq!(single.uri, uri);
        assert_eq!(
            single.range,
            Range::new(Position::new(4, 10), Position::new(4, 16))
        );

        // `play bassline` inside a layered track.
        let layered = definition(&source, &uri, Position::new(13, 22))
            .expect("layer play pattern should resolve");
        assert_eq!(
            layered.range,
            Range::new(Position::new(5, 10), Position::new(5, 18))
        );

        // Same pattern name in another song must not be chosen from a declaration site.
        assert!(definition(&source, &uri, Position::new(1, 10)).is_none());

        // Unresolved pattern names return no location.
        let missing = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "song \"Test\" {\n",
                "  instrument lead = triangle\n",
                "  track chords role harmony {\n",
                "    instrument lead\n",
                "    play missing\n",
                "  }\n",
                "}\n",
            ),
        );
        assert!(definition(&missing, &uri, Position::new(4, 9)).is_none());
    }

    #[test]
    fn finds_instrument_definitions_from_track_body() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "song \"First\" {\n",
                "  instrument lead = triangle\n",
                "}\n",
                "song \"Second\" {\n",
                "  instrument lead = triangle\n",
                "  instrument sub = triangle\n",
                "  pattern melody = sequence {}\n",
                "  track chords role harmony {\n",
                "    instrument lead\n",
                "    play melody\n",
                "  }\n",
                "  track bass role low {\n",
                "    layer {\n",
                "      use sub { play melody }\n",
                "    }\n",
                "  }\n",
                "}\n",
            ),
        );
        let uri = "file:///test.sym".parse::<Uri>().expect("URI should parse");

        // `instrument lead` inside a single-body track.
        let single = definition(&source, &uri, Position::new(8, 15))
            .expect("track instrument reference should resolve");
        assert_eq!(single.uri, uri);
        assert_eq!(
            single.range,
            Range::new(Position::new(4, 13), Position::new(4, 17))
        );

        // `use sub` inside a layered track.
        let layered = definition(&source, &uri, Position::new(13, 10))
            .expect("layer use instrument reference should resolve");
        assert_eq!(
            layered.range,
            Range::new(Position::new(5, 13), Position::new(5, 16))
        );

        // Same instrument name in another song must not be chosen.
        assert!(definition(&source, &uri, Position::new(1, 13)).is_none());

        // Unresolved instrument names return no location.
        let missing = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "song \"Test\" {\n",
                "  pattern melody = sequence {}\n",
                "  track chords role harmony {\n",
                "    instrument missing\n",
                "    play melody\n",
                "  }\n",
                "}\n",
            ),
        );
        assert!(definition(&missing, &uri, Position::new(3, 15)).is_none());
    }

    #[test]
    fn finds_track_definitions_from_section_play_track() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "song \"First\" {\n",
                "  track pad role harmony { instrument lead play melody }\n",
                "}\n",
                "song \"Second\" {\n",
                "  track pad role harmony { instrument lead play melody }\n",
                "  track bass role low { instrument sub play bassline }\n",
                "  section intro bars 2 {\n",
                "    parallel {\n",
                "      play track pad\n",
                "      play track bass\n",
                "    }\n",
                "  }\n",
                "}",
            ),
        );
        let uri = "file:///test.sym".parse::<Uri>().expect("URI should parse");

        let pad =
            definition(&source, &uri, Position::new(8, 17)).expect("play track pad should resolve");
        assert_eq!(pad.uri, uri);
        assert_eq!(
            pad.range,
            Range::new(Position::new(4, 8), Position::new(4, 11))
        );

        let bass = definition(&source, &uri, Position::new(9, 17))
            .expect("play track bass should resolve");
        assert_eq!(
            bass.range,
            Range::new(Position::new(5, 8), Position::new(5, 12))
        );

        // Same name in another song must not be used as the declaration target.
        assert!(definition(&source, &uri, Position::new(1, 8)).is_none());
        // Unresolved track names return no location.
        let missing = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "song \"Test\" {\n",
                "  section intro bars 2 {\n",
                "    parallel { play track missing }\n",
                "  }\n",
                "}",
            ),
        );
        assert!(definition(&missing, &uri, Position::new(2, 26)).is_none());
    }

    #[test]
    fn finds_references_from_declarations_and_uses() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "song \"First\" {\n",
                "  pattern melody = sequence {}\n",
                "}\n",
                "song \"Second\" {\n",
                "  pattern melody = sequence {}\n",
                "  instrument lead = triangle\n",
                "  track chords role harmony {\n",
                "    instrument lead\n",
                "    play melody\n",
                "  }\n",
                "  arrangement { melody with lead }\n",
                "}\n",
            ),
        );
        let uri = "file:///test.sym".parse::<Uri>().expect("URI should parse");

        // Declaration site of Second.melody, excluding the declaration itself.
        let from_decl = references(&source, &uri, Position::new(4, 10), false);
        assert_eq!(from_decl.len(), 2);
        assert_eq!(
            from_decl[0].range,
            Range::new(Position::new(8, 9), Position::new(8, 15))
        );
        assert_eq!(
            from_decl[1].range,
            Range::new(Position::new(10, 16), Position::new(10, 22))
        );

        // Same symbol from a use site, including the declaration.
        let from_use = references(&source, &uri, Position::new(8, 9), true);
        assert_eq!(from_use.len(), 3);
        assert_eq!(
            from_use[0].range,
            Range::new(Position::new(4, 10), Position::new(4, 16))
        );

        // First.melody is a different symbol with zero uses in its song.
        let other_song = references(&source, &uri, Position::new(1, 10), false);
        assert!(other_song.is_empty());
    }

    #[test]
    fn builds_reference_code_lenses_for_declarations() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "song \"Test\" {\n",
                "  pattern melody = sequence {}\n",
                "  pattern unused = sequence {}\n",
                "  arrangement { melody }\n",
                "}\n",
            ),
        );
        let uri = "file:///test.sym".parse::<Uri>().expect("URI should parse");

        let lenses = code_lenses(&source, &uri);
        let melody = lenses
            .iter()
            .find(|lens| {
                lens.range == Range::new(Position::new(1, 10), Position::new(1, 16))
            })
            .expect("melody should have a code lens");
        let unused = lenses
            .iter()
            .find(|lens| {
                lens.range == Range::new(Position::new(2, 10), Position::new(2, 16))
            })
            .expect("unused should have a code lens");

        assert_eq!(
            melody.command.as_ref().map(|command| command.title.as_str()),
            Some("1 reference")
        );
        assert_eq!(
            unused
                .command
                .as_ref()
                .map(|command| command.title.as_str()),
            Some("0 references")
        );
        assert_eq!(
            melody
                .command
                .as_ref()
                .map(|command| command.command.as_str()),
            Some("symphra.showReferences")
        );
    }

    #[test]
    fn prepares_and_renames_pattern_names_in_one_song() {
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
                "}\n",
            ),
        );
        let uri = "file:///test.sym".parse::<Uri>().expect("URI should parse");

        // prepareRename on the Second.melody declaration.
        let prepared = prepare_rename(&source, Position::new(4, 10))
            .expect("pattern declaration should be renameable");
        let PrepareRenameResponse::RangeWithPlaceholder {
            range,
            placeholder,
        } = prepared
        else {
            panic!("prepareRename should return a range with placeholder");
        };
        assert_eq!(
            range,
            Range::new(Position::new(4, 10), Position::new(4, 16))
        );
        assert_eq!(placeholder, "melody");

        // prepareRename on the arrangement use site.
        let prepared_use = prepare_rename(&source, Position::new(5, 17))
            .expect("pattern reference should be renameable");
        let PrepareRenameResponse::RangeWithPlaceholder {
            range: use_range, ..
        } = prepared_use
        else {
            panic!("prepareRename should return a range with placeholder");
        };
        assert_eq!(
            use_range,
            Range::new(Position::new(5, 16), Position::new(5, 22))
        );

        // Keywords and the other song's identical name are not mixed in.
        assert!(prepare_rename(&source, Position::new(3, 1)).is_none());

        let edit = rename(&source, &uri, Position::new(4, 10), "theme")
            .expect("rename should succeed")
            .expect("rename should produce an edit");
        let changes = edit.changes.expect("workspace edit should use changes");
        let edits = changes.get(&uri).expect("edits for the open document");
        // Later occurrence first (arrangement), then declaration.
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].new_text, "theme");
        assert_eq!(
            edits[0].range,
            Range::new(Position::new(5, 16), Position::new(5, 22))
        );
        assert_eq!(
            edits[1].range,
            Range::new(Position::new(4, 10), Position::new(4, 16))
        );

        // First.melody must stay untouched: only one declaration + one use in Second.
        let first = rename(&source, &uri, Position::new(1, 10), "theme")
            .expect("rename should succeed")
            .expect("rename should produce an edit");
        assert_eq!(
            first.changes.as_ref().map(|changes| changes[&uri].len()),
            Some(1)
        );
    }

    #[test]
    fn rejects_invalid_or_conflicting_rename_names() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "song \"Test\" {\n",
                "  pattern melody = sequence {}\n",
                "  pattern bass = sequence {}\n",
                "  arrangement { melody }\n",
                "}\n",
            ),
        );
        let uri = "file:///test.sym".parse::<Uri>().expect("URI should parse");

        let invalid = rename(&source, &uri, Position::new(1, 10), "1bad")
            .expect_err("invalid identifiers should error");
        assert!(
            invalid
                .message
                .contains("not a valid Symphra identifier")
        );

        let keyword = rename(&source, &uri, Position::new(1, 10), "pattern")
            .expect_err("keywords should error");
        assert!(
            keyword
                .message
                .contains("not a valid Symphra identifier")
        );

        let conflict = rename(&source, &uri, Position::new(1, 10), "bass")
            .expect_err("same-kind name collisions should error");
        assert!(conflict.message.contains("already exists"));
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
