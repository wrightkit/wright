//! Deterministic debug dump for the internal Opy HIR model.

use crate::source::Span;

use super::{Event, Expr, Program, Stmt, UnaryOp};

/// Render a deterministic, human-readable dump of the program.
pub(crate) fn dump(program: &Program) -> String {
    let mut out = String::new();
    out.push_str("program (internal Opy HIR)\n");
    out.push_str("files:\n");
    for (index, file) in program.files.iter().enumerate() {
        out.push_str(&format!("  {index} {}\n", file.path));
    }
    out.push_str("declarations:\n");
    for (index, global) in program.globals.iter().enumerate() {
        out.push_str(&format!(
            "  globalVariable {} (id {index}){}\n",
            global.name,
            span_suffix(global.span),
        ));
        if let Some(initializer) = global.initializer {
            out.push_str("    initializer = ");
            render_expr(program, initializer, &mut out);
            out.push('\n');
        }
    }
    for (index, player) in program.players.iter().enumerate() {
        out.push_str(&format!(
            "  playerVariable {} (id {index}){}\n",
            player.name,
            span_suffix(player.span),
        ));
        if let Some(initializer) = player.initializer {
            out.push_str("    initializer = ");
            render_expr(program, initializer, &mut out);
            out.push('\n');
        }
    }
    for (index, subroutine) in program.subroutines.iter().enumerate() {
        out.push_str(&format!(
            "  subroutine {} (id {index}){}\n",
            subroutine.name,
            span_suffix(subroutine.decl_span),
        ));
    }
    for (index, constant) in program.constants.iter().enumerate() {
        out.push_str(&format!(
            "  constant {} (id {index}){} = ",
            constant.name,
            span_suffix(constant.span),
        ));
        render_expr(program, constant.value, &mut out);
        out.push('\n');
    }
    for (index, macro_) in program.macros.iter().enumerate() {
        let args = if macro_.args.is_empty() {
            String::new()
        } else {
            format!("({})", macro_.args.join(", "))
        };
        out.push_str(&format!(
            "  macro {}{args} (id {index}){}\n",
            macro_.name,
            span_suffix(macro_.span),
        ));
        for statement in &macro_.body {
            render_stmt(program, *statement, &mut out, 2);
        }
    }
    for (index, enum_) in program.enums.iter().enumerate() {
        out.push_str(&format!(
            "  enum {} (id {index}){} = [{}]\n",
            enum_.name,
            span_suffix(enum_.span),
            enum_
                .members
                .iter()
                .map(|member| member.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for (index, function) in program.functions.iter().enumerate() {
        let return_type = function
            .return_type
            .as_ref()
            .map(|t| format!(": {}", t.display()))
            .unwrap_or_default();
        let params = function
            .params
            .iter()
            .map(|param| param.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "  function {}({params}){return_type} (id {index}){}\n",
            function.name,
            span_suffix(function.span),
        ));
        match &function.body {
            super::FunctionBody::Expression(expr) => {
                out.push_str("    = ");
                render_expr(program, *expr, &mut out);
                out.push('\n');
            }
            super::FunctionBody::Statements(statements) => {
                for statement in statements {
                    render_stmt(program, *statement, &mut out, 2);
                }
            }
        }
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
            render_expr(program, *condition, &mut out);
            out.push('\n');
        }
        for action in &rule.actions {
            render_stmt(program, *action, &mut out, 2);
        }
    }
    out
}

fn render_event(program: &Program, event: &Event, out: &mut String, level: usize) {
    out.push_str(&format!(
        "{}event {}{}\n",
        indent(level),
        event.name,
        span_suffix(event.span),
    ));
    for arg in &event.args {
        out.push_str(&format!("{}arg ", indent(level + 1)));
        render_expr(program, *arg, out);
        out.push('\n');
    }
}

fn render_stmt(program: &Program, id: super::StmtId, out: &mut String, level: usize) {
    let Some(statement) = program.stmts.get(id) else {
        out.push_str(&format!("{}<dangling stmt {}/>\n", indent(level), id));
        return;
    };
    match statement {
        Stmt::Expr { expr, span } => {
            out.push_str(&format!("{}expr ", indent(level)));
            render_expr(program, *expr, out);
            out.push_str(&format!("{}\n", span_suffix(*span)));
        }
        Stmt::Assign {
            target,
            value,
            span,
        } => {
            out.push_str(&format!("{}assign ", indent(level)));
            render_expr(program, *target, out);
            out.push_str(" = ");
            render_expr(program, *value, out);
            out.push_str(&format!("{}\n", span_suffix(*span)));
        }
        Stmt::If {
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
                render_expr(program, branch.condition, out);
                out.push('\n');
                for statement in &branch.body {
                    render_stmt(program, *statement, out, level + 2);
                }
            }
            if let Some(else_body) = else_body {
                out.push_str(&format!("{}  else\n", indent(level)));
                for statement in else_body {
                    render_stmt(program, *statement, out, level + 2);
                }
            }
        }
        Stmt::For {
            variable,
            iterable,
            body,
            span,
            ..
        } => {
            let variable_name = program.globals.get(*variable).map_or_else(
                || format!("<dangling {}>", variable.index()),
                |v| v.name.clone(),
            );
            out.push_str(&format!("{}for {variable_name} in ", indent(level)));
            render_expr(program, *iterable, out);
            out.push_str(&format!("{}\n", span_suffix(*span)));
            for statement in body {
                render_stmt(program, *statement, out, level + 1);
            }
        }
        Stmt::CFor {
            variable,
            start,
            condition,
            step,
            body,
            span,
        } => {
            let variable_name = program.globals.get(*variable).map_or_else(
                || format!("<dangling {}>", variable.index()),
                |v| v.name.clone(),
            );
            out.push_str(&format!("{}cfor {variable_name} (", indent(level)));
            if let Some(start) = start {
                render_expr(program, *start, out);
            }
            out.push_str("; ");
            if let Some(condition) = condition {
                render_expr(program, *condition, out);
            }
            out.push_str("; ");
            if let Some(step) = step {
                render_expr(program, *step, out);
            }
            out.push_str(&format!("){}\n", span_suffix(*span)));
            for statement in body {
                render_stmt(program, *statement, out, level + 1);
            }
        }
        Stmt::Foreach {
            variable,
            iterable,
            body,
            span,
        } => {
            let variable_name = program.globals.get(*variable).map_or_else(
                || format!("<dangling {}>", variable.index()),
                |v| v.name.clone(),
            );
            out.push_str(&format!("{}foreach {variable_name} in ", indent(level)));
            render_expr(program, *iterable, out);
            out.push_str(&format!("{}\n", span_suffix(*span)));
            for statement in body {
                render_stmt(program, *statement, out, level + 1);
            }
        }
        Stmt::While {
            condition,
            body,
            span,
        } => {
            out.push_str(&format!("{}while ", indent(level)));
            render_expr(program, *condition, out);
            out.push_str(&format!("{}\n", span_suffix(*span)));
            for statement in body {
                render_stmt(program, *statement, out, level + 1);
            }
        }
        Stmt::Switch {
            value, cases, span, ..
        } => {
            out.push_str(&format!("{}switch ", indent(level)));
            render_expr(program, *value, out);
            out.push_str(&format!("{}\n", span_suffix(*span)));
            for case in cases {
                match &case.value {
                    Some(value) => {
                        out.push_str(&format!("{}  case ", indent(level)));
                        render_expr(program, *value, out);
                        out.push('\n');
                    }
                    None => out.push_str(&format!("{}  default\n", indent(level))),
                }
                for statement in &case.body {
                    render_stmt(program, *statement, out, level + 2);
                }
            }
        }
        Stmt::Return { value, span } => {
            out.push_str(&format!("{}return ", indent(level)));
            if let Some(value) = value {
                render_expr(program, *value, out);
            }
            out.push_str(&format!("{}\n", span_suffix(*span)));
        }
        Stmt::Break { span } => {
            out.push_str(&format!("{}break{}\n", indent(level), span_suffix(*span)));
        }
        Stmt::Continue { span } => {
            out.push_str(&format!(
                "{}continue{}\n",
                indent(level),
                span_suffix(*span)
            ));
        }
        Stmt::CallSubroutine {
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
        Stmt::Pass { span } => {
            out.push_str(&format!("{}pass{}\n", indent(level), span_suffix(*span)));
        }
    }
}

fn render_expr(program: &Program, id: super::ExprId, out: &mut String) {
    let Some(expression) = program.exprs.get(id) else {
        out.push_str(&format!("<dangling expr {id}>"));
        return;
    };
    match expression {
        Expr::Number { value, .. } => out.push_str(&format_number(*value)),
        Expr::String { value, .. } => out.push_str(&format!("{:?}", value)),
        Expr::Bool { value, .. } => out.push_str(if *value { "true" } else { "false" }),
        Expr::Null { .. } => out.push_str("null"),
        Expr::Array { elements, .. } => {
            out.push('[');
            for (index, element) in elements.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                render_expr(program, *element, out);
            }
            out.push(']');
        }
        Expr::Vector { x, y, z, .. } => {
            out.push_str("vect(");
            render_expr(program, *x, out);
            out.push_str(", ");
            render_expr(program, *y, out);
            out.push_str(", ");
            render_expr(program, *z, out);
            out.push(')');
        }
        Expr::Enum {
            value_type, value, ..
        } => {
            out.push_str(value_type);
            out.push('.');
            out.push_str(value);
        }
        Expr::UserEnum {
            enum_id, member, ..
        } => {
            out.push_str(&symbol_name(
                program.enums.get(*enum_id).map(|e| e.name.as_str()),
                "enum",
                enum_id.index(),
            ));
            out.push('.');
            out.push_str(member);
        }
        Expr::GlobalVar { variable, .. } => out.push_str(&symbol_name(
            program.globals.get(*variable).map(|g| g.name.as_str()),
            "global",
            variable.index(),
        )),
        Expr::PlayerVar {
            player, variable, ..
        } => {
            render_expr(program, *player, out);
            out.push('.');
            out.push_str(&symbol_name(
                program.players.get(*variable).map(|p| p.name.as_str()),
                "player",
                variable.index(),
            ));
        }
        Expr::EventPlayer { .. } => out.push_str("eventPlayer"),
        Expr::Constant { constant, .. } => out.push_str(&symbol_name(
            program.constants.get(*constant).map(|c| c.name.as_str()),
            "constant",
            constant.index(),
        )),
        Expr::UserCall { function, args, .. } => {
            out.push_str(&symbol_name(
                program.functions.get(*function).map(|f| f.name.as_str()),
                "function",
                function.index(),
            ));
            render_args(program, args, out);
        }
        Expr::Param { name, .. } => {
            out.push('$');
            out.push_str(name);
        }
        Expr::Call { name, args, .. } => {
            out.push_str(name);
            render_args(program, args, out);
        }
        Expr::ReceiverCall {
            receiver,
            name,
            args,
            ..
        } => {
            render_expr(program, *receiver, out);
            out.push('.');
            out.push_str(name);
            render_args(program, args, out);
        }
        Expr::MacroCall { macro_, args, .. } => {
            out.push('$');
            out.push_str(&symbol_name(
                program.macros.get(*macro_).map(|m| m.name.as_str()),
                "macro",
                macro_.index(),
            ));
            render_args(program, args, out);
        }
        Expr::MacroParam { name, .. } => {
            out.push('$');
            out.push_str(name);
        }
        Expr::Binary {
            op, left, right, ..
        } => {
            out.push('(');
            render_expr(program, *left, out);
            out.push(' ');
            out.push_str(op.as_str());
            out.push(' ');
            render_expr(program, *right, out);
            out.push(')');
        }
        Expr::Unary { op, operand, .. } => {
            render_unary(op, program, *operand, out);
        }
        Expr::Index { array, index, .. } => {
            render_expr(program, *array, out);
            out.push('[');
            render_expr(program, *index, out);
            out.push(']');
        }
        Expr::Format { text, args, .. } => {
            out.push_str("format(");
            out.push_str(&format!("{:?}", text));
            for arg in args {
                out.push_str(", ");
                render_expr(program, *arg, out);
            }
            out.push(')');
        }
        Expr::Ternary {
            condition,
            then_value,
            else_value,
            ..
        } => {
            out.push('(');
            render_expr(program, *condition, out);
            out.push_str(" ? ");
            render_expr(program, *then_value, out);
            out.push_str(" : ");
            render_expr(program, *else_value, out);
            out.push(')');
        }
        Expr::Cast {
            type_name, value, ..
        } => {
            out.push('<');
            out.push_str(&type_name.display());
            out.push('>');
            render_expr(program, *value, out);
        }
    }
}

fn render_args(program: &Program, args: &[super::ExprId], out: &mut String) {
    out.push('(');
    for (index, arg) in args.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        render_expr(program, *arg, out);
    }
    out.push(')');
}

fn render_unary(op: &UnaryOp, program: &Program, operand: super::ExprId, out: &mut String) {
    if *op == UnaryOp::Negate {
        out.push('-');
        render_expr(program, operand, out);
    } else {
        out.push_str(op.as_str());
        out.push(' ');
        render_expr(program, operand, out);
    }
}

fn symbol_name(name: Option<&str>, what: &str, id: usize) -> String {
    match name {
        Some(name) => name.to_string(),
        None => format!("<dangling {what} {id}>"),
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
