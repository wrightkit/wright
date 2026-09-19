use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use workshop_rs::Program;
use workshop_rs::catalog::Catalog;

use crate::analysis::{EvidenceClass, Finding, Severity, analyze};
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
    Error,
}

impl SeverityLabel {
    fn severity(self) -> Option<Severity> {
        match self {
            Self::Off => None,
            Self::Warn => Some(Severity::Warning),
            Self::Error => Some(Severity::Error),
        }
    }
}

impl From<Severity> for SeverityLabel {
    fn from(severity: Severity) -> Self {
        match severity {
            Severity::Warning | Severity::Info => SeverityLabel::Warn,
            Severity::Error => SeverityLabel::Error,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LintConfig {
    #[serde(default)]
    rules: HashMap<String, RuleConfig>,
}

impl LintConfig {
    pub fn from_yaml_str(input: &str) -> Result<Self, yaml_serde::Error> {
        yaml_serde::from_str(input)
    }

    pub fn from_yaml_path(path: &Path) -> Result<Self, std::io::Error> {
        let input = std::fs::read_to_string(path)?;
        yaml_serde::from_str(&input).map_err(std::io::Error::other)
    }

    pub fn disable(&mut self, rule_id: &str) {
        self.rules.entry(rule_id.to_string()).or_default().enabled = false;
    }

    pub fn enable(&mut self, rule_id: &str) {
        self.rules.entry(rule_id.to_string()).or_default().enabled = true;
    }

    pub fn set_severity(&mut self, rule_id: &str, severity: Severity) {
        self.rules
            .entry(rule_id.to_string())
            .or_default()
            .severity_override = Some(severity.into());
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

#[allow(clippy::too_many_arguments)]
const fn builtin_meta(
    id: &'static str,
    default_severity: Severity,
    evidence: EvidenceClass,
    summary: &'static str,
    rationale: &'static str,
    documentation: &'static str,
    known_limits: &'static str,
    tags: &'static [&'static str],
) -> RuleMeta {
    RuleMeta {
        id,
        default_severity,
        evidence,
        summary,
        rationale,
        documentation,
        known_limits,
        tags,
    }
}

static BUILTIN_RULES: &[RuleMeta] = &[
    builtin_meta(
        "min-wait-loop",
        Severity::Warning,
        EvidenceClass::StaticIndicator,
        "loop body waits at the workshop minimum rate",
        "Avoid sustained maximum-frequency loop execution.",
        "A loop whose body contains a `wait` call at the minimum Workshop duration (~0.016 s) runs at maximum server frequency. Sustained high-frequency loops can degrade server performance for all players.",
        "Wait durations that are not statically known (computed at runtime) are treated as not-minimum and do not trigger this rule.",
        &["performance", "stability"],
    ),
    builtin_meta(
        "duplicate-condition",
        Severity::Warning,
        EvidenceClass::Exact,
        "condition is evaluated more than once within one rule",
        "Avoid unreachable or redundant conditional branches.",
        "The same condition appears in two or more branches of the same rule. Because Workshop conditions are evaluated sequentially, a later branch with an identical condition can never be taken.",
        "Detection is structural (not value-flow) and rule-local: two structurally identical conditions in different rules are not compared.",
        &["correctness"],
    ),
    builtin_meta(
        "expensive-loop-check",
        Severity::Info,
        EvidenceClass::Heuristic,
        "geometry predicate evaluated inside a loop body",
        "Surface expensive per-iteration geometry work.",
        "A geometry predicate (`distance`, `raycast`, or `isInLoS`) is called inside a loop body. These predicates may be expensive per evaluation and can accumulate significant cost at loop frequency.",
        "The expensive-call list is a fixed heuristic. It may miss unusual predicates or over-flag predicates that have been made cheap by a Workshop update.",
        &["performance"],
    ),
    builtin_meta(
        "ongoing-condition-hot-path",
        Severity::Info,
        EvidenceClass::Heuristic,
        "geometry predicate evaluated in an ongoing-rule condition",
        "Make high-frequency condition evaluation visible.",
        "An `Ongoing - Global` or `Ongoing - Each Player` rule evaluates a geometry predicate (`distance`, `raycast`, or `isInLoS`) in one of its conditions. Each server tick evaluates conditions in source order until one short-circuits the rule.",
        "The geometry-predicate list is a fixed heuristic. Non-ongoing player events and subroutines are deliberately excluded.",
        &["performance", "stability"],
    ),
    builtin_meta(
        "repeated-value",
        Severity::Warning,
        EvidenceClass::StaticIndicator,
        "identical value expression evaluated more than once in one loop scope",
        "Avoid repeated evaluation of the same loop-local expression.",
        "A structurally identical value expression appears more than once within one loop scope, so it is re-evaluated every iteration even though one evaluation would suffice.",
        "Detection is structural and scoped to one loop body.",
        &["performance"],
    ),
    builtin_meta(
        "while-without-wait",
        Severity::Warning,
        EvidenceClass::StaticIndicator,
        "while loop contains no wait call and may run without yielding",
        "Prevent infinite tight loops that crash the Workshop server.",
        "A `While` loop whose body contains no `Wait` call may repeat indefinitely without yielding execution to other rules or the game server tick.",
        "Calls to subroutines that contain a `Wait` are not traced interprocedurally.",
        &["correctness", "stability"],
    ),
];

impl Default for LintRegistry {
    fn default() -> Self {
        Self {
            entries: BUILTIN_RULES
                .iter()
                .cloned()
                .map(|meta| RegistryEntry {
                    meta: Some(meta),
                    declarative: None,
                })
                .collect(),
        }
    }
}

impl LintRegistry {
    pub fn rules(&self) -> impl Iterator<Item = &RuleMeta> {
        self.entries.iter().filter_map(|entry| entry.meta.as_ref())
    }

    pub fn load_yaml_str(&mut self, input: &str) -> Result<(), RuleRegistryError> {
        let catalog =
            Catalog::builtin().map_err(|error| RuleRegistryError::Catalog(error.to_string()))?;
        let definition = RuleDefinition::from_yaml_str(input).map_err(RuleRegistryError::Rule)?;
        let rule = DeclarativeRule::from_definition(definition, &catalog)
            .map_err(RuleRegistryError::Rule)?;
        self.insert_declarative(rule)
    }

    pub fn load_path(&mut self, path: &Path) -> Result<(), RuleRegistryError> {
        let catalog =
            Catalog::builtin().map_err(|error| RuleRegistryError::Catalog(error.to_string()))?;
        self.load_path_with_catalog(path, &catalog)
    }

    fn load_path_with_catalog(
        &mut self,
        path: &Path,
        catalog: &Catalog,
    ) -> Result<(), RuleRegistryError> {
        if path.is_dir() {
            let mut files = std::fs::read_dir(path)
                .map_err(|error| RuleRegistryError::Io(path.to_path_buf(), error))?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    matches!(
                        path.extension().and_then(|e| e.to_str()),
                        Some("yaml" | "yml")
                    )
                })
                .collect::<Vec<_>>();
            files.sort();
            for file in files {
                self.load_path_with_catalog(&file, catalog)?;
            }
            return Ok(());
        }
        let input = std::fs::read_to_string(path)
            .map_err(|error| RuleRegistryError::Io(path.to_path_buf(), error))?;
        let definition = RuleDefinition::from_yaml_str(&input).map_err(RuleRegistryError::Rule)?;
        let rule = DeclarativeRule::from_definition(definition, catalog)
            .map_err(RuleRegistryError::Rule)?;
        self.insert_declarative(rule)
    }

    fn insert_declarative(&mut self, rule: DeclarativeRule) -> Result<(), RuleRegistryError> {
        let id = rule.id();
        if self.entries.iter().any(|entry| {
            entry.meta.as_ref().is_some_and(|meta| meta.id == id)
                || entry
                    .declarative
                    .as_ref()
                    .is_some_and(|other| other.id() == id)
        }) {
            return Err(RuleRegistryError::DuplicateId(id.to_string()));
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
                let (id, default_sev, evidence, summary, rationale, doc, limits, tags, kind) =
                    if let Some(meta) = &entry.meta {
                        (
                            meta.id.to_string(),
                            meta.default_severity,
                            meta.evidence,
                            meta.summary.to_string(),
                            meta.rationale.to_string(),
                            meta.documentation.to_string(),
                            meta.known_limits.to_string(),
                            meta.tags.iter().map(|tag| (*tag).to_string()).collect(),
                            "native",
                        )
                    } else {
                        let rule = entry
                            .declarative
                            .as_ref()
                            .expect("registry entry has a rule");
                        let md = rule.metadata();
                        (
                            rule.id().to_string(),
                            rule.default_severity(),
                            rule.evidence(),
                            md.summary.clone(),
                            md.rationale.clone(),
                            md.documentation.clone(),
                            md.known_limits.clone(),
                            md.tags.clone(),
                            "declarative",
                        )
                    };
                RuleDescriptor {
                    effective_severity: config.effective_severity_for(&id, default_sev),
                    enabled: config.is_enabled(&id),
                    id,
                    default_severity: default_sev,
                    summary,
                    rationale,
                    documentation: doc,
                    known_limits: limits,
                    evidence,
                    tags,
                    kind,
                }
            })
            .collect()
    }

    pub fn run(&self, program: &Program, config: &LintConfig) -> Vec<Finding> {
        let mut findings = analyze(program, config);
        findings.extend(self.run_canonical_custom(program, config));
        findings
    }

    pub fn run_canonical_custom(&self, program: &Program, config: &LintConfig) -> Vec<Finding> {
        let mut findings = Vec::new();
        for rule in 0..program.rules.len() {
            for entry in &self.entries {
                let Some(declarative) = &entry.declarative else {
                    continue;
                };
                let id = declarative.id();
                if !config.is_enabled(id) {
                    continue;
                }
                let opts = config.options(id);
                let mut rule_findings =
                    declarative.run(program, rule, opts.min_matches, opts.max_matches);
                let severity = config.effective_severity_for(id, declarative.default_severity());
                for finding in &mut rule_findings {
                    finding.severity = severity;
                }
                findings.extend(rule_findings);
            }
        }
        findings
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
            Self::Io(path, error) => {
                write!(f, "cannot read rule source {}: {error}", path.display())
            }
            Self::Catalog(error) => write!(f, "cannot load Workshop catalog: {error}"),
            Self::Rule(error) => error.fmt(f),
            Self::DuplicateId(id) => write!(f, "rule ID '{id}' is already registered"),
        }
    }
}

impl std::error::Error for RuleRegistryError {}
