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
use workshop_rs::wir;

use crate::analysis::{
    Analysis, DuplicateCondition, EvidenceClass, ExpensiveLoopCheck, Finding, MinWaitLoop,
    OngoingConditionHotPath, RepeatedValue, Severity, WhileWithoutWait,
};
use crate::cfg::Cfg;
use crate::declarative::{DeclarativeRule, RuleDefinition, RuleError};
use crate::facts::SemanticFacts;

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
    /// runtime-validated. Mirrors [`Analysis::evidence`] of the rule's
    /// implementation (single source of truth).
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
///
/// Matches the string names used in structured findings (`"warning"`, `"info"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SeverityLabel {
    Warning,
    Info,
}

impl From<SeverityLabel> for Severity {
    fn from(label: SeverityLabel) -> Self {
        match label {
            SeverityLabel::Warning => Severity::Warning,
            SeverityLabel::Info => Severity::Info,
        }
    }
}

impl From<Severity> for SeverityLabel {
    fn from(severity: Severity) -> Self {
        match severity {
            Severity::Warning => SeverityLabel::Warning,
            Severity::Info => SeverityLabel::Info,
        }
    }
}

/// The deterministic lint configuration passed to [`LintRegistry::run`].
///
/// [`LintConfig::default`] enables all registered rules at their default
/// severities with no overrides. Unknown rule IDs are accepted and stored but
/// do not affect registered rules.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LintConfig {
    #[serde(default)]
    rules: HashMap<String, RuleConfig>,
}

impl LintConfig {
    /// Parse a project lint configuration from the stable YAML surface.
    pub fn from_yaml_str(input: &str) -> Result<Self, serde_yaml::Error> {
        serde_yaml::from_str(input)
    }

    /// Read and parse a project lint configuration from YAML.
    pub fn from_yaml_path(path: &Path) -> Result<Self, std::io::Error> {
        let input = std::fs::read_to_string(path)?;
        serde_yaml::from_str(&input).map_err(std::io::Error::other)
    }

    /// Disable a rule by its stable ID.
    ///
    /// Has no effect on other rules. Silently accepted for unknown IDs.
    pub fn disable(&mut self, rule_id: &str) {
        self.rules.entry(rule_id.to_string()).or_default().enabled = false;
    }

    /// Enable a rule by its stable ID.
    ///
    /// Silently accepted for unknown IDs.
    pub fn enable(&mut self, rule_id: &str) {
        self.rules.entry(rule_id.to_string()).or_default().enabled = true;
    }

    /// Override the severity for a rule by its stable ID.
    ///
    /// Silently accepted for unknown IDs; the override is stored and has no
    /// effect on rules that are not registered.
    pub fn set_severity(&mut self, rule_id: &str, severity: Severity) {
        self.rules
            .entry(rule_id.to_string())
            .or_default()
            .severity_override = Some(severity.into());
    }

    /// Override the severity of a rule from its CLI spelling.
    ///
    /// Accepts the stable severity names `"warning"` and `"info"`. Returns
    /// `false` when `severity` is not a known label, leaving the
    /// configuration unchanged.
    pub fn set_severity_by_name(&mut self, rule_id: &str, severity: &str) -> bool {
        let label = match severity {
            "warning" => SeverityLabel::Warning,
            "info" => SeverityLabel::Info,
            _ => return false,
        };
        self.set_severity(rule_id, label.into());
        true
    }

    /// Whether a rule is enabled.
    ///
    /// Returns `true` for unknown IDs (no config entry = enabled by default).
    pub fn is_enabled(&self, rule_id: &str) -> bool {
        self.rules.get(rule_id).is_none_or(|config| config.enabled)
    }

    /// Effective severity for a rule given its metadata and this config.
    ///
    /// Returns the override if one is set; otherwise the rule's default.
    pub fn effective_severity(&self, meta: &RuleMeta) -> Severity {
        self.rules
            .get(meta.id)
            .and_then(|config| config.severity_override)
            .map(Severity::from)
            .unwrap_or(meta.default_severity)
    }

    pub fn effective_severity_for(&self, id: &str, default: Severity) -> Severity {
        self.rules
            .get(id)
            .and_then(|config| config.severity_override)
            .map(Severity::from)
            .unwrap_or(default)
    }

