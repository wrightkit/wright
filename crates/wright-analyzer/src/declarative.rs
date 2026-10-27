use workshop_rs::catalog::{Catalog, Kind, Locale};
use workshop_rs::source::Span;
use workshop_rs::{Action as PublicAction, Program as PublicProgram, Value as PublicValue};

use crate::analysis::{EvidenceClass, Severity};
use crate::canonical::{
    Finding, dead_actions, matching_end, value_identity_map, visit_action_roots,
};

mod error;
mod pattern;
mod schema;

pub use error::RuleError;
pub use schema::{
    ActionKind, ActionPattern, CallPattern, ComparisonOperator, ComparisonPattern, Count,
    EnumPattern, InArgs, NodePattern, ParameterPattern, ReportMode, RuleDefinition, RuleMatcher,
    RuleMetadata, Scope, Scopes, ValuePattern, WithinPattern,
};

use pattern::{
    CanonicalActionPattern, CanonicalNodePattern, CanonicalValuePattern, canonical_action,
    canonical_node, canonical_value, public_action_matches, public_value_matches,
    search_value_tree,
};
pub(crate) use schema::default_true;

#[derive(Debug, Clone)]
pub struct DeclarativeRule {
    id: String,
    metadata: RuleMetadata,
    scopes: Vec<Scope>,
    report: ReportMode,
    event: Option<String>,
    conditions: Option<CanonicalNodePattern>,
    actions: Vec<CanonicalActionPattern>,
    values: Vec<CanonicalValuePattern>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuleIdentity {
    /// External rule: exactly `namespace/rule-id` outside the reserved
    /// `wright` namespace (SPEC-309).
    External,
    /// First-party package rule: a bare built-in ID or the reserved `wright`
    /// namespace — the only source allowed bare IDs (issue #843).
    FirstParty,
}

impl DeclarativeRule {
    pub fn from_definition(
        definition: RuleDefinition,
        catalog: &Catalog,
    ) -> Result<Self, RuleError> {
        Self::from_definition_with_identity(definition, catalog, RuleIdentity::External)
    }

