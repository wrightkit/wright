//! The OSTW semantic phase (#118): declaration collection, cross-file name
//! resolution, type resolution, and frontend-neutral HIR construction over
//! the #117 entry-point reachable project graph.
//!
//! Ownership: the resolver lives entirely in `wright-ostw`; the produced HIR
//! is `wright_ir::hir::Program` (with narrow frontend-neutral extensions for
//! user enums, typed functions/parameters, rule priority, ternary/cast, and
//! switch/return/break/foreach/for-loop forms evidenced by the pinned
//! reference probes). Workshop actions/values/enums resolve through the
//! canonical Wright-owned Workshop catalog (via the OSTW-only source bindings
//! in [`crate::signature`]) — no OSTW game-derived table.
//!
//! Unsupported reachable boundaries (classes/`new`, `define` function
//! macros, the missing `../OSTWUtils`/`Cursor`/`Math` surfaces) fail during
//! resolution with deterministic, structured, source-located diagnostics —
//! never deferred to emission. The HIR records best-effort generic calls for
//! those boundary forms so it stays structurally valid.

use std::collections::HashMap;

use wright_ir::hir::{self, ExprId, FunctionId, GlobalVarId, PlayerVarId, StmtId, SubroutineId};

use wright_workshop::catalog::{Catalog, Kind};

use crate::cst;
use crate::diag::FrontendError;
use crate::project::Project;
use crate::signature;
use wright_ir::source::Span;

/// The canonical Wright Workshop catalog, loaded once. Workshop builtins and
/// enum domains resolve through it at the consume sites; wright-ostw ships
/// only OSTW source-name bindings ([`crate::signature`]).
fn catalog() -> &'static Catalog {
    static CATALOG: std::sync::OnceLock<Catalog> = std::sync::OnceLock::new();
    CATALOG
        .get_or_init(|| Catalog::builtin().expect("the embedded Wright Workshop catalog validates"))
}

/// The outcome of the semantic phase: validated HIR plus diagnostics.
#[derive(Debug, Clone)]
pub struct SemanticOutcome {
    /// The resolved frontend-neutral HIR, when the project loaded. Even with
    /// diagnostics the HIR is produced (boundary forms are recorded as
    /// generic calls) and validates structurally.
    pub hir: Option<hir::Program>,
    pub diagnostics: Vec<FrontendError>,
}

/// Resolve the semantic surface of a loaded project into HIR.
pub fn compile(project: &Project) -> SemanticOutcome {
    Resolver::new(project).run()
}

/// A local binding visible while resolving a body.
enum Local {
    /// A named parameter.
    Param,
    /// The foreach element: references lower to `Index(iterable, counter)`.
    ForeachElement(ExprId),
}

struct Resolver<'a> {
    project: &'a Project,
    hir: hir::Program,
    diagnostics: Vec<FrontendError>,

    global_ids: HashMap<String, GlobalVarId>,
    player_ids: HashMap<String, PlayerVarId>,
    enum_ids: HashMap<String, hir::EnumId>,
    function_ids: HashMap<String, FunctionId>,
    subroutine_ids: HashMap<String, SubroutineId>,
    constant_ids: HashMap<String, hir::ConstantId>,

    /// The rule-named void functions recorded as Workshop subroutines, in
    /// declaration order.
    pending_subroutines: Vec<cst::FunctionDecl>,
    /// Rules whose bodies resolve after declarations are collected.
    pending_rules: Vec<cst::RuleDecl>,

    /// Explicit global/player indexes seen during collection (P3b duplicate
    /// id detection).
    explicit_global_ids: HashMap<u32, Span>,
    explicit_player_ids: HashMap<u32, Span>,

    /// The current rule's event ("global"/"eachPlayer").
    current_event: String,
    /// Local bindings while resolving a body (params, foreach elements).
    locals: Vec<HashMap<String, Local>>,
}

impl<'a> Resolver<'a> {
    fn new(project: &'a Project) -> Self {
        Resolver {
            project,
            hir: hir::Program::default(),
            diagnostics: Vec::new(),
            global_ids: HashMap::new(),
            player_ids: HashMap::new(),
            enum_ids: HashMap::new(),
            function_ids: HashMap::new(),
            subroutine_ids: HashMap::new(),
            constant_ids: HashMap::new(),
            pending_subroutines: Vec::new(),
            pending_rules: Vec::new(),
            explicit_global_ids: HashMap::new(),
            explicit_player_ids: HashMap::new(),
            current_event: "global".to_string(),
            locals: Vec::new(),
        }
    }

    fn run(mut self) -> SemanticOutcome {
        // File registry: the project registry (ds.toml at id 0 then sources),
        // so every CST span keeps its FileId/provenance.
        for file in &self.project.files {
            self.hir
                .files
                .push(wright_ir::source::SourceFile::new(file.path.clone()));
        }
        self.collect_declarations();
        self.resolve_bodies();
        let diagnostics = self.diagnostics;
        SemanticOutcome {
            hir: Some(self.hir),
            diagnostics,
        }
    }

    // -- declaration collection --------------------------------------------

