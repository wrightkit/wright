use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use workshop_rs::source::Span;
use workshop_rs::{Action, Event, ModifyOp, Program, Rule, Value};

use crate::registry::LintConfig;
use crate::symbols::{ActionId, RuleId, ValueId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceClass {
    Exact,
    StaticIndicator,
    Heuristic,
}

impl EvidenceClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::StaticIndicator => "static-indicator",
            Self::Heuristic => "heuristic",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Boundedness {
    ObviouslyUnbounded,
    StaticallyBounded,
    Unknown,
}

impl Boundedness {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ObviouslyUnbounded => "obviously-unbounded",
            Self::StaticallyBounded => "statically-bounded",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    pub span: Option<Span>,
    pub rule: RuleId,
    pub action: Option<ActionId>,
    pub value: Option<ValueId>,
    pub evidence: EvidenceClass,
    pub boundedness: Option<Boundedness>,
}

pub fn analyze(program: &Program, config: &LintConfig) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (rule_id, rule) in program.rules.iter().enumerate() {
        if rule.disabled {
            continue;
        }
        for (action_id, action) in rule.actions.iter().enumerate() {
            let Some((start, end)) = loop_body(rule, action_id) else {
                continue;
            };
            let body = &rule.actions[start..end];
            let span = program.action_span(rule_id, action_id);
            if config.is_enabled("min-wait-loop") && body.iter().any(|action| is_wait(action, true))
            {
                findings.push(Finding {
                    code: "min-wait-loop".into(),
                    severity: Severity::Warning,
                    message: "loop body waits at the workshop minimum rate; the loop runs at maximum frequency".into(),
                    span,
                    rule: rule_id,
                    action: Some(action_id),
                    value: None,
                    evidence: EvidenceClass::StaticIndicator,
                    boundedness: None,
                });
            }
            if config.is_enabled("expensive-loop-check")
                && body.iter().any(action_contains_expensive)
            {
                findings.push(Finding {
                    code: "expensive-loop-check".into(),
                    severity: Severity::Warning,
                    message: "loop body evaluates a potentially expensive geometry predicate"
                        .into(),
                    span,
                    rule: rule_id,
                    action: Some(action_id),
                    value: None,
                    evidence: EvidenceClass::Heuristic,
                    boundedness: None,
                });
            }
            if matches!(action, Action::While { .. })
                && config.is_enabled("while-without-wait")
                && !body.iter().any(|action| is_wait(action, false))
            {
                let boundedness = while_boundedness(action, body);
                findings.push(Finding {
                    code: "while-without-wait".into(),
                    severity: Severity::Warning,
                    message: "loop body contains no wait call and may repeat without yielding"
                        .into(),
                    span,
                    rule: rule_id,
                    action: Some(action_id),
                    value: None,
                    evidence: EvidenceClass::StaticIndicator,
                    boundedness: Some(boundedness),
                });
            }
        }
        if config.is_enabled("duplicate-condition") {
            let mut seen = Vec::new();
            for (condition, value) in rule.conditions.iter().enumerate() {
                if seen
                    .iter()
                    .any(|previous: &&Value| values_equal(previous, &value.value))
                {
                    findings.push(Finding {
                        code: "duplicate-condition".into(),
                        severity: Severity::Warning,
                        message: "condition is evaluated more than once in this rule; a later branch can never be taken".into(),
                        span: program.condition_span(rule_id, condition),
                        rule: rule_id,
                        action: None,
                        value: Some(condition),
                        evidence: EvidenceClass::Exact,
                        boundedness: None,
                    });
                }
                seen.push(&value.value);
            }
        }
    }
    let registry = crate::registry::LintRegistry::default();
    for finding in &mut findings {
        if let Some(meta) = registry.rules().find(|meta| meta.id == finding.code) {
            finding.severity = config.effective_severity(meta);
        }
    }
    findings
}

pub fn persistent_objects(program: &Program) -> Vec<JsonValue> {
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
            let span = program
                .action_span(rule, action)
                .map_or(JsonValue::Null, |span| {
                    json!({
                        "file": span.file.index(),
                        "start": {"line": span.start.line, "col": span.start.col},
                        "end": {"line": span.end.line, "col": span.end.col}
                    })
                });
            output.push(json!({
                "kind": kind,
                "rule": rule,
                "action": action,
                "executionScope": execution_scope(&data.event),
                "visibility": object_visibility(args),
                "reevaluation": reevaluation,
                "identityRetained": true,
                "sameKindCleanupInRule": data.actions.iter().any(|action| matches!(action, Action::Call { name, .. } if name == cleanup)),
                "span": span,
            }));
        }
    }
    output
}

