use serde_json::json;
use workshop_rs::{Action, Program};

use crate::analysis::is_wait;
use crate::service::{ErrorInfo, Response};
use crate::symbols::RuleId;

pub fn cfg_response(program: &Program, rule: RuleId) -> Response {
    let Some(data) = program.rules.get(rule) else {
        return Response::Error {
            error: ErrorInfo {
                code: "invalid-id".into(),
                message: format!("unknown rule {rule}"),
            },
        };
    };
    let loop_actions: Vec<(usize, &str)> = data
        .actions
        .iter()
        .enumerate()
        .filter_map(|(action, action_data)| {
            let kind = match action_data {
                Action::While { .. } => "while",
                Action::ForGlobalVariable { .. } | Action::ForPlayerVariable { .. } => "for",
                _ => return None,
            };
            Some((action, kind))
        })
        .collect();
    let exit = loop_actions.len() + 1;
    let mut blocks = vec![json!({
        "id": 0,
        "kind": "entry",
        "waits": data.actions.iter().any(|action| is_wait(action, false)),
        "calls": [],
        "actions": if loop_actions.is_empty() { (0..data.actions.len()).collect::<Vec<_>>() } else { Vec::new() },
        "successors": [{"to": 1, "kind": "fallthrough"}]
    })];
    for (offset, (action, kind)) in loop_actions.iter().enumerate() {
        blocks.push(json!({
            "id": offset + 1,
            "kind": kind,
            "waits": is_wait(&data.actions[*action], false),
            "calls": [],
            "actions": [action],
            "successors": [
                {"to": offset + 1, "kind": "back-edge"},
                {"to": if offset + 1 == exit { exit } else { offset + 2 }, "kind": "fallthrough"}
            ]
        }));
    }
    blocks.push(json!({
        "id": exit,
        "kind": "exit",
        "waits": false,
        "calls": [],
        "actions": [],
        "successors": []
    }));
    Response::Ok {
        result: json!({"entry": 0, "exit": exit, "blocks": blocks}),
    }
}
