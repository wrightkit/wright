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
//! * [`RepeatedValue`] (`repeated-value`) — a value expression evaluated
//!   more than once within one loop scope, reported once per maximal
//!   duplicated shape.
//! * [`WhileWithoutWait`] (`while-without-wait`) — a `While` loop whose body
//!   tree contains no `wait` call, so it cannot yield while its condition
//!   holds; each finding also classifies the loop's boundedness evidence
//!   (`obviously-unbounded` / `statically-bounded` / `unknown`, #103).
//!
//! Known limits (documented, not silent): wait durations that are not
//! statically known are treated as not-minimum; duplicate detection is
//! structural (arena-id-independent) and rule-local; the expensive-call list
//! is a heuristic that may miss or over-flag exotic predicates; repeated-value
//! detection is structural (no value-flow) and loop-scope-local, reports each
//! duplicated shape once at its maximal form (nested duplicates subsumed) and
//! never flags single-call expressions such as bare array reads; the
//! while-without-wait trigger is static but the impact (loop frequency) is an
//! indicator, and `For Global Variable` loops are never flagged.
//!
//! Every [`Finding`] also carries the [`EvidenceClass`] of its rule (#98):
//! whether the finding is an exact structural fact, a static indicator,
//! a documented heuristic, or (reserved) runtime-validated.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use workshop_rs::source::Span;
use workshop_rs::wir::{
    self, Action, ActionId, GlobalVarId, ModifyOp, PlayerVarId, RuleId, Value, ValueId,
};

use crate::cfg::Cfg;
use crate::registry::{LintConfig, LintRegistry};

/// The severity of a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Warning,
    Info,
}

/// How strongly a finding is supported by the available evidence.
///
/// Classifies the *kind* of evidence behind a rule's findings, not the
/// severity or the certainty of an individual finding:
///
/// * `Exact` — a structural fact of the program (e.g. a duplicated
///   condition) that holds regardless of runtime values.
/// * `StaticIndicator` — the trigger is statically known but the impact
///   (e.g. runtime loop frequency) is an indicator, not a measurement.
/// * `Heuristic` — a documented fixed heuristic list that may miss or
///   over-flag edge cases.
/// * `RuntimeValidated` — reserved for rules whose findings are confirmed
///   by runtime evaluation; not produced by the v0.2 static rule set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceClass {
    Exact,
    StaticIndicator,
    Heuristic,
    RuntimeValidated,
}

impl EvidenceClass {
    /// The stable serialized spelling of this class
    /// (`"exact"`, `"static-indicator"`, `"heuristic"`,
    /// `"runtime-validated"`).
    pub fn as_str(self) -> &'static str {
        match self {
            EvidenceClass::Exact => "exact",
            EvidenceClass::StaticIndicator => "static-indicator",
            EvidenceClass::Heuristic => "heuristic",
            EvidenceClass::RuntimeValidated => "runtime-validated",
        }
    }
}

/// The boundedness evidence of a no-yield `While` loop (issue #103).
///
/// Classifies the loop's repetition evidence separately from the no-yield
/// fact: whether the loop is statically provable to terminate (bounded),
/// statically provable to never terminate on its own (obviously unbounded),
/// or not statically decidable from the modeled WIR (unknown).
///
/// * `ObviouslyUnbounded` — the condition is statically `true`; the modeled
///   WIR has no break/goto action, so a constant-true condition with no wait
///   never terminates.
/// * `StaticallyBounded` — the condition compares a variable against a
///   numeric literal and every direct child of the body either provably moves
///   the compared variable toward the literal by a non-zero literal step or
///   provably cannot write it.
/// * `Unknown` — any other condition shape (data-dependent, unrecognized
///   counter pattern, conditional progress).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Boundedness {
    ObviouslyUnbounded,
    StaticallyBounded,
    Unknown,
}

