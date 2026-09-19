use std::fmt;

use serde::Deserialize;
use workshop_rs::catalog::{Catalog, Kind, Locale};
use workshop_rs::source::Span;
use workshop_rs::{Action, Event, PlayerEventKind, Program, Rule, Value};

use crate::analysis::{EvidenceClass, Finding, Severity};

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

    pub fn evidence(&self) -> EvidenceClass {
        EvidenceClass::Exact
    }

    pub fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    pub fn run(
        &self,
        program: &Program,
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
            .is_some_and(|e| e != event_id(&rule_data.event))
        {
            return Vec::new();
        }
        let mut findings = Vec::new();
        for (scope_id, actions, values, anchor) in rule_scopes(rule_data) {
            if let Some(p) = &self.conditions {
                let count = values.iter().filter(|v| value_matches(v, &p.value)).count();
                if !p.count.accepts(count) {
                    continue;
                }
            }
            let mut counts = Vec::new();
            if self.actions.iter().any(|p| {
                let count = actions.iter().filter(|a| action_matches(a, p)).count();
                let accepted = if p.present {
                    p.count.accepts(count)
                } else {
                    count == 0 && p.count.accepts(0)
                };
                counts.push(count);
                !accepted
            }) {
                continue;
            }
            let matched = counts.into_iter().max().unwrap_or(1);
            if min_matches.is_some_and(|m| matched < m) || max_matches.is_some_and(|m| matched > m)
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

    pub fn run_canonical(
        &self,
        program: &Program,
        rule: usize,
        min_matches: Option<usize>,
        max_matches: Option<usize>,
    ) -> Vec<Finding> {
        self.run(program, rule, min_matches, max_matches)
    }
}

type ScopeData<'a> = (Option<usize>, Vec<&'a Action>, Vec<&'a Value>, Option<Span>);

fn rule_scopes(rule: &Rule) -> Vec<ScopeData<'_>> {
    let conds: Vec<&Value> = rule.conditions.iter().map(|c| &c.value).collect();
    match rule.actions.iter().enumerate().find(|(_, a)| {
        matches!(
            a,
            Action::While { .. }
                | Action::ForGlobalVariable { .. }
                | Action::ForPlayerVariable { .. }
        )
    }) {
        Some((index, _)) => {
            let mut depth = 0;
            let end = rule.actions[index + 1..]
                .iter()
                .enumerate()
                .find_map(|(offset, action)| match action {
                    Action::If { .. }
                    | Action::While { .. }
                    | Action::ForGlobalVariable { .. }
                    | Action::ForPlayerVariable { .. } => {
                        depth += 1;
                        None
                    }
                    Action::End if depth == 0 => Some(index + 1 + offset),
                    Action::End => {
                        depth -= 1;
                        None
                    }
                    _ => None,
                })
                .unwrap_or(rule.actions.len());
            vec![(
                Some(index),
                rule.actions[index + 1..end].iter().collect(),
                conds,
                None,
            )]
        }
        None => vec![(None, rule.actions.iter().collect(), conds, None)],
    }
}

fn action_matches(action: &Action, pattern: &CanonicalActionPattern) -> bool {
    let kind_matches = match pattern.kind {
        None | Some(ActionKind::Any) => true,
        Some(ActionKind::Call) => matches!(action, Action::Call { .. }),
        Some(ActionKind::While) => matches!(action, Action::While { .. }),
        Some(ActionKind::ForGlobalVariable) => matches!(action, Action::ForGlobalVariable { .. }),
        Some(ActionKind::ForPlayerVariable) => matches!(action, Action::ForPlayerVariable { .. }),
        Some(ActionKind::If) => matches!(action, Action::If { .. }),
    };
    if !kind_matches {
        return false;
    }
    let Action::Call { name, args } = action else {
        return pattern.name.is_none() && pattern.args.is_empty();
    };
    if pattern
        .name
        .as_deref()
        .is_some_and(|expected| expected != name)
    {
        return false;
    }
    pattern
        .args
        .iter()
        .enumerate()
        .all(|(i, exp)| args.get(i).is_some_and(|act| value_matches(act, exp)))
        && pattern
            .parameters
            .iter()
            .all(|(i, exp)| args.get(*i).is_some_and(|act| value_matches(act, exp)))
}

fn value_matches(value: &Value, pattern: &CanonicalValuePattern) -> bool {
    match (pattern, value) {
        (CanonicalValuePattern::Any, _) => true,
        (CanonicalValuePattern::Number(expected), Value::Number(actual)) => expected == actual,
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
                    .all(|(i, exp)| value_matches(&actual_args[i], exp))
        }
        (CanonicalValuePattern::Comparison { operator, value }, Value::Number(actual)) => {
            match value.as_ref() {
                CanonicalValuePattern::Number(expected) => match operator {
                    ComparisonOperator::Less => actual < expected,
                    ComparisonOperator::LessOrEqual => actual <= expected,
                    ComparisonOperator::Greater => actual > expected,
                    ComparisonOperator::GreaterOrEqual => actual >= expected,
                },
                _ => false,
            }
        }
        _ => false,
    }
}

