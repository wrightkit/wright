//! # Contract
//!
//! * Rule IDs are stable `&'static str` values that match the `code` field on
//!   every [`Finding`] produced by that rule.
//! * [`LintRegistry::default`] returns the complete first-party rule set in a
//!   fixed, deterministic order.
//! * [`LintConfig::default`] enables every registered rule at its default
//!   severity with no overrides.
//! * [`LintRegistry::run`] is deterministic: same program, same config, same
//!   toolchain → same findings in the same order.
//! * Unknown rule IDs supplied to [`LintConfig`] are silently stored but do
//!   not affect registered rules and do not prevent execution.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use workshop_rs::catalog::Catalog;

use crate::analysis::{EvidenceClass, Finding, Severity};
use crate::declarative::{DeclarativeRule, RuleDefinition, RuleError};

/// Static metadata for one lint rule.
///
/// All fields are `&'static str` / `&'static [&'static str]` to support
/// zero-cost embedding in registry entries and CLI/agent output without heap
/// allocation per rule.
#[derive(Debug, Clone)]
pub struct RuleMeta {
    /// Stable machine-readable identifier. Matches the `code` field on every
    /// [`Finding`] this rule produces.
    pub id: &'static str,
    /// Severity used when the rule fires and no override is configured.
    pub default_severity: Severity,
    /// The evidence class of this rule's findings: whether a finding is an
    /// exact structural fact, a static indicator, a documented heuristic, or
    /// runtime-validated.
    pub evidence: EvidenceClass,
    /// One-line human-readable description of what the rule detects.
    pub summary: &'static str,
    /// Why this rule is useful to a Workshop author.
    pub rationale: &'static str,
    /// Longer explanation, suitable for documentation or CLI `--explain` output.
    pub documentation: &'static str,
    /// Documented conditions under which the rule may produce false positives
    /// or false negatives.
    pub known_limits: &'static str,
    /// Coarse classification tags (e.g. `"performance"`, `"correctness"`).
    pub tags: &'static [&'static str],
}

/// Configuration applied to one rule at registry execution time.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleConfig {
    /// When `false` the rule is skipped entirely and produces no findings.
    #[serde(default = "default_true")]
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

fn default_true() -> bool {
    true
}

/// The intentionally small option surface shared by declarative rules.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

/// The effective configuration for a lint run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LintConfig {
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

/// Owned metadata used by CLI, agent, embedding, and documentation consumers.
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

/// A rule that was not executed because its canonical semantic input was not
/// available for one Workshop rule.
#[derive(Debug, Clone, Serialize)]
pub struct SkippedRule {
    pub id: String,
    pub rule: usize,
    pub reason: String,
}

/// The findings and explicit unavailable/skip statuses from one registry run.
#[derive(Debug, Clone, Default)]
pub struct LintRun {
    pub findings: Vec<Finding>,
    pub skipped: Vec<SkippedRule>,
}

/// One registered rule: its stable metadata and optional declarative rule.
struct RegistryEntry {
    meta: Option<RuleMeta>,
    declarative: Option<DeclarativeRule>,
}

/// The Wright lint rule registry.
pub struct LintRegistry {
    entries: Vec<RegistryEntry>,
}

