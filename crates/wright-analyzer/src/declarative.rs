use std::fmt;

use serde::Deserialize;
use workshop_rs::source::Span;
use workshop_rs::wir::{self, Action, ActionId, Event, RuleId, Value, ValueId};
use workshop_rs_catalog::catalog::{Catalog, Kind, Locale};

use crate::analysis::{EvidenceClass, Finding, Severity};
use crate::facts::{RuleFacts, SemanticFacts};

const DEFAULT_LOCALE: &str = "en-US";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleDefinition {
    pub id: String,
    #[serde(default = "default_locale")]
    pub locale: String,
    pub metadata: RuleMetadata,
    pub matcher: RuleMatcher,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleMetadata {
    pub summary: String,
    pub rationale: String,
    pub documentation: String,
    #[serde(rename = "known-limits")]
    pub known_limits: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct RuleMatcher {
    pub event: Option<String>,
    #[serde(default)]
    pub scope: Scope,
    pub conditions: Option<NodePattern>,
    #[serde(default)]
    pub actions: Vec<ActionPattern>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    #[default]
    Rule,
    While,
    ForGlobalVariable,
    ForPlayerVariable,
    If,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodePattern {
    #[serde(flatten)]
    pub value: ValuePattern,
    #[serde(default)]
    pub count: Count,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionPattern {
    pub kind: Option<ActionKind>,
    pub name: Option<String>,
    #[serde(default)]
    pub args: Vec<ValuePattern>,
    #[serde(default, alias = "params")]
    pub parameters: Vec<ParameterPattern>,
    #[serde(default)]
    pub count: Count,
    #[serde(default = "default_true")]
    pub present: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterPattern {
    pub name: String,
    #[serde(flatten)]
    pub value: ValuePattern,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionKind {
    Any,
    Call,
    While,
    ForGlobalVariable,
    ForPlayerVariable,
    If,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Count {
    pub min: Option<usize>,
    pub max: Option<usize>,
}

impl Count {
    fn accepts(&self, count: usize) -> bool {
        self.min.is_none_or(|min| count >= min) && self.max.is_none_or(|max| count <= max)
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ValuePattern {
    pub number: Option<f64>,
    pub string: Option<String>,
    pub boolean: Option<bool>,
    pub call: Option<CallPattern>,
    #[serde(rename = "enum")]
    pub enum_value: Option<EnumPattern>,
    pub comparison: Option<ComparisonPattern>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonPattern {
    pub operator: ComparisonOperator,
    pub value: Box<ValuePattern>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub enum ComparisonOperator {
    #[serde(rename = "<")]
    Less,
    #[serde(rename = "<=")]
    LessOrEqual,
    #[serde(rename = ">")]
    Greater,
    #[serde(rename = ">=")]
    GreaterOrEqual,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallPattern {
    pub name: String,
    #[serde(default)]
    pub args: Vec<ValuePattern>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnumPattern {
    pub domain: String,
    pub member: String,
}

#[derive(Debug, Clone)]
pub struct DeclarativeRule {
    definition: RuleDefinition,
    event: Option<String>,
    conditions: Option<CanonicalNodePattern>,
    actions: Vec<CanonicalActionPattern>,
}

#[derive(Debug, Clone)]
struct CanonicalNodePattern {
    value: CanonicalValuePattern,
    count: Count,
}

#[derive(Debug, Clone)]
struct CanonicalActionPattern {
    kind: Option<ActionKind>,
    name: Option<String>,
    args: Vec<CanonicalValuePattern>,
    parameters: Vec<(usize, CanonicalValuePattern)>,
    count: Count,
    present: bool,
}

type ScopeMatch = (Option<ActionId>, Vec<ActionId>, Vec<ValueId>, Option<Span>);

#[derive(Debug, Clone)]
enum CanonicalValuePattern {
    Any,
    Number(f64),
    String(String),
    Boolean(bool),
    Call {
        name: String,
        args: Vec<CanonicalValuePattern>,
    },
    Enum {
        domain: String,
        member: String,
    },
    Comparison {
        operator: ComparisonOperator,
        value: Box<CanonicalValuePattern>,
    },
}

impl RuleDefinition {
    pub fn from_yaml_str(input: &str) -> Result<Self, RuleError> {
        serde_yaml::from_str(input).map_err(RuleError::Yaml)
    }
}

impl DeclarativeRule {
    pub fn from_definition(
        definition: RuleDefinition,
        catalog: &Catalog,
    ) -> Result<Self, RuleError> {
        validate_identity(&definition.id)?;
        let locale = Locale::new(&definition.locale);
        if !catalog.supports(&locale) {
            return Err(RuleError::UnsupportedLocale(definition.locale));
        }
        let event = definition
            .matcher
            .event
            .as_deref()
            .map(|value| resolve(catalog, Kind::Event, &locale, value))
            .transpose()?;
        let conditions = definition
            .matcher
            .conditions
            .as_ref()
            .map(|pattern| canonical_node(pattern, catalog, &locale))
            .transpose()?;
        let actions = definition
            .matcher
            .actions
            .iter()
            .map(|pattern| canonical_action(pattern, catalog, &locale))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            definition,
            event,
            conditions,
            actions,
        })
    }

    pub fn id(&self) -> &str {
        &self.definition.id
    }

    pub fn metadata(&self) -> &RuleMetadata {
        &self.definition.metadata
    }

    /// Declarative matchers only report exact structural facts from canonical
    /// WIR; they do not claim runtime behavior.
    pub fn evidence(&self) -> EvidenceClass {
        EvidenceClass::Exact
    }

    /// The built-in policy default. Project configuration owns overrides.
    pub fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    pub fn run(
        &self,
        facts: &SemanticFacts<'_>,
        rule: RuleId,
        min_matches: Option<usize>,
        max_matches: Option<usize>,
    ) -> Vec<Finding> {
        let Some(rule_facts) = facts.rule(rule) else {
            return Vec::new();
        };
        let scopes = matching_scopes(&rule_facts, self.definition.matcher.scope);
        let mut findings = Vec::new();
        for (scope_id, actions, conditions, anchor) in scopes {
            if self
                .event
                .as_deref()
                .is_some_and(|event| event != event_id(rule_facts.event()))
            {
                continue;
            }
            if let Some(pattern) = &self.conditions {
                let count = matching_values(&rule_facts, conditions, &pattern.value);
                if !pattern.count.accepts(count) {
                    continue;
                }
            }
            let mut counts = Vec::new();
            if self.actions.iter().any(|pattern| {
                let count = matching_actions(&rule_facts, &actions, pattern);
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
            let matched = counts.iter().copied().max().unwrap_or(1);
            if min_matches.is_some_and(|min| matched < min)
                || max_matches.is_some_and(|max| matched > max)
            {
                continue;
            }
            findings.push(Finding {
                code: self.id().to_string(),
                severity: self.default_severity(),
                message: format!(
                    "{} (matched {matched} node{})",
                    self.metadata().summary,
                    if matched == 1 { "" } else { "s" }
                ),
                span: anchor,
                rule,
                action: scope_id,
                value: None,
                evidence: self.evidence(),
                boundedness: None,
                persistent_object: None,
            });
        }
        findings
    }
}

fn matching_scopes<'a>(facts: &RuleFacts<'a>, scope: Scope) -> Vec<ScopeMatch> {
    if matches!(scope, Scope::Rule) {
        return vec![(
            None,
            facts.actions().to_vec(),
            facts.conditions().to_vec(),
            facts
                .program()
                .rules
                .get(facts.id())
                .and_then(|rule| rule.span),
        )];
    }
    let mut result = Vec::new();
    visit_actions(facts.program(), facts.actions(), &mut |id, action| {
        let matches = matches!(
            (scope, action),
            (Scope::While, Action::While { .. })
                | (Scope::ForGlobalVariable, Action::ForGlobalVariable { .. })
                | (Scope::ForPlayerVariable, Action::ForPlayerVariable { .. })
                | (Scope::If, Action::If { .. })
        );
        if !matches {
            return;
        }
        let (body, conditions) = match action {
            Action::While {
                condition, body, ..
            } => (body.clone(), vec![*condition]),
            Action::ForGlobalVariable { body, .. } | Action::ForPlayerVariable { body, .. } => {
                (body.clone(), Vec::new())
            }
            Action::If {
                branches,
                else_body,
                ..
            } => {
                let mut body = branches
                    .iter()
                    .flat_map(|branch| branch.body.clone())
                    .collect::<Vec<_>>();
                if let Some(else_body) = else_body {
                    body.extend(else_body);
                }
                let conditions = branches.iter().map(|branch| branch.condition).collect();
                (body, conditions)
            }
            _ => (Vec::new(), Vec::new()),
        };
        result.push((Some(id), body, conditions, action.span()));
    });
    result
}

fn matching_actions(
    facts: &RuleFacts<'_>,
    actions: &[ActionId],
    pattern: &CanonicalActionPattern,
) -> usize {
    let mut count = 0;
    visit_actions(facts.program(), actions, &mut |id, action| {
        if action_matches(facts, id, action, pattern) {
            count += 1;
        }
    });
    count
}

fn action_matches(
    facts: &RuleFacts<'_>,
    _id: ActionId,
    action: &Action,
    pattern: &CanonicalActionPattern,
) -> bool {
    if let Some(kind) = pattern.kind {
        let matches = matches!(
            (kind, action),
            (ActionKind::Any, _)
                | (ActionKind::Call, Action::Call { .. })
                | (ActionKind::While, Action::While { .. })
                | (
                    ActionKind::ForGlobalVariable,
                    Action::ForGlobalVariable { .. }
                )
                | (
                    ActionKind::ForPlayerVariable,
                    Action::ForPlayerVariable { .. }
                )
                | (ActionKind::If, Action::If { .. })
        );
        if !matches {
            return false;
        }
    }
    let Some((name, args)) = (match action {
        Action::Call { name, args, .. } => Some((name, args)),
        _ => None,
    }) else {
        return pattern.name.is_none() && pattern.args.is_empty();
    };
    if pattern
        .name
        .as_deref()
        .is_some_and(|expected| expected != name)
    {
        return false;
    }
    pattern.args.iter().enumerate().all(|(index, expected)| {
        args.get(index)
            .is_some_and(|actual| value_matches(facts, *actual, expected))
    }) && pattern.parameters.iter().all(|(index, expected)| {
        args.get(*index)
            .is_some_and(|actual| value_matches(facts, *actual, expected))
    })
}

fn matching_values(
    facts: &RuleFacts<'_>,
    values: Vec<ValueId>,
    pattern: &CanonicalValuePattern,
) -> usize {
    values
        .into_iter()
        .filter(|value| value_matches(facts, *value, pattern))
        .count()
}

fn value_matches(facts: &RuleFacts<'_>, id: ValueId, pattern: &CanonicalValuePattern) -> bool {
    let Some(value) = facts.value(id) else {
        return false;
    };
    match (pattern, value) {
        (CanonicalValuePattern::Any, _) => true,
        (CanonicalValuePattern::Number(expected), Value::Number { value, .. }) => expected == value,
        (CanonicalValuePattern::String(expected), Value::String(actual))
        | (CanonicalValuePattern::String(expected), Value::LocalizedString(actual)) => {
            expected == actual
        }
        (CanonicalValuePattern::Boolean(expected), Value::Bool(actual)) => expected == actual,
        (CanonicalValuePattern::Enum { domain, member }, Value::Enum { value_type, value }) => {
            domain == value_type && member == value
        }
        (
            CanonicalValuePattern::Call { name, args },
            Value::Call {
                name: actual,
                args: actual_args,
            },
        ) => {
            name == actual
                && args.len() <= actual_args.len()
                && args
                    .iter()
                    .enumerate()
                    .all(|(index, expected)| value_matches(facts, actual_args[index], expected))
        }
        (
            CanonicalValuePattern::Comparison { operator, value },
            Value::Number { value: actual, .. },
        ) => {
            let CanonicalValuePattern::Number(expected) = value.as_ref() else {
                return false;
            };
            match operator {
                ComparisonOperator::Less => actual < expected,
                ComparisonOperator::LessOrEqual => actual <= expected,
                ComparisonOperator::Greater => actual > expected,
                ComparisonOperator::GreaterOrEqual => actual >= expected,
            }
        }
        _ => false,
    }
}

fn visit_actions(
    program: &wir::Program,
    actions: &[ActionId],
    f: &mut impl FnMut(ActionId, &Action),
) {
    for id in actions {
        let Some(action) = program.actions.get(*id) else {
            continue;
        };
        f(*id, action);
        match action {
            Action::If {
                branches,
                else_body,
                ..
            } => {
                for branch in branches {
                    visit_actions(program, &branch.body, f);
                }
                if let Some(body) = else_body {
                    visit_actions(program, body, f);
                }
            }
            Action::While { body, .. }
            | Action::ForGlobalVariable { body, .. }
            | Action::ForPlayerVariable { body, .. } => visit_actions(program, body, f),
            _ => {}
        }
    }
}

fn canonical_node(
    pattern: &NodePattern,
    catalog: &Catalog,
    locale: &Locale,
) -> Result<CanonicalNodePattern, RuleError> {
    Ok(CanonicalNodePattern {
        value: canonical_value(&pattern.value, catalog, locale)?,
        count: pattern.count.clone(),
    })
}

fn canonical_action(
    pattern: &ActionPattern,
    catalog: &Catalog,
    locale: &Locale,
) -> Result<CanonicalActionPattern, RuleError> {
    let name = pattern
        .name
        .as_deref()
        .map(|name| resolve(catalog, Kind::Action, locale, name))
        .transpose()?;
    let parameters = pattern
        .parameters
        .iter()
        .map(|parameter| {
            let action = name
                .as_deref()
                .and_then(|name| catalog.entry(Kind::Action, name))
                .ok_or_else(|| RuleError::ParameterNeedsActionName(parameter.name.clone()))?;
            let index = resolve_parameter(action, locale, &parameter.name).ok_or_else(|| {
                RuleError::UnknownParameter {
                    action: action.id.clone(),
                    spelling: parameter.name.clone(),
                    locale: locale.to_string(),
                }
            })?;
            Ok((index, canonical_value(&parameter.value, catalog, locale)?))
        })
        .collect::<Result<Vec<_>, RuleError>>()?;
    Ok(CanonicalActionPattern {
        kind: pattern.kind,
        name,
        args: pattern
            .args
            .iter()
            .map(|value| canonical_value(value, catalog, locale))
            .collect::<Result<_, _>>()?,
        parameters,
        count: pattern.count.clone(),
        present: pattern.present,
    })
}

fn resolve_parameter(
    action: &workshop_rs_catalog::catalog::CatalogEntry,
    locale: &Locale,
    spelling: &str,
) -> Option<usize> {
    action.resolve_param(locale, spelling)
}

fn canonical_value(
    pattern: &ValuePattern,
    catalog: &Catalog,
    locale: &Locale,
) -> Result<CanonicalValuePattern, RuleError> {
    let selected = usize::from(pattern.number.is_some())
        + usize::from(pattern.string.is_some())
        + usize::from(pattern.boolean.is_some())
        + usize::from(pattern.call.is_some())
        + usize::from(pattern.enum_value.is_some())
        + usize::from(pattern.comparison.is_some());
    if selected > 1 {
        return Err(RuleError::AmbiguousValuePattern);
    }
    if let Some(number) = pattern.number {
        return Ok(CanonicalValuePattern::Number(number));
    }
    if let Some(string) = &pattern.string {
        return Ok(CanonicalValuePattern::String(string.clone()));
    }
    if let Some(boolean) = pattern.boolean {
        return Ok(CanonicalValuePattern::Boolean(boolean));
    }
    if let Some(call) = &pattern.call {
        return Ok(CanonicalValuePattern::Call {
            name: resolve_value_or_operator(catalog, locale, &call.name)?,
            args: call
                .args
                .iter()
                .map(|value| canonical_value(value, catalog, locale))
                .collect::<Result<_, _>>()?,
        });
    }
    if let Some(en) = &pattern.enum_value {
        let domain = resolve_enum_domain(catalog, locale, &en.domain)?;
        let member = catalog
            .resolve_enum_member(&domain, locale, &en.member)
            .ok_or_else(|| RuleError::UnknownSpelling {
                kind: "enum member",
                spelling: en.member.clone(),
                locale: locale.to_string(),
            })?
            .1;
        return Ok(CanonicalValuePattern::Enum { domain, member });
    }
    if let Some(comparison) = &pattern.comparison {
        let value = canonical_value(&comparison.value, catalog, locale)?;
        if !matches!(value, CanonicalValuePattern::Number(_)) {
            return Err(RuleError::ComparisonNeedsNumber);
        }
        return Ok(CanonicalValuePattern::Comparison {
            operator: comparison.operator,
            value: Box::new(value),
        });
    }
    Ok(CanonicalValuePattern::Any)
}

fn resolve_value_or_operator(
    catalog: &Catalog,
    locale: &Locale,
    spelling: &str,
) -> Result<String, RuleError> {
    resolve(catalog, Kind::Value, locale, spelling).or_else(|value_error| {
        resolve(catalog, Kind::Operator, locale, spelling).or(Err(value_error))
    })
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

fn resolve_enum_domain(
    catalog: &Catalog,
    locale: &Locale,
    spelling: &str,
) -> Result<String, RuleError> {
    catalog
        .enum_domain(spelling)
        .map(|_| spelling.to_string())
        .or_else(|| {
            catalog
                .resolve_enum_domain(locale, spelling)
                .map(str::to_string)
        })
        .ok_or_else(|| RuleError::UnknownSpelling {
            kind: "enum domain",
            spelling: spelling.to_string(),
            locale: locale.to_string(),
        })
}

fn event_id(event: &Event) -> &str {
    match event {
        Event::Global => "global",
        Event::EachPlayer | Event::EachPlayerWithFilters { .. } => "eachPlayer",
        Event::Player { kind, .. } => kind.catalog_id(),
        Event::Subroutine(_) => "subroutine",
    }
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

fn default_locale() -> String {
    DEFAULT_LOCALE.to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug)]
pub enum RuleError {
    Yaml(serde_yaml::Error),
    InvalidIdentity(String),
    UnsupportedLocale(String),
    UnknownSpelling {
        kind: &'static str,
        spelling: String,
        locale: String,
    },
    UnknownParameter {
        action: String,
        spelling: String,
        locale: String,
    },
    ParameterNeedsActionName(String),
    AmbiguousValuePattern,
    ComparisonNeedsNumber,
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Yaml(error) => write!(f, "invalid rule YAML: {error}"),
            Self::InvalidIdentity(id) => write!(
                f,
                "external rule ID '{id}' must be <namespace>/<rule-id>; namespace 'wright' is reserved"
            ),
            Self::UnsupportedLocale(locale) => write!(
                f,
                "rule locale '{locale}' is not supported by the Workshop catalog"
            ),
            Self::UnknownSpelling {
                kind,
                spelling,
                locale,
            } => write!(
                f,
                "unknown {kind} spelling '{spelling}' in locale '{locale}'"
            ),
            Self::UnknownParameter {
                action,
                spelling,
                locale,
            } => write!(
                f,
                "unknown parameter '{spelling}' for action '{action}' in locale '{locale}'"
            ),
            Self::ParameterNeedsActionName(parameter) => {
                write!(f, "parameter '{parameter}' requires an action name")
            }
            Self::AmbiguousValuePattern => {
                f.write_str("a value pattern must select at most one literal or call shape")
            }
            Self::ComparisonNeedsNumber => {
                f.write_str("comparison patterns require a numeric literal")
            }
        }
    }
}

impl std::error::Error for RuleError {}
