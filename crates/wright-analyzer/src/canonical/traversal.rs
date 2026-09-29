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

/// Visit a value tree in pre-order. `path` passed to `visit` is the child's
/// position chain matching `Program::condition_value_span` /
/// `Program::action_argument_value_span`: `Array`/`Call` children by index,
/// `Vector` components as 0/1/2, a `PlayerVariable` player at 0.
pub(super) fn visit_value_tree<'a>(
    value: &'a Value,
    parent: Option<usize>,
    visit: &mut impl FnMut(&'a Value, Option<usize>, &[usize]) -> usize,
) {
    let mut path = Vec::new();
    visit_value_tree_at(value, parent, &mut path, visit);
}

fn visit_value_tree_at<'a>(
    value: &'a Value,
    parent: Option<usize>,
    path: &mut Vec<usize>,
    visit: &mut impl FnMut(&'a Value, Option<usize>, &[usize]) -> usize,
) {
    let parent = Some(visit(value, parent, path));
    let mut visit_child = |index: usize, value: &'a Value| {
        path.push(index);
        visit_value_tree_at(value, parent, path, visit);
        path.pop();
    };
    match value {
        Value::Array(values) | Value::Call { args: values, .. } => {
            for (index, value) in values.iter().enumerate() {
                visit_child(index, value);
            }
        }
        Value::Vector { x, y, z } => {
            for (index, value) in [x, y, z].iter().enumerate() {
                visit_child(index, value);
            }
        }
        Value::PlayerVariable { player, .. } => visit_child(0, player),
        _ => {}
    }
}