impl Boundedness {
    /// The stable serialized spelling of this class
    /// (`"obviously-unbounded"`, `"statically-bounded"`, `"unknown"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Boundedness::ObviouslyUnbounded => "obviously-unbounded",
            Boundedness::StaticallyBounded => "statically-bounded",
            Boundedness::Unknown => "unknown",
        }
    }
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
    /// The evidence class of this finding, taken from the producing rule.
    pub evidence: EvidenceClass,
    /// The boundedness evidence of a no-yield `While` loop finding
    /// (`while-without-wait` only; `None` on every other rule).
    pub boundedness: Option<Boundedness>,
}

/// A Workshop-specific static analysis.
pub trait Analysis {
    /// The stable analysis name (also the finding code).
    fn name(&self) -> &'static str;
    /// The evidence class of this rule's findings (single source of truth
    /// for the `evidence` field on every [`Finding`] and on the rule's
    /// metadata).
    fn evidence(&self) -> EvidenceClass;
    /// Run the analysis over one rule and its CFG.
    fn run(&self, program: &wir::Program, rule: RuleId, cfg: &Cfg) -> Vec<Finding>;
}

/// Run every shipped analysis over every rule and return all findings.
///
/// This is a convenience wrapper over [`LintRegistry::default`] with the
/// default [`LintConfig`] (all rules enabled, no severity overrides). Callers
/// that need selective enabling/disabling or severity control should call
/// [`LintRegistry::run`] directly with an explicit [`LintConfig`].
pub fn analyze(program: &wir::Program) -> Vec<Finding> {
    LintRegistry::default().run(program, &LintConfig::default())
}

/// A loop whose body waits at the workshop minimum rate.
pub struct MinWaitLoop;

/// The workshop minimum wait duration (seconds), ~60 iterations/second.
pub const MIN_WAIT_SECONDS: f64 = 0.016;

impl Analysis for MinWaitLoop {
    fn name(&self) -> &'static str {
        "min-wait-loop"
    }

    fn evidence(&self) -> EvidenceClass {
        // The minimum-duration wait is statically known, but the loop's
        // runtime frequency impact is an indicator, not a measurement.
        EvidenceClass::StaticIndicator
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
                    evidence: self.evidence(),
                    boundedness: None,
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
        Value::Number {
            value: duration, ..
        } => Some(*duration),
        _ => None,
    }
}

/// The same condition evaluated more than once within one rule.
pub struct DuplicateCondition;

