//! The HIR → Workshop IR lowering boundary (ADR-0006).
//!
//! [`lower`] maps a validated internal Opy HIR program onto the Workshop IR
//! model: variables and subroutines receive workshop indexes, rules keep
//! their events/conditions/actions, and statements/expressions become
//! workshop actions/values. Text emission and optimization are deliberately
//! out of scope; unsupported constructs are reported structurally with a
//! stable code and the offending span.
//!
//! Lowering decisions:
//!
//! * Variable, player-variable, and subroutine IDs map by table order, so the
//!   workshop tables are built in HIR order before any body is lowered.
//! * A subroutine definition (`def`) lowers to one workshop rule whose event
//!   is `Subroutine(id)`.
//! * `assign` lowers to `Set*Variable`; the compound-assignment pattern
//!   (`x = x + 1`, where the value is a binary op over the same variable)
//!   lowers to `Modify*Variable` with the matching modify operator.
//! * `.append(receiver, value)` lowers to a modify action with
//!   `AppendToArray`; other member calls lower to `Action::Call` with the
//!   receiver as the first argument.
//! * `debug(x)` and `print(s)` lower to `Action::Debug`/`Action::Print`.
//! * `for` loops lower to `ForGlobalVariable`; the iterable must be a `range`
//!   call (1-, 2-, or 3-argument forms).
//! * Source-level macro calls in value position are expanded inline: the
//!   macro body is lowered with its parameters bound to the call arguments.
//! * `pass` statements are dropped (they are no-ops by definition).
//! * Call/value names keep Wright's source-level function names; mapping
//!   them to Workshop presentation names is an emission concern.

use std::collections::HashMap;

use crate::error::{IrError, unsupported};
use crate::hir::{
    self, BinaryOp, Expr, ExprId, GlobalVarId, MacroId, PlayerVarId, Stmt, StmtId, SubroutineId,
};
use crate::source::{Position, SourceFile, Span};
use crate::wir::{self, Action, ModifyOp, Value, ValueId, ValueNode};

/// Lower an internal Opy HIR program into a Workshop IR program.
pub fn lower(program: &hir::Program) -> Result<wir::Program, IrError> {
    Lowerer::new(program).lower()
}

struct Lowerer<'a> {
    hir: &'a hir::Program,
    target: wir::Program,
    /// HIR global variable id → WIR global variable id (table order).
    globals: HashMap<GlobalVarId, wir::GlobalVarId>,
    /// HIR player variable id → WIR player variable id (table order).
    players: HashMap<PlayerVarId, wir::PlayerVarId>,
    /// HIR subroutine id → WIR subroutine id (table order).
    subroutines: HashMap<SubroutineId, wir::SubroutineId>,
}

impl<'a> Lowerer<'a> {
    fn new(hir: &'a hir::Program) -> Self {
        Lowerer {
            hir,
            target: wir::Program::default(),
            globals: HashMap::new(),
            players: HashMap::new(),
            subroutines: HashMap::new(),
        }
    }

