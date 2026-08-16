//! Conversion from the `wright/opy-hir` bridge protocol into the internal
//! Opy HIR model (`wright_ir::hir`).
//!
//! The protocol payload is expected to be already validated (see
//! [`super::parse_str`]); this conversion maps it onto the typed, arena-based
//! model, resolving name references to typed IDs and rejecting operators
//! outside the v0.1 surface.

use std::collections::HashMap;

use workshop_rs::source::{FileId, Position, SourceFile, Span};
use wright_ir::error::IrError;
use wright_ir::hir::{BinaryOp, UnaryOp};
use wright_ir::ids::Id;

use super::types::{self, Declaration, Expr, RuleEntry, Stmt, default_var_index};
use crate::hir::Program as ProtocolProgram;

/// Convert a validated protocol `Program` into the internal Opy HIR model.
pub fn convert(program: &ProtocolProgram) -> Result<wright_ir::hir::Program, IrError> {
    Builder::new(program).build()
}

impl ProtocolProgram {
    /// Convert this validated protocol payload into the internal Opy HIR
    /// model.
    pub fn to_ir(&self) -> Result<wright_ir::hir::Program, IrError> {
        convert(self)
    }
}

struct Builder<'a> {
    protocol: &'a ProtocolProgram,
    target: wright_ir::hir::Program,
    files: HashMap<u32, FileId>,
    globals: HashMap<String, wright_ir::hir::GlobalVarId>,
    players: HashMap<String, wright_ir::hir::PlayerVarId>,
    subroutines: HashMap<String, wright_ir::hir::SubroutineId>,
    constants: HashMap<String, wright_ir::hir::ConstantId>,
    macros: HashMap<String, wright_ir::hir::MacroId>,
}

impl<'a> Builder<'a> {
    fn new(protocol: &'a ProtocolProgram) -> Self {
        Builder {
            protocol,
            target: wright_ir::hir::Program::default(),
            files: HashMap::new(),
            globals: HashMap::new(),
            players: HashMap::new(),
            subroutines: HashMap::new(),
            constants: HashMap::new(),
            macros: HashMap::new(),
        }
    }

