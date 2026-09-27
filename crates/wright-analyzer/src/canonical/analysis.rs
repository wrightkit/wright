use std::collections::HashMap;

use workshop_rs::source::Span;
use workshop_rs::{Action, Event, ModifyOp, Program, Rule, Value};

use super::cfg::{is_wait, matching_end};
use super::symbols::{ActionId, RuleId, ValueId, value_identity_map, value_occurrence};
use super::traversal::{visit_action_roots, visit_value_tree};
use crate::analysis::{Boundedness, EvidenceClass, Severity};
use crate::registry::LintConfig;

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
    let value_ids = value_identity_map(program);
    let mut findings = Vec::new();
    for (rule_id, rule) in program.rules.iter().enumerate() {
        if rule.disabled {
            continue;
        }
        if config.is_enabled("ongoing-condition-hot-path") {
            findings.extend(ongoing_condition_findings(
                program, rule_id, rule, &value_ids,
            ));
        }
        for (action_id, action) in rule.actions.iter().enumerate() {
            let Some((start, end)) = loop_body(rule, action_id) else {
                continue;
            };
            let body = &rule.actions[start..end];
            if config.is_enabled("min-wait-loop") && body.iter().any(|action| is_wait(action, true))
            {
                findings.push(Finding { code: "min-wait-loop".into(), severity: Severity::Warning, message: "loop body waits at the workshop minimum rate; the loop runs at maximum frequency".into(), span: program.action_span(rule_id, action_id), rule: rule_id, action: Some(action_id), value: None, evidence: EvidenceClass::StaticIndicator, boundedness: None });
            }
            if config.is_enabled("expensive-loop-check") {
                for (offset, body_action) in body.iter().enumerate() {
                    let mut expensive = Vec::new();
                    visit_action_roots(body_action, &mut |_, value| {
                        collect_expensive_values(value, &mut expensive)
                    });
                    for value in expensive {
                        let Value::Call { name, .. } = value else {
                            unreachable!("only expensive calls are collected")
                        };
                        findings.push(Finding {
                            code: "expensive-loop-check".into(),
                            severity: Severity::Info,
                            message: "geometry predicate evaluated inside a loop body may be expensive per iteration"
                                .into(),
                            span: value_occurrence(
                                program,
                                program.action_span(rule_id, start + offset),
                                name,
                            ),
                            rule: rule_id,
                            action: Some(action_id),
                            value: value_ids.get(&(value as *const Value as usize)).copied(),
                            evidence: EvidenceClass::Heuristic,
                            boundedness: None,
                        });
                    }
                }
            }
            if let Action::While { condition } = action {
                if config.is_enabled("while-without-wait")
                    && !body.iter().any(|action| is_wait(action, false))
                {
                    let boundedness = while_boundedness(condition, body, &program.subroutines);
                    let severity = match boundedness {
                        Boundedness::StaticallyBounded => Severity::Info,
                        Boundedness::ObviouslyUnbounded | Boundedness::Unknown => Severity::Warning,
                    };
                    findings.push(Finding {
                        code: "while-without-wait".into(),
                        severity,
                        message: while_without_wait_message(boundedness),
                        span: program.action_span(rule_id, action_id),
                        rule: rule_id,
                        action: Some(action_id),
                        value: None,
                        evidence: EvidenceClass::StaticIndicator,
                        boundedness: Some(boundedness),
                    });
                }
            }
            if config.is_enabled("repeated-value")
                && matches!(
                    action,
                    Action::While { .. } | Action::ForGlobalVariable { .. }
                )
            {
                findings.extend(repeated_value_findings(
                    program, rule_id, rule, action_id, start, body, &value_ids,
                ));
            }
        }
        if config.is_enabled("duplicate-condition") {
            findings.extend(duplicate_condition_findings(
                program, rule_id, rule, &value_ids,
            ));
        }
    }
    for finding in &mut findings {
        if let Some(severity) = config.severity_override(&finding.code) {
            finding.severity = severity;
        }
    }
    findings
}

