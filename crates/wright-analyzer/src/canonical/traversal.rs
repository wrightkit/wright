use workshop_rs::{Action, Value};

pub(crate) fn unwrapped(action: &Action) -> &Action {
    let mut current = action;
    while let Action::Disabled { action } = current {
        current = action;
    }
    current
}

pub(crate) fn opens_block(action: &Action) -> bool {
    matches!(
        unwrapped(action),
        Action::If { .. }
            | Action::While { .. }
            | Action::ForGlobalVariable { .. }
            | Action::ForPlayerVariable { .. }
    )
}

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

pub(crate) fn dead_actions(actions: &[Action]) -> Vec<bool> {
    let mut dead = vec![true; actions.len()];
    for (index, _) in live_actions(actions) {
        dead[index] = false;
    }
    dead
}

/// The subroutine a statement calls, when it statically names one: an
/// explicit `Call Subroutine`, or the `Start Rule` builtin whose first
/// argument is a `Value::Subroutine`. Every other builtin `Call` resolves
/// by catalog id, never by matching a declared subroutine's name.
pub fn subroutine_callee(action: &Action) -> Option<&str> {
    match action {
        Action::CallSubroutine { subroutine } => Some(subroutine.as_str()),
        Action::Call { name, args } if name == "startRule" => match args.first() {
            Some(Value::Subroutine(subroutine)) => Some(subroutine.as_str()),
            _ => None,
        },
        _ => None,
    }
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

pub(super) fn visit_value_tree<'a>(
    value: &'a Value,
    parent: Option<usize>,
    visit: &mut impl FnMut(&'a Value, Option<usize>, &[usize]) -> usize,
) {
    let mut path = Vec::new();
    visit_value_tree_at(value, parent, &mut path, visit);
}

pub(super) fn value_children(value: &Value) -> Vec<&Value> {
    match value {
        Value::Array(values) | Value::Call { args: values, .. } => values.iter().collect(),
        Value::Vector { x, y, z } => vec![x, y, z],
        Value::PlayerVariable { player, .. } => vec![player],
        _ => Vec::new(),
    }
}

fn visit_value_tree_at<'a>(
    value: &'a Value,
    parent: Option<usize>,
    path: &mut Vec<usize>,
    visit: &mut impl FnMut(&'a Value, Option<usize>, &[usize]) -> usize,
) {
    let parent = Some(visit(value, parent, path));
    for (index, child) in value_children(value).into_iter().enumerate() {
        path.push(index);
        visit_value_tree_at(child, parent, path, visit);
        path.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_tree_keeps_preorder_identities_parents_and_paths() {
        let value = Value::Array(vec![
            Value::call(
                "outer",
                [
                    Value::Vector {
                        x: Box::new(Value::Number(1.0)),
                        y: Box::new(Value::Number(2.0)),
                        z: Box::new(Value::Number(3.0)),
                    },
                    Value::PlayerVariable {
                        player: Box::new(Value::EventPlayer),
                        variable: "state".into(),
                    },
                ],
            ),
            Value::Number(4.0),
            Value::Array(Vec::new()),
            Value::call("empty", []),
        ]);
        let mut visits = Vec::new();
        visit_value_tree(&value, Some(42), &mut |value, parent, path| {
            let number = match value {
                Value::Number(number) => Some(*number),
                _ => None,
            };
            let id = visits.len();
            visits.push((number, parent, path.to_vec()));
            id
        });
        assert_eq!(
            visits,
            [
                (None, Some(42), vec![]),
                (None, Some(0), vec![0]),
                (None, Some(1), vec![0, 0]),
                (Some(1.0), Some(2), vec![0, 0, 0]),
                (Some(2.0), Some(2), vec![0, 0, 1]),
                (Some(3.0), Some(2), vec![0, 0, 2]),
                (None, Some(1), vec![0, 1]),
                (None, Some(6), vec![0, 1, 0]),
                (Some(4.0), Some(0), vec![1]),
                (None, Some(0), vec![2]),
                (None, Some(0), vec![3]),
            ]
        );
    }
}
