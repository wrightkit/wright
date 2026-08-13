//! Deterministic debug dump for the Workshop IR model.

use crate::source::Span;

use super::{Action, Event, Program, Value};

/// Render a deterministic, human-readable dump of the workshop program.
pub(crate) fn dump(program: &Program) -> String {
    let mut out = String::new();
    out.push_str("program (Workshop IR)\n");
    out.push_str("files:\n");
    for (index, file) in program.files.iter().enumerate() {
        out.push_str(&format!("  {index} {}\n", file.path));
    }
    out.push_str("global variables:\n");
    for variable in program.global_variables.iter() {
        out.push_str(&format!(
            "  {} (index {}){}\n",
            variable.name,
            variable.index,
            span_suffix(variable.span),
        ));
        if let Some(initializer) = variable.initializer {
            out.push_str("    initializer = ");
            render_value(program, initializer, &mut out);
            out.push('\n');
        }
    }
    out.push_str("player variables:\n");
    for variable in program.player_variables.iter() {
        out.push_str(&format!(
            "  {} (index {}){}\n",
            variable.name,
            variable.index,
            span_suffix(variable.span),
        ));
        if let Some(initializer) = variable.initializer {
            out.push_str("    initializer = ");
            render_value(program, initializer, &mut out);
            out.push('\n');
        }
    }
    out.push_str("subroutines:\n");
    for subroutine in program.subroutines.iter() {
        out.push_str(&format!(
            "  {} (index {}){}\n",
            subroutine.name,
            subroutine.index,
            span_suffix(subroutine.span),
        ));
    }
    out.push_str("rules:\n");
    for (index, rule) in program.rules.iter().enumerate() {
        out.push_str(&format!(
            "  rule \"{}\" (id {index}){}\n",
            rule.name,
            span_suffix(rule.span),
        ));
        render_event(program, &rule.event, &mut out, 2);
        for condition in &rule.conditions {
            out.push_str("    condition ");
            render_value(program, *condition, &mut out);
            out.push('\n');
        }
        for action in &rule.actions {
            render_action(program, *action, &mut out, 2);
        }
    }
    out
}

fn render_event(program: &Program, event: &Event, out: &mut String, level: usize) {
    match event {
        Event::Global => out.push_str(&format!("{}event Global\n", indent(level))),
        Event::EachPlayer => out.push_str(&format!("{}event EachPlayer\n", indent(level))),
        Event::Subroutine(subroutine) => {
            let name = program
                .subroutines
                .get(*subroutine)
                .map_or_else(|| "<dangling>".to_string(), |s| s.name.clone());
            out.push_str(&format!(
                "{}event Subroutine {} (id {})\n",
                indent(level),
                name,
                subroutine
            ));
        }
    }
}