fn loop_body(rule: &Rule, action: usize) -> Option<(usize, usize)> {
    if !matches!(
        rule.actions.get(action),
        Some(Action::While { .. } | Action::ForGlobalVariable { .. })
    ) {
        return None;
    }
    let end = matching_end(&rule.actions, action)?;
    Some((action + 1, end))
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
fn ongoing_condition_findings(
    program: &Program,
    rule_id: RuleId,
    rule: &Rule,
    value_ids: &HashMap<usize, ValueId>,
) -> Vec<Finding> {
    if !matches!(
        &rule.event,
        Event::Global | Event::EachPlayer | Event::EachPlayerWithFilters { .. }
    ) {
        return Vec::new();
    }

    let active_conditions: Vec<_> = rule
        .conditions
        .iter()
        .enumerate()
        .filter(|(_, condition)| !condition.disabled)
        .collect();
    let condition_count = active_conditions.len();
    let mut findings = Vec::new();
    for (index, &(source_index, condition)) in active_conditions.iter().enumerate() {
        let mut expensive = Vec::new();
        collect_expensive_values(&condition.value, &mut expensive);
        for value in expensive {
            let preceding = index;
            let later = condition_count - index - 1;
            let evaluation = match preceding {
                0 => "is evaluated every server tick".to_string(),
                1 => "is evaluated only after 1 preceding condition passes".to_string(),
                count => format!("is evaluated only after {count} preceding conditions pass"),
            };
            let later_gates = if later == 0 {
                String::new()
            } else {
                format!(
                    ", before {later} later short-circuit gate{}",
                    if later == 1 { "" } else { "s" }
                )
            };
            let name = match value {
                Value::Call { name, .. } => name,
                _ => unreachable!("only expensive call values are collected"),
            };
            let span = program.condition_span(rule_id, source_index);
            findings.push(Finding {
                code: "ongoing-condition-hot-path".into(),
                severity: Severity::Info,
                message: format!(
                    "geometry predicate in an ongoing-rule condition {} of {condition_count} {evaluation}{later_gates}; its cost is heuristic, not measured runtime load",
                    index + 1,
                ),
                span: value_occurrence(program, span, name).or(span),
                rule: rule_id,
                action: None,
                value: value_ids.get(&(value as *const Value as usize)).copied(),
                evidence: EvidenceClass::Heuristic,
                boundedness: None,
            });
        }
    }
    findings
}

fn collect_expensive_values<'a>(value: &'a Value, out: &mut Vec<&'a Value>) {
    visit_value_tree(value, None, &mut |value, _| {
        if let Value::Call { name, .. } = value
            && ["distance", "raycast", "isInLoS"].contains(&name.as_str())
        {
            out.push(value);
        }
        0
    });
}

fn duplicate_condition_findings(
    program: &Program,
    rule_id: RuleId,
    rule: &Rule,
    value_ids: &HashMap<usize, ValueId>,
) -> Vec<Finding> {
    let mut seen: Vec<&Value> = Vec::new();
    let mut findings = Vec::new();
    for (action_id, action) in rule.actions.iter().enumerate() {
        let condition = match action {
            Action::If { condition }
            | Action::ElseIf { condition }
            | Action::While { condition } => condition,
            _ => continue,
        };
        if seen
            .iter()
            .any(|previous| values_equal(previous, condition))
        {
            let span = program.action_argument_span(rule_id, action_id, 0);
            findings.push(Finding {
                code: "duplicate-condition".into(),
                severity: Severity::Warning,
                message: "condition is evaluated more than once in this rule; a later branch can never be taken".into(),
                span: span.or_else(|| program.action_span(rule_id, action_id)),
                rule: rule_id,
                action: Some(action_id),
                value: value_ids.get(&(condition as *const Value as usize)).copied(),
                evidence: EvidenceClass::Exact,
                boundedness: None,
            });
        } else {
            seen.push(condition);
        }
    }
    findings
}

fn repeated_value_findings(
    program: &Program,
    rule_id: RuleId,
    rule: &Rule,
    loop_action: ActionId,
    body_start: usize,
    body: &[Action],
    value_ids: &HashMap<usize, ValueId>,
) -> Vec<Finding> {
    let mut values = Vec::new();
    let mut parents = Vec::new();
    let mut spans = Vec::new();
    if let Action::While { condition } = &rule.actions[loop_action] {
        collect_value_tree(
            condition,
            None,
            program.action_argument_span(rule_id, loop_action, 0),
            &mut values,
            &mut parents,
            &mut spans,
        );
    }
    let mut action = 0;
    while action < body.len() {
        if matches!(
            body[action],
            Action::While { .. }
                | Action::ForGlobalVariable { .. }
                | Action::ForPlayerVariable { .. }
        ) {
            action = matching_end(body, action).map_or(action + 1, |end| end + 1);
            continue;
        }
        let action_id = body_start + action;
        if !matches!(body[action], Action::Disabled { .. }) {
            visit_action_roots(&body[action], &mut |argument, value| {
                collect_value_tree(
                    value,
                    None,
                    program.action_argument_span(rule_id, action_id, argument),
                    &mut values,
                    &mut parents,
                    &mut spans,
                );
            });
        }
        action += 1;
    }

    duplicated_value_families(&values, &parents)
        .into_iter()
        .map(|family| {
            let first = family[0];
            Finding {
                code: "repeated-value".into(),
                severity: Severity::Warning,
                message: format!(
                    "this value expression is evaluated {} times within the same loop scope",
                    family.len()
                ),
                span: spans[first].or_else(|| program.action_span(rule_id, loop_action)),
                rule: rule_id,
                action: Some(loop_action),
                value: value_ids
                    .get(&(values[first] as *const Value as usize))
                    .copied(),
                evidence: EvidenceClass::Exact,
                boundedness: None,
            }
        })
        .collect()
}

