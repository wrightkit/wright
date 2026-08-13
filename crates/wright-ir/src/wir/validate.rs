//! Structural validation of the Workshop IR model.

use crate::error::IrError;
use crate::settings::{Settings as IrSettings, SettingsNode as IrSettingsNode};
use crate::source::Span;

use super::{Action, Event, Program, Rule, Value};

/// Validate every ID resolves and every span is valid, returning the first
/// violation.
pub(crate) fn validate(program: &Program) -> Result<(), IrError> {
    if let Some(settings) = &program.settings {
        check_settings(settings, program)?;
    }
    for variable in program.global_variables.iter() {
        check_span(variable.span, program)?;
        if let Some(initializer) = variable.initializer {
            check_value(program, initializer)?;
        }
    }
    for variable in program.player_variables.iter() {
        check_span(variable.span, program)?;
        if let Some(initializer) = variable.initializer {
            check_value(program, initializer)?;
        }
    }
    for subroutine in program.subroutines.iter() {
        check_span(subroutine.span, program)?;
    }
    for rule in program.rules.iter() {
        check_rule(program, rule)?;
    }
    Ok(())
}

fn check_rule(program: &Program, rule: &Rule) -> Result<(), IrError> {
    check_span(rule.span, program)?;
    if let Event::Subroutine(subroutine) = &rule.event {
        if !program.subroutines.contains(*subroutine) {
            return Err(dangling("subroutine", subroutine.index()));
        }
    }
    for condition in &rule.conditions {
        check_value(program, *condition)?;
    }
    for action in &rule.actions {
        check_action(program, *action)?;
    }
    Ok(())
}

fn check_action(program: &Program, id: super::ActionId) -> Result<(), IrError> {
    let action = program
        .actions
        .get(id)
        .ok_or_else(|| dangling("action", id.index()))?;
    check_span(action.span(), program)?;
    match action {
        Action::SetGlobalVariable {
            variable, value, ..
        } => {
            if !program.global_variables.contains(*variable) {
                return Err(dangling("global variable", variable.index()));
            }
            check_value(program, *value)
        }
        Action::ModifyGlobalVariable {
            variable, value, ..
        } => {
            if !program.global_variables.contains(*variable) {
                return Err(dangling("global variable", variable.index()));
            }
            check_value(program, *value)
        }
        Action::SetPlayerVariable {
            player,
            variable,
            value,
            ..
        } => {
            check_value(program, *player)?;
            if !program.player_variables.contains(*variable) {
                return Err(dangling("player variable", variable.index()));
            }
            check_value(program, *value)
        }
        Action::ModifyPlayerVariable {
            player,
            variable,
            value,
            ..
        } => {
            check_value(program, *player)?;
            if !program.player_variables.contains(*variable) {
                return Err(dangling("player variable", variable.index()));
            }
            check_value(program, *value)
        }
        Action::CallSubroutine { subroutine, .. } => {
            if !program.subroutines.contains(*subroutine) {
                return Err(dangling("subroutine", subroutine.index()));
            }
            Ok(())
        }
        Action::If {
            branches,
            else_body,
            ..
        } => {
            for branch in branches {
                check_value(program, branch.condition)?;
                for action in &branch.body {
                    check_action(program, *action)?;
                }
            }
            if let Some(else_body) = else_body {
                for action in else_body {
                    check_action(program, *action)?;
                }
            }
            Ok(())
        }
        Action::While {
            condition, body, ..
        } => {
            check_value(program, *condition)?;
            for action in body {
                check_action(program, *action)?;
            }
            Ok(())
        }
        Action::ForGlobalVariable {
            variable,
            start,
            stop,
            step,
            body,
            ..
        } => {
            if !program.global_variables.contains(*variable) {
                return Err(dangling("global variable", variable.index()));
            }
            check_value(program, *start)?;
            check_value(program, *stop)?;
            check_value(program, *step)?;
            for action in body {
                check_action(program, *action)?;
            }
            Ok(())
        }
        Action::Debug { value, .. } => check_value(program, *value),
        Action::Print { message, .. } => check_value(program, *message),
        Action::Call { args, .. } => {
            for arg in args {
                check_value(program, *arg)?;
            }
            Ok(())
        }
    }
}

fn check_value(program: &Program, id: super::ValueId) -> Result<(), IrError> {
    let node = program
        .values
        .get(id)
        .ok_or_else(|| dangling("value", id.index()))?;
    check_span(node.span, program)?;
    let value = &node.value;
    match value {
        Value::Array(elements) => {
            for element in elements {
                check_value(program, *element)?;
            }
        }
        Value::Vector { x, y, z } => {
            check_value(program, *x)?;
            check_value(program, *y)?;
            check_value(program, *z)?;
        }
        Value::PlayerVariable { player, variable } => {
            check_value(program, *player)?;
            if !program.player_variables.contains(*variable) {
                return Err(dangling("player variable", variable.index()));
            }
        }
        Value::GlobalVariable(variable) => {
            if !program.global_variables.contains(*variable) {
                return Err(dangling("global variable", variable.index()));
            }
        }
        Value::Call { args, .. } => {
            for arg in args {
                check_value(program, *arg)?;
            }
        }
        Value::Number(_)
        | Value::String(_)
        | Value::Bool(_)
        | Value::Null
        | Value::Enum { .. }
        | Value::EventPlayer => {}
    }
    Ok(())
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

/// Recursive span checks for the settings carrier (structural only; the
/// settings tree is emitted verbatim, #86).
fn check_settings(settings: &IrSettings, program: &Program) -> Result<(), IrError> {
    check_span(settings.span, program)?;
    for node in &settings.children {
        check_settings_node(node, program)?;
    }
    Ok(())
}

fn check_settings_node(node: &IrSettingsNode, program: &Program) -> Result<(), IrError> {
    match node {
        IrSettingsNode::Group { children, span, .. } => {
            check_span(*span, program)?;
            for child in children {
                check_settings_node(child, program)?;
            }
            Ok(())
        }
        IrSettingsNode::Number { span, .. }
        | IrSettingsNode::Bool { span, .. }
        | IrSettingsNode::String { span, .. } => check_span(*span, program),
        IrSettingsNode::List { elements, span, .. } => {
            check_span(*span, program)?;
            for element in elements {
                check_span(element.span, program)?;
            }
            Ok(())
        }
    }
}

fn dangling(what: &'static str, id: usize) -> IrError {
    IrError::DanglingReference {
        what,
        id: id as u32,
    }
}