impl Analysis for DuplicateCondition {
    fn name(&self) -> &'static str {
        "duplicate-condition"
    }

    fn evidence(&self) -> EvidenceClass {
        // A duplicated condition is a structural fact of the rule: it holds
        // for every execution, independent of runtime values.
        EvidenceClass::Exact
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
                        evidence: self.evidence(),
                        boundedness: None,
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

    fn evidence(&self) -> EvidenceClass {
        // The expensive-call list is a documented fixed heuristic that may
        // miss unusual predicates or over-flag cheap ones.
        EvidenceClass::Heuristic
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
                    evidence: self.evidence(),
                    boundedness: None,
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

/// The same value expression evaluated more than once within one loop scope.
///
/// Reports exactly one finding per maximal duplicated shape per loop scope:
/// a shape is duplicated when it occurs at least twice and every occurrence
/// contains at least two `Call` value nodes; nested duplicates are subsumed
/// by their maximal enclosing duplicated shape (REQ-001, amended).
pub struct RepeatedValue;

impl Analysis for RepeatedValue {
    fn name(&self) -> &'static str {
        "repeated-value"
    }

    fn evidence(&self) -> EvidenceClass {
        // A structurally identical value scheduled in the same loop scope is
        // re-evaluated every time its enclosing action executes, independent
        // of runtime values: a structural fact (the same basis as
        // duplicate-condition's `exact`).
        EvidenceClass::Exact
    }

    fn run(&self, program: &wir::Program, rule: RuleId, _cfg: &Cfg) -> Vec<Finding> {
        let Some(rule_data) = program.rules.get(rule).cloned() else {
            return Vec::new();
        };
        let mut findings = Vec::new();
        visit_actions(program, &rule_data.actions, &mut |action_id, action| {
            let (condition, body) = match action {
                Action::While {
                    condition, body, ..
                } => (Some(*condition), body),
                Action::ForGlobalVariable { body, .. } => (None, body),
                _ => return,
            };
            // The loop scope: the loop's own condition (`While`) plus the
            // value positions of every action in the body, in deterministic
            // program order. Nested loops are their own scopes and are
            // excluded; `For Global Variable` bounds are excluded.
            let mut scope: Vec<ValueId> = Vec::new();
            let mut parents: HashMap<ValueId, ValueId> = HashMap::new();
            if let Some(condition) = condition {
                visit_value_with_parent(program, condition, &mut parents, &mut scope);
            }
            collect_loop_scope_values(program, body, &mut parents, &mut scope);
            for family in duplicated_shapes(program, &scope, &parents) {
                let first = family[0];
                findings.push(Finding {
                    code: self.name(),
                    severity: Severity::Warning,
                    message: format!(
                        "this value expression is evaluated {} times within the same loop scope",
                        family.len()
                    ),
                    span: program.values.get(first).and_then(|node| node.span),
                    rule,
                    action: Some(action_id),
                    value: Some(first),
                    evidence: self.evidence(),
                    boundedness: None,
                });
            }
        });
        findings
    }
}

/// The maximal duplicated shapes of one loop scope: structural families with
/// at least two members whose subtrees each contain at least two `Call`
/// value nodes (the root counting), reported only when no member is a
/// descendant of a member of a different candidate family (maximal-shape
/// subsumption). Returned in first-occurrence order (families are grouped in
/// program order, and survivors are re-sorted by their first-occurrence
/// scope index).
fn duplicated_shapes(
    program: &wir::Program,
    scope: &[ValueId],
    parents: &HashMap<ValueId, ValueId>,
) -> Vec<Vec<ValueId>> {
    // Group scope values into structural families, keeping each family's
    // members in first-occurrence (program) order.
    let mut families: Vec<(usize, Vec<ValueId>)> = Vec::new();
    for (position, &value) in scope.iter().enumerate() {
        if let Some((_, family)) = families
            .iter_mut()
            .find(|(_, family)| structurally_equal(program, family[0], value))
        {
            family.push(value);
        } else {
            families.push((position, vec![value]));
        }
    }
    // Candidate families: at least two occurrences, and every member's
    // subtree contains at least two `Call` nodes. Structurally identical
    // members share one call count, so the first member determines it.
    let candidates: Vec<usize> = families
        .iter()
        .enumerate()
        .filter(|(_, (_, family))| family.len() >= 2 && call_count(program, family[0]) >= 2)
        .map(|(index, _)| index)
        .collect();
    // Map every candidate member to its family so ancestry can be tested.
    let mut member_to_family: HashMap<ValueId, usize> = HashMap::new();
    for &family_index in &candidates {
        for &member in &families[family_index].1 {
            member_to_family.insert(member, family_index);
        }
    }
    // Maximal-shape subsumption: a candidate family is reported only when no
    // member of it is a descendant of a member of a different candidate
    // family (nested duplicates are reported once, at the maximal shape).
    let mut reported: Vec<usize> = Vec::new();
    for &family_index in &candidates {
        let subsumed = families[family_index].1.iter().any(|&member| {
            ancestor_belongs_to_other_family(member, family_index, parents, &member_to_family)
        });
        if !subsumed {
            reported.push(family_index);
        }
    }
    // Deterministic order: source position of each shape's first occurrence
    // (recorded while grouping in program order).
    reported.sort_by_key(|&family_index| families[family_index].0);
    let mut survivors = Vec::with_capacity(reported.len());
    for family_index in reported {
        survivors.push(std::mem::take(&mut families[family_index].1));
    }
    survivors
}

/// Whether walking `member`'s ancestor chain reaches a member of a different
/// candidate family (i.e. `member` is nested inside a larger duplicated
/// shape).
fn ancestor_belongs_to_other_family(
    mut member: ValueId,
    own_family: usize,
    parents: &HashMap<ValueId, ValueId>,
    member_to_family: &HashMap<ValueId, usize>,
) -> bool {
    while let Some(&parent) = parents.get(&member) {
        if let Some(&family) = member_to_family.get(&parent) {
            if family != own_family {
                return true;
            }
        }
        member = parent;
    }
    false
}

/// The number of [`Value::Call`] nodes in the value subtree (the root
/// counting).
fn call_count(program: &wir::Program, id: ValueId) -> usize {
    let mut count = 0;
    visit_value(program, id, &mut |value_id| {
        if let Value::Call { .. } = &program.values.get(value_id).expect("in range").value {
            count += 1;
        }
    });
    count
}

/// Collect the value surface of a loop body for [`RepeatedValue`]: every
/// value reachable from each action's value positions, in program order,
/// recording each child's parent (for ancestry tests). `If` branches are
/// descended into; nested loops (`While`/`ForGlobalVariable`) are treated as
/// their own scopes and excluded from the enclosing loop's scope.
fn collect_loop_scope_values(
    program: &wir::Program,
    actions: &[ActionId],
    parents: &mut HashMap<ValueId, ValueId>,
    out: &mut Vec<ValueId>,
) {
    for action in actions {
        let Some(data) = program.actions.get(*action) else {
            continue;
        };
        match data {
            Action::While { .. }
            | Action::ForGlobalVariable { .. }
            | Action::ForPlayerVariable { .. } => {
                // Nested loops are analyzed as their own separate scopes.
            }
            Action::If {
                branches,
                else_body,
                ..
            } => {
                for branch in branches {
                    visit_value_with_parent(program, branch.condition, parents, out);
                    collect_loop_scope_values(program, &branch.body, parents, out);
                }
                if let Some(else_body) = else_body {
                    collect_loop_scope_values(program, else_body, parents, out);
                }
            }
            other => visit_action_value_roots(program, other, parents, out),
        }
    }
}

/// Visit the value positions of an action's arguments and conditions with
/// parent tracking. Only called for non-loop, non-`If` actions by the loop
/// scope walker; the `If`/loop arms mirror [`visit_values_in_action`] for
/// exhaustiveness.
fn visit_action_value_roots(
    program: &wir::Program,
    action: &Action,
    parents: &mut HashMap<ValueId, ValueId>,
    out: &mut Vec<ValueId>,
) {
    match action {
        Action::SetGlobalVariable { value, .. }
        | Action::ModifyGlobalVariable { value, .. }
        | Action::Debug { value, .. }
        | Action::Print { message: value, .. } => {
            visit_value_with_parent(program, *value, parents, out);
        }
        Action::SetPlayerVariable { player, value, .. }
        | Action::ModifyPlayerVariable { player, value, .. } => {
            visit_value_with_parent(program, *player, parents, out);
            visit_value_with_parent(program, *value, parents, out);
        }
        Action::CallSubroutine { .. } => {}
        Action::If { branches, .. } => {
            for branch in branches {
                visit_value_with_parent(program, branch.condition, parents, out);
            }
        }
        Action::While { .. }
        | Action::ForGlobalVariable { .. }
        | Action::ForPlayerVariable { .. } => {
            // Nested loops are excluded from the enclosing loop's scope.
        }
        Action::Call { args, .. } => {
            for arg in args {
                visit_value_with_parent(program, *arg, parents, out);
            }
        }
    }
}

/// Collect a value and every value in its subtree into `out` (pre-order),
/// recording each child's parent in `parents` for ancestry tests.
fn visit_value_with_parent(
    program: &wir::Program,
    id: ValueId,
    parents: &mut HashMap<ValueId, ValueId>,
    out: &mut Vec<ValueId>,
) {
    out.push(id);
    let Some(node) = program.values.get(id) else {
        return;
    };
    visit_value_children(&node.value, &mut |child| {
        parents.insert(child, id);
        visit_value_with_parent(program, child, parents, out);
    });
}

/// Visit every direct child value of a value.
fn visit_value_children(value: &Value, f: &mut impl FnMut(ValueId)) {
    match value {
        Value::Array(elements) => {
            for element in elements {
                f(*element);
            }
        }
        Value::Vector { x, y, z } => {
            f(*x);
            f(*y);
            f(*z);
        }
        Value::PlayerVariable { player, .. } => f(*player),
        Value::Call { args, .. } => {
            for arg in args {
                f(*arg);
            }
        }
        Value::Number { .. }
        | Value::String(_)
        | Value::Bool(_)
        | Value::Null
        | Value::Enum { .. }
        | Value::GlobalVariable(_)
        | Value::EventPlayer => {}
    }
}

/// A `While` loop whose body tree contains no `wait` call.
///
/// Each flagged loop additionally carries a [`Boundedness`] classification
/// (issue #103) that separates the static no-yield fact from the loop's
/// repetition evidence: a statically bounded no-yield loop is reported at
/// `info` severity and is explicitly NOT treated as equivalent to an
/// obviously unbounded one (which, like an unknown one, reports at
/// `warning`). The rule never claims a guaranteed crash or a measured
/// runtime cost.
pub struct WhileWithoutWait;

impl Analysis for WhileWithoutWait {
    fn name(&self) -> &'static str {
        "while-without-wait"
    }

    fn evidence(&self) -> EvidenceClass {
        // The absence of a `wait` call in the loop body is statically known,
        // but the impact (loop frequency) is an indicator, not a measurement.
        EvidenceClass::StaticIndicator
    }

    fn run(&self, program: &wir::Program, rule: RuleId, _cfg: &Cfg) -> Vec<Finding> {
        let Some(rule_data) = program.rules.get(rule).cloned() else {
            return Vec::new();
        };
        let mut findings = Vec::new();
        visit_actions(program, &rule_data.actions, &mut |action_id, action| {
            let Action::While {
                condition,
                body,
                span,
            } = action
            else {
                return;
            };
            if !body_has_wait(program, body) {
                let class = boundedness_of(program, *condition, body);
                findings.push(Finding {
                    code: self.name(),
                    severity: match class {
                        Boundedness::StaticallyBounded => Severity::Info,
                        Boundedness::ObviouslyUnbounded | Boundedness::Unknown => Severity::Warning,
                    },
                    message: no_wait_message(class),
                    span: *span,
                    rule,
                    action: Some(action_id),
                    value: None,
                    evidence: self.evidence(),
                    boundedness: Some(class),
                });
            }
        });
        findings
    }
}

