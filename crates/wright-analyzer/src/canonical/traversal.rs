use workshop_rs::{Action, Value};

pub(super) fn visit_action_roots<'a>(action: &'a Action, visit: &mut impl FnMut(usize, &'a Value)) {
    match action {
        Action::SetGlobalVariable { value, .. } | Action::ModifyGlobalVariable { value, .. } => {
            visit(0, value)
        }
        Action::SetPlayerVariable { player, value, .. }
        | Action::ModifyPlayerVariable { player, value, .. } => {
            visit(0, player);
            visit(1, value);
        }
        Action::AssignMember { target, value, .. } => {
            visit(0, target);
            visit(1, value);
        }
        Action::If { condition } | Action::ElseIf { condition } | Action::While { condition } => {
            visit(0, condition);
        }
        Action::ForGlobalVariable {
            start, stop, step, ..
        } => {
            visit(0, start);
            visit(1, stop);
            visit(2, step);
        }
        Action::ForPlayerVariable {
            player,
            start,
            stop,
            step,
            ..
        } => {
            visit(0, player);
            visit(1, start);
            visit(2, stop);
            visit(3, step);
        }
        Action::Call { args, .. } => {
            for (index, value) in args.iter().enumerate() {
                visit(index, value);
            }
        }
        Action::Disabled { action } => visit_action_roots(action, visit),
        Action::CallSubroutine { .. } | Action::Else | Action::End => {}
    }
}

pub(super) fn visit_value_tree<'a>(
    value: &'a Value,
    parent: Option<usize>,
    visit: &mut impl FnMut(&'a Value, Option<usize>) -> usize,
) {
    let parent = Some(visit(value, parent));
    match value {
        Value::Array(values) | Value::Call { args: values, .. } => {
            for value in values {
                visit_value_tree(value, parent, visit);
            }
        }
        Value::Vector { x, y, z } => {
            for value in [x, y, z] {
                visit_value_tree(value, parent, visit);
            }
        }
        Value::PlayerVariable { player, .. } => visit_value_tree(player, parent, visit),
        _ => {}
    }
}