    fn lower(mut self) -> Result<wir::Program, IrError> {
        // File registry: same order as the source HIR, so file IDs line up.
        for file in self.hir.files.iter() {
            self.target.files.push(SourceFile::new(file.path.clone()));
        }
        // The settings carrier is copied inertly (file ids align 1:1); the
        // settings tree is carried to emission, never lowered (#86).
        self.target.settings = self.hir.settings.clone();

        // Variable, player, and subroutine tables in HIR order.
        for id in range_ids::<hir::GlobalVar>(self.hir.globals.len()) {
            let (name, span, name_span, initializer, source_index) = {
                let global = self
                    .hir
                    .globals
                    .get(id)
                    .ok_or_else(|| dangling("global variable", id))?;
                (
                    global.name.clone(),
                    global.span,
                    global.name_span,
                    global.initializer,
                    global.index,
                )
            };
            let initializer = match initializer {
                Some(initializer) => Some(self.lower_value(initializer)?),
                None => None,
            };
            let wir_id = self.target.global_variables.push(wir::WorkshopVariable {
                name,
                index: source_index.unwrap_or(id.index() as u32),
                span,
                name_span,
                initializer,
            });
            self.globals.insert(id, wir_id);
        }
        for id in range_ids::<hir::PlayerVar>(self.hir.players.len()) {
            let (name, span, name_span, initializer, source_index) = {
                let player = self
                    .hir
                    .players
                    .get(id)
                    .ok_or_else(|| dangling("player variable", id))?;
                (
                    player.name.clone(),
                    player.span,
                    player.name_span,
                    player.initializer,
                    player.index,
                )
            };
            let initializer = match initializer {
                Some(initializer) => Some(self.lower_value(initializer)?),
                None => None,
            };
            let wir_id = self.target.player_variables.push(wir::WorkshopVariable {
                name,
                index: source_index.unwrap_or(id.index() as u32),
                span,
                name_span,
                initializer,
            });
            self.players.insert(id, wir_id);
        }
        for id in range_ids::<hir::Subroutine>(self.hir.subroutines.len()) {
            let (name, decl_span, decl_name_span) = {
                let subroutine = self
                    .hir
                    .subroutines
                    .get(id)
                    .ok_or_else(|| dangling("subroutine", id))?;
                (
                    subroutine.name.clone(),
                    subroutine.decl_span,
                    subroutine.decl_name_span,
                )
            };
            let wir_id = self.target.subroutines.push(wir::WorkshopSubroutine {
                name,
                index: id.index() as u32,
                span: decl_span,
                name_span: decl_name_span,
            });
            self.subroutines.insert(id, wir_id);
        }

        // Rules.
        // Subroutine definition bodies become rules with the Subroutine
        // event, emitted before normal rules (reference ordering).
        for id in range_ids::<hir::Subroutine>(self.hir.subroutines.len()) {
            let (name, body_span, body_name_span, statements) = {
                let subroutine = self
                    .hir
                    .subroutines
                    .get(id)
                    .ok_or_else(|| dangling("subroutine", id))?;
                let Some(body) = &subroutine.body else {
                    continue;
                };
                (
                    subroutine.name.clone(),
                    body.span,
                    body.name_span,
                    body.statements.clone(),
                )
            };
            let actions = self.lower_actions(&statements)?;
            self.target.rules.push(wir::Rule {
                name: format!("Subroutine {name}"),
                span: body_span,
                name_span: body_name_span,
                disabled: false,
                event: wir::Event::Subroutine(self.subroutines[&id]),
                conditions: Vec::new(),
                actions,
            });
        }
        let rule_ids = range_ids::<hir::Rule>(self.hir.rules.len());
        for id in rule_ids {
            self.lower_rule(id)?;
        }

        Ok(self.target)
    }

    fn lower_rule(&mut self, id: hir::RuleId) -> Result<(), IrError> {
        let rule = self
            .hir
            .rules
            .get(id)
            .ok_or_else(|| dangling("rule", id))?
            .clone();
        let event = match rule.event.name.as_str() {
            "global" if rule.event.args.is_empty() => wir::Event::Global,
            "eachPlayer" if rule.event.args.is_empty() => wir::Event::EachPlayer,
            name => {
                return Err(unsupported(
                    format!("event '{name}' is outside the v0.1 lowering surface"),
                    rule.event.span,
                ));
            }
        };
        let conditions = rule
            .conditions
            .iter()
            .map(|condition| self.lower_value(*condition))
            .collect::<Result<Vec<_>, _>>()?;
        let actions = self.lower_actions(&rule.actions)?;
        self.target.rules.push(wir::Rule {
            name: rule.name,
            span: rule.span,
            name_span: rule.name_span,
            disabled: rule.disabled,
            event,
            conditions,
            actions,
        });
        Ok(())
    }

    fn lower_actions(&mut self, statements: &[StmtId]) -> Result<Vec<wir::ActionId>, IrError> {
        let mut actions = Vec::new();
        for statement in statements {
            self.lower_action(*statement, &mut actions)?;
        }
        Ok(actions)
    }

