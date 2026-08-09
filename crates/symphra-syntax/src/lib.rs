//! Lexing and parsing for the Symphra source language.
//!
//! This crate deliberately performs no name resolution, type checking, or
//! musical validation. Those responsibilities belong to `symphra-compiler`.

pub mod ast;
mod diagnostic;
mod lexer;
mod parser;
mod source;
mod span;
mod token;

pub use diagnostic::{Diagnostic, DiagnosticKind};
pub use lexer::{Comment, Lexed, lex};
pub use parser::{ParsedSource, parse};
pub use source::{SourcePosition, SourceRange, SourceText};
pub use span::{SourceId, SourceSpan};
pub use token::{Token, TokenKind};
