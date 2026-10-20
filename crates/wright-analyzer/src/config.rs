//! Lint run configuration: the YAML contract a project lint file and the
//! programmatic `SessionConfig` share, plus the `packages:` package pins
//! (issue #843).

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::analysis::Severity;
use crate::declarative::RuleError;
use crate::registry::RuleMeta;

/// Configuration applied to one rule at registry execution time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleConfig {
    /// When `false` the rule is skipped entirely and produces no findings.
    #[serde(default = "crate::declarative::default_true")]
    pub enabled: bool,
    /// When `Some`, replaces the rule's [`RuleMeta::default_severity`] in
    /// every finding produced during this run.
    #[serde(
        rename = "severity",
        alias = "severity_override",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub severity_override: Option<SeverityLabel>,
    /// Bounded rule-specific match-count options.
    #[serde(default, skip_serializing_if = "RuleOptions::is_empty")]
    pub options: RuleOptions,
}

impl Default for RuleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            severity_override: None,
            options: RuleOptions::default(),
        }
    }
}

/// The intentionally small option surface shared by declarative rules.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleOptions {
    /// Require at least this many matched nodes.
    #[serde(
        rename = "min-matches",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub min_matches: Option<usize>,
    /// Require at most this many matched nodes.
    #[serde(
        rename = "max-matches",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub max_matches: Option<usize>,
}

impl RuleOptions {
    fn is_empty(&self) -> bool {
        self.min_matches.is_none() && self.max_matches.is_none()
    }
}

/// A serialization-friendly severity label for use in configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SeverityLabel {
    Off,
    Warn,
    Error,
    Info,
}

impl SeverityLabel {
    pub fn severity(self) -> Option<Severity> {
        match self {
            SeverityLabel::Off => None,
            SeverityLabel::Warn => Some(Severity::Warning),
            SeverityLabel::Error => Some(Severity::Error),
            SeverityLabel::Info => Some(Severity::Info),
        }
    }
}

impl From<Severity> for SeverityLabel {
    fn from(severity: Severity) -> Self {
        match severity {
            Severity::Warning => SeverityLabel::Warn,
            Severity::Error => SeverityLabel::Error,
            Severity::Info => SeverityLabel::Info,
        }
    }
}

/// One `packages:` entry — a lint rule package pinned to one repository
/// commit (issue #843).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LintPackagePin {
    /// The GitHub repository — `owner/repo` or a github.com repository URL.
    pub repo: String,
    /// The pinned full commit SHA.
    pub sha: String,
}

/// The effective configuration for a lint run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LintConfig {
    /// Lint rule packages pinned by repository and full commit SHA
    /// (issue #843): the configuration itself is the lock.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<LintPackagePin>,
    /// Per-rule configuration overrides keyed by stable rule ID.
    #[serde(default)]
    pub rules: HashMap<String, RuleConfig>,
}

impl LintConfig {
    pub fn from_yaml_path(path: &Path) -> Result<Self, std::io::Error> {
        let input = std::fs::read_to_string(path)?;
        yaml_serde::from_str(&input).map_err(std::io::Error::other)
    }

    pub fn from_yaml_str(input: &str) -> Result<Self, RuleError> {
        yaml_serde::from_str(input).map_err(RuleError::Yaml)
    }

    /// Disable a rule by its stable ID.
    pub fn disable(&mut self, rule_id: &str) {
        self.rules.entry(rule_id.to_string()).or_default().enabled = false;
    }

    /// Enable a rule by its stable ID.
    pub fn enable(&mut self, rule_id: &str) {
        self.rules.entry(rule_id.to_string()).or_default().enabled = true;
    }

    /// Override the severity for a rule by its stable ID.
    pub fn set_severity(&mut self, rule_id: &str, severity: Severity) {
        self.rules
            .entry(rule_id.to_string())
            .or_default()
            .severity_override = Some(severity.into());
    }

    /// Override the severity of a rule from its CLI spelling.
    pub fn set_severity_by_name(&mut self, rule_id: &str, severity: &str) -> bool {
        let label = match severity {
            "off" => SeverityLabel::Off,
            "warn" => SeverityLabel::Warn,
            "error" => SeverityLabel::Error,
            _ => return false,
        };
        self.rules
            .entry(rule_id.to_string())
            .or_default()
            .severity_override = Some(label);
        true
    }

    /// Whether a rule is enabled.
    pub fn is_enabled(&self, rule_id: &str) -> bool {
        self.rules.get(rule_id).is_none_or(|config| {
            config.enabled && config.severity_override != Some(SeverityLabel::Off)
        })
    }

    /// Effective severity for a rule given its metadata and this config.
    pub fn effective_severity(&self, meta: &RuleMeta) -> Severity {
        self.effective_severity_for(meta.id, meta.default_severity)
    }

    pub fn effective_severity_for(&self, id: &str, default: Severity) -> Severity {
        self.severity_override(id).unwrap_or(default)
    }

    pub fn severity_override(&self, id: &str) -> Option<Severity> {
        self.rules
            .get(id)
            .and_then(|config| config.severity_override)
            .and_then(SeverityLabel::severity)
    }

    pub fn options(&self, id: &str) -> &RuleOptions {
        self.rules
            .get(id)
            .map_or(&EMPTY_OPTIONS, |config| &config.options)
    }
}

static EMPTY_OPTIONS: RuleOptions = RuleOptions {
    min_matches: None,
    max_matches: None,
};