    fn lower_action(&mut self, id: StmtId, out: &mut Vec<wir::ActionId>) -> Result<(), IrError> {
        let statement = self
            .hir
            .stmts
            .get(id)
            .ok_or_else(|| dangling("statement", id))?
            .clone();
        let span = statement.span();
        let action = match &statement {
            Stmt::Expr { expr, .. } => self.lower_expr_stmt(*expr, span)?,
            Stmt::Assign { target, value, .. } => self.lower_assign(target, value, span)?,
            Stmt::If {
                branches,
                else_body,
                ..
            } => {
                let mut lowered_branches = Vec::with_capacity(branches.len());
                for branch in branches {
                    lowered_branches.push(wir::IfBranch {
                        condition: self.lower_value(branch.condition)?,
                        body: self.lower_actions(&branch.body)?,
                    });
                }
                let lowered_else = match else_body {
                    Some(body) => Some(self.lower_actions(body)?),
                    None => None,
                };
                Action::If {
                    branches: lowered_branches,
                    else_body: lowered_else,
                    span,
                }
            }
            Stmt::For {
                variable,
                iterable,
                body,
                variable_span,
                ..
            } => self.lower_for(*variable, *iterable, body, span, *variable_span)?,
            Stmt::While {
                condition, body, ..
            } => Action::While {
                condition: self.lower_value(*condition)?,
                body: self.lower_actions(body)?,
                span,
            },
            Stmt::CallSubroutine {
                subroutine,
                callee_span,
                ..
            } => Action::CallSubroutine {
                subroutine: self.subroutine(*subroutine)?,
                span,
                callee_span: *callee_span,
            },
            Stmt::Pass { .. } => return Ok(()), // no-op; dropped
        };
        out.push(self.target.actions.push(action));
        Ok(())
    }

    fn lower_expr_stmt(&mut self, expr: ExprId, span: Option<Span>) -> Result<Action, IrError> {
        let expression = self
            .hir
            .exprs
            .get(expr)
            .ok_or_else(|| dangling("expression", expr))?;
        match expression {
            Expr::Call { name, args, .. } if name == "debug" && args.len() == 1 => {
                Ok(Action::Debug {
                    value: self.lower_value(args[0])?,
                    span,
                })
            }
            Expr::Call { name, args, .. } if name == "print" && args.len() == 1 => {
                Ok(Action::Print {
                    message: self.lower_value(args[0])?,
                    span,
                })
            }
            Expr::ReceiverCall {
                receiver,
                name,
                args,
                ..
            } if name == "append" => self.lower_append(receiver, args, span),
            Expr::Call { name, args, .. } => Ok(Action::Call {
                name: name.clone(),
                args: self.lower_values(args)?,
                span,
            }),
            Expr::ReceiverCall {
                receiver,
                name,
                args,
                ..
            } => {
                let mut lowered = vec![self.lower_value(*receiver)?];
                lowered.extend(self.lower_values(args)?);
                Ok(Action::Call {
                    name: name.clone(),
                    args: lowered,
                    span,
                })
            }
            other => Err(unsupported(
                format!(
                    "expression statement '{}' is outside the v0.1 lowering surface",
                    other.kind_name()
                ),
                span,
            )),
        }
    }

    fn lower_append(
        &mut self,
        receiver: &ExprId,
        args: &[ExprId],
        span: Option<Span>,
    ) -> Result<Action, IrError> {
        let value = match args {
            [value] => self.lower_value(*value)?,
            _ => {
                return Err(unsupported(
                    "append must take exactly one value for the v0.1 lowering surface",
                    span,
                ));
            }
        };
        let receiver_expr = self
            .hir
            .exprs
            .get(*receiver)
            .ok_or_else(|| dangling("expression", *receiver))?;
        match receiver_expr {
            Expr::GlobalVar { variable, .. } => Ok(Action::ModifyGlobalVariable {
                variable: self.global(*variable)?,
                op: ModifyOp::AppendToArray,
                value,
                span,
                target_span: self.occurrence_span(receiver_expr),
            }),
            Expr::PlayerVar {
                player, variable, ..
            } => {
                let player = self.lower_value(*player)?;
                Ok(Action::ModifyPlayerVariable {
                    player,
                    variable: self.player(*variable)?,
                    op: ModifyOp::AppendToArray,
                    value,
                    span,
                    target_span: self.occurrence_span(receiver_expr),
                })
            }
            other => Err(unsupported(
                format!(
                    "append on '{}' receiver is outside the v0.1 lowering surface",
                    other.kind_name()
                ),
                span,
            )),
        }
    }

