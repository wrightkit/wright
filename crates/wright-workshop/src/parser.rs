//! Native localized Workshop parser.
//!
//! Parses vanilla Workshop text directly into validated, locale-independent
//! Workshop IR. Localized actions, values, events, enums, and structural
//! keywords resolve through the canonical catalog; malformed input,
//! unknown spellings, and recognized-but-unsupported constructs are reported
//! as distinct structured diagnostics with source spans.

use std::collections::HashMap;

use wright_ir::source::{Position, SourceFile, Span};
use wright_ir::wir::{self, Action, Event, ModifyOp, Value, ValueNode};

use crate::catalog::{Catalog, Kind, Locale};
use crate::error::{Result, WorkshopError};
use crate::lexer::{Token, TokenKind, tokenize};

/// Where action parsing stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    /// The enclosing `}` was consumed.
    SectionClosed,
    /// The `End` keyword is next (not consumed).
    End,
    /// The `Else If` keyword is next (not consumed).
    ElseIf,
    /// The `Else` keyword is next (not consumed).
    Else,
}

/// Parse localized Workshop text into Workshop IR.
pub fn parse(input: &str, catalog: &Catalog, locale: &Locale) -> Result<wir::Program> {
    let tokens = tokenize(input).map_err(|error| WorkshopError::Malformed {
        message: error.message,
        span: Some(synthetic_span(error.position)),
    })?;
    Parser {
        tokens,
        pos: 0,
        catalog,
        locale: locale.clone(),
        target: wir::Program::default(),
        globals: HashMap::new(),
        players: HashMap::new(),
        subroutines: HashMap::new(),
    }
    .program()
}

/// A synthetic single-position span (used before a file registry exists).
fn synthetic_span(position: Position) -> Span {
    Span::new(wright_ir::ids::Id::from_index(0), position, position)
}

struct Parser<'a> {
    tokens: Vec<Token>,
    pos: usize,
    catalog: &'a Catalog,
    locale: Locale,
    target: wir::Program,
    globals: HashMap<String, wir::GlobalVarId>,
    players: HashMap<String, wir::PlayerVarId>,
    subroutines: HashMap<String, wir::SubroutineId>,
}