fn event_id(event: &Event) -> &str {
    match event {
        Event::Global => "global",
        Event::EachPlayer | Event::EachPlayerWithFilters { .. } => "eachPlayer",
        Event::Subroutine(_) => "subroutine",
        Event::Player { kind, .. } => match kind {
            PlayerEventKind::DealtDamage => "playerDealtDamage",
            PlayerEventKind::DealtFinalBlow => "playerDealtFinalBlow",
            PlayerEventKind::DealtHealing => "playerDealtHealing",
            PlayerEventKind::DealtKnockback => "playerDealtKnockback",
            PlayerEventKind::Died => "playerDied",
            PlayerEventKind::EarnedElimination => "playerEarnedElimination",
            PlayerEventKind::Joined => "playerJoined",
            PlayerEventKind::Left => "playerLeft",
            PlayerEventKind::ReceivedHealing => "playerReceivedHealing",
            PlayerEventKind::ReceivedKnockback => "playerReceivedKnockback",
            PlayerEventKind::TookDamage => "playerTookDamage",
        },
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
            let index = action
                .resolve_param(locale, &parameter.name)
                .ok_or_else(|| RuleError::UnknownParameter {
                    action: action.id.clone(),
                    spelling: parameter.name.clone(),
                    locale: locale.to_string(),
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

fn canonical_value(
    pattern: &ValuePattern,
    catalog: &Catalog,
    locale: &Locale,
) -> Result<CanonicalValuePattern, RuleError> {
    let selected = pattern.number.is_some() as usize
        + pattern.string.is_some() as usize
        + pattern.boolean.is_some() as usize
        + pattern.call.is_some() as usize
        + pattern.enum_value.is_some() as usize
        + pattern.comparison.is_some() as usize;
    if selected > 1 {
        return Err(RuleError::AmbiguousValuePattern);
    }
    if let Some(number) = pattern.number {
        Ok(CanonicalValuePattern::Number(number))
    } else if let Some(string) = &pattern.string {
        Ok(CanonicalValuePattern::String(string.clone()))
    } else if let Some(boolean) = pattern.boolean {
        Ok(CanonicalValuePattern::Boolean(boolean))
    } else if let Some(call) = &pattern.call {
        let args = call
            .args
            .iter()
            .map(|value| canonical_value(value, catalog, locale))
            .collect::<Result<_, _>>()?;
        Ok(CanonicalValuePattern::Call {
            name: resolve_value_or_operator(catalog, locale, &call.name)?,
            args,
        })
    } else if let Some(en) = &pattern.enum_value {
        let domain = resolve_enum_domain(catalog, locale, &en.domain)?;
        let member = catalog
            .resolve_enum_member(&domain, locale, &en.member)
            .ok_or_else(|| RuleError::UnknownSpelling {
                kind: "enum member",
                spelling: en.member.clone(),
                locale: locale.to_string(),
            })?
            .1;
        Ok(CanonicalValuePattern::Enum { domain, member })
    } else if let Some(comparison) = &pattern.comparison {
        let value = canonical_value(&comparison.value, catalog, locale)?;
        if !matches!(value, CanonicalValuePattern::Number(_)) {
            return Err(RuleError::ComparisonNeedsNumber);
        }
        Ok(CanonicalValuePattern::Comparison {
            operator: comparison.operator,
            value: Box::new(value),
        })
    } else {
        Ok(CanonicalValuePattern::Any)
    }
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
        .or_else(|| catalog.resolve(kind, locale, spelling))
        .map(|entry| entry.id.clone())
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
    match id.split_once('/') {
        Some((ns, rid))
            if !ns.is_empty() && !rid.is_empty() && ns != "wright" && !rid.contains('/') =>
        {
            Ok(())
        }
        _ => Err(RuleError::InvalidIdentity(id.to_string())),
    }
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
            Self::Yaml(err) => write!(f, "invalid rule YAML: {err}"),
            Self::InvalidIdentity(id) => write!(
                f,
                "external rule ID '{id}' must be <namespace>/<rule-id>; namespace 'wright' is reserved"
            ),
            Self::UnsupportedLocale(loc) => {
                write!(
                    f,
                    "rule locale '{loc}' is not supported by the Workshop catalog"
                )
            }
            Self::UnknownSpelling {
                kind,
                spelling,
                locale,
            } => {
                write!(
                    f,
                    "unknown {kind} spelling '{spelling}' in locale '{locale}'"
                )
            }
            Self::UnknownParameter {
                action,
                spelling,
                locale,
            } => write!(
                f,
                "unknown parameter '{spelling}' for action '{action}' in locale '{locale}'"
            ),
            Self::ParameterNeedsActionName(p) => {
                write!(f, "parameter '{p}' requires an action name")
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
