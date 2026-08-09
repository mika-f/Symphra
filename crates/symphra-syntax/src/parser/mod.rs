mod literal;

use crate::ast::{
    Declaration, Identifier, NoteExpression, NumberLiteral, PatternBody, PatternDeclaration,
    ProjectDeclaration, ProjectStatement, QuotedString, RateLiteral, RestExpression, SequenceItem,
    SongDeclaration, SongStatement, SourceFile,
};
use crate::{Diagnostic, SourceId, SourceSpan, Token, TokenKind, lex};

const DECLARATION_START: &[TokenKind] = &[TokenKind::Project, TokenKind::Song, TokenKind::Eof];
const PROJECT_STATEMENT_START: &[TokenKind] = &[
    TokenKind::Seed,
    TokenKind::SampleRate,
    TokenKind::Output,
    TokenKind::RightBrace,
    TokenKind::Eof,
];
const SONG_STATEMENT_START: &[TokenKind] = &[
    TokenKind::Tempo,
    TokenKind::Meter,
    TokenKind::Key,
    TokenKind::Pattern,
    TokenKind::Arrangement,
    TokenKind::RightBrace,
    TokenKind::Eof,
];
const SEQUENCE_ITEM_START: &[TokenKind] = &[
    TokenKind::Note,
    TokenKind::Rest,
    TokenKind::RightBrace,
    TokenKind::Eof,
];

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedSource {
    pub file: SourceFile,
    pub diagnostics: Vec<Diagnostic>,
}