    fn lower_assign(
        &mut self,
        target: &ExprId,
        value: &ExprId,
        span: Option<Span>,
    ) -> Result<Action, IrError> {
        let target_expr = self
            .hir
            .exprs
            .get(*target)
            .ok_or_else(|| dangling("expression", *target))?;
        match target_expr {
            Expr::GlobalVar { variable, .. } => {
                let global = self.global(*variable)?;
                let target_span = self.occurrence_span(target_expr);
                match self.global_modify(value, *variable)? {
                    Some((op, operand)) => Ok(Action::ModifyGlobalVariable {
                        variable: global,
                        op,
                        value: operand,
                        span,
                        target_span,
                    }),
                    None => Ok(Action::SetGlobalVariable {
                        variable: global,
                        value: self.lower_value(*value)?,
                        span,
                        target_span,
                    }),
                }
            }
            Expr::PlayerVar {
                player, variable, ..
            } => {
                let player_value = self.lower_value(*player)?;
                let player_id = *player;
                let player_var = self.player(*variable)?;
                let target_span = self.occurrence_span(target_expr);
                match self.player_modify(value, player_id, *variable)? {
                    Some((op, operand)) => Ok(Action::ModifyPlayerVariable {
                        player: player_value,
                        variable: player_var,
                        op,
                        value: operand,
                        span,
                        target_span,
                    }),
                    None => Ok(Action::SetPlayerVariable {
                        player: player_value,
                        variable: player_var,
                        value: self.lower_value(*value)?,
                        span,
                        target_span,
                    }),
                }
            }
            other => Err(unsupported(
                format!(
                    "assignment to '{}' target is outside the v0.1 lowering surface",
                    other.kind_name()
                ),
                span,
            )),
        }
    }

    /// The exact identifier occurrence of a variable-reference expression:
    /// the name token for a global variable, or the member name for a player
    /// variable (`receiver.name` — the member is the final token of the
    /// member expression). Derived where semantic identity is known, never by
    /// scanning source text for the spelling.
    fn occurrence_span(&self, expression: &hir::Expr) -> Option<Span> {
        match expression {
            hir::Expr::GlobalVar { span, .. } => *span,
            hir::Expr::PlayerVar { variable, span, .. } => span.map(|span| {
                let name_len = self
                    .hir
                    .players
                    .get(*variable)
                    .map(|player| player.name.chars().count() as u32)
                    .unwrap_or(0);
                Span::new(
                    span.file,
                    Position::new(
                        span.end.line,
                        span.end.col.saturating_sub(name_len).max(span.start.col),
                    ),
                    span.end,
                )
            }),
            _ => None,
        }
    }

    /// Detect `x = x <op> rhs` (or `x = rhs <op> x`) for a global target.
    fn global_modify(
        &mut self,
        value: &ExprId,
        target: GlobalVarId,
    ) -> Result<Option<(ModifyOp, ValueId)>, IrError> {
        self.modify_pattern(
            value,
            |other| matches!(other, Expr::GlobalVar { variable, .. } if *variable == target),
        )
    }

    /// Detect `x = x <op> rhs` (or `x = rhs <op> x`) for a player target.
    ///
    /// The target and the binary's operand are distinct HIR nodes (the
    /// frontend clones the target for the augmented-assignment value), so
    /// the player expression is compared structurally, not by node id
    /// (the oracle renders playervar augmented assignments as
    /// `Modify Player Variable(Event Player, p, <op>, value)`, #87).
    fn player_modify(
        &mut self,
        value: &ExprId,
        player: ExprId,
        variable: PlayerVarId,
    ) -> Result<Option<(ModifyOp, ValueId)>, IrError> {
        let target_player = self
            .hir
            .exprs
            .get(player)
            .cloned()
            .ok_or_else(|| dangling("expression", player))?;
        let expression = self
            .hir
            .exprs
            .get(*value)
            .ok_or_else(|| dangling("expression", *value))?
            .clone();
        let Expr::Binary {
            op, left, right, ..
        } = &expression
        else {
            return Ok(None);
        };
        let Some(op) = modify_op(*op) else {
            return Ok(None);
        };
        let left_is_target = self.player_operand_is_target(*left, &target_player, variable)?;
        let right_is_target = self.player_operand_is_target(*right, &target_player, variable)?;
        match (left_is_target, right_is_target) {
            (true, _) => Ok(Some((op, self.lower_value(*right)?))),
            (false, true) => Ok(Some((op, self.lower_value(*left)?))),
            (false, false) => Ok(None),
        }
    }

