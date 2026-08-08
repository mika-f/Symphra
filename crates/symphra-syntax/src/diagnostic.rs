use crate::SourceSpan;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticKind {
    Lexical,
    Syntax,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub message: String,
    pub span: SourceSpan,
}

impl Diagnostic {
    pub(crate) fn lexical(message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            kind: DiagnosticKind::Lexical,
            message: message.into(),
            span,
        }
    }

    pub(crate) fn syntax(message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            kind: DiagnosticKind::Syntax,
            message: message.into(),
            span,
        }
    }
}
