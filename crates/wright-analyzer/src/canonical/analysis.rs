use std::collections::HashMap;
use std::ops::Range;

use workshop_rs::catalog::{Catalog, Kind};
use workshop_rs::source::{FileId, Position, SourceDocument, Span};
use workshop_rs::{Action, Event, ModifyOp, Program, Rule, Value};

use super::cfg::{is_wait, matching_end};
use super::symbols::{ActionId, RuleId, ValueId, value_identity_map};
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
    /// The mechanically determined correction for this finding, when one
    /// exists and its safety preconditions hold (#556). The plan names the
    /// spans to rewrite; the driver materializes it into a validated
    /// source-edit transaction.
    pub fix: Option<LintFix>,
}

/// A mechanically determined correction for a lint finding (#556): the
/// semantic plan a consumer turns into a validated source-edit transaction.
/// Fixes exist only for `exact` findings whose correction is unambiguous;
/// every plan carries the exact spans it may touch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LintFix {
    /// Delete the dead `Else If` branch this span covers — from the `Else
    /// If` marker through the start of the next chain marker.
    RemoveDeadBranch { span: Span },
    /// Wrap each occurrence of the duplicated expression in `Evaluate Once`,
    /// freezing it for the enclosing action's evaluation.
    EvaluateOnce { occurrences: Vec<Span> },
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
                findings.push(Finding { code: "min-wait-loop".into(), severity: Severity::Warning, message: "loop body waits at the workshop minimum rate; the loop runs at maximum frequency".into(), span: program.action_span(rule_id, action_id), rule: rule_id, action: Some(action_id), value: None, evidence: EvidenceClass::StaticIndicator, boundedness: None, fix: None });
            }
            if config.is_enabled("expensive-loop-check") {
                for (offset, body_action) in body.iter().enumerate() {
                    let body_action_id = start + offset;
                    let mut expensive = Vec::new();
                    visit_action_roots(body_action, &mut |argument, value| {
                        for (path, value) in collect_expensive_values(value) {
                            expensive.push((argument, path, value));
                        }
                    });
                    for (argument, path, value) in expensive {
                        debug_assert!(
                            matches!(value, Value::Call { .. }),
                            "only expensive calls are collected"
                        );
                        findings.push(Finding {
                            code: "expensive-loop-check".into(),
                            severity: Severity::Info,
                            message: "geometry predicate evaluated inside a loop body may be expensive per iteration"
                                .into(),
                            span: program.action_argument_value_span(
                                rule_id,
                                body_action_id,
                                argument,
                                &path,
                            ),
                            rule: rule_id,
                            action: Some(action_id),
                            value: value_ids.get(&(value as *const Value as usize)).copied(),
                            evidence: EvidenceClass::Heuristic,
                            boundedness: None,
                            fix: None,
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
                        fix: None,
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
        let expensive = collect_expensive_values(&condition.value);
        for (path, value) in expensive {
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
            let span = program.condition_value_span(rule_id, source_index, &path);
            findings.push(Finding {
                code: "ongoing-condition-hot-path".into(),
                severity: Severity::Info,
                message: format!(
                    "geometry predicate in an ongoing-rule condition {} of {condition_count} {evaluation}{later_gates}; its cost is heuristic, not measured runtime load",
                    index + 1,
                ),
                span,
                rule: rule_id,
                action: None,
                value: value_ids.get(&(value as *const Value as usize)).copied(),
                evidence: EvidenceClass::Heuristic,
                boundedness: None,
                fix: None,
            });
        }
    }
    findings
}

fn collect_expensive_values(value: &Value) -> Vec<(Vec<usize>, &Value)> {
    let mut out = Vec::new();
    visit_value_tree(value, None, &mut |value, _, path| {
        if let Value::Call { name, .. } = value {
            if ["distance", "raycast", "isInLoS"].contains(&name.as_str()) {
                out.push((path.to_vec(), value));
            }
        }
        0
    });
    out
}

fn duplicate_condition_findings(
    program: &Program,
    rule_id: RuleId,
    rule: &Rule,
    value_ids: &HashMap<usize, ValueId>,
) -> Vec<Finding> {
    // A repeated condition is unreachable only as a later branch of the same
    // `If`/`Else If` chain, so conditions are compared within the chain that
    // owns them. Non-`If` blocks still push an entry so their `End` stays
    // aligned; a `disabled` action is unwrapped only to detect a block opener.
    enum Block<'a> {
        IfChain(Vec<&'a Value>),
        Other,
    }
    let mut blocks: Vec<Block> = Vec::new();
    let mut findings = Vec::new();
    for (action_id, action) in rule.actions.iter().enumerate() {
        match action {
            Action::ElseIf { condition } => {
                let Some(Block::IfChain(seen)) = blocks.last_mut() else {
                    continue;
                };
                if seen
                    .iter()
                    .any(|previous| values_equal(previous, condition))
                {
                    let span = program.action_argument_span(rule_id, action_id, 0);
                    findings.push(Finding {
                        code: "duplicate-condition".into(),
                        severity: Severity::Warning,
                        message: "an Else If condition repeats an earlier branch's condition in the same If/Else If chain; that branch can never be taken".into(),
                        span: span.or_else(|| program.action_span(rule_id, action_id)),
                        rule: rule_id,
                        action: Some(action_id),
                        value: value_ids.get(&(condition as *const Value as usize)).copied(),
                        evidence: EvidenceClass::Exact,
                        boundedness: None,
                        fix: dead_branch_fix(program, rule_id, rule, action_id, condition),
                    });
                } else {
                    seen.push(condition);
                }
            }
            Action::End => {
                blocks.pop();
            }
            _ => {
                let mut current = action;
                while let Action::Disabled { action } = current {
                    current = action;
                }
                match current {
                    Action::If { condition } => {
                        blocks.push(Block::IfChain(vec![condition]));
                    }
                    Action::While { .. }
                    | Action::ForGlobalVariable { .. }
                    | Action::ForPlayerVariable { .. } => blocks.push(Block::Other),
                    _ => {}
                }
            }
        }
    }
    findings
}

/// The mechanically determined fix for a `duplicate-condition` finding
/// (#556): delete the dead `Else If` branch — the marker line plus its body,
/// through the next `Else If`/`Else`/`End` chain marker. The fix is offered
/// only when the repeated condition cannot yield a different result when the
/// chain re-evaluates it: calls evaluated fresh per evaluation rather than
/// read from the tick snapshot (random numbers, advancing clocks, sampled
/// server load) keep the branch reachable, so it stays.
///
/// Provenance alone cannot bound the branch: `Else If`, `Else`, and `End`
/// markers carry no action span, so the marker extents are derived
/// textually against the retained source — see [`SourceScan`].
fn dead_branch_fix(
    program: &Program,
    rule_id: RuleId,
    rule: &Rule,
    action_id: ActionId,
    condition: &Value,
) -> Option<LintFix> {
    if contains_volatile_call(condition) {
        return None;
    }
    let boundary = elseif_branch_end(&rule.actions, action_id)?;
    let file = program
        .action_argument_span(rule_id, action_id, 0)
        .or_else(|| program.action_span(rule_id, action_id))?
        .file;
    let scan = SourceScan::new(program.source(file)?, file);
    let condition_end = expr_extent_end(
        condition,
        &mut |path| program.action_argument_value_span(rule_id, action_id, 0, path),
        &scan,
    )?;
    let start = chain_marker_start(&scan, condition_end, "If")?;
    let end = match &rule.actions[boundary] {
        Action::ElseIf { condition } => {
            let end = expr_extent_end(
                condition,
                &mut |path| program.action_argument_value_span(rule_id, boundary, 0, path),
                &scan,
            )?;
            chain_marker_start(&scan, end, "If")?
        }
        Action::Else => {
            let anchor = program.action_span(rule_id, boundary + 1)?;
            let anchor = scan.byte(anchor.start)?;
            keyword_marker_start(&scan, "Else", anchor)?
        }
        Action::End => {
            let chain = chain_if_index(&rule.actions, action_id)?;
            let anchor = program.action_span(rule_id, chain)?;
            let anchor = scan.byte(anchor.end)?;
            keyword_marker_start(&scan, "End", anchor)?
        }
        _ => return None,
    };
    dead_branch_span(&scan, start, end).map(|span| LintFix::RemoveDeadBranch { span })
}

/// The branch-removal span between a dead `Else If` marker and the next
/// chain marker, rounded to whole lines. `EditRange`s are half-open
/// line/column positions applied as a standard text splice, so whole-line
/// deletion covers the marker's line start through the boundary marker's
/// line start — the newline-terminated dead lines entirely; either side
/// keeps its exact extent when the marker shares its line with other
/// statements.
fn dead_branch_span(scan: &SourceScan, marker: usize, boundary: usize) -> Option<Span> {
    let text = scan.document.text();
    let marker_line = scan.line_start(marker);
    let start = if text[marker_line..marker].chars().all(char::is_whitespace) {
        marker_line
    } else {
        marker
    };
    let boundary_line = scan.line_start(boundary);
    let end = if boundary_line > start
        && text[boundary_line..boundary]
            .chars()
            .all(char::is_whitespace)
    {
        boundary_line
    } else {
        boundary
    };
    (start < end).then(|| scan.span(start, end))
}

/// The position of the chain marker (`Else If`, `Else`, or `End`) that ends
/// the `Else If` branch opened at `elseif` — nested blocks are skipped by
/// the same opener/`End` accounting the chain scan uses.
fn elseif_branch_end(actions: &[Action], elseif: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, action) in actions.iter().enumerate().skip(elseif + 1) {
        match action {
            Action::ElseIf { .. } | Action::Else if depth == 0 => return Some(index),
            Action::End => {
                if depth == 0 {
                    return Some(index);
                }
                depth -= 1;
            }
            _ => {
                let mut current = action;
                while let Action::Disabled { action } = current {
                    current = action;
                }
                if matches!(
                    current,
                    Action::If { .. }
                        | Action::While { .. }
                        | Action::ForGlobalVariable { .. }
                        | Action::ForPlayerVariable { .. }
                ) {
                    depth += 1;
                }
            }
        }
    }
    None
}

/// The `If` that opens the chain containing the `Else If` at `elseif` —
/// nested blocks are skipped by the same depth accounting, walked backward.
fn chain_if_index(actions: &[Action], elseif: usize) -> Option<usize> {
    let mut depth = 0usize;
    for index in (0..elseif).rev() {
        let mut action = &actions[index];
        while let Action::Disabled { action: inner } = action {
            action = inner;
        }
        match action {
            Action::End => depth += 1,
            Action::If { .. } if depth == 0 => return Some(index),
            Action::If { .. }
            | Action::While { .. }
            | Action::ForGlobalVariable { .. }
            | Action::ForPlayerVariable { .. } => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Whether `value` contains a call whose result can move between two
/// evaluations in one synchronous pass — the `Random *` family, the
/// advancing clocks, and the sampled server-load metrics, which are
/// evaluated fresh per call. Every other call reads the per-tick snapshot
/// the engine updates between passes — positions and objective state
/// included (`Update Every Frame` exists precisely because position reads
/// refresh only every few ticks) — so a duplicated condition stays equal.
fn contains_volatile_call(value: &Value) -> bool {
    const VOLATILE: &[&str] = &[
        "randomInteger",
        "randomReal",
        "randomValueInArray",
        "randomizedArray",
        "getTotalTimeElapsed",
        "getMatchTime",
        "getServerLoad",
        "getAverageServerLoad",
        "getPeakServerLoad",
    ];
    let mut volatile = false;
    visit_value_tree(value, None, &mut |value, _, _| {
        volatile |= matches!(value, Value::Call { name, .. } if VOLATILE.contains(&name.as_str()));
        0
    });
    volatile
}

/// A byte-oriented view over one retained source document for deriving
/// authored extents that provenance does not record (#556): value-call
/// spans cover only the callee phrase, and `Else If`/`Else`/`End` markers
/// carry no span at all. Positions inside `//` comments are trivia; every
/// derived extent is re-verified by the edit transaction's reparse before
/// any write, so a wrong derivation refuses rather than writes.
struct SourceScan<'a> {
    document: &'a SourceDocument,
    comments: Vec<Range<usize>>,
    line_starts: Vec<usize>,
    file: FileId,
}

impl<'a> SourceScan<'a> {
    fn new(document: &'a SourceDocument, file: FileId) -> Self {
        let mut comments: Vec<Range<usize>> =
            document.comments().map(|comment| comment.range()).collect();
        comments.sort_by_key(|comment| comment.start);
        let mut line_starts = vec![0];
        line_starts.extend(
            document
                .text()
                .match_indices('\n')
                .map(|(index, _)| index + 1),
        );
        SourceScan {
            document,
            comments,
            line_starts,
            file,
        }
    }

    /// The byte offset of a 1-based line/column position.
    fn byte(&self, position: Position) -> Option<usize> {
        self.document
            .byte_range(Span::new(self.file, position, position))
            .map(|range| range.start)
    }

    /// The first non-trivia character at or after `pos`.
    fn next(&self, mut pos: usize) -> Option<(usize, char)> {
        let text = self.document.text();
        loop {
            if let Some(comment) = self
                .comments
                .iter()
                .find(|comment| comment.start <= pos && pos < comment.end)
            {
                pos = comment.end;
                continue;
            }
            let c = text.get(pos..)?.chars().next()?;
            if c.is_whitespace() {
                pos += c.len_utf8();
                continue;
            }
            return Some((pos, c));
        }
    }

    /// The last non-trivia character before `pos`.
    fn prev(&self, mut pos: usize) -> Option<(usize, char)> {
        let text = self.document.text();
        loop {
            if pos == 0 {
                return None;
            }
            if let Some(comment) = self
                .comments
                .iter()
                .find(|comment| comment.start < pos && pos <= comment.end)
            {
                pos = comment.start;
                continue;
            }
            let c = text.get(..pos)?.chars().next_back()?;
            if c.is_whitespace() {
                pos -= c.len_utf8();
                continue;
            }
            return Some((pos - c.len_utf8(), c));
        }
    }

    /// The position a byte offset lands at, recomputed from the line index.
    fn position(&self, byte: usize) -> Position {
        let line = match self.line_starts.binary_search(&byte) {
            Ok(line) => line + 1,
            Err(after) => after,
        } as u32;
        let start = self.line_starts[(line - 1) as usize];
        let col = self.document.text()[start..byte].chars().count() as u32 + 1;
        Position::new(line, col)
    }

    /// The byte a byte offset's line begins at.
    fn line_start(&self, byte: usize) -> usize {
        match self.line_starts.binary_search(&byte) {
            Ok(line) => self.line_starts[line],
            Err(after) => self.line_starts[after - 1],
        }
    }

    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(self.file, self.position(start), self.position(end))
    }
}

/// The direct children of `value`, in [`visit_value_tree`] order.
fn value_children(value: &Value) -> Vec<&Value> {
    match value {
        Value::Array(values) | Value::Call { args: values, .. } => values.iter().collect(),
        Value::Vector { x, y, z } => vec![x, y, z],
        Value::PlayerVariable { player, .. } => vec![player],
        _ => Vec::new(),
    }
}

/// The full authored extent of `value` as a byte range: spans record the
/// callee phrase (`Compare`), not the `(...)` the expression occupies, so
/// the end is completed from the last child's extent through the call's
/// closing bracket. A derived/infix-shaped node — `a + b`, `a[i]`, `x ?
/// y : z` — begins before its own token and is anchored backward to the
/// `(` or `,` that starts its argument position. Returns `None` when the
/// extent cannot be derived exactly.
fn expr_extent_bytes(
    value: &Value,
    span_of: &mut dyn FnMut(&[usize]) -> Option<Span>,
    scan: &SourceScan,
) -> Option<Range<usize>> {
    let node = scan.document.byte_range(span_of(&[])?)?;
    let children = value_children(value);
    let mut kids = Vec::with_capacity(children.len());
    for (index, child) in children.iter().enumerate() {
        let extent = expr_extent_bytes(
            child,
            &mut |path| {
                let mut full = Vec::with_capacity(path.len() + 1);
                full.push(index);
                full.extend_from_slice(path);
                span_of(&full)
            },
            scan,
        )?;
        kids.push(extent);
    }
    let infix = kids.iter().any(|kid| kid.start < node.start);
    let start = if infix {
        expr_start_anchor(scan, kids.iter().map(|kid| kid.start).min()?)?
    } else {
        node.start
    };
    let contains_children = kids
        .iter()
        .all(|kid| kid.start >= node.start && kid.end <= node.end);
    let closer = match value {
        Value::Call { .. } | Value::Vector { .. } | Value::PlayerVariable { .. } => ')',
        Value::Array(_) => ']',
        _ => return (start < node.end).then_some(start..node.end),
    };
    let end = if children.is_empty() {
        // Zero-argument call: `Name` alone, or `Name()` spelled out.
        let (index, char) = scan.next(node.end)?;
        if char == '(' {
            let (close, ')') = scan.next(index + 1)? else {
                return None;
            };
            close + 1
        } else {
            node.end
        }
    } else if contains_children {
        node.end
    } else if infix {
        kids.iter().map(|kid| kid.end).max()?.max(node.end)
    } else {
        let (index, char) = scan.next(kids.last()?.end)?;
        (char == closer).then_some(index + 1)?
    };
    (start < end).then_some(start..end)
}

/// The end byte of `value`'s authored extent — see [`expr_extent_bytes`].
fn expr_extent_end(
    value: &Value,
    span_of: &mut dyn FnMut(&[usize]) -> Option<Span>,
    scan: &SourceScan,
) -> Option<usize> {
    Some(expr_extent_bytes(value, span_of, scan)?.end)
}

/// The byte a derived/infix-shaped expression begins at: from the earliest
/// child position, walk back over the leftmost token's characters to the
/// `(` or `,` that separates it from the previous argument, then forward
/// past trivia.
fn expr_start_anchor(scan: &SourceScan, mut pos: usize) -> Option<usize> {
    let text = scan.document.text();
    loop {
        pos = scan.prev(pos)?.0;
        if text[pos..]
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '.'))
        {
            continue;
        }
        return match text[pos..].chars().next()? {
            '(' | ',' => Some(scan.next(pos + 1)?.0),
            _ => None,
        };
    }
}

/// The start byte of the `Else If (<condition>);` marker statement whose
/// condition's authored extent ends at `condition_end`: forward to the
/// marker's `;` through its `)`s, back to the matching `(`, then over the
/// `Else If` phrase.
fn chain_marker_start(scan: &SourceScan, condition_end: usize, phrase: &str) -> Option<usize> {
    let mut pos = condition_end;
    let semi = loop {
        let (index, char) = scan.next(pos)?;
        match char {
            ')' => pos = index + 1,
            ';' => break index,
            _ => return None,
        }
    };
    let (close, ')') = scan.prev(semi)? else {
        return None;
    };
    let open = paren_match_back(scan, close)?;
    let (phrase_start, phrase_end) = word_back(scan, open)?;
    if &scan.document.text()[phrase_start..phrase_end] != phrase {
        return None;
    }
    let (else_start, else_end) = word_back(scan, phrase_start)?;
    (&scan.document.text()[else_start..else_end] == "Else").then_some(else_start)
}

/// The start byte of the `keyword;` marker ending before `anchor` — an
/// `Else` or `End` statement carries no span, so it is found by its `;`
/// directly before the next authored position.
fn keyword_marker_start(scan: &SourceScan, keyword: &str, anchor: usize) -> Option<usize> {
    let (semi, ';') = scan.prev(anchor)? else {
        return None;
    };
    let (start, end) = word_back(scan, semi)?;
    (&scan.document.text()[start..end] == keyword).then_some(start)
}

/// The `(` matching the `)` at `close`, scanning backward over trivia and
/// balanced parens. Strings refuse outright — a paren inside a string
/// would confuse the depth count, and a rare shape the transaction's
/// reparse would catch anyway.
fn paren_match_back(scan: &SourceScan, close: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut pos = close + 1;
    loop {
        let (index, char) = scan.prev(pos)?;
        match char {
            ')' => depth += 1,
            '(' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            '"' | ';' | '{' | '}' => return None,
            _ => {}
        }
        pos = index;
    }
}

/// The word ending immediately before `pos`, skipping trivia.
fn word_back(scan: &SourceScan, pos: usize) -> Option<(usize, usize)> {
    let text = scan.document.text();
    let (end_pos, c) = scan.prev(pos)?;
    if !c.is_alphabetic() {
        return None;
    }
    let end = end_pos + c.len_utf8();
    let mut start = end_pos;
    while start > 0
        && text[..start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
    {
        start = text[..start]
            .chars()
            .next_back()
            .map_or(start, |c| start - c.len_utf8());
    }
    Some((start, end))
}

/// Where a collected value node sits inside the loop scope. The
/// `Evaluate Once` remediation is only offered for positions the engine
/// evaluates once per action execution; re-evaluating positions must keep
/// their live read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OccurrenceOrigin {
    /// The loop's own `While` condition: re-evaluated every iteration, so
    /// freezing it would change loop semantics.
    LoopCondition,
    /// An `Update Every Frame` call or one of its descendants: the call
    /// exists to re-evaluate its argument every tick, so freezing it —
    /// directly, or by wrapping a containing occurrence — would stale the
    /// value it keeps live.
    Reevaluating,
    /// A body action's argument position, by index into the body slice.
    BodyAction(usize),
}

/// What the collector records for every gathered value node.
struct Collected<'a> {
    values: Vec<&'a Value>,
    parents: Vec<Option<usize>>,
    /// The reported node span (`identifier.or(span)` — a callee phrase,
    /// not the full expression extent).
    spans: Vec<Option<Span>>,
    /// The node's path from its argument root, for span lookups.
    paths: Vec<Vec<usize>>,
    /// The argument index the node was collected under.
    arguments: Vec<usize>,
    origins: Vec<OccurrenceOrigin>,
}

