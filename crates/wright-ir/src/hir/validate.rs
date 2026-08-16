//! Structural validation of the internal Opy HIR model.

use crate::error::IrError;
use workshop_rs::source::Span;

use super::{
    Constant, EnumDecl, Expr, Function, GlobalVar, Macro, PlayerVar, Program, Rule, Stmt,
    Subroutine,
};

/// Validate every ID resolves and every span is valid, returning the first
/// violation.
pub(crate) fn validate(program: &Program) -> Result<(), IrError> {
    for global in program.globals.iter() {
        validate_global(program, global)?;
    }
    for player in program.players.iter() {
        validate_player(program, player)?;
    }
    for subroutine in program.subroutines.iter() {
        validate_subroutine(program, subroutine)?;
    }
    for constant in program.constants.iter() {
        validate_constant(program, constant)?;
    }
    for macro_ in program.macros.iter() {
        validate_macro(program, macro_)?;
    }
    for enum_ in program.enums.iter() {
        validate_enum(program, enum_)?;
    }
    for function in program.functions.iter() {
        validate_function(program, function)?;
    }
    for rule in program.rules.iter() {
        validate_rule(program, rule)?;
    }
    Ok(())
}

fn validate_global(program: &Program, global: &GlobalVar) -> Result<(), IrError> {
    check_span(global.span, program)?;
    if let Some(initializer) = global.initializer {
        check_expr(program, initializer)?;
    }
    Ok(())
}

fn validate_player(program: &Program, player: &PlayerVar) -> Result<(), IrError> {
    check_span(player.span, program)?;
    if let Some(initializer) = player.initializer {
        check_expr(program, initializer)?;
    }
    Ok(())
}

fn validate_subroutine(program: &Program, subroutine: &Subroutine) -> Result<(), IrError> {
    check_span(subroutine.decl_span, program)?;
    if let Some(body) = &subroutine.body {
        check_span(body.span, program)?;
        for statement in &body.statements {
            check_stmt(program, *statement)?;
        }
    }
    Ok(())
}

fn validate_constant(program: &Program, constant: &Constant) -> Result<(), IrError> {
    check_span(constant.span, program)?;
    check_expr(program, constant.value)
}

fn validate_macro(program: &Program, macro_: &Macro) -> Result<(), IrError> {
    check_span(macro_.span, program)?;
    for statement in &macro_.body {
        check_stmt(program, *statement)?;
    }
    Ok(())
}

fn validate_enum(program: &Program, enum_: &EnumDecl) -> Result<(), IrError> {
    check_span(enum_.span, program)
}

fn validate_function(program: &Program, function: &Function) -> Result<(), IrError> {
    check_span(function.span, program)?;
    for param in &function.params {
        check_span(param.span, program)?;
        if let Some(default) = param.default {
            check_expr(program, default)?;
        }
    }
    match &function.body {
        super::FunctionBody::Expression(expr) => check_expr(program, *expr),
        super::FunctionBody::Statements(statements) => {
            for statement in statements {
                check_stmt(program, *statement)?;
            }
            Ok(())
        }
    }
}

fn validate_rule(program: &Program, rule: &Rule) -> Result<(), IrError> {
    check_span(rule.span, program)?;
    check_span(rule.event.span, program)?;
    for arg in &rule.event.args {
        check_expr(program, *arg)?;
    }
    for condition in &rule.conditions {
        check_expr(program, *condition)?;
    }
    for action in &rule.actions {
        check_stmt(program, *action)?;
    }
    Ok(())
}

fn check_stmt(program: &Program, id: super::StmtId) -> Result<(), IrError> {
    let statement = program
        .stmts
        .get(id)
        .ok_or_else(|| dangling("statement", id.index()))?;
    check_span(statement.span(), program)?;
    match statement {
        Stmt::Expr { expr, .. } => check_expr(program, *expr),
        Stmt::Assign { target, value, .. } => {
            check_expr(program, *target)?;
            check_expr(program, *value)
        }
        Stmt::If {
            branches,
            else_body,
            ..
        } => {
            for branch in branches {
                check_expr(program, branch.condition)?;
                for statement in &branch.body {
                    check_stmt(program, *statement)?;
                }
            }
            if let Some(else_body) = else_body {
                for statement in else_body {
                    check_stmt(program, *statement)?;
                }
            }
            Ok(())
        }
        Stmt::For {
            variable,
            iterable,
            body,
            ..
        } => {
            if !program.globals.contains(*variable) {
                return Err(dangling("global variable", variable.index()));
            }
            check_expr(program, *iterable)?;
            for statement in body {
                check_stmt(program, *statement)?;
            }
            Ok(())
        }
        Stmt::CFor {
            variable,
            start,
            condition,
            step,
            body,
            ..
        } => {
            if !program.globals.contains(*variable) {
                return Err(dangling("global variable", variable.index()));
            }
            if let Some(start) = start {
                check_expr(program, *start)?;
            }
            if let Some(condition) = condition {
                check_expr(program, *condition)?;
            }
            if let Some(step) = step {
                check_expr(program, *step)?;
            }
            for statement in body {
                check_stmt(program, *statement)?;
            }
            Ok(())
        }
        Stmt::Foreach {
            variable,
            iterable,
            body,
            ..
        } => {
            if !program.globals.contains(*variable) {
                return Err(dangling("global variable", variable.index()));
            }
            check_expr(program, *iterable)?;
            for statement in body {
                check_stmt(program, *statement)?;
            }
            Ok(())
        }
        Stmt::While {
            condition, body, ..
        } => {
            check_expr(program, *condition)?;
            for statement in body {
                check_stmt(program, *statement)?;
            }
            Ok(())
        }
        Stmt::Switch { value, cases, .. } => {
            check_expr(program, *value)?;
            for case in cases {
                check_span(case.span, program)?;
                if let Some(case_value) = case.value {
                    check_expr(program, case_value)?;
                }
                for statement in &case.body {
                    check_stmt(program, *statement)?;
                }
            }
            Ok(())
        }
        Stmt::Return { value, .. } => {
            if let Some(value) = value {
                check_expr(program, *value)?;
            }
            Ok(())
        }
        Stmt::Break { .. } | Stmt::Continue { .. } => Ok(()),
        Stmt::CallSubroutine { subroutine, .. } => {
            if !program.subroutines.contains(*subroutine) {
                return Err(dangling("subroutine", subroutine.index()));
            }
            Ok(())
        }
        Stmt::Pass { .. } => Ok(()),
    }
}

