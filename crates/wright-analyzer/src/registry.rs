//! Lint rule registry and configuration contract (M12, #97).
//!
//! This module defines the stable rule-identity and metadata types, the
//! [`LintRegistry`] that holds the first-party rule set, and [`LintConfig`]
//! that controls which rules are active and at what severity.
//!
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

use serde::{Deserialize, Serialize};
use wright_ir::wir;

use crate::analysis::{
    Analysis, DuplicateCondition, EvidenceClass, ExpensiveLoopCheck, Finding, MinWaitLoop, Severity,
};
use crate::cfg::Cfg;

// ── Rule metadata ─────────────────────────────────────────────────────────────

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
    /// Longer explanation, suitable for documentation or CLI `--explain` output.
    pub documentation: &'static str,
    /// Documented conditions under which the rule may produce false positives
    /// or false negatives.
    pub known_limits: &'static str,
    /// Coarse classification tags (e.g. `"performance"`, `"correctness"`).
    pub tags: &'static [&'static str],
}

// ── Per-rule configuration ────────────────────────────────────────────────────

/// Configuration applied to one rule at registry execution time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleConfig {
    /// When `false` the rule is skipped entirely and produces no findings.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// When `Some`, replaces the rule's [`RuleMeta::default_severity`] in
    /// every finding produced during this run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity_override: Option<SeverityLabel>,
}

impl Default for RuleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            severity_override: None,
        }
    }
}

fn default_true() -> bool {
    true
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

// ── Lint configuration ────────────────────────────────────────────────────────

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
}

// ── Registry ──────────────────────────────────────────────────────────────────

/// One registered rule: its stable metadata and the analysis implementation.
struct RegistryEntry {
    meta: RuleMeta,
    analysis: Box<dyn Analysis>,
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
/// for M12 (issue #97).
pub struct LintRegistry {
    entries: Vec<RegistryEntry>,
}

impl Default for LintRegistry {
    /// Build the registry containing all three first-party M12 lint rules in
    /// their canonical order: `min-wait-loop`, `duplicate-condition`,
    /// `expensive-loop-check`.
    fn default() -> Self {
        // Build each analysis in a local binding first so the rule metadata
        // can take its evidence class from the same implementation that
        // produces findings (single source of truth).
        let min_wait: Box<dyn Analysis> = Box::new(MinWaitLoop);
        let duplicate_condition: Box<dyn Analysis> = Box::new(DuplicateCondition);
        let expensive_loop_check: Box<dyn Analysis> = Box::new(ExpensiveLoopCheck);
        let entries = vec![
            RegistryEntry {
                meta: RuleMeta {
                    id: "min-wait-loop",
                    default_severity: Severity::Warning,
                    evidence: min_wait.evidence(),
                    summary: "loop body waits at the workshop minimum rate",
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
                },
                analysis: min_wait,
            },
            RegistryEntry {
                meta: RuleMeta {
                    id: "duplicate-condition",
                    default_severity: Severity::Warning,
                    evidence: duplicate_condition.evidence(),
                    summary: "condition is evaluated more than once within one rule",
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
                },
                analysis: duplicate_condition,
            },
            RegistryEntry {
                meta: RuleMeta {
                    id: "expensive-loop-check",
                    default_severity: Severity::Info,
                    evidence: expensive_loop_check.evidence(),
                    summary: "geometry predicate evaluated inside a loop body",
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
                },
                analysis: expensive_loop_check,
            },
        ];
        Self { entries }
    }
}

impl LintRegistry {
    /// Iterate over the metadata of every registered rule in registry order.
    pub fn rules(&self) -> impl Iterator<Item = &RuleMeta> {
        self.entries.iter().map(|entry| &entry.meta)
    }

    /// Run every enabled rule over every Workshop rule in `program` and return
    /// all findings, with configured severity overrides applied.
    ///
    /// Output order is deterministic: rules execute in registry order, over
    /// Workshop rules in program index order.
    pub fn run(&self, program: &wir::Program, config: &LintConfig) -> Vec<Finding> {
        let mut findings = Vec::new();
        for (index, _) in program.rules.iter().enumerate() {
            let rule = wir::RuleId::from_index(index);
            let Ok(cfg) = Cfg::build(program, rule) else {
                continue; // invalid rule skipped; cannot be analyzed
            };
            for entry in &self.entries {
                if !config.is_enabled(entry.meta.id) {
                    continue;
                }
                let mut rule_findings = entry.analysis.run(program, rule, &cfg);
                // Apply the severity override configured for this rule.
                let effective = config
                    .rules
                    .get(entry.meta.id)
                    .and_then(|c| c.severity_override);
                if let Some(label) = effective {
                    let sv = Severity::from(label);
                    for finding in &mut rule_findings {
                        finding.severity = sv;
                    }
                }
                findings.extend(rule_findings);
            }
        }
        findings
    }
}
