use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use symphra_compiler::compile;
use symphra_syntax::ast::{Declaration, SongStatement};
use symphra_syntax::{SourceId, SourceSpan, SourceText, parse};
use tokio::sync::RwLock;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse,
    InitializeParams, InitializeResult, Location, OneOf, Position, PositionEncodingKind, Range,
    ServerCapabilities, ServerInfo, SymbolInformation, SymbolKind, TextDocumentSyncCapability,
    TextDocumentSyncKind, Uri,
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
                    .filter_map(|statement| {
                        let SongStatement::Pattern(pattern) = statement else {
                            return None;
                        };
                        symbol(
                            source,
                            pattern.name.text.clone(),
                            SymbolKind::FUNCTION,
                            pattern.span,
                            pattern.name.span,
                            None,
                        )
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
    use super::{SourceId, SourceText, diagnostics, document_symbols, flatten_document_symbols};
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
}
