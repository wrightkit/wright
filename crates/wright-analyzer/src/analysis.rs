//! Workshop-specific static analyses over Workshop IR and CFG.
//!
//! Each analysis produces [`Finding`]s with a stable code, a severity, a
//! human-readable message, and the offending rule/action/value and span. The
//! v0.2 analysis set is deliberately small and low-false-positive:
//!
//! * [`MinWaitLoop`] (`min-wait-loop`) — a loop whose body waits at the
//!   workshop minimum rate (~0.016s), i.e. it executes at maximum frequency.
//! * [`DuplicateCondition`] (`duplicate-condition`) — the same condition
//!   evaluated twice within one rule (a later branch can never be taken).
//! * [`ExpensiveLoopCheck`] (`expensive-loop-check`) — a geometry predicate
//!   (`distance`, `raycast`, `isInLoS`) evaluated inside a loop body.
//!
//! Known limits (documented, not silent): wait durations that are not
//! statically known are treated as not-minimum; duplicate detection is
//! structural (arena-id-independent) and rule-local; the expensive-call list
//! is a heuristic that may miss or over-flag exotic predicates.

use wright_ir::source::Span;
use wright_ir::wir::{self, Action, ActionId, RuleId, Value, ValueId};

use crate::cfg::Cfg;

/// The severity of a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Info,
}

/// One analysis finding, linked to its source location and IR node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Stable machine-readable code, e.g. `min-wait-loop`.
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
    pub span: Option<Span>,
    pub rule: RuleId,
    pub action: Option<ActionId>,
    pub value: Option<ValueId>,
}

/// A Workshop-specific static analysis.
pub trait Analysis {
    /// The stable analysis name (also the finding code).
    fn name(&self) -> &'static str;
    /// Run the analysis over one rule and its CFG.
    fn run(&self, program: &wir::Program, rule: RuleId, cfg: &Cfg) -> Vec<Finding>;
}

/// Run every shipped analysis over every rule and return all findings.
pub fn analyze(program: &wir::Program) -> Vec<Finding> {
    let analyses: [&dyn Analysis; 3] = [&MinWaitLoop, &DuplicateCondition, &ExpensiveLoopCheck];
    let mut findings = Vec::new();
    for (index, _) in program.rules.iter().enumerate() {
        let rule = RuleId::from_index(index);
        let Ok(cfg) = Cfg::build(program, rule) else {
            continue; // an invalid rule cannot be analyzed
        };
        for analysis in &analyses {
            findings.extend(analysis.run(program, rule, &cfg));
        }
    }
    findings
}

/// A loop whose body waits at the workshop minimum rate.
pub struct MinWaitLoop;

/// The workshop minimum wait duration (seconds), ~60 iterations/second.
pub const MIN_WAIT_SECONDS: f64 = 0.016;

impl Analysis for MinWaitLoop {
    fn name(&self) -> &'static str {
        "min-wait-loop"
    }

    fn run(&self, program: &wir::Program, rule: RuleId, _cfg: &Cfg) -> Vec<Finding> {
        let Some(rule_data) = program.rules.get(rule).cloned() else {
            return Vec::new();
        };
        let mut findings = Vec::new();
        visit_actions(program, &rule_data.actions, &mut |action_id, action| {
            let (body, span) = match action {
                Action::While { body, span, .. } => (body, span),
                Action::ForGlobalVariable { body, span, .. } => (body, span),
                _ => return,
            };
            if body_has_min_wait(program, body) {
                findings.push(Finding {
                    code: self.name(),
                    severity: Severity::Warning,
                    message: "loop body waits at the workshop minimum rate; the loop runs at maximum frequency"
                        .to_string(),
                    span: *span,
                    rule,
                    action: Some(action_id),
                    value: None,
                });
            }
        });
        findings
    }
}

/// Whether any action in the tree contains a `wait` at the minimum duration.
fn body_has_min_wait(program: &wir::Program, actions: &[ActionId]) -> bool {
    let mut found = false;
    visit_actions(program, actions, &mut |_, action| {
        if !found {
            if let Action::Call { name, args, .. } = action {
                if name == "wait" {
                    if let Some(duration) = wait_duration(program, args) {
                        if duration <= MIN_WAIT_SECONDS {
                            found = true;
                        }
                    }
                }
            }
        }
    });
    found
}