/// The human-readable message for a no-yield finding, stating the static
/// fact AND the boundedness class explicitly without claiming a guaranteed
/// crash or a measured runtime cost.
fn no_wait_message(class: Boundedness) -> String {
    match class {
        Boundedness::ObviouslyUnbounded => {
            "loop body contains no wait call and the loop condition is statically true, so the loop repeats without yielding and never terminates on its own; it runs without bound while the rule is active (exact server impact is not statically measurable)".to_string()
        }
        Boundedness::StaticallyBounded => {
            "loop body contains no wait call; the loop is statically bounded by a counter against a literal bound, so it runs a finite number of back-to-back iterations".to_string()
        }
        Boundedness::Unknown => {
            "loop body contains no wait call and the loop's boundedness is unknown (data-dependent condition with no static counter pattern), so the loop may repeat without yielding".to_string()
        }
    }
}

/// Classify the boundedness evidence of a `While` loop with a no-yield body.
///
/// Deliberately conservative and structural (issue #103):
///
/// * `ObviouslyUnbounded` — the condition is `Value::Bool(true)`. The modeled
///   WIR has no break/goto action, so a constant-true condition with no wait
///   never terminates.
/// * `StaticallyBounded` — the condition is a literal-bound comparison
///   (`<`, `<=`, `>`, `>=`) of one variable against a numeric literal, and
///   EVERY direct child that can affect the compared variable either provably
///   moves it toward the literal by a non-zero literal step or provably cannot
///   write it (no away-direction modify, no `Set`/non-literal/zero-step
///   modify, no `CallSubroutine`, no `If`/nested-loop subtree writing it),
///   and no direct child is a nested loop whose termination is not statically
///   provable (a non-terminating nested loop prevents the outer loop from
///   completing an iteration).
/// * `Unknown` — anything else.
fn boundedness_of(program: &wir::Program, condition: ValueId, body: &[ActionId]) -> Boundedness {
    if matches!(
        program.values.get(condition).map(|node| &node.value),
        Some(Value::Bool(true))
    ) {
        return Boundedness::ObviouslyUnbounded;
    }
    let Some((variable, toward)) = counter_comparison(program, condition) else {
        return Boundedness::Unknown;
    };
    let mut progresses = false;
    for action_id in body {
        let Some(action) = program.actions.get(*action_id) else {
            continue;
        };
        // A direct-child nested loop whose termination is not statically
        // provable prevents the outer loop from completing an iteration, so
        // the outer loop is not provably finite.
        if action_has_unprovable_loop(program, *action_id) {
            return Boundedness::Unknown;
        }
        match modify_direction(program, action, &variable) {
            Some(direction) if direction == toward => progresses = true,
            // A direct child moves the compared variable away from the bound:
            // progress is not provable.
            Some(_) => return Boundedness::Unknown,
            // A direct child that provably cannot progress the counter still
            // writes the variable (Set, non-literal/zero-step Modify,
            // CallSubroutine, or an If/nested-loop subtree writing it):
            // progress is not provable.
            None if action_writes(program, action, &variable) => return Boundedness::Unknown,
            // Debug/Print and generic calls that are not user subroutines are
            // documented non-writers.
            None => {}
        }
    }
    if progresses {
        Boundedness::StaticallyBounded
    } else {
        Boundedness::Unknown
    }
}

