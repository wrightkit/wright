//! The YAML schema of a declarative lint rule (SPEC-309, #970): the
//! `Deserialize` forms accepted in rule files, before catalog resolution
//! turns spellings into canonical identities (see `super::pattern`).

use serde::Deserialize;

use crate::analysis::{EvidenceClass, Severity};

use super::RuleError;

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
    /// First-party packages only: the declared evidence class of this rule's
    /// findings (#616 addendum, #970). Rejected for external rules.
    #[serde(default)]
    pub evidence: Option<EvidenceClass>,
    /// First-party packages only: the declared default severity (#616
    /// addendum, #970). Rejected for external rules; project configuration
    /// still owns effective severity overrides.
    #[serde(default)]
    pub severity: Option<Severity>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleMatcher {
    pub event: Option<String>,
    /// One scope or a list of scopes the matcher enumerates (#970).
    #[serde(default)]
    pub scope: Scopes,
    pub conditions: Option<NodePattern>,
    #[serde(default)]
    pub actions: Vec<ActionPattern>,
    /// Value patterns searched at any depth inside every value tree of the
    /// scope (#970): rule conditions and live action arguments for `rule`,
    /// live body-action arguments for a loop scope, and the condition list
    /// for `if`.
    #[serde(default)]
    pub values: Vec<ValuePattern>,
    /// Finding anchoring: `scope` (default) reports once per scope at the
    /// scope anchor; `each-match` reports once per matched value node at the
    /// node's own span (#970).
    #[serde(default)]
    pub report: ReportMode,
}

/// `matcher.scope` accepts a single scope or a list of scopes (#970).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Scopes {
    One(Scope),
    Many(Vec<Scope>),
}

impl Default for Scopes {
    fn default() -> Self {
        Self::One(Scope::Rule)
    }
}

impl Scopes {
    pub(crate) fn list(&self) -> &[Scope] {
        match self {
            Self::One(scope) => std::slice::from_ref(scope),
            Self::Many(scopes) => scopes,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReportMode {
    #[default]
    Scope,
    EachMatch,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Count {
    pub min: Option<usize>,
    pub max: Option<usize>,
}

impl Count {
    pub(crate) fn accepts(&self, count: usize) -> bool {
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
    /// An optional ancestor constraint, valid only on a `values` entry
    /// (#970): a value pattern that must match a strict ancestor of the
    /// matched node.
    #[serde(default)]
    pub within: Option<Box<WithinPattern>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WithinPattern {
    #[serde(flatten)]
    pub value: ValuePattern,
    /// Restricts which ancestor argument positions carry the constraint: only
    /// an ancestor edge at a child index `>= from` qualifies, so a binder's
    /// array-source argument at index 0 stays outside (#970).
    #[serde(rename = "in-args")]
    pub in_args: Option<InArgs>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InArgs {
    pub from: usize,
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
    /// Exactly one of `name` (one canonical or localized spelling) or `names`
    /// (a set matched as alternatives) is required (#970).
    pub name: Option<String>,
    pub names: Option<Vec<String>>,
    #[serde(default)]
    pub args: Vec<ValuePattern>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnumPattern {
    pub domain: String,
    pub member: String,
}

impl RuleDefinition {
    pub fn from_yaml_str(input: &str) -> Result<Self, RuleError> {
        yaml_serde::from_str(input).map_err(RuleError::Yaml)
    }
}

fn default_locale() -> String {
    DEFAULT_LOCALE.to_string()
}

pub(crate) fn default_true() -> bool {
    true
}