    pub(crate) fn from_definition_with_identity(
        definition: RuleDefinition,
        catalog: &Catalog,
        identity: RuleIdentity,
    ) -> Result<Self, RuleError> {
        let RuleDefinition {
            id,
            locale: locale_name,
            metadata,
            matcher,
        } = definition;
        match identity {
            RuleIdentity::External => validate_identity(&id)?,
            RuleIdentity::FirstParty => validate_first_party_identity(&id)?,
        }
        if identity != RuleIdentity::FirstParty {
            if metadata.evidence.is_some() {
                return Err(RuleError::FirstPartyMetadataField { field: "evidence" });
            }
            if metadata.severity.is_some() {
                return Err(RuleError::FirstPartyMetadataField { field: "severity" });
            }
        }
        // Error-severity audit (#1010): an error-severity finding yields
        // `ok: false` and `exit: 1`, so `error` is reserved for findings
        // confirmed by runtime evaluation. A first-party rule that declares
        // `severity: error` without `evidence: runtime-validated` is rejected
        // rather than silently downgraded — the load error is the structured
        // `lint-rule-error` diagnostic, same as any invalid definition.
        if identity == RuleIdentity::FirstParty
            && metadata.severity == Some(Severity::Error)
            && metadata.evidence != Some(EvidenceClass::RuntimeValidated)
        {
            return Err(RuleError::UnjustifiedErrorSeverity);
        }
        let locale = Locale::new(&locale_name);
        if !catalog.supports(&locale) {
            return Err(RuleError::UnsupportedLocale(locale_name));
        }
        let RuleMatcher {
            event,
            scope,
            conditions,
            actions,
            values,
            report,
        } = matcher;
        if scope.list().is_empty() {
            return Err(RuleError::EmptyScopeList);
        }
        if report == ReportMode::EachMatch && values.is_empty() {
            return Err(RuleError::EachMatchNeedsValues);
        }
        let event = event
            .as_deref()
            .map(|value| resolve(catalog, Kind::Event, &locale, value))
            .transpose()?;
        let conditions = conditions
            .map(|pattern| canonical_node(pattern, catalog, &locale))
            .transpose()?;
        let actions = actions
            .into_iter()
            .map(|pattern| canonical_action(pattern, catalog, &locale))
            .collect::<Result<Vec<_>, _>>()?;
        let values = values
            .into_iter()
            .map(|pattern| canonical_value(pattern, catalog, &locale, true))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            id,
            metadata,
            scopes: scope.list().to_vec(),
            report,
            event,
            conditions,
            actions,
            values,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn metadata(&self) -> &RuleMetadata {
        &self.metadata
    }

    /// Declarative matchers only report exact structural facts from canonical
    /// Workshop representations; they do not claim runtime behavior. A
    /// first-party package rule may declare its class (#970, #616 addendum).
    pub fn evidence(&self) -> EvidenceClass {
        self.metadata.evidence.unwrap_or(EvidenceClass::Exact)
    }

    /// The built-in policy default — or the first-party package's declared
    /// default (#970). Project configuration owns overrides.
    pub fn default_severity(&self) -> Severity {
        self.metadata.severity.unwrap_or(Severity::Warning)
    }

    /// Run a declarative matcher over the canonical public Workshop program.
    pub fn run(
        &self,
        program: &PublicProgram,
        rule: usize,
        min_matches: Option<usize>,
        max_matches: Option<usize>,
    ) -> Vec<Finding> {
        self.run_canonical(program, rule, min_matches, max_matches)
    }

    /// Run a declarative matcher over the canonical public Workshop program.
    pub fn run_canonical(
        &self,
        program: &PublicProgram,
        rule: usize,
        min_matches: Option<usize>,
        max_matches: Option<usize>,
    ) -> Vec<Finding> {
        let Some(rule_data) = program.rules.get(rule) else {
            return Vec::new();
        };
        if rule_data.disabled
            || self
                .event
                .as_deref()
                .is_some_and(|event| event != rule_data.event.catalog_id())
        {
            return Vec::new();
        }
        let dead = dead_actions(&rule_data.actions);
        let value_ids = (!self.values.is_empty()).then(|| value_identity_map(program));
        let scopes = public_scopes(program, rule, rule_data, &self.scopes, &dead);
        let mut findings = Vec::new();
        for scope in scopes {
            let PublicScope {
                anchor_action,
                actions,
                values: gate_values,
                roots,
                anchor,
            } = scope;
            if let Some(pattern) = &self.conditions {
                let count = gate_values
                    .iter()
                    .filter(|value| public_value_matches(value, &pattern.value))
                    .count();
                if !pattern.count.accepts(count) {
                    continue;
                }
            }
            let mut counts = Vec::new();
            if self.actions.iter().any(|pattern| {
                let count = actions
                    .iter()
                    .filter(|action| public_action_matches(action, pattern))
                    .count();
                let accepted = if pattern.present {
                    pattern.count.accepts(count)
                } else {
                    count == 0 && pattern.count.accepts(0)
                };
                counts.push(count);
                !accepted
            }) {
                continue;
            }
            if self.values.is_empty() {
                let matched = counts.iter().copied().max().unwrap_or(1);
                if min_matches.is_some_and(|min| matched < min)
                    || max_matches.is_some_and(|max| matched > max)
                {
                    continue;
                }
                findings.push(self.finding(matched, anchor, rule, anchor_action, None));
                continue;
            }
            // `values`: search every value tree of the scope for target nodes.
            let mut matches: Vec<(Option<Span>, Option<usize>, Option<usize>)> = Vec::new();
            for root in &roots {
                let mut path = Vec::new();
                let mut ancestors = Vec::new();
                search_value_tree(
                    root.value(),
                    &mut ancestors,
                    &self.values,
                    &mut path,
                    &mut |path, node| {
                        let span = root.span_of(program, rule, path, anchor);
                        let value_id = value_ids.as_ref().and_then(|ids| {
                            ids.get(&(node as *const PublicValue as usize)).copied()
                        });
                        let owner_action = match root {
                            ValueRoot::Condition { .. } => None,
                            ValueRoot::Argument { action, .. } => Some(*action),
                        };
                        matches.push((span, value_id, owner_action));
                    },
                );
            }
            let matched = matches.len();
            if matched == 0
                || min_matches.is_some_and(|min| matched < min)
                || max_matches.is_some_and(|max| matched > max)
            {
                continue;
            }
            match self.report {
                ReportMode::EachMatch => {
                    for (span, value_id, owner_action) in matches {
                        // The native loop rules address the scope anchor
                        // (the loop), not the statement owning the matched
                        // value; a `rule` scope has no anchor, so it keeps
                        // the owning statement (`nested-binder-lambda`).
                        let action = anchor_action.or(owner_action);
                        findings.push(self.finding(matched, span, rule, action, value_id));
                    }
                }
                ReportMode::Scope => {
                    findings.push(self.finding(matched, anchor, rule, anchor_action, None));
                }
            }
        }
        findings
    }

    fn finding(
        &self,
        matched: usize,
        span: Option<Span>,
        rule: usize,
        action: Option<usize>,
        value: Option<usize>,
    ) -> Finding {
        Finding {
            code: self.id().to_string(),
            severity: self.default_severity(),
            message: format!(
                "{} (matched {matched} node{})",
                self.metadata().summary,
                if matched == 1 { "" } else { "s" }
            ),
            span,
            rule,
            action,
            value,
            evidence: self.evidence(),
            boundedness: None,
            fix: None,
            repeated: None,
        }
    }
}

/// One scope region a matcher inspects: `anchor_action` is the scope's own
/// action index (`None` for a whole rule), `actions` are the live actions
/// inside the scope for the `actions` matcher, `values` are the gate values
/// for the `conditions` matcher, `roots` are the value trees the `values`
/// matcher searches (#970), and `anchor` is the scope span.
struct PublicScope<'a> {
    anchor_action: Option<usize>,
    actions: Vec<&'a PublicAction>,
    values: Vec<&'a PublicValue>,
    roots: Vec<ValueRoot<'a>>,
    anchor: Option<Span>,
}

/// A value tree the `values` matcher searches, with enough addressing to
/// anchor each match (#970).
#[derive(Clone, Copy)]
enum ValueRoot<'a> {
    /// A live rule condition: spans resolve through
    /// `condition_value_span(rule, condition, path)`.
    Condition {
        condition: usize,
        value: &'a PublicValue,
    },
    /// A live action argument: spans resolve through
    /// `action_argument_value_span(rule, action, argument, path)`. The
    /// `action` is also the finding's owning action index.
    Argument {
        action: usize,
        argument: usize,
        value: &'a PublicValue,
    },
}

