use serde_json::json;
use workshop_rs::{Action, Program};

use super::symbols::RuleId;
use crate::service::{ErrorInfo, Response};

pub(super) fn is_wait(action: &Action, minimum: bool) -> bool {
    matches!(action, Action::Call { name, args } if name == "wait" && (!minimum || matches!(args.first(), Some(workshop_rs::Value::Number(value)) if *value <= 0.016)))
}

pub(crate) fn matching_end(actions: &[Action], start: usize) -> Option<usize> {
    if !matches!(
        actions.get(start),
        Some(
            Action::If { .. }
                | Action::While { .. }
                | Action::ForGlobalVariable { .. }
                | Action::ForPlayerVariable { .. }
        )
    ) {
        return None;
    }
    let mut depth = 0;
    for (index, action) in actions.iter().enumerate().skip(start + 1) {
        match action {
            Action::If { .. }
            | Action::While { .. }
            | Action::ForGlobalVariable { .. }
            | Action::ForPlayerVariable { .. } => depth += 1,
            Action::End if depth == 0 => return Some(index),
            Action::End => depth -= 1,
            _ => {}
        }
    }
    None
}

pub(super) fn cfg_response(program: &Program, rule: RuleId) -> Response {
    let Some(data) = program.rules.get(rule) else {
        return Response::Error {
            error: ErrorInfo {
                code: "invalid-id".into(),
                message: format!("unknown rule {rule}"),
            },
        };
    };
    let mut builder = CanonicalCfgBuilder {
        program,
        actions: &data.actions,
        blocks: Vec::new(),
    };
    let (entry, terminal) = builder.sequence(0, data.actions.len(), "entry");
    let exit = builder.new_block("exit");
    builder.edge(terminal, exit, "fallthrough");
    Response::Ok {
        result: json!({
            "entry": entry,
            "exit": exit,
            "blocks": builder.blocks.iter().enumerate().map(|(id, block)| json!({
                "id": id,
                "kind": block.kind,
                "waits": block.waits,
                "calls": block.calls,
                "actions": block.actions,
                "successors": block.successors.iter().map(|(to, kind)| json!({"to": to, "kind": kind})).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }),
    }
}

struct CanonicalCfgBlock {
    kind: &'static str,
    waits: bool,
    calls: Vec<usize>,
    actions: Vec<usize>,
    successors: Vec<(usize, &'static str)>,
}

struct CanonicalCfgBuilder<'a> {
    program: &'a Program,
    actions: &'a [Action],
    blocks: Vec<CanonicalCfgBlock>,
}

struct IfBranch {
    condition_action: usize,
    body_start: usize,
    body_end: usize,
}

struct IfParts {
    close: usize,
    branches: Vec<IfBranch>,
    else_body: Option<(usize, usize)>,
}

impl CanonicalCfgBuilder<'_> {
    fn new_block(&mut self, kind: &'static str) -> usize {
        let id = self.blocks.len();
        self.blocks.push(CanonicalCfgBlock {
            kind,
            waits: false,
            calls: Vec::new(),
            actions: Vec::new(),
            successors: Vec::new(),
        });
        id
    }

    fn edge(&mut self, from: usize, to: usize, kind: &'static str) {
        self.blocks[from].successors.push((to, kind));
    }

    fn sequence(&mut self, start: usize, end: usize, entry_kind: &'static str) -> (usize, usize) {
        let entry = self.new_block(entry_kind);
        let mut current = entry;
        let mut index = start;
        while index < end {
            if matches!(self.actions[index], Action::If { .. }) {
                if let Some(parts) = if_parts(self.actions, index) {
                    let merge = self.new_block("block");
                    let mut false_target = if let Some((else_start, else_end)) = parts.else_body {
                        let (else_entry, else_exit) = self.sequence(else_start, else_end, "block");
                        self.edge(else_exit, merge, "fallthrough");
                        Some(else_entry)
                    } else {
                        None
                    };
                    for branch_data in parts.branches.into_iter().rev() {
                        let branch = self.new_block("if");
                        self.blocks[branch]
                            .actions
                            .push(branch_data.condition_action);
                        let (body_entry, body_exit) =
                            self.sequence(branch_data.body_start, branch_data.body_end, "block");
                        self.edge(branch, body_entry, "true");
                        self.edge(body_exit, merge, "fallthrough");
                        self.edge(branch, false_target.unwrap_or(merge), "false");
                        false_target = Some(branch);
                    }
                    self.edge(current, false_target.unwrap_or(merge), "fallthrough");
                    current = merge;
                    index = parts.close + 1;
                    continue;
                }
            }
            if matches!(
                self.actions[index],
                Action::While { .. }
                    | Action::ForGlobalVariable { .. }
                    | Action::ForPlayerVariable { .. }
            ) {
                if let Some(close) = matching_end(self.actions, index) {
                    let kind = if matches!(self.actions[index], Action::While { .. }) {
                        "while"
                    } else {
                        "for"
                    };
                    let header = self.new_block(kind);
                    self.blocks[header].actions.push(index);
                    self.edge(current, header, "fallthrough");
                    let (body_entry, body_exit) = self.sequence(index + 1, close, "block");
                    self.edge(
                        header,
                        body_entry,
                        if kind == "while" {
                            "true"
                        } else {
                            "fallthrough"
                        },
                    );
                    self.edge(body_exit, header, "back");
                    let after = self.new_block("block");
                    self.edge(header, after, "loop-exit");
                    current = after;
                    index = close + 1;
                    continue;
                }
            }
            match &self.actions[index] {
                Action::Else | Action::ElseIf { .. } | Action::End => {}
                action => {
                    let block = &mut self.blocks[current];
                    block.actions.push(index);
                    if is_wait(action, false) {
                        block.waits = true;
                    }
                    if let Some(subroutine) = action_subroutine(self.program, action) {
                        block.calls.push(subroutine);
                    }
                }
            }
            index += 1;
        }
        (entry, current)
    }
}

fn if_parts(actions: &[Action], start: usize) -> Option<IfParts> {
    let close = matching_end(actions, start)?;
    let mut branches = Vec::new();
    let mut condition_action = start;
    let mut body_start = start + 1;
    let mut index = body_start;
    let mut else_body = None;
    while index < close {
        match actions[index] {
            Action::ElseIf { .. } => {
                branches.push(IfBranch {
                    condition_action,
                    body_start,
                    body_end: index,
                });
                condition_action = index;
                body_start = index + 1;
            }
            Action::Else => {
                branches.push(IfBranch {
                    condition_action,
                    body_start,
                    body_end: index,
                });
                else_body = Some((index + 1, close));
                break;
            }
            _ => {
                if let Some(nested_close) = matching_end(actions, index) {
                    index = nested_close + 1;
                    continue;
                }
            }
        }
        index += 1;
    }
    if else_body.is_none() {
        branches.push(IfBranch {
            condition_action,
            body_start,
            body_end: close,
        });
    }
    Some(IfParts {
        close,
        branches,
        else_body,
    })
}

fn action_subroutine(program: &Program, action: &Action) -> Option<usize> {
    let name = match action {
        Action::CallSubroutine { subroutine } => subroutine,
        Action::Call { name, .. } => name,
        _ => return None,
    };
    // Name resolution mirrors workshop-rs: a redeclared name binds to the
    // last declaration.
    program
        .subroutines
        .iter()
        .rposition(|subroutine| subroutine.name == *name)
}
