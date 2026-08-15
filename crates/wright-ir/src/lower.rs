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
//! * Declaration initializers lower directly into synthetic
//!   "Initialize global variables" / "Initialize player variables" rules in
//!   this profile-independent path (#112): the initializer semantics are
//!   source semantics owned by lowering, never by an optimization profile,
//!   and the reference frontend emits the same Initialize rules.
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
    /// Per-parameter-name call counter for the reference-shaped
    /// materialization of void-function arguments into per-call player
    /// variables (`by`, `by_0`, `by_1`, … in call order, P5 evidence).
    materialized: HashMap<String, u32>,
    /// Whether the statement currently being lowered sits inside a `switch`
    /// case body (any nesting depth). Nested `break` inside a case is the
    /// declared #118 surface; it is recorded structurally because only
    /// top-level trailing breaks take part in the reference's Skip dispatch
    /// (#119).
    in_switch_case: bool,
    /// Inlining/expansion recursion depth (bounded recursion, #118
    /// category 7).
    depth: u32,
}

/// How a user-function parameter is bound at a call site.
#[derive(Clone)]
enum ParamBinding {
    /// Substituted directly by the call argument (value functions).
    Inline(ExprId),
    /// Materialized into the named per-call player variable (void
    /// functions, reference shape P5).
    Var(wir::PlayerVarId),
}

impl<'a> Lowerer<'a> {
    fn new(hir: &'a hir::Program) -> Self {
        Lowerer {
            hir,
            target: wir::Program::default(),
            globals: HashMap::new(),
            players: HashMap::new(),
            subroutines: HashMap::new(),
            materialized: HashMap::new(),
            in_switch_case: false,
            depth: 0,
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

        // Variable, player, and subroutine tables. Non-trivial initializers
        // are collected and lowered into synthetic Initialize rules below.
        //
        // Global slots follow the pinned reference's variable manager
        // (#114): explicit source indices are honored, unindexed globals
        // (declared without an index, or implicit OverPy default variables
        // such as `for I in range(...)`) take the lowest free slot, and the
        // table is emitted in slot order — so `globalvar total` plus an
        // implicit `I` (fixed slot 8) emits `0: total, 8: I`, exactly like
        // the reference. Initializers stay in declaration order regardless
        // of the slot-ordered table, matching the reference's
        // `globalInitDirectives` order.
        let mut global_initializers: Vec<(GlobalVarId, ExprId)> = Vec::new();
        for id in range_ids::<hir::GlobalVar>(self.hir.globals.len()) {
            let initializer = self
                .hir
                .globals
                .get(id)
                .ok_or_else(|| dangling("global variable", id))?
                .initializer;
            if let Some(initializer) = initializer {
                global_initializers.push((id, initializer));
            }
        }
        let mut global_taken: std::collections::HashSet<u32> = std::collections::HashSet::new();
        // (HIR id, name, span, name_span, final Workshop slot).
        type GlobalEntry = (GlobalVarId, String, Option<Span>, Option<Span>, Option<u32>);
        let mut global_entries: Vec<GlobalEntry> = Vec::new();
        for id in range_ids::<hir::GlobalVar>(self.hir.globals.len()) {
            let global = self
                .hir
                .globals
                .get(id)
                .ok_or_else(|| dangling("global variable", id))?;
            let (name, span, name_span, source_index) = (
                global.name.clone(),
                global.span,
                global.name_span,
                global.index,
            );
            if let Some(index) = source_index {
                global_taken.insert(index);
            }
            global_entries.push((id, name, span, name_span, source_index));
        }
        let mut next_free = 0u32;
        for entry in global_entries.iter_mut() {
            if entry.4.is_some() {
                continue;
            }
            while global_taken.contains(&next_free) {
                next_free += 1;
            }
            entry.4 = Some(next_free);
            global_taken.insert(next_free);
            next_free += 1;
        }
        global_entries.sort_by_key(|(_, _, _, _, index)| index.expect("assigned above"));
        for (id, name, span, name_span, index) in global_entries {
            let wir_id = self.target.global_variables.push(wir::WorkshopVariable {
                name,
                index: index.expect("assigned above"),
                span,
                name_span,
            });
            self.globals.insert(id, wir_id);
        }
        let mut player_initializers = Vec::new();
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
            let wir_id = self.target.player_variables.push(wir::WorkshopVariable {
                name,
                index: source_index.unwrap_or(id.index() as u32),
                span,
                name_span,
            });
            if let Some(initializer) = initializer {
                player_initializers.push((id, initializer));
            }
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

        // Rules. Declaration initializers become synthetic
        // "Initialize global variables" / "Initialize player variables"
        // rules here, in the profile-independent lowering path, so
        // initialization semantics never depend on an optimization profile
        // (#112). The initialize rules have priority 0 like the reference's
        // synthesized initial rules (P1: a `-1` priority rule sorts before
        // "Initial Global"); every rule carries its declaration priority and
        // the emitted table is a stable sort by (priority, declaration
        // order), matching the reference's rule-priority ordering. The
        // lowered initializers are the single source of truth; the variable
        // tables carry no initializer field.
        let mut rules: Vec<(i32, usize, wir::Rule)> = Vec::new();
        let mut order = 0usize;
        if !global_initializers.is_empty() {
            let mut actions = Vec::new();
            for (id, initializer) in global_initializers {
                let value = self.lower_value_with(initializer, &HashMap::new(), &mut actions)?;
                actions.push(self.target.actions.push(Action::SetGlobalVariable {
                    variable: self.globals[&id],
                    value,
                    span: None,
                    target_span: None,
                }));
            }
            rules.push((
                0,
                order,
                wir::Rule {
                    name: "Initialize global variables".to_string(),
                    span: None,
                    name_span: None,
                    disabled: false,
                    event: wir::Event::Global,
                    conditions: Vec::new(),
                    actions,
                },
            ));
            order += 1;
        }
        if !player_initializers.is_empty() {
            let mut actions = Vec::new();
            for (id, initializer) in player_initializers {
                let value = self.lower_value_with(initializer, &HashMap::new(), &mut actions)?;
                let player = self
                    .target
                    .values
                    .push(wir::ValueNode::new(wir::Value::EventPlayer, None));
                actions.push(self.target.actions.push(Action::SetPlayerVariable {
                    player,
                    variable: self.players[&id],
                    value,
                    span: None,
                    target_span: None,
                }));
            }
            rules.push((
                0,
                order,
                wir::Rule {
                    name: "Initialize player variables".to_string(),
                    span: None,
                    name_span: None,
                    disabled: false,
                    event: wir::Event::EachPlayer,
                    conditions: Vec::new(),
                    actions,
                },
            ));
            order += 1;
        }

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
            rules.push((
                0,
                order,
                wir::Rule {
                    name: format!("Subroutine {name}"),
                    span: body_span,
                    name_span: body_name_span,
                    disabled: false,
                    event: wir::Event::Subroutine(self.subroutines[&id]),
                    conditions: Vec::new(),
                    actions,
                },
            ));
            order += 1;
        }
        let rule_ids = range_ids::<hir::Rule>(self.hir.rules.len());
        for id in rule_ids {
            let priority = self
                .hir
                .rules
                .get(id)
                .ok_or_else(|| dangling("rule", id))?
                .priority;
            let rule = self.lower_rule(id)?;
            rules.push((priority.unwrap_or(0), order, rule));
            order += 1;
        }
        rules.sort_by_key(|(priority, order, _)| (*priority, *order));
        for (_, _, rule) in rules {
            self.target.rules.push(rule);
        }

        Ok(self.target)
    }