fn render_action(program: &Program, id: super::ActionId, out: &mut String, level: usize) {
    let Some(action) = program.actions.get(id) else {
        out.push_str(&format!("{}<dangling action {id}/>\n", indent(level)));
        return;
    };
    match action {
        Action::SetGlobalVariable {
            variable,
            value,
            span,
            ..
        } => {
            out.push_str(&format!("{}setGlobalVariable ", indent(level)));
            render_variable_ref(program, *variable, "global", out);
            out.push_str(" = ");
            render_value(program, *value, out);
            out.push_str(&format!("{}\n", span_suffix(*span)));
        }
        Action::ModifyGlobalVariable {
            variable,
            op,
            value,
            span,
            ..
        } => {
            out.push_str(&format!("{}modifyGlobalVariable ", indent(level)));
            render_variable_ref(program, *variable, "global", out);
            out.push_str(&format!(" {} ", op.as_str()));
            render_value(program, *value, out);
            out.push_str(&format!("{}\n", span_suffix(*span)));
        }
        Action::SetPlayerVariable {
            player,
            variable,
            value,
            span,
            ..
        } => {
            out.push_str(&format!("{}setPlayerVariable ", indent(level)));
            render_value(program, *player, out);
            out.push('.');
            out.push_str(&variable_name(
                program.player_variables.get(*variable),
                variable.index(),
            ));
            out.push_str(" = ");
            render_value(program, *value, out);
            out.push_str(&format!("{}\n", span_suffix(*span)));
        }
        Action::ModifyPlayerVariable {
            player,
            variable,
            op,
            value,
            span,
            ..
        } => {
            out.push_str(&format!("{}modifyPlayerVariable ", indent(level)));
            render_value(program, *player, out);
            out.push('.');
            out.push_str(&variable_name(
                program.player_variables.get(*variable),
                variable.index(),
            ));
            out.push_str(&format!(" {} ", op.as_str()));
            render_value(program, *value, out);
            out.push_str(&format!("{}\n", span_suffix(*span)));
        }
        Action::CallSubroutine {
            subroutine, span, ..
        } => {
            let name = program
                .subroutines
                .get(*subroutine)
                .map_or_else(|| "<dangling>".to_string(), |s| s.name.clone());
            out.push_str(&format!(
                "{}callSubroutine {} (id {}){}\n",
                indent(level),
                name,
                subroutine,
                span_suffix(*span),
            ));
        }
        Action::If {
            branches,
            else_body,
            span,
        } => {
            out.push_str(&format!("{}if{}\n", indent(level), span_suffix(*span)));
            for (index, branch) in branches.iter().enumerate() {
                out.push_str(&format!(
                    "{}  {}condition ",
                    indent(level),
                    if index == 0 { "" } else { "elif " }
                ));
                render_value(program, branch.condition, out);
                out.push('\n');
                for action in &branch.body {
                    render_action(program, *action, out, level + 2);
                }
            }
            if let Some(else_body) = else_body {
                out.push_str(&format!("{}  else\n", indent(level)));
                for action in else_body {
                    render_action(program, *action, out, level + 2);
                }
            }
        }
        Action::While {
            condition,
            body,
            span,
        } => {
            out.push_str(&format!("{}while ", indent(level)));
            render_value(program, *condition, out);
            out.push_str(&format!("{}\n", span_suffix(*span)));
            for action in body {
                render_action(program, *action, out, level + 1);
            }
        }
        Action::ForGlobalVariable {
            variable,
            start,
            stop,
            step,
            body,
            span,
            ..
        } => {
            out.push_str(&format!("{}forGlobalVariable ", indent(level)));
            render_variable_ref(program, *variable, "global", out);
            out.push_str(" in ");
            render_value(program, *start, out);
            out.push_str(", ");
            render_value(program, *stop, out);
            out.push_str(", ");
            render_value(program, *step, out);
            out.push_str(&format!("{}\n", span_suffix(*span)));
            for action in body {
                render_action(program, *action, out, level + 1);
            }
        }
        Action::Debug { value, span } => {
            out.push_str(&format!("{}debug ", indent(level)));
            render_value(program, *value, out);
            out.push_str(&format!("{}\n", span_suffix(*span)));
        }
        Action::Print { message, span } => {
            out.push_str(&format!("{}print ", indent(level)));
            render_value(program, *message, out);
            out.push_str(&format!("{}\n", span_suffix(*span)));
        }
        Action::Call { name, args, span } => {
            out.push_str(&format!("{}call {name}(", indent(level)));
            for (index, arg) in args.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                render_value(program, *arg, out);
            }
            out.push_str(&format!("){}\n", span_suffix(*span)));
        }
    }
}

fn render_variable_ref(program: &Program, id: super::GlobalVarId, what: &str, out: &mut String) {
    let name = match what {
        "global" => program.global_variables.get(id).map(|v| v.name.clone()),
        _ => program.player_variables.get(id).map(|v| v.name.clone()),
    };
    match name {
        Some(name) => out.push_str(&name),
        None => out.push_str(&format!("<dangling {what} {}>", id.index())),
    }
}

fn variable_name(name: Option<&super::WorkshopVariable>, id: usize) -> String {
    match name {
        Some(variable) => variable.name.clone(),
        None => format!("<dangling {id}>"),
    }
}

fn render_value(program: &Program, id: super::ValueId, out: &mut String) {
    let Some(node) = program.values.get(id) else {
        out.push_str(&format!("<dangling value {id}>"));
        return;
    };
    let value = &node.value;
    match value {
        Value::Number(value) => out.push_str(&format_number(*value)),
        Value::String(value) => out.push_str(&format!("{:?}", value)),
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Value::Null => out.push_str("null"),
        Value::Array(elements) => {
            out.push('[');
            for (index, element) in elements.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                render_value(program, *element, out);
            }
            out.push(']');
        }
        Value::Vector { x, y, z } => {
            out.push_str("vect(");
            render_value(program, *x, out);
            out.push_str(", ");
            render_value(program, *y, out);
            out.push_str(", ");
            render_value(program, *z, out);
            out.push(')');
        }
        Value::Enum { value_type, value } => {
            out.push_str(value_type);
            out.push('.');
            out.push_str(value);
        }
        Value::GlobalVariable(variable) => out.push_str(&variable_name(
            program.global_variables.get(*variable),
            variable.index(),
        )),
        Value::PlayerVariable { player, variable } => {
            render_value(program, *player, out);
            out.push('.');
            out.push_str(&variable_name(
                program.player_variables.get(*variable),
                variable.index(),
            ));
        }
        Value::EventPlayer => out.push_str("eventPlayer"),
        Value::Call { name, args } => {
            out.push_str(name);
            out.push('(');
            for (index, arg) in args.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                render_value(program, *arg, out);
            }
            out.push(')');
        }
    }
}

fn format_number(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn indent(level: usize) -> String {
    "  ".repeat(level)
}

fn span_suffix(span: Option<Span>) -> String {
    match span {
        Some(span) => format!(
            " @{}:{}:{}-{}:{}",
            span.file.index(),
            span.start.line,
            span.start.col,
            span.end.line,
            span.end.col
        ),
        None => String::new(),
    }
}