/// Whether the subtree rooted at one action id contains a nested loop whose
/// termination is not statically provable. Recursion is well-founded: it
/// descends the action tree, which strictly decreases in depth (the action
/// arena is a tree).
fn action_has_unprovable_loop(program: &wir::Program, id: ActionId) -> bool {
    let Some(action) = program.actions.get(id) else {
        return false;
    };
    match action {
        // A nested `While` is provably finite only when its own boundedness
        // classification is `StaticallyBounded` (reused verbatim: a nested
        // `while true:` classifies `ObviouslyUnbounded`, so the outer loop
        // cannot be proven to complete an iteration).
        Action::While {
            condition, body, ..
        } => {
            boundedness_of(program, *condition, body) != Boundedness::StaticallyBounded
                || subtree_has_unprovable_loop(program, body)
        }
        // A nested `For Global Variable` is provably finite only with a
        // non-zero literal step whose subtree does not write its own loop
        // variable (the Workshop engine re-checks the control variable after
        // each `+= Step`; a zero or dynamic step may not terminate).
        Action::ForGlobalVariable {
            variable,
            step,
            body,
            ..
        } => {
            let finite = step_direction(program, &ModifyOp::Add, *step).is_some()
                && !subtree_writes(program, id, &Variable::Global(*variable));
            !finite || subtree_has_unprovable_loop(program, body)
        }
        Action::If {
            branches,
            else_body,
            ..
        } => {
            branches
                .iter()
                .any(|branch| subtree_has_unprovable_loop(program, &branch.body))
                || else_body
                    .as_ref()
                    .is_some_and(|body| subtree_has_unprovable_loop(program, body))
        }
        _ => false,
    }
}