impl Collected<'_> {
    fn new() -> Self {
        Collected {
            values: Vec::new(),
            parents: Vec::new(),
            spans: Vec::new(),
            paths: Vec::new(),
            arguments: Vec::new(),
            origins: Vec::new(),
        }
    }
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
    let mut collected = Collected::new();
    if let Action::While { condition } = &rule.actions[loop_action] {
        collect_value_tree(
            condition,
            0,
            None,
            &mut |path| program.action_argument_value_span(rule_id, loop_action, 0, path),
            &mut collected,
            OccurrenceOrigin::LoopCondition,
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
                    argument,
                    None,
                    &mut |path| {
                        program.action_argument_value_span(rule_id, action_id, argument, path)
                    },
                    &mut collected,
                    OccurrenceOrigin::BodyAction(action),
                );
            });
        }
        action += 1;
    }

    duplicated_value_families(&collected.values, &collected.parents)
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
                span: collected.spans[first].or_else(|| program.action_span(rule_id, loop_action)),
                rule: rule_id,
                action: Some(loop_action),
                value: value_ids
                    .get(&(collected.values[first] as *const Value as usize))
                    .copied(),
                evidence: EvidenceClass::Exact,
                boundedness: None,
                fix: evaluate_once_fix(
                    program,
                    rule_id,
                    loop_action,
                    body_start,
                    body,
                    &collected,
                    &family,
                ),
            }
        })
        .collect()
}