    fn lower_rule(&mut self, id: hir::RuleId) -> Result<wir::Rule, IrError> {
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
        Ok(wir::Rule {
            name: rule.name,
            span: rule.span,
            name_span: rule.name_span,
            disabled: rule.disabled,
            event,
            conditions,
            actions,
        })
    }

    fn lower_actions(&mut self, statements: &[StmtId]) -> Result<Vec<wir::ActionId>, IrError> {
        let empty = HashMap::new();
        self.lower_actions_with(statements, &empty)
    }

    /// Lower statements with the enclosing user-function parameter bindings
    /// (used by the #119 function-inlining path; rules/subroutines pass an
    /// empty binding map).
    fn lower_actions_with(
        &mut self,
        statements: &[StmtId],
        params: &HashMap<String, ParamBinding>,
    ) -> Result<Vec<wir::ActionId>, IrError> {
        let mut actions = Vec::new();
        for statement in statements {
            self.lower_action_with(*statement, &mut actions, params)?;
        }
        Ok(actions)
    }

    fn lower_action_with(
        &mut self,
        id: StmtId,
        out: &mut Vec<wir::ActionId>,
        params: &HashMap<String, ParamBinding>,
    ) -> Result<(), IrError> {
        self.depth += 1;
        let result = self.lower_action_with_inner(id, out, params);
        self.depth -= 1;
        result
    }