impl<'a> ValueRoot<'a> {
    fn value(&self) -> &'a PublicValue {
        match self {
            Self::Condition { value, .. } | Self::Argument { value, .. } => value,
        }
    }

    /// The match span: the deepest recorded provenance on the path, else the
    /// owning statement's span, else the scope anchor, else the rule.
    fn span_of(
        &self,
        program: &PublicProgram,
        rule: usize,
        path: &[usize],
        anchor: Option<Span>,
    ) -> Option<Span> {
        match self {
            Self::Condition { condition, .. } => (0..=path.len())
                .rev()
                .find_map(|len| program.condition_value_span(rule, *condition, &path[..len]))
                .or_else(|| program.rule_span(rule)),
            Self::Argument {
                action, argument, ..
            } => (0..=path.len())
                .rev()
                .find_map(|len| {
                    program.action_argument_value_span(rule, *action, *argument, &path[..len])
                })
                .or_else(|| program.action_span(rule, *action))
                .or(anchor)
                .or_else(|| program.rule_span(rule)),
        }
    }
}

fn public_scopes<'a>(
    program: &PublicProgram,
    rule_id: usize,
    rule: &'a workshop_rs::Rule,
    scopes: &[Scope],
    dead: &[bool],
) -> Vec<PublicScope<'a>> {
    let mut result = Vec::new();
    if scopes.contains(&Scope::Rule) {
        let mut roots = Vec::new();
        for (index, condition) in rule.conditions.iter().enumerate() {
            if condition.disabled {
                continue;
            }
            roots.push(ValueRoot::Condition {
                condition: index,
                value: &condition.value,
            });
        }
        for (index, action) in rule.actions.iter().enumerate() {
            if dead[index] {
                continue;
            }
            visit_action_roots(action, &mut |argument, value| {
                roots.push(ValueRoot::Argument {
                    action: index,
                    argument,
                    value,
                });
            });
        }
        result.push(PublicScope {
            anchor_action: None,
            actions: public_scope_actions(&rule.actions, dead, 0, rule.actions.len()),
            values: rule
                .conditions
                .iter()
                .filter(|condition| !condition.disabled)
                .map(|condition| &condition.value)
                .collect(),
            roots,
            anchor: program.rule_span(rule_id),
        });
    }
    let wanted = |action: &PublicAction| {
        scopes.iter().any(|scope| {
            matches!(
                (scope, action),
                (Scope::While, PublicAction::While { .. })
                    | (
                        Scope::ForGlobalVariable,
                        PublicAction::ForGlobalVariable { .. }
                    )
                    | (
                        Scope::ForPlayerVariable,
                        PublicAction::ForPlayerVariable { .. }
                    )
                    | (Scope::If, PublicAction::If { .. })
            )
        })
    };
    for (index, action) in rule.actions.iter().enumerate() {
        if dead[index] || !wanted(action) {
            continue;
        }
        let Some(close) = matching_end(&rule.actions, index) else {
            continue;
        };
        // `conditions` gate values keep the established semantics: a `while`
        // scope sees its own condition, an `if` scope sees the chain's
        // condition list. The `values` search range differs (#970 owner
        // decision): a loop scope searches its live body-action arguments
        // only — not the loop's own condition — and a nested loop's body is
        // searched once per enclosing loop. An `if` scope's `values` search
        // covers its condition list.
        let values = match action {
            PublicAction::While { condition } => vec![condition],
            PublicAction::If { condition } => {
                public_if_conditions(&rule.actions, index, close, condition)
            }
            _ => Vec::new(),
        };
        let mut roots = Vec::new();
        match action {
            PublicAction::If { condition } => {
                roots = public_if_condition_roots(&rule.actions, index, close, condition);
            }
            _ => {
                for (body_index, body_action) in rule.actions[index + 1..close].iter().enumerate() {
                    let action_id = index + 1 + body_index;
                    if dead[action_id] {
                        continue;
                    }
                    visit_action_roots(body_action, &mut |argument, value| {
                        roots.push(ValueRoot::Argument {
                            action: action_id,
                            argument,
                            value,
                        });
                    });
                }
            }
        }
        result.push(PublicScope {
            anchor_action: Some(index),
            actions: public_scope_actions(&rule.actions, dead, index + 1, close),
            values,
            roots,
            anchor: program.action_span(rule_id, index),
        });
    }
    result
}