impl Default for LintRegistry {
    /// Build the registry containing the first-party lint rules in their
    /// canonical order: `min-wait-loop`, `duplicate-condition`,
    /// `expensive-loop-check`, `ongoing-condition-hot-path`, `repeated-value`,
    /// `while-without-wait`.
    fn default() -> Self {
        let entries = vec![
            RegistryEntry {
                meta: Some(RuleMeta {
                    id: "min-wait-loop",
                    default_severity: Severity::Warning,
                    evidence: EvidenceClass::StaticIndicator,
                    summary: "loop body waits at the workshop minimum rate",
                    rationale: "Avoid sustained maximum-frequency loop execution.",
                    documentation: concat!(
                        "A loop whose body contains a `wait` call at the minimum Workshop ",
                        "duration (~0.016 s) runs at maximum server frequency. Sustained ",
                        "high-frequency loops can degrade server performance for all players.",
                    ),
                    known_limits: concat!(
                        "Wait durations that are not statically known (computed at runtime) ",
                        "are treated as not-minimum and do not trigger this rule.",
                    ),
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
                    documentation: concat!(
                        "The same condition appears in two or more branches of the same rule. ",
                        "Because Workshop conditions are evaluated sequentially, a later branch ",
                        "with an identical condition can never be taken.",
                    ),
                    known_limits: concat!(
                        "Detection is structural (not value-flow) and rule-local: two ",
                        "structurally identical conditions in different rules are not compared.",
                    ),
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
                    documentation: concat!(
                        "A geometry predicate (`distance`, `raycast`, or `isInLoS`) is called ",
                        "inside a loop body. These predicates may be expensive per evaluation ",
                        "and can accumulate significant cost at loop frequency.",
                    ),
                    known_limits: concat!(
                        "The expensive-call list is a fixed heuristic. It may miss unusual ",
                        "predicates or over-flag predicates that have been made cheap by a ",
                        "Workshop update.",
                    ),
                    tags: &["performance"],
                }),
                declarative: None,
            },
            RegistryEntry {
                meta: Some(RuleMeta {
                    id: "ongoing-condition-hot-path",
                    default_severity: Severity::Info,
                    evidence: EvidenceClass::StaticIndicator,
                    summary: "geometry predicate evaluated in an ongoing-rule condition",
                    rationale: "Make high-frequency condition evaluation visible.",
                    documentation: concat!(
                        "An `Ongoing - Global` or `Ongoing - Each Player` rule evaluates a ",
                        "geometry predicate (`distance`, `raycast`, or `isInLoS`) in one of ",
                        "its conditions. Each server tick evaluates conditions in source order ",
                        "until one short-circuits the rule, so a predicate in a later condition ",
                        "is reached only after every preceding condition passes. The finding ",
                        "identifies the condition's position and any later short-circuit gates. ",
                        "This concerns condition evaluation, not a claim ",
                        "that the action block executes every tick while conditions remain true. ",
                        "Real-project evidence: overpy-cronch's `challenge 1 ",
                        "finished` rule (Zezombye/overpy commit ",
                        "`eea67adbcf6926c4004e35e25ab4be072624a44e`, GPL-3.0-only) evaluates ",
                        "`Distance Between` in an ongoing global condition after its cheap ",
                        "challenge-state gate.",
                    ),
                    known_limits: concat!(
                        "The geometry-predicate list is a fixed heuristic and may miss other ",
                        "costly operations or over-flag a predicate made cheap by a Workshop ",
                        "update. The analysis reports canonical event identity, condition order, ",
                        "and predicate presence; it does not measure runtime CPU cost, assume a ",
                        "server population, or infer the selectivity of any condition. ",
                        "Non-ongoing player events and subroutines are deliberately excluded.",
                    ),
                    tags: &["performance", "stability"],
                }),
                declarative: None,
            },
            RegistryEntry {
                meta: Some(RuleMeta {
                    id: "repeated-value",
                    default_severity: Severity::Warning,
                    evidence: EvidenceClass::StaticIndicator,
                    summary: "identical value expression evaluated more than once in one loop scope",
                    rationale: "Avoid repeated evaluation of the same loop-local expression.",
                    documentation: concat!(
                        "A structurally identical value expression appears more than once within one ",
                        "loop scope, so it is re-evaluated every iteration even though one ",
                        "evaluation would suffice. Within a single atomic evaluation the ",
                        "re-evaluation is redundant for deterministic expressions; the Workshop ",
                        "ecosystem has an `Evaluate Once` idiom for exactly this cost. One ",
                        "finding is reported per distinct duplicated shape per loop scope, at ",
                        "the shape's first occurrence, with the statically known occurrence ",
                        "count. Real-project evidence: overpy-santa (workshop lines 108-112; ",
                        "`santa.opy:72-77`) and overpy-parabola (workshop lines 79-80; ",
                        "`parabola.opy:45-51`), both pinned at `Zezombye/overpy` commit ",
                        "`eea67adbcf6926c4004e35e25ab4be072624a44e` (GPL-3.0-only, redistributable).",
                    ),
                    known_limits: concat!(
                        "Detection is rule-local and structural (arena-id-independent call name ",
                        "plus argument shape) with no value-flow analysis: a duplicate across ",
                        "separate actions proves re-scheduling, not result-equality, because an ",
                        "intervening action may mutate a read variable. A duplicated expression ",
                        "is reported once per loop scope at its maximal shape, so nested ",
                        "duplicates are subsumed; expressions with fewer than two call nodes, ",
                        "including bare array reads, are never flagged. Sub-expressions ",
                        "containing non-deterministic values (e.g. `Random Value`/`Random Real`) ",
                        "are still guaranteed to be re-evaluated, but the finding may not ",
                        "indicate a defect. `Evaluate Once`-wrapped inner reads reduce but do ",
                        "not eliminate the outer recomputation. Loop coverage is `While` + ",
                        "`For Global Variable` only; `For Player Variable` loops are not modeled.",
                    ),
                    tags: &["performance", "stability"],
                }),
                declarative: None,
            },
            RegistryEntry {
                meta: Some(RuleMeta {
                    id: "while-without-wait",
                    default_severity: Severity::Warning,
                    evidence: EvidenceClass::StaticIndicator,
                    summary: "while loop body contains no wait call",
                    rationale: "Ensure a loop can yield to the Workshop scheduler.",
                    documentation: concat!(
                        "A `While` loop whose body contains no `wait` call cannot yield to the ",
                        "server while its condition holds. Each finding carries the loop's ",
                        "boundedness evidence (`obviously-unbounded`, `statically-bounded`, or ",
                        "`unknown`), and the severity is derived from that evidence: `info` for ",
                        "a statically bounded no-yield loop, `warning` for an obviously ",
                        "unbounded or unknown one. A bounded no-yield loop is explicitly NOT ",
                        "treated as equivalent to an unbounded one. Real-consumer evidence: ",
                        "the agent-lab repro `loop-waitless.opy` (wrightkit/agent-lab#68) — ",
                        "`globalvar loopCount; rule \"waitless loop\": @Condition ",
                        "getTotalTimeElapsed() > 5; loopCount = 0; while loopCount < 10: ",
                        "loopCount += 1` — is classified as `statically-bounded` (a finite ",
                        "10-iteration counter loop), not as an unbounded hazard; the ",
                        "Workshop Agent analyzer reports the same construct as ",
                        "`workshop.performance.waitless-loop`, and Wright deliberately does ",
                        "not copy that severity semantics (issue #103).",
                    ),
                    known_limits: concat!(
                        "Counter-pattern detection is conservative and structural: only ",
                        "literal-bound comparisons (`<`, `<=`, `>`, `>=`) are recognized. A ",
                        "loop is bounded only when: (1) its condition compares a variable to a ",
                        "literal bound, (2) the body unconditionally increments/decrements that ",
                        "same variable, and (3) the step moves the variable toward the bound. ",
                        "Arbitrary step expressions, dynamic bounds, multiple counter ",
                        "mutations, nested condition resets, and loops that yield via `wait` ",
                        "are outside this classification. An unclassified loop produces `unknown` ",
                        "evidence; it is NOT assumed to be unbounded (issue #103).",
                    ),
                    tags: &["performance", "stability"],
                }),
                declarative: None,
            },
        ];
        Self { entries }
    }
}