fn check_expr(program: &Program, id: super::ExprId) -> Result<(), IrError> {
    let expression = program
        .exprs
        .get(id)
        .ok_or_else(|| dangling("expression", id.index()))?;
    check_span(expression.span(), program)?;
    match expression {
        Expr::Array { elements, .. } => {
            for element in elements {
                check_expr(program, *element)?;
            }
            Ok(())
        }
        Expr::Vector { x, y, z, .. } => {
            check_expr(program, *x)?;
            check_expr(program, *y)?;
            check_expr(program, *z)?;
            Ok(())
        }
        Expr::PlayerVar {
            player, variable, ..
        } => {
            check_expr(program, *player)?;
            if !program.players.contains(*variable) {
                return Err(dangling("player variable", variable.index()));
            }
            Ok(())
        }
        Expr::GlobalVar { variable, .. } => {
            if !program.globals.contains(*variable) {
                return Err(dangling("global variable", variable.index()));
            }
            Ok(())
        }
        Expr::Constant { constant, .. } => {
            if !program.constants.contains(*constant) {
                return Err(dangling("constant", constant.index()));
            }
            Ok(())
        }
        Expr::UserEnum {
            enum_id, member, ..
        } => {
            let Some(enum_) = program.enums.get(*enum_id) else {
                return Err(dangling("enum", enum_id.index()));
            };
            if !enum_
                .members
                .iter()
                .any(|candidate| candidate.name == *member)
            {
                return Err(IrError::Invalid {
                    code: "unknown-enum-member",
                    message: format!("enum '{}' has no member '{member}'", enum_.name),
                    span: expression.span(),
                });
            }
            Ok(())
        }
        Expr::UserCall { function, args, .. } => {
            if !program.functions.contains(*function) {
                return Err(dangling("function", function.index()));
            }
            for arg in args {
                check_expr(program, *arg)?;
            }
            Ok(())
        }
        Expr::Param { .. } => Ok(()),
        Expr::MacroCall { macro_, args, .. } => {
            if !program.macros.contains(*macro_) {
                return Err(dangling("macro", macro_.index()));
            }
            for arg in args {
                check_expr(program, *arg)?;
            }
            Ok(())
        }
        Expr::Call { args, .. } | Expr::Format { args, .. } => {
            for arg in args {
                check_expr(program, *arg)?;
            }
            Ok(())
        }
        Expr::ReceiverCall { receiver, args, .. } => {
            check_expr(program, *receiver)?;
            for arg in args {
                check_expr(program, *arg)?;
            }
            Ok(())
        }
        Expr::Binary { left, right, .. } => {
            check_expr(program, *left)?;
            check_expr(program, *right)?;
            Ok(())
        }
        Expr::Unary { operand, .. } => {
            check_expr(program, *operand)?;
            Ok(())
        }
        Expr::Index { array, index, .. } => {
            check_expr(program, *array)?;
            check_expr(program, *index)?;
            Ok(())
        }
        Expr::Ternary {
            condition,
            then_value,
            else_value,
            ..
        } => {
            check_expr(program, *condition)?;
            check_expr(program, *then_value)?;
            check_expr(program, *else_value)?;
            Ok(())
        }
        Expr::Cast { value, .. } => {
            check_expr(program, *value)?;
            Ok(())
        }
        Expr::Number { .. }
        | Expr::String { .. }
        | Expr::Bool { .. }
        | Expr::Null { .. }
        | Expr::Enum { .. }
        | Expr::EventPlayer { .. }
        | Expr::MacroParam { .. } => Ok(()),
    }
}

fn check_span(span: Option<Span>, program: &Program) -> Result<(), IrError> {
    let Some(span) = span else {
        return Ok(());
    };
    if !span.is_valid() {
        return Err(IrError::Invalid {
            code: "invalid-span",
            message: "span end precedes start or a position is zero".into(),
            span: Some(span),
        });
    }
    if !program.files.contains(span.file) {
        return Err(IrError::Invalid {
            code: "invalid-span",
            message: format!("span references unknown file {}", span.file.index()),
            span: Some(span),
        });
    }
    Ok(())
}

fn dangling(what: &'static str, id: usize) -> IrError {
    IrError::DanglingReference {
        what,
        id: id as u32,
    }
}