/// The `values` search roots of an `if` scope: every condition of the
/// chain, addressed as the owning action's argument 0 (#970).
fn public_if_condition_roots<'a>(
    actions: &'a [PublicAction],
    start: usize,
    close: usize,
    first: &'a PublicValue,
) -> Vec<ValueRoot<'a>> {
    let mut roots = vec![ValueRoot::Argument {
        action: start,
        argument: 0,
        value: first,
    }];
    let mut index = start + 1;
    while index < close {
        match &actions[index] {
            PublicAction::ElseIf { condition } => roots.push(ValueRoot::Argument {
                action: index,
                argument: 0,
                value: condition,
            }),
            _ => {
                if let Some(nested_close) = matching_end(actions, index) {
                    index = nested_close + 1;
                    continue;
                }
            }
        }
        index += 1;
    }
    roots
}

fn public_scope_actions<'a>(
    actions: &'a [PublicAction],
    dead: &[bool],
    start: usize,
    end: usize,
) -> Vec<&'a PublicAction> {
    actions[start..end]
        .iter()
        .zip(&dead[start..end])
        .filter(|(action, dead)| {
            !*dead
                && !matches!(
                    action,
                    PublicAction::Else | PublicAction::ElseIf { .. } | PublicAction::End
                )
        })
        .map(|(action, _)| action)
        .collect()
}

fn public_if_conditions<'a>(
    actions: &'a [PublicAction],
    start: usize,
    close: usize,
    first: &'a PublicValue,
) -> Vec<&'a PublicValue> {
    let mut conditions = vec![first];
    let mut index = start + 1;
    while index < close {
        match &actions[index] {
            PublicAction::ElseIf { condition } => conditions.push(condition),
            _ => {
                if let Some(nested_close) = matching_end(actions, index) {
                    index = nested_close + 1;
                    continue;
                }
            }
        }
        index += 1;
    }
    conditions
}

fn resolve(
    catalog: &Catalog,
    kind: Kind,
    locale: &Locale,
    spelling: &str,
) -> Result<String, RuleError> {
    catalog
        .entry(kind, spelling)
        .map(|entry| entry.id.clone())
        .or_else(|| {
            catalog
                .resolve(kind, locale, spelling)
                .map(|entry| entry.id.clone())
        })
        .ok_or_else(|| RuleError::UnknownSpelling {
            kind: kind.as_str(),
            spelling: spelling.to_string(),
            locale: locale.to_string(),
        })
}

fn validate_identity(id: &str) -> Result<(), RuleError> {
    let Some((namespace, rule_id)) = id.split_once('/') else {
        return Err(RuleError::InvalidIdentity(id.to_string()));
    };
    if namespace.is_empty()
        || rule_id.is_empty()
        || id.matches('/').count() != 1
        || namespace == "wright"
    {
        return Err(RuleError::InvalidIdentity(id.to_string()));
    }
    Ok(())
}

fn validate_first_party_identity(id: &str) -> Result<(), RuleError> {
    let valid = match id.split_once('/') {
        None => !id.is_empty(),
        Some(("wright", rule_id)) => !rule_id.is_empty() && id.matches('/').count() == 1,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(RuleError::InvalidFirstPartyIdentity(id.to_string()))
    }
}