/// Whether any action in a slice has an unprovable nested loop in its
/// subtree.
fn subtree_has_unprovable_loop(program: &wir::Program, actions: &[ActionId]) -> bool {
    actions
        .iter()
        .any(|id| action_has_unprovable_loop(program, *id))
}

/// Whether an action can write the compared variable.
///
/// Conservative and tree-walking: `Set`/`Modify` actions targeting the
/// variable (for player variables, the player expression must be structurally
/// identical to the one in the condition, so the write provably touches the
/// same slot), `CallSubroutine` (the callee body may write any variable),
/// `If`/`While`/`ForGlobalVariable` subtrees containing any of the above, and
/// a generic `Call` whose name matches a user-defined subroutine (some
/// frontends lower `def`-defined subroutine calls as generic calls rather
/// than `CallSubroutine`) all count as writers. `Debug`, `Print`, and generic
/// `Action::Call`s that are not user subroutines are documented NON-writers:
/// within the supported OPY/Workshop surface (docs/opy/support-matrix.md)
/// user-variable writes lower only to `Set`/`Modify` actions (`.append`
/// lowers to a `Modify` on the variable, so it is caught by the modify
/// branch), and a generic `Call` that is not a user subroutine is a built-in
/// workshop function.
fn action_writes(program: &wir::Program, action: &Action, variable: &Variable) -> bool {
    match action {
        Action::SetGlobalVariable {
            variable: target, ..
        } => {
            matches!(variable, Variable::Global(v) if *target == *v)
        }
        Action::SetPlayerVariable {
            player,
            variable: target,
            ..
        } => matches!(variable, Variable::Player(condition_player, v)
            if *target == *v && structurally_equal(program, *condition_player, *player)),
        Action::ModifyGlobalVariable {
            variable: target, ..
        } => {
            matches!(variable, Variable::Global(v) if *target == *v)
        }
        Action::ModifyPlayerVariable {
            player,
            variable: target,
            ..
        } => matches!(variable, Variable::Player(condition_player, v)
            if *target == *v && structurally_equal(program, *condition_player, *player)),
        Action::CallSubroutine { .. } => true,
        Action::Call { name, .. } => program
            .subroutines
            .iter()
            .any(|subroutine| subroutine.name == *name),
        Action::If {
            branches,
            else_body,
            ..
        } => {
            branches.iter().any(|branch| {
                branch
                    .body
                    .iter()
                    .any(|id| subtree_writes(program, *id, variable))
            }) || else_body
                .as_ref()
                .is_some_and(|body| body.iter().any(|id| subtree_writes(program, *id, variable)))
        }
        Action::While { body, .. }
        | Action::ForGlobalVariable { body, .. }
        | Action::ForPlayerVariable { body, .. } => {
            body.iter().any(|id| subtree_writes(program, *id, variable))
        }
        Action::Debug { .. } | Action::Print { .. } => false,
    }
}

