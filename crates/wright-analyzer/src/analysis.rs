use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use workshop_rs::source::Span;
use workshop_rs::{Action, Event, ModifyOp, Program, Rule, Value};

use crate::registry::LintConfig;
use crate::symbols::{ActionId, RuleId, ValueId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceClass {
    Exact,
    StaticIndicator,
    Heuristic,
    RuntimeValidated,
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

        // duplicate-condition
        if config.is_enabled("duplicate-condition") {
            let mut seen = Vec::new();
            for (cond_idx, cond) in rule.conditions.iter().enumerate() {
                if seen.iter().any(|prev: &&Value| values_equal(prev, &cond.value)) {
                    findings.push(Finding {
                        code: "duplicate-condition".into(),
                        severity: Severity::Warning,
                        message: "condition is evaluated more than once in this rule; a later branch can never be taken".into(),
                        span: program.condition_span(rule_id, cond_idx),
                        rule: rule_id,
                        action: None,
                        value: Some(cond_idx),
                        evidence: EvidenceClass::Exact,
                        boundedness: None,
                    });
                } else {
                    seen.push(&cond.value);
                }
            }
        }

        // ongoing-condition-hot-path
        if config.is_enabled("ongoing-condition-hot-path") && is_ongoing_event(&rule.event) {
            let total = rule.conditions.len();
            for (idx, cond) in rule.conditions.iter().enumerate() {
                let mut expensive_spans = Vec::new();
                collect_expensive_values(&cond.value, program.condition_span(rule_id, idx), &mut expensive_spans);
                for span in expensive_spans {
                    let preceding = idx;
                    let later = total.saturating_sub(idx + 1);
                    let evaluation = if preceding == 0 {
                        "is evaluated every server tick".to_string()
                    } else if preceding == 1 {
                        "is evaluated only after 1 preceding condition passes".to_string()
                    } else {
                        format!("is evaluated only after {preceding} preceding conditions pass")
                    };
                    let later_gates = if later == 0 {
                        String::new()
                    } else {
                        format!(", before {later} later short-circuit gate{}", if later == 1 { "" } else { "s" })
                    };
                    findings.push(Finding {
                        code: "ongoing-condition-hot-path".into(),
                        severity: Severity::Info,
                        message: format!(
                            "geometry predicate in an ongoing-rule condition {} of {total} {evaluation}{later_gates}; its cost is heuristic, not measured runtime load",
                            idx + 1
                        ),
                        span,
                        rule: rule_id,
                        action: None,
                        value: Some(idx),
                        evidence: EvidenceClass::Heuristic,
                        boundedness: None,
                    });
                }
            }
        }

        // Loop checks
        for (action_id, action) in rule.actions.iter().enumerate() {
            let Some((start, end)) = loop_body(rule, action_id) else {
                continue;
            };
            let body = &rule.actions[start..end];
            let span = program.action_span(rule_id, action_id);

            // min-wait-loop
            if config.is_enabled("min-wait-loop") && body.iter().any(|a| is_wait(a, true)) {
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

            // expensive-loop-check
            if config.is_enabled("expensive-loop-check") && body.iter().any(action_contains_expensive) {
                findings.push(Finding {
                    code: "expensive-loop-check".into(),
                    severity: Severity::Warning,
                    message: "loop body evaluates a potentially expensive geometry predicate".into(),
                    span,
                    rule: rule_id,
                    action: Some(action_id),
                    value: None,
                    evidence: EvidenceClass::Heuristic,
                    boundedness: None,
                });
            }

            // while-without-wait
            if matches!(action, Action::While { .. })
                && config.is_enabled("while-without-wait")
                && !body.iter().any(|a| is_wait(a, false))
            {
                let boundedness = while_boundedness(action, body);
                findings.push(Finding {
                    code: "while-without-wait".into(),
                    severity: Severity::Warning,
                    message: "loop body contains no wait call and may repeat without yielding".into(),
                    span,
                    rule: rule_id,
                    action: Some(action_id),
                    value: None,
                    evidence: EvidenceClass::StaticIndicator,
                    boundedness: Some(boundedness),
                });
            }

            // repeated-value
            if config.is_enabled("repeated-value") {
                let loop_cond = match action {
                    Action::While { condition } => Some(condition),
                    _ => None,
                };
                let mut values_in_scope = Vec::new();
                if let Some(cond) = loop_cond {
                    collect_values(cond, &mut values_in_scope);
                }
                for a in body {
                    collect_action_values_shallow(a, &mut values_in_scope);
                }
                for (_family, count) in find_repeated_values(&values_in_scope) {
                    findings.push(Finding {
                        code: "repeated-value".into(),
                        severity: Severity::Warning,
                        message: format!("this value expression is evaluated {count} times within the same loop scope"),
                        span,
                        rule: rule_id,
                        action: Some(action_id),
                        value: None,
                        evidence: EvidenceClass::Exact,
                        boundedness: None,
                    });
                }
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

fn loop_body(rule: &Rule, action: usize) -> Option<(usize, usize)> {
    if !matches!(
        rule.actions.get(action),
        Some(Action::While { .. } | Action::ForGlobalVariable { .. } | Action::ForPlayerVariable { .. })
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

fn is_wait(action: &Action, minimum: bool) -> bool {
    matches!(
        action,
        Action::Call { name, args }
            if name == "wait" && (!minimum || matches!(args.first(), Some(Value::Number(v)) if *v <= 0.016))
    )
}

fn is_ongoing_event(event: &Event) -> bool {
    matches!(event, Event::Global | Event::EachPlayer)
}

fn is_expensive_name(name: &str) -> bool {
    matches!(name, "distance" | "raycast" | "isInLoS")
}

fn action_contains_expensive(action: &Action) -> bool {
    match action {
        Action::Call { name, args } => is_expensive_name(name) || args.iter().any(value_contains_expensive),
        Action::SetGlobalVariable { value, .. } | Action::ModifyGlobalVariable { value, .. } => value_contains_expensive(value),
        Action::SetPlayerVariable { player, value, .. } | Action::ModifyPlayerVariable { player, value, .. } => {
            value_contains_expensive(player) || value_contains_expensive(value)
        }
        Action::AssignMember { target, value, .. } => value_contains_expensive(target) || value_contains_expensive(value),
        Action::If { condition } | Action::ElseIf { condition } | Action::While { condition } => value_contains_expensive(condition),
        Action::ForGlobalVariable { start, stop, step, .. } => {
            value_contains_expensive(start) || value_contains_expensive(stop) || value_contains_expensive(step)
        }
        Action::ForPlayerVariable { player, start, stop, step, .. } => {
            value_contains_expensive(player) || value_contains_expensive(start) || value_contains_expensive(stop) || value_contains_expensive(step)
        }
        Action::Disabled { action } => action_contains_expensive(action),
        _ => false,
    }
}

fn value_contains_expensive(value: &Value) -> bool {
    match value {
        Value::Call { name, args } => is_expensive_name(name) || args.iter().any(value_contains_expensive),
        Value::Array(elements) => elements.iter().any(value_contains_expensive),
        Value::Vector { x, y, z } => value_contains_expensive(x) || value_contains_expensive(y) || value_contains_expensive(z),
        Value::PlayerVariable { player, .. } => value_contains_expensive(player),
        _ => false,
    }
}

fn collect_expensive_values(value: &Value, span: Option<Span>, out: &mut Vec<Option<Span>>) {
    if let Value::Call { name, args } = value {
        if is_expensive_name(name) {
            out.push(span);
        }
        for arg in args {
            collect_expensive_values(arg, span, out);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CounterVar {
    Global(String),
    Player(String),
}

fn while_boundedness(action: &Action, body: &[Action]) -> Boundedness {
    let Action::While { condition } = action else {
        return Boundedness::Unknown;
    };
    if matches!(condition, Value::Bool(true)) {
        return Boundedness::ObviouslyUnbounded;
    }
    let Some((variable, toward)) = counter_comparison(condition) else {
        return Boundedness::Unknown;
    };

    let mut progresses = false;
    for a in body {
        if body_action_has_unprovable_loop(a) {
            return Boundedness::Unknown;
        }
        match modify_direction(a, &variable) {
            Some(dir) if dir == toward => progresses = true,
            Some(_) => return Boundedness::Unknown,
            None if action_writes_var(a, &variable) => return Boundedness::Unknown,
            None => {}
        }
    }

    if progresses {
        Boundedness::StaticallyBounded
    } else {
        Boundedness::Unknown
    }
}

fn counter_comparison(condition: &Value) -> Option<(CounterVar, i32)> {
    let Value::Call { name, args } = condition else {
        return None;
    };
    if args.len() != 2 {
        return None;
    }
    let var_left = var_of_value(&args[0]);
    let lit_left = matches!(&args[0], Value::Number(_));
    let var_right = var_of_value(&args[1]);
    let lit_right = matches!(&args[1], Value::Number(_));

    let (variable, var_is_left) = match (var_left, lit_right) {
        (Some(v), true) => (v, true),
        (None, false) => match (lit_left, var_right) {
            (true, Some(v)) => (v, false),
            _ => return None,
        },
        _ => return None,
    };

    let toward = match (name.as_str(), var_is_left) {
        ("<", true) | ("<=", true) | (">", false) | (">=", false) => 1,
        (">", true) | (">=", true) | ("<", false) | ("<=", false) => -1,
        _ => return None,
    };
    Some((variable, toward))
}

fn var_of_value(value: &Value) -> Option<CounterVar> {
    match value {
        Value::GlobalVariable(name) => Some(CounterVar::Global(name.clone())),
        Value::PlayerVariable { variable, .. } => Some(CounterVar::Player(variable.clone())),
        _ => None,
    }
}

fn modify_direction(action: &Action, variable: &CounterVar) -> Option<i32> {
    match (action, variable) {
        (Action::ModifyGlobalVariable { variable: target, op, value }, CounterVar::Global(wanted)) if target == wanted => {
            step_dir(op, value)
        }
        (Action::ModifyPlayerVariable { variable: target, op, value, .. }, CounterVar::Player(wanted)) if target == wanted => {
            step_dir(op, value)
        }
        _ => None,
    }
}

fn step_dir(op: &ModifyOp, value: &Value) -> Option<i32> {
    let Value::Number(step) = value else {
        return None;
    };
    if *step == 0.0 {
        return None;
    }
    let sign = if *step > 0.0 { 1 } else { -1 };
    match op {
        ModifyOp::Add => Some(sign),
        ModifyOp::Subtract => Some(-sign),
        _ => None,
    }
}

fn action_writes_var(action: &Action, variable: &CounterVar) -> bool {
    match (action, variable) {
        (Action::SetGlobalVariable { variable: target, .. } | Action::ModifyGlobalVariable { variable: target, .. }, CounterVar::Global(w)) => target == w,
        (Action::SetPlayerVariable { variable: target, .. } | Action::ModifyPlayerVariable { variable: target, .. }, CounterVar::Player(w)) => target == w,
        (Action::CallSubroutine { .. } | Action::AssignMember { .. }, _) => true,
        _ => false,
    }
}

fn body_action_has_unprovable_loop(action: &Action) -> bool {
    match action {
        Action::While { condition } => !matches!(condition, Value::Bool(false)),
        _ => false,
    }
}

fn collect_values<'a>(value: &'a Value, out: &mut Vec<&'a Value>) {
    out.push(value);
    match value {
        Value::Call { args, .. } => {
            for arg in args {
                collect_values(arg, out);
            }
        }
        Value::Array(elements) => {
            for el in elements {
                collect_values(el, out);
            }
        }
        Value::Vector { x, y, z } => {
            collect_values(x, out);
            collect_values(y, out);
            collect_values(z, out);
        }
        Value::PlayerVariable { player, .. } => collect_values(player, out),
        _ => {}
    }
}

fn collect_action_values_shallow<'a>(action: &'a Action, out: &mut Vec<&'a Value>) {
    match action {
        Action::While { .. } | Action::ForGlobalVariable { .. } | Action::ForPlayerVariable { .. } => {}
        Action::SetGlobalVariable { value, .. } | Action::ModifyGlobalVariable { value, .. } => collect_values(value, out),
        Action::SetPlayerVariable { player, value, .. } | Action::ModifyPlayerVariable { player, value, .. } => {
            collect_values(player, out);
            collect_values(value, out);
        }
        Action::AssignMember { target, value, .. } => {
            collect_values(target, out);
            collect_values(value, out);
        }
        Action::Call { args, .. } => {
            for arg in args {
                collect_values(arg, out);
            }
        }
        Action::If { condition } | Action::ElseIf { condition } => collect_values(condition, out),
        _ => {}
    }
}

fn count_calls(value: &Value) -> usize {
    let mut count = 0;
    match value {
        Value::Call { args, .. } => {
            count += 1;
            for arg in args {
                count += count_calls(arg);
            }
        }
        Value::Array(elements) => {
            for el in elements {
                count += count_calls(el);
            }
        }
        Value::Vector { x, y, z } => count += count_calls(x) + count_calls(y) + count_calls(z),
        Value::PlayerVariable { player, .. } => count += count_calls(player),
        _ => {}
    }
    count
}

fn find_repeated_values<'a>(values: &[&'a Value]) -> Vec<(&'a Value, usize)> {
    let mut families: Vec<(&'a Value, usize)> = Vec::new();
    for &v in values {
        if count_calls(v) < 2 {
            continue;
        }
        if let Some((_, count)) = families.iter_mut().find(|(lead, _)| values_equal(lead, v)) {
            *count += 1;
        } else {
            families.push((v, 1));
        }
    }
    families.retain(|(_, count)| *count >= 2);
    // Subsumption: exclude candidates that are subtrees of larger candidates
    let mut survivors = Vec::new();
    for i in 0..families.len() {
        let is_subsumed = families.iter().enumerate().any(|(j, (other, _))| {
            i != j && contains_value_subtree(other, families[i].0)
        });
        if !is_subsumed {
            survivors.push(families[i]);
        }
    }
    survivors
}

fn contains_value_subtree(tree: &Value, target: &Value) -> bool {
    if values_equal(tree, target) {
        return false;
    }
    match tree {
        Value::Call { args, .. } => args.iter().any(|arg| values_equal(arg, target) || contains_value_subtree(arg, target)),
        Value::Array(elements) => elements.iter().any(|el| values_equal(el, target) || contains_value_subtree(el, target)),
        Value::Vector { x, y, z } => {
            values_equal(x, target) || contains_value_subtree(x, target)
                || values_equal(y, target) || contains_value_subtree(y, target)
                || values_equal(z, target) || contains_value_subtree(z, target)
        }
        Value::PlayerVariable { player, .. } => values_equal(player, target) || contains_value_subtree(player, target),
        _ => false,
    }
}

pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::String(a), Value::String(b))
        | (Value::LocalizedString(a), Value::LocalizedString(b))
        | (Value::GlobalVariable(a), Value::GlobalVariable(b))
        | (Value::Subroutine(a), Value::Subroutine(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Null, Value::Null) | (Value::EventPlayer, Value::EventPlayer) => true,
        (Value::Array(a), Value::Array(b)) => a.len() == b.len() && a.iter().zip(b).all(|(a, b)| values_equal(a, b)),
        (Value::Call { name: an, args: aa }, Value::Call { name: bn, args: ba }) => {
            an == bn && aa.len() == ba.len() && aa.iter().zip(ba).all(|(a, b)| values_equal(a, b))
        }
        (Value::Vector { x: ax, y: ay, z: az }, Value::Vector { x: bx, y: by, z: bz }) => {
            values_equal(ax, bx) && values_equal(ay, by) && values_equal(az, bz)
        }
        (Value::Enum { value_type: at, value: av }, Value::Enum { value_type: bt, value: bv }) => at == bt && av == bv,
        (Value::PlayerVariable { player: ap, variable: av }, Value::PlayerVariable { player: bp, variable: bv }) => {
            av == bv && values_equal(ap, bp)
        }
        _ => false,
    }
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
            let identity_name = match kind {
                "hud-text" | "in-world-text" => "lastTextId",
                _ => "lastCreatedEntity",
            };
            let identity_retained = data.actions.get(action + 1).is_some_and(|next| {
                action_retains_identity(next, identity_name)
            });
            let reevaluation_index = if kind == "hud-text" { 9 } else { 5 };
            let reevaluation = args.get(reevaluation_index).and_then(|value| match value {
                Value::Enum { value_type, value } => Some(json!({"domain": value_type, "mode": value})),
                _ => None,
            });
            let span = program.action_span(rule, action).map_or(JsonValue::Null, |span| {
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
                "identityRetained": identity_retained,
                "sameKindCleanupInRule": data.actions.iter().any(|a| matches!(a, Action::Call { name, .. } if name == cleanup)),
                "span": span,
            }));
        }
    }
    output
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
        Some(Value::Call { .. } | Value::PlayerVariable { .. } | Value::GlobalVariable(_)) => "dynamic",
        _ => "unknown",
    }
}
