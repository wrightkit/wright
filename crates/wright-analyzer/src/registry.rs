use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;
use workshop_rs::catalog::Catalog;

use crate::analysis::{EvidenceClass, Finding, Severity};
pub use crate::config::{LintConfig, LintPackagePin, RuleConfig, RuleOptions, SeverityLabel};
use crate::declarative::{DeclarativeRule, RuleDefinition, RuleError, RuleIdentity};

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

enum RegistryEntry {
    Native(RuleMeta),
    Declarative(Box<DeclarativeRule>),
}

impl RegistryEntry {
    fn id(&self) -> &str {
        match self {
            Self::Native(meta) => meta.id,
            Self::Declarative(rule) => rule.id(),
        }
    }
}

/// The Wright lint rule registry.
pub struct LintRegistry {
    entries: Vec<RegistryEntry>,
}

pub(crate) const MIN_WAIT_LOOP: RuleMeta = RuleMeta {
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
};

pub(crate) const DUPLICATE_CONDITION: RuleMeta = RuleMeta {
    id: "duplicate-condition",
    default_severity: Severity::Warning,
    evidence: EvidenceClass::Exact,
    summary: "an Else If repeats a condition earlier in the same If/Else If chain",
    rationale: "Avoid unreachable or redundant conditional branches.",
    documentation: concat!(
        "An `Else If` branch repeats the condition of an earlier `If` or ",
        "`Else If` branch in the same chain. Workshop evaluates a chain's ",
        "conditions in order until one passes, so the repeated branch is ",
        "unreachable. Conditions in separate `If` statements, `While` ",
        "conditions, and branches of different chains are not compared.",
    ),
    known_limits: concat!(
        "Detection is structural (not value-flow) and chain-local. ",
        "Structurally identical conditions built from nondeterministic ",
        "values (such as the `Random *` family) are evaluated separately ",
        "per branch and can differ even within one chain; Workshop ",
        "semantics expose no nondeterminism fact yet, so such repeats ",
        "are still reported.",
    ),
    tags: &["correctness"],
};

pub(crate) const EXPENSIVE_LOOP_CHECK: RuleMeta = RuleMeta {
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
};

pub(crate) const ONGOING_CONDITION_HOT_PATH: RuleMeta = RuleMeta {
    id: "ongoing-condition-hot-path",
    default_severity: Severity::Info,
    evidence: EvidenceClass::Heuristic,
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
};

pub(crate) const REPEATED_VALUE: RuleMeta = RuleMeta {
    id: "repeated-value",
    default_severity: Severity::Warning,
    evidence: EvidenceClass::Exact,
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
};

pub(crate) const WHILE_WITHOUT_WAIT: RuleMeta = RuleMeta {
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
};

pub(crate) const NESTED_BINDER_LAMBDA: RuleMeta = RuleMeta {
    id: "nested-binder-lambda",
    default_severity: Severity::Warning,
    evidence: EvidenceClass::StaticIndicator,
    summary: "element-binding array call nested inside another binder's per-element argument",
    rationale: "Surface a Workshop shape observed only in client-rejected builds.",
    documentation: concat!(
        "The element-binding values — `filteredArray`, `sortedArray`, ",
        "`mappedArray`, `isTrueForAll`, `isTrueForAny` — evaluate every ",
        "argument after the first once per array element, binding ",
        "`Current Array Element`/`Current Array Index` to that element. ",
        "A binder inside that per-element argument creates a nested ",
        "binding scope: inside the inner call's own per-element ",
        "argument the element values denote the inner array and the ",
        "outer element is shadowed. The shape compiles and is ",
        "semantically defined, but it is the one structural signature ",
        "measured so far that separates client-rejected Workshop builds ",
        "from accepted ones: the `OWBastion/Bastion#391`-era build and ",
        "the workshop-rs import-limits verbatim reproducer carry it, ",
        "and no measured accepted build does (`workshop-rs` ",
        "`docs/import-limits.md`; wright issue #713). Whether the ",
        "client enforces a structural limit on the shape is unconfirmed; ",
        "the finding is an indicator, not an established import boundary.",
    ),
    known_limits: concat!(
        "The trigger is structurally exact but its impact is indicative: ",
        "nested binders are valid Workshop and may be intentional — an ",
        "inner aggregation over an unrelated array reads correctly. A ",
        "binder in the array-source argument (index 0), e.g. ",
        "`Filtered Array(Sorted Array(x, r), c)`, creates no nested ",
        "scope and is not reported. The rule names a suspect shape, not ",
        "a measured client boundary: no confirmed import depth or ",
        "nesting limit exists yet in workshop-rs.",
    ),
    tags: &["correctness", "stability"],
};