    fn collect_declarations(&mut self) {
        let files = self.project.files.clone();
        for file in &files {
            let Some(cst) = &file.cst else {
                continue;
            };
            let items = cst.items.clone();
            for item in &items {
                match item {
                    cst::Item::GlobalVar(decl) => self.collect_global(decl),
                    cst::Item::PlayerVar(decl) => self.collect_player(decl),
                    cst::Item::Enum(decl) => {
                        let enum_id = self.hir.enums.push(hir::EnumDecl {
                            name: decl.name.clone(),
                            span: Some(decl.span),
                            members: decl
                                .members
                                .iter()
                                .map(|member| hir::EnumMember {
                                    name: member.clone(),
                                    span: None,
                                })
                                .collect(),
                        });
                        self.enum_ids.insert(decl.name.clone(), enum_id);
                    }
                    cst::Item::Define(decl) => {
                        if decl.params.is_empty() {
                            let constant = hir::Constant {
                                name: decl.name.clone(),
                                span: Some(decl.span),
                                value: placeholder_expr(&mut self.hir, decl.span),
                            };
                            self.constant_ids
                                .insert(decl.name.clone(), self.hir.constants.push(constant));
                        } else {
                            self.diagnostics.push(FrontendError::at(
                                "ostw-unsupported",
                                "define function macros are outside the #118 semantic surface \
                                 (only the exercised constant defines are supported)",
                                decl.span,
                            ));
                        }
                    }
                    cst::Item::TypedDecl(decl) => {
                        self.collect_function(
                            decl.name.clone(),
                            Some(decl.type_name.clone()),
                            decl.params.clone().unwrap_or_default(),
                            FunctionBodySpec::Expression(decl.value.clone()),
                            decl.span,
                            None,
                        );
                    }
                    cst::Item::Function(decl) => {
                        if decl.rule_name.is_some() {
                            // Rule-named void functions are Workshop subroutines.
                            self.pending_subroutines.push(decl.clone());
                        } else {
                            self.collect_function(
                                decl.name.clone(),
                                decl.return_type.clone(),
                                decl.params.clone(),
                                FunctionBodySpec::Statements(decl.body.clone()),
                                decl.span,
                                Some(decl.name_span),
                            );
                        }
                    }
                    cst::Item::Rule(decl) => self.pending_rules.push(decl.clone()),
                    cst::Item::Class(decl) => {
                        self.diagnostics.push(FrontendError::at(
                            "ostw-unsupported",
                            format!("class '{}' is outside the #118 semantic surface", decl.name),
                            decl.span,
                        ));
                    }
                }
            }
        }
        // Register the rule-named subroutines.
        for decl in &self.pending_subroutines {
            if self.subroutine_ids.contains_key(&decl.name) {
                continue;
            }
            let subroutine = self.hir.subroutines.push(hir::Subroutine {
                name: decl.name.clone(),
                index: None,
                decl_span: Some(decl.span),
                decl_name_span: Some(decl.name_span),
                body: Some(hir::SubroutineBody {
                    span: Some(decl.span),
                    name_span: None,
                    statements: Vec::new(),
                }),
            });
            self.subroutine_ids.insert(decl.name.clone(), subroutine);
        }
    }

    fn collect_global(&mut self, decl: &cst::VarDecl) {
        let name = decl.name.clone();
        if self.global_ids.contains_key(&name) {
            self.diagnostics.push(FrontendError::at(
                "ostw-duplicate-name",
                format!("global variable '{name}' is declared more than once"),
                decl.span,
            ));
            return;
        }
        let index = decl.index.as_ref().and_then(|expr| match expr {
            cst::Expr::Number { value, .. } => Some(*value as u32),
            _ => None,
        });
        if let Some(index) = index {
            if let Some(other_span) = self.explicit_global_ids.get(&index) {
                self.diagnostics.push(FrontendError::at(
                    "ostw-duplicate-variable-id",
                    format!("The id {index} is already reserved in the global collection."),
                    *other_span,
                ));
            } else {
                self.explicit_global_ids.insert(index, decl.span);
            }
        }
        let initializer = decl
            .value
            .as_ref()
            .map(|_| placeholder_expr(&mut self.hir, decl.span));
        let id = self.hir.globals.push(hir::GlobalVar {
            name: name.clone(),
            index,
            span: Some(decl.span),
            name_span: Some(decl.name_span),
            initializer,
        });
        self.global_ids.insert(name, id);
    }

    fn collect_player(&mut self, decl: &cst::VarDecl) {
        let name = decl.name.clone();
        if self.player_ids.contains_key(&name) {
            self.diagnostics.push(FrontendError::at(
                "ostw-duplicate-name",
                format!("player variable '{name}' is declared more than once"),
                decl.span,
            ));
            return;
        }
        let index = decl.index.as_ref().and_then(|expr| match expr {
            cst::Expr::Number { value, .. } => Some(*value as u32),
            _ => None,
        });
        if let Some(index) = index {
            if let Some(other_span) = self.explicit_player_ids.get(&index) {
                self.diagnostics.push(FrontendError::at(
                    "ostw-duplicate-variable-id",
                    format!("The id {index} is already reserved in the player collection."),
                    *other_span,
                ));
            } else {
                self.explicit_player_ids.insert(index, decl.span);
            }
        }
        let initializer = decl
            .value
            .as_ref()
            .map(|_| placeholder_expr(&mut self.hir, decl.span));
        let id = self.hir.players.push(hir::PlayerVar {
            name: name.clone(),
            index,
            span: Some(decl.span),
            name_span: Some(decl.name_span),
            initializer,
        });
        self.player_ids.insert(name, id);
    }

    fn collect_function(
        &mut self,
        name: String,
        return_type: Option<cst::TypeRef>,
        _params: Vec<cst::Param>,
        body: FunctionBodySpec,
        span: Span,
        name_span: Option<Span>,
    ) {
        if self.function_ids.contains_key(&name) {
            self.diagnostics.push(FrontendError::at(
                "ostw-duplicate-name",
                format!("function '{name}' is declared more than once"),
                span,
            ));
            return;
        }
        let placeholder = match body {
            FunctionBodySpec::Expression(_) => {
                hir::FunctionBody::Expression(placeholder_expr(&mut self.hir, span))
            }
            FunctionBodySpec::Statements(_) => hir::FunctionBody::Statements(Vec::new()),
        };
        let function = self.hir.functions.push(hir::Function {
            name: name.clone(),
            params: Vec::new(), // resolved in pass 2
            return_type: return_type.as_ref().map(cst_type_to_hir),
            body: placeholder,
            span: Some(span),
            name_span,
        });
        self.function_ids.insert(name, function);
    }

    // -- pass 2: resolve bodies --------------------------------------------

    fn resolve_bodies(&mut self) {
        self.resolve_constant_values();
        self.assign_variable_indexes();
        self.resolve_initializers();
        self.resolve_functions();
        self.resolve_subroutine_bodies();
        self.resolve_rules();
    }

    fn resolve_constant_values(&mut self) {
        let files = self.project.files.clone();
        for file in &files {
            let Some(cst) = &file.cst else {
                continue;
            };
            let items = cst.items.clone();
            for item in &items {
                if let cst::Item::Define(decl) = item {
                    if decl.params.is_empty() {
                        if let Some(id) = self.constant_ids.get(&decl.name).copied() {
                            let value = self.resolve_expr(&decl.value);
                            if let Some(constant) = self.hir.constants.get_mut(id) {
                                constant.value = value;
                            }
                        }
                    }
                }
            }
        }
    }

