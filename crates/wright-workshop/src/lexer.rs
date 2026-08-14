//! Tokenizer for localized vanilla Workshop text.
//!
//! Tokens carry 1-based line/column spans so parser diagnostics and the
//! produced WIR preserve source locations. Workshop identifiers are
//! multi-word phrases; the tokenizer emits single [`Word`] tokens and the
//! parser groups them, which keeps locale spellings data-driven.

use wright_ir::source::Position;

/// A lexical token kind.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// A run of identifier characters (`[A-Za-z_][A-Za-z0-9_]*`).
    Word(String),
    /// A numeric literal.
    Number(f64),
    /// A string literal (content, unescaped).
    String(String),
    /// An operator token: `== != < <= > >= + - * / %`.
    Op(String),
    LParen,
    RParen,
    Comma,
    Semi,
    LBrace,
    RBrace,
    Colon,
    Dot,
    Eof,
}

/// A token with its source span.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: Position,
    pub end: Position,
}

/// A lexing error with a source position.
#[derive(Debug, Clone, PartialEq)]
pub struct LexError {
    pub message: String,
    pub position: Position,
}

/// Tokenize Workshop text.
pub fn tokenize(input: &str) -> Result<Vec<Token>, LexError> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    let mut line = 1u32;
    let mut col = 1u32;

    macro_rules! pos {
        () => {
            Position::new(line, col)
        };
    }

    let advance = |index: &mut usize, line: &mut u32, col: &mut u32, chars: &Vec<char>| {
        let ch = chars[*index];
        *index += 1;
        if ch == '\n' {
            *line += 1;
            *col = 1;
        } else {
            *col += 1;
        }
    };

    while index < chars.len() {
        let ch = chars[index];
        match ch {
            ' ' | '\t' | '\r' | '\n' => advance(&mut index, &mut line, &mut col, &chars),
            '(' => {
                let start = pos!();
                advance(&mut index, &mut line, &mut col, &chars);
                tokens.push(Token {
                    kind: TokenKind::LParen,
                    start,
                    end: pos!(),
                });
            }
            ')' => {
                let start = pos!();
                advance(&mut index, &mut line, &mut col, &chars);
                tokens.push(Token {
                    kind: TokenKind::RParen,
                    start,
                    end: pos!(),
                });
            }
            ',' => {
                let start = pos!();
                advance(&mut index, &mut line, &mut col, &chars);
                tokens.push(Token {
                    kind: TokenKind::Comma,
                    start,
                    end: pos!(),
                });
            }
            ';' => {
                let start = pos!();
                advance(&mut index, &mut line, &mut col, &chars);
                tokens.push(Token {
                    kind: TokenKind::Semi,
                    start,
                    end: pos!(),
                });
            }
            '{' => {
                let start = pos!();
                advance(&mut index, &mut line, &mut col, &chars);
                tokens.push(Token {
                    kind: TokenKind::LBrace,
                    start,
                    end: pos!(),
                });
            }
            '}' => {
                let start = pos!();
                advance(&mut index, &mut line, &mut col, &chars);
                tokens.push(Token {
                    kind: TokenKind::RBrace,
                    start,
                    end: pos!(),
                });
            }
            ':' => {
                let start = pos!();
                advance(&mut index, &mut line, &mut col, &chars);
                tokens.push(Token {
                    kind: TokenKind::Colon,
                    start,
                    end: pos!(),
                });
            }
            '.' => {
                let start = pos!();
                advance(&mut index, &mut line, &mut col, &chars);
                tokens.push(Token {
                    kind: TokenKind::Dot,
                    start,
                    end: pos!(),
                });
            }
            '"' => {
                let start = pos!();
                advance(&mut index, &mut line, &mut col, &chars);
                let mut content = String::new();
                let mut closed = false;
                while index < chars.len() {
                    let c = chars[index];
                    // Decode the escape spellings the emitter produces so a
                    // settings/value string round-trips byte-identically
                    // (`\"`, `\\`, `\n`, `\r`, `\t`; #87).
                    if c == '\\' && index + 1 < chars.len() {
                        let escaped = chars[index + 1];
                        let decoded = match escaped {
                            '"' => Some('"'),
                            '\\' => Some('\\'),
                            'n' => Some('\n'),
                            'r' => Some('\r'),
                            't' => Some('\t'),
                            _ => None,
                        };
                        if let Some(decoded) = decoded {
                            advance(&mut index, &mut line, &mut col, &chars);
                            advance(&mut index, &mut line, &mut col, &chars);
                            content.push(decoded);
                            continue;
                        }
                    }
                    if c == '"' {
                        advance(&mut index, &mut line, &mut col, &chars);
                        closed = true;
                        break;
                    }
                    advance(&mut index, &mut line, &mut col, &chars);
                    content.push(c);
                }
                if !closed {
                    return Err(LexError {
                        message: "unterminated string literal".to_string(),
                        position: start,
                    });
                }
                tokens.push(Token {
                    kind: TokenKind::String(content),
                    start,
                    end: pos!(),
                });
            }
            '0'..='9' => {
                let start = pos!();
                let mut text = String::new();
                while index < chars.len() && (chars[index].is_ascii_digit() || chars[index] == '.')
                {
                    text.push(chars[index]);
                    advance(&mut index, &mut line, &mut col, &chars);
                }
                let value: f64 = text.parse().map_err(|_| LexError {
                    message: format!("invalid number '{text}'"),
                    position: start,
                })?;
                tokens.push(Token {
                    kind: TokenKind::Number(value),
                    start,
                    end: pos!(),
                });
            }
            '=' => {
                let start = pos!();
                advance(&mut index, &mut line, &mut col, &chars);
                if index < chars.len() && chars[index] == '=' {
                    advance(&mut index, &mut line, &mut col, &chars);
                    tokens.push(Token {
                        kind: TokenKind::Op("==".to_string()),
                        start,
                        end: pos!(),
                    });
                } else {
                    return Err(LexError {
                        message: "unexpected '='; expected '=='".to_string(),
                        position: start,
                    });
                }
            }
            '!' => {
                let start = pos!();
                advance(&mut index, &mut line, &mut col, &chars);
                if index < chars.len() && chars[index] == '=' {
                    advance(&mut index, &mut line, &mut col, &chars);
                    tokens.push(Token {
                        kind: TokenKind::Op("!=".to_string()),
                        start,
                        end: pos!(),
                    });
                } else {
                    return Err(LexError {
                        message: "unexpected '!'".to_string(),
                        position: start,
                    });
                }
            }
            '<' | '>' => {
                let start = pos!();
                let op = ch.to_string();
                advance(&mut index, &mut line, &mut col, &chars);
                if index < chars.len() && chars[index] == '=' {
                    advance(&mut index, &mut line, &mut col, &chars);
                    tokens.push(Token {
                        kind: TokenKind::Op(format!("{op}=")),
                        start,
                        end: pos!(),
                    });
                } else {
                    tokens.push(Token {
                        kind: TokenKind::Op(op),
                        start,
                        end: pos!(),
                    });
                }
            }
            '+' | '*' | '/' | '%' => {
                let start = pos!();
                let op = ch.to_string();
                advance(&mut index, &mut line, &mut col, &chars);
                tokens.push(Token {
                    kind: TokenKind::Op(op),
                    start,
                    end: pos!(),
                });
            }
            '-' => {
                let start = pos!();
                advance(&mut index, &mut line, &mut col, &chars);
                tokens.push(Token {
                    kind: TokenKind::Op("-".to_string()),
                    start,
                    end: pos!(),
                });
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = pos!();
                let mut word = String::new();
                while index < chars.len() {
                    let c = chars[index];
                    let interior_dash = c == '-'
                        && index + 1 < chars.len()
                        && (chars[index + 1].is_alphanumeric() || chars[index + 1] == '_');
                    if c.is_alphanumeric() || c == '_' || interior_dash {
                        word.push(c);
                        advance(&mut index, &mut line, &mut col, &chars);
                    } else {
                        break;
                    }
                }
                tokens.push(Token {
                    kind: TokenKind::Word(word),
                    start,
                    end: pos!(),
                });
            }
            other => {
                return Err(LexError {
                    message: format!("unexpected character '{other}'"),
                    position: pos!(),
                });
            }
        }
    }

    let end = pos!();
    tokens.push(Token {
        kind: TokenKind::Eof,
        start: end,
        end,
    });
    Ok(tokens)
}