impl Parser<'_> {
    fn program(mut self) -> Result<wir::Program> {
        let file = self.target.files.push(SourceFile::new("workshop.txt"));
        // Re-point synthetic spans at the real file id by keeping a helper.
        let _ = file;

        loop {
            let phrase = match self.peek() {
                Some(Token {
                    kind: TokenKind::Word(word),
                    ..
                }) => word.clone(),
                Some(Token {
                    kind: TokenKind::Eof,
                    ..
                }) => break,
                Some(token) => {
                    return Err(self.malformed("expected a top-level section", &token));
                }
                None => break,
            };
            match phrase.as_str() {
                "variables" => self.variables_section()?,
                "subroutines" => self.subroutines_section()?,
                "rule" => self.rule()?,
                other => {
                    return Err(self.unknown("top-level section", other));
                }
            }
        }
        Ok(self.target)
    }

    fn variables_section(&mut self) -> Result<()> {
        self.expect_word("variables")?;
        self.expect(TokenKind::LBrace, "expected '{' after 'variables'")?;
        let mut saw_section = false;
        loop {
            match self.peek() {
                Some(Token {
                    kind: TokenKind::RBrace,
                    ..
                }) => {
                    self.pos += 1;
                    break;
                }
                Some(Token {
                    kind: TokenKind::Word(word),
                    ..
                }) if word == "global" => {
                    self.pos += 1;
                    self.expect(TokenKind::Colon, "expected ':' after 'global'")?;
                    while let Some(Token {
                        kind: TokenKind::Number { .. },
                        ..
                    }) = self.peek()
                    {
                        let variable = self.variable_line()?;
                        let id = self.target.global_variables.push(variable);
                        self.globals.insert(
                            self.target.global_variables.get(id).unwrap().name.clone(),
                            id,
                        );
                    }
                    saw_section = true;
                }
                Some(Token {
                    kind: TokenKind::Word(word),
                    ..
                }) if word == "player" => {
                    self.pos += 1;
                    self.expect(TokenKind::Colon, "expected ':' after 'player'")?;
                    while let Some(Token {
                        kind: TokenKind::Number { .. },
                        ..
                    }) = self.peek()
                    {
                        let variable = self.variable_line()?;
                        let id = self.target.player_variables.push(variable);
                        self.players.insert(
                            self.target.player_variables.get(id).unwrap().name.clone(),
                            id,
                        );
                    }
                    saw_section = true;
                }
                Some(token) => {
                    return Err(self.malformed("expected 'global', 'player', or '}'", &token));
                }
                None => {
                    return Err(self.malformed("unexpected end of input in variables", self.eof()));
                }
            }
        }
        if !saw_section {
            return Err(self.malformed("variables section is empty", self.previous()));
        }
        Ok(())
    }

    fn variable_line(&mut self) -> Result<wir::WorkshopVariable> {
        let (index, span) = match self.next() {
            Some(Token {
                kind: TokenKind::Number { value, .. },
                start,
                end,
            }) => (
                value as u32,
                Span::new(synthetic_span(start).file, start, end),
            ),
            Some(token) => return Err(self.malformed("expected a variable index", &token)),
            None => return Err(self.malformed("expected a variable index", self.eof())),
        };
        self.expect(TokenKind::Colon, "expected ':' after variable index")?;
        let (name, name_start, name_end) = self.phrase_on_line()?;
        let name_span = Span::new(self.file(), name_start, name_end);
        Ok(wir::WorkshopVariable {
            name,
            index,
            span: Some(if span.file.index() == 0 {
                name_span
            } else {
                span
            }),
            // Workshop-text sources carry no `.opy` identifier provenance;
            // exact rename occurrences are only produced by the native path.
            name_span: None,
            initializer: None,
        })
    }

    fn subroutines_section(&mut self) -> Result<()> {
        self.expect_word("subroutines")?;
        self.expect(TokenKind::LBrace, "expected '{' after 'subroutines'")?;
        while let Some(Token {
            kind: TokenKind::Number { .. },
            ..
        }) = self.peek()
        {
            let index = match self.next() {
                Some(Token {
                    kind: TokenKind::Number { value, .. },
                    ..
                }) => value as u32,
                _ => unreachable!(),
            };
            self.expect(TokenKind::Colon, "expected ':' after subroutine index")?;
            let (name, start, end) = self.phrase_on_line()?;
            let id = self.target.subroutines.push(wir::WorkshopSubroutine {
                name,
                index,
                span: Some(Span::new(self.file(), start, end)),
                name_span: None,
            });
            self.subroutines
                .insert(self.target.subroutines.get(id).unwrap().name.clone(), id);
        }
        self.expect(TokenKind::RBrace, "expected '}' after subroutines")?;
        Ok(())
    }

    fn rule(&mut self) -> Result<()> {
        self.expect_word("rule")?;
        self.expect(TokenKind::LParen, "expected '(' after 'rule'")?;
        let name = self.expect_string("expected a rule name string")?;
        self.expect(TokenKind::RParen, "expected ')' after rule name")?;
        let (rule_start, rule_end) = self.previous_span();
        self.expect(TokenKind::LBrace, "expected '{' after rule header")?;

        let mut rule = wir::Rule {
            name,
            span: Some(Span::new(self.file(), rule_start, rule_end)),
            name_span: None,
            disabled: false,
            event: Event::Global,
            conditions: Vec::new(),
            actions: Vec::new(),
        };
        let mut seen_sections = Vec::new();
        loop {
            match self.peek() {
                Some(Token {
                    kind: TokenKind::RBrace,
                    ..
                }) => {
                    self.pos += 1;
                    break;
                }
                Some(Token {
                    kind: TokenKind::Word(word),
                    ..
                }) => match word.as_str() {
                    "event" => {
                        if seen_sections.contains(&"event") {
                            return Err(
                                self.malformed("duplicate 'event' section", &self.peek().unwrap())
                            );
                        }
                        seen_sections.push("event");
                        rule.event = self.event_section()?;
                    }
                    "conditions" => {
                        if seen_sections.contains(&"conditions") {
                            return Err(self.malformed(
                                "duplicate 'conditions' section",
                                &self.peek().unwrap(),
                            ));
                        }
                        seen_sections.push("conditions");
                        rule.conditions = self.conditions_section()?;
                    }
                    "actions" => {
                        if seen_sections.contains(&"actions") {
                            return Err(self
                                .malformed("duplicate 'actions' section", &self.peek().unwrap()));
                        }
                        seen_sections.push("actions");
                        rule.actions = self.actions_section()?;
                    }
                    _ => return Err(self.unknown("rule section", &word)),
                },
                Some(token) => return Err(self.malformed("expected a rule section or '}'", &token)),
                None => return Err(self.malformed("unexpected end of input in rule", self.eof())),
            }
        }
        self.target.rules.push(rule);
        Ok(())
    }

    fn event_section(&mut self) -> Result<Event> {
        self.expect_word("event")?;
        self.expect(TokenKind::LBrace, "expected '{' after 'event'")?;
        let mut lines: Vec<String> = Vec::new();
        loop {
            match self.peek() {
                Some(Token {
                    kind: TokenKind::RBrace,
                    ..
                }) => {
                    self.pos += 1;
                    break;
                }
                Some(Token {
                    kind: TokenKind::Semi,
                    ..
                }) => {
                    self.pos += 1;
                    lines.push(String::new());
                }
                Some(_) => {
                    let text = self.line_text()?;
                    lines.push(text);
                }
                None => return Err(self.malformed("unexpected end of input in event", self.eof())),
            }
        }
        let Some(name_line) = lines.first().cloned() else {
            return Err(self.malformed("event section is empty", self.previous()));
        };
        let name_line = name_line.trim();
        let entry = self
            .catalog
            .resolve(Kind::Event, &self.locale, name_line)
            .ok_or_else(|| WorkshopError::Unknown {
                kind: "event",
                spelling: name_line.to_string(),
                locale: self.locale.clone(),
                span: None,
            })?;
        match entry.id.as_str() {
            "global" => Ok(Event::Global),
            "eachPlayer" => {
                for sub in &lines[1..] {
                    let sub = sub.trim();
                    if !sub.is_empty() && sub != "All" {
                        return Err(WorkshopError::Unsupported {
                            message: format!("unsupported 'eachPlayer' event parameter '{sub}'"),
                            span: None,
                        });
                    }
                }
                Ok(Event::EachPlayer)
            }
            "subroutine" => {
                let Some(sub_name) = lines.get(1).map(|s| s.trim()) else {
                    return Err(self.malformed(
                        "subroutine event requires a subroutine name",
                        self.previous(),
                    ));
                };
                let id = self.subroutine_by_name(sub_name)?;
                Ok(Event::Subroutine(id))
            }
            other => Err(WorkshopError::Unsupported {
                message: format!("unsupported event '{other}'"),
                span: None,
            }),
        }
    }

    fn conditions_section(&mut self) -> Result<Vec<wir::ValueId>> {
        self.expect_word("conditions")?;
        self.expect(TokenKind::LBrace, "expected '{' after 'conditions'")?;
        let mut conditions = Vec::new();
        loop {
            match self.peek() {
                Some(Token {
                    kind: TokenKind::RBrace,
                    ..
                }) => {
                    self.pos += 1;
                    break;
                }
                Some(Token {
                    kind: TokenKind::Semi,
                    ..
                }) => {
                    self.pos += 1;
                }
                Some(_) => {
                    let condition = self.value()?;
                    self.expect(TokenKind::Semi, "expected ';' after condition")?;
                    conditions.push(condition);
                }
                None => {
                    return Err(self.malformed("unexpected end of input in conditions", self.eof()));
                }
            }
        }
        Ok(conditions)
    }

    fn actions_section(&mut self) -> Result<Vec<wir::ActionId>> {
        self.expect_word("actions")?;
        self.expect(TokenKind::LBrace, "expected '{' after 'actions'")?;
        let (actions, stop) = self.actions_until_end()?;
        if stop != Stop::SectionClosed {
            return Err(self.malformed(
                "'End'/'Else' outside an If/While/For group",
                self.previous(),
            ));
        }
        Ok(actions)
    }

    /// Parse actions until a structural `else`/`elseIf`/`end` terminator
    /// (not consumed; the token position is preserved) or the enclosing `}`
    /// (consumed). Returns where the parse stopped.
    fn actions_until_end(&mut self) -> Result<(Vec<wir::ActionId>, Stop)> {
        let mut actions = Vec::new();
        loop {
            match self.peek() {
                Some(Token {
                    kind: TokenKind::RBrace,
                    ..
                }) => {
                    self.pos += 1;
                    return Ok((actions, Stop::SectionClosed));
                }
                Some(Token {
                    kind: TokenKind::Word(_),
                    ..
                }) => {
                    let saved = self.pos;
                    let (phrase, start, end) = self.phrase()?;
                    match phrase.as_str() {
                        "End" => {
                            self.pos = saved;
                            return Ok((actions, Stop::End));
                        }
                        "Else If" => {
                            self.pos = saved;
                            return Ok((actions, Stop::ElseIf));
                        }
                        "Else" => {
                            self.pos = saved;
                            return Ok((actions, Stop::Else));
                        }
                        "If" => actions.push(self.if_group()?),
                        "For Global Variable" => actions.push(self.for_group()?),
                        "While" => actions.push(self.while_group()?),
                        _ => actions.push(self.action_call_from_phrase(phrase, start, end)?),
                    }
                }
                Some(token) => return Err(self.malformed("expected an action", &token)),
                None => {
                    return Err(self.malformed("unexpected end of input in actions", self.eof()));
                }
            }
        }
    }

    fn if_group(&mut self) -> Result<wir::ActionId> {
        let start = self.previous_span().0;
        self.expect(TokenKind::LParen, "expected '(' after 'If'")?;
        let condition = self.value()?;
        self.expect(TokenKind::RParen, "expected ')' after If condition")?;
        self.expect(TokenKind::Semi, "expected ';' after If condition")?;

        let mut branches = Vec::new();
        let mut stop = {
            let (body, stop) = self.actions_until_end()?;
            branches.push(wir::IfBranch { condition, body });
            stop
        };

        let mut else_body = None;
        loop {
            match stop {
                Stop::End => {
                    self.consume_phrase("End")?;
                    self.expect(TokenKind::Semi, "expected ';' after 'End'")?;
                    break;
                }
                Stop::ElseIf => {
                    self.consume_phrase("Else If")?;
                    self.expect(TokenKind::LParen, "expected '(' after 'Else If'")?;
                    let condition = self.value()?;
                    self.expect(TokenKind::RParen, "expected ')' after Else If condition")?;
                    self.expect(TokenKind::Semi, "expected ';' after Else If condition")?;
                    let (body, next) = self.actions_until_end()?;
                    branches.push(wir::IfBranch { condition, body });
                    stop = next;
                }
                Stop::Else => {
                    self.consume_phrase("Else")?;
                    self.expect(TokenKind::Semi, "expected ';' after 'Else'")?;
                    let (body, next) = self.actions_until_end()?;
                    else_body = Some(body);
                    stop = next;
                }
                Stop::SectionClosed => {
                    return Err(self.malformed("'If' requires a matching 'End'", self.previous()));
                }
            }
        }
        let end_span = self.previous_span();
        let action = Action::If {
            branches,
            else_body,
            span: Some(Span::new(self.file(), start, end_span.1)),
        };
        Ok(self.target.actions.push(action))
    }

    fn for_group(&mut self) -> Result<wir::ActionId> {
        let start = self.previous_span().0;
        self.expect(
            TokenKind::LParen,
            "expected '(' after 'For Global Variable'",
        )?;
        let (name, _, _) = self.phrase()?;
        let variable = self.global_by_name(&name)?;
        self.expect(TokenKind::Comma, "expected ',' after loop variable")?;
        let start_value = self.value()?;
        self.expect(TokenKind::Comma, "expected ',' after start")?;
        let stop = self.value()?;
        self.expect(TokenKind::Comma, "expected ',' after stop")?;
        let step = self.value()?;
        self.expect(TokenKind::RParen, "expected ')' after For bounds")?;
        self.expect(TokenKind::Semi, "expected ';' after For Global Variable")?;
        let (body, loop_stop) = self.actions_until_end()?;
        if loop_stop != Stop::End {
            return Err(self.malformed(
                "'For Global Variable' requires a matching 'End'",
                self.previous(),
            ));
        }
        self.consume_phrase("End")?;
        self.expect(TokenKind::Semi, "expected ';' after 'End'")?;
        let end_span = self.previous_span();
        let action = Action::ForGlobalVariable {
            variable,
            start: start_value,
            stop,
            step,
            body,
            span: Some(Span::new(self.file(), start, end_span.1)),
            target_span: None,
        };
        Ok(self.target.actions.push(action))
    }

    fn while_group(&mut self) -> Result<wir::ActionId> {
        let start = self.previous_span().0;
        self.expect(TokenKind::LParen, "expected '(' after 'While'")?;
        let condition = self.value()?;
        self.expect(TokenKind::RParen, "expected ')' after While condition")?;
        self.expect(TokenKind::Semi, "expected ';' after While condition")?;
        let (body, stop) = self.actions_until_end()?;
        if stop != Stop::End {
            return Err(self.malformed("'While' requires a matching 'End'", self.previous()));
        }
        self.consume_phrase("End")?;
        self.expect(TokenKind::Semi, "expected ';' after 'End'")?;
        let end_span = self.previous_span();
        let action = Action::While {
            condition,
            body,
            span: Some(Span::new(self.file(), start, end_span.1)),
        };
        Ok(self.target.actions.push(action))
    }

    fn action_call_from_phrase(
        &mut self,
        phrase: String,
        start: Position,
        end: Position,
    ) -> Result<wir::ActionId> {
        match self
            .catalog
            .resolve(Kind::Structural, &self.locale, &phrase)
        {
            Some(entry) => match entry.id.as_str() {
                "setGlobalVariable" => {
                    self.expect(
                        TokenKind::LParen,
                        "expected '(' after 'Set Global Variable'",
                    )?;
                    let (name, _, _) = self.phrase()?;
                    let variable = self.global_by_name(&name)?;
                    self.expect(TokenKind::Comma, "expected ',' after variable")?;
                    let value = self.value()?;
                    self.expect(TokenKind::RParen, "expected ')'")?;
                    self.expect(TokenKind::Semi, "expected ';'")?;
                    Ok(self.target.actions.push(Action::SetGlobalVariable {
                        variable,
                        value,
                        span: Some(Span::new(self.file(), start, end)),
                        target_span: None,
                    }))
                }
                "modifyGlobalVariable" => {
                    self.expect(TokenKind::LParen, "expected '('")?;
                    let (name, _, _) = self.phrase()?;
                    let variable = self.global_by_name(&name)?;
                    self.expect(TokenKind::Comma, "expected ',' after variable")?;
                    let op = self.modify_op()?;
                    self.expect(TokenKind::Comma, "expected ',' after modify operator")?;
                    let value = self.value()?;
                    self.expect(TokenKind::RParen, "expected ')'")?;
                    self.expect(TokenKind::Semi, "expected ';'")?;
                    Ok(self.target.actions.push(Action::ModifyGlobalVariable {
                        variable,
                        op,
                        value,
                        span: Some(Span::new(self.file(), start, end)),
                        target_span: None,
                    }))
                }
                "setPlayerVariable" => {
                    self.expect(TokenKind::LParen, "expected '('")?;
                    let player = self.value()?;
                    self.expect(TokenKind::Comma, "expected ',' after player")?;
                    let (name, _, _) = self.phrase()?;
                    let variable = self.player_by_name(&name)?;
                    self.expect(TokenKind::Comma, "expected ',' after variable")?;
                    let value = self.value()?;
                    self.expect(TokenKind::RParen, "expected ')'")?;
                    self.expect(TokenKind::Semi, "expected ';'")?;
                    Ok(self.target.actions.push(Action::SetPlayerVariable {
                        player,
                        variable,
                        value,
                        span: Some(Span::new(self.file(), start, end)),
                        target_span: None,
                    }))
                }
                "modifyPlayerVariable" => {
                    self.expect(TokenKind::LParen, "expected '('")?;
                    let player = self.value()?;
                    self.expect(TokenKind::Comma, "expected ',' after player")?;
                    let (name, _, _) = self.phrase()?;
                    let variable = self.player_by_name(&name)?;
                    self.expect(TokenKind::Comma, "expected ',' after variable")?;
                    let op = self.modify_op()?;
                    self.expect(TokenKind::Comma, "expected ',' after modify operator")?;
                    let value = self.value()?;
                    self.expect(TokenKind::RParen, "expected ')'")?;
                    self.expect(TokenKind::Semi, "expected ';'")?;
                    Ok(self.target.actions.push(Action::ModifyPlayerVariable {
                        player,
                        variable,
                        op,
                        value,
                        span: Some(Span::new(self.file(), start, end)),
                        target_span: None,
                    }))
                }
                "callSubroutine" => {
                    self.expect(TokenKind::LParen, "expected '('")?;
                    let (name, _, _) = self.phrase()?;
                    let subroutine = self.subroutine_by_name(&name)?;
                    self.expect(TokenKind::RParen, "expected ')'")?;
                    self.expect(TokenKind::Semi, "expected ';'")?;
                    Ok(self.target.actions.push(Action::CallSubroutine {
                        subroutine,
                        span: Some(Span::new(self.file(), start, end)),
                        callee_span: None,
                    }))
                }
                other => Err(WorkshopError::Unsupported {
                    message: format!(
                        "structural action '{other}' is not supported in action position"
                    ),
                    span: Some(Span::new(self.file(), start, end)),
                }),
            },
            None => {
                // Generic action call; the argument list is optional.
                let action = self
                    .catalog
                    .resolve(Kind::Action, &self.locale, &phrase)
                    .ok_or_else(|| WorkshopError::Unknown {
                        kind: "action",
                        spelling: phrase.clone(),
                        locale: self.locale.clone(),
                        span: Some(Span::new(self.file(), start, end)),
                    })?;
                let args = if let Some(Token {
                    kind: TokenKind::LParen,
                    ..
                }) = self.peek()
                {
                    self.pos += 1;
                    let args = self.value_args()?;
                    self.expect(TokenKind::RParen, "expected ')'")?;
                    args
                } else {
                    Vec::new()
                };
                self.expect(TokenKind::Semi, "expected ';' after action")?;
                Ok(self.target.actions.push(Action::Call {
                    name: action.id.clone(),
                    args,
                    span: Some(Span::new(self.file(), start, end)),
                }))
            }
        }
    }

    fn modify_op(&mut self) -> Result<ModifyOp> {
        let (phrase, start, end) = self.phrase()?;
        let entry = self
            .catalog
            .resolve(Kind::Operator, &self.locale, &phrase)
            .ok_or_else(|| WorkshopError::Unknown {
                kind: "modify operator",
                spelling: phrase.clone(),
                locale: self.locale.clone(),
                span: Some(Span::new(self.file(), start, end)),
            })?;
        let op = match entry.id.as_str() {
            "add" => ModifyOp::Add,
            "subtract" => ModifyOp::Subtract,
            "multiply" => ModifyOp::Multiply,
            "divide" => ModifyOp::Divide,
            "modulo" => ModifyOp::Modulo,
            "raiseToPower" => ModifyOp::RaiseToPower,
            "appendToArray" => ModifyOp::AppendToArray,
            "removeFromArray" => ModifyOp::RemoveFromArray,
            other => {
                return Err(WorkshopError::Unsupported {
                    message: format!("unsupported modify operator '{other}'"),
                    span: Some(Span::new(self.file(), start, end)),
                });
            }
        };
        Ok(op)
    }

    fn value(&mut self) -> Result<wir::ValueId> {
        let primary = self.primary()?;
        if let Some(Token {
            kind: TokenKind::Op(op),
            start,
            end,
        }) = self.peek()
        {
            if is_comparison(&op) {
                self.pos += 1;
                let right = self.primary()?;
                let span = Some(Span::new(self.file(), start, end));
                let call = ValueNode::new(
                    Value::Call {
                        name: op,
                        args: vec![primary, right],
                    },
                    span,
                );
                return Ok(self.target.values.push(call));
            }
        }
        Ok(primary)
    }

    fn primary(&mut self) -> Result<wir::ValueId> {
        match self.peek() {
            Some(Token {
                kind: TokenKind::Number { value, text },
                start,
                end,
            }) => {
                let span = Some(Span::new(self.file(), start, end));
                self.pos += 1;
                Ok(self
                    .target
                    .values
                    .push(ValueNode::new(Value::Number { value, text }, span)))
            }
            Some(Token {
                kind: TokenKind::Op(op),
                start,
                ..
            }) if op == "-" => {
                if let Some(Token {
                    kind: TokenKind::Number { value, text },
                    end: number_end,
                    ..
                }) = self.peek_at(1)
                {
                    let span = Some(Span::new(self.file(), start, number_end));
                    self.pos += 2;
                    Ok(self.target.values.push(ValueNode::new(
                        Value::Number {
                            value: -value,
                            text: format!("-{text}"),
                        },
                        span,
                    )))
                } else {
                    Err(self.malformed("expected a number after '-'", &self.peek().unwrap()))
                }
            }
            Some(Token {
                kind: TokenKind::String(content),
                start,
                end,
            }) => {
                let span = Some(Span::new(self.file(), start, end));
                self.pos += 1;
                Ok(self
                    .target
                    .values
                    .push(ValueNode::new(Value::String(content), span)))
            }
            Some(Token {
                kind: TokenKind::Word(word),
                ..
            }) if word == "Global" => {
                let (start, end) = self.span_here();
                self.pos += 1;
                self.expect(TokenKind::Dot, "expected '.' after 'Global'")?;
                let (name, _, _) = self.phrase()?;
                let variable = self.global_by_name(&name)?;
                let span = Some(Span::new(self.file(), start, end));
                Ok(self
                    .target
                    .values
                    .push(ValueNode::new(Value::GlobalVariable(variable), span)))
            }
            _ => {
                let (phrase, start, end) = self.phrase()?;
                match phrase.as_str() {
                    "True" => Ok(self.push_bool(true, start, end)),
                    "False" => Ok(self.push_bool(false, start, end)),
                    "Event Player" => Ok(self.target.values.push(ValueNode::new(
                        Value::EventPlayer,
                        Some(Span::new(self.file(), start, end)),
                    ))),
                    "Null" => Ok(self.target.values.push(ValueNode::new(
                        Value::Null,
                        Some(Span::new(self.file(), start, end)),
                    ))),
                    _ => {
                        if let Some(Token {
                            kind: TokenKind::LParen,
                            ..
                        }) = self.peek()
                        {
                            self.call_or_enum(&phrase, start, end)
                        } else {
                            self.bare_member(&phrase, start, end)
                        }
                    }
                }
            }
        }
    }

    fn call_or_enum(
        &mut self,
        phrase: &str,
        start: Position,
        end: Position,
    ) -> Result<wir::ValueId> {
        // A value function wins over an enum domain of the same spelling
        // (e.g. `Vector(x, y, z)` is the value function; `Vector` as an enum
        // domain only appears through bare members like `Up`).
        if let Some(entry) = self.catalog.resolve(Kind::Value, &self.locale, phrase) {
            self.expect(TokenKind::LParen, "expected '(' after value name")?;
            if entry.id == "compare" {
                // Compare(a, op, b) -> Call(op, [a, b]).
                let left = self.value()?;
                self.expect(TokenKind::Comma, "expected ',' after Compare operand")?;
                let (op, op_start, op_end) = match self.next() {
                    Some(Token {
                        kind: TokenKind::Op(op),
                        start,
                        end,
                    }) => (op, start, end),
                    Some(token) => {
                        return Err(
                            self.malformed("expected a comparison operator in Compare", &token)
                        );
                    }
                    None => {
                        return Err(
                            self.malformed("expected a comparison operator in Compare", self.eof())
                        );
                    }
                };
                self.expect(TokenKind::Comma, "expected ',' after Compare operator")?;
                let right = self.value()?;
                self.expect(TokenKind::RParen, "expected ')'")?;
                let span = Some(Span::new(self.file(), op_start, op_end));
                return Ok(self.target.values.push(ValueNode::new(
                    Value::Call {
                        name: op,
                        args: vec![left, right],
                    },
                    span,
                )));
            }
            let args = self.value_args()?;
            self.expect(TokenKind::RParen, "expected ')'")?;
            return Ok(self.target.values.push(ValueNode::new(
                Value::Call {
                    name: entry.id.clone(),
                    args,
                },
                Some(Span::new(self.file(), start, end)),
            )));
        }
        if let Some(domain) = self.catalog.enum_domain(phrase) {
            // Enum call: `Color(Yellow)`.
            self.expect(TokenKind::LParen, "expected '('")?;
            let (member_phrase, m_start, m_end) = self.phrase()?;
            let member = self
                .catalog
                .resolve_enum_member(&domain.domain, &self.locale, &member_phrase)
                .ok_or_else(|| WorkshopError::Unknown {
                    kind: "enum member",
                    spelling: member_phrase.clone(),
                    locale: self.locale.clone(),
                    span: Some(Span::new(self.file(), m_start, m_end)),
                })?;
            self.expect(TokenKind::RParen, "expected ')' after enum member")?;
            return Ok(self.target.values.push(ValueNode::new(
                Value::Enum {
                    value_type: member.0,
                    value: member.1,
                },
                Some(Span::new(self.file(), start, end)),
            )));
        }
        Err(WorkshopError::Unknown {
            kind: "value",
            spelling: phrase.to_string(),
            locale: self.locale.clone(),
            span: Some(Span::new(self.file(), start, end)),
        })
    }

    fn bare_member(
        &mut self,
        phrase: &str,
        start: Position,
        end: Position,
    ) -> Result<wir::ValueId> {
        let matches: Vec<(String, String)> = self.catalog.bare_member_matches(&self.locale, phrase);
        if matches.len() == 1 {
            return Ok(self.target.values.push(ValueNode::new(
                Value::Enum {
                    value_type: matches[0].0.clone(),
                    value: matches[0].1.clone(),
                },
                Some(Span::new(self.file(), start, end)),
            )));
        }
        if matches.len() > 1 {
            return Err(WorkshopError::Unsupported {
                message: format!("ambiguous enum member '{phrase}' (multiple domains match)"),
                span: Some(Span::new(self.file(), start, end)),
            });
        }
        // A bare value constant (e.g. Empty Array).
        if let Some(entry) = self.catalog.resolve(Kind::Value, &self.locale, phrase) {
            return Ok(self.target.values.push(ValueNode::new(
                Value::Call {
                    name: entry.id.clone(),
                    args: Vec::new(),
                },
                Some(Span::new(self.file(), start, end)),
            )));
        }
        Err(WorkshopError::Unknown {
            kind: "value",
            spelling: phrase.to_string(),
            locale: self.locale.clone(),
            span: Some(Span::new(self.file(), start, end)),
        })
    }

    fn value_args(&mut self) -> Result<Vec<wir::ValueId>> {
        let mut args = Vec::new();
        if let Some(Token {
            kind: TokenKind::RParen,
            ..
        }) = self.peek()
        {
            return Ok(args);
        }
        loop {
            args.push(self.value()?);
            match self.peek() {
                Some(Token {
                    kind: TokenKind::Comma,
                    ..
                }) => {
                    self.pos += 1;
                }
                _ => break,
            }
        }
        Ok(args)
    }

    fn push_bool(&mut self, value: bool, start: Position, end: Position) -> wir::ValueId {
        self.target.values.push(ValueNode::new(
            Value::Bool(value),
            Some(Span::new(self.file(), start, end)),
        ))
    }

    fn global_by_name(&self, name: &str) -> Result<wir::GlobalVarId> {
        self.globals
            .get(name)
            .copied()
            .ok_or_else(|| WorkshopError::Unknown {
                kind: "global variable",
                spelling: name.to_string(),
                locale: self.locale.clone(),
                span: None,
            })
    }

    fn player_by_name(&self, name: &str) -> Result<wir::PlayerVarId> {
        self.players
            .get(name)
            .copied()
            .ok_or_else(|| WorkshopError::Unknown {
                kind: "player variable",
                spelling: name.to_string(),
                locale: self.locale.clone(),
                span: None,
            })
    }

    fn subroutine_by_name(&self, name: &str) -> Result<wir::SubroutineId> {
        self.subroutines
            .get(name)
            .copied()
            .ok_or_else(|| WorkshopError::Unknown {
                kind: "subroutine",
                spelling: name.to_string(),
                locale: self.locale.clone(),
                span: None,
            })
    }

    /// Read the maximal phrase of consecutive words (space-joined). Phrases
    /// may span lines because long Workshop action arguments wrap mid-phrase.
    fn phrase(&mut self) -> Result<(String, Position, Position)> {
        let mut words = Vec::new();
        let (start, mut end) = match self.peek() {
            Some(Token {
                kind: TokenKind::Word(word),
                start,
                end,
            }) => {
                words.push(word.clone());
                (start, end)
            }
            Some(token) => return Err(self.malformed("expected an identifier", &token)),
            None => return Err(self.malformed("expected an identifier", self.eof())),
        };
        self.pos += 1;
        while let Some(Token {
            kind: TokenKind::Word(word),
            end: word_end,
            ..
        }) = self.peek()
        {
            words.push(word);
            end = word_end;
            self.pos += 1;
        }
        Ok((words.join(" "), start, end))
    }

    /// Read a single-line phrase (stops at a line boundary). Used for names
    /// that are structurally one per line, such as variable declarations.
    fn phrase_on_line(&mut self) -> Result<(String, Position, Position)> {
        let mut words = Vec::new();
        let (start, mut end, line) = match self.peek() {
            Some(Token {
                kind: TokenKind::Word(word),
                start,
                end,
            }) => {
                words.push(word.clone());
                (start, end, start.line)
            }
            Some(token) => return Err(self.malformed("expected an identifier", &token)),
            None => return Err(self.malformed("expected an identifier", self.eof())),
        };
        self.pos += 1;
        while let Some(Token {
            kind: TokenKind::Word(word),
            start,
            end: word_end,
        }) = self.peek()
        {
            if start.line != line {
                break;
            }
            words.push(word);
            end = word_end;
            self.pos += 1;
        }
        Ok((words.join(" "), start, end))
    }

    /// Read a text line (tokens until `;`), joining words and dashes into
    /// the literal text, and consume the terminating `;`.
    fn line_text(&mut self) -> Result<String> {
        let mut parts = Vec::new();
        loop {
            match self.peek() {
                Some(Token {
                    kind: TokenKind::Semi,
                    ..
                }) => {
                    self.pos += 1;
                    break;
                }
                Some(Token {
                    kind: TokenKind::Word(word),
                    ..
                }) => {
                    parts.push(word.clone());
                    self.pos += 1;
                }
                Some(Token {
                    kind: TokenKind::Op(op),
                    ..
                }) if op == "-" => {
                    parts.push("-".to_string());
                    self.pos += 1;
                }
                Some(Token {
                    kind: TokenKind::Number { value, .. },
                    ..
                }) => {
                    parts.push(value.to_string());
                    self.pos += 1;
                }
                Some(token) => return Err(self.malformed("expected a text line", &token)),
                None => return Err(self.malformed("unexpected end of input in line", self.eof())),
            }
        }
        Ok(parts.join(" "))
    }

    /// Consume a known keyword phrase, verifying its spelling.
    fn consume_phrase(&mut self, expected: &str) -> Result<()> {
        let (phrase, _, _) = self.phrase()?;
        if phrase != expected {
            return Err(self.malformed(&format!("expected '{expected}'"), self.previous()));
        }
        Ok(())
    }

    fn expect_word(&mut self, expected: &str) -> Result<Position> {
        match self.next() {
            Some(Token {
                kind: TokenKind::Word(word),
                start,
                ..
            }) if word == expected => Ok(start),
            Some(token) => Err(self.malformed(&format!("expected '{expected}'"), &token)),
            None => Err(self.malformed(&format!("expected '{expected}'"), self.eof())),
        }
    }

    fn expect(&mut self, kind: TokenKind, message: &str) -> Result<()> {
        match self.next() {
            Some(token) if token.kind == kind => Ok(()),
            Some(token) => Err(self.malformed(message, &token)),
            None => Err(self.malformed(message, self.eof())),
        }
    }

    fn expect_string(&mut self, message: &str) -> Result<String> {
        match self.next() {
            Some(Token {
                kind: TokenKind::String(content),
                ..
            }) => Ok(content),
            Some(token) => Err(self.malformed(message, &token)),
            None => Err(self.malformed(message, self.eof())),
        }
    }

    fn malformed(&self, message: &str, token: &Token) -> WorkshopError {
        WorkshopError::Malformed {
            message: message.to_string(),
            span: Some(Span::new(self.file(), token.start, token.end)),
        }
    }

    fn unknown(&self, kind: &'static str, spelling: &str) -> WorkshopError {
        WorkshopError::Unknown {
            kind,
            spelling: spelling.to_string(),
            locale: self.locale.clone(),
            span: None,
        }
    }

    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.pos).cloned()
    }

    fn peek_at(&self, offset: usize) -> Option<Token> {
        self.tokens.get(self.pos + offset).cloned()
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn previous(&self) -> &Token {
        self.tokens
            .get(self.pos.saturating_sub(1))
            .unwrap_or_else(|| self.tokens.last().unwrap())
    }

    fn previous_span(&self) -> (Position, Position) {
        let token = self.previous();
        (token.start, token.end)
    }

    fn span_here(&self) -> (Position, Position) {
        let token = self
            .peek()
            .unwrap_or_else(|| self.tokens.last().unwrap().clone());
        (token.start, token.end)
    }

    fn eof(&self) -> &Token {
        self.tokens.last().unwrap()
    }

    fn file(&self) -> wright_ir::ids::Id<SourceFile> {
        wright_ir::ids::Id::from_index(0)
    }
}

fn is_comparison(op: &str) -> bool {
    matches!(op, "==" | "!=" | "<" | "<=" | ">" | ">=")
}