impl Default for LintRegistry {
    fn default() -> Self {
        let entries = vec![
            RegistryEntry::Native(MIN_WAIT_LOOP),
            RegistryEntry::Native(DUPLICATE_CONDITION),
            RegistryEntry::Native(EXPENSIVE_LOOP_CHECK),
            RegistryEntry::Native(ONGOING_CONDITION_HOT_PATH),
            RegistryEntry::Native(REPEATED_VALUE),
            RegistryEntry::Native(WHILE_WITHOUT_WAIT),
            RegistryEntry::Native(NESTED_BINDER_LAMBDA),
        ];
        Self { entries }
    }
}

impl LintRegistry {
    pub fn rules(&self) -> impl Iterator<Item = &RuleMeta> {
        self.entries.iter().filter_map(|entry| match entry {
            RegistryEntry::Native(meta) => Some(meta),
            RegistryEntry::Declarative(_) => None,
        })
    }

    /// Load one external declarative rule against the canonical Workshop catalog.
    pub fn load_yaml_str(&mut self, input: &str) -> Result<(), RuleRegistryError> {
        self.load_yaml_str_as(input, RuleIdentity::External)
    }

    /// Load one first-party package rule; see [`Self::load_first_party_path`].
    pub fn load_first_party_yaml_str(&mut self, input: &str) -> Result<(), RuleRegistryError> {
        self.load_yaml_str_as(input, RuleIdentity::FirstParty)
    }

    fn load_yaml_str_as(
        &mut self,
        input: &str,
        identity: RuleIdentity,
    ) -> Result<(), RuleRegistryError> {
        let catalog = Catalog::builtin_ref()
            .map_err(|error| RuleRegistryError::Catalog(error.to_string()))?;
        let definition = RuleDefinition::from_yaml_str(input).map_err(RuleRegistryError::Rule)?;
        let rule = DeclarativeRule::from_definition_with_identity(definition, catalog, identity)
            .map_err(RuleRegistryError::Rule)?;
        self.insert_declarative(rule, identity)
    }

    /// Load all `.yaml`/`.yml` files in a path, or one file, in lexical order.
    pub fn load_path(&mut self, path: &Path) -> Result<(), RuleRegistryError> {
        self.load_path_as(path, RuleIdentity::External)
    }

    /// Load `.yaml`/`.yml` files as first-party rules (issue #843); see
    /// [`RuleIdentity`] for the accepted IDs and native replacement.
    pub fn load_first_party_path(&mut self, path: &Path) -> Result<(), RuleRegistryError> {
        self.load_path_as(path, RuleIdentity::FirstParty)
    }

    fn load_path_as(
        &mut self,
        path: &Path,
        identity: RuleIdentity,
    ) -> Result<(), RuleRegistryError> {
        let catalog = Catalog::builtin_ref()
            .map_err(|error| RuleRegistryError::Catalog(error.to_string()))?;
        self.load_path_with_catalog(path, catalog, identity)
    }