/// Collect a value tree for `repeated-value`, treating `Evaluate Once`
/// subtrees as opaque leaves: their contents evaluate at most once per
/// action evaluation, so they can never be the repeated-evaluation defect —
/// and the rule must not re-flag code the remediation already wrapped
/// (#556).
fn collect_value_tree<'a>(
    value: &'a Value,
    argument: usize,
    parent: Option<usize>,
    span_at: &mut impl FnMut(&[usize]) -> Option<Span>,
    collected: &mut Collected<'a>,
    origin: OccurrenceOrigin,
) {
    collect_value_at(
        value,
        argument,
        parent,
        &mut Vec::new(),
        span_at,
        collected,
        origin,
    );
}

fn collect_value_at<'a>(
    value: &'a Value,
    argument: usize,
    parent: Option<usize>,
    path: &mut Vec<usize>,
    span_at: &mut impl FnMut(&[usize]) -> Option<Span>,
    collected: &mut Collected<'a>,
    origin: OccurrenceOrigin,
) {
    if matches!(value, Value::Call { name, .. } if name == "evaluateOnce") {
        return;
    }
    let origin = if matches!(value, Value::Call { name, .. } if name == "updateEveryFrame") {
        OccurrenceOrigin::Reevaluating
    } else {
        origin
    };
    let index = collected.values.len();
    collected.values.push(value);
    collected.parents.push(parent);
    collected.spans.push(span_at(path));
    collected.paths.push(path.clone());
    collected.arguments.push(argument);
    collected.origins.push(origin);
    let parent = Some(index);
    for (child_index, child) in value_children(value).into_iter().enumerate() {
        path.push(child_index);
        collect_value_at(child, argument, parent, path, span_at, collected, origin);
        path.pop();
    }
}

