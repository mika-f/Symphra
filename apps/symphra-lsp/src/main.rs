use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use symphra_compiler::compile;
use symphra_syntax::ast::{
    ArrangementEntry, ChordPitches, Declaration, Identifier, PatternBody, PlaySource,
    PlayStatement, SequenceItem, SongDeclaration, SongStatement, TrackBody, TrackEffect,
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
    DocumentFormattingParams, DocumentHighlight, DocumentHighlightKind, DocumentHighlightParams,
    DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeParams, InitializeResult, InlayHint, InlayHintKind, InlayHintLabel, InlayHintOptions,
    InlayHintParams, InlayHintServerCapabilities, Location, MarkupContent, MarkupKind, OneOf,
    Position, PositionEncodingKind, PrepareRenameResponse, Range, ReferenceParams, RenameOptions,
    RenameParams, SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens,
    SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions, SemanticTokensParams,
    SemanticTokensResult, SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo,
    SymbolInformation, SymbolKind, TextDocumentIdentifier, TextDocumentPositionParams,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Uri, WorkDoneProgressOptions,
    WorkspaceEdit,
};
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

#[derive(Debug)]
struct Backend {
    client: Client,
    documents: RwLock<HashMap<Uri, SourceText>>,
    preview_tracks: RwLock<HashMap<Uri, PreviewTrackState>>,
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

    async fn section_preview(
        &self,
        params: SectionPreviewParams,
    ) -> Result<Option<SectionPreview>> {
        let documents = self.documents.read().await;
        Ok(documents.get(&params.text_document.uri).and_then(|source| {
            section_preview(source, params.position, params.section_name.as_deref())
        }))
    }

    async fn arrangement_preview(
        &self,
        params: ArrangementPreviewParams,
    ) -> Result<Option<SectionPreview>> {
        let documents = self.documents.read().await;
        Ok(documents
            .get(&params.text_document.uri)
            .and_then(|source| arrangement_preview(source, params.index)))
    }