/// The static duration of a `wait` call, when its first argument is a
/// numeric literal.
fn wait_duration(program: &wir::Program, args: &[ValueId]) -> Option<f64> {
    let first = program.values.get(*args.first()?)?;
    match &first.value {
        Value::Number(duration) => Some(*duration),
        _ => None,
    }
}

/// The same condition evaluated more than once within one rule.
pub struct DuplicateCondition;

impl Analysis for DuplicateCondition {
    fn name(&self) -> &'static str {
        "duplicate-condition"
    }

    fn run(&self, program: &wir::Program, rule: RuleId, _cfg: &Cfg) -> Vec<Finding> {
        let Some(rule_data) = program.rules.get(rule).cloned() else {
            return Vec::new();
        };
        let mut conditions: Vec<(ValueId, Option<ActionId>, Option<Span>)> = Vec::new();
        let mut findings = Vec::new();
        visit_actions(program, &rule_data.actions, &mut |action_id, action| {
            let rule_conditions: Vec<ValueId> = match action {
                Action::While { condition, .. } => vec![*condition],
                Action::If { branches, .. } => {
                    branches.iter().map(|branch| branch.condition).collect()
                }
                _ => return,
            };
            for condition in rule_conditions {
                let span = program.values.get(condition).and_then(|node| node.span);
                let duplicate = conditions
                    .iter()
                    .any(|(earlier, _, _)| structurally_equal(program, *earlier, condition));
                if duplicate {
                    findings.push(Finding {
                        code: self.name(),
                        severity: Severity::Warning,
                        message: "condition is evaluated more than once in this rule; a later branch can never be taken"
                            .to_string(),
                        span,
                        rule,
                        action: Some(action_id),
                        value: Some(condition),
                    });
                } else {
                    conditions.push((condition, Some(action_id), span));
                }
            }
        });
        findings
    }
}

/// A geometry predicate evaluated inside a loop body.
pub struct ExpensiveLoopCheck;

/// Predicates treated as potentially expensive per evaluation.
pub const EXPENSIVE_PREDICATES: &[&str] = &["distance", "raycast", "isInLoS"];

impl Analysis for ExpensiveLoopCheck {
    fn name(&self) -> &'static str {
        "expensive-loop-check"
    }

    fn run(&self, program: &wir::Program, rule: RuleId, _cfg: &Cfg) -> Vec<Finding> {
        let Some(rule_data) = program.rules.get(rule).cloned() else {
            return Vec::new();
        };
        let mut findings = Vec::new();
        visit_actions(program, &rule_data.actions, &mut |action_id, action| {
            let body = match action {
                Action::While { body, .. } => body,
                Action::ForGlobalVariable { body, .. } => body,
                _ => return,
            };
            for value in expensive_values_in_actions(program, body) {
                findings.push(Finding {
                    code: self.name(),
                    severity: Severity::Info,
                    message: "geometry predicate evaluated inside a loop body may be expensive per iteration"
                        .to_string(),
                    span: program.values.get(value).and_then(|node| node.span),
                    rule,
                    action: Some(action_id),
                    value: Some(value),
                });
            }
        });
        findings
    }
}

/// Every expensive predicate value inside a tree of actions.
fn expensive_values_in_actions(program: &wir::Program, actions: &[ActionId]) -> Vec<ValueId> {
    let mut found = Vec::new();
    visit_actions(program, actions, &mut |_, action| {
        visit_values_in_action(program, action, &mut |value_id| {
            let node = program.values.get(value_id).expect("in range");
            if let Value::Call { name, .. } = &node.value {
                if EXPENSIVE_PREDICATES.contains(&name.as_str()) {
                    found.push(value_id);
                }
            }
        });
    });
    found
}

/// Visit every action in a tree (including nested bodies), in program order.
fn visit_actions(
    program: &wir::Program,
    actions: &[ActionId],
    f: &mut impl FnMut(ActionId, &Action),
) {
    for action in actions {
        let Some(data) = program.actions.get(*action) else {
            continue;
        };
        f(*action, data);
        match data {
            Action::If {
                branches,
                else_body,
                ..
            } => {
                for branch in branches {
                    visit_actions(program, &branch.body, f);
                }
                if let Some(else_body) = else_body {
                    visit_actions(program, else_body, f);
                }
            }
            Action::While { body, .. } | Action::ForGlobalVariable { body, .. } => {
                visit_actions(program, body, f);
            }
            Action::SetGlobalVariable { .. }
            | Action::ModifyGlobalVariable { .. }
            | Action::SetPlayerVariable { .. }
            | Action::ModifyPlayerVariable { .. }
            | Action::CallSubroutine { .. }
            | Action::Debug { .. }
            | Action::Print { .. }
            | Action::Call { .. } => {}
        }
    }
}