/// Whether the `Evaluate Once` remediation applies to one duplicated
/// family (#556): every occurrence must resolve to an exact authored
/// extent, and after wrapping the once-evaluated occurrences at most one
/// unwrappable occurrence may keep the shape — otherwise the family (and
/// its finding) survives.
#[allow(clippy::too_many_arguments)]
fn evaluate_once_fix(
    program: &Program,
    rule_id: RuleId,
    loop_action: ActionId,
    body_start: usize,
    body: &[Action],
    collected: &Collected,
    family: &[usize],
) -> Option<LintFix> {
    let catalog = crate::catalog::builtin().ok()?;
    let mut occurrences = Vec::new();
    let mut unwrappable = 0usize;
    for &member in family {
        let (action, wrappable) = match collected.origins[member] {
            OccurrenceOrigin::LoopCondition | OccurrenceOrigin::Reevaluating => {
                (loop_action, false)
            }
            OccurrenceOrigin::BodyAction(index) => (
                body_start + index,
                !action_argument_reevaluates(&catalog, &body[index], collected.arguments[member])
                    && !contains_update_every_frame(collected.values[member]),
            ),
        };
        let extent = wrappable.then(|| {
            let file = collected.spans[member]?.file;
            let scan = SourceScan::new(program.source(file)?, file);
            let base = &collected.paths[member];
            let extent = expr_extent_bytes(
                collected.values[member],
                &mut |path| {
                    let mut full = base.clone();
                    full.extend_from_slice(path);
                    program.action_argument_value_span(
                        rule_id,
                        action,
                        collected.arguments[member],
                        &full,
                    )
                },
                &scan,
            )?;
            Some(scan.span(extent.start, extent.end))
        });
        match extent {
            Some(Some(span)) => occurrences.push(span),
            _ => unwrappable += 1,
        }
    }
    (unwrappable <= 1 && !occurrences.is_empty()).then_some(LintFix::EvaluateOnce { occurrences })
}