fn collect_value_tree<'a>(
    value: &'a Value,
    parent: Option<usize>,
    span: Option<Span>,
    values: &mut Vec<&'a Value>,
    parents: &mut Vec<Option<usize>>,
    spans: &mut Vec<Option<Span>>,
) {
    visit_value_tree(value, parent, &mut |value, parent| {
        let index = values.len();
        values.push(value);
        parents.push(parent);
        spans.push(span);
        index
    });
}

fn duplicated_value_families(values: &[&Value], parents: &[Option<usize>]) -> Vec<Vec<usize>> {
    let mut families: Vec<(usize, Vec<usize>)> = Vec::new();
    for (position, value) in values.iter().enumerate() {
        if let Some((_, family)) = families
            .iter_mut()
            .find(|(_, family)| values_equal(values[family[0]], value))
        {
            family.push(position);
        } else {
            families.push((position, vec![position]));
        }
    }

    let candidates: Vec<usize> = families
        .iter()
        .enumerate()
        .filter(|(_, (_, family))| family.len() >= 2 && value_call_count(values[family[0]]) >= 2)
        .map(|(index, _)| index)
        .collect();
    let mut member_family = vec![None; values.len()];
    for &family in &candidates {
        for &member in &families[family].1 {
            member_family[member] = Some(family);
        }
    }
    let mut reported: Vec<usize> = candidates
        .into_iter()
        .filter(|&family| {
            !families[family].1.iter().any(|&member| {
                let mut ancestor = parents[member];
                while let Some(index) = ancestor {
                    if member_family[index].is_some_and(|other| other != family) {
                        return true;
                    }
                    ancestor = parents[index];
                }
                false
            })
        })
        .collect();
    reported.sort_by_key(|&family| families[family].0);
    reported
        .into_iter()
        .map(|family| std::mem::take(&mut families[family].1))
        .collect()
}

fn value_call_count(value: &Value) -> usize {
    let mut count = 0;
    visit_value_tree(value, None, &mut |value, _| {
        count += usize::from(matches!(value, Value::Call { .. }));
        0
    });
    count
}

fn while_without_wait_message(boundedness: Boundedness) -> String {
    match boundedness {
        Boundedness::ObviouslyUnbounded => "loop body contains no wait call and the loop condition is statically true, so the loop repeats without yielding and never terminates on its own; it runs without bound while the rule is active (exact server impact is not statically measurable)".to_string(),
        Boundedness::StaticallyBounded => "loop body contains no wait call; the loop is statically bounded by a counter against a literal bound, so it runs a finite number of back-to-back iterations".to_string(),
        Boundedness::Unknown => "loop body contains no wait call and the loop's boundedness is unknown (data-dependent condition with no static counter pattern), so the loop may repeat without yielding".to_string(),
    }
}

fn while_boundedness(
    condition: &Value,
    body: &[Action],
    subroutines: &[workshop_rs::Subroutine],
) -> Boundedness {
    if matches!(condition, Value::Bool(true)) {
        return Boundedness::ObviouslyUnbounded;
    }
    let Some((variable, toward)) = counter_comparison(condition) else {
        return Boundedness::Unknown;
    };
    let mut progresses = false;
    for (start, end) in direct_action_ranges(body) {
        if action_has_unprovable_loop(body, start, end, subroutines) {
            return Boundedness::Unknown;
        }
        match modify_direction(&body[start], &variable) {
            Some(direction) if direction == toward => progresses = true,
            Some(_) => return Boundedness::Unknown,
            None if region_writes(&body[start..end], &variable, subroutines) => {
                return Boundedness::Unknown;
            }
            None => {}
        }
    }
    if progresses {
        Boundedness::StaticallyBounded
    } else {
        Boundedness::Unknown
    }
}

#[derive(Debug, Clone)]
enum CounterVariable {
    Global(String),
    Player { player: Value, variable: String },
}

