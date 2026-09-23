use std::fmt;

use serde::Deserialize;
use workshop_rs::catalog::{Catalog, Kind, Locale};
use workshop_rs::source::Span;
use workshop_rs::{Action as PublicAction, Program as PublicProgram, Value as PublicValue};

use crate::analysis::{EvidenceClass, Severity};
use crate::canonical::Finding;

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
        yaml_serde::from_str(input).map_err(RuleError::Yaml)
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
    /// Workshop representations; they do not claim runtime behavior.
    pub fn evidence(&self) -> EvidenceClass {
        EvidenceClass::Exact
    }

    /// The built-in policy default. Project configuration owns overrides.
    pub fn default_severity(&self) -> Severity {
        Severity::Warning
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
        if self
            .event
            .as_deref()
            .is_some_and(|event| event != public_event_id(&rule_data.event))
        {
            return Vec::new();
        }
        let scopes = public_scopes(program, rule, rule_data, self.definition.matcher.scope);
        let mut findings = Vec::new();
        for (scope_id, actions, values, anchor) in scopes {
            if let Some(pattern) = &self.conditions {
                let count = values
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
            });
        }
        findings
    }
}

type PublicScope<'a> = (
    Option<usize>,
    Vec<&'a PublicAction>,
    Vec<&'a PublicValue>,
    Option<Span>,
);

fn public_scopes<'a>(
    program: &PublicProgram,
    rule_id: usize,
    rule: &'a workshop_rs::Rule,
    scope: Scope,
) -> Vec<PublicScope<'a>> {
    if matches!(scope, Scope::Rule) {
        return vec![(
            None,
            public_scope_actions(&rule.actions, 0, rule.actions.len()),
            rule.conditions
                .iter()
                .filter(|condition| !condition.disabled)
                .map(|condition| &condition.value)
                .collect(),
            program.rule_span(rule_id),
        )];
    }

    let mut result = Vec::new();
    for (index, action) in rule.actions.iter().enumerate() {
        let matches_scope = matches!(
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
        );
        if !matches_scope {
            continue;
        }
        let Some(close) = public_matching_end(&rule.actions, index) else {
            continue;
        };
        let values = match action {
            PublicAction::While { condition } => vec![condition],
            PublicAction::If { condition } => {
                public_if_conditions(&rule.actions, index, close, condition)
            }
            _ => Vec::new(),
        };
        result.push((
            Some(index),
            public_scope_actions(&rule.actions, index + 1, close),
            values,
            program.action_span(rule_id, index),
        ));
    }
    result
}

fn public_scope_actions(actions: &[PublicAction], start: usize, end: usize) -> Vec<&PublicAction> {
    let mut result = Vec::new();
    let mut index = start;
    while index < end {
        match &actions[index] {
            PublicAction::Else | PublicAction::ElseIf { .. } | PublicAction::End => index += 1,
            action => {
                result.push(action);
                index = public_matching_end(actions, index).map_or(index + 1, |close| close + 1);
            }
        }
    }
    result
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
                if let Some(nested_close) = public_matching_end(actions, index) {
                    index = nested_close + 1;
                    continue;
                }
            }
        }
        index += 1;
    }
    conditions
}

fn public_matching_end(actions: &[PublicAction], start: usize) -> Option<usize> {
    if !matches!(
        actions.get(start),
        Some(
            PublicAction::If { .. }
                | PublicAction::While { .. }
                | PublicAction::ForGlobalVariable { .. }
                | PublicAction::ForPlayerVariable { .. }
        )
    ) {
        return None;
    }
    let mut depth = 0;
    for (index, action) in actions.iter().enumerate().skip(start + 1) {
        match action {
            PublicAction::If { .. }
            | PublicAction::While { .. }
            | PublicAction::ForGlobalVariable { .. }
            | PublicAction::ForPlayerVariable { .. } => depth += 1,
            PublicAction::End if depth == 0 => return Some(index),
            PublicAction::End => depth -= 1,
            _ => {}
        }
    }
    None
}

fn public_action_matches(action: &PublicAction, pattern: &CanonicalActionPattern) -> bool {
    let kind_matches = match pattern.kind {
        None | Some(ActionKind::Any) => true,
        Some(ActionKind::Call) => matches!(action, PublicAction::Call { .. }),
        Some(ActionKind::While) => matches!(action, PublicAction::While { .. }),
        Some(ActionKind::ForGlobalVariable) => {
            matches!(action, PublicAction::ForGlobalVariable { .. })
        }
        Some(ActionKind::ForPlayerVariable) => {
            matches!(action, PublicAction::ForPlayerVariable { .. })
        }
        Some(ActionKind::If) => matches!(action, PublicAction::If { .. }),
    };
    if !kind_matches {
        return false;
    }
    let PublicAction::Call { name, args } = action else {
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
            .is_some_and(|actual| public_value_matches(actual, expected))
    }) && pattern.parameters.iter().all(|(index, expected)| {
        args.get(*index)
            .is_some_and(|actual| public_value_matches(actual, expected))
    })
}