    fn lower_action_with_inner(
        &mut self,
        id: StmtId,
        out: &mut Vec<wir::ActionId>,
        params: &HashMap<String, ParamBinding>,
    ) -> Result<(), IrError> {
        if self.depth > 256 {
            return Err(unsupported(
                "statement inlining exceeded the bounded-recursion limit (256)",
                self.hir.stmts.get(id).and_then(|s| s.span()),
            ));
        }
        let statement = self
            .hir
            .stmts
            .get(id)
            .ok_or_else(|| dangling("statement", id))?
            .clone();
        let span = statement.span();
        let action = match &statement {
            Stmt::Expr { expr, .. } => match self.lower_expr_stmt_with(*expr, span, out, params)? {
                Some(action) => action,
                // The statement inlined into `out` (void-function call or a
                // value-function call used for its side effects); nothing
                // further to push.
                None => return Ok(()),
            },
            Stmt::Assign { target, value, .. } => {
                self.lower_assign_with(target, value, span, out, params)?
            }
            Stmt::If {
                branches,
                else_body,
                ..
            } => {
                let mut lowered_branches = Vec::with_capacity(branches.len());
                for branch in branches {
                    lowered_branches.push(wir::IfBranch {
                        condition: self.lower_value_with(branch.condition, params, out)?,
                        body: self.lower_actions_with(&branch.body, params)?,
                    });
                }
                let lowered_else = match else_body {
                    Some(body) => Some(self.lower_actions_with(body, params)?),
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
            } => self.lower_for_with(
                *variable,
                *iterable,
                body,
                span,
                *variable_span,
                out,
                params,
            )?,
            Stmt::While {
                condition, body, ..
            } => Action::While {
                condition: self.lower_value_with(*condition, params, out)?,
                body: self.lower_actions_with(body, params)?,
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
            // #118 frontend-neutral extensions: their faithful Workshop
            // lowering is #119. `Foreach`/`CFor` lower to the reference's
            // `For Global Variable` forms; `Switch` lowers to the reference's
            // Skip-array dispatch with sequential case bodies (fallthrough),
            // `break` = `Skip` over the remaining bodies, `default` = the
            // last body; `Return` in a rule lowers to `Abort` (P5 evidence).
            Stmt::Foreach {
                variable,
                iterable,
                body,
                ..
            } => self.lower_foreach(*variable, *iterable, body, span, out, params)?,
            Stmt::CFor {
                variable,
                start,
                condition,
                step,
                body,
                ..
            } => self.lower_c_for(
                *variable, *start, *condition, *step, body, span, out, params,
            )?,
            Stmt::Switch { value, cases, .. } => {
                // Pushes the dispatch and every case body into `out`.
                return self.lower_switch(*value, cases, span, out, params);
            }
            Stmt::Return { value, .. } => {
                // P5: `return;` inside a rule body is `Abort` (early exit).
                // The return VALUE of a statement-bodied user function is
                // consumed by the inlining path before this point; a valued
                // return reaching a rule body has no Workshop meaning beyond
                // the same early exit (declared #119 contract).
                if let Some(value) = value {
                    let _ = self.lower_value_with(*value, params, out)?;
                }
                Action::Call {
                    name: "abort".to_string(),
                    args: Vec::new(),
                    span,
                }
            }
            // `break` outside a switch case body has no Workshop lowering
            // (no loop break in the declared surface); `continue` is not in
            // the declared surface either. Both fail deterministically
            // instead of deferring to emission.
            // `break` inside a switch case body (any nesting depth) is the
            // declared #118 surface; only top-level trailing breaks take
            // part in the reference's Skip dispatch (#119), so nested breaks
            // are recorded structurally for the shared analyzer. A `break`
            // outside a switch has no Workshop lowering and fails
            // deterministically; `continue` is not in the declared surface.
            Stmt::Break { span } => {
                if self.in_switch_case {
                    Action::Call {
                        name: "break".to_string(),
                        args: Vec::new(),
                        span: *span,
                    }
                } else {
                    return Err(unsupported(
                        "break is only supported inside a switch case body on the #119 surface",
                        *span,
                    ));
                }
            }
            Stmt::Continue { span } => {
                return Err(unsupported(
                    "continue is outside the #119 OSTW lowering surface",
                    *span,
                ));
            }
            Stmt::Pass { .. } => return Ok(()), // no-op; dropped
        };
        out.push(self.target.actions.push(action));
        Ok(())
    }

    /// Lower an expression statement. A call to a void user function inlines
    /// its body at the call site (#119, P5 evidence): each argument is
    /// materialized into a fresh per-call player variable named after the
    /// parameter (`by`, `by_0`, …), then the body statements are lowered
    /// with the parameters bound to those variables — the reference's exact
    /// emission shape. A call to a value function used as a statement lowers
    /// its body for the side effects and discards the value.
    fn lower_expr_stmt_with(
        &mut self,
        expr: ExprId,
        span: Option<Span>,
        out: &mut Vec<wir::ActionId>,
        params: &HashMap<String, ParamBinding>,
    ) -> Result<Option<Action>, IrError> {
        let expression = self
            .hir
            .exprs
            .get(expr)
            .ok_or_else(|| dangling("expression", expr))?;
        if let Expr::UserCall { function, args, .. } = expression {
            let decl = self
                .hir
                .functions
                .get(*function)
                .ok_or_else(|| dangling("function", *function))?;
            match &decl.body {
                hir::FunctionBody::Statements(statements) => {
                    self.inline_void_call(*function, args, statements, out, params, span)?;
                    return Ok(None);
                }
                hir::FunctionBody::Expression(body) => {
                    let bound = self.bind_inline_args(decl, args, params)?;
                    let _ = self.lower_value_with(*body, &bound, out)?;
                    return Ok(None);
                }
            }
        }
        let action = match expression {
            Expr::Call { name, args, .. } if name == "debug" && args.len() == 1 => Action::Debug {
                value: self.lower_value_with(args[0], params, out)?,
                span,
            },
            Expr::Call { name, args, .. } if name == "print" && args.len() == 1 => Action::Print {
                message: self.lower_value_with(args[0], params, out)?,
                span,
            },
            Expr::ReceiverCall {
                receiver,
                name,
                args,
                ..
            } if name == "append" => self.lower_append_with(receiver, args, span, out, params)?,
            Expr::Call { name, args, .. } => Action::Call {
                name: name.clone(),
                args: self.lower_values_with(args, params, out)?,
                span,
            },
            Expr::ReceiverCall {
                receiver,
                name,
                args,
                ..
            } => {
                let mut lowered = vec![self.lower_value_with(*receiver, params, out)?];
                lowered.extend(self.lower_values_with(args, params, out)?);
                Action::Call {
                    name: name.clone(),
                    args: lowered,
                    span,
                }
            }
            other => {
                return Err(unsupported(
                    format!(
                        "expression statement '{}' is outside the v0.1 lowering surface",
                        other.kind_name()
                    ),
                    span,
                ));
            }
        };
        Ok(Some(action))
    }

    /// The per-parameter-name call counter: the reference names materialized
    /// call variables after the parameter with `_0`, `_1`, … suffixes in
    /// call order (P5 evidence: `by`, `by_0`, `by_1`, …).
    fn materialized_var_name(&mut self, param: &str) -> String {
        let counter = self.materialized.entry(param.to_string()).or_insert(0);
        let name = if *counter == 0 {
            param.to_string()
        } else {
            format!("{param}_{}", *counter - 1)
        };
        *counter += 1;
        name
    }

    /// Materialize the call arguments into fresh per-call player variables
    /// and bind the function parameters to them (reference shape, P5).
    fn bind_materialized_args(
        &mut self,
        function: &hir::Function,
        args: &[ExprId],
        out: &mut Vec<wir::ActionId>,
        params: &HashMap<String, ParamBinding>,
        span: Option<Span>,
    ) -> Result<HashMap<String, ParamBinding>, IrError> {
        let mut bound = HashMap::with_capacity(function.params.len());
        let player = self
            .target
            .values
            .push(ValueNode::new(Value::EventPlayer, span));
        for (index, param) in function.params.iter().enumerate() {
            let arg = args.get(index).copied().ok_or_else(|| {
                unsupported(
                    format!(
                        "function '{}' is missing an argument for parameter '{}' \
                         (defaults are resolved by the frontend)",
                        function.name, param.name
                    ),
                    param.span,
                )
            })?;
            let value = self.lower_value_with(arg, params, out)?;
            let name = self.materialized_var_name(&param.name);
            let variable = self.target.player_variables.push(wir::WorkshopVariable {
                name,
                index: self.target.player_variables.len() as u32,
                span: None,
                name_span: None,
            });
            out.push(self.target.actions.push(Action::SetPlayerVariable {
                player,
                variable,
                value,
                span,
                target_span: None,
            }));
            bound.insert(param.name.clone(), ParamBinding::Var(variable));
        }
        Ok(bound)
    }

    /// Bind function parameters to the call arguments for direct value
    /// substitution (value functions, reference shape P5).
    ///
    /// Arguments that are bare `Param` references to an *outer* binding are
    /// resolved through `outer` at bind time: the inner binding map is
    /// keyed by name and cannot distinguish scopes, so a shadowed name
    /// (`shouldAllowSelection(p)` inside a function whose parameter is also
    /// `p`) would otherwise substitute the inner parameter with itself and
    /// recurse forever. Outer bindings for non-shadowed names remain visible
    /// inside the inlined body (like macro expansion), so nested `Param`
    /// references inside argument expressions still resolve.
    fn bind_inline_args(
        &self,
        function: &hir::Function,
        args: &[ExprId],
        outer: &HashMap<String, ParamBinding>,
    ) -> Result<HashMap<String, ParamBinding>, IrError> {
        let mut bound = HashMap::with_capacity(function.params.len());
        for (index, param) in function.params.iter().enumerate() {
            let arg = args.get(index).copied().ok_or_else(|| {
                unsupported(
                    format!(
                        "function '{}' is missing an argument for parameter '{}' \
                         (defaults are resolved by the frontend)",
                        function.name, param.name
                    ),
                    param.span,
                )
            })?;
            bound.insert(param.name.clone(), self.resolve_param_arg(arg, outer, 0)?);
        }
        for (name, binding) in outer {
            bound.entry(name.clone()).or_insert_with(|| binding.clone());
        }
        Ok(bound)
    }

    /// Resolve a call argument through the outer parameter bindings: a bare
    /// `Param` reference to an outer binding substitutes that binding
    /// directly; materialized outer parameters propagate as `Var` bindings.
    fn resolve_param_arg(
        &self,
        arg: ExprId,
        outer: &HashMap<String, ParamBinding>,
        depth: u32,
    ) -> Result<ParamBinding, IrError> {
        if depth > 64 {
            return Err(unsupported(
                "parameter bindings exceeded the resolution depth limit",
                self.hir.exprs.get(arg).and_then(|e| e.span()),
            ));
        }
        match self.hir.exprs.get(arg) {
            Some(Expr::Param { name, span: _ }) => match outer.get(name) {
                Some(ParamBinding::Inline(expr)) => self.resolve_param_arg(*expr, outer, depth + 1),
                Some(ParamBinding::Var(variable)) => Ok(ParamBinding::Var(*variable)),
                None => Ok(ParamBinding::Inline(arg)),
            },
            _ => Ok(ParamBinding::Inline(arg)),
        }
    }

    /// Inline a void function's statement body at its call site. `return`
    /// inside a void body has no faithful lowering on the declared surface
    /// and fails deterministically.
    fn inline_void_call(
        &mut self,
        function_id: hir::FunctionId,
        args: &[ExprId],
        statements: &[StmtId],
        out: &mut Vec<wir::ActionId>,
        params: &HashMap<String, ParamBinding>,
        span: Option<Span>,
    ) -> Result<(), IrError> {
        let function = self
            .hir
            .functions
            .get(function_id)
            .ok_or_else(|| dangling("function", function_id))?;
        for statement in statements {
            if matches!(self.hir.stmts.get(*statement), Some(Stmt::Return { .. })) {
                let statement_span = self
                    .hir
                    .stmts
                    .get(*statement)
                    .and_then(|statement| statement.span());
                return Err(unsupported(
                    format!(
                        "return inside function '{}' is outside the #119 inlining surface \
                         (only the terminal return of a value function lowers)",
                        function.name
                    ),
                    statement_span,
                ));
            }
        }
        let bound = self.bind_materialized_args(function, args, out, params, span)?;
        out.extend(self.lower_actions_with(statements, &bound)?);
        Ok(())
    }

    fn lower_append_with(
        &mut self,
        receiver: &ExprId,
        args: &[ExprId],
        span: Option<Span>,
        out: &mut Vec<wir::ActionId>,
        params: &HashMap<String, ParamBinding>,
    ) -> Result<Action, IrError> {
        let value = match args {
            [value] => self.lower_value_with(*value, params, out)?,
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
                let player = self.lower_value_with(*player, params, out)?;
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

    fn lower_assign_with(
        &mut self,
        target: &ExprId,
        value: &ExprId,
        span: Option<Span>,
        out: &mut Vec<wir::ActionId>,
        params: &HashMap<String, ParamBinding>,
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
                match self.global_modify_with(value, *variable, out, params)? {
                    Some((op, operand)) => Ok(Action::ModifyGlobalVariable {
                        variable: global,
                        op,
                        value: operand,
                        span,
                        target_span,
                    }),
                    None => Ok(Action::SetGlobalVariable {
                        variable: global,
                        value: self.lower_value_with(*value, params, out)?,
                        span,
                        target_span,
                    }),
                }
            }
            Expr::PlayerVar {
                player, variable, ..
            } => {
                let player_value = self.lower_value_with(*player, params, out)?;
                let player_id = *player;
                let player_var = self.player(*variable)?;
                let target_span = self.occurrence_span(target_expr);
                match self.player_modify_with(value, player_id, *variable, out, params)? {
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
                        value: self.lower_value_with(*value, params, out)?,
                        span,
                        target_span,
                    }),
                }
            }
            Expr::Index { array, index, .. } => {
                // Indexed assignment (`arr[i] = v`): the reference emits
                // `Set/Modify … Variable At Index`. Record it structurally as
                // a named action so the shared analyzer still sees the
                // occurrence; faithful emission spelling is #119's concern.
                let index_value = self.lower_value_with(*index, params, out)?;
                let value = self.lower_value_with(*value, params, out)?;
                let array_value = self.lower_value_with(*array, params, out)?;
                match indexed_array_kind(array, &self.hir) {
                    IndexedArrayKind::Global => Ok(Action::Call {
                        name: "setGlobalVariableAtIndex".to_string(),
                        args: vec![array_value, index_value, value],
                        span,
                    }),
                    IndexedArrayKind::Player => Ok(Action::Call {
                        name: "setPlayerVariableAtIndex".to_string(),
                        args: vec![array_value, index_value, value],
                        span,
                    }),
                    IndexedArrayKind::Other => {
                        return Err(unsupported(
                            "indexed assignment requires a global or player array target",
                            span,
                        ));
                    }
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
    fn global_modify_with(
        &mut self,
        value: &ExprId,
        target: GlobalVarId,
        out: &mut Vec<wir::ActionId>,
        params: &HashMap<String, ParamBinding>,
    ) -> Result<Option<(ModifyOp, ValueId)>, IrError> {
        self.modify_pattern_with(
            value,
            |other| matches!(other, Expr::GlobalVar { variable, .. } if *variable == target),
            out,
            params,
        )
    }

    /// Detect `x = x <op> rhs` (or `x = rhs <op> x`) for a player target.
    ///
    /// The target and the binary's operand are distinct HIR nodes (the
    /// frontend clones the target for the augmented-assignment value), so
    /// the player expression is compared structurally, not by node id
    /// (the oracle renders playervar augmented assignments as
    /// `Modify Player Variable(Event Player, p, <op>, value)`, #87).
    fn player_modify_with(
        &mut self,
        value: &ExprId,
        player: ExprId,
        variable: PlayerVarId,
        out: &mut Vec<wir::ActionId>,
        params: &HashMap<String, ParamBinding>,
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
            (true, _) => Ok(Some((op, self.lower_value_with(*right, params, out)?))),
            (false, true) => Ok(Some((op, self.lower_value_with(*left, params, out)?))),
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

    fn modify_pattern_with(
        &mut self,
        value: &ExprId,
        is_target: impl Fn(&Expr) -> bool,
        out: &mut Vec<wir::ActionId>,
        params: &HashMap<String, ParamBinding>,
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
            (true, _) => Ok(Some((op, self.lower_value_with(*right, params, out)?))),
            (false, true) => Ok(Some((op, self.lower_value_with(*left, params, out)?))),
            (false, false) => Ok(None),
        }
    }

    fn lower_for_with(
        &mut self,
        variable: GlobalVarId,
        iterable: ExprId,
        body: &[StmtId],
        span: Option<Span>,
        variable_span: Option<Span>,
        out: &mut Vec<wir::ActionId>,
        params: &HashMap<String, ParamBinding>,
    ) -> Result<Action, IrError> {
        let (start, stop, step) = self.range_bounds_with(iterable, out, params)?;
        Ok(Action::ForGlobalVariable {
            variable: self.global(variable)?,
            start,
            stop,
            step,
            body: self.lower_actions_with(body, params)?,
            span,
            target_span: variable_span,
        })
    }

    /// Split a `range` iterable into start/stop/step bounds.
    fn range_bounds_with(
        &mut self,
        iterable: ExprId,
        out: &mut Vec<wir::ActionId>,
        params: &HashMap<String, ParamBinding>,
    ) -> Result<(ValueId, ValueId, ValueId), IrError> {
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
            [stop] => Ok((zero, self.lower_value_with(*stop, params, out)?, one)),
            [start, stop] => Ok((
                self.lower_value_with(*start, params, out)?,
                self.lower_value_with(*stop, params, out)?,
                one,
            )),
            [start, stop, step] => Ok((
                self.lower_value_with(*start, params, out)?,
                self.lower_value_with(*stop, params, out)?,
                self.lower_value_with(*step, params, out)?,
            )),
            _ => Err(unsupported(
                "range() must take 1, 2, or 3 arguments for the v0.1 lowering surface",
                *span,
            )),
        }
    }

    /// Lower a `foreach (x in arr)` loop: the reference emits
    /// `For Global Variable(counter, 0, Count Of(arr), 1)`; body references
    /// to the loop element were already rewritten to `Index(arr, counter)`
    /// by the frontend. The counter lowers as a global variable under the
    /// declared #119 contract (the reference places the foreach local in the
    /// per-player table; Workshop rule execution is atomic, so the loop
    /// semantics coincide — documented divergence).
    fn lower_foreach(
        &mut self,
        variable: GlobalVarId,
        iterable: ExprId,
        body: &[StmtId],
        span: Option<Span>,
        out: &mut Vec<wir::ActionId>,
        params: &HashMap<String, ParamBinding>,
    ) -> Result<Action, IrError> {
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
        let iterable_value = self.lower_value_with(iterable, params, out)?;
        let count = self.target.values.push(ValueNode::new(
            Value::Call {
                name: "countOf".to_string(),
                args: vec![iterable_value],
            },
            span,
        ));
        Ok(Action::ForGlobalVariable {
            variable: self.global(variable)?,
            start: zero,
            stop: count,
            step: one,
            body: self.lower_actions_with(body, params)?,
            span,
            target_span: None,
        })
    }

    /// Lower a C-style `for (init; condition; step)` loop: the reference
    /// emits `For Global Variable(variable, start, condition, step)` (P5).
    fn lower_c_for(
        &mut self,
        variable: GlobalVarId,
        start: Option<ExprId>,
        condition: Option<ExprId>,
        step: Option<ExprId>,
        body: &[StmtId],
        span: Option<Span>,
        out: &mut Vec<wir::ActionId>,
        params: &HashMap<String, ParamBinding>,
    ) -> Result<Action, IrError> {
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
        let true_value = self
            .target
            .values
            .push(ValueNode::new(Value::Bool(true), None));
        let start = match start {
            Some(start) => self.lower_value_with(start, params, out)?,
            None => zero,
        };
        let stop = match condition {
            Some(condition) => self.lower_value_with(condition, params, out)?,
            None => true_value,
        };
        let step = match step {
            Some(step) => self.lower_value_with(step, params, out)?,
            None => one,
        };
        Ok(Action::ForGlobalVariable {
            variable: self.global(variable)?,
            start,
            stop,
            step,
            body: self.lower_actions_with(body, params)?,
            span,
            target_span: None,
        })
    }

    /// Lower a `switch` to the reference's Skip-array dispatch (P5 evidence):
    /// sequential case bodies in source order (fallthrough = no skip between
    /// consecutive bodies), `break` = `Skip` over the remaining bodies,
    /// `default` = the last body, and a non-matching value skips to the
    /// default (or past everything without one). The dispatch is
    /// `Skip(Value In Array(jump_table, Add(Index Of Array Value(case
    /// constants, value), 1)))`; the jump table and break skips are computed
    /// over the emitted action counts, so the construction is identical to
    /// the reference's for the same body shape. The dispatch and every case
    /// body are pushed into `out` in sequence.
    fn lower_switch(
        &mut self,
        value: ExprId,
        cases: &[crate::hir::SwitchCase],
        span: Option<Span>,
        out: &mut Vec<wir::ActionId>,
        params: &HashMap<String, ParamBinding>,
    ) -> Result<(), IrError> {
        let lowered_value = self.lower_value_with(value, params, out)?;
        let mut case_constants: Vec<ValueId> = Vec::new();
        let mut bodies: Vec<(Vec<wir::ActionId>, bool)> = Vec::new();
        for case in cases {
            let mut body = Vec::new();
            let mut broke = false;
            let previous_switch = self.in_switch_case;
            self.in_switch_case = true;
            for statement in &case.body {
                if matches!(self.hir.stmts.get(*statement), Some(Stmt::Break { .. })) {
                    // The reference emits `Skip(N)` over the remaining
                    // bodies at the break point; anything after an
                    // unconditional break is unreachable.
                    broke = true;
                    break;
                }
                self.lower_action_with(*statement, &mut body, params)?;
            }
            self.in_switch_case = previous_switch;
            if let Some(case_value) = case.value {
                case_constants.push(self.lower_value_with(case_value, params, out)?);
            }
            bodies.push((body, broke));
        }
        // Break skips: `Skip(tail)` after a body that ends in `break`,
        // where tail = actions of every later body plus their break skips
        // (computed back-to-front).
        let mut tail = 0usize;
        for index in (0..bodies.len()).rev() {
            if bodies[index].1 {
                let skip = self.target.values.push(ValueNode::new(
                    Value::Number {
                        value: tail as f64,
                        text: tail.to_string(),
                    },
                    span,
                ));
                bodies[index].0.push(self.target.actions.push(Action::Call {
                    name: "skip".to_string(),
                    args: vec![skip],
                    span,
                }));
                tail += 1;
            }
            tail += bodies[index].0.len();
        }
        // Jump table: entry 0 = the non-matched skip (to the default body,
        // or past everything without one), entry i+1 = the actions before
        // case i's body (0 for the first case). The break skips are already
        // part of their body's action list at this point.
        let mut offsets: Vec<usize> = Vec::with_capacity(bodies.len());
        let mut running = 0usize;
        for (body, _) in &bodies {
            offsets.push(running);
            running += body.len();
        }
        let default_index = cases.iter().position(|case| case.value.is_none());
        let not_matched = default_index.map_or(running, |index| offsets[index]);
        let mut table = Vec::with_capacity(case_constants.len() + 1);
        table.push(self.target.values.push(ValueNode::new(
            Value::Number {
                value: not_matched as f64,
                text: not_matched.to_string(),
            },
            span,
        )));
        // Only the non-default cases get a jump-table entry (the default is
        // reached through the not-matched entry, matching the reference).
        for (index, case) in cases.iter().enumerate() {
            if case.value.is_none() {
                continue;
            }
            let offset = offsets[index];
            table.push(self.target.values.push(ValueNode::new(
                Value::Number {
                    value: offset as f64,
                    text: offset.to_string(),
                },
                span,
            )));
        }
        let table_array = self
            .target
            .values
            .push(ValueNode::new(Value::Array(table), span));
        let constants_array = self
            .target
            .values
            .push(ValueNode::new(Value::Array(case_constants), span));
        let index = self.target.values.push(ValueNode::new(
            Value::Call {
                name: "indexOfArrayValue".to_string(),
                args: vec![constants_array, lowered_value],
            },
            span,
        ));
        let one = self.target.values.push(ValueNode::new(
            Value::Number {
                value: 1.0,
                text: "1".to_string(),
            },
            span,
        ));
        let shifted = self.target.values.push(ValueNode::new(
            Value::Call {
                name: "add".to_string(),
                args: vec![index, one],
            },
            span,
        ));
        let jump = self.target.values.push(ValueNode::new(
            Value::Call {
                name: "valueInArray".to_string(),
                args: vec![table_array, shifted],
            },
            span,
        ));
        out.push(self.target.actions.push(Action::Call {
            name: "skip".to_string(),
            args: vec![jump],
            span,
        }));
        for (body, _) in bodies {
            out.extend(body);
        }
        Ok(())
    }

    fn lower_value(&mut self, id: ExprId) -> Result<ValueId, IrError> {
        let mut out = Vec::new();
        let empty = HashMap::new();
        let result = self.lower_value_with(id, &empty, &mut out)?;
        if !out.is_empty() {
            return Err(unsupported(
                "a statement-bodied function call escaped the action stream",
                value_span(self.hir, id),
            ));
        }
        Ok(result)
    }

    /// Lower an expression with the enclosing bindings: macro parameters
    /// (`MacroParam`) and inlined user-function parameters (`Param`,
    /// #119). Statement-bodied user-function calls hoist their side-effect
    /// statements into `out` before the enclosing action, matching the
    /// reference's unconditional hoisting (P4 evidence).
    fn lower_value_with(
        &mut self,
        id: ExprId,
        params: &HashMap<String, ParamBinding>,
        out: &mut Vec<wir::ActionId>,
    ) -> Result<ValueId, IrError> {
        self.depth += 1;
        let result = self.lower_value_with_inner(id, params, out);
        self.depth -= 1;
        result
    }

    fn lower_value_with_inner(
        &mut self,
        id: ExprId,
        params: &HashMap<String, ParamBinding>,
        out: &mut Vec<wir::ActionId>,
    ) -> Result<ValueId, IrError> {
        if self.depth > 256 {
            return Err(unsupported(
                "expression inlining exceeded the bounded-recursion limit (256)",
                self.hir.exprs.get(id).and_then(|e| e.span()),
            ));
        }
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
                Value::Array(self.lower_values_with(elements, params, out)?),
                span,
            ),
            Expr::Vector { x, y, z, .. } => ValueNode::new(
                Value::Vector {
                    x: self.lower_value_with(*x, params, out)?,
                    y: self.lower_value_with(*y, params, out)?,
                    z: self.lower_value_with(*z, params, out)?,
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
                    player: self.lower_value_with(*player, params, out)?,
                    variable: self.player(*variable)?,
                },
                span,
            ),
            Expr::EventPlayer { .. } => ValueNode::new(Value::EventPlayer, span),
            Expr::Call { name, args, .. } => ValueNode::new(
                Value::Call {
                    name: name.clone(),
                    args: self.lower_values_with(args, params, out)?,
                },
                span,
            ),
            Expr::ReceiverCall {
                receiver,
                name,
                args,
                ..
            } => {
                let mut lowered = vec![self.lower_value_with(*receiver, params, out)?];
                lowered.extend(self.lower_values_with(args, params, out)?);
                ValueNode::new(
                    Value::Call {
                        name: name.clone(),
                        args: lowered,
                    },
                    span,
                )
            }
            Expr::MacroCall { macro_, args, .. } => {
                return self.expand_macro_call(*macro_, args, params, out);
            }
            Expr::MacroParam { name, .. } => match params.get(name) {
                Some(ParamBinding::Inline(arg)) => {
                    return self.lower_value_with(*arg, params, out);
                }
                Some(ParamBinding::Var(_)) | None => {
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
                        self.lower_value_with(*left, params, out)?,
                        self.lower_value_with(*right, params, out)?,
                    ],
                },
                span,
            ),
            Expr::Unary { op, operand, .. } => ValueNode::new(
                Value::Call {
                    name: op.as_str().to_string(),
                    args: vec![self.lower_value_with(*operand, params, out)?],
                },
                span,
            ),
            Expr::Index { array, index, .. } => ValueNode::new(
                Value::Call {
                    name: "valueInArray".to_string(),
                    args: vec![
                        self.lower_value_with(*array, params, out)?,
                        self.lower_value_with(*index, params, out)?,
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
                lowered.extend(self.lower_values_with(args, params, out)?);
                ValueNode::new(
                    Value::Call {
                        name: "format".to_string(),
                        args: lowered,
                    },
                    span,
                )
            }
            Expr::Constant { constant, .. } => {
                // The #118 semantic phase records constant (`define`)
                // references by name with a placeholder value; keep the
                // named reference structurally visible for shared analysis.
                // Value resolution is #119's emission concern.
                let name = self
                    .hir
                    .constants
                    .get(*constant)
                    .map(|constant| constant.name.clone())
                    .unwrap_or_default();
                ValueNode::new(
                    Value::Call {
                        name,
                        args: Vec::new(),
                    },
                    span,
                )
            }
            // #118 frontend-neutral extensions with their faithful #119
            // lowering: user enums are 0-based integers (P4 evidence),
            // value functions inline as expressions, statement-bodied value
            // functions hoist their side effects and inline the terminal
            // return value, ternaries lower to `If-Then-Else`, and casts are
            // emission pass-throughs (P4 evidence).
            Expr::UserEnum {
                enum_id, member, ..
            } => {
                let enum_ = self
                    .hir
                    .enums
                    .get(*enum_id)
                    .ok_or_else(|| dangling("enum", *enum_id))?;
                let index = enum_
                    .members
                    .iter()
                    .position(|candidate| &candidate.name == member)
                    .ok_or_else(|| {
                        unsupported(
                            format!("member '{}' is not in user enum '{}'", member, enum_.name),
                            span,
                        )
                    })?;
                ValueNode::new(
                    Value::Number {
                        value: index as f64,
                        text: index.to_string(),
                    },
                    span,
                )
            }
            Expr::UserCall { function, args, .. } => {
                let function = self
                    .hir
                    .functions
                    .get(*function)
                    .ok_or_else(|| dangling("function", *function))?;
                if function
                    .return_type
                    .as_ref()
                    .is_none_or(|type_name| type_name.is_void())
                {
                    return Err(unsupported(
                        format!(
                            "void function '{}' cannot be used as a value on the #119 surface",
                            function.name
                        ),
                        span,
                    ));
                }
                let bound = self.bind_inline_args(function, args, params)?;
                match &function.body {
                    hir::FunctionBody::Expression(body) => {
                        return self.lower_value_with(*body, &bound, out);
                    }
                    hir::FunctionBody::Statements(statements) => {
                        // Hoist every statement before the terminal return;
                        // the return value is inlined (reference P4/P5
                        // shape). Nested statement-bodied calls inside the
                        // body keep hoisting through the same `out`.
                        let mut body_out = Vec::new();
                        for statement in statements {
                            if let Some(Stmt::Return { value, .. }) = self.hir.stmts.get(*statement)
                            {
                                let value = value.ok_or_else(|| {
                                    let statement_span = self
                                        .hir
                                        .stmts
                                        .get(*statement)
                                        .and_then(|statement| statement.span());
                                    unsupported(
                                        format!(
                                            "function '{}' must return a value on the #119 \
                                             surface",
                                            function.name
                                        ),
                                        statement_span,
                                    )
                                })?;
                                let result = self.lower_value_with(value, &bound, &mut body_out)?;
                                out.extend(body_out);
                                return Ok(result);
                            }
                            self.lower_action_with(*statement, &mut body_out, &bound)?;
                        }
                        return Err(unsupported(
                            format!(
                                "function '{}' has no terminal return on the #119 surface",
                                function.name
                            ),
                            span,
                        ));
                    }
                }
            }
            Expr::Ternary {
                condition,
                then_value,
                else_value,
                ..
            } => ValueNode::new(
                Value::Call {
                    name: "ifThenElse".to_string(),
                    args: vec![
                        self.lower_value_with(*condition, params, out)?,
                        self.lower_value_with(*then_value, params, out)?,
                        self.lower_value_with(*else_value, params, out)?,
                    ],
                },
                span,
            ),
            Expr::Cast { value, .. } => return self.lower_value_with(*value, params, out),
            Expr::Param { name, .. } => match params.get(name) {
                Some(ParamBinding::Inline(arg)) => {
                    return self.lower_value_with(*arg, params, out);
                }
                Some(ParamBinding::Var(variable)) => {
                    let player = self
                        .target
                        .values
                        .push(ValueNode::new(Value::EventPlayer, span));
                    return Ok(self.target.values.push(ValueNode::new(
                        Value::PlayerVariable {
                            player,
                            variable: *variable,
                        },
                        span,
                    )));
                }
                None => {
                    return Err(unsupported(
                        format!(
                            "user-function parameter '{name}' reached lowering outside its \
                             function body"
                        ),
                        expression.span(),
                    ));
                }
            },
        };
        Ok(self.target.values.push(value))
    }

    fn lower_values_with(
        &mut self,
        ids: &[ExprId],
        params: &HashMap<String, ParamBinding>,
        out: &mut Vec<wir::ActionId>,
    ) -> Result<Vec<ValueId>, IrError> {
        ids.iter()
            .map(|id| self.lower_value_with(*id, params, out))
            .collect()
    }

    /// Expand a source-level macro call: bind the call arguments to the macro
    /// parameters and lower the macro body expression with that context.
    fn expand_macro_call(
        &mut self,
        macro_: MacroId,
        args: &[ExprId],
        outer: &HashMap<String, ParamBinding>,
        out: &mut Vec<wir::ActionId>,
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
        let mut bound: HashMap<String, ParamBinding> = HashMap::with_capacity(macro_.args.len());
        for (name, arg) in macro_.args.iter().zip(args.iter()) {
            bound.insert(name.clone(), ParamBinding::Inline(*arg));
        }
        // Outer parameters remain visible inside the expanded body so nested
        // macro calls keep working.
        for (name, arg) in outer {
            bound.entry(name.clone()).or_insert_with(|| arg.clone());
        }
        self.lower_value_with(*expr, &bound, out)
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

/// The kind of an indexed-assignment target array.
enum IndexedArrayKind {
    Global,
    Player,
    Other,
}

/// Classify an indexed-assignment array expression: a global array, a player
/// array (on a player expression), or another target.
fn indexed_array_kind(array: &ExprId, hir: &hir::Program) -> IndexedArrayKind {
    match hir.exprs.get(*array) {
        Some(Expr::GlobalVar { .. }) => IndexedArrayKind::Global,
        Some(Expr::PlayerVar { .. }) => IndexedArrayKind::Player,
        _ => IndexedArrayKind::Other,
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

/// The source span of an expression, when the node resolves.
fn value_span(hir: &hir::Program, id: ExprId) -> Option<Span> {
    hir.exprs.get(id).and_then(|expr| expr.span())
}
