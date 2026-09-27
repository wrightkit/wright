use super::service::span_json;
use serde_json::{Value as JsonValue, json};
use workshop_rs::{Action, Event, Program, Value};

pub(super) fn persistent_objects(program: &Program) -> Vec<JsonValue> {
    let mut output = Vec::new();
    for (rule, data) in program.rules.iter().enumerate() {
        for (action, action_data) in data.actions.iter().enumerate() {
            let Action::Call { name, args } = action_data else {
                continue;
            };
            let Some(kind) = persistent_object_kind(name) else {
                continue;
            };
            let cleanup = match kind {
                "hud-text" => "destroyHudText",
                "in-world-text" => "destroyInWorldText",
                "effect" => "destroyEffect",
                _ => continue,
            };
            let reevaluation_index = if kind == "hud-text" { 9 } else { 5 };
            let reevaluation = args.get(reevaluation_index).and_then(|value| match value {
                Value::Enum { value_type, value } => {
                    Some(json!({"domain": value_type, "mode": value}))
                }
                _ => None,
            });
            let identity = match kind {
                "hud-text" | "in-world-text" => "lastTextId",
                "effect" => "lastCreatedEntity",
                _ => unreachable!(),
            };
            let identity_retained = data
                .actions
                .get(action + 1)
                .is_some_and(|next| action_retains_identity(next, identity));
            let span = span_json(program.action_span(rule, action));
            output.push(json!({
                "kind": kind,
                "rule": rule,
                "action": action,
                "executionScope": execution_scope(&data.event),
                "visibility": object_visibility(args),
                "reevaluation": reevaluation,
                "identityRetained": identity_retained,
                "sameKindCleanupInRule": data.actions.iter().any(|action| matches!(action, Action::Call { name, .. } if name == cleanup)),
                "span": span,
            }));
        }
    }
    output
}

fn persistent_object_kind(name: &str) -> Option<&'static str> {
    match name {
        "createHudText" => Some("hud-text"),
        "createInWorldText" => Some("in-world-text"),
        "createEffect" => Some("effect"),
        _ => None,
    }
}

fn execution_scope(event: &Event) -> &'static str {
    match event {
        Event::Global => "global",
        Event::EachPlayer | Event::EachPlayerWithFilters { .. } | Event::Player { .. } => {
            "per-player"
        }
        Event::Subroutine(_) => "subroutine",
    }
}

fn action_retains_identity(action: &Action, identity: &str) -> bool {
    let value = match action {
        Action::SetGlobalVariable { value, .. }
        | Action::SetPlayerVariable { value, .. }
        | Action::AssignMember { value, .. } => value,
        _ => return false,
    };
    matches!(value, Value::Call { name, args } if name == identity && args.is_empty())
}

fn object_visibility(args: &[Value]) -> &'static str {
    match args.first() {
        Some(Value::EventPlayer) => "event-player",
        Some(Value::Array(_)) => "explicit-set",
        Some(Value::Call { name, .. }) if name == "allPlayers" => "all-players",
        Some(Value::Call { .. } | Value::PlayerVariable { .. } | Value::GlobalVariable(_)) => {
            "dynamic"
        }
        _ => "unknown",
    }
}