    /// Whether a binary operand is a read of the given player variable on the
    /// given player expression (compared structurally, #87).
    fn player_operand_is_target(
        &self,
        operand: ExprId,
        target_player: &Expr,
        variable: PlayerVarId,
    ) -> Result<bool, IrError> {
        let node = self
            .hir
            .exprs
            .get(operand)
            .ok_or_else(|| dangling("expression", operand))?;
        match node {
            Expr::PlayerVar {
                player: p,
                variable: v,
                ..
            } if *v == variable => match self.hir.exprs.get(*p) {
                Some(player_expr) => Ok(player_exprs_equal(player_expr, target_player)),
                None => Err(dangling("expression", *p)),
            },
            _ => Ok(false),
        }
    }

    fn modify_pattern(
        &mut self,
        value: &ExprId,
        is_target: impl Fn(&Expr) -> bool,
    ) -> Result<Option<(ModifyOp, ValueId)>, IrError> {
        let expression = self
            .hir
            .exprs
            .get(*value)
            .ok_or_else(|| dangling("expression", *value))?;
        let Expr::Binary {
            op, left, right, ..
        } = expression
        else {
            return Ok(None);
        };
        let Some(op) = modify_op(*op) else {
            return Ok(None);
        };
        let left_is_target = {
            let Some(left_expr) = self.hir.exprs.get(*left) else {
                return Err(dangling("expression", *left));
            };
            is_target(left_expr)
        };
        let right_is_target = {
            let Some(right_expr) = self.hir.exprs.get(*right) else {
                return Err(dangling("expression", *right));
            };
            is_target(right_expr)
        };
        match (left_is_target, right_is_target) {
            (true, _) => Ok(Some((op, self.lower_value(*right)?))),
            (false, true) => Ok(Some((op, self.lower_value(*left)?))),
            (false, false) => Ok(None),
        }
    }

    fn lower_for(
        &mut self,
        variable: GlobalVarId,
        iterable: ExprId,
        body: &[StmtId],
        span: Option<Span>,
        variable_span: Option<Span>,
    ) -> Result<Action, IrError> {
        let (start, stop, step) = self.range_bounds(iterable)?;
        Ok(Action::ForGlobalVariable {
            variable: self.global(variable)?,
            start,
            stop,
            step,
            body: self.lower_actions(body)?,
            span,
            target_span: variable_span,
        })
    }

    /// Split a `range` iterable into start/stop/step bounds.
    fn range_bounds(&mut self, iterable: ExprId) -> Result<(ValueId, ValueId, ValueId), IrError> {
        let expression = self
            .hir
            .exprs
            .get(iterable)
            .ok_or_else(|| dangling("expression", iterable))?;
        let Expr::Call { name, args, span } = expression else {
            return Err(unsupported(
                "for-loop iterable must be a range() call for the v0.1 lowering surface",
                expression.span(),
            ));
        };
        if name != "range" {
            return Err(unsupported(
                format!("for-loop iterable '{name}' is outside the v0.1 lowering surface"),
                *span,
            ));
        }
        let zero = self.target.values.push(ValueNode::new(
            Value::Number {
                value: 0.0,
                text: "0".to_string(),
            },
            None,
        ));
        let one = self.target.values.push(ValueNode::new(
            Value::Number {
                value: 1.0,
                text: "1".to_string(),
            },
            None,
        ));
        match args.as_slice() {
            [stop] => Ok((zero, self.lower_value(*stop)?, one)),
            [start, stop] => Ok((self.lower_value(*start)?, self.lower_value(*stop)?, one)),
            [start, stop, step] => Ok((
                self.lower_value(*start)?,
                self.lower_value(*stop)?,
                self.lower_value(*step)?,
            )),
            _ => Err(unsupported(
                "range() must take 1, 2, or 3 arguments for the v0.1 lowering surface",
                *span,
            )),
        }
    }

    fn lower_value(&mut self, id: ExprId) -> Result<ValueId, IrError> {
        let empty = HashMap::new();
        self.lower_value_with(id, &empty)
    }

