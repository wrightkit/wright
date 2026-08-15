//! The native OSTW lexer.
//!
//! Produces a flat token stream from one source file. OSTW is a
//! brace/semicolon language (not indentation-based), so newlines are
//! insignificant and skipped with other whitespace. Comments (`#`, `//`,
//! `/* */`) are skipped; string literals preserve their unescaped value and
//! exact span. Positions are 1-based line/column, matching the shared
//! `wright_ir::source` registry.

use wright_ir::source::{FileId, Position, Span};

use crate::diag::{FrontendError, FrontendResult};

/// The kind of a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// An identifier or keyword (keywords are resolved by the parser).
    Ident,
    /// A numeric literal (`text` holds the source spelling).
    Number,
    /// A `"..."` string (`text` holds the unescaped value).
    String,
    /// An `@"..."` verbatim string (`text` holds the unescaped value).
    VerbatimString,
    /// End of file.
    Eof,
    // Punctuation and operators.
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Semi,
    Dot,
    Pipe,
    At,
    Assign,
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,
    PercentAssign,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Power,
    PlusPlus,
    MinusMinus,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Bang,
    Question,
}

/// One token with its source span and payload text.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    /// The source text of this token (numbers keep their spelling; strings
    /// keep their unescaped value; identifiers keep their name).
    pub text: String,
    pub span: Span,
}

impl Token {
    fn new(kind: TokenKind, text: impl Into<String>, span: Span) -> Token {
        Token {
            kind,
            text: text.into(),
            span,
        }
    }
}

/// The lexer input: one file's text with its file id.
pub struct LexInput<'a> {
    pub file_id: FileId,
    pub text: &'a str,
}

/// Lex one source file into a token stream.
pub fn lex(input: LexInput<'_>) -> FrontendResult<Vec<Token>> {
    Lexer::new(input.file_id, input.text).run()
}

struct Lexer {
    file_id: FileId,
    chars: Vec<char>,
    pos: usize,
    line: u32,
    col: u32,
    tokens: Vec<Token>,
}

impl Lexer {
    fn new(file_id: FileId, text: &str) -> Lexer {
        Lexer {
            file_id,
            chars: text.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
            tokens: Vec::new(),
        }
    }

    fn run(mut self) -> FrontendResult<Vec<Token>> {
        while self.pos < self.chars.len() {
            let ch = self.chars[self.pos];
            match ch {
                ' ' | '\t' | '\r' | '\n' => self.advance(),
                '#' => self.line_comment(),
                '/' if self.peek(1) == Some('/') => self.line_comment(),
                '/' if self.peek(1) == Some('*') => self.block_comment()?,
                'a'..='z' | 'A'..='Z' | '_' => self.identifier(),
                '0'..='9' => self.number(),
                '"' => self.string(false)?,
                '@' if self.peek(1) == Some('"') => {
                    self.advance();
                    self.string(true)?;
                }
                '(' => self.punct(TokenKind::LParen, 1),
                ')' => self.punct(TokenKind::RParen, 1),
                '[' => self.punct(TokenKind::LBracket, 1),
                ']' => self.punct(TokenKind::RBracket, 1),
                '{' => self.punct(TokenKind::LBrace, 1),
                '}' => self.punct(TokenKind::RBrace, 1),
                ',' => self.punct(TokenKind::Comma, 1),
                ':' => self.punct(TokenKind::Colon, 1),
                ';' => self.punct(TokenKind::Semi, 1),
                '.' => self.punct(TokenKind::Dot, 1),
                '|' if self.peek(1) == Some('|') => self.punct(TokenKind::Or, 2),
                '|' => self.punct(TokenKind::Pipe, 1),
                '@' => self.punct(TokenKind::At, 1),
                '=' if self.peek(1) == Some('=') => self.punct(TokenKind::Eq, 2),
                '=' => self.punct(TokenKind::Assign, 1),
                '+' if self.peek(1) == Some('=') => self.punct(TokenKind::PlusAssign, 2),
                '+' if self.peek(1) == Some('+') => self.punct(TokenKind::PlusPlus, 2),
                '+' => self.punct(TokenKind::Plus, 1),
                '-' if self.peek(1) == Some('=') => self.punct(TokenKind::MinusAssign, 2),
                '-' if self.peek(1) == Some('-') => self.punct(TokenKind::MinusMinus, 2),
                '-' => self.punct(TokenKind::Minus, 1),
                '*' if self.peek(1) == Some('=') => self.punct(TokenKind::StarAssign, 2),
                '*' => self.punct(TokenKind::Star, 1),
                '/' if self.peek(1) == Some('=') => self.punct(TokenKind::SlashAssign, 2),
                '/' => self.punct(TokenKind::Slash, 1),
                '%' if self.peek(1) == Some('=') => self.punct(TokenKind::PercentAssign, 2),
                '%' => self.punct(TokenKind::Percent, 1),
                '^' => self.punct(TokenKind::Power, 1),
                '!' if self.peek(1) == Some('=') => self.punct(TokenKind::Ne, 2),
                '!' => self.punct(TokenKind::Bang, 1),
                '<' if self.peek(1) == Some('=') => self.punct(TokenKind::Le, 2),
                '<' => self.punct(TokenKind::Lt, 1),
                '>' if self.peek(1) == Some('=') => self.punct(TokenKind::Ge, 2),
                '>' => self.punct(TokenKind::Gt, 1),
                '&' if self.peek(1) == Some('&') => self.punct(TokenKind::And, 2),
                '?' => self.punct(TokenKind::Question, 1),
                other => {
                    return Err(self.error_at(
                        "ostw-lex-error",
                        format!("unexpected character '{other}'"),
                        1,
                    ));
                }
            }
        }
        let here = self.here(0);
        let span = Span::new(self.file_id, here, here);
        self.tokens.push(Token::new(TokenKind::Eof, "", span));
        Ok(self.tokens)
    }