    fn build(mut self) -> Result<wright_ir::hir::Program, IrError> {
        // Phase A: file registry.
        for file in &self.protocol.files {
            let id = self.target.files.push(SourceFile::new(file.path.clone()));
            self.files.insert(file.id, id);
        }

        // Phase A: symbols with empty bodies; maps are populated before any
        // body conversion so references resolve regardless of order.
        for declaration in &self.protocol.declarations {
            match declaration {
                Declaration::GlobalVariable {
                    name,
                    index,
                    span,
                    name_span,
                    ..
                } => {
                    let span = self.span(*span)?;
                    let name_span = self.span(*name_span)?;
                    let id = self.target.globals.push(wright_ir::hir::GlobalVar {
                        name: name.clone(),
                        index: *index,
                        span,
                        name_span,
                        initializer: None,
                    });
                    self.globals.insert(name.clone(), id);
                }
                Declaration::PlayerVariable {
                    name,
                    index,
                    span,
                    name_span,
                    ..
                } => {
                    let span = self.span(*span)?;
                    let name_span = self.span(*name_span)?;
                    let id = self.target.players.push(wright_ir::hir::PlayerVar {
                        name: name.clone(),
                        index: *index,
                        span,
                        name_span,
                        initializer: None,
                    });
                    self.players.insert(name.clone(), id);
                }
                Declaration::Subroutine {
                    name,
                    index,
                    span,
                    name_span,
                    ..
                } => {
                    let span = self.span(*span)?;
                    let name_span = self.span(*name_span)?;
                    let id = self.target.subroutines.push(wright_ir::hir::Subroutine {
                        name: name.clone(),
                        index: *index,
                        decl_span: span,
                        decl_name_span: name_span,
                        body: None,
                    });
                    self.subroutines.insert(name.clone(), id);
                }
                Declaration::Constant { name, span, .. } => {
                    let span = self.span(*span)?;
                    let id = self.target.constants.push(wright_ir::hir::Constant {
                        name: name.clone(),
                        span,
                        value: Id::from_index(0), // filled in phase C
                    });
                    self.constants.insert(name.clone(), id);
                }
                Declaration::Macro {
                    name, args, span, ..
                } => {
                    let span = self.span(*span)?;
                    let id = self.target.macros.push(wright_ir::hir::Macro {
                        name: name.clone(),
                        args: args.clone(),
                        span,
                        body: Vec::new(), // filled in phase C
                    });
                    self.macros.insert(name.clone(), id);
                }
            }
        }

        // Phase B: merge subroutine definitions into the subroutine table.
        for entry in &self.protocol.rules {
            if let RuleEntry::SubroutineDef {
                name,
                span,
                name_span,
                body,
                ..
            } = entry
            {
                let id = match self.subroutines.get(name) {
                    Some(id) => *id,
                    None => {
                        let id = self.target.subroutines.push(wright_ir::hir::Subroutine {
                            name: name.clone(),
                            index: None,
                            decl_span: None,
                            decl_name_span: None,
                            body: None,
                        });
                        self.subroutines.insert(name.clone(), id);
                        id
                    }
                };
                let statements = self.convert_stmts(body)?;
                let span = self.span(*span)?;
                let name_span = self.span(*name_span)?;
                let subroutine = self
                    .target
                    .subroutines
                    .get_mut(id)
                    .expect("subroutine created above");
                subroutine.body = Some(wright_ir::hir::SubroutineBody {
                    span,
                    name_span,
                    statements,
                });
            }
        }

        // Phase C: initializers, constant values, and macro bodies.
        for declaration in &self.protocol.declarations {
            match declaration {
                Declaration::GlobalVariable {
                    name, initializer, ..
                } => {
                    if let Some(initializer) = initializer {
                        let converted = self.convert_expr(initializer)?;
                        self.target
                            .globals
                            .get_mut(self.globals[name])
                            .expect("global created above")
                            .initializer = Some(converted);
                    }
                }
                Declaration::PlayerVariable {
                    name, initializer, ..
                } => {
                    if let Some(initializer) = initializer {
                        let converted = self.convert_expr(initializer)?;
                        self.target
                            .players
                            .get_mut(self.players[name])
                            .expect("player created above")
                            .initializer = Some(converted);
                    }
                }
                Declaration::Constant { name, value, .. } => {
                    let converted = self.convert_expr(value)?;
                    self.target
                        .constants
                        .get_mut(self.constants[name])
                        .expect("constant created above")
                        .value = converted;
                }
                Declaration::Macro { name, body, .. } => {
                    let converted = self.convert_stmts(body)?;
                    self.target
                        .macros
                        .get_mut(self.macros[name])
                        .expect("macro created above")
                        .body = converted;
                }
                Declaration::Subroutine { .. } => {}
            }
        }

        // Phase D: rules.
        for entry in &self.protocol.rules {
            if let RuleEntry::Rule(rule) = entry {
                let event = wright_ir::hir::Event {
                    name: rule.event.name.clone(),
                    args: rule
                        .event
                        .args
                        .iter()
                        .map(|arg| self.convert_expr(arg))
                        .collect::<Result<_, _>>()?,
                    span: self.span(rule.event.span)?,
                };
                let conditions = rule
                    .conditions
                    .iter()
                    .map(|condition| self.convert_expr(condition))
                    .collect::<Result<_, _>>()?;
                let actions = self.convert_stmts(&rule.actions)?;
                self.target.rules.push(wright_ir::hir::Rule {
                    name: rule.name.clone(),
                    span: self.span(rule.span)?,
                    name_span: self.span(rule.name_span)?,
                    disabled: rule.disabled,
                    event,
                    // The OPY protocol has no rule priority; OSTW sets it.
                    priority: None,
                    conditions,
                    actions,
                });
            }
        }

        // Phase E: the settings carrier (spans map through the registry).
        if let Some(settings) = &self.protocol.settings {
            let mut converted = Vec::with_capacity(settings.children.len());
            for child in &settings.children {
                converted.push(self.convert_settings_node(child)?);
            }
            self.target.settings = Some(workshop_rs::settings::Settings {
                span: self.span(settings.span)?,
                children: converted,
            });
        }

        Ok(self.target)
    }

