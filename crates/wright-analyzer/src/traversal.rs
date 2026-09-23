use workshop_rs::wir::{self, Action, ActionId};

/// Visit nested action bodies in pre-order, preserving branch order.
pub(crate) fn visit_actions(
    program: &wir::Program,
    actions: &[ActionId],
    f: &mut impl FnMut(ActionId, &Action),
) {
    for id in actions {
        let Some(action) = program.actions.get(*id) else {
            continue;
        };
        f(*id, action);
        match action {
            Action::If {
                branches,
                else_body,
                ..
            } => {
                for branch in branches {
                    visit_actions(program, &branch.body, f);
                }
                if let Some(body) = else_body {
                    visit_actions(program, body, f);
                }
            }
            Action::While { body, .. }
            | Action::ForGlobalVariable { body, .. }
            | Action::ForPlayerVariable { body, .. } => visit_actions(program, body, f),
            Action::SetGlobalVariable { .. }
            | Action::ModifyGlobalVariable { .. }
            | Action::SetPlayerVariable { .. }
            | Action::ModifyPlayerVariable { .. }
            | Action::CallSubroutine { .. }
            | Action::AssignMember { .. }
            | Action::Call { .. } => {}
        }
    }
}