pub fn is_wait(action: &Action, minimum: bool) -> bool {
    matches!(
        action,
        Action::Call { name, args }
            if name == "wait" && (!minimum || matches!(args.first(), Some(Value::Number(value)) if *value <= 0.016))
    )
}

fn loop_body(rule: &Rule, action: usize) -> Option<(usize, usize)> {
    if !matches!(
        rule.actions.get(action),
        Some(
            Action::While { .. }
                | Action::ForGlobalVariable { .. }
                | Action::ForPlayerVariable { .. }
        )
    ) {
        return None;
    }
    let mut depth = 0;
    for index in action + 1..rule.actions.len() {
        match rule.actions[index] {
            Action::If { .. }
            | Action::While { .. }
            | Action::ForGlobalVariable { .. }
            | Action::ForPlayerVariable { .. } => depth += 1,
            Action::End if depth == 0 => return Some((action + 1, index)),
            Action::End => depth -= 1,
            _ => {}
        }
    }
    None
}

fn action_contains_expensive(action: &Action) -> bool {
    match action {
        Action::Call { name, args } => {
            ["distance", "raycast", "isInLoS"].contains(&name.as_str())
                || args.iter().any(value_contains_expensive)
        }
        Action::SetGlobalVariable { value, .. } | Action::ModifyGlobalVariable { value, .. } => {
            value_contains_expensive(value)
        }
        Action::SetPlayerVariable { player, value, .. }
        | Action::ModifyPlayerVariable { player, value, .. } => {
            value_contains_expensive(player) || value_contains_expensive(value)
        }
        Action::AssignMember { target, value, .. } => {
            value_contains_expensive(target) || value_contains_expensive(value)
        }
        Action::If { condition } | Action::ElseIf { condition } | Action::While { condition } => {
            value_contains_expensive(condition)
        }
        Action::ForGlobalVariable {
            start, stop, step, ..
        }
        | Action::ForPlayerVariable {
            start, stop, step, ..
        } => [start, stop, step]
            .iter()
            .any(|value| value_contains_expensive(value)),
        Action::Disabled { action } => action_contains_expensive(action),
        _ => false,
    }
}

fn value_contains_expensive(value: &Value) -> bool {
    match value {
        Value::Call { name, args } => {
            ["distance", "raycast", "isInLoS"].contains(&name.as_str())
                || args.iter().any(value_contains_expensive)
        }
        Value::Array(values) => values.iter().any(value_contains_expensive),
        Value::Vector { x, y, z } => [x, y, z]
            .iter()
            .any(|value| value_contains_expensive(value)),
        Value::PlayerVariable { player, .. } => value_contains_expensive(player),
        _ => false,
    }
}

fn values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::String(a), Value::String(b))
        | (Value::LocalizedString(a), Value::LocalizedString(b))
        | (Value::GlobalVariable(a), Value::GlobalVariable(b))
        | (Value::Subroutine(a), Value::Subroutine(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Null, Value::Null) | (Value::EventPlayer, Value::EventPlayer) => true,
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| values_equal(a, b))
        }
        (Value::Call { name: an, args: aa }, Value::Call { name: bn, args: ba }) => {
            an == bn && aa.len() == ba.len() && aa.iter().zip(ba).all(|(a, b)| values_equal(a, b))
        }
        (
            Value::Vector {
                x: ax,
                y: ay,
                z: az,
            },
            Value::Vector {
                x: bx,
                y: by,
                z: bz,
            },
        ) => values_equal(ax, bx) && values_equal(ay, by) && values_equal(az, bz),
        (
            Value::Enum {
                value_type: at,
                value: av,
            },
            Value::Enum {
                value_type: bt,
                value: bv,
            },
        ) => at == bt && av == bv,
        (
            Value::PlayerVariable {
                player: ap,
                variable: av,
            },
            Value::PlayerVariable {
                player: bp,
                variable: bv,
            },
        ) => av == bv && values_equal(ap, bp),
        _ => false,
    }
}

fn while_boundedness(action: &Action, body: &[Action]) -> Boundedness {
    if matches!(
        action,
        Action::While {
            condition: Value::Bool(true)
        }
    ) {
        Boundedness::ObviouslyUnbounded
    } else if body.iter().any(|action| {
        matches!(
            action,
            Action::ModifyGlobalVariable {
                op: ModifyOp::Add | ModifyOp::Subtract,
                value: Value::Number(_),
                ..
            }
        )
    }) {
        Boundedness::StaticallyBounded
    } else {
        Boundedness::Unknown
    }
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
        Event::EachPlayer => "per-player",
        Event::Subroutine(_) => "subroutine",
        _ => "unknown",
    }
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