    fn convert_settings_node(
        &self,
        node: &types::SettingsNode,
    ) -> Result<workshop_rs::settings::SettingsNode, IrError> {
        Ok(match node {
            types::SettingsNode::Group {
                name,
                children,
                span,
            } => workshop_rs::settings::SettingsNode::Group {
                name: name.clone(),
                children: children
                    .iter()
                    .map(|child| self.convert_settings_node(child))
                    .collect::<Result<_, _>>()?,
                span: self.span(*span)?,
            },
            types::SettingsNode::Number { name, value, span } => {
                workshop_rs::settings::SettingsNode::Number {
                    name: name.clone(),
                    value: *value,
                    span: self.span(*span)?,
                }
            }
            types::SettingsNode::Bool { name, value, span } => {
                workshop_rs::settings::SettingsNode::Bool {
                    name: name.clone(),
                    value: *value,
                    span: self.span(*span)?,
                }
            }
            types::SettingsNode::String { name, value, span } => {
                workshop_rs::settings::SettingsNode::String {
                    name: name.clone(),
                    value: value.clone(),
                    span: self.span(*span)?,
                }
            }
            types::SettingsNode::List {
                name,
                elements,
                span,
            } => workshop_rs::settings::SettingsNode::List {
                name: name.clone(),
                elements: elements
                    .iter()
                    .map(|element| {
                        Ok(workshop_rs::settings::SettingsListElement {
                            value: element.value.clone(),
                            span: self.span(element.span)?,
                        })
                    })
                    .collect::<Result<_, IrError>>()?,
                span: self.span(*span)?,
            },
        })
    }

    fn convert_stmts(
        &mut self,
        statements: &[Stmt],
    ) -> Result<Vec<wright_ir::hir::StmtId>, IrError> {
        statements
            .iter()
            .map(|statement| self.convert_stmt(statement))
            .collect()
    }

    /// Resolve a global-variable reference, auto-creating the internal
    /// declaration for an OverPy default variable name (`A`–`Z`, `AA`–…,
    /// `DX`) that the pinned reference accepts without a `globalvar`
    /// declaration (#114). The implicit declaration carries the name's fixed
    /// Workshop slot as its index and no source span, exactly as the
    /// reference models the implicit variable (no protocol declaration is
    /// added, so native/adapter HIR parity is preserved).
    fn global_var(
        &mut self,
        name: &str,
        span: Option<Span>,
    ) -> Result<wright_ir::hir::GlobalVarId, IrError> {
        if let Some(id) = self.globals.get(name) {
            return Ok(*id);
        }
        let Some(index) = default_var_index(name) else {
            return Err(unresolved(format!("global variable '{name}'"), span));
        };
        let id = self.target.globals.push(wright_ir::hir::GlobalVar {
            name: name.to_string(),
            index: Some(index),
            span: None,
            name_span: None,
            initializer: None,
        });
        self.globals.insert(name.to_string(), id);
        Ok(id)
    }

