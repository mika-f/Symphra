use crate::{Diagnostic, SourceId, SourceSpan, Token, TokenKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lexed {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Lexes one Symphra source buffer.
///
/// # Panics
///
/// Panics if `input` is larger than 4 GiB, because source spans use 32-bit
/// byte offsets.
#[must_use]
pub fn lex(source: SourceId, input: &str) -> Lexed {
    let mut lexer = Lexer {
        source,
        input,
        offset: 0,
        tokens: Vec::new(),
        diagnostics: Vec::new(),
    };
    while lexer.offset < input.len() {
        lexer.next();
    }
    lexer.tokens.push(Token {
        kind: TokenKind::Eof,
        text: String::new(),
        span: SourceSpan::empty(
            source,
            u32::try_from(input.len()).expect("source exceeds 4 GiB"),
        ),
    });
    Lexed {
        tokens: lexer.tokens,
        diagnostics: lexer.diagnostics,
    }
}

struct Lexer<'a> {
    source: SourceId,
    input: &'a str,
    offset: usize,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl Lexer<'_> {
    fn next(&mut self) {
        let start = self.offset;
        let ch = self.bump().expect("called only before EOF");
        match ch {
            c if c.is_whitespace() => self.skip_while(char::is_whitespace),
            '#' => self.skip_while(|c| c != '\n'),
            '/' if self.peek() == Some('/') => {
                self.bump();
                self.skip_while(|c| c != '\n');
            }
            '{' => self.push(TokenKind::LeftBrace, start),
            '}' => self.push(TokenKind::RightBrace, start),
            '=' => self.push(TokenKind::Equal, start),
            '/' => self.push(TokenKind::Slash, start),
            '|' if self.peek() == Some('>') => {
                self.bump();
                self.push(TokenKind::PipeGreater, start);
            }
            '"' => self.string(start),
            c if c.is_ascii_digit() => self.number(start),
            c if is_identifier_start(c) => {
                self.skip_while(is_identifier_continue);
                self.pitch_suffix(start);
                let text = &self.input[start..self.offset];
                self.push(
                    TokenKind::keyword(text).unwrap_or(TokenKind::Identifier),
                    start,
                );
            }
            _ => self.diagnostics.push(Diagnostic::lexical(
                format!("unexpected character `{ch}`"),
                SourceSpan::new(self.source, start..self.offset),
            )),
        }
    }

    fn string(&mut self, start: usize) {
        let mut escaped = false;
        while let Some(ch) = self.peek() {
            self.bump();
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                self.push(TokenKind::String, start);
                return;
            }
        }
        self.diagnostics.push(Diagnostic::lexical(
            "unterminated string literal",
            SourceSpan::new(self.source, start..self.offset),
        ));
        self.push(TokenKind::String, start);
    }

    fn pitch_suffix(&mut self, start: usize) {
        if !is_pitch_prefix(&self.input[start..self.offset]) {
            return;
        }
        if self.peek() == Some('#') && octave_follows(&self.input[self.offset + 1..]) {
            self.bump();
        }
        if self.peek() == Some('-') && octave_follows(&self.input[self.offset..]) {
            self.bump();
        }
        self.skip_while(|c| c.is_ascii_digit());
    }

    fn number(&mut self, start: usize) {
        self.skip_while(|c| c.is_ascii_digit());
        let kind = if self.peek() == Some('.')
            && self.input[self.offset + 1..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        {
            self.bump();
            self.skip_while(|c| c.is_ascii_digit());
            TokenKind::Decimal
        } else {
            TokenKind::Integer
        };
        self.push(kind, start);
    }

    fn push(&mut self, kind: TokenKind, start: usize) {
        self.tokens.push(Token {
            kind,
            text: self.input[start..self.offset].to_owned(),
            span: SourceSpan::new(self.source, start..self.offset),
        });
    }

    fn peek(&self) -> Option<char> {
        self.input[self.offset..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.offset += ch.len_utf8();
        Some(ch)
    }

    fn skip_while(&mut self, predicate: impl Fn(char) -> bool) {
        while self.peek().is_some_and(&predicate) {
            self.bump();
        }
    }
}

fn is_identifier_start(ch: char) -> bool {
    ch == '_' || ch.is_alphabetic()
}

fn is_identifier_continue(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

fn is_pitch_prefix(text: &str) -> bool {
    matches!(text.as_bytes(), [b'A'..=b'G'] | [b'A'..=b'G', b'b'])
}

fn octave_follows(text: &str) -> bool {
    text.strip_prefix('-')
        .unwrap_or(text)
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit())
}