/// Whether the subtree rooted at one action id can write the compared
/// variable (used for `If`/`While`/`ForGlobalVariable` sub-trees).
fn subtree_writes(program: &wir::Program, action_id: ActionId, variable: &Variable) -> bool {
    program
        .actions
        .get(action_id)
        .is_some_and(|action| action_writes(program, action, variable))
}

/// If `condition` is a literal-bound comparison of exactly one variable
/// against a numeric literal, return the compared variable and the direction
/// it must move to reach the literal (`+1` = increasing toward the bound,
/// `-1` = decreasing). `==`/`!=` and other conditions are not recognized.
fn counter_comparison(program: &wir::Program, condition: ValueId) -> Option<(Variable, i32)> {
    let Value::Call { name, args } = &program.values.get(condition)?.value else {
        return None;
    };
    if args.len() != 2 {
        return None;
    }
    let (left, right) = (program.values.get(args[0])?, program.values.get(args[1])?);
    let variable_left = variable_of(&left.value);
    let literal_left = matches!(&left.value, Value::Number { .. });
    let variable_right = variable_of(&right.value);
    let literal_right = matches!(&right.value, Value::Number { .. });
    // Exactly one argument is the variable; the other is the literal.
    let (variable, variable_is_left) = match (variable_left, literal_right) {
        (Some(variable), true) => (variable, true),
        (None, false) => match (literal_left, variable_right) {
            (true, Some(variable)) => (variable, false),
            _ => return None,
        },
        _ => return None,
    };
    let toward = match (name.as_str(), variable_is_left) {
        // V must INCREASE toward K: V < K, V <= K, K > V, K >= V.
        ("<", true) | ("<=", true) | (">", false) | (">=", false) => 1,
        // V must DECREASE toward K: V > K, V >= K, K < V, K <= V.
        (">", true) | (">=", true) | ("<", false) | ("<=", false) => -1,
        _ => return None,
    };
    Some((variable, toward))
}