    async fn set_preview_track_state(&self, params: PreviewTrackStateParams) -> Result<()> {
        self.preview_tracks.write().await.insert(
            params.text_document.uri,
            PreviewTrackState {
                muted: params.muted.into_iter().collect(),
                soloed: params.soloed.into_iter().collect(),
            },
        );
        self.client.code_lens_refresh().await
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SectionPreviewParams {
    text_document: TextDocumentIdentifier,
    position: Option<Position>,
    section_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArrangementPreviewParams {
    text_document: TextDocumentIdentifier,
    index: usize,
}

#[derive(Clone, Debug, Default)]
struct PreviewTrackState {
    muted: HashSet<String>,
    soloed: HashSet<String>,
}

#[derive(Debug, Deserialize)]
struct PreviewTrackStateParams {
    #[serde(rename = "textDocument")]
    text_document: TextDocumentIdentifier,
    muted: Vec<String>,
    soloed: Vec<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SectionPreview {
    name: String,
    start_frame: u64,
    end_frame: u64,
}

fn section_preview(
    source: &SourceText,
    position: Option<Position>,
    section_name: Option<&str>,
) -> Option<SectionPreview> {
    let parsed = parse(source.id, &source.text);
    if !parsed.diagnostics.is_empty() {
        return None;
    }
    let program = compile(&parsed.file).ok()?;
    let song_declaration = parsed.file.declarations.iter().find_map(|declaration| {
        if let Declaration::Song(song) = declaration {
            Some(song)
        } else {
            None
        }
    })?;
    let song = program.songs.first()?;
    let offset = position.and_then(|position| {
        source.byte_offset_utf16(SourcePosition {
            line: position.line,
            utf16_column: position.character,
        })
    });
    let target = song_declaration.statements.iter().find_map(|statement| {
        let SongStatement::Section(section) = statement else {
            return None;
        };
        let matches = section_name.map_or_else(
            || {
                offset
                    .is_some_and(|offset| section.span.start <= offset && offset < section.span.end)
            },
            |name| section.name.text == name,
        );
        matches.then_some(section)
    })?;
    let entries = song_declaration
        .statements
        .iter()
        .find_map(|statement| match statement {
            SongStatement::Arrangement { entries, .. } => Some(entries),
            _ => None,
        })?;
    let sections = song_declaration
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SongStatement::Section(section) => Some((section.name.text.as_str(), section.bars)),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let mut elapsed_bars = 0_u64;
    for entry in entries {
        let ArrangementEntry::Play { name, .. } = entry else {
            return None;
        };
        let bars = u64::from(*sections.get(name.text.as_str())?);
        if name.text == target.name.text {
            let end_bars = elapsed_bars.checked_add(bars)?;
            return Some(SectionPreview {
                name: target.name.text.clone(),
                start_frame: bars_to_frames(elapsed_bars, song, program.project.sample_rate_hz)?,
                end_frame: bars_to_frames(end_bars, song, program.project.sample_rate_hz)?,
            });
        }
        elapsed_bars = elapsed_bars.checked_add(bars)?;
    }
    None
}

fn arrangement_preview(source: &SourceText, target_index: usize) -> Option<SectionPreview> {
    let parsed = parse(source.id, &source.text);
    if !parsed.diagnostics.is_empty() {
        return None;
    }
    let program = compile(&parsed.file).ok()?;
    let song_declaration = parsed.file.declarations.iter().find_map(|declaration| {
        if let Declaration::Song(song) = declaration {
            Some(song)
        } else {
            None
        }
    })?;
    let song = program.songs.first()?;
    let entries = song_declaration
        .statements
        .iter()
        .find_map(|statement| match statement {
            SongStatement::Arrangement { entries, .. } => Some(entries),
            _ => None,
        })?;
    let sections = song_declaration
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SongStatement::Section(section) => Some((section.name.text.as_str(), section.bars)),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let mut elapsed_bars = 0_u64;
    let mut target = None;
    for (index, entry) in entries.iter().enumerate() {
        let ArrangementEntry::Play { name, .. } = entry else {
            return None;
        };
        if index == target_index {
            target = Some((name.text.clone(), elapsed_bars));
        }
        elapsed_bars = elapsed_bars.checked_add(u64::from(*sections.get(name.text.as_str())?))?;
    }
    let (name, start_bars) = target?;
    Some(SectionPreview {
        name,
        start_frame: bars_to_frames(start_bars, song, program.project.sample_rate_hz)?,
        end_frame: bars_to_frames(elapsed_bars, song, program.project.sample_rate_hz)?,
    })
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "the finite non-negative frame count is range-checked before conversion"
)]
fn bars_to_frames(
    bars: u64,
    song: &symphra_compiler::hir::Song,
    sample_rate_hz: u32,
) -> Option<u64> {
    let frames = bars as f64 * f64::from(song.meter.numerator) / f64::from(song.meter.denominator)
        * 240.0
        * f64::from(sample_rate_hz)
        / song.tempo_bpm;
    (frames.is_finite() && frames >= 0.0 && frames <= u64::MAX as f64)
        .then(|| frames.round() as u64)
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
                document_highlight_provider: Some(OneOf::Left(true)),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(false),
                }),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                })),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: semantic_tokens_legend(),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: Some(false),
                            work_done_progress_options: WorkDoneProgressOptions::default(),
                        },
                    ),
                ),
                inlay_hint_provider: Some(OneOf::Right(InlayHintServerCapabilities::Options(
                    InlayHintOptions {
                        resolve_provider: Some(false),
                        work_done_progress_options: WorkDoneProgressOptions::default(),
                    },
                ))),
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

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let documents = self.documents.read().await;
        let position = params.text_document_position_params.position;
        let uri = params.text_document_position_params.text_document.uri;
        Ok(documents
            .get(&uri)
            .map(|source| document_highlights(source, position)))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let documents = self.documents.read().await;
        let uri = params.text_document.uri;
        Ok(documents
            .get(&uri)
            .map(|source| SemanticTokensResult::Tokens(semantic_tokens(source))))
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let documents = self.documents.read().await;
        let uri = params.text_document.uri;
        Ok(documents
            .get(&uri)
            .map(|source| inlay_hints(source, &params.range)))
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let documents = self.documents.read().await;
        let uri = params.text_document.uri;
        let preview_tracks = self.preview_tracks.read().await;
        let state = preview_tracks.get(&uri).cloned().unwrap_or_default();
        Ok(documents
            .get(&uri)
            .map(|source| code_lenses(source, &uri, &state)))
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
                        SongStatement::EffectPreset(preset) => symbol(
                            source,
                            preset.name.text.clone(),
                            SymbolKind::OBJECT,
                            preset.span,
                            preset.name.span,
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
    /// `pattern x = arpeggiate y { … }` body fields.
    Arpeggiate,
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

    let mut items: Vec<CompletionItem> = completion_labels(block, line_tokens)
        .iter()
        .map(|label| CompletionItem {
            label: (*label).to_owned(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Symphra keyword".to_owned()),
            ..CompletionItem::default()
        })
        .collect();

    if let Some(kind) = name_completion_kind(block, line_tokens) {
        let Ok(offset) = u32::try_from(byte_offset) else {
            return items;
        };
        items.extend(named_completion_items(source, offset, kind));
    }
    items
}

/// Contexts where a declared song-local name is expected next.
fn name_completion_kind(
    block: Option<CompletionBlock>,
    line_tokens: &[Token],
) -> Option<NamedKind> {
    // Ignore a trailing partial identifier so `play mel|` still matches `play`.
    let tokens = match line_tokens.last() {
        Some(Token {
            kind: TokenKind::Identifier,
            ..
        }) => &line_tokens[..line_tokens.len().saturating_sub(1)],
        _ => line_tokens,
    };

    name_completion_kind_play(block, tokens)
        .or_else(|| name_completion_kind_bindings(block, tokens))
        .or_else(|| name_completion_kind_sugar(tokens))
}

/// `play` / arrangement / `trigger_with` reference sites.
fn name_completion_kind_play(
    block: Option<CompletionBlock>,
    tokens: &[Token],
) -> Option<NamedKind> {
    match tokens {
        // `play <pattern>` inside a track or layer use.
        [
            Token {
                kind: TokenKind::Play,
                ..
            },
        ] if matches!(block, Some(CompletionBlock::Track | CompletionBlock::Use)) => {
            Some(NamedKind::Pattern)
        }
        // `play track <name>` inside a section parallel block.
        [
            Token {
                kind: TokenKind::Play,
                ..
            },
            Token {
                kind: TokenKind::Track,
                ..
            },
        ] if matches!(block, Some(CompletionBlock::Parallel)) => Some(NamedKind::Track),
        // `play <section>` inside arrangement.
        [
            Token {
                kind: TokenKind::Play,
                ..
            },
        ] if matches!(block, Some(CompletionBlock::Arrangement)) => Some(NamedKind::Section),
        // Bare pattern occurrence in arrangement: empty line or partial name.
        [] if matches!(block, Some(CompletionBlock::Arrangement)) => Some(NamedKind::Pattern),
        // `arrangement { melody with <instrument> }`
        [
            ..,
            Token {
                kind: TokenKind::With,
                ..
            },
        ] if matches!(block, Some(CompletionBlock::Arrangement))
            && !matches!(
                tokens.first(),
                Some(Token {
                    kind: TokenKind::Play,
                    ..
                })
            ) =>
        {
            Some(NamedKind::Instrument)
        }
        // `play drum "kick" with <rhythm>` or `play ... |> trigger_with <rhythm>`.
        [
            Token {
                kind: TokenKind::Play,
                ..
            },
            Token {
                kind: TokenKind::Drum,
                ..
            },
            Token {
                kind: TokenKind::String,
                ..
            },
            Token {
                kind: TokenKind::With,
                ..
            },
        ]
        | [
            ..,
            Token {
                kind: TokenKind::TriggerWith,
                ..
            },
        ] => Some(NamedKind::Rhythm),
        _ => None,
    }
}

/// `instrument` / `use` / `effect` binding sites inside a track (or override).
fn name_completion_kind_bindings(
    block: Option<CompletionBlock>,
    tokens: &[Token],
) -> Option<NamedKind> {
    match tokens {
        [
            Token {
                kind: TokenKind::Instrument,
                ..
            },
        ] if matches!(block, Some(CompletionBlock::Track)) => Some(NamedKind::Instrument),
        [
            Token {
                kind: TokenKind::Use,
                ..
            },
        ] if matches!(block, Some(CompletionBlock::Layer | CompletionBlock::Use)) => {
            Some(NamedKind::Instrument)
        }
        // Song-level `effect name = …` is a declaration site — excluded so the
        // next token is treated as a fresh name, not a preset reference.
        [
            Token {
                kind: TokenKind::Effect,
                ..
            },
        ] if matches!(
            block,
            Some(CompletionBlock::Track | CompletionBlock::Parallel | CompletionBlock::Other)
        ) =>
        {
            Some(NamedKind::Effect)
        }
        _ => None,
    }
}

/// RFC 0001 pattern-source sites: `arpeggiate <name>` and `pattern x = <name>`.
fn name_completion_kind_sugar(tokens: &[Token]) -> Option<NamedKind> {
    match tokens {
        // Trailing `arpeggiate` so a full `pattern … = arpeggiate` line matches.
        [
            ..,
            Token {
                kind: TokenKind::Arpeggiate,
                ..
            },
        ]
        | [
            Token {
                kind: TokenKind::Pattern,
                ..
            },
            Token {
                kind: TokenKind::Identifier,
                ..
            },
            Token {
                kind: TokenKind::Equal,
                ..
            },
        ] => Some(NamedKind::Pattern),
        _ => None,
    }
}

fn named_completion_items(
    source: &SourceText,
    offset: u32,
    kind: NamedKind,
) -> Vec<CompletionItem> {
    let parsed = parse(source.id, &source.text);
    let Some(song) = parsed.file.declarations.iter().find_map(|declaration| {
        let Declaration::Song(song) = declaration else {
            return None;
        };
        // Song spans cover nested blocks, so the cursor need not sit on `song`.
        span_contains(song.span, offset).then_some(song)
    }) else {
        return Vec::new();
    };
    let (item_kind, detail) = match kind {
        NamedKind::Pattern => (CompletionItemKind::FUNCTION, "pattern"),
        NamedKind::Instrument => (CompletionItemKind::VARIABLE, "instrument"),
        NamedKind::Rhythm => (CompletionItemKind::FUNCTION, "rhythm"),
        NamedKind::Track => (CompletionItemKind::VARIABLE, "track"),
        NamedKind::Section => (CompletionItemKind::MODULE, "section"),
        NamedKind::Effect => (CompletionItemKind::VARIABLE, "effect"),
    };
    named_declarations(song)
        .filter(|(declaration_kind, _)| *declaration_kind == kind)
        .map(|(_, name)| CompletionItem {
            label: name.text.clone(),
            kind: Some(item_kind),
            detail: Some(detail.to_owned()),
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
            // Pattern body keywords, plus derivation via a bare source name
            // (names come from name_completion_kind).
            &["sequence", "steps", "arpeggiate"]
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
    } else if matches!(line_tokens.last(), Some(token) if token.kind == TokenKind::Style) {
        // Arpeggio walk orders (validated identifiers, same set the compiler
        // accepts on `style` inside `arpeggiate`).
        &["up", "down", "up_down", "down_up", "as_written"]
    } else if matches!(line_tokens.last(), Some(token) if token.kind == TokenKind::Master) {
        &["limiter"]
    } else if matches!(line_tokens.last(), Some(token) if token.kind == TokenKind::Parallel) {
        &["exact"]
    } else if matches!(line_tokens.last(), Some(token) if token.kind == TokenKind::Play) {
        if matches!(block, Some(CompletionBlock::Parallel)) {
            &["track"]
        } else if matches!(block, Some(CompletionBlock::Arrangement)) {
            // Arrangement uses `play <section>`; `play drum` is a track-body form.
            &[]
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
    } else if name_taking_keyword_line(block, line_tokens) {
        // The keyword is already typed; only declared-name items apply next.
        &[]
    } else if completion_statement_start(line_tokens) {
        completion_block_labels(block)
    } else {
        &[]
    }
}

/// `instrument <name>` / `use <name>` lines: suppress statement keywords once the
/// keyword itself is present so name completions are the only suggestions.
fn name_taking_keyword_line(block: Option<CompletionBlock>, line_tokens: &[Token]) -> bool {
    matches!(
        (block, line_tokens),
        (
            Some(CompletionBlock::Track),
            [Token {
                kind: TokenKind::Instrument,
                ..
            }]
        ) | (
            Some(CompletionBlock::Layer | CompletionBlock::Use),
            [Token {
                kind: TokenKind::Use,
                ..
            }]
        )
    )
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
        Some(CompletionBlock::Arpeggiate) => &["style", "step", "octaves"],
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
            TokenKind::Arpeggiate => pending = Some(CompletionBlock::Arpeggiate),
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
                    | TokenKind::Spread
                    | TokenKind::Step
                    | TokenKind::Arpeggiate
                    | TokenKind::Style
                    | TokenKind::Octaves
                    | TokenKind::Fit,
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
    /// Song-level `effect <name> = …` preset (RFC 0001 S10).
    Effect,
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
    let Some((declaration, ref_spans)) = named_symbol_spans(source, position) else {
        return Vec::new();
    };

    let mut locations = Vec::new();
    if include_declaration && let Some(range) = lsp_range(source, declaration) {
        locations.push(Location::new(uri.clone(), range));
    }
    for span in ref_spans {
        if let Some(range) = lsp_range(source, span) {
            locations.push(Location::new(uri.clone(), range));
        }
    }
    locations
}

fn document_highlights(source: &SourceText, position: Position) -> Vec<DocumentHighlight> {
    let Some((declaration, ref_spans)) = named_symbol_spans(source, position) else {
        return Vec::new();
    };

    let mut highlights = Vec::with_capacity(ref_spans.len() + 1);
    if let Some(range) = lsp_range(source, declaration) {
        highlights.push(DocumentHighlight {
            range,
            kind: Some(DocumentHighlightKind::WRITE),
        });
    }
    for span in ref_spans {
        if let Some(range) = lsp_range(source, span) {
            highlights.push(DocumentHighlight {
                range,
                kind: Some(DocumentHighlightKind::READ),
            });
        }
    }
    highlights
}

/// Song-local declaration span plus reference spans for the symbol under `position`.
fn named_symbol_spans(
    source: &SourceText,
    position: Position,
) -> Option<(SourceSpan, Vec<SourceSpan>)> {
    let offset = source.byte_offset_utf16(SourcePosition {
        line: position.line,
        utf16_column: position.character,
    })?;
    let parsed = parse(source.id, &source.text);
    parsed.file.declarations.iter().find_map(|declaration| {
        let Declaration::Song(song) = declaration else {
            return None;
        };
        let (kind, name, declaration_span) = named_symbol_at(song, offset)?;
        Some((declaration_span, reference_spans(song, kind, name)))
    })
}

fn code_lenses(source: &SourceText, uri: &Uri, state: &PreviewTrackState) -> Vec<CodeLens> {
    let parsed = parse(source.id, &source.text);
    let mut lenses = Vec::new();
    for declaration in &parsed.file.declarations {
        let Declaration::Song(song) = declaration else {
            continue;
        };
        for section in song.statements.iter().filter_map(|statement| {
            if let SongStatement::Section(section) = statement {
                Some(section)
            } else {
                None
            }
        }) {
            let Some(range) = lsp_range(source, section.name.span) else {
                continue;
            };
            lenses.push(CodeLens {
                range,
                command: Some(Command {
                    title: "▶ Loop section".to_owned(),
                    command: "symphra.loopSection".to_owned(),
                    arguments: Some(vec![
                        serde_json::Value::String(uri.as_str().to_owned()),
                        serde_json::Value::String(section.name.text.clone()),
                    ]),
                }),
                data: None,
            });
        }
        if let Some(entries) = song
            .statements
            .iter()
            .find_map(|statement| match statement {
                SongStatement::Arrangement { entries, .. } => Some(entries),
                _ => None,
            })
        {
            for (index, entry) in entries.iter().enumerate() {
                let ArrangementEntry::Play { name, .. } = entry else {
                    continue;
                };
                let Some(range) = lsp_range(source, name.span) else {
                    continue;
                };
                lenses.push(CodeLens {
                    range,
                    command: Some(Command {
                        title: "▶ From here".to_owned(),
                        command: "symphra.playFromHere".to_owned(),
                        arguments: Some(vec![
                            serde_json::Value::String(uri.as_str().to_owned()),
                            serde_json::Value::from(index),
                        ]),
                    }),
                    data: None,
                });
            }
        }
        lenses.extend(track_preview_code_lenses(source, uri, song, state));
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

fn track_preview_code_lenses(
    source: &SourceText,
    uri: &Uri,
    song: &SongDeclaration,
    state: &PreviewTrackState,
) -> Vec<CodeLens> {
    let mut lenses = Vec::new();
    for track in song.statements.iter().filter_map(|statement| {
        if let SongStatement::Track(track) = statement {
            Some(track)
        } else {
            None
        }
    }) {
        let Some(range) = lsp_range(source, track.name.span) else {
            continue;
        };
        for (title, command) in [
            (
                if state.muted.contains(&track.name.text) {
                    "Unmute"
                } else {
                    "Mute"
                },
                "symphra.toggleMute",
            ),
            (
                if state.soloed.contains(&track.name.text) {
                    "Unsolo"
                } else {
                    "Solo"
                },
                "symphra.toggleSolo",
            ),
        ] {
            lenses.push(CodeLens {
                range,
                command: Some(Command {
                    title: title.to_owned(),
                    command: command.to_owned(),
                    arguments: Some(vec![
                        serde_json::Value::String(uri.as_str().to_owned()),
                        serde_json::Value::String(track.name.text.clone()),
                    ]),
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
        (declaration_kind == kind && identifier.text == name).then_some((
            kind,
            identifier.text.as_str(),
            occurrence,
        ))
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
            Self::Effect => "effect",
        }
    }
}

/// Declaration or resolved reference under `offset` → `(kind, name, declaration span)`.
fn named_symbol_at(song: &SongDeclaration, offset: u32) -> Option<(NamedKind, &str, SourceSpan)> {
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
    song.statements
        .iter()
        .filter_map(|statement| match statement {
            SongStatement::Pattern(pattern) => Some((NamedKind::Pattern, &pattern.name)),
            SongStatement::Instrument(instrument) => {
                Some((NamedKind::Instrument, &instrument.name))
            }
            SongStatement::Rhythm(rhythm) => Some((NamedKind::Rhythm, &rhythm.name)),
            SongStatement::Track(track) => Some((NamedKind::Track, &track.name)),
            SongStatement::Section(section) => Some((NamedKind::Section, &section.name)),
            SongStatement::EffectPreset(preset) => Some((NamedKind::Effect, &preset.name)),
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
                    visit(NamedKind::Track, &track.name);
                    visit_track_effect_references(track.effect.as_ref(), &mut visit);
                }
            }
            SongStatement::Track(track) => {
                visit_track_effect_references(track.effect.as_ref(), &mut visit);
                match &track.body {
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
                }
            }
            SongStatement::Pattern(pattern) => {
                visit_pattern_body_references(&pattern.body, &mut visit);
            }
            _ => {}
        }
    }
}

fn visit_track_effect_references(
    effect: Option<&TrackEffect>,
    visit: &mut impl FnMut(NamedKind, &Identifier),
) {
    if let Some(TrackEffect::Preset(name)) = effect {
        visit(NamedKind::Effect, name);
    }
}

fn visit_pattern_body_references(
    body: &PatternBody,
    visit: &mut impl FnMut(NamedKind, &Identifier),
) {
    match body {
        // `pattern x = y |> …` and `pattern x = arpeggiate y { … }` both
        // name another pattern as their source material (RFC 0001 S6/S7).
        PatternBody::Derived { source, .. } | PatternBody::Arpeggiate { source, .. } => {
            visit(NamedKind::Pattern, source);
        }
        PatternBody::Sequence { .. } | PatternBody::Steps { .. } => {}
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

/// Legend indices used by [`semantic_tokens`]. Keep in sync with
/// [`semantic_tokens_legend`].
const SEMANTIC_TOKEN_KEYWORD: u32 = 0;
const SEMANTIC_TOKEN_FUNCTION: u32 = 1;
const SEMANTIC_TOKEN_VARIABLE: u32 = 2;
const SEMANTIC_TOKEN_NAMESPACE: u32 = 3;
const SEMANTIC_TOKEN_STRING: u32 = 4;
const SEMANTIC_TOKEN_NUMBER: u32 = 5;
const SEMANTIC_TOKEN_COMMENT: u32 = 6;
const SEMANTIC_TOKEN_TYPE: u32 = 7;
/// Bit 0 of the modifier bitset: declaration site of a named symbol.
const SEMANTIC_MOD_DECLARATION: u32 = 1;

fn semantic_tokens_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::KEYWORD,
            SemanticTokenType::FUNCTION,
            SemanticTokenType::VARIABLE,
            SemanticTokenType::NAMESPACE,
            SemanticTokenType::STRING,
            SemanticTokenType::NUMBER,
            SemanticTokenType::COMMENT,
            SemanticTokenType::TYPE,
        ],
        token_modifiers: vec![SemanticTokenModifier::DECLARATION],
    }
}

fn semantic_tokens(source: &SourceText) -> SemanticTokens {
    let lexed = lex(source.id, &source.text);
    let name_tokens = named_semantic_classifications(source);
    let mut absolute = Vec::new();

    for comment in &lexed.comments {
        if let Some(token) =
            absolute_semantic_token(source, comment.span, SEMANTIC_TOKEN_COMMENT, 0)
        {
            absolute.push(token);
        }
    }

    for token in &lexed.tokens {
        if token.kind == TokenKind::Eof {
            continue;
        }
        let (token_type, modifiers) = if let Some(classification) = name_tokens.get(&token.span) {
            *classification
        } else if let Some(classification) = lex_token_classification(token) {
            classification
        } else {
            continue;
        };
        if let Some(encoded) = absolute_semantic_token(source, token.span, token_type, modifiers) {
            absolute.push(encoded);
        }
    }

    absolute.sort_by(|left, right| {
        left.line
            .cmp(&right.line)
            .then(left.start.cmp(&right.start))
    });
    SemanticTokens {
        result_id: None,
        data: encode_semantic_tokens(&absolute),
    }
}

#[derive(Clone, Copy)]
struct AbsoluteSemanticToken {
    line: u32,
    start: u32,
    length: u32,
    token_type: u32,
    modifiers: u32,
}

fn absolute_semantic_token(
    source: &SourceText,
    span: SourceSpan,
    token_type: u32,
    modifiers: u32,
) -> Option<AbsoluteSemanticToken> {
    let range = source.utf16_range(span)?;
    // Symphra tokens and comments are single-line; skip anything that wraps.
    if range.start.line != range.end.line {
        return None;
    }
    let length = range
        .end
        .utf16_column
        .saturating_sub(range.start.utf16_column);
    (length > 0).then_some(AbsoluteSemanticToken {
        line: range.start.line,
        start: range.start.utf16_column,
        length,
        token_type,
        modifiers,
    })
}

fn encode_semantic_tokens(tokens: &[AbsoluteSemanticToken]) -> Vec<SemanticToken> {
    let mut previous_line = 0;
    let mut previous_start = 0;
    tokens
        .iter()
        .map(|token| {
            let delta_line = token.line - previous_line;
            let delta_start = if delta_line == 0 {
                token.start - previous_start
            } else {
                token.start
            };
            previous_line = token.line;
            previous_start = token.start;
            SemanticToken {
                delta_line,
                delta_start,
                length: token.length,
                token_type: token.token_type,
                token_modifiers_bitset: token.modifiers,
            }
        })
        .collect()
}

fn lex_token_classification(token: &Token) -> Option<(u32, u32)> {
    if keyword_description(token.kind).is_some() {
        return Some((SEMANTIC_TOKEN_KEYWORD, 0));
    }
    match token.kind {
        TokenKind::String => Some((SEMANTIC_TOKEN_STRING, 0)),
        TokenKind::Integer | TokenKind::Decimal => Some((SEMANTIC_TOKEN_NUMBER, 0)),
        TokenKind::Identifier if looks_like_pitch(&token.text) => Some((SEMANTIC_TOKEN_TYPE, 0)),
        _ => None,
    }
}

/// Heuristic pitch form used by the lexer for note names (`C4`, `F#3`, `Bb-1`).
fn looks_like_pitch(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(letter) = chars.next() else {
        return false;
    };
    if !matches!(letter, 'A'..='G') {
        return false;
    }
    let rest = chars.as_str();
    let rest = rest
        .strip_prefix('#')
        .or_else(|| rest.strip_prefix('b'))
        .unwrap_or(rest);
    let rest = rest.strip_prefix('-').unwrap_or(rest);
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

fn named_semantic_classifications(source: &SourceText) -> HashMap<SourceSpan, (u32, u32)> {
    let parsed = parse(source.id, &source.text);
    let mut map = HashMap::new();
    for declaration in &parsed.file.declarations {
        let Declaration::Song(song) = declaration else {
            continue;
        };
        for (kind, name) in named_declarations(song) {
            map.insert(
                name.span,
                (named_kind_token_type(kind), SEMANTIC_MOD_DECLARATION),
            );
        }
        visit_name_references(song, |kind, identifier| {
            if declaration_span(song, kind, &identifier.text).is_some() {
                map.entry(identifier.span)
                    .or_insert((named_kind_token_type(kind), 0));
            }
        });
    }
    map
}

const fn named_kind_token_type(kind: NamedKind) -> u32 {
    match kind {
        NamedKind::Pattern | NamedKind::Rhythm => SEMANTIC_TOKEN_FUNCTION,
        NamedKind::Instrument | NamedKind::Track | NamedKind::Effect => SEMANTIC_TOKEN_VARIABLE,
        NamedKind::Section => SEMANTIC_TOKEN_NAMESPACE,
    }
}

fn inlay_hints(source: &SourceText, visible_range: &Range) -> Vec<InlayHint> {
    let mut hints = Vec::new();

    for (span, midi) in pitch_midi_spans(source) {
        if let Some(hint) = trailing_inlay_hint(source, span, format!("MIDI {midi}"), visible_range)
        {
            hints.push(hint);
        }
    }

    // `G3:maj7` only spells the root in the AST; show the expanded voicing
    // after the quality so the sugar is readable without expanding by hand.
    for (span, midis) in chord_symbol_voicing_spans(source) {
        let label = format!(
            "MIDI {}",
            midis
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        );
        if let Some(hint) = trailing_inlay_hint(source, span, label, visible_range) {
            hints.push(hint);
        }
    }

    let parsed = parse(source.id, &source.text);
    for declaration in &parsed.file.declarations {
        let Declaration::Song(song) = declaration else {
            continue;
        };
        visit_name_references(song, |kind, identifier| {
            if declaration_span(song, kind, &identifier.text).is_none() {
                return;
            }
            if let Some(hint) = trailing_inlay_hint(
                source,
                identifier.span,
                kind.label().to_owned(),
                visible_range,
            ) {
                hints.push(hint);
            }
        });
    }

    hints.sort_by(|left, right| {
        left.position
            .line
            .cmp(&right.position.line)
            .then(left.position.character.cmp(&right.position.character))
            .then_with(|| inlay_label_text(&left.label).cmp(inlay_label_text(&right.label)))
    });
    hints
}

fn trailing_inlay_hint(
    source: &SourceText,
    span: SourceSpan,
    label: String,
    visible_range: &Range,
) -> Option<InlayHint> {
    let range = lsp_range(source, span)?;
    let position = range.end;
    if !range_contains_position(visible_range, position) {
        return None;
    }
    Some(InlayHint {
        position,
        label: InlayHintLabel::String(label),
        kind: Some(InlayHintKind::TYPE),
        text_edits: None,
        tooltip: None,
        padding_left: Some(true),
        padding_right: None,
        data: None,
    })
}

fn range_contains_position(range: &Range, position: Position) -> bool {
    let after_start = position.line > range.start.line
        || (position.line == range.start.line && position.character >= range.start.character);
    let before_end = position.line < range.end.line
        || (position.line == range.end.line && position.character <= range.end.character);
    after_start && before_end
}

fn inlay_label_text(label: &InlayHintLabel) -> &str {
    match label {
        InlayHintLabel::String(text) => text.as_str(),
        InlayHintLabel::LabelParts(parts) => parts.first().map_or("", |part| part.value.as_str()),
    }
}

fn pitch_description(source: &SourceText, span: SourceSpan) -> Option<String> {
    pitch_midi_spans(source)
        .into_iter()
        .find_map(|(pitch_span, midi)| (pitch_span == span).then(|| format!("MIDI note {midi}.")))
}

/// Compiled MIDI values for every written sequence pitch that the compiler lowered.
///
/// Chord symbols contribute only their root here — the full voicing is
/// exposed separately by [`chord_symbol_voicing_spans`] for inlay labels.
fn pitch_midi_spans(source: &SourceText) -> Vec<(SourceSpan, u8)> {
    let mut pitches = Vec::new();
    for_compiled_sequence_steps(source, |expanded, step| match (expanded, step) {
        (SequenceItem::Note(source_note), symphra_compiler::hir::PatternStep::Note(note)) => {
            pitches.push((source_note.pitch.span, note.midi_pitch));
        }
        (SequenceItem::Chord(source_chord), symphra_compiler::hir::PatternStep::Chord(chord)) => {
            for (source_pitch, note) in source_chord.pitches.spelled().iter().zip(&chord.notes) {
                pitches.push((source_pitch.span, note.midi_pitch));
            }
        }
        _ => {}
    });
    pitches
}

/// Expanded MIDI notes for each `root:quality` chord symbol, keyed on the
/// quality span so an inlay can sit after `maj7` rather than the root alone.
fn chord_symbol_voicing_spans(source: &SourceText) -> Vec<(SourceSpan, Vec<u8>)> {
    let mut voicings = Vec::new();
    for_compiled_sequence_steps(source, |expanded, step| {
        let (SequenceItem::Chord(source_chord), symphra_compiler::hir::PatternStep::Chord(chord)) =
            (expanded, step)
        else {
            return;
        };
        let ChordPitches::Symbol { quality, .. } = &source_chord.pitches else {
            return;
        };
        let midis: Vec<u8> = chord.notes.iter().map(|note| note.midi_pitch).collect();
        if !midis.is_empty() {
            voicings.push((quality.span, midis));
        }
    });
    voicings
}

/// Walks each successfully compiled sequence pattern's expanded items against
/// the matching HIR steps. Shared by pitch inlays/hover and chord-symbol
/// voicing labels so repetition expansion stays in one place.
fn for_compiled_sequence_steps(
    source: &SourceText,
    mut visit: impl FnMut(&SequenceItem, &symphra_compiler::hir::PatternStep),
) {
    let parsed = parse(source.id, &source.text);
    let Some(program) = parsed
        .diagnostics
        .is_empty()
        .then(|| compile(&parsed.file).ok())
        .flatten()
    else {
        return;
    };
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
            // Repetitions have to be expanded here the same way the compiler
            // expanded them, or every pitch after a `* N` would line up with
            // the wrong lowered step.
            let Some(items) = symphra_compiler::expand::sequence_items(items) else {
                continue;
            };
            for (expanded, step) in items.iter().zip(&pattern.steps) {
                visit(expanded.item, step);
            }
        }
    }
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
        TokenKind::Effect => {
            "declares a named effect preset, or applies an effect (inline or by preset name) to a track."
        }
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
        TokenKind::Step => {
            "sets a default duration for sequence items that omit `for`, such as `sequence step 1/8`."
        }
        TokenKind::Arpeggiate => {
            "builds a pattern by walking another pattern's chords one note at a time."
        }
        TokenKind::Style => {
            "sets an arpeggio walk order, such as `up`, `down`, `up_down`, `down_up`, or `as_written`."
        }
        TokenKind::Octaves => {
            "caps how many octaves of a chord's tones an arpeggio may use before wrapping."
        }
        TokenKind::Fit => {
            "on `repeat fit`, repeats a pattern enough times to fill the enclosing section."
        }
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
    let (service, socket) = LspService::build(|client| Backend {
        client,
        documents: RwLock::new(HashMap::new()),
        preview_tracks: RwLock::new(HashMap::new()),
        hierarchical_symbols: AtomicBool::new(false),
    })
    .custom_method("symphra/sectionPreview", Backend::section_preview)
    .custom_method("symphra/arrangementPreview", Backend::arrangement_preview)
    .custom_method(
        "symphra/setPreviewTrackState",
        Backend::set_preview_track_state,
    )
    .finish();
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        PreviewTrackState, SEMANTIC_MOD_DECLARATION, SEMANTIC_TOKEN_COMMENT,
        SEMANTIC_TOKEN_FUNCTION, SEMANTIC_TOKEN_KEYWORD, SEMANTIC_TOKEN_NUMBER,
        SEMANTIC_TOKEN_STRING, SEMANTIC_TOKEN_TYPE, SectionPreview, SourceId, SourceText,
        arrangement_preview, code_lenses, completions, definition, diagnostics,
        document_highlights, document_symbols, flatten_document_symbols, formatting_edits, hover,
        inlay_hints, prepare_rename, references, rename, section_preview, semantic_tokens,
    };

    #[test]
    fn resolves_the_section_at_the_cursor_to_arrangement_frames() {
        let text = include_str!("../../../examples/draft-0.1/001-example.sym");
        let source = SourceText::new(SourceId(0), "test.sym", text);
        let drop_line = text
            .lines()
            .position(|line| line.trim_start().starts_with("section drop bars"))
            .expect("example should declare drop");

        let preview = section_preview(
            &source,
            Some(Position::new(
                u32::try_from(drop_line).expect("line fits in u32"),
                10,
            )),
            None,
        )
        .expect("drop should be arranged");

        assert_eq!(
            preview,
            super::SectionPreview {
                name: "drop".to_owned(),
                start_frame: 614_400,
                end_frame: 1_228_800,
            }
        );
    }

    #[test]
    fn resolves_an_arrangement_entry_from_its_start_to_the_song_end() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "project { seed 1 sample_rate 8khz output mono }\n",
                "song \"Test\" {\n",
                "  tempo 120bpm meter 4/4 key C major\n",
                "  instrument tone = sine\n",
                "  pattern notes = sequence { note C4 for 1/4 }\n",
                "  track bass role bass { instrument tone play notes }\n",
                "  section intro bars 1 { parallel { play track bass } }\n",
                "  section drop bars 2 { parallel { play track bass } }\n",
                "  arrangement { play intro play drop play intro }\n",
                "}\n",
            ),
        );

        let preview = arrangement_preview(&source, 1).expect("drop should resolve");

        assert_eq!(
            preview,
            SectionPreview {
                name: "drop".to_owned(),
                start_frame: 16_000,
                end_frame: 64_000,
            }
        );
    }
    use tower_lsp_server::ls_types::{
        Command, CompletionItemKind, DiagnosticSeverity, DocumentHighlightKind, InlayHintLabel,
        Position, PrepareRenameResponse, Range, SemanticToken, SymbolKind, Uri,
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
            ["sequence", "steps", "arpeggiate"]
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
    fn completes_declared_patterns_on_play() {
        let labels = |source: &str, line, character| {
            completions(
                &SourceText::new(SourceId(0), "test.sym", source),
                Position::new(line, character),
            )
            .into_iter()
            .map(|item| item.label)
            .collect::<Vec<_>>()
        };

        // `play <pattern>` offers declared patterns after the `drum` keyword.
        assert_eq!(
            labels(
                concat!(
                    "song \"Test\" {\n",
                    "  pattern melody = sequence {}\n",
                    "  pattern bass = sequence {}\n",
                    "  track lead role harmony {\n",
                    "    play \n",
                    "  }\n",
                    "}\n",
                ),
                4,
                9,
            ),
            ["drum", "melody", "bass"]
        );

        // Partial identifier after `play` still matches the pattern context
        // (keyword suggestions no longer apply once an identifier is started).
        assert_eq!(
            labels(
                concat!(
                    "song \"Test\" {\n",
                    "  pattern melody = sequence {}\n",
                    "  track lead role harmony {\n",
                    "    play mel\n",
                    "  }\n",
                    "}\n",
                ),
                3,
                12,
            ),
            ["melody"]
        );

        // Names stay song-local: First's pattern is not offered inside Second.
        assert_eq!(
            labels(
                concat!(
                    "song \"First\" {\n",
                    "  pattern other = sequence {}\n",
                    "}\n",
                    "song \"Second\" {\n",
                    "  pattern melody = sequence {}\n",
                    "  track lead role harmony {\n",
                    "    play \n",
                    "  }\n",
                    "}\n",
                ),
                6,
                9,
            ),
            ["drum", "melody"]
        );

        let play_items = completions(
            &SourceText::new(
                SourceId(0),
                "test.sym",
                concat!(
                    "song \"Test\" {\n",
                    "  pattern melody = sequence {}\n",
                    "  track lead role harmony {\n",
                    "    play \n",
                    "  }\n",
                    "}\n",
                ),
            ),
            Position::new(3, 9),
        );
        let melody = play_items
            .iter()
            .find(|item| item.label == "melody")
            .expect("melody completion");
        assert_eq!(melody.kind, Some(CompletionItemKind::FUNCTION));
        assert_eq!(melody.detail.as_deref(), Some("pattern"));
    }

    #[test]
    fn completes_declared_instruments_on_instrument_and_use() {
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
                concat!(
                    "song \"Test\" {\n",
                    "  instrument lead = triangle\n",
                    "  instrument sub = triangle\n",
                    "  track chords role harmony {\n",
                    "    instrument \n",
                    "  }\n",
                    "}\n",
                ),
                4,
                15,
            ),
            ["lead", "sub"]
        );
        assert_eq!(
            labels(
                concat!(
                    "song \"Test\" {\n",
                    "  instrument lead = triangle\n",
                    "  track bass role low {\n",
                    "    layer {\n",
                    "      use \n",
                    "    }\n",
                    "  }\n",
                    "}\n",
                ),
                4,
                10,
            ),
            ["lead"]
        );
    }

    #[test]
    fn completes_declared_rhythms_tracks_and_sections() {
        let labels = |source: &str, line, character| {
            completions(
                &SourceText::new(SourceId(0), "test.sym", source),
                Position::new(line, character),
            )
            .into_iter()
            .map(|item| item.label)
            .collect::<Vec<_>>()
        };

        // `trigger_with` offers rhythm names.
        assert_eq!(
            labels(
                concat!(
                    "song \"Test\" {\n",
                    "  rhythm stabs resolution 1/8 { hit rest }\n",
                    "  rhythm pulse resolution 1/8 { hit hit }\n",
                    "  pattern melody = sequence {}\n",
                    "  track lead role harmony {\n",
                    "    instrument lead\n",
                    "    play melody |> trigger_with \n",
                    "  }\n",
                    "}\n",
                ),
                6,
                31,
            ),
            ["stabs", "pulse"]
        );

        // Arrangement bare entries and `with` / `play` contexts.
        assert_eq!(
            labels(
                concat!(
                    "song \"Test\" {\n",
                    "  pattern melody = sequence {}\n",
                    "  instrument lead = triangle\n",
                    "  section intro bars 2 { parallel { play track pad } }\n",
                    "  arrangement {\n",
                    "    \n",
                    "  }\n",
                    "}\n",
                ),
                5,
                4,
            ),
            ["melody"]
        );
        assert_eq!(
            labels(
                concat!(
                    "song \"Test\" {\n",
                    "  pattern melody = sequence {}\n",
                    "  instrument lead = triangle\n",
                    "  arrangement {\n",
                    "    melody with \n",
                    "  }\n",
                    "}\n",
                ),
                4,
                16,
            ),
            ["lead"]
        );
        assert_eq!(
            labels(
                concat!(
                    "song \"Test\" {\n",
                    "  section intro bars 2 { parallel { play track pad } }\n",
                    "  section verse bars 4 { parallel { play track pad } }\n",
                    "  arrangement {\n",
                    "    play \n",
                    "  }\n",
                    "}\n",
                ),
                4,
                9,
            ),
            ["intro", "verse"]
        );

        // `play track <name>` inside parallel.
        assert_eq!(
            labels(
                concat!(
                    "song \"Test\" {\n",
                    "  track pad role harmony { instrument lead play melody }\n",
                    "  track bass role low { instrument sub play bassline }\n",
                    "  section intro bars 2 {\n",
                    "    parallel {\n",
                    "      play track \n",
                    "    }\n",
                    "  }\n",
                    "}\n",
                ),
                5,
                16,
            ),
            ["pad", "bass"]
        );
    }

    #[test]
    fn completes_effect_presets_and_arpeggiate_sources() {
        let labels = |source: &str, line, character| {
            completions(
                &SourceText::new(SourceId(0), "test.sym", source),
                Position::new(line, character),
            )
            .into_iter()
            .map(|item| item.label)
            .collect::<Vec<_>>()
        };

        // Track-body `effect` offers kinds plus declared preset names.
        assert_eq!(
            labels(
                concat!(
                    "song \"Test\" {\n",
                    "  effect hall = reverb { mix 0.5 size 0.7 }\n",
                    "  effect room = reverb { mix 0.3 size 0.5 }\n",
                    "  track lead role harmony {\n",
                    "    effect \n",
                    "  }\n",
                    "}\n",
                ),
                4,
                11,
            ),
            ["delay", "filter", "reverb", "hall", "room"]
        );

        // Section override `effect` also offers presets (kinds still apply).
        assert_eq!(
            labels(
                concat!(
                    "song \"Test\" {\n",
                    "  effect hall = reverb { mix 0.5 size 0.7 }\n",
                    "  track pad role harmony { instrument lead play melody }\n",
                    "  section intro bars 2 {\n",
                    "    parallel {\n",
                    "      play track pad { effect \n",
                    "    }\n",
                    "  }\n",
                    "}\n",
                ),
                5,
                30,
            ),
            ["delay", "filter", "reverb", "hall"]
        );

        // Partial preset name still resolves the effect context.
        assert_eq!(
            labels(
                concat!(
                    "song \"Test\" {\n",
                    "  effect hall = reverb { mix 0.5 size 0.7 }\n",
                    "  track lead role harmony {\n",
                    "    effect ha\n",
                    "  }\n",
                    "}\n",
                ),
                3,
                13,
            ),
            ["hall"]
        );

        // Song-level `effect` is a declaration site: no preset name offers.
        assert_eq!(
            labels(
                concat!(
                    "song \"Test\" {\n",
                    "  effect hall = reverb { mix 0.5 size 0.7 }\n",
                    "  effect \n",
                    "}\n",
                ),
                2,
                9,
            ),
            ["delay", "filter", "reverb"]
        );

        // `arpeggiate <source>` offers pattern names.
        assert_eq!(
            labels(
                concat!(
                    "song \"Test\" {\n",
                    "  pattern pad = sequence {}\n",
                    "  pattern drop = sequence {}\n",
                    "  pattern arp = arpeggiate \n",
                    "}\n",
                ),
                3,
                27,
            ),
            // Incomplete `pattern arp = arpeggiate` is not yet a declaration,
            // so only the finished sources appear.
            ["pad", "drop"]
        );

        // `pattern x = ` offers body keywords and derivation sources.
        assert_eq!(
            labels(
                concat!(
                    "song \"Test\" {\n",
                    "  pattern pad = sequence {}\n",
                    "  pattern high = \n",
                    "}\n",
                ),
                2,
                17,
            ),
            // Same: unfinished `high` is absent until its body parses.
            ["sequence", "steps", "arpeggiate", "pad"]
        );
    }

    #[test]
    fn completes_arpeggiate_style_and_body_keywords() {
        let labels = |source: &str, line, character| {
            completions(
                &SourceText::new(SourceId(0), "test.sym", source),
                Position::new(line, character),
            )
            .into_iter()
            .map(|item| item.label)
            .collect::<Vec<_>>()
        };

        // Empty line inside `arpeggiate { … }` offers field keywords.
        assert_eq!(
            labels(
                concat!(
                    "song \"Test\" {\n",
                    "  pattern pad = sequence {}\n",
                    "  pattern arp = arpeggiate pad {\n",
                    "    \n",
                    "  }\n",
                    "}\n",
                ),
                3,
                4,
            ),
            ["style", "step", "octaves"]
        );

        // After `style`, offer the five walk orders the compiler accepts.
        assert_eq!(
            labels(
                concat!(
                    "song \"Test\" {\n",
                    "  pattern pad = sequence {}\n",
                    "  pattern arp = arpeggiate pad {\n",
                    "    style \n",
                    "  }\n",
                    "}\n",
                ),
                3,
                10,
            ),
            ["up", "down", "up_down", "down_up", "as_written"]
        );

        // Partial field keyword still starts the arpeggiate body set.
        assert_eq!(
            labels(
                concat!(
                    "song \"Test\" {\n",
                    "  pattern pad = sequence {}\n",
                    "  pattern arp = arpeggiate pad {\n",
                    "    st\n",
                    "  }\n",
                    "}\n",
                ),
                3,
                6,
            ),
            ["style", "step", "octaves"]
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
    fn builds_inlay_hints_for_midi_pitches_and_reference_kinds() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "project { seed 1 sample_rate 48khz output stereo }\n",
                "song \"Test\" {\n",
                "  tempo 120bpm\n",
                "  meter 4/4\n",
                "  key C major\n",
                "  pattern melody = sequence { note C4 for 1/4 }\n",
                "  arrangement { melody }\n",
                "}\n",
            ),
        );
        let whole = Range::new(Position::new(0, 0), Position::new(20, 0));
        let hints = inlay_hints(&source, &whole);

        let labels: Vec<(u32, u32, String)> = hints
            .iter()
            .map(|hint| {
                let label = match &hint.label {
                    InlayHintLabel::String(text) => text.clone(),
                    InlayHintLabel::LabelParts(parts) => parts
                        .iter()
                        .map(|part| part.value.as_str())
                        .collect::<String>(),
                };
                (hint.position.line, hint.position.character, label)
            })
            .collect();

        assert!(
            labels
                .iter()
                .any(|(line, _, label)| *line == 5 && label == "MIDI 60"),
            "C4 should show MIDI 60: {labels:?}"
        );
        assert!(
            labels
                .iter()
                .any(|(line, _, label)| *line == 6 && label == "pattern"),
            "arrangement melody use should show pattern: {labels:?}"
        );

        // A narrow range around the arrangement line should exclude the MIDI hint.
        let arrangement_only = Range::new(Position::new(6, 0), Position::new(7, 0));
        let filtered = inlay_hints(&source, &arrangement_only);
        assert!(
            filtered.iter().all(|hint| match &hint.label {
                InlayHintLabel::String(text) => text != "MIDI 60",
                InlayHintLabel::LabelParts(_) => true,
            }),
            "range-filtered hints should omit MIDI 60: {filtered:?}"
        );
        assert!(
            filtered.iter().any(|hint| match &hint.label {
                InlayHintLabel::String(text) => text == "pattern",
                InlayHintLabel::LabelParts(_) => false,
            }),
            "range-filtered hints should keep pattern: {filtered:?}"
        );
    }

    #[test]
    fn builds_inlay_hints_for_chord_symbols_and_sugar_references() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "project { seed 1 sample_rate 48khz output stereo }\n",
                "song \"Test\" {\n",
                "  tempo 120bpm\n",
                "  meter 4/4\n",
                "  key C major\n",
                "  instrument lead = triangle\n",
                "  effect hall = reverb { mix 0.5 size 0.7 }\n",
                "  pattern pad = sequence step 1bar { chord G3:maj7 }\n",
                "  pattern high = pad |> transpose 12 st\n",
                "  pattern arp = arpeggiate pad { style up step 1/8 }\n",
                "  track t role harmony {\n",
                "    instrument lead\n",
                "    play high\n",
                "    effect hall\n",
                "  }\n",
                "}\n",
            ),
        );
        let whole = Range::new(Position::new(0, 0), Position::new(30, 0));
        let labels: Vec<(u32, String)> = inlay_hints(&source, &whole)
            .into_iter()
            .map(|hint| {
                let label = match hint.label {
                    InlayHintLabel::String(text) => text,
                    InlayHintLabel::LabelParts(parts) => {
                        parts.into_iter().map(|part| part.value).collect::<String>()
                    }
                };
                (hint.position.line, label)
            })
            .collect();

        // G3 root + maj7 voicing G3 B3 D4 F#4 → MIDI 55 59 62 66
        assert!(
            labels
                .iter()
                .any(|(line, label)| *line == 7 && label == "MIDI 55"),
            "chord root G3: {labels:?}"
        );
        assert!(
            labels
                .iter()
                .any(|(line, label)| *line == 7 && label == "MIDI 55 59 62 66"),
            "maj7 voicing after quality: {labels:?}"
        );
        assert!(
            labels
                .iter()
                .any(|(line, label)| *line == 8 && label == "pattern"),
            "derived source pad: {labels:?}"
        );
        assert!(
            labels
                .iter()
                .any(|(line, label)| *line == 9 && label == "pattern"),
            "arpeggiate source pad: {labels:?}"
        );
        assert!(
            labels
                .iter()
                .any(|(line, label)| *line == 13 && label == "effect"),
            "effect preset use: {labels:?}"
        );
    }

    #[test]
    fn finds_pattern_and_effect_definitions_from_sugar() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "song \"Test\" {\n",
                "  instrument lead = triangle\n",
                "  effect hall = reverb { mix 0.5 size 0.7 }\n",
                "  pattern pad = sequence { chord C4 E4 G4 for 1bar }\n",
                "  pattern high = pad |> transpose 12 st\n",
                "  pattern arp = arpeggiate pad { style up step 1/8 }\n",
                "  track t role harmony {\n",
                "    instrument lead\n",
                "    play high\n",
                "    effect hall\n",
                "  }\n",
                "}\n",
            ),
        );
        let uri = "file:///test.sym".parse::<Uri>().expect("URI should parse");

        // `pad` in `pattern high = pad |> …` — col 17 is the `p` of `pad`.
        let derived = definition(&source, &uri, Position::new(4, 17))
            .expect("derived pattern source should resolve");
        assert_eq!(
            derived.range,
            Range::new(Position::new(3, 10), Position::new(3, 13))
        );

        // `pad` in `arpeggiate pad` — col 27 is the `p` of `pad`.
        let arp = definition(&source, &uri, Position::new(5, 27))
            .expect("arpeggiate source should resolve");
        assert_eq!(
            arp.range,
            Range::new(Position::new(3, 10), Position::new(3, 13))
        );

        // `hall` in `effect hall` on the track — col 11 is the `h` of `hall`.
        let effect = definition(&source, &uri, Position::new(9, 11))
            .expect("effect preset use should resolve");
        assert_eq!(
            effect.range,
            Range::new(Position::new(2, 9), Position::new(2, 13))
        );
    }

    #[test]
    fn highlights_sugar_keywords_as_semantic_tokens() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "song \"Test\" {\n",
                "  pattern pad = sequence step 1bar { rest for 1bar }\n",
                "  pattern arp = arpeggiate pad { style up step 1/8 octaves 2 }\n",
                "  track t role harmony {\n",
                "    play arp |> repeat fit\n",
                "  }\n",
                "}\n",
            ),
        );
        let decoded = decode_semantic_tokens(&semantic_tokens(&source).data);

        // Keywords that arrived with RFC 0001 must classify as keywords, not
        // fall through as bare identifiers. Columns are absolute (not delta).
        let keyword_on = |line: u32, col: u32, len: u32| {
            decoded.iter().any(|token| {
                token.0 == line
                    && token.1 == col
                    && token.2 == len
                    && token.3 == SEMANTIC_TOKEN_KEYWORD
            })
        };
        assert!(keyword_on(1, 25, 4), "step: {decoded:?}");
        assert!(keyword_on(2, 16, 10), "arpeggiate: {decoded:?}");
        assert!(keyword_on(2, 33, 5), "style: {decoded:?}");
        assert!(keyword_on(2, 51, 7), "octaves: {decoded:?}");
        assert!(keyword_on(4, 23, 3), "fit: {decoded:?}");

        // `pad` source of arpeggiate is a pattern reference (function, no declaration).
        assert!(
            decoded.iter().any(|token| {
                token.0 == 2
                    && token.1 == 27
                    && token.2 == 3
                    && token.3 == SEMANTIC_TOKEN_FUNCTION
                    && token.4 == 0
            }),
            "arpeggiate source pad: {decoded:?}"
        );
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

    fn decode_semantic_tokens(data: &[SemanticToken]) -> Vec<(u32, u32, u32, u32, u32)> {
        let mut line = 0;
        let mut start = 0;
        data.iter()
            .map(|token| {
                line += token.delta_line;
                start = if token.delta_line == 0 {
                    start + token.delta_start
                } else {
                    token.delta_start
                };
                (
                    line,
                    start,
                    token.length,
                    token.token_type,
                    token.token_modifiers_bitset,
                )
            })
            .collect()
    }

    #[test]
    fn builds_semantic_tokens_for_keywords_names_literals_and_pitches() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "# header\n",
                "song \"Test\" {\n",
                "  pattern melody = sequence { note C4 for 1/4 }\n",
                "  arrangement { melody }\n",
                "}\n",
            ),
        );

        let decoded = decode_semantic_tokens(&semantic_tokens(&source).data);

        assert!(
            decoded
                .iter()
                .any(|token| token.3 == SEMANTIC_TOKEN_COMMENT && token.0 == 0),
            "comment on line 0: {decoded:?}"
        );
        assert!(
            decoded.iter().any(|token| {
                token.3 == SEMANTIC_TOKEN_KEYWORD && token.0 == 1 && token.1 == 0 && token.2 == 4
            }),
            "song keyword: {decoded:?}"
        );
        assert!(
            decoded
                .iter()
                .any(|token| token.3 == SEMANTIC_TOKEN_STRING && token.0 == 1),
            "song title string: {decoded:?}"
        );
        // `melody` declaration: function + declaration modifier.
        assert!(
            decoded.iter().any(|token| {
                token.0 == 2
                    && token.3 == SEMANTIC_TOKEN_FUNCTION
                    && token.4 == SEMANTIC_MOD_DECLARATION
                    && token.2 == 6
            }),
            "melody declaration: {decoded:?}"
        );
        // arrangement use of `melody`: function without declaration.
        assert!(
            decoded.iter().any(|token| {
                token.0 == 3 && token.3 == SEMANTIC_TOKEN_FUNCTION && token.4 == 0 && token.2 == 6
            }),
            "melody reference: {decoded:?}"
        );
        assert!(
            decoded
                .iter()
                .any(|token| token.3 == SEMANTIC_TOKEN_TYPE && token.2 == 2),
            "pitch C4 as type: {decoded:?}"
        );
        assert!(
            decoded
                .iter()
                .any(|token| token.3 == SEMANTIC_TOKEN_NUMBER && token.2 == 1),
            "duration numerator: {decoded:?}"
        );
    }

    #[test]
    fn highlights_named_symbol_reads_and_writes() {
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

        // Declaration site in Second: write + one read.
        let from_decl = document_highlights(&source, Position::new(4, 10));
        assert_eq!(from_decl.len(), 2);
        assert_eq!(from_decl[0].kind, Some(DocumentHighlightKind::WRITE));
        assert_eq!(
            from_decl[0].range,
            Range::new(Position::new(4, 10), Position::new(4, 16))
        );
        assert_eq!(from_decl[1].kind, Some(DocumentHighlightKind::READ));
        assert_eq!(
            from_decl[1].range,
            Range::new(Position::new(5, 16), Position::new(5, 22))
        );

        // Use site yields the same set.
        let from_use = document_highlights(&source, Position::new(5, 17));
        assert_eq!(from_use, from_decl);

        // First.melody has only its declaration write.
        let other = document_highlights(&source, Position::new(1, 10));
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].kind, Some(DocumentHighlightKind::WRITE));
        assert!(document_highlights(&source, Position::new(0, 1)).is_empty());
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

        let lenses = code_lenses(&source, &uri, &PreviewTrackState::default());
        let melody = lenses
            .iter()
            .find(|lens| lens.range == Range::new(Position::new(1, 10), Position::new(1, 16)))
            .expect("melody should have a code lens");
        let unused = lenses
            .iter()
            .find(|lens| lens.range == Range::new(Position::new(2, 10), Position::new(2, 16)))
            .expect("unused should have a code lens");

        assert_eq!(
            melody
                .command
                .as_ref()
                .map(|command| command.title.as_str()),
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
    fn builds_loop_code_lenses_for_sections() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "song \"Test\" {\n",
                "  section intro bars 2 { parallel { play track pad } }\n",
                "  arrangement { play intro }\n",
                "}\n",
            ),
        );
        let uri = "file:///test.sym".parse::<Uri>().expect("URI should parse");

        let lens = code_lenses(&source, &uri, &PreviewTrackState::default())
            .into_iter()
            .find(|lens| {
                lens.command
                    .as_ref()
                    .is_some_and(|command| command.command == "symphra.loopSection")
            })
            .expect("section should have a loop code lens");

        assert_eq!(
            lens.command,
            Some(Command {
                title: "▶ Loop section".to_owned(),
                command: "symphra.loopSection".to_owned(),
                arguments: Some(vec![
                    serde_json::Value::String("file:///test.sym".to_owned()),
                    serde_json::Value::String("intro".to_owned()),
                ]),
            })
        );
    }

    #[test]
    fn builds_from_here_code_lenses_for_section_arrangement_entries() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "song \"Test\" {\n",
                "  section intro bars 2 { parallel { play track pad } }\n",
                "  arrangement { play intro play intro }\n",
                "}\n",
            ),
        );
        let uri = "file:///test.sym".parse::<Uri>().expect("URI should parse");

        let lenses = code_lenses(&source, &uri, &PreviewTrackState::default())
            .into_iter()
            .filter_map(|lens| lens.command)
            .filter(|command| command.command == "symphra.playFromHere")
            .collect::<Vec<_>>();

        assert_eq!(lenses.len(), 2);
        assert_eq!(lenses[1].title, "▶ From here");
        assert_eq!(
            lenses[1].arguments,
            Some(vec![
                serde_json::Value::String("file:///test.sym".to_owned()),
                serde_json::Value::from(1),
            ])
        );
    }

    #[test]
    fn builds_stateful_mute_and_solo_code_lenses_for_tracks() {
        let source = SourceText::new(
            SourceId(0),
            "test.sym",
            concat!(
                "project { seed 1 sample_rate 8khz output mono }\n",
                "song \"Test\" {\n",
                "  tempo 120bpm meter 4/4 key C major\n",
                "  instrument tone = sine\n",
                "  pattern notes = sequence { note C4 for 1/4 }\n",
                "  track bass role bass { instrument tone play notes }\n",
                "}\n",
            ),
        );
        let uri = "file:///test.sym".parse::<Uri>().expect("URI should parse");
        let state = PreviewTrackState {
            muted: HashSet::from(["bass".to_owned()]),
            soloed: HashSet::new(),
        };

        let titles = code_lenses(&source, &uri, &state)
            .into_iter()
            .filter_map(|lens| lens.command)
            .filter(|command| command.command.starts_with("symphra.toggle"))
            .map(|command| command.title)
            .collect::<Vec<_>>();

        assert_eq!(titles, vec!["Unmute", "Solo"]);
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
        let PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } = prepared else {
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
        assert!(invalid.message.contains("not a valid Symphra identifier"));

        let keyword = rename(&source, &uri, Position::new(1, 10), "pattern")
            .expect_err("keywords should error");
        assert!(keyword.message.contains("not a valid Symphra identifier"));

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