    fn convert_stmt(&mut self, statement: &Stmt) -> Result<wright_ir::hir::StmtId, IrError> {
        let span = self.span(statement.span().copied())?;
        let converted = match statement {
            Stmt::Expr { expr, .. } => {
                let expr = self.convert_expr(expr)?;
                wright_ir::hir::Stmt::Expr { expr, span }
            }
            Stmt::Assign { target, value, .. } => {
                let target = self.convert_expr(target)?;
                let value = self.convert_expr(value)?;
                wright_ir::hir::Stmt::Assign {
                    target,
                    value,
                    span,
                }
            }
            Stmt::If {
                branches, r#else, ..
            } => {
                let mut converted_branches = Vec::with_capacity(branches.len());
                for branch in branches {
                    converted_branches.push(wright_ir::hir::IfBranch {
                        condition: self.convert_expr(&branch.condition)?,
                        body: self.convert_stmts(&branch.body)?,
                    });
                }
                let else_body = match r#else {
                    Some(body) => Some(self.convert_stmts(body)?),
                    None => None,
                };
                wright_ir::hir::Stmt::If {
                    branches: converted_branches,
                    else_body,
                    span,
                }
            }
            Stmt::For {
                variable,
                iterable,
                body,
                ..
            } => {
                let variable_span = match variable.as_ref() {
                    Expr::GlobalVar { span, .. } => self.span(*span)?,
                    other => {
                        return Err(unresolved(
                            format!(
                                "for-loop variable must be a global variable, got '{}'",
                                other.kind_name()
                            ),
                            span,
                        ));
                    }
                };
                let variable = match variable.as_ref() {
                    Expr::GlobalVar { name, .. } => self.global_var(name, span)?,
                    other => {
                        return Err(unresolved(
                            format!(
                                "for-loop variable must be a global variable, got '{}'",
                                other.kind_name()
                            ),
                            span,
                        ));
                    }
                };
                let iterable = self.convert_expr(iterable)?;
                let body = self.convert_stmts(body)?;
                wright_ir::hir::Stmt::For {
                    variable,
                    iterable,
                    body,
                    span,
                    variable_span,
                }
            }
            Stmt::While {
                condition, body, ..
            } => {
                let condition = self.convert_expr(condition)?;
                let body = self.convert_stmts(body)?;
                wright_ir::hir::Stmt::While {
                    condition,
                    body,
                    span,
                }
            }
            Stmt::CallSubroutine { name, .. } => {
                let subroutine = *self
                    .subroutines
                    .get(name)
                    .ok_or_else(|| unresolved(format!("subroutine '{name}'"), span))?;
                // The callee identifier starts the call statement and runs for
                // exactly the name's character count (columns are char-based).
                let callee_span = span.map(|span| {
                    let end_col = span.start.col + name.chars().count() as u32;
                    workshop_rs::source::Span::new(
                        span.file,
                        span.start,
                        workshop_rs::source::Position::new(span.start.line, end_col),
                    )
                });
                wright_ir::hir::Stmt::CallSubroutine {
                    subroutine,
                    span,
                    callee_span,
                }
            }
            Stmt::Pass { .. } => wright_ir::hir::Stmt::Pass { span },
        };
        Ok(self.target.stmts.push(converted))
    }

    fn convert_expr(&mut self, expr: &Expr) -> Result<wright_ir::hir::ExprId, IrError> {
        let span = self.span(expr.span().copied())?;
        let converted = match expr {
            Expr::Number { value, text, .. } => wright_ir::hir::Expr::Number {
                value: *value,
                text: text.clone(),
                span,
            },
            Expr::String { value, .. } => wright_ir::hir::Expr::String {
                value: value.clone(),
                span,
            },
            Expr::Bool { value, .. } => wright_ir::hir::Expr::Bool {
                value: *value,
                span,
            },
            Expr::Null { .. } => wright_ir::hir::Expr::Null { span },
            Expr::Array { elements, .. } => wright_ir::hir::Expr::Array {
                elements: self.convert_exprs(elements)?,
                span,
            },
            Expr::Vector { x, y, z, .. } => wright_ir::hir::Expr::Vector {
                x: self.convert_expr(x)?,
                y: self.convert_expr(y)?,
                z: self.convert_expr(z)?,
                span,
            },
            Expr::Enum {
                value_type, value, ..
            } => wright_ir::hir::Expr::Enum {
                value_type: value_type.clone(),
                value: value.clone(),
                span,
            },
            Expr::GlobalVar { name, .. } => {
                let variable = self.global_var(name, span)?;
                wright_ir::hir::Expr::GlobalVar { variable, span }
            }
            Expr::PlayerVar { player, name, .. } => {
                let player = self.convert_expr(player)?;
                let variable = *self
                    .players
                    .get(name)
                    .ok_or_else(|| unresolved(format!("player variable '{name}'"), span))?;
                wright_ir::hir::Expr::PlayerVar {
                    player,
                    variable,
                    span,
                }
            }
            Expr::EventPlayer { .. } => wright_ir::hir::Expr::EventPlayer { span },
            Expr::Constant { name, .. } => {
                let constant = *self
                    .constants
                    .get(name)
                    .ok_or_else(|| unresolved(format!("constant '{name}'"), span))?;
                wright_ir::hir::Expr::Constant { constant, span }
            }
            Expr::Call { name, args, .. } => wright_ir::hir::Expr::Call {
                name: name.clone(),
                args: self.convert_exprs(args)?,
                span,
            },
            Expr::ReceiverCall {
                receiver,
                name,
                args,
                ..
            } => wright_ir::hir::Expr::ReceiverCall {
                receiver: self.convert_expr(receiver)?,
                name: name.clone(),
                args: self.convert_exprs(args)?,
                span,
            },
            Expr::MacroCall { name, args, .. } => {
                let macro_ = *self
                    .macros
                    .get(name)
                    .ok_or_else(|| unresolved(format!("macro '{name}'"), span))?;
                wright_ir::hir::Expr::MacroCall {
                    macro_,
                    args: self.convert_exprs(args)?,
                    span,
                }
            }
            Expr::MacroParam { name, .. } => wright_ir::hir::Expr::MacroParam {
                name: name.clone(),
                span,
            },
            Expr::Binary {
                op, left, right, ..
            } => {
                let op = BinaryOp::parse(op)
                    .ok_or_else(|| unsupported(format!("binary operator '{op}'"), span))?;
                let left = self.convert_expr(left)?;
                let right = self.convert_expr(right)?;
                wright_ir::hir::Expr::Binary {
                    op,
                    left,
                    right,
                    span,
                }
            }
            Expr::Unary { op, operand, .. } => {
                let op = UnaryOp::parse(op)
                    .ok_or_else(|| unsupported(format!("unary operator '{op}'"), span))?;
                let operand = self.convert_expr(operand)?;
                wright_ir::hir::Expr::Unary { op, operand, span }
            }
            Expr::Index { array, index, .. } => wright_ir::hir::Expr::Index {
                array: self.convert_expr(array)?,
                index: self.convert_expr(index)?,
                span,
            },
            Expr::Format { text, args, .. } => wright_ir::hir::Expr::Format {
                text: text.clone(),
                args: self.convert_exprs(args)?,
                span,
            },
        };
        Ok(self.target.exprs.push(converted))
    }

    fn convert_exprs(&mut self, exprs: &[Expr]) -> Result<Vec<wright_ir::hir::ExprId>, IrError> {
        exprs.iter().map(|expr| self.convert_expr(expr)).collect()
    }

    fn span(&self, span: Option<types::Span>) -> Result<Option<Span>, IrError> {
        let Some(span) = span else {
            return Ok(None);
        };
        let file = *self.files.get(&span.file).ok_or_else(|| IrError::Invalid {
            code: "invalid-span",
            message: format!("span references unknown file {}", span.file),
            span: Some(Span {
                file: Id::from_index(span.file as usize),
                start: Position::new(span.start.line, span.start.col),
                end: Position::new(span.end.line, span.end.col),
            }),
        })?;
        Ok(Some(Span {
            file,
            start: Position::new(span.start.line, span.start.col),
            end: Position::new(span.end.line, span.end.col),
        }))
    }
}

fn unresolved(message: String, span: Option<Span>) -> IrError {
    IrError::Invalid {
        code: "unresolved-reference",
        message,
        span,
    }
}

fn unsupported(message: String, span: Option<Span>) -> IrError {
    IrError::Unsupported { message, span }
}
