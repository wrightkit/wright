//! The native OSTW parser: CST construction over the lexer token stream.
//!
//! Parses the syntax forms the committed protect-ban corpus exercises:
//! `import` statements, `globalvar`/`playervar` declarations (with explicit
//! indexes and pipe-union types), `define` constants/macros, typed
//! declarations (`Type name: expr;`, `Type name(params): expr;`), brace and
//! expression-bodied functions, `enum`, `rule:` blocks with `Event.`/`if`
//! modifiers and priorities, `class` bodies (syntax only), and statements
//! (`if`/`else`, `for`, `foreach`, `while`, `switch`/`case`/`default`,
//! `return`, `break`, `continue`, assignment, expression statements).
//!
//! Angle brackets are disambiguated like the reference: at expression start,
//! `<` followed by a string is a formatted string (`<"fmt", args>`), `<`
//! followed by a type then `>` is a cast (`<Type>expr`); in binary position
//! `<`/`<=` are comparisons. Every node carries its exact source span.

use wright_ir::source::{Position, Span};

use crate::cst::*;
use crate::diag::{FrontendError, FrontendResult};
use crate::lexer::{Token, TokenKind};

/// Parse one file's token stream into a CST.
pub fn parse(tokens: Vec<Token>) -> FrontendResult<File> {
    Parser::new(tokens).parse_file()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Depth of enclosing `<"..."` formatted strings. While greater than
    /// zero, a bare `>` terminates the enclosing formatted string instead of
    /// parsing as the `Greater` comparison operator.
    format_depth: u32,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Parser {
        Parser {
            tokens,
            pos: 0,
            format_depth: 0,
        }
    }

    // -- token helpers -----------------------------------------------------

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_kind(&self, offset: usize) -> TokenKind {
        self.tokens[(self.pos + offset).min(self.tokens.len() - 1)].kind
    }

    fn peek_ident(&self, offset: usize) -> Option<&str> {
        let token = &self.tokens[(self.pos + offset).min(self.tokens.len() - 1)];
        if token.kind == TokenKind::Ident {
            Some(&token.text)
        } else {
            None
        }
    }

    fn advance(&mut self) -> Token {
        let token = self.tokens[self.pos.min(self.tokens.len() - 1)].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        token
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.peek().kind == kind
    }

    fn at_ident(&self, name: &str) -> bool {
        self.peek().kind == TokenKind::Ident && self.peek().text == name
    }

    fn eat(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn eat_ident(&mut self, name: &str) -> bool {
        if self.at_ident(name) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: TokenKind, what: &str) -> FrontendResult<Token> {
        if self.at(kind) {
            Ok(self.advance())
        } else {
            Err(self.error(format!("expected {what}")))
        }
    }

    fn expect_ident(&mut self, what: &str) -> FrontendResult<String> {
        let token = self.peek().clone();
        if token.kind == TokenKind::Ident {
            self.advance();
            Ok(token.text)
        } else {
            Err(self.error(format!("expected {what}")))
        }
    }

    fn error(&self, message: impl Into<String>) -> FrontendError {
        FrontendError::at("ostw-parse-error", message, self.peek().span)
    }

    fn span_from(&self, start: Span) -> Span {
        Span::new(start.file, start.start, self.peek().span.start)
    }

    // -- file --------------------------------------------------------------

    fn parse_file(&mut self) -> FrontendResult<File> {
        let start = self.peek().span;
        let mut imports = Vec::new();
        let mut items = Vec::new();
        while !self.at(TokenKind::Eof) {
            if self.at_ident("import") {
                imports.push(self.parse_import()?);
            } else if self.at_ident("disabled") && self.peek_ident(1) == Some("rule") {
                self.advance();
                items.push(Item::Rule(self.parse_rule(true)?));
            } else if self.at_ident("rule") && self.peek_kind(1) == TokenKind::Colon {
                items.push(Item::Rule(self.parse_rule(false)?));
            } else if self.at_ident("globalvar") {
                self.advance();
                items.push(Item::GlobalVar(self.parse_var_decl(false)?));
            } else if self.at_ident("playervar") {
                self.advance();
                items.push(Item::PlayerVar(self.parse_var_decl(false)?));
            } else if self.at_ident("define") {
                items.push(Item::Define(self.parse_define()?));
            } else if self.at_ident("enum") {
                items.push(Item::Enum(self.parse_enum()?));
            } else if self.at_ident("class") {
                items.push(Item::Class(self.parse_class()?));
            } else {
                items.push(self.parse_typed_or_function()?);
            }
        }
        let span = Span::new(start.file, start.start, self.peek().span.start);
        Ok(File {
            imports,
            items,
            span,
        })
    }

    fn parse_import(&mut self) -> FrontendResult<Import> {
        let start = self.advance().span;
        let path_token = self.peek().clone();
        if !matches!(
            path_token.kind,
            TokenKind::String | TokenKind::VerbatimString
        ) {
            return Err(self.error("expected a quoted import path"));
        }
        self.advance();
        self.expect(TokenKind::Semi, "';' after import")?;
        let span = Span::new(start.file, start.start, path_token.span.end);
        Ok(Import {
            path: path_token.text,
            span,
        })
    }

    // -- declarations ------------------------------------------------------

    fn parse_var_decl(&mut self, _player: bool) -> FrontendResult<VarDecl> {
        let start = self.peek().span;
        let type_name = if self.at(TokenKind::Ident) && self.peek_kind(1) != TokenKind::Semi {
            Some(self.parse_type_ref()?)
        } else {
            None
        };
        // The exact declared-identifier occurrence (the rename target, #129).
        let name_token = self.peek().clone();
        let name = self.expect_ident("a variable name")?;
        let name_span = name_token.span;
        let mut index = None;
        let mut value = None;
        if self.eat(TokenKind::Assign) {
            value = Some(self.parse_expr()?);
        } else if !self.at(TokenKind::Semi) {
            // The explicit-index form: `globalvar Number i 127;`.
            index = Some(self.parse_expr()?);
        }
        self.expect(TokenKind::Semi, "';' after variable declaration")?;
        let span = self.span_from(start);
        Ok(VarDecl {
            type_name,
            name,
            index,
            value,
            span,
            name_span,
        })
    }

    fn parse_define(&mut self) -> FrontendResult<DefineDecl> {
        let start = self.advance().span;
        let name = self.expect_ident("a define name")?;
        let mut params = Vec::new();
        if self.eat(TokenKind::LParen) {
            params = self.parse_params_until_rparen()?;
        }
        self.expect(TokenKind::Colon, "':' after define")?;
        let value = self.parse_expr()?;
        self.expect(TokenKind::Semi, "';' after define")?;
        let span = self.span_from(start);
        Ok(DefineDecl {
            name,
            params,
            value,
            span,
        })
    }

    fn parse_enum(&mut self) -> FrontendResult<EnumDecl> {
        let start = self.advance().span;
        let name = self.expect_ident("an enum name")?;
        self.expect(TokenKind::LBrace, "'{' after enum name")?;
        let mut members = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            members.push(self.expect_ident("an enum member")?);
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RBrace, "'}' to close enum")?;
        let span = self.span_from(start);
        Ok(EnumDecl {
            name,
            members,
            span,
        })
    }

    /// Parse `Type name: expr;`, `Type name(params): expr;`, or
    /// `Type name(params) ["rule-name"] { body }` (function).
    fn parse_typed_or_function(&mut self) -> FrontendResult<Item> {
        let start = self.peek().span;
        let type_name = self.parse_type_ref()?;
        // The exact declared-identifier occurrence (the rename target, #129).
        let name_token = self.peek().clone();
        let name = self.expect_ident("a declaration name")?;
        let name_span = name_token.span;
        if self.at(TokenKind::Colon) {
            // Expression-bodied declaration.
            self.advance();
            let value = self.parse_expr()?;
            self.expect(TokenKind::Semi, "';' after declaration")?;
            let span = self.span_from(start);
            return Ok(Item::TypedDecl(TypedDecl {
                type_name,
                name,
                params: None,
                value,
                span,
            }));
        }
        self.expect(TokenKind::LParen, "'(' before parameters")?;
        let params = self.parse_params_until_rparen()?;
        if self.at(TokenKind::Colon) {
            // Expression-bodied function-like declaration.
            self.advance();
            let value = self.parse_expr()?;
            self.expect(TokenKind::Semi, "';' after declaration")?;
            let span = self.span_from(start);
            return Ok(Item::TypedDecl(TypedDecl {
                type_name,
                name,
                params: Some(params),
                value,
                span,
            }));
        }
        let rule_name = if self.at(TokenKind::String) || self.at(TokenKind::VerbatimString) {
            Some(self.advance().text)
        } else {
            None
        };
        self.expect(TokenKind::LBrace, "'{' to open function body")?;
        let body = self.parse_block_body()?;
        let span = self.span_from(start);
        Ok(Item::Function(FunctionDecl {
            return_type: Some(type_name),
            name,
            params,
            rule_name,
            body,
            span,
            name_span,
        }))
    }

    fn parse_class(&mut self) -> FrontendResult<ClassDecl> {
        let start = self.advance().span;
        let name = self.expect_ident("a class name")?;
        self.expect(TokenKind::LBrace, "'{' after class name")?;
        let mut members = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            members.push(self.parse_class_member()?);
        }
        self.expect(TokenKind::RBrace, "'}' to close class")?;
        let span = self.span_from(start);
        Ok(ClassDecl {
            name,
            members,
            span,
        })
    }

    fn parse_class_member(&mut self) -> FrontendResult<ClassMember> {
        // Members are written with a leading `public` in the corpus.
        let start = self.peek().span;
        self.eat_ident("public");
        let type_name = self.parse_type_ref()?;
        if type_name.name == "constructor"
            && type_name.array_depth == 0
            && self.at(TokenKind::LParen)
        {
            self.advance();
            let params = self.parse_params_until_rparen()?;
            self.expect(TokenKind::LBrace, "'{' to open constructor body")?;
            let body = self.parse_block_body()?;
            let span = self.span_from(start);
            return Ok(ClassMember::Constructor { params, body, span });
        }
        let name = self.expect_ident("a class member name")?;
        if self.eat(TokenKind::Semi) {
            let span = self.span_from(start);
            return Ok(ClassMember::Field {
                type_name,
                name,
                span,
            });
        }
        self.expect(TokenKind::LParen, "'(' before parameters")?;
        let params = self.parse_params_until_rparen()?;
        if self.at(TokenKind::Colon) {
            self.advance();
            let value = self.parse_expr()?;
            self.expect(TokenKind::Semi, "';' after class method")?;
            let span = self.span_from(start);
            return Ok(ClassMember::Method {
                type_name,
                name,
                params,
                value: Some(value),
                body: None,
                span,
            });
        }
        self.expect(TokenKind::LBrace, "'{' to open method body")?;
        let body = self.parse_block_body()?;
        let span = self.span_from(start);
        Ok(ClassMember::Method {
            type_name,
            name,
            params,
            value: None,
            body: Some(body),
            span,
        })
    }

    fn parse_rule(&mut self, disabled: bool) -> FrontendResult<RuleDecl> {
        let start = self.advance().span; // `rule`
        self.expect(TokenKind::Colon, "':' after rule")?;
        let mut name = None;
        let mut name_span = None;
        let mut priority = None;
        let mut event = None;
        let mut conditions = Vec::new();
        if self.at(TokenKind::String) || self.at(TokenKind::VerbatimString) {
            let name_token = self.advance();
            name = Some(name_token.text);
            // The exact rule-name occurrence is the string content between
            // the quotes (the token itself spans the quotes).
            name_span = Some(Span::new(
                name_token.span.file,
                Position::new(name_token.span.start.line, name_token.span.start.col + 1),
                Position::new(
                    name_token.span.end.line,
                    name_token
                        .span
                        .end
                        .col
                        .saturating_sub(1)
                        .max(name_token.span.start.col + 1),
                ),
            ));
        }
        loop {
            if self.at(TokenKind::LBrace) {
                break;
            }
            if self.at(TokenKind::Eof) || self.at(TokenKind::Semi) {
                break;
            }
            if self.at_ident("if") {
                self.advance();
                self.expect(TokenKind::LParen, "'(' after if")?;
                let condition = self.parse_expr()?;
                self.expect(TokenKind::RParen, "')' after condition")?;
                conditions.push(condition);
            } else if self.at_ident("Event") && self.peek_kind(1) == TokenKind::Dot {
                event = Some(self.parse_postfix_expr()?);
            } else if self.at(TokenKind::Minus) || self.at(TokenKind::Number) {
                priority = Some(self.parse_unary()?);
            } else {
                return Err(self.error(format!(
                    "unexpected token '{}' in rule modifiers",
                    self.peek().text
                )));
            }
        }
        let mut body = Vec::new();
        if self.eat(TokenKind::LBrace) {
            body = self.parse_block_body()?;
        }
        let span = self.span_from(start);
        Ok(RuleDecl {
            disabled,
            name,
            name_span,
            priority,
            event,
            conditions,
            body,
            span,
        })
    }

    // -- parameters --------------------------------------------------------

    fn parse_params_until_rparen(&mut self) -> FrontendResult<Vec<Param>> {
        let mut params = Vec::new();
        while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
            params.push(self.parse_param()?);
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen, "')' after parameters")?;
        Ok(params)
    }

    fn parse_param(&mut self) -> FrontendResult<Param> {
        let start = self.peek().span;
        // The `in` marker (corpus: `in Player | Player[] VisibleTo = ...`).
        self.eat_ident("in");
        // Parameters in the corpus always declare a type (including array and
        // pipe-union types); a missing type is rejected explicitly.
        let type_name = Some(self.parse_type_ref()?);
        let name = self.expect_ident("a parameter name")?;
        let default = if self.eat(TokenKind::Assign) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let span = self.span_from(start);
        Ok(Param {
            type_name,
            name,
            default,
            span,
        })
    }

    fn parse_type_ref(&mut self) -> FrontendResult<TypeRef> {
        let start = self.peek().span;
        let name = self.expect_ident("a type name")?;
        let mut array_depth = 0;
        while self.eat(TokenKind::LBracket) {
            self.expect(TokenKind::RBracket, "']' in array type")?;
            array_depth += 1;
        }
        let mut unions = Vec::new();
        while self.at(TokenKind::Pipe) {
            self.advance();
            let union_start = self.peek().span;
            let union_name = self.expect_ident("a union type name")?;
            let mut union_depth = 0;
            while self.eat(TokenKind::LBracket) {
                self.expect(TokenKind::RBracket, "']' in union array type")?;
                union_depth += 1;
            }
            let span = Span::new(union_start.file, union_start.start, self.peek().span.start);
            unions.push(TypeRef {
                name: union_name,
                array_depth: union_depth,
                unions: Vec::new(),
                span,
            });
        }
        let span = self.span_from(start);
        Ok(TypeRef {
            name,
            array_depth,
            unions,
            span,
        })
    }

    // -- statements --------------------------------------------------------

    fn parse_block_body(&mut self) -> FrontendResult<Vec<Stmt>> {
        let mut statements = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            statements.push(self.parse_stmt()?);
        }
        self.expect(TokenKind::RBrace, "'}' to close block")?;
        Ok(statements)
    }

    fn parse_stmt(&mut self) -> FrontendResult<Stmt> {
        let start = self.peek().span;
        if self.at_ident("if") {
            return self.parse_if_stmt(start);
        }
        if self.at_ident("for") && self.peek_kind(1) == TokenKind::LParen {
            return self.parse_for_stmt(start);
        }
        if self.at_ident("foreach") {
            return self.parse_foreach_stmt(start);
        }
        if self.at_ident("while") {
            return self.parse_while_stmt(start);
        }
        if self.at_ident("switch") {
            return self.parse_switch_stmt(start);
        }
        if self.at_ident("return") {
            self.advance();
            let value = if self.at(TokenKind::Semi) {
                None
            } else {
                Some(self.parse_expr()?)
            };
            self.expect(TokenKind::Semi, "';' after return")?;
            return Ok(Stmt::Return {
                value,
                span: self.span_from(start),
            });
        }
        if self.eat_ident("break") {
            self.expect(TokenKind::Semi, "';' after break")?;
            return Ok(Stmt::Break {
                span: self.span_from(start),
            });
        }
        if self.eat_ident("continue") {
            self.expect(TokenKind::Semi, "';' after continue")?;
            return Ok(Stmt::Continue {
                span: self.span_from(start),
            });
        }
        if self.at(TokenKind::LBrace) {
            self.advance();
            let body = self.parse_block_body()?;
            return Ok(Stmt::Block {
                body,
                span: self.span_from(start),
            });
        }
        if self.at_ident("define") && self.peek_kind(1) == TokenKind::Ident {
            // Local define: `define name = expr;`.
            self.advance();
            let name = self.expect_ident("a define name")?;
            self.expect(TokenKind::Assign, "'=' in local define")?;
            let value = self.parse_expr()?;
            self.expect(TokenKind::Semi, "';' after define")?;
            return Ok(Stmt::LocalDefine {
                name,
                value,
                span: self.span_from(start),
            });
        }
        if self.at(TokenKind::Ident) && self.peek_kind(1) == TokenKind::Ident {
            // Local typed declaration: `Type name = expr;`.
            let type_name = self.parse_type_ref()?;
            let name = self.expect_ident("a declaration name")?;
            self.expect(TokenKind::Assign, "'=' in local declaration")?;
            let value = self.parse_expr()?;
            self.expect(TokenKind::Semi, "';' after declaration")?;
            return Ok(Stmt::LocalDecl {
                type_name,
                name,
                value,
                span: self.span_from(start),
            });
        }
        // Expression or assignment statement.
        let expression = self.parse_assign_expr()?;
        self.expect(TokenKind::Semi, "';' after statement")?;
        let span = self.span_from(start);
        match expression {
            Expr::Assign {
                target, op, value, ..
            } => Ok(Stmt::Assign {
                target: *target,
                op,
                value: *value,
                span,
            }),
            other => Ok(Stmt::Expr { expr: other, span }),
        }
    }

    fn parse_if_stmt(&mut self, start: Span) -> FrontendResult<Stmt> {
        self.advance(); // if
        self.expect(TokenKind::LParen, "'(' after if")?;
        let condition = self.parse_expr()?;
        self.expect(TokenKind::RParen, "')' after if condition")?;
        let body = self.parse_stmt_or_block()?;
        let mut branches = vec![IfBranch {
            condition,
            body,
            span: self.span_from(start),
        }];
        let mut else_body = None;
        if self.at_ident("else") {
            self.advance();
            if self.at_ident("if") {
                // else if (...) {...} — fold into the next branch.
                let next = self.parse_if_stmt(start)?;
                if let Stmt::If {
                    branches: nested,
                    else_body: nested_else,
                    ..
                } = next
                {
                    branches.extend(nested);
                    else_body = nested_else;
                }
            } else {
                let body = self.parse_stmt_or_block()?;
                else_body = Some(body);
            }
        }
        Ok(Stmt::If {
            branches,
            else_body,
            span: self.span_from(start),
        })
    }

    fn parse_for_stmt(&mut self, start: Span) -> FrontendResult<Stmt> {
        self.advance(); // for
        self.expect(TokenKind::LParen, "'(' after for")?;
        let init = if self.at(TokenKind::Semi) {
            None
        } else {
            Some(self.parse_assign_expr()?)
        };
        self.expect(TokenKind::Semi, "';' after for initializer")?;
        let condition = if self.at(TokenKind::Semi) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect(TokenKind::Semi, "';' after for condition")?;
        let increment = if self.at(TokenKind::RParen) {
            None
        } else {
            Some(self.parse_assign_expr()?)
        };
        self.expect(TokenKind::RParen, "')' after for header")?;
        let body = self.parse_stmt_or_block()?;
        Ok(Stmt::For {
            init,
            condition,
            increment,
            body,
            span: self.span_from(start),
        })
    }

    fn parse_foreach_stmt(&mut self, start: Span) -> FrontendResult<Stmt> {
        self.advance(); // foreach
        self.expect(TokenKind::LParen, "'(' after foreach")?;
        let var_type = if self.at(TokenKind::Ident) && self.peek_ident(1) != Some("in") {
            Some(self.parse_type_ref()?)
        } else {
            None
        };
        let var = self.expect_ident("a foreach variable name")?;
        self.expect_ident("in")?;
        let iterable = self.parse_expr()?;
        self.expect(TokenKind::RParen, "')' after foreach header")?;
        let body = self.parse_stmt_or_block()?;
        Ok(Stmt::Foreach {
            var_type,
            var,
            iterable,
            body,
            span: self.span_from(start),
        })
    }

    fn parse_while_stmt(&mut self, start: Span) -> FrontendResult<Stmt> {
        self.advance(); // while
        self.expect(TokenKind::LParen, "'(' after while")?;
        let condition = self.parse_expr()?;
        self.expect(TokenKind::RParen, "')' after while condition")?;
        let body = self.parse_stmt_or_block()?;
        Ok(Stmt::While {
            condition,
            body,
            span: self.span_from(start),
        })
    }

    fn parse_switch_stmt(&mut self, start: Span) -> FrontendResult<Stmt> {
        self.advance(); // switch
        self.expect(TokenKind::LParen, "'(' after switch")?;
        let value = self.parse_expr()?;
        self.expect(TokenKind::RParen, "')' after switch value")?;
        self.expect(TokenKind::LBrace, "'{' to open switch")?;
        let mut cases = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            let case_start = self.peek().span;
            let case_value = if self.eat_ident("default") {
                None
            } else {
                self.expect_ident("case")?;
                Some(self.parse_expr()?)
            };
            self.expect(TokenKind::Colon, "':' after switch case")?;
            let mut body = Vec::new();
            while !self.at(TokenKind::RBrace)
                && !self.at(TokenKind::Eof)
                && !self.at_ident("case")
                && !self.at_ident("default")
            {
                body.push(self.parse_stmt()?);
            }
            let span = self.span_from(case_start);
            cases.push(SwitchCase {
                value: case_value,
                body,
                span,
            });
        }
        self.expect(TokenKind::RBrace, "'}' to close switch")?;
        Ok(Stmt::Switch {
            value,
            cases,
            span: self.span_from(start),
        })
    }

    /// A statement body: a `{ ... }` block or a single statement.
    fn parse_stmt_or_block(&mut self) -> FrontendResult<Vec<Stmt>> {
        if self.at(TokenKind::LBrace) {
            self.advance();
            self.parse_block_body()
        } else {
            Ok(vec![self.parse_stmt()?])
        }
    }

    // -- expressions -------------------------------------------------------

    /// Parse a full expression (ternary level).
    fn parse_expr(&mut self) -> FrontendResult<Expr> {
        self.parse_ternary_expr()
    }

    /// An assignment expression (`x = e`, `x += e`) — used by statements and
    /// for-loop headers.
    fn parse_assign_expr(&mut self) -> FrontendResult<Expr> {
        let start = self.peek().span;
        let target = self.parse_ternary_expr()?;
        let op = match self.peek().kind {
            TokenKind::Assign => AssignOp::Assign,
            TokenKind::PlusAssign => AssignOp::AddAssign,
            TokenKind::MinusAssign => AssignOp::SubtractAssign,
            TokenKind::StarAssign => AssignOp::MultiplyAssign,
            TokenKind::SlashAssign => AssignOp::DivideAssign,
            TokenKind::PercentAssign => AssignOp::ModuloAssign,
            _ => return Ok(target),
        };
        self.advance();
        let value = self.parse_assign_expr()?;
        let span = self.span_from(start);
        Ok(Expr::Assign {
            target: Box::new(target),
            op,
            value: Box::new(value),
            span,
        })
    }

    fn parse_ternary_expr(&mut self) -> FrontendResult<Expr> {
        let start = self.peek().span;
        let condition = self.parse_binary_expr(0)?;
        if !self.eat(TokenKind::Question) {
            return Ok(condition);
        }
        let then_value = self.parse_ternary_expr()?;
        self.expect(TokenKind::Colon, "':' in ternary expression")?;
        let else_value = self.parse_ternary_expr()?;
        let span = self.span_from(start);
        Ok(Expr::Ternary {
            condition: Box::new(condition),
            then_value: Box::new(then_value),
            else_value: Box::new(else_value),
            span,
        })
    }

    fn parse_binary_expr(&mut self, min_precedence: u8) -> FrontendResult<Expr> {
        let mut left = self.parse_unary_expr()?;
        loop {
            // A bare `>` while inside a `<"..."` formatted string closes it
            // (the closing `>` is consumed by parse_format_string).
            if self.format_depth > 0 && self.at(TokenKind::Gt) {
                break;
            }
            let Some((op, precedence)) = self.binary_op() else {
                break;
            };
            if precedence < min_precedence {
                break;
            }
            self.advance();
            let right = self.parse_binary_expr(precedence + 1)?;
            let span = Span::new(left.span().file, left.span().start, right.span().end);
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn binary_op(&self) -> Option<(BinaryOp, u8)> {
        let (op, precedence) = match self.peek().kind {
            TokenKind::Or => (BinaryOp::Or, 1),
            TokenKind::And => (BinaryOp::And, 2),
            TokenKind::Eq => (BinaryOp::Equal, 3),
            TokenKind::Ne => (BinaryOp::NotEqual, 3),
            TokenKind::Lt => (BinaryOp::Less, 4),
            TokenKind::Le => (BinaryOp::LessEqual, 4),
            TokenKind::Gt => (BinaryOp::Greater, 4),
            TokenKind::Ge => (BinaryOp::GreaterEqual, 4),
            TokenKind::Plus => (BinaryOp::Add, 5),
            TokenKind::Minus => (BinaryOp::Subtract, 5),
            TokenKind::Star => (BinaryOp::Multiply, 6),
            TokenKind::Slash => (BinaryOp::Divide, 6),
            TokenKind::Percent => (BinaryOp::Modulo, 6),
            TokenKind::Power => (BinaryOp::Power, 7),
            _ => return None,
        };
        Some((op, precedence))
    }

    fn parse_unary_expr(&mut self) -> FrontendResult<Expr> {
        let start = self.peek().span;
        if self.eat(TokenKind::Minus) {
            let operand = self.parse_unary_expr()?;
            let span = self.span_from(start);
            return Ok(Expr::Unary {
                op: UnaryOp::Negate,
                operand: Box::new(operand),
                span,
            });
        }
        if self.eat(TokenKind::Bang) {
            let operand = self.parse_unary_expr()?;
            let span = self.span_from(start);
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                operand: Box::new(operand),
                span,
            });
        }
        self.parse_postfix_expr()
    }

    /// Alias used by the rule-priority modifier (`-1`).
    fn parse_unary(&mut self) -> FrontendResult<Expr> {
        self.parse_unary_expr()
    }

    fn parse_postfix_expr(&mut self) -> FrontendResult<Expr> {
        let mut expression = self.parse_primary_expr()?;
        loop {
            let start = expression.span();
            if self.eat(TokenKind::Dot) {
                let name = self.expect_ident("a member name")?;
                let span = self.span_from(start);
                expression = Expr::Member {
                    receiver: Box::new(expression),
                    name,
                    span,
                };
            } else if self.at(TokenKind::LParen) {
                let args = self.parse_call_args()?;
                let span = self.span_from(start);
                expression = Expr::Call {
                    callee: Box::new(expression),
                    args,
                    span,
                };
            } else if self.eat(TokenKind::LBracket) {
                let index = self.parse_expr()?;
                self.expect(TokenKind::RBracket, "']' after index")?;
                let span = self.span_from(start);
                expression = Expr::Index {
                    array: Box::new(expression),
                    index: Box::new(index),
                    span,
                };
            } else if self.at(TokenKind::PlusPlus) {
                self.advance();
                let span = self.span_from(start);
                expression = Expr::Postfix {
                    op: PostfixOp::Increment,
                    operand: Box::new(expression),
                    span,
                };
            } else if self.at(TokenKind::MinusMinus) {
                self.advance();
                let span = self.span_from(start);
                expression = Expr::Postfix {
                    op: PostfixOp::Decrement,
                    operand: Box::new(expression),
                    span,
                };
            } else {
                break;
            }
        }
        Ok(expression)
    }

    fn parse_primary_expr(&mut self) -> FrontendResult<Expr> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Number => {
                self.advance();
                let value = token.text.parse::<f64>().unwrap_or(0.0);
                Ok(Expr::Number {
                    value,
                    text: token.text,
                    span: token.span,
                })
            }
            TokenKind::String => {
                self.advance();
                Ok(Expr::String {
                    value: token.text,
                    span: token.span,
                })
            }
            TokenKind::VerbatimString => {
                self.advance();
                Ok(Expr::VerbatimString {
                    value: token.text,
                    span: token.span,
                })
            }
            TokenKind::Ident => {
                if token.text == "new" {
                    self.advance();
                    let type_name = self.expect_ident("a type name after 'new'")?;
                    let args = self.parse_call_args()?;
                    let span = Span::new(token.span.file, token.span.start, self.peek().span.start);
                    return Ok(Expr::New {
                        type_name,
                        args,
                        span,
                    });
                }
                self.advance();
                match token.text.as_str() {
                    "null" => Ok(Expr::Null { span: token.span }),
                    "true" => Ok(Expr::Bool {
                        value: true,
                        span: token.span,
                    }),
                    "false" => Ok(Expr::Bool {
                        value: false,
                        span: token.span,
                    }),
                    _ => Ok(Expr::Ident {
                        name: token.text,
                        span: token.span,
                    }),
                }
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(TokenKind::RParen, "')' after expression")?;
                Ok(inner)
            }
            TokenKind::LBracket => {
                self.advance();
                let mut elements = Vec::new();
                while !self.at(TokenKind::RBracket) && !self.at(TokenKind::Eof) {
                    elements.push(self.parse_expr()?);
                    if !self.eat(TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::RBracket, "']' to close array")?;
                let span = Span::new(token.span.file, token.span.start, self.peek().span.start);
                Ok(Expr::Array { elements, span })
            }
            TokenKind::Lt => {
                // `<"..."` → formatted string; `<Type>` → cast.
                if matches!(
                    self.peek_kind(1),
                    TokenKind::String | TokenKind::VerbatimString
                ) {
                    self.parse_format_string(token.span)
                } else if self.is_type_cast_lookahead() {
                    self.advance(); // `<`
                    let type_name = self.parse_type_ref()?;
                    self.expect(TokenKind::Gt, "'>' to close cast")?;
                    let value = self.parse_unary_expr()?;
                    let span = Span::new(token.span.file, token.span.start, value.span().end);
                    Ok(Expr::Cast {
                        type_name,
                        value: Box::new(value),
                        span,
                    })
                } else {
                    Err(self.error("expected a formatted string or type cast after '<'"))
                }
            }
            TokenKind::Eof => Err(self.error("unexpected end of file in expression")),
            _ => Err(self.error("unexpected token in expression")),
        }
    }

    fn parse_format_string(&mut self, start: Span) -> FrontendResult<Expr> {
        self.advance(); // `<`
        self.format_depth += 1;
        let format = self.parse_primary_expr()?;
        let mut args = Vec::new();
        while self.eat(TokenKind::Comma) {
            args.push(self.parse_expr()?);
        }
        self.format_depth -= 1;
        self.expect(TokenKind::Gt, "'>' to close formatted string")?;
        let span = Span::new(start.file, start.start, self.peek().span.start);
        Ok(Expr::FormatString {
            format: Box::new(format),
            args,
            span,
        })
    }

    fn parse_call_args(&mut self) -> FrontendResult<Vec<CallArg>> {
        self.advance(); // `(`
        let mut args = Vec::new();
        while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
            args.push(self.parse_call_arg()?);
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen, "')' after arguments")?;
        Ok(args)
    }

    fn parse_call_arg(&mut self) -> FrontendResult<CallArg> {
        let start = self.peek().span;
        // `Name: value` — a named argument (an identifier directly followed
        // by a colon at argument position).
        if self.at(TokenKind::Ident) && self.peek_kind(1) == TokenKind::Colon {
            let name = self.advance().text;
            self.advance(); // `:`
            let value = self.parse_expr()?;
            let span = Span::new(start.file, start.start, value.span().end);
            return Ok(CallArg::Named { name, value, span });
        }
        let value = self.parse_expr()?;
        let span = Span::new(start.file, start.start, value.span().end);
        Ok(CallArg::Positional { value, span })
    }

    /// Lookahead: is the current `<` the start of a `<Type>` cast?
    fn is_type_cast_lookahead(&self) -> bool {
        // `<` Ident ... `>` with optional array markers and pipe unions.
        let mut index = self.pos + 1;
        let Some(first) = self.tokens.get(index) else {
            return false;
        };
        if first.kind != TokenKind::Ident {
            return false;
        }
        index += 1;
        loop {
            match self.tokens.get(index).map(|token| token.kind) {
                Some(TokenKind::LBracket) => {
                    if self.tokens.get(index + 1).map(|token| token.kind)
                        != Some(TokenKind::RBracket)
                    {
                        return false;
                    }
                    index += 2;
                }
                Some(TokenKind::Pipe) => {
                    if self.tokens.get(index + 1).map(|token| token.kind) != Some(TokenKind::Ident)
                    {
                        return false;
                    }
                    index += 2;
                    // Allow array markers after a union member.
                    while let Some(TokenKind::LBracket) =
                        self.tokens.get(index).map(|token| token.kind)
                    {
                        if self.tokens.get(index + 1).map(|token| token.kind)
                            != Some(TokenKind::RBracket)
                        {
                            return false;
                        }
                        index += 2;
                    }
                }
                Some(TokenKind::Gt) => return true,
                _ => return false,
            }
        }
    }
}

impl Expr {
    fn span(&self) -> Span {
        match self {
            Expr::Number { span, .. }
            | Expr::String { span, .. }
            | Expr::VerbatimString { span, .. }
            | Expr::Ident { span, .. }
            | Expr::Null { span }
            | Expr::Bool { span, .. }
            | Expr::Member { span, .. }
            | Expr::Call { span, .. }
            | Expr::Index { span, .. }
            | Expr::Array { span, .. }
            | Expr::FormatString { span, .. }
            | Expr::Cast { span, .. }
            | Expr::Unary { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Ternary { span, .. }
            | Expr::New { span, .. }
            | Expr::Assign { span, .. }
            | Expr::Postfix { span, .. } => *span,
        }
    }
}