    fn assign_variable_indexes(&mut self) {
        let globals_len = self.hir.globals.len();
        let mut used: Vec<u32> = self
            .hir
            .globals
            .iter()
            .filter_map(|global| global.index)
            .collect();
        for id in 0..globals_len {
            let id = GlobalVarId::from_index(id);
            if self
                .hir
                .globals
                .get(id)
                .is_some_and(|global| global.index.is_none())
            {
                let mut candidate = 0u32;
                while used.contains(&candidate) {
                    candidate += 1;
                }
                if let Some(global) = self.hir.globals.get_mut(id) {
                    global.index = Some(candidate);
                }
                used.push(candidate);
            }
        }
        let players_len = self.hir.players.len();
        let mut used: Vec<u32> = self
            .hir
            .players
            .iter()
            .filter_map(|player| player.index)
            .collect();
        for id in 0..players_len {
            let id = PlayerVarId::from_index(id);
            if self
                .hir
                .players
                .get(id)
                .is_some_and(|player| player.index.is_none())
            {
                let mut candidate = 0u32;
                while used.contains(&candidate) {
                    candidate += 1;
                }
                if let Some(player) = self.hir.players.get_mut(id) {
                    player.index = Some(candidate);
                }
                used.push(candidate);
            }
        }
    }

    fn resolve_initializers(&mut self) {
        let files = self.project.files.clone();
        for file in &files {
            let Some(cst) = &file.cst else {
                continue;
            };
            let items = cst.items.clone();
            for item in &items {
                match item {
                    cst::Item::GlobalVar(decl) => {
                        if let Some(id) = self.global_ids.get(&decl.name).copied() {
                            if let Some(value) = &decl.value {
                                let expr = self.resolve_expr(value);
                                if let Some(global) = self.hir.globals.get_mut(id) {
                                    global.initializer = Some(expr);
                                }
                            }
                        }
                    }
                    cst::Item::PlayerVar(decl) => {
                        if let Some(id) = self.player_ids.get(&decl.name).copied() {
                            if let Some(value) = &decl.value {
                                let expr = self.resolve_expr(value);
                                if let Some(player) = self.hir.players.get_mut(id) {
                                    player.initializer = Some(expr);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn resolve_functions(&mut self) {
        // Pass A: resolve every function's parameters (and defaults) first,
        // so a call in one body sees the callee's arity/params.
        let files = self.project.files.clone();
        let mut specs: Vec<(FunctionId, Vec<cst::Param>, FunctionBodySpec)> = Vec::new();
        for file in &files {
            let Some(cst) = &file.cst else {
                continue;
            };
            let items = cst.items.clone();
            for item in &items {
                match item {
                    cst::Item::TypedDecl(decl) => {
                        if let Some(id) = self.function_ids.get(&decl.name).copied() {
                            let mut hir_params = Vec::new();
                            for param in decl.params.clone().unwrap_or_default().iter() {
                                let default =
                                    param.default.as_ref().map(|expr| self.resolve_expr(expr));
                                hir_params.push(hir::Param {
                                    type_name: param.type_name.as_ref().map(cst_type_to_hir),
                                    name: param.name.clone(),
                                    default,
                                    span: Some(param.span),
                                });
                            }
                            if let Some(function) = self.hir.functions.get_mut(id) {
                                function.params = hir_params;
                            }
                            specs.push((
                                id,
                                decl.params.clone().unwrap_or_default(),
                                FunctionBodySpec::Expression(decl.value.clone()),
                            ));
                        }
                    }
                    cst::Item::Function(decl) => {
                        // Non-rule-named void functions are user functions.
                        let id = if decl.rule_name.is_none() {
                            self.function_ids.get(&decl.name).copied()
                        } else {
                            None
                        };
                        if let Some(id) = id {
                            let mut hir_params = Vec::new();
                            for param in &decl.params {
                                let default =
                                    param.default.as_ref().map(|expr| self.resolve_expr(expr));
                                hir_params.push(hir::Param {
                                    type_name: param.type_name.as_ref().map(cst_type_to_hir),
                                    name: param.name.clone(),
                                    default,
                                    span: Some(param.span),
                                });
                            }
                            if let Some(function) = self.hir.functions.get_mut(id) {
                                function.params = hir_params;
                            }
                            specs.push((
                                id,
                                decl.params.clone(),
                                FunctionBodySpec::Statements(decl.body.clone()),
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        // Pass B: resolve bodies with the parameters in scope.
        for (id, params, body) in specs {
            self.locals.push(HashMap::new());
            for param in &params {
                self.locals
                    .last_mut()
                    .unwrap()
                    .insert(param.name.clone(), Local::Param);
            }
            match body {
                FunctionBodySpec::Expression(expr) => {
                    let value = self.resolve_expr(&expr);
                    if let Some(function) = self.hir.functions.get_mut(id) {
                        function.body = hir::FunctionBody::Expression(value);
                    }
                }
                FunctionBodySpec::Statements(statements) => {
                    let resolved = self.resolve_statements(&statements);
                    if let Some(function) = self.hir.functions.get_mut(id) {
                        function.body = hir::FunctionBody::Statements(resolved);
                    }
                }
            }
            self.locals.pop();
        }
    }

    fn resolve_subroutine_bodies(&mut self) {
        let subroutines = self.pending_subroutines.clone();
        for decl in subroutines {
            if let Some(id) = self.subroutine_ids.get(&decl.name).copied() {
                let statements = self.resolve_statements(&decl.body);
                if let Some(subroutine) = self.hir.subroutines.get_mut(id) {
                    if let Some(body) = &mut subroutine.body {
                        body.statements = statements;
                    }
                }
            }
        }
    }

    fn resolve_rules(&mut self) {
        let rules = self.pending_rules.clone();
        for decl in rules {
            self.resolve_rule(&decl);
        }
    }

    fn resolve_rule(&mut self, decl: &cst::RuleDecl) {
        let event_name = match &decl.event {
            Some(cst::Expr::Member { receiver, name, .. }) => match receiver.as_ref() {
                cst::Expr::Ident {
                    name: receiver_name,
                    ..
                } if receiver_name == "Event" => match name.as_str() {
                    "OngoingPlayer" => "eachPlayer",
                    other => {
                        self.diagnostics.push(FrontendError::at(
                            "ostw-unsupported-event",
                            format!("event 'Event.{other}' is outside the #118 semantic surface"),
                            decl.span,
                        ));
                        "global"
                    }
                },
                _ => "global",
            },
            _ => "global",
        };
        let previous_event = self.current_event.clone();
        self.current_event = event_name.to_string();

        let priority = decl.priority.as_ref().and_then(|expr| match expr {
            cst::Expr::Number { value, .. } => Some(*value as i32),
            cst::Expr::Unary {
                op: cst::UnaryOp::Negate,
                operand,
                ..
            } => match operand.as_ref() {
                cst::Expr::Number { value, .. } => Some(-(*value as i32)),
                _ => None,
            },
            _ => None,
        });

        let conditions = decl
            .conditions
            .iter()
            .map(|condition| self.resolve_expr(condition))
            .collect();
        let actions = self.resolve_statements(&decl.body);
        self.current_event = previous_event;

        self.hir.rules.push(hir::Rule {
            name: decl.name.clone().unwrap_or_default(),
            span: Some(decl.span),
            name_span: decl.name_span,
            disabled: decl.disabled,
            event: hir::Event {
                name: event_name.to_string(),
                args: Vec::new(),
                span: Some(decl.span),
            },
            priority,
            conditions,
            actions,
        });
    }

    // -- statements ---------------------------------------------------------

    fn resolve_statements(&mut self, statements: &[cst::Stmt]) -> Vec<StmtId> {
        let mut out = Vec::new();
        for statement in statements {
            self.resolve_stmt_into(statement, &mut out);
        }
        out
    }

    fn resolve_stmt_into(&mut self, statement: &cst::Stmt, out: &mut Vec<StmtId>) {
        match statement {
            cst::Stmt::Expr { expr, span } => {
                // A call to a rule-named subroutine is a subroutine call;
                // check before expression resolution so it never resolves as
                // a value.
                if let cst::Expr::Call { callee, .. } = expr {
                    if let cst::Expr::Ident { name, .. } = callee.as_ref() {
                        if let Some(subroutine) = self.subroutine_ids.get(name) {
                            out.push(self.hir.stmts.push(hir::Stmt::CallSubroutine {
                                subroutine: *subroutine,
                                span: Some(*span),
                                callee_span: None,
                            }));
                            return;
                        }
                    }
                }
                let resolved = self.resolve_expr(expr);
                out.push(self.hir.stmts.push(hir::Stmt::Expr {
                    expr: resolved,
                    span: Some(*span),
                }));
            }
            cst::Stmt::Assign {
                target,
                op,
                value,
                span,
            } => {
                let target_expr = self.resolve_expr(target);
                let value_expr = self.resolve_expr(value);
                let value_expr = if *op == cst::AssignOp::Assign {
                    value_expr
                } else {
                    let binary_op = match op {
                        cst::AssignOp::AddAssign => hir::BinaryOp::Add,
                        cst::AssignOp::SubtractAssign => hir::BinaryOp::Subtract,
                        cst::AssignOp::MultiplyAssign => hir::BinaryOp::Multiply,
                        cst::AssignOp::DivideAssign => hir::BinaryOp::Divide,
                        cst::AssignOp::ModuloAssign => hir::BinaryOp::Modulo,
                        cst::AssignOp::Assign => unreachable!(),
                    };
                    self.hir.exprs.push(hir::Expr::Binary {
                        op: binary_op,
                        left: target_expr,
                        right: value_expr,
                        span: Some(*span),
                    })
                };
                out.push(self.hir.stmts.push(hir::Stmt::Assign {
                    target: target_expr,
                    value: value_expr,
                    span: Some(*span),
                }));
            }
            cst::Stmt::If {
                branches,
                else_body,
                span,
            } => {
                let mut hir_branches = Vec::new();
                for branch in branches {
                    let condition = self.resolve_expr(&branch.condition);
                    let body = self.resolve_statements(&branch.body);
                    hir_branches.push(hir::IfBranch { condition, body });
                }
                let hir_else = else_body.as_ref().map(|body| self.resolve_statements(body));
                out.push(self.hir.stmts.push(hir::Stmt::If {
                    branches: hir_branches,
                    else_body: hir_else,
                    span: Some(*span),
                }));
            }
            cst::Stmt::For {
                init,
                condition,
                increment,
                body,
                span,
            } => {
                // C-style for: the emitted form is
                // `For Global Variable(i, start, condition, step)` (P5b).
                let (variable, start) = match init {
                    Some(cst::Expr::Assign { target, value, .. }) => match target.as_ref() {
                        cst::Expr::Ident { name, .. } => match self.global_ids.get(name).copied() {
                            Some(variable) => {
                                let start = self.resolve_expr(value);
                                (variable, start)
                            }
                            None => {
                                self.diagnostics.push(FrontendError::at(
                                    "ostw-unsupported",
                                    "for-loop variable must be a global variable",
                                    *span,
                                ));
                                return;
                            }
                        },
                        _ => {
                            self.diagnostics.push(FrontendError::at(
                                "ostw-unsupported",
                                "for-loop initializer must assign a global variable",
                                *span,
                            ));
                            return;
                        }
                    },
                    _ => {
                        self.diagnostics.push(FrontendError::at(
                            "ostw-unsupported",
                            "for-loop initializer must be `variable = value`",
                            *span,
                        ));
                        return;
                    }
                };
                let condition = condition.as_ref().map(|expr| self.resolve_expr(expr));
                let step = increment.as_ref().map(|expr| self.resolve_expr(expr));
                let body = self.resolve_statements(body);
                out.push(self.hir.stmts.push(hir::Stmt::CFor {
                    variable,
                    start: Some(start),
                    condition,
                    step,
                    body,
                    span: Some(*span),
                }));
            }
            cst::Stmt::Foreach {
                var_type: _,
                var,
                iterable,
                body,
                span,
            } => {
                let iterable_expr = self.resolve_expr(iterable);
                // Allocate a global counter for the loop variable (the
                // reference emits `For Global Variable(x, 0, Count Of(arr), 1)`
                // with the element rewritten to `Value In Array(arr, x)`).
                let counter = self.hir.globals.push(hir::GlobalVar {
                    name: var.clone(),
                    index: None,
                    span: Some(*span),
                    name_span: None,
                    initializer: None,
                });
                self.global_ids.insert(var.clone(), counter);
                let counter_ref = self.hir.exprs.push(hir::Expr::GlobalVar {
                    variable: counter,
                    span: Some(*span),
                });
                let element = self.hir.exprs.push(hir::Expr::Index {
                    array: iterable_expr,
                    index: counter_ref,
                    span: Some(*span),
                });
                self.locals.push(HashMap::new());
                self.locals
                    .last_mut()
                    .unwrap()
                    .insert(var.clone(), Local::ForeachElement(element));
                let body = self.resolve_statements(body);
                self.locals.pop();
                out.push(self.hir.stmts.push(hir::Stmt::Foreach {
                    variable: counter,
                    iterable: iterable_expr,
                    body,
                    span: Some(*span),
                }));
            }
            cst::Stmt::While {
                condition,
                body,
                span,
            } => {
                let condition = self.resolve_expr(condition);
                let body = self.resolve_statements(body);
                out.push(self.hir.stmts.push(hir::Stmt::While {
                    condition,
                    body,
                    span: Some(*span),
                }));
            }
            cst::Stmt::Switch { value, cases, span } => {
                let value = self.resolve_expr(value);
                let mut hir_cases = Vec::new();
                for case in cases {
                    let case_value = case.value.as_ref().map(|expr| self.resolve_expr(expr));
                    let body = self.resolve_statements(&case.body);
                    hir_cases.push(hir::SwitchCase {
                        value: case_value,
                        body,
                        span: Some(case.span),
                    });
                }
                out.push(self.hir.stmts.push(hir::Stmt::Switch {
                    value,
                    cases: hir_cases,
                    span: Some(*span),
                }));
            }
            cst::Stmt::Return { value, span } => {
                let value = value.as_ref().map(|expr| self.resolve_expr(expr));
                out.push(self.hir.stmts.push(hir::Stmt::Return {
                    value,
                    span: Some(*span),
                }));
            }
            cst::Stmt::Break { span } => {
                out.push(self.hir.stmts.push(hir::Stmt::Break { span: Some(*span) }));
            }
            cst::Stmt::Continue { span } => {
                out.push(
                    self.hir
                        .stmts
                        .push(hir::Stmt::Continue { span: Some(*span) }),
                );
            }
            cst::Stmt::Block { body, span } => {
                // Flatten a bare block into the enclosing statement list.
                let _ = span;
                for statement in body {
                    self.resolve_stmt_into(statement, out);
                }
            }
            cst::Stmt::LocalDefine { span, .. } | cst::Stmt::LocalDecl { span, .. } => {
                self.diagnostics.push(FrontendError::at(
                    "ostw-unsupported",
                    "local define/typed declarations are outside the #118 semantic surface",
                    *span,
                ));
            }
        }
    }

    // -- expressions --------------------------------------------------------

    fn resolve_expr(&mut self, expression: &cst::Expr) -> ExprId {
        match expression {
            cst::Expr::Number { value, text, span } => self.hir.exprs.push(hir::Expr::Number {
                value: *value,
                text: text.clone(),
                span: Some(*span),
            }),
            cst::Expr::String { value, span } | cst::Expr::VerbatimString { value, span } => {
                self.hir.exprs.push(hir::Expr::String {
                    value: value.clone(),
                    span: Some(*span),
                })
            }
            cst::Expr::Bool { value, span } => self.hir.exprs.push(hir::Expr::Bool {
                value: *value,
                span: Some(*span),
            }),
            cst::Expr::Null { span } => self.hir.exprs.push(hir::Expr::Null { span: Some(*span) }),
            cst::Expr::Array { elements, span } => {
                let elements = elements
                    .iter()
                    .map(|element| self.resolve_expr(element))
                    .collect();
                self.hir.exprs.push(hir::Expr::Array {
                    elements,
                    span: Some(*span),
                })
            }
            cst::Expr::Ident { name, span } => self.resolve_ident(name, *span),
            cst::Expr::Member {
                receiver,
                name,
                span,
            } => self.resolve_member(receiver, name, *span),
            cst::Expr::Call { callee, args, span } => self.resolve_call(callee, args, *span),
            cst::Expr::Index { array, index, span } => {
                let array = self.resolve_expr(array);
                let index = self.resolve_expr(index);
                self.hir.exprs.push(hir::Expr::Index {
                    array,
                    index,
                    span: Some(*span),
                })
            }
            cst::Expr::FormatString { format, args, span } => {
                let text = match format.as_ref() {
                    cst::Expr::String { value, .. } | cst::Expr::VerbatimString { value, .. } => {
                        value.clone()
                    }
                    _ => String::new(),
                };
                let args = args.iter().map(|arg| self.resolve_expr(arg)).collect();
                self.hir.exprs.push(hir::Expr::Format {
                    text,
                    args,
                    span: Some(*span),
                })
            }
            cst::Expr::Cast {
                type_name,
                value,
                span,
            } => {
                let value = self.resolve_expr(value);
                self.hir.exprs.push(hir::Expr::Cast {
                    type_name: cst_type_to_hir(type_name),
                    value,
                    span: Some(*span),
                })
            }
            cst::Expr::Unary { op, operand, span } => {
                let hir_op = match op {
                    cst::UnaryOp::Negate => hir::UnaryOp::Negate,
                    cst::UnaryOp::Not => hir::UnaryOp::Not,
                };
                let operand = self.resolve_expr(operand);
                self.hir.exprs.push(hir::Expr::Unary {
                    op: hir_op,
                    operand,
                    span: Some(*span),
                })
            }
            cst::Expr::Binary {
                op,
                left,
                right,
                span,
            } => {
                let hir_op = match op {
                    cst::BinaryOp::Add => hir::BinaryOp::Add,
                    cst::BinaryOp::Subtract => hir::BinaryOp::Subtract,
                    cst::BinaryOp::Multiply => hir::BinaryOp::Multiply,
                    cst::BinaryOp::Divide => hir::BinaryOp::Divide,
                    cst::BinaryOp::Modulo => hir::BinaryOp::Modulo,
                    cst::BinaryOp::Power => hir::BinaryOp::Power,
                    cst::BinaryOp::Equal => hir::BinaryOp::Equal,
                    cst::BinaryOp::NotEqual => hir::BinaryOp::NotEqual,
                    cst::BinaryOp::Less => hir::BinaryOp::Less,
                    cst::BinaryOp::LessEqual => hir::BinaryOp::LessEqual,
                    cst::BinaryOp::Greater => hir::BinaryOp::Greater,
                    cst::BinaryOp::GreaterEqual => hir::BinaryOp::GreaterEqual,
                    cst::BinaryOp::And => hir::BinaryOp::And,
                    cst::BinaryOp::Or => hir::BinaryOp::Or,
                };
                let left = self.resolve_expr(left);
                let right = self.resolve_expr(right);
                self.hir.exprs.push(hir::Expr::Binary {
                    op: hir_op,
                    left,
                    right,
                    span: Some(*span),
                })
            }
            cst::Expr::Ternary {
                condition,
                then_value,
                else_value,
                span,
            } => {
                let condition = self.resolve_expr(condition);
                let then_value = self.resolve_expr(then_value);
                let else_value = self.resolve_expr(else_value);
                self.hir.exprs.push(hir::Expr::Ternary {
                    condition,
                    then_value,
                    else_value,
                    span: Some(*span),
                })
            }
            cst::Expr::New {
                type_name,
                args,
                span,
            } => {
                self.diagnostics.push(FrontendError::at(
                    "ostw-unsupported",
                    format!(
                        "class instantiation ('new {type_name}') is outside the #118 semantic surface"
                    ),
                    *span,
                ));
                let args = args.iter().map(|arg| self.resolve_call_arg(arg)).collect();
                self.hir.exprs.push(hir::Expr::Call {
                    name: format!("new {type_name}"),
                    args,
                    span: Some(*span),
                })
            }
            cst::Expr::Assign { span, .. } => {
                // Assignment expressions only occur in for-loop headers, which
                // resolve_stmt handles directly; reject elsewhere.
                self.diagnostics.push(FrontendError::at(
                    "ostw-unsupported",
                    "assignment expressions outside for-loop headers are unsupported",
                    *span,
                ));
                placeholder_expr(&mut self.hir, *span)
            }
            cst::Expr::Postfix { op, operand, span } => {
                self.diagnostics.push(FrontendError::at(
                    "ostw-unsupported",
                    format!(
                        "postfix '{}' is outside the #118 semantic surface",
                        match op {
                            cst::PostfixOp::Increment => "++",
                            cst::PostfixOp::Decrement => "--",
                        }
                    ),
                    *span,
                ));
                self.resolve_expr(operand)
            }
        }
    }

    fn resolve_ident(&mut self, name: &str, span: Span) -> ExprId {
        for scope in self.locals.iter().rev() {
            if let Some(local) = scope.get(name) {
                return match local {
                    Local::ForeachElement(expr) => *expr,
                    Local::Param => self.hir.exprs.push(hir::Expr::Param {
                        name: name.to_string(),
                        span: Some(span),
                    }),
                };
            }
        }
        if let Some(id) = self.global_ids.get(name) {
            return self.hir.exprs.push(hir::Expr::GlobalVar {
                variable: *id,
                span: Some(span),
            });
        }
        if let Some(id) = self.constant_ids.get(name) {
            return self.hir.exprs.push(hir::Expr::Constant {
                constant: *id,
                span: Some(span),
            });
        }
        if let Some(id) = self.function_ids.get(name) {
            // A zero-parameter typed declaration used as a value.
            return self.hir.exprs.push(hir::Expr::UserCall {
                function: *id,
                args: Vec::new(),
                span: Some(span),
            });
        }
        if let Some(id) = self.player_ids.get(name) {
            // A bare player variable in a player rule: `EventPlayer().name`.
            let player = self
                .hir
                .exprs
                .push(hir::Expr::EventPlayer { span: Some(span) });
            return self.hir.exprs.push(hir::Expr::PlayerVar {
                player,
                variable: *id,
                span: Some(span),
            });
        }
        if matches!(name, "Math" | "Cursor" | "Diagnostics") {
            self.diagnostics.push(FrontendError::at(
                "ostw-unsupported",
                format!(
                    "'{name}' comes from outside the committed protect-ban closure \
                     (#118 boundary); its members cannot be resolved"
                ),
                span,
            ));
        } else {
            self.diagnostics.push(FrontendError::at(
                "ostw-unknown-name",
                format!("no variable or type by the name of '{name}' exists in the project"),
                span,
            ));
        }
        self.hir.exprs.push(hir::Expr::Call {
            name: name.to_string(),
            args: Vec::new(),
            span: Some(span),
        })
    }

    fn resolve_member(&mut self, receiver: &cst::Expr, name: &str, span: Span) -> ExprId {
        // `Type.Member` on a known builtin enum domain.
        if let cst::Expr::Ident {
            name: receiver_name,
            ..
        } = receiver
        {
            if let Some(binding) = signature::enum_domain(receiver_name) {
                // The OSTW binding maps the source member name to its canonical
                // catalog member id; the catalog is the authority on existence.
                // The HIR carries the canonical domain and member ids (#119),
                // so emission resolves spellings purely through the catalog.
                let canonical_member = binding
                    .members
                    .iter()
                    .find(|(source, _)| *source == name)
                    .map(|(_, canonical)| *canonical);
                let known_member = canonical_member.is_some_and(|canonical| {
                    catalog().enum_domain(binding.domain).is_some_and(|domain| {
                        domain
                            .members
                            .iter()
                            .any(|member| member.member == canonical)
                    })
                });
                if !known_member {
                    self.diagnostics.push(FrontendError::at(
                        "ostw-unknown-enum-member",
                        format!("'{receiver_name}' has no member '{name}'"),
                        span,
                    ));
                }
                return self.hir.exprs.push(hir::Expr::Enum {
                    value_type: binding.domain.to_string(),
                    value: canonical_member.unwrap_or(name).to_string(),
                    span: Some(span),
                });
            }
            if let Some(enum_id) = self.enum_ids.get(receiver_name) {
                let has_member = self
                    .hir
                    .enums
                    .get(*enum_id)
                    .map(|enum_| enum_.members.iter().any(|member| member.name == name))
                    .unwrap_or(false);
                if !has_member {
                    self.diagnostics.push(FrontendError::at(
                        "ostw-unknown-enum-member",
                        format!("'{receiver_name}' has no member '{name}'"),
                        span,
                    ));
                }
                return self.hir.exprs.push(hir::Expr::UserEnum {
                    enum_id: *enum_id,
                    member: name.to_string(),
                    span: Some(span),
                });
            }
        }
        // Player-variable receiver access (`EventPlayer().p`,
        // `LocalPlayer().cursor`, `AllPlayers().isReady`, `players[i].x`).
        if let Some(variable) = self.player_ids.get(name).copied() {
            let player = self.resolve_expr(receiver);
            return self.hir.exprs.push(hir::Expr::PlayerVar {
                player,
                variable,
                span: Some(span),
            });
        }
        // Unresolved module/member boundaries (Math, Cursor).
        if let cst::Expr::Ident {
            name: receiver_name,
            ..
        } = receiver
        {
            if matches!(receiver_name.as_str(), "Math" | "Cursor" | "Diagnostics") {
                self.diagnostics.push(FrontendError::at(
                    "ostw-unsupported",
                    format!(
                        "'{receiver_name}.{name}' comes from outside the committed protect-ban \
                         closure (#118 boundary); its members cannot be resolved"
                    ),
                    span,
                ));
                return self.hir.exprs.push(hir::Expr::Call {
                    name: format!("{receiver_name}.{name}"),
                    args: Vec::new(),
                    span: Some(span),
                });
            }
        }
        self.diagnostics.push(FrontendError::at(
            "ostw-unknown-member",
            format!("cannot resolve member '{name}' on this receiver"),
            span,
        ));
        let receiver = self.resolve_expr(receiver);
        self.hir.exprs.push(hir::Expr::ReceiverCall {
            receiver,
            name: name.to_string(),
            args: Vec::new(),
            span: Some(span),
        })
    }

    fn resolve_call(&mut self, callee: &cst::Expr, args: &[cst::CallArg], span: Span) -> ExprId {
        match callee {
            cst::Expr::Ident { name, .. } => {
                // The EventPlayer/LocalPlayer restricted-value check is
                // call-site/inlining dependent (the pinned reference flags
                // direct uses in global rules but not protect-ban's
                // function-argument uses); it is deferred to #119 where the
                // reference's exact behavior can be matched.
                if let Some(function) = self.function_ids.get(name).copied() {
                    let ordered = self.bind_function_args(function, args, span);
                    return self.hir.exprs.push(hir::Expr::UserCall {
                        function,
                        args: ordered,
                        span: Some(span),
                    });
                }
                if name == "Vector" && args.len() == 3 {
                    // Reuse the frontend-neutral Vector value node for the
                    // Workshop `Vector(x, y, z)` value.
                    let x = self.resolve_call_arg(&args[0]);
                    let y = self.resolve_call_arg(&args[1]);
                    let z = self.resolve_call_arg(&args[2]);
                    return self.hir.exprs.push(hir::Expr::Vector {
                        x,
                        y,
                        z,
                        span: Some(span),
                    });
                }
                if let Some((kind, id)) = signature::builtin(name) {
                    // Canonical param order/spellings come from the catalog;
                    // the binding only supplies the OSTW source identity.
                    // The HIR call name is the canonical catalog id, so the
                    // shared HIR → WIR → emission pipeline resolves
                    // presentation spellings purely through the catalog
                    // (#119); only genuinely OSTW-specific source names stay
                    // in `signature.rs`.
                    let entry = catalog().entry(kind, id);
                    let params = entry.map(|entry| entry.params.clone()).unwrap_or_default();
                    let defaults = entry
                        .map(|entry| entry.param_defaults.clone())
                        .unwrap_or_default();
                    let ordered = self.bind_builtin_args(name, &params, &defaults, args, span);
                    return self.hir.exprs.push(hir::Expr::Call {
                        name: id.to_string(),
                        args: ordered,
                        span: Some(span),
                    });
                }
                if self.subroutine_ids.contains_key(name) {
                    self.diagnostics.push(FrontendError::at(
                        "ostw-unknown-value",
                        format!("'{name}' is a subroutine and cannot be used as a value"),
                        span,
                    ));
                } else {
                    self.diagnostics.push(FrontendError::at(
                        "ostw-unknown-value",
                        format!("no function or builtin by the name of '{name}' exists"),
                        span,
                    ));
                }
                let args = args.iter().map(|arg| self.resolve_call_arg(arg)).collect();
                self.hir.exprs.push(hir::Expr::Call {
                    name: name.clone(),
                    args,
                    span: Some(span),
                })
            }
            cst::Expr::Member { receiver, name, .. } => {
                self.resolve_receiver_call(receiver, name, args, span)
            }
            other => {
                let receiver = self.resolve_expr(other);
                let args = args.iter().map(|arg| self.resolve_call_arg(arg)).collect();
                self.hir.exprs.push(hir::Expr::ReceiverCall {
                    receiver,
                    name: String::new(),
                    args,
                    span: Some(span),
                })
            }
        }
    }

    fn resolve_receiver_call(
        &mut self,
        receiver: &cst::Expr,
        name: &str,
        args: &[cst::CallArg],
        span: Span,
    ) -> ExprId {
        let receiver_name = match receiver {
            cst::Expr::Ident { name, .. } => Some(name.as_str()),
            _ => None,
        };
        let is_boundary = match receiver_name {
            Some(receiver_name) => {
                matches!(receiver_name, "Math" | "Cursor" | "Diagnostics")
                    || self.player_ids.contains_key(receiver_name)
            }
            None => false,
        };
        if is_boundary && !name.is_empty() {
            self.diagnostics.push(FrontendError::at(
                "ostw-unsupported",
                format!(
                    "member call '{}.{}' is outside the #118 semantic surface \
                     (missing-import/Cursor/Math boundary)",
                    receiver_name.unwrap_or("?"),
                    name
                ),
                span,
            ));
            let mut lowered = vec![self.resolve_expr(receiver)];
            lowered.extend(args.iter().map(|arg| self.resolve_call_arg(arg)));
            return self.hir.exprs.push(hir::Expr::Call {
                name: format!("{}.{}", receiver_name.unwrap_or("?"), name),
                args: lowered,
                span: Some(span),
            });
        }
        let mut lowered = vec![self.resolve_expr(receiver)];
        lowered.extend(args.iter().map(|arg| self.resolve_call_arg(arg)));
        self.hir.exprs.push(hir::Expr::ReceiverCall {
            receiver: lowered[0],
            name: name.to_string(),
            args: lowered[1..].to_vec(),
            span: Some(span),
        })
    }

    fn resolve_call_arg(&mut self, arg: &cst::CallArg) -> ExprId {
        match arg {
            cst::CallArg::Positional { value, .. } | cst::CallArg::Named { value, .. } => {
                self.resolve_expr(value)
            }
        }
    }

    /// Bind positional + named arguments against a builtin's canonical param
    /// order (the catalog owns the canonical param names and Wright-owned
    /// default values; probe evidence P6/P6b): named args reorder to the
    /// canonical order and omitted gaps resolve the catalog default value
    /// (`paramDefaults`), matching the reference's call-site default filling
    /// (#119). Slots without a declared default keep the zero literal.
    fn bind_builtin_args(
        &mut self,
        name: &str,
        params: &[String],
        defaults: &[Option<String>],
        args: &[cst::CallArg],
        span: Span,
    ) -> Vec<ExprId> {
        let mut slots: Vec<Option<ExprId>> = vec![None; params.len()];
        let mut positional_index = 0usize;
        for arg in args {
            match arg {
                cst::CallArg::Positional { value, .. } => {
                    if positional_index < slots.len() {
                        slots[positional_index] = Some(self.resolve_expr(value));
                        positional_index += 1;
                    } else {
                        let _ = self.resolve_expr(value);
                        self.diagnostics.push(FrontendError::at(
                            "ostw-arity",
                            format!("'{name}' takes at most {} arguments", slots.len()),
                            span,
                        ));
                    }
                }
                cst::CallArg::Named {
                    name: arg_name,
                    value,
                    ..
                } => {
                    let Some(slot) = params.iter().position(|param| param.as_str() == arg_name)
                    else {
                        let _ = self.resolve_expr(value);
                        self.diagnostics.push(FrontendError::at(
                            "ostw-unknown-argument",
                            format!("'{name}' has no argument named '{arg_name}'"),
                            span,
                        ));
                        continue;
                    };
                    if slots[slot].is_some() {
                        self.diagnostics.push(FrontendError::at(
                            "ostw-duplicate-argument",
                            format!("argument '{arg_name}' is supplied more than once"),
                            span,
                        ));
                    }
                    slots[slot] = Some(self.resolve_expr(value));
                }
            }
        }
        slots
            .into_iter()
            .enumerate()
            .map(|(index, slot)| {
                slot.unwrap_or_else(|| match defaults.get(index).and_then(Option::as_deref) {
                    Some(default) => self.resolve_catalog_default(name, default, span),
                    None => zero_expr(&mut self.hir),
                })
            })
            .collect()
    }

    /// Resolve a catalog `paramDefaults` value into HIR: `null`, a numeric
    /// literal, `Domain.MEMBER` (a builtin enum member through the catalog),
    /// or a catalog value id resolved as a zero-argument call.
    fn resolve_catalog_default(&mut self, call_name: &str, default: &str, span: Span) -> ExprId {
        if default == "null" {
            return self.hir.exprs.push(hir::Expr::Null { span: Some(span) });
        }
        if let Ok(number) = default.parse::<f64>() {
            return self.hir.exprs.push(hir::Expr::Number {
                value: number,
                text: default.to_string(),
                span: Some(span),
            });
        }
        if let Some((domain, member)) = default.split_once('.') {
            if catalog().enum_domain(domain).is_some() {
                return self.hir.exprs.push(hir::Expr::Enum {
                    value_type: domain.to_string(),
                    value: member.to_string(),
                    span: Some(span),
                });
            }
        }
        if let Some(entry) = catalog().entry(Kind::Value, default) {
            // A value-id default resolves as a call with the entry's own
            // defaults filled (e.g. `allPlayers` -> `allPlayers(Team.ALL)`,
            // matching the reference's call-site filling).
            let args = entry
                .param_defaults
                .iter()
                .map(|slot| match slot {
                    Some(slot) => self.resolve_catalog_default(default, slot, span),
                    None => zero_expr(&mut self.hir),
                })
                .collect();
            return self.hir.exprs.push(hir::Expr::Call {
                name: default.to_string(),
                args,
                span: Some(span),
            });
        }
        self.diagnostics.push(FrontendError::at(
            "ostw-default",
            format!(
                "catalog default '{default}' for '{call_name}' is not resolvable \
                 (expected null, a number, Domain.MEMBER, or a catalog value id)"
            ),
            span,
        ));
        zero_expr(&mut self.hir)
    }

    /// Bind arguments against a user function's parameters (positional then
    /// named, then defaults), matching the reference (probe P6: user
    /// functions fill defaults, e.g. `userCall(C: 9, A: 1)` -> B defaults).
    fn bind_function_args(
        &mut self,
        function: FunctionId,
        args: &[cst::CallArg],
        span: Span,
    ) -> Vec<ExprId> {
        let param_names: Vec<String> = self
            .hir
            .functions
            .get(function)
            .map(|f| f.params.iter().map(|p| p.name.clone()).collect())
            .unwrap_or_default();
        let mut slots: Vec<Option<ExprId>> = vec![None; param_names.len()];
        let mut positional_index = 0usize;
        for arg in args {
            match arg {
                cst::CallArg::Positional { value, .. } => {
                    if positional_index < slots.len() {
                        slots[positional_index] = Some(self.resolve_expr(value));
                        positional_index += 1;
                    } else {
                        let _ = self.resolve_expr(value);
                        self.diagnostics.push(FrontendError::at(
                            "ostw-arity",
                            "too many arguments for function",
                            span,
                        ));
                    }
                }
                cst::CallArg::Named {
                    name: arg_name,
                    value,
                    ..
                } => {
                    let Some(slot) = param_names.iter().position(|param| param == arg_name) else {
                        let _ = self.resolve_expr(value);
                        self.diagnostics.push(FrontendError::at(
                            "ostw-unknown-argument",
                            format!("function has no argument named '{arg_name}'"),
                            span,
                        ));
                        continue;
                    };
                    if slots[slot].is_some() {
                        self.diagnostics.push(FrontendError::at(
                            "ostw-duplicate-argument",
                            format!("argument '{arg_name}' is supplied more than once"),
                            span,
                        ));
                    }
                    slots[slot] = Some(self.resolve_expr(value));
                }
            }
        }
        let mut out = Vec::with_capacity(slots.len());
        for (index, slot) in slots.into_iter().enumerate() {
            match slot {
                Some(expr) => out.push(expr),
                None => {
                    let default = self
                        .hir
                        .functions
                        .get(function)
                        .and_then(|f| f.params.get(index))
                        .and_then(|param| param.default);
                    match default {
                        Some(default) => out.push(default),
                        None => {
                            self.diagnostics.push(FrontendError::at(
                                "ostw-missing-argument",
                                format!("missing argument for parameter '{}'", param_names[index]),
                                span,
                            ));
                            out.push(zero_expr(&mut self.hir));
                        }
                    }
                }
            }
        }
        out
    }
}

/// The body of a user function being collected.
enum FunctionBodySpec {
    Expression(cst::Expr),
    Statements(Vec<cst::Stmt>),
}

/// A placeholder number expression used before a body resolves (pushed into
/// the expression arena).
fn placeholder_expr(program: &mut hir::Program, span: Span) -> ExprId {
    program.exprs.push(hir::Expr::Number {
        value: 0.0,
        text: "0".to_string(),
        span: Some(span),
    })
}

/// The zero literal used to fill unspecified builtin/default argument gaps.
fn zero_expr(program: &mut hir::Program) -> ExprId {
    program.exprs.push(hir::Expr::Number {
        value: 0.0,
        text: "0".to_string(),
        span: None,
    })
}

fn cst_type_to_hir(type_ref: &cst::TypeRef) -> hir::TypeName {
    hir::TypeName {
        name: type_ref.name.clone(),
        array_depth: type_ref.array_depth,
        unions: type_ref
            .unions
            .iter()
            .map(|union| hir::TypeName {
                name: union.name.clone(),
                array_depth: union.array_depth,
                unions: Vec::new(),
                span: None,
            })
            .collect(),
        span: None,
    }
}