/// A variable reference compared in a loop condition.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Variable {
    Global(GlobalVarId),
    Player(ValueId, PlayerVarId),
}

/// The variable read by a value, when it is a variable reference.
fn variable_of(value: &Value) -> Option<Variable> {
    match value {
        Value::GlobalVariable(variable) => Some(Variable::Global(*variable)),
        Value::PlayerVariable { player, variable } => Some(Variable::Player(*player, *variable)),
        _ => None,
    }
}

/// The effective iteration direction of a direct-child `Modify` action on
/// `variable` (`+1` = the variable increases each iteration, `-1` =
/// decreases), when the modify has a non-zero numeric literal step. For a
/// player variable the player expression must be structurally identical to
/// the one in the condition, so the modify provably touches the same slot.
fn modify_direction(program: &wir::Program, action: &Action, variable: &Variable) -> Option<i32> {
    match (action, variable) {
        (
            Action::ModifyGlobalVariable {
                variable: target,
                op,
                value,
                ..
            },
            Variable::Global(wanted),
        ) => {
            if target != wanted {
                return None;
            }
            step_direction(program, op, *value)
        }
        (
            Action::ModifyPlayerVariable {
                player,
                variable: target,
                op,
                value,
                ..
            },
            Variable::Player(condition_player, wanted),
        ) => {
            if target != wanted {
                return None;
            }
            if !structurally_equal(program, *condition_player, *player) {
                return None;
            }
            step_direction(program, op, *value)
        }
        _ => None,
    }
}

/// The direction of a `Modify` op with a non-zero literal numeric step:
/// `Add` steps by `sign(step)`, `Subtract` by `-sign(step)`.
fn step_direction(program: &wir::Program, op: &ModifyOp, value: ValueId) -> Option<i32> {
    let Value::Number { value: step, .. } = &program.values.get(value)?.value else {
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

/// Whether any action in the tree contains a `wait` call (presence only;
/// the wait duration does not matter).
fn body_has_wait(program: &wir::Program, actions: &[ActionId]) -> bool {
    let mut found = false;
    visit_actions(program, actions, &mut |_, action| {
        if !found {
            if let Action::Call { name, .. } = action {
                if name == "wait" {
                    found = true;
                }
            }
        }
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
            Action::While { body, .. }
            | Action::ForGlobalVariable { body, .. }
            | Action::ForPlayerVariable { body, .. } => {
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
        }
        | Action::ForPlayerVariable {
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
    visit_value_children(&node.value, &mut |child| visit_value(program, child, f));
}

/// Structural equality of two values, ignoring arena ids (two separately
/// lowered but identically shaped values are equal).
fn structurally_equal(program: &wir::Program, a: ValueId, b: ValueId) -> bool {
    let (Some(na), Some(nb)) = (program.values.get(a), program.values.get(b)) else {
        return false;
    };
    match (&na.value, &nb.value) {
        (Value::Number { value: x, .. }, Value::Number { value: y, .. }) => x == y,
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
