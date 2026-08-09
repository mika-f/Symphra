use symphra_compiler::compile;
use symphra_syntax::{SourceId, SourceSpan, SourceText, parse};
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, InitializeParams, InitializeResult, Position, PositionEncodingKind,
    Range, ServerCapabilities, ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
};
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

#[derive(Debug)]
struct Backend {
    client: Client,
}

impl Backend {
    async fn publish(&self, uri: Uri, version: Option<i32>, text: String) {
        let source = SourceText::new(SourceId(0), uri.as_str(), text);
        self.client
            .publish_diagnostics(uri, diagnostics(&source), version)
            .await;
    }
}

impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                position_encoding: Some(PositionEncodingKind::UTF16),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
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
        self.publish(document.uri, Some(document.version), document.text)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let document = params.text_document;
        if let Some(change) = params.content_changes.into_iter().last() {
            self.publish(document.uri, Some(document.version), change.text)
                .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.client
            .publish_diagnostics(params.text_document.uri, Vec::new(), None)
            .await;
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
    let range = source.utf16_range(span)?;
    Some(Diagnostic {
        range: Range {
            start: Position::new(range.start.line, range.start.utf16_column),
            end: Position::new(range.end.line, range.end.utf16_column),
        },
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("symphra".to_owned()),
        message,
        ..Diagnostic::default()
    })
}

#[tokio::main]
async fn main() {
    let (service, socket) = LspService::new(|client| Backend { client });
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;
}

#[cfg(test)]
mod tests {
    use super::{SourceId, SourceText, diagnostics};
    use tower_lsp_server::ls_types::{DiagnosticSeverity, Position, Range};

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
}