fn public_value_matches(value: &PublicValue, pattern: &CanonicalValuePattern) -> bool {
    match (pattern, value) {
        (CanonicalValuePattern::Any, _) => true,
        (CanonicalValuePattern::Number(expected), PublicValue::Number(actual)) => {
            expected == actual
        }
        (CanonicalValuePattern::String(expected), PublicValue::String(actual))
        | (CanonicalValuePattern::String(expected), PublicValue::LocalizedString(actual)) => {
            expected == actual
        }
        (CanonicalValuePattern::Boolean(expected), PublicValue::Bool(actual)) => expected == actual,
        (
            CanonicalValuePattern::Enum { domain, member },
            PublicValue::Enum { value_type, value },
        ) => domain == value_type && member == value,
        (
            CanonicalValuePattern::Call { name, args },
            PublicValue::Call {
                name: actual,
                args: actual_args,
            },
        ) => {
            name == actual
                && args.len() <= actual_args.len()
                && args
                    .iter()
                    .enumerate()
                    .all(|(index, expected)| public_value_matches(&actual_args[index], expected))
        }
        (CanonicalValuePattern::Comparison { operator, value }, PublicValue::Number(actual)) => {
            let CanonicalValuePattern::Number(expected) = value.as_ref() else {
                return false;
            };
            match operator {
                ComparisonOperator::Less => *actual < *expected,
                ComparisonOperator::LessOrEqual => *actual <= *expected,
                ComparisonOperator::Greater => *actual > *expected,
                ComparisonOperator::GreaterOrEqual => *actual >= *expected,
            }
        }
        _ => false,
    }
}

#[allow(unreachable_patterns)]
fn public_event_id(event: &workshop_rs::Event) -> &str {
    match event {
        workshop_rs::Event::Global => "global",
        workshop_rs::Event::EachPlayer | workshop_rs::Event::EachPlayerWithFilters { .. } => {
            "eachPlayer"
        }
        workshop_rs::Event::Player { kind, .. } => match kind {
            workshop_rs::PlayerEventKind::DealtDamage => "playerDealtDamage",
            workshop_rs::PlayerEventKind::DealtFinalBlow => "playerDealtFinalBlow",
            workshop_rs::PlayerEventKind::DealtHealing => "playerDealtHealing",
            workshop_rs::PlayerEventKind::DealtKnockback => "playerDealtKnockback",
            workshop_rs::PlayerEventKind::Died => "playerDied",
            workshop_rs::PlayerEventKind::EarnedElimination => "playerEarnedElimination",
            workshop_rs::PlayerEventKind::Joined => "playerJoined",
            workshop_rs::PlayerEventKind::Left => "playerLeft",
            workshop_rs::PlayerEventKind::ReceivedHealing => "playerReceivedHealing",
            workshop_rs::PlayerEventKind::ReceivedKnockback => "playerReceivedKnockback",
            workshop_rs::PlayerEventKind::TookDamage => "playerTookDamage",
            _ => "unknown",
        },
        workshop_rs::Event::Subroutine(_) => "subroutine",
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
    let action_entry = name
        .as_deref()
        .and_then(|id| catalog.entry(Kind::Action, id));
    let mut parameters = Vec::new();
    for param in &pattern.parameters {
        let Some(entry) = action_entry else {
            return Err(RuleError::NamedParameterRequiresAction);
        };
        let Some(index) = resolve_parameter(entry, locale, &param.name) else {
            return Err(RuleError::UnknownParameter {
                action: entry.id.clone(),
                parameter: param.name.clone(),
                locale: locale.to_string(),
            });
        };
        parameters.push((index, canonical_value(&param.value, catalog, locale)?));
    }
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
    action: &workshop_rs::catalog::CatalogEntry,
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
    Yaml(yaml_serde::Error),
    InvalidIdentity(String),
    UnsupportedLocale(String),
    UnknownSpelling {
        kind: &'static str,
        spelling: String,
        locale: String,
    },
    UnknownParameter {
        action: String,
        parameter: String,
        locale: String,
    },
    NamedParameterRequiresAction,
    AmbiguousValuePattern,
    ComparisonNeedsNumber,
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleError::Yaml(err) => write!(f, "invalid rule YAML: {err}"),
            RuleError::InvalidIdentity(id) => write!(
                f,
                "invalid rule ID '{id}': must be '<namespace>/<id>' and '<namespace>' cannot be 'wright'"
            ),
            RuleError::UnsupportedLocale(locale) => {
                write!(f, "unsupported catalog locale: {locale}")
            }
            RuleError::UnknownSpelling {
                kind,
                spelling,
                locale,
            } => write!(f, "unknown {kind} '{spelling}' for locale {locale}"),
            RuleError::UnknownParameter {
                action,
                parameter,
                locale,
            } => write!(
                f,
                "unknown parameter '{parameter}' for action '{action}' in locale {locale}"
            ),
            RuleError::NamedParameterRequiresAction => {
                write!(f, "parameter matchers require an exact action name")
            }
            RuleError::AmbiguousValuePattern => {
                write!(f, "value pattern specifies more than one pattern kind")
            }
            RuleError::ComparisonNeedsNumber => {
                write!(f, "comparison pattern value must be a number pattern")
            }
        }
    }
}

impl std::error::Error for RuleError {}