    /// Lower an expression, substituting any `MacroParam` references from the
    /// enclosing macro definition with the bound call arguments.
    fn lower_value_with(
        &mut self,
        id: ExprId,
        params: &HashMap<String, ExprId>,
    ) -> Result<ValueId, IrError> {
        let expression = self
            .hir
            .exprs
            .get(id)
            .ok_or_else(|| dangling("expression", id))?;
        let span = expression.span();
        let value = match expression {
            Expr::Number { value, text, .. } => ValueNode::new(
                Value::Number {
                    value: *value,
                    text: text.clone(),
                },
                span,
            ),
            Expr::String { value, .. } => ValueNode::new(Value::String(value.clone()), span),
            Expr::Bool { value, .. } => ValueNode::new(Value::Bool(*value), span),
            Expr::Null { .. } => ValueNode::new(Value::Null, span),
            Expr::Array { elements, .. } => ValueNode::new(
                Value::Array(self.lower_values_with(elements, params)?),
                span,
            ),
            Expr::Vector { x, y, z, .. } => ValueNode::new(
                Value::Vector {
                    x: self.lower_value_with(*x, params)?,
                    y: self.lower_value_with(*y, params)?,
                    z: self.lower_value_with(*z, params)?,
                },
                span,
            ),
            Expr::Enum {
                value_type, value, ..
            } => ValueNode::new(
                Value::Enum {
                    value_type: value_type.clone(),
                    value: value.clone(),
                },
                span,
            ),
            Expr::GlobalVar { variable, .. } => {
                ValueNode::new(Value::GlobalVariable(self.global(*variable)?), span)
            }
            Expr::PlayerVar {
                player, variable, ..
            } => ValueNode::new(
                Value::PlayerVariable {
                    player: self.lower_value_with(*player, params)?,
                    variable: self.player(*variable)?,
                },
                span,
            ),
            Expr::EventPlayer { .. } => ValueNode::new(Value::EventPlayer, span),
            Expr::Call { name, args, .. } => ValueNode::new(
                Value::Call {
                    name: name.clone(),
                    args: self.lower_values_with(args, params)?,
                },
                span,
            ),
            Expr::ReceiverCall {
                receiver,
                name,
                args,
                ..
            } => {
                let mut lowered = vec![self.lower_value_with(*receiver, params)?];
                lowered.extend(self.lower_values_with(args, params)?);
                ValueNode::new(
                    Value::Call {
                        name: name.clone(),
                        args: lowered,
                    },
                    span,
                )
            }
            Expr::MacroCall { macro_, args, .. } => {
                return self.expand_macro_call(*macro_, args, params);
            }
            Expr::MacroParam { name, .. } => match params.get(name) {
                Some(arg) => return self.lower_value_with(*arg, params),
                None => {
                    return Err(unsupported(
                        format!("macro parameter '${name}' escaped its macro definition"),
                        expression.span(),
                    ));
                }
            },
            Expr::Binary {
                op, left, right, ..
            } => ValueNode::new(
                Value::Call {
                    name: op.as_str().to_string(),
                    args: vec![
                        self.lower_value_with(*left, params)?,
                        self.lower_value_with(*right, params)?,
                    ],
                },
                span,
            ),
            Expr::Unary { op, operand, .. } => ValueNode::new(
                Value::Call {
                    name: op.as_str().to_string(),
                    args: vec![self.lower_value_with(*operand, params)?],
                },
                span,
            ),
            Expr::Index { array, index, .. } => ValueNode::new(
                Value::Call {
                    name: "valueInArray".to_string(),
                    args: vec![
                        self.lower_value_with(*array, params)?,
                        self.lower_value_with(*index, params)?,
                    ],
                },
                span,
            ),
            Expr::Format { text, args, .. } => {
                let mut lowered = vec![
                    self.target
                        .values
                        .push(ValueNode::new(Value::String(text.clone()), span)),
                ];
                lowered.extend(self.lower_values_with(args, params)?);
                ValueNode::new(
                    Value::Call {
                        name: "format".to_string(),
                        args: lowered,
                    },
                    span,
                )
            }
            Expr::Constant { .. } => {
                return Err(unsupported(
                    "constant references are folded by the frontend and are outside the v0.1 lowering surface",
                    expression.span(),
                ));
            }
        };
        Ok(self.target.values.push(value))
    }