    fn identifier(&mut self) {
        let start = self.here(0);
        let mut text = String::new();
        while self.pos < self.chars.len() {
            let ch = self.chars[self.pos];
            if ch.is_ascii_alphanumeric() || ch == '_' {
                text.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        let end = self.here(0);
        self.tokens.push(Token::new(
            TokenKind::Ident,
            text,
            Span::new(self.file_id, start, end),
        ));
    }

    fn number(&mut self) {
        let start = self.here(0);
        let mut text = String::new();
        while self.pos < self.chars.len() && self.chars[self.pos].is_ascii_digit() {
            text.push(self.chars[self.pos]);
            self.advance();
        }
        if self.pos < self.chars.len()
            && self.chars[self.pos] == '.'
            && self.peek(1).is_some_and(|c| c.is_ascii_digit())
        {
            text.push('.');
            self.advance();
            while self.pos < self.chars.len() && self.chars[self.pos].is_ascii_digit() {
                text.push(self.chars[self.pos]);
                self.advance();
            }
        }
        let end = self.here(0);
        self.tokens.push(Token::new(
            TokenKind::Number,
            text,
            Span::new(self.file_id, start, end),
        ));
    }

    fn string(&mut self, verbatim: bool) -> FrontendResult<()> {
        let start = self.here(0);
        // Consume the opening quote (the `@` was already consumed).
        self.advance();
        let mut value = String::new();
        loop {
            if self.pos >= self.chars.len() {
                return Err(self.error_at(
                    "ostw-lex-error",
                    "unterminated string literal",
                    self.chars.len().saturating_sub(1),
                ));
            }
            let ch = self.chars[self.pos];
            if ch == '"' {
                self.advance();
                break;
            }
            if ch == '\\' {
                // Escapes: advance past the backslash and translate.
                self.advance();
                if self.pos >= self.chars.len() {
                    return Err(self.error_at(
                        "ostw-lex-error",
                        "unterminated string escape",
                        self.chars.len().saturating_sub(1),
                    ));
                }
                let escaped = self.chars[self.pos];
                self.advance();
                match escaped {
                    'n' => value.push('\n'),
                    't' => value.push('\t'),
                    'r' => value.push('\r'),
                    '"' => value.push('"'),
                    '\\' => value.push('\\'),
                    '\'' => value.push('\''),
                    other => {
                        value.push('\\');
                        value.push(other);
                    }
                }
                continue;
            }
            value.push(ch);
            self.advance();
        }
        let end = self.here(0);
        let kind = if verbatim {
            TokenKind::VerbatimString
        } else {
            TokenKind::String
        };
        self.tokens
            .push(Token::new(kind, value, Span::new(self.file_id, start, end)));
        Ok(())
    }

    fn line_comment(&mut self) {
        while self.pos < self.chars.len() && self.chars[self.pos] != '\n' {
            self.advance();
        }
    }

    fn block_comment(&mut self) -> FrontendResult<()> {
        self.advance();
        self.advance();
        loop {
            if self.pos >= self.chars.len() {
                return Err(FrontendError::at(
                    "ostw-lex-error",
                    "unterminated block comment",
                    Span::new(self.file_id, self.here(0), self.here(1)),
                ));
            }
            if self.chars[self.pos] == '*' && self.peek(1) == Some('/') {
                self.advance();
                self.advance();
                return Ok(());
            }
            self.advance();
        }
    }

    fn punct(&mut self, kind: TokenKind, len: usize) {
        let start = self.here(0);
        for _ in 0..len {
            self.advance();
        }
        let end = self.here(0);
        self.tokens
            .push(Token::new(kind, "", Span::new(self.file_id, start, end)));
    }

    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn advance(&mut self) {
        if self.pos >= self.chars.len() {
            return;
        }
        if self.chars[self.pos] == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        self.pos += 1;
    }

    fn here(&self, offset: usize) -> Position {
        Position::new(self.line, self.col + offset as u32)
    }

    fn error_at(
        &self,
        code: &str,
        message: impl Into<String>,
        char_offset: usize,
    ) -> FrontendError {
        let mut line = self.line;
        let mut col = self.col;
        // Walk `char_offset` characters forward for a more accurate position.
        let mut walked = 0;
        let mut index = self.pos;
        while walked < char_offset && index < self.chars.len() {
            if self.chars[index] == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
            index += 1;
            walked += 1;
        }
        FrontendError::at(
            code,
            message,
            Span::new(
                self.file_id,
                Position::new(line, col),
                Position::new(line, col + 1),
            ),
        )
    }
}