    pub fn severity_override(&self, id: &str) -> Option<Severity> {
        self.rules
            .get(id)
            .and_then(|config| config.severity_override)
            .map(Severity::from)
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

/// One registered rule: its stable metadata and the analysis implementation.
struct RegistryEntry {
    meta: Option<RuleMeta>,
    analysis: Option<Box<dyn Analysis>>,
    declarative: Option<DeclarativeRule>,
}

/// The Wright lint rule registry.
///
/// [`LintRegistry::default`] returns the complete first-party rule set.
/// Use [`LintRegistry::run`] to execute the active rules over a
/// [`wir::Program`].
///
/// # Adding rules
///
/// First-party rules are added by pushing a [`RegistryEntry`] in
/// [`Default::default`]. Third-party plugin loading is explicitly out of scope
/// (issue #97).
pub struct LintRegistry {
    entries: Vec<RegistryEntry>,
}

impl Default for LintRegistry {
    /// Build the registry containing the first-party lint rules in their
    /// canonical order: `min-wait-loop`, `duplicate-condition`,
    /// `expensive-loop-check`, `ongoing-condition-hot-path`, `repeated-value`,
    /// `while-without-wait`.
    fn default() -> Self {
        // Build each analysis in a local binding first so the rule metadata
        // can take its evidence class from the same implementation that
        // produces findings (single source of truth).
        let min_wait: Box<dyn Analysis> = Box::new(MinWaitLoop);
        let duplicate_condition: Box<dyn Analysis> = Box::new(DuplicateCondition);
        let expensive_loop_check: Box<dyn Analysis> = Box::new(ExpensiveLoopCheck);
        let ongoing_condition_hot_path: Box<dyn Analysis> = Box::new(OngoingConditionHotPath);
        let repeated_value: Box<dyn Analysis> = Box::new(RepeatedValue);
        let while_without_wait: Box<dyn Analysis> = Box::new(WhileWithoutWait);
        let entries = vec![
            RegistryEntry {
                meta: Some(RuleMeta {
                    id: "min-wait-loop",
                    default_severity: Severity::Warning,
                    evidence: min_wait.evidence(),
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
                analysis: Some(min_wait),
                declarative: None,
            },
            RegistryEntry {
                meta: Some(RuleMeta {
                    id: "duplicate-condition",
                    default_severity: Severity::Warning,
                    evidence: duplicate_condition.evidence(),
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
                analysis: Some(duplicate_condition),
                declarative: None,
            },
            RegistryEntry {
                meta: Some(RuleMeta {
                    id: "expensive-loop-check",
                    default_severity: Severity::Info,
                    evidence: expensive_loop_check.evidence(),
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
                analysis: Some(expensive_loop_check),
                declarative: None,
            },
            RegistryEntry {
                meta: Some(RuleMeta {
                    id: "ongoing-condition-hot-path",
                    default_severity: Severity::Info,
                    evidence: ongoing_condition_hot_path.evidence(),
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
                analysis: Some(ongoing_condition_hot_path),
                declarative: None,
            },
            RegistryEntry {
                meta: Some(RuleMeta {
                    id: "repeated-value",
                    default_severity: Severity::Warning,
                    evidence: repeated_value.evidence(),
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
                analysis: Some(repeated_value),
                declarative: None,
            },
            RegistryEntry {
                meta: Some(RuleMeta {
                    id: "while-without-wait",
                    default_severity: Severity::Warning,
                    evidence: while_without_wait.evidence(),
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
                        "statically-bounded claim additionally requires every direct child of ",
                        "the body that can affect the compared variable to either provably move ",
                        "it toward the literal bound (a non-zero literal-step modify) or ",
                        "provably not write it; a `Set` on the variable, an away-direction or ",
                        "non-literal/zero-step modify, a `CallSubroutine`, and `If`/nested-loop ",
                        "subtrees writing the variable all force `unknown`. Modeling assumption: ",
                        "within the supported OPY/Workshop surface, `Debug`/`Print` and generic ",
                        "calls that are not user subroutines do not write user variables (a ",
                        "generic call matching a user-defined subroutine name is treated as a ",
                        "potential writer, for frontend fidelity). A direct-child nested loop ",
                        "whose termination is not statically provable (a non-counter `While`, ",
                        "or a `For Global Variable` with a zero or dynamic step, including ",
                        "through `If` branches) forces the enclosing loop to `unknown`, because ",
                        "a nested loop that never terminates prevents the outer loop from ",
                        "completing an iteration. There is no value-flow or ",
                        "initial-value analysis, so no maximum-iteration count is claimed even ",
                        "when the bound literal is small. `==`/`!=` conditions and ",
                        "constant-folded comparisons are not recognized (classified `unknown`). ",
                        "A wait placed anywhere in the body tree suppresses the finding, and ",
                        "`For Global Variable` loops are never flagged. A statically true ",
                        "condition is classified as non-terminating only because the modeled ",
                        "WIR has no break/goto action.",
                    ),
                    tags: &["stability"],
                }),
                analysis: Some(while_without_wait),
                declarative: None,
            },
        ];
        Self { entries }
    }
}

impl LintRegistry {
    /// Iterate over the metadata of every registered rule in registry order.
    pub fn rules(&self) -> impl Iterator<Item = &RuleMeta> {
        self.entries.iter().filter_map(|entry| entry.meta.as_ref())
    }

    /// Load one external declarative rule. The rule is canonicalized against
    /// the owning Workshop catalog before it can enter the execution registry.
    pub fn load_yaml_str(
        &mut self,
        input: &str,
        catalog: &workshop_rs::catalog::Catalog,
    ) -> Result<(), RuleRegistryError> {
        let definition = RuleDefinition::from_yaml_str(input).map_err(RuleRegistryError::Rule)?;
        let rule = DeclarativeRule::from_definition(definition, catalog)
            .map_err(RuleRegistryError::Rule)?;
        self.insert_declarative(rule)
    }

    /// Load all `.yaml`/`.yml` files in a path, or one file, in lexical order.
    pub fn load_path(
        &mut self,
        path: &Path,
        catalog: &workshop_rs::catalog::Catalog,
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
                self.load_path(&file, catalog)?;
            }
            return Ok(());
        }
        let input = std::fs::read_to_string(path)
            .map_err(|error| RuleRegistryError::Io(path.to_path_buf(), error))?;
        self.load_yaml_str(&input, catalog)
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
            analysis: None,
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
                        default_severity: metadata.default_severity.into(),
                        effective_severity: config
                            .effective_severity_for(rule.id(), metadata.default_severity.into()),
                        enabled: config.is_enabled(rule.id()),
                        summary: metadata.summary.clone(),
                        rationale: metadata.rationale.clone(),
                        documentation: metadata.documentation.clone(),
                        known_limits: metadata.known_limits.clone(),
                        evidence: metadata.evidence,
                        tags: metadata.tags.clone(),
                        kind: "declarative",
                    }
                }
            })
            .collect()
    }

    /// Run every enabled rule over every Workshop rule in `program` and return
    /// all findings, with configured severity overrides applied.
    ///
    /// Output order is deterministic: rules execute in registry order, over
    /// Workshop rules in program index order.
    pub fn run(&self, program: &wir::Program, config: &LintConfig) -> Vec<Finding> {
        self.run_report(program, config).findings
    }

    /// Run the registry while retaining machine-readable unavailable/skip
    /// statuses for rules whose canonical input cannot be analyzed.
    pub fn run_report(&self, program: &wir::Program, config: &LintConfig) -> LintRun {
        let mut report = LintRun::default();
        for (index, _) in program.rules.iter().enumerate() {
            let rule = wir::RuleId::from_index(index);
            let Ok(cfg) = Cfg::build(program, rule) else {
                for entry in &self.entries {
                    let id = entry
                        .meta
                        .as_ref()
                        .map(|meta| meta.id.to_string())
                        .or_else(|| entry.declarative.as_ref().map(|rule| rule.id().to_string()));
                    if let Some(id) = id.filter(|id| config.is_enabled(id)) {
                        report.skipped.push(SkippedRule {
                            id,
                            rule: index,
                            reason: "canonical CFG unavailable".to_string(),
                        });
                    }
                }
                continue;
            };
            let facts = SemanticFacts::new(program);
            for entry in &self.entries {
                let (id, mut rule_findings) =
                    if let (Some(meta), Some(analysis)) = (&entry.meta, &entry.analysis) {
                        if !config.is_enabled(meta.id) {
                            continue;
                        }
                        (meta.id.to_string(), analysis.run(program, rule, &cfg))
                    } else {
                        let declarative = entry
                            .declarative
                            .as_ref()
                            .expect("registry entry has a rule");
                        if !config.is_enabled(declarative.id()) {
                            continue;
                        }
                        (
                            declarative.id().to_string(),
                            declarative.run(
                                &facts,
                                rule,
                                config.options(declarative.id()).min_matches,
                                config.options(declarative.id()).max_matches,
                            ),
                        )
                    };
                if let Some(effective) = config.severity_override(&id) {
                    for finding in &mut rule_findings {
                        finding.severity = effective;
                    }
                }
                report.findings.extend(rule_findings);
            }
        }
        report
    }
}

#[derive(Debug)]
pub enum RuleRegistryError {
    Io(std::path::PathBuf, std::io::Error),
    Rule(RuleError),
    DuplicateId(String),
}

impl std::fmt::Display for RuleRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(path, error) => {
                write!(f, "cannot read rule source {}: {error}", path.display())
            }
            Self::Rule(error) => error.fmt(f),
            Self::DuplicateId(id) => write!(f, "rule ID '{id}' is already registered"),
        }
    }
}

impl std::error::Error for RuleRegistryError {}