    fn lower_values_with(
        &mut self,
        ids: &[ExprId],
        params: &HashMap<String, ExprId>,
    ) -> Result<Vec<ValueId>, IrError> {
        ids.iter()
            .map(|id| self.lower_value_with(*id, params))
            .collect()
    }

    fn lower_values(&mut self, ids: &[ExprId]) -> Result<Vec<ValueId>, IrError> {
        let empty = HashMap::new();
        self.lower_values_with(ids, &empty)
    }

    /// Expand a source-level macro call: bind the call arguments to the macro
    /// parameters and lower the macro body expression with that context.
    fn expand_macro_call(
        &mut self,
        macro_: MacroId,
        args: &[ExprId],
        outer: &HashMap<String, ExprId>,
    ) -> Result<ValueId, IrError> {
        let macro_ = self
            .hir
            .macros
            .get(macro_)
            .ok_or_else(|| dangling("macro", macro_))?;
        if args.len() != macro_.args.len() {
            return Err(unsupported(
                format!(
                    "macro '{}' expects {} arguments, got {}",
                    macro_.name,
                    macro_.args.len(),
                    args.len()
                ),
                macro_.span,
            ));
        }
        let body = &macro_.body;
        if body.len() != 1 {
            return Err(unsupported(
                format!(
                    "macro '{}' must have a single expression body for the v0.1 lowering surface",
                    macro_.name
                ),
                macro_.span,
            ));
        }
        let statement = self
            .hir
            .stmts
            .get(body[0])
            .ok_or_else(|| dangling("statement", body[0]))?;
        let Stmt::Expr { expr, .. } = statement else {
            return Err(unsupported(
                format!(
                    "macro '{}' must have an expression body for the v0.1 lowering surface",
                    macro_.name
                ),
                macro_.span,
            ));
        };
        let mut bound: HashMap<String, ExprId> = HashMap::with_capacity(macro_.args.len());
        for (name, arg) in macro_.args.iter().zip(args.iter()) {
            bound.insert(name.clone(), *arg);
        }
        // Outer parameters remain visible inside the expanded body so nested
        // macro calls keep working.
        for (name, arg) in outer {
            bound.entry(name.clone()).or_insert(*arg);
        }
        self.lower_value_with(*expr, &bound)
    }

    fn global(&self, id: GlobalVarId) -> Result<wir::GlobalVarId, IrError> {
        self.globals
            .get(&id)
            .copied()
            .ok_or_else(|| dangling("global variable", id))
    }

    fn player(&self, id: PlayerVarId) -> Result<wir::PlayerVarId, IrError> {
        self.players
            .get(&id)
            .copied()
            .ok_or_else(|| dangling("player variable", id))
    }

    fn subroutine(&self, id: SubroutineId) -> Result<wir::SubroutineId, IrError> {
        self.subroutines
            .get(&id)
            .copied()
            .ok_or_else(|| dangling("subroutine", id))
    }
}

/// The workshop modify operator for a binary operator, if it is one.
fn modify_op(op: BinaryOp) -> Option<ModifyOp> {
    Some(match op {
        BinaryOp::Add => ModifyOp::Add,
        BinaryOp::Subtract => ModifyOp::Subtract,
        BinaryOp::Multiply => ModifyOp::Multiply,
        BinaryOp::Divide => ModifyOp::Divide,
        BinaryOp::Modulo => ModifyOp::Modulo,
        BinaryOp::Power => ModifyOp::RaiseToPower,
        _ => return None,
    })
}

/// Structural equality of two player expressions (the augmented-assignment
/// target and the binary's operand are distinct nodes, #87). The producible
/// receivers are `eventPlayer` (the surface form) and global references.
fn player_exprs_equal(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::EventPlayer { .. }, Expr::EventPlayer { .. }) => true,
        (Expr::GlobalVar { variable: x, .. }, Expr::GlobalVar { variable: y, .. }) => x == y,
        _ => false,
    }
}

/// All arena indices of a given type in `0..len`.
fn range_ids<T>(len: usize) -> Vec<crate::ids::Id<T>> {
    (0..len).map(crate::ids::Id::from_index).collect()
}

fn dangling(what: &'static str, id: impl crate::ids::IdLike) -> IrError {
    IrError::DanglingReference {
        what,
        id: id.index() as u32,
    }
}