    fn load_path_with_catalog(
        &mut self,
        path: &Path,
        catalog: &Catalog,
        identity: RuleIdentity,
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
                self.load_path_with_catalog(&file, catalog, identity)?;
            }
            return Ok(());
        }
        let input = std::fs::read_to_string(path)
            .map_err(|error| RuleRegistryError::Io(path.to_path_buf(), error))?;
        let definition = RuleDefinition::from_yaml_str(&input).map_err(RuleRegistryError::Rule)?;
        let rule = DeclarativeRule::from_definition_with_identity(definition, catalog, identity)
            .map_err(RuleRegistryError::Rule)?;
        self.insert_declarative(rule, identity)
    }

    fn insert_declarative(
        &mut self,
        rule: DeclarativeRule,
        identity: RuleIdentity,
    ) -> Result<(), RuleRegistryError> {
        let id = rule.id().to_string();
        if let Some(position) = self.entries.iter().position(|entry| entry.id() == id) {
            if identity == RuleIdentity::FirstParty
                && matches!(self.entries[position], RegistryEntry::Native(_))
            {
                self.entries[position] = RegistryEntry::Declarative(Box::new(rule));
                return Ok(());
            }
            return Err(RuleRegistryError::DuplicateId(id));
        }
        self.entries
            .push(RegistryEntry::Declarative(Box::new(rule)));
        Ok(())
    }

    /// Whether a native or declarative rule with this ID is registered.
    pub fn contains(&self, id: &str) -> bool {
        self.entries.iter().any(|entry| entry.id() == id)
    }

    fn native_owns(&self, id: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| matches!(entry, RegistryEntry::Native(_)) && entry.id() == id)
    }

    /// Return metadata for native and declarative rules in deterministic order.
    pub fn descriptors(&self, config: &LintConfig) -> Vec<RuleDescriptor> {
        self.entries
            .iter()
            .map(|entry| match entry {
                RegistryEntry::Native(meta) => RuleDescriptor {
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
                },
                RegistryEntry::Declarative(rule) => {
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
        self.run_report(program, config).findings
    }

    /// Run registered rules and retain per-rule statuses when canonical control
    /// flow cannot be analyzed.
    #[allow(unsafe_code)] // hotpath 0.28 emits `#[unsafe(export_name)]` statics
    pub fn run_report(&self, program: &workshop_rs::Program, config: &LintConfig) -> LintRun {
        let mut report = LintRun::default();
        let mut available = Vec::with_capacity(program.rules.len());
        for (rule_id, rule) in program.rules.iter().enumerate() {
            let valid = !rule.disabled && canonical_cfg_available(&rule.actions);
            available.push(valid);
            if valid {
                continue;
            }
            if rule.disabled {
                continue;
            }
            for entry in &self.entries {
                if config.is_enabled(entry.id()) {
                    report.skipped.push(SkippedRule {
                        id: entry.id().to_string(),
                        rule: rule_id,
                        reason: "canonical CFG unavailable".to_string(),
                    });
                }
            }
        }

        let mut findings = hotpath::measure_block!("analyzer::lint_analyze", {
            crate::canonical::analyze(program, config)
        });
        findings.retain(|finding| self.native_owns(&finding.code));
        findings.extend(hotpath::measure_block!("analyzer::lint_custom", {
            self.run_canonical_custom(program, config)
        }));
        findings.retain(|finding| available.get(finding.rule).copied().unwrap_or(false));
        let order: HashMap<_, _> = self
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.id().to_string(), index))
            .collect();
        findings.sort_by_key(|finding| {
            (
                finding.rule,
                order.get(&finding.code).copied().unwrap_or(usize::MAX),
            )
        });
        report.findings = findings;
        report
    }

    /// Run declarative rules over the canonical Workshop model; native
    /// analysis is owned by `canonical::analyze`.
    pub fn run_canonical_custom(
        &self,
        program: &workshop_rs::Program,
        config: &LintConfig,
    ) -> Vec<Finding> {
        let mut findings = Vec::new();
        for rule in 0..program.rules.len() {
            for entry in &self.entries {
                let RegistryEntry::Declarative(declarative) = entry else {
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

fn canonical_cfg_available(actions: &[workshop_rs::Action]) -> bool {
    let mut control_flow = Vec::<(bool, bool)>::new();
    for action in actions {
        let action = crate::canonical::unwrapped(action);
        match action {
            workshop_rs::Action::If { .. } => control_flow.push((true, false)),
            workshop_rs::Action::While { .. }
            | workshop_rs::Action::ForGlobalVariable { .. }
            | workshop_rs::Action::ForPlayerVariable { .. } => control_flow.push((false, false)),
            workshop_rs::Action::ElseIf { .. } => {
                if !matches!(control_flow.last(), Some((true, false))) {
                    return false;
                }
            }
            workshop_rs::Action::Else => match control_flow.last_mut() {
                Some((true, has_else)) if !*has_else => *has_else = true,
                _ => return false,
            },
            workshop_rs::Action::End if control_flow.pop().is_none() => return false,
            workshop_rs::Action::End => {}
            _ => {}
        }
    }
    control_flow.is_empty()
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