/// Whether `value` holds a live `Update Every Frame` subtree — one not
/// already frozen inside `Evaluate Once`. Wrapping such an expression in
/// `Evaluate Once` would freeze a value the engine re-evaluates every tick.
fn contains_update_every_frame(value: &Value) -> bool {
    match value {
        Value::Call { name, .. } if name == "evaluateOnce" => false,
        Value::Call { name, .. } if name == "updateEveryFrame" => true,
        _ => value_children(value)
            .iter()
            .any(|child| contains_update_every_frame(child)),
    }
}

/// Whether executing `action` can evaluate `argument` — the call's
/// parameter position — more than once (#562): `Wait Until`'s polled
/// condition and the `Loop If` family's condition are re-evaluated per
/// pass (the condition `Loop` itself never polls), and a persistent-object
/// action's reevaluation-mode parameter selects which fields stay live.
/// The mode is catalog-pinned: its parameter's domain ends in `Reeval`,
/// and the selected member's `reevaluation_coverage` names the parameter
/// positions it re-evaluates. `If`/`Else If` conditions and assignment
/// arguments evaluate once per action execution, so only `Call` actions
/// can re-evaluate.
///
/// The gate stays conservative whenever coverage is unknown: a member that
/// is not a reviewed literal (computed, or a non-`Enum` value), a `None`
/// mode omitted where the catalog declares no explicit `NONE` default, an
/// action without `reevaluation_coverage`, or a member absent from that
/// map all keep their argument live — a wrong `Evaluate Once` freezes a
/// value the engine reevaluates, which is a silent behavior change.
fn action_argument_reevaluates(catalog: &Catalog, action: &Action, argument: usize) -> bool {
    let Action::Call { name, args } = action else {
        return false;
    };
    if matches!(
        name.as_str(),
        "waitUntil" | "loopIf" | "loopIfConditionIsTrue" | "__loopIfConditionIsFalse__"
    ) {
        // `Wait Until` polls only its condition argument; the `Loop If`
        // family's only input is the condition. Their other positions
        // evaluate once per action execution.
        return argument == 0;
    }
    let Some(entry) = catalog.entry(Kind::Action, name) else {
        return false;
    };
    (0..entry.param_count()).any(|index| {
        let Some(domain) = entry.param_domain(index) else {
            return false;
        };
        if !domain.ends_with("Reeval") {
            return false;
        }
        match args.get(index) {
            Some(Value::Enum { value, .. }) if value == "NONE" => false,
            Some(Value::Enum { value, .. }) => {
                // Reviewed coverage names the live positions; an
                // unreviewed member keeps the conservative refusal.
                entry
                    .reevaluation_coverage(value)
                    .is_none_or(|coverage| coverage.contains(&argument))
            }
            // A computed mode cannot be proven `NONE`, so every argument
            // stays live — matching the previous whole-action refusal.
            Some(_) => true,
            // Omitted means the engine's implicit mode applies — and the
            // catalog only records a declared default when there is one —
            // so only an explicit `NONE` default proves the field evaluates
            // once; anything else stays live.
            None => entry
                .param_default(index)
                .is_none_or(|default| default.rsplit('.').next() != Some("NONE")),
        }
    })
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
    visit_value_tree(value, None, &mut |value, _, _| {
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