impl LintRegistry {
    pub fn rules(&self) -> impl Iterator<Item = &RuleMeta> {
        self.entries.iter().filter_map(|entry| entry.meta.as_ref())
    }

    /// Load all `.yaml`/`.yml` files in a path, or one file, in lexical order.
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
        let id = rule.id().to_string();
        if self.entries.iter().any(|entry| {
            entry.meta.as_ref().is_some_and(|meta| meta.id == id)
                || entry
                    .declarative
                    .as_ref()
                    .is_some_and(|other| other.id() == id)
        }) {
            return Err(RuleRegistryError::DuplicateId(id));
        }
        self.entries.push(RegistryEntry {
            meta: None,
            declarative: Some(rule),
        });
        Ok(())
    }

    /// Return metadata for native and declarative rules in deterministic order.
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
                        tags: meta.tags.iter().map(|tag| (*tag).to_string()).collect(),
                        kind: "native",
                    }
                } else {
                    let rule = entry
                        .declarative
                        .as_ref()
                        .expect("registry entry has a rule");
                    let metadata = rule.metadata();
                    RuleDescriptor {
                        id: rule.id().to_string(),
                        default_severity: rule.default_severity(),
                        effective_severity: config
                            .effective_severity_for(rule.id(), rule.default_severity()),
                        enabled: config.is_enabled(rule.id()),
                        summary: metadata.summary.clone(),
                        rationale: metadata.rationale.clone(),
                        documentation: metadata.documentation.clone(),
                        known_limits: metadata.known_limits.clone(),
                        evidence: rule.evidence(),
                        tags: metadata.tags.clone(),
                        kind: "declarative",
                    }
                }
            })
            .collect()
    }

    /// Run every enabled rule over `program` and return all findings, with
    /// configured severity overrides applied.
    pub fn run(&self, program: &workshop_rs::Program, config: &LintConfig) -> Vec<Finding> {
        let mut findings = crate::canonical::analyze(program, config);
        findings.extend(self.run_canonical_custom(program, config));
        findings
    }

    /// Run custom declarative rules over the canonical public Workshop model.
    /// First-party rules are owned by `canonical::analyze`; this method only
    /// supplies the registry-owned extension entries.
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
