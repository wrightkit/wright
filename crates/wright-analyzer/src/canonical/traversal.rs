use workshop_rs::{Action, Value};

/// The action a `Disabled` wrapper carries, through nested wrappers.
/// `disabled` prefixes one statement: a disabled block opener still claims
/// the region through its matching `End`, so block matching sees through the
/// wrapper while nothing inside the region is live.
pub(crate) fn unwrapped(action: &Action) -> &Action {
    let mut current = action;
    while let Action::Disabled { action } = current {
        current = action;
    }
    current
}

/// Whether the statement opens a block closed by a matching `End`.
/// `Disabled` is seen through: a disabled opener still owns its region.
pub(crate) fn opens_block(action: &Action) -> bool {
    matches!(
        unwrapped(action),
        Action::If { .. }
            | Action::While { .. }
            | Action::ForGlobalVariable { .. }
            | Action::ForPlayerVariable { .. }
    )
}

/// One past the last index the `disabled` statement at `index` claims: the
/// `End` matching a disabled block opener, or the statement itself for a
/// leaf. An unclosed disabled opener claims the rest of the list.
fn disabled_end(actions: &[Action], index: usize) -> usize {
    if opens_block(&actions[index]) {
        super::cfg::matching_end(actions, index).map_or(actions.len(), |end| end + 1)
    } else {
        index + 1
    }
}

/// Live `(index, action)` pairs of a rule's flat action list. `disabled`
/// statements and everything inside a disabled block — through the `End`
/// that closes it — are not live and yield nothing.
pub fn live_actions(actions: &[Action]) -> impl Iterator<Item = (usize, &Action)> {
    let mut index = 0;
    std::iter::from_fn(move || {
        while index < actions.len() {
            let current = index;
            index = if matches!(actions[current], Action::Disabled { .. }) {
                disabled_end(actions, current)
            } else {
                current + 1
            };
            if !matches!(actions[current], Action::Disabled { .. }) {
                return Some((current, &actions[current]));
            }
        }
        None
    })
}

/// The [`live_actions`] complement as an index-addressable mask over the
/// same flat action list.
pub(crate) fn dead_actions(actions: &[Action]) -> Vec<bool> {
    let mut dead = vec![true; actions.len()];
    for (index, _) in live_actions(actions) {
        dead[index] = false;
    }
    dead
}

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
