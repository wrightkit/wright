use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use workshop_rs::catalog::Catalog;
use workshop_rs::wir;

use crate::analysis::{EvidenceClass, Finding, Severity};
use crate::declarative::{DeclarativeRule, RuleDefinition, RuleError};

#[derive(Debug, Clone)]
pub struct RuleMeta {
    pub id: &'static str,
    pub default_severity: Severity,
    pub evidence: EvidenceClass,
    pub summary: &'static str,
    pub rationale: &'static str,
    pub documentation: &'static str,
    pub known_limits: &'static str,
    pub tags: &'static [&'static str],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(
        rename = "severity",
        alias = "severity_override",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub severity_override: Option<SeverityLabel>,
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

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleOptions {
    #[serde(
        rename = "min-matches",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub min_matches: Option<usize>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SeverityLabel {
    Off,
    Warn,
    Warning,
    Error,
    Info,
}

impl SeverityLabel {
    pub fn severity(self) -> Option<Severity> {
        match self {
            Self::Off => None,
            Self::Warn | Self::Warning => Some(Severity::Warning),
            Self::Error => Some(Severity::Error),
            Self::Info => Some(Severity::Info),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LintConfig {
    #[serde(default)]
    pub rules: HashMap<String, RuleConfig>,
}

impl LintConfig {
    pub fn from_yaml_str(yaml: &str) -> Result<Self, yaml_serde::Error> {
        yaml_serde::from_str(yaml)
    }

    pub fn from_yaml_path(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read lint config {}: {e}", path.display()))?;
        Self::from_yaml_str(&content).map_err(|e| e.to_string())
    }

    pub fn disable(&mut self, rule_id: &str) {
        self.rules.entry(rule_id.to_string()).or_default().enabled = false;
    }

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

    pub fn is_enabled(&self, rule_id: &str) -> bool {
        self.rules.get(rule_id).is_none_or(|config| {
            config.enabled && config.severity_override != Some(SeverityLabel::Off)
        })
    }

    pub fn effective_severity(&self, meta: &RuleMeta) -> Severity {
        self.rules
            .get(meta.id)
            .and_then(|config| config.severity_override)
            .and_then(SeverityLabel::severity)
            .unwrap_or(meta.default_severity)
    }

    pub fn effective_severity_for(&self, id: &str, default: Severity) -> Severity {
        self.rules
            .get(id)
            .and_then(|config| config.severity_override)
            .and_then(SeverityLabel::severity)
            .unwrap_or(default)
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

#[derive(Debug, Clone, Serialize)]
pub struct RuleDescriptor {
    pub id: String,
    pub default_severity: Severity,
    pub effective_severity: Severity,
    pub enabled: bool,
    pub summary: String,
    pub rationale: String,
    pub documentation: String,
    pub known_limits: String,
    pub evidence: EvidenceClass,
    pub tags: Vec<String>,
    pub kind: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkippedRule {
    pub id: String,
    pub rule: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct LintRun {
    pub findings: Vec<Finding>,
    pub skipped: Vec<SkippedRule>,
}

struct RegistryEntry {
    meta: Option<RuleMeta>,
    declarative: Option<DeclarativeRule>,
}

pub struct LintRegistry {
    entries: Vec<RegistryEntry>,
}

impl Default for LintRegistry {
    fn default() -> Self {
        let entries = vec![
            RegistryEntry {
                meta: Some(RuleMeta {
                    id: "min-wait-loop",
                    default_severity: Severity::Warning,
                    evidence: EvidenceClass::StaticIndicator,
                    summary: "loop body waits at the workshop minimum rate",
                    rationale: "Avoid sustained maximum-frequency loop execution.",
                    documentation: "A loop whose body waits at ~0.016s runs at maximum server frequency.",
                    known_limits: "Wait durations computed at runtime do not trigger this rule.",
                    tags: &["performance", "stability"],
                }),
                declarative: None,
            },
            RegistryEntry {
                meta: Some(RuleMeta {
                    id: "duplicate-condition",
                    default_severity: Severity::Warning,
                    evidence: EvidenceClass::Exact,
                    summary: "condition is evaluated more than once within one rule",
                    rationale: "Avoid unreachable or redundant conditional branches.",
                    documentation: "The same condition appears in two or more branches of the same rule.",
                    known_limits: "Detection is structural and rule-local.",
                    tags: &["correctness"],
                }),
                declarative: None,
            },
            RegistryEntry {
                meta: Some(RuleMeta {
                    id: "expensive-loop-check",
                    default_severity: Severity::Info,
                    evidence: EvidenceClass::Heuristic,
                    summary: "geometry predicate evaluated inside a loop body",
                    rationale: "Surface expensive per-iteration geometry work.",
                    documentation: "A geometry predicate is called inside a loop body.",
                    known_limits: "The expensive-call list is a fixed heuristic.",
                    tags: &["performance"],
                }),
                declarative: None,
            },
            RegistryEntry {
                meta: Some(RuleMeta {
                    id: "ongoing-condition-hot-path",
                    default_severity: Severity::Info,
                    evidence: EvidenceClass::Heuristic,
                    summary: "geometry predicate evaluated in an ongoing-rule condition",
                    rationale: "Make high-frequency condition evaluation visible.",
                    documentation: "An ongoing rule evaluates a geometry predicate in its condition.",
                    known_limits: "The expensive-call list is a fixed heuristic.",
                    tags: &["performance", "stability"],
                }),
                declarative: None,
            },
            RegistryEntry {
                meta: Some(RuleMeta {
                    id: "repeated-value",
                    default_severity: Severity::Warning,
                    evidence: EvidenceClass::Exact,
                    summary: "identical value expression evaluated more than once in one loop scope",
                    rationale: "Surface duplicated value computation in loop iterations.",
                    documentation: "The same value expression is evaluated multiple times in a loop.",
                    known_limits: "Subtrees containing fewer than two calls are ignored.",
                    tags: &["performance"],
                }),
                declarative: None,
            },
            RegistryEntry {
                meta: Some(RuleMeta {
                    id: "while-without-wait",
                    default_severity: Severity::Warning,
                    evidence: EvidenceClass::StaticIndicator,
                    summary: "while loop whose body contains no wait action",
                    rationale: "Surface loops that can freeze or crash the server under load.",
                    documentation: "A while loop whose body contains no wait executes without yielding.",
                    known_limits: "Requires conservative counter proof for statically-bounded classification.",
                    tags: &["stability"],
                }),
                declarative: None,
            },
        ];
        Self { entries }
    }
}

impl LintRegistry {
    pub fn rules(&self) -> impl Iterator<Item = &RuleMeta> {
        self.entries.iter().filter_map(|e| e.meta.as_ref())
    }

    pub fn load_yaml_str(&mut self, input: &str) -> Result<(), RuleRegistryError> {
        let catalog = Catalog::builtin().map_err(|e| RuleRegistryError::Catalog(e.to_string()))?;
        let definition = RuleDefinition::from_yaml_str(input).map_err(RuleRegistryError::Rule)?;
        let rule = DeclarativeRule::from_definition(definition, &catalog).map_err(RuleRegistryError::Rule)?;
        self.insert_declarative(rule)
    }

    pub fn load_path(&mut self, path: &Path) -> Result<(), RuleRegistryError> {
        let catalog = Catalog::builtin().map_err(|e| RuleRegistryError::Catalog(e.to_string()))?;
        self.load_path_with_catalog(path, &catalog)
    }

    fn load_path_with_catalog(&mut self, path: &Path, catalog: &Catalog) -> Result<(), RuleRegistryError> {
        if path.is_dir() {
            let mut files = std::fs::read_dir(path)
                .map_err(|e| RuleRegistryError::Io(path.to_path_buf(), e))?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("yaml" | "yml")))
                .collect::<Vec<_>>();
            files.sort();
            for file in files {
                self.load_path_with_catalog(&file, catalog)?;
            }
            return Ok(());
        }
        let input = std::fs::read_to_string(path)
            .map_err(|e| RuleRegistryError::Io(path.to_path_buf(), e))?;
        let definition = RuleDefinition::from_yaml_str(&input).map_err(RuleRegistryError::Rule)?;
        let rule = DeclarativeRule::from_definition(definition, catalog).map_err(RuleRegistryError::Rule)?;
        self.insert_declarative(rule)
    }

    fn insert_declarative(&mut self, rule: DeclarativeRule) -> Result<(), RuleRegistryError> {
        let id = rule.id().to_string();
        if self.entries.iter().any(|e| {
            e.meta.as_ref().is_some_and(|m| m.id == id)
                || e.declarative.as_ref().is_some_and(|r| r.id() == id)
        }) {
            return Err(RuleRegistryError::DuplicateId(id));
        }
        self.entries.push(RegistryEntry {
            meta: None,
            declarative: Some(rule),
        });
        Ok(())
    }

    pub fn descriptors(&self, config: &LintConfig) -> Vec<RuleDescriptor> {
        self.entries
            .iter()
            .map(|entry| {
                if let Some(meta) = &entry.meta {
                    RuleDescriptor {
                        id: meta.id.to_string(),
                        default_severity: meta.default_severity,
                        effective_severity: config.effective_severity(meta),
                        enabled: config.is_enabled(meta.id),
                        summary: meta.summary.to_string(),
                        rationale: meta.rationale.to_string(),
                        documentation: meta.documentation.to_string(),
                        known_limits: meta.known_limits.to_string(),
                        evidence: meta.evidence,
                        tags: meta.tags.iter().map(|t| (*t).to_string()).collect(),
                        kind: "native",
                    }
                } else {
                    let rule = entry.declarative.as_ref().expect("has rule");
                    let meta = rule.metadata();
                    RuleDescriptor {
                        id: rule.id().to_string(),
                        default_severity: rule.default_severity(),
                        effective_severity: config.effective_severity_for(rule.id(), rule.default_severity()),
                        enabled: config.is_enabled(rule.id()),
                        summary: meta.summary.clone(),
                        rationale: meta.rationale.clone(),
                        documentation: meta.documentation.clone(),
                        known_limits: meta.known_limits.clone(),
                        evidence: rule.evidence(),
                        tags: meta.tags.clone(),
                        kind: "declarative",
                    }
                }
            })
            .collect()
    }

    pub fn run_canonical_custom(
        &self,
        program: &workshop_rs::Program,
        config: &LintConfig,
    ) -> Vec<Finding> {
        let mut findings = Vec::new();
        for rule in 0..program.rules.len() {
            for entry in &self.entries {
                let Some(declarative) = &entry.declarative else {
                    continue;
                };
                if !config.is_enabled(declarative.id()) {
                    continue;
                }
                let mut rule_findings = declarative.run_canonical(
                    program,
                    rule,
                    config.options(declarative.id()).min_matches,
                    config.options(declarative.id()).max_matches,
                );
                let severity =
                    config.effective_severity_for(declarative.id(), declarative.default_severity());
                for finding in &mut rule_findings {
                    finding.severity = severity;
                }
                findings.extend(rule_findings);
            }
        }
        findings
    }

    pub fn run(&self, program: &wir::Program, config: &LintConfig) -> Vec<Finding> {
        self.run_report(program, config).findings
    }

    pub fn run_report(&self, _program: &wir::Program, _config: &LintConfig) -> LintRun {
        LintRun::default()
    }
}

#[derive(Debug)]
pub enum RuleRegistryError {
    Io(std::path::PathBuf, std::io::Error),
    Catalog(String),
    Rule(RuleError),
    DuplicateId(String),
}

impl std::fmt::Display for RuleRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(path, error) => write!(f, "cannot read rule source {}: {error}", path.display()),
            Self::Catalog(error) => write!(f, "cannot load Workshop catalog: {error}"),
            Self::Rule(error) => error.fmt(f),
            Self::DuplicateId(id) => write!(f, "rule ID '{id}' is already registered"),
        }
    }
}

impl std::error::Error for RuleRegistryError {}