fn counter_comparison(condition: &Value) -> Option<(CounterVariable, i32)> {
    let Value::Call { name, args } = condition else {
        return None;
    };
    if args.len() != 2 {
        return None;
    }
    let left_variable = variable_of(&args[0]);
    let right_variable = variable_of(&args[1]);
    let left_literal = matches!(&args[0], Value::Number(_));
    let right_literal = matches!(&args[1], Value::Number(_));
    let (variable, variable_is_left) = match (left_variable, right_literal) {
        (Some(variable), true) => (variable, true),
        (None, false) if left_literal => (right_variable?, false),
        _ => return None,
    };
    let direction = match (name.as_str(), variable_is_left) {
        ("<", true) | ("<=", true) | (">", false) | (">=", false) => 1,
        (">", true) | (">=", true) | ("<", false) | ("<=", false) => -1,
        _ => return None,
    };
    Some((variable, direction))
}

fn variable_of(value: &Value) -> Option<CounterVariable> {
    match value {
        Value::GlobalVariable(name) => Some(CounterVariable::Global(name.clone())),
        Value::PlayerVariable { player, variable } => Some(CounterVariable::Player {
            player: player.as_ref().clone(),
            variable: variable.clone(),
        }),
        _ => None,
    }
}

fn modify_direction(action: &Action, variable: &CounterVariable) -> Option<i32> {
    let (op, value) = match action {
        Action::ModifyGlobalVariable {
            variable: target,
            op,
            value,
        } if matches!(variable, CounterVariable::Global(name) if name == target) => (op, value),
        Action::ModifyPlayerVariable {
            player,
            variable: target,
            op,
            value,
        } => {
            let CounterVariable::Player {
                player: expected_player,
                variable: expected_variable,
            } = variable
            else {
                return None;
            };
            if target != expected_variable || !values_equal(player, expected_player) {
                return None;
            }
            (op, value)
        }
        _ => return None,
    };
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

fn region_writes(
    actions: &[Action],
    variable: &CounterVariable,
    subroutines: &[workshop_rs::Subroutine],
) -> bool {
    actions
        .iter()
        .any(|action| action_writes(action, variable, subroutines))
}

fn action_writes(
    action: &Action,
    variable: &CounterVariable,
    subroutines: &[workshop_rs::Subroutine],
) -> bool {
    match action {
        Action::SetGlobalVariable {
            variable: target, ..
        }
        | Action::ModifyGlobalVariable {
            variable: target, ..
        } => {
            matches!(variable, CounterVariable::Global(name) if name == target)
        }
        Action::SetPlayerVariable {
            player,
            variable: target,
            ..
        }
        | Action::ModifyPlayerVariable {
            player,
            variable: target,
            ..
        } => {
            matches!(variable, CounterVariable::Player { player: expected_player, variable: expected_variable }
            if target == expected_variable && values_equal(player, expected_player))
        }
        Action::CallSubroutine { .. } | Action::AssignMember { .. } => true,
        Action::Call { name, .. } => subroutines
            .iter()
            .any(|subroutine| subroutine.name == *name),
        Action::Disabled { action } => action_writes(action, variable, subroutines),
        _ => false,
    }
}

fn action_has_unprovable_loop(
    actions: &[Action],
    start: usize,
    end: usize,
    subroutines: &[workshop_rs::Subroutine],
) -> bool {
    let inner = if end > start + 1 {
        &actions[start + 1..end - 1]
    } else {
        &[]
    };
    match &actions[start] {
        Action::While { condition } => {
            while_boundedness(condition, inner, subroutines) != Boundedness::StaticallyBounded
                || region_has_unprovable_loop(inner, subroutines)
        }
        Action::ForGlobalVariable { variable, step, .. } => {
            let finite_step = step_direction(step).is_some();
            !finite_step
                || region_writes(
                    inner,
                    &CounterVariable::Global(variable.clone()),
                    subroutines,
                )
                || region_has_unprovable_loop(inner, subroutines)
        }
        Action::ForPlayerVariable { .. } => true,
        Action::If { .. } => region_has_unprovable_loop(inner, subroutines),
        _ => false,
    }
}

fn region_has_unprovable_loop(actions: &[Action], subroutines: &[workshop_rs::Subroutine]) -> bool {
    direct_action_ranges(actions)
        .into_iter()
        .any(|(start, end)| action_has_unprovable_loop(actions, start, end, subroutines))
}

fn direct_action_ranges(actions: &[Action]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < actions.len() {
        let end = matching_end(actions, start).map_or(start + 1, |end| end + 1);
        ranges.push((start, end));
        start = end;
    }
    ranges
}

fn step_direction(step: &Value) -> Option<i32> {
    let Value::Number(step) = step else {
        return None;
    };
    if *step == 0.0 {
        None
    } else {
        Some(if *step > 0.0 { 1 } else { -1 })
    }
}