/// Visit every value reachable from an action's arguments and conditions.
fn visit_values_in_action(program: &wir::Program, action: &Action, f: &mut impl FnMut(ValueId)) {
    match action {
        Action::SetGlobalVariable { value, .. }
        | Action::ModifyGlobalVariable { value, .. }
        | Action::Debug { value, .. }
        | Action::Print { message: value, .. } => visit_value(program, *value, f),
        Action::SetPlayerVariable { player, value, .. }
        | Action::ModifyPlayerVariable { player, value, .. } => {
            visit_value(program, *player, f);
            visit_value(program, *value, f);
        }
        Action::CallSubroutine { .. } => {}
        Action::If { branches, .. } => {
            for branch in branches {
                visit_value(program, branch.condition, f);
            }
        }
        Action::While { condition, .. } => visit_value(program, *condition, f),
        Action::ForGlobalVariable {
            start, stop, step, ..
        } => {
            visit_value(program, *start, f);
            visit_value(program, *stop, f);
            visit_value(program, *step, f);
        }
        Action::Call { args, .. } => {
            for arg in args {
                visit_value(program, *arg, f);
            }
        }
    }
}

/// Visit a value and all its children.
fn visit_value(program: &wir::Program, id: ValueId, f: &mut impl FnMut(ValueId)) {
    f(id);
    let Some(node) = program.values.get(id) else {
        return;
    };
    match &node.value {
        Value::Array(elements) => {
            for element in elements {
                visit_value(program, *element, f);
            }
        }
        Value::Vector { x, y, z } => {
            visit_value(program, *x, f);
            visit_value(program, *y, f);
            visit_value(program, *z, f);
        }
        Value::PlayerVariable { player, .. } => visit_value(program, *player, f),
        Value::Call { args, .. } => {
            for arg in args {
                visit_value(program, *arg, f);
            }
        }
        Value::Number(_)
        | Value::String(_)
        | Value::Bool(_)
        | Value::Null
        | Value::Enum { .. }
        | Value::GlobalVariable(_)
        | Value::EventPlayer => {}
    }
}

/// Structural equality of two values, ignoring arena ids (two separately
/// lowered but identically shaped values are equal).
fn structurally_equal(program: &wir::Program, a: ValueId, b: ValueId) -> bool {
    let (Some(na), Some(nb)) = (program.values.get(a), program.values.get(b)) else {
        return false;
    };
    match (&na.value, &nb.value) {
        (Value::Number(x), Value::Number(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Null, Value::Null) => true,
        (Value::Array(xs), Value::Array(ys)) => {
            xs.len() == ys.len()
                && xs
                    .iter()
                    .zip(ys.iter())
                    .all(|(x, y)| structurally_equal(program, *x, *y))
        }
        (
            Value::Vector {
                x: x1,
                y: y1,
                z: z1,
            },
            Value::Vector {
                x: x2,
                y: y2,
                z: z2,
            },
        ) => {
            structurally_equal(program, *x1, *x2)
                && structurally_equal(program, *y1, *y2)
                && structurally_equal(program, *z1, *z2)
        }
        (
            Value::Enum {
                value_type: t1,
                value: v1,
            },
            Value::Enum {
                value_type: t2,
                value: v2,
            },
        ) => t1 == t2 && v1 == v2,
        (Value::GlobalVariable(x), Value::GlobalVariable(y)) => x == y,
        (
            Value::PlayerVariable {
                player: p1,
                variable: v1,
            },
            Value::PlayerVariable {
                player: p2,
                variable: v2,
            },
        ) => v1 == v2 && structurally_equal(program, *p1, *p2),
        (Value::EventPlayer, Value::EventPlayer) => true,
        (Value::Call { name: n1, args: a1 }, Value::Call { name: n2, args: a2 }) => {
            n1 == n2
                && a1.len() == a2.len()
                && a1
                    .iter()
                    .zip(a2.iter())
                    .all(|(x, y)| structurally_equal(program, *x, *y))
        }
        _ => false,
    }
}