#[must_use]
pub fn parse(source: SourceId, input: &str) -> ParsedSource {
    let lexed = lex(source, input);
    let mut parser = Parser {
        tokens: lexed.tokens,
        cursor: 0,
        diagnostics: lexed.diagnostics,
    };
    let declarations = parser.source_file();
    ParsedSource {
        file: SourceFile {
            source,
            declarations,
            span: SourceSpan::new(source, 0..input.len()),
        },
        diagnostics: parser.diagnostics,
    }
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    fn source_file(&mut self) -> Vec<Declaration> {
        let mut declarations = Vec::new();
        while !self.at(TokenKind::Eof) {
            let declaration = match self.current().kind {
                TokenKind::Project => self.project().map(Declaration::Project),
                TokenKind::Song => self.song().map(Declaration::Song),
                _ => {
                    self.error("expected `project` or `song`");
                    None
                }
            };
            if let Some(declaration) = declaration {
                declarations.push(declaration);
            } else {
                self.recover_to(DECLARATION_START);
            }
        }
        declarations
    }

    fn project(&mut self) -> Option<ProjectDeclaration> {
        let start = self.bump().span;
        self.required(TokenKind::LeftBrace, "expected `{` after `project`")?;
        let mut statements = Vec::new();
        while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
            let statement = match self.current().kind {
                TokenKind::Seed => self.seed(),
                TokenKind::SampleRate => self.sample_rate(),
                TokenKind::Output => self.output(),
                _ => {
                    self.error("expected a project setting");
                    None
                }
            };
            if let Some(statement) = statement {
                statements.push(statement);
            } else {
                self.recover_to(PROJECT_STATEMENT_START);
            }
        }
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close project")?
            .span;
        Some(ProjectDeclaration {
            statements,
            span: start.cover(end),
        })
    }

    fn seed(&mut self) -> Option<ProjectStatement> {
        let start = self.bump().span;
        let token = self.required(TokenKind::Integer, "expected an integer seed")?;
        let value = self.parse_u64(&token)?;
        Some(ProjectStatement::Seed {
            value,
            span: start.cover(token.span),
        })
    }

    fn sample_rate(&mut self) -> Option<ProjectStatement> {
        let start = self.bump().span;
        let value = self.rate()?;
        let span = start.cover(value.span);
        Some(ProjectStatement::SampleRate { value, span })
    }

    fn output(&mut self) -> Option<ProjectStatement> {
        let start = self.bump().span;
        let channels = self.identifier("expected an output layout")?;
        let span = start.cover(channels.span);
        Some(ProjectStatement::Output { channels, span })
    }

    fn song(&mut self) -> Option<SongDeclaration> {
        let start = self.bump().span;
        let name = self.string("expected a quoted song name")?;
        self.required(TokenKind::LeftBrace, "expected `{` after song name")?;
        let mut statements = Vec::new();
        while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
            let statement = match self.current().kind {
                TokenKind::Tempo => self.tempo(),
                TokenKind::Meter => self.meter(),
                TokenKind::Key => self.key(),
                TokenKind::Arrangement => self.arrangement(),
                TokenKind::Pattern => self.pattern().map(SongStatement::Pattern),
                _ => {
                    self.error("expected a song setting or pattern");
                    None
                }
            };
            if let Some(statement) = statement {
                statements.push(statement);
            } else {
                self.recover_to(SONG_STATEMENT_START);
            }
        }
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close song")?
            .span;
        Some(SongDeclaration {
            name,
            statements,
            span: start.cover(end),
        })
    }

    fn tempo(&mut self) -> Option<SongStatement> {
        let start = self.bump().span;
        let value = self.rate()?;
        let span = start.cover(value.span);
        Some(SongStatement::Tempo { value, span })
    }

    fn meter(&mut self) -> Option<SongStatement> {
        let start = self.bump().span;
        let numerator_token = self.required(TokenKind::Integer, "expected meter numerator")?;
        self.required(TokenKind::Slash, "expected `/` in meter")?;
        let denominator_token = self.required(TokenKind::Integer, "expected meter denominator")?;
        let numerator = self.parse_u32(&numerator_token)?;
        let denominator = self.parse_u32(&denominator_token)?;
        Some(SongStatement::Meter {
            numerator,
            denominator,
            span: start.cover(denominator_token.span),
        })
    }

    fn key(&mut self) -> Option<SongStatement> {
        let start = self.bump().span;
        let tonic = self.identifier("expected key tonic")?;
        let mode = self.identifier("expected key mode")?;
        let span = start.cover(mode.span);
        Some(SongStatement::Key { tonic, mode, span })
    }

    fn arrangement(&mut self) -> Option<SongStatement> {
        let start = self.bump().span;
        self.required(TokenKind::LeftBrace, "expected `{` after `arrangement`")?;
        let mut patterns = Vec::new();
        while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
            if self.at(TokenKind::Identifier) {
                patterns.push(self.identifier("expected a pattern name in arrangement")?);
            } else {
                self.error("expected a pattern name in arrangement");
                while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
                    self.bump();
                }
            }
        }
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close arrangement")?
            .span;
        Some(SongStatement::Arrangement {
            patterns,
            span: start.cover(end),
        })
    }

    fn pattern(&mut self) -> Option<PatternDeclaration> {
        let start = self.bump().span;
        let name = self.identifier("expected pattern name")?;
        self.required(TokenKind::Equal, "expected `=` after pattern name")?;
        self.required(TokenKind::Sequence, "expected `sequence` pattern body")?;
        let body_start = self
            .required(TokenKind::LeftBrace, "expected `{` after `sequence`")?
            .span;
        let mut items = Vec::new();
        while !self.at_any(&[TokenKind::RightBrace, TokenKind::Eof]) {
            let item = match self.current().kind {
                TokenKind::Note => self.note().map(SequenceItem::Note),
                TokenKind::Rest => self.rest().map(SequenceItem::Rest),
                _ => {
                    self.error("expected a note or rest in sequence");
                    None
                }
            };
            if let Some(item) = item {
                items.push(item);
            } else {
                self.recover_to(SEQUENCE_ITEM_START);
            }
        }
        let end = self
            .required(TokenKind::RightBrace, "expected `}` to close sequence")?
            .span;
        Some(PatternDeclaration {
            name,
            body: PatternBody::Sequence {
                items,
                span: body_start.cover(end),
            },
            span: start.cover(end),
        })
    }

    fn note(&mut self) -> Option<NoteExpression> {
        let start = self.bump().span;
        let pitch = self.identifier("expected note pitch")?;
        self.required(TokenKind::For, "expected `for` after note pitch")?;
        let numerator_token = self.required(TokenKind::Integer, "expected duration numerator")?;
        self.required(TokenKind::Slash, "expected `/` in note duration")?;
        let denominator_token =
            self.required(TokenKind::Integer, "expected duration denominator")?;
        Some(NoteExpression {
            pitch,
            duration_numerator: self.parse_u32(&numerator_token)?,
            duration_denominator: self.parse_u32(&denominator_token)?,
            span: start.cover(denominator_token.span),
        })
    }

    fn rest(&mut self) -> Option<RestExpression> {
        let start = self.bump().span;
        self.required(TokenKind::For, "expected `for` after `rest`")?;
        let numerator_token = self.required(TokenKind::Integer, "expected duration numerator")?;
        self.required(TokenKind::Slash, "expected `/` in rest duration")?;
        let denominator_token =
            self.required(TokenKind::Integer, "expected duration denominator")?;
        Some(RestExpression {
            duration_numerator: self.parse_u32(&numerator_token)?,
            duration_denominator: self.parse_u32(&denominator_token)?,
            span: start.cover(denominator_token.span),
        })
    }

    fn rate(&mut self) -> Option<RateLiteral> {
        let number = self.required_any(
            &[TokenKind::Integer, TokenKind::Decimal],
            "expected a number",
        )?;
        let unit = self.identifier("expected a unit immediately after the number")?;
        let Ok(value) = number.text.parse::<f64>() else {
            self.diagnostics
                .push(Diagnostic::syntax("number is out of range", number.span));
            return None;
        };
        let span = number.span.cover(unit.span);
        Some(RateLiteral {
            value: NumberLiteral {
                value,
                span: number.span,
            },
            unit,
            span,
        })
    }

    fn identifier(&mut self, message: &str) -> Option<Identifier> {
        let token = self.required(TokenKind::Identifier, message)?;
        Some(Identifier {
            text: token.text,
            span: token.span,
        })
    }

    fn string(&mut self, message: &str) -> Option<QuotedString> {
        let token = self.required(TokenKind::String, message)?;
        let inner = token
            .text
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or("");
        let value = inner
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
            .replace("\\n", "\n");
        Some(QuotedString {
            value,
            span: token.span,
        })
    }

    fn parse_u64(&mut self, token: &Token) -> Option<u64> {
        token
            .text
            .parse()
            .map_err(|_| {
                self.diagnostics
                    .push(Diagnostic::syntax("integer is out of range", token.span));
            })
            .ok()
    }

    fn parse_u32(&mut self, token: &Token) -> Option<u32> {
        token
            .text
            .parse()
            .map_err(|_| {
                self.diagnostics
                    .push(Diagnostic::syntax("integer is out of range", token.span));
            })
            .ok()
    }

    fn required(&mut self, kind: TokenKind, message: &str) -> Option<Token> {
        self.required_any(&[kind], message)
    }

    fn required_any(&mut self, kinds: &[TokenKind], message: &str) -> Option<Token> {
        if self.at_any(kinds) {
            Some(self.bump())
        } else {
            self.error(message);
            None
        }
    }

    fn error(&mut self, message: &str) {
        self.diagnostics
            .push(Diagnostic::syntax(message, self.current().span));
    }

    fn recover_to(&mut self, kinds: &[TokenKind]) {
        while !self.at_any(kinds) && !self.at(TokenKind::Eof) {
            self.bump();
        }
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.current().kind == kind
    }

    fn at_any(&self, kinds: &[TokenKind]) -> bool {
        kinds.contains(&self.current().kind)
    }

    fn current(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn bump(&mut self) -> Token {
        let token = self.current().clone();
        if token.kind != TokenKind::Eof {
            self.cursor += 1;
        }
        token
    }
}
