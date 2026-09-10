//! Registry contract tests (#97): stable rule identity, metadata, deterministic
//! configuration (enable/disable/severity-override), and execution determinism.

use std::path::{Path, PathBuf};

use workshop_rs::catalog::Catalog;
use workshop_rs::wir::Program as WirProgram;
use wright_analyzer::analysis::{EvidenceClass, Severity};
use wright_analyzer::registry::{LintConfig, LintRegistry};
use wright_core::hir;
use wright_ir::lower;

// ── Test helpers ──────────────────────────────────────────────────────────────

fn fixture_path(fixture_id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../adapter/fixtures")
        .join(format!("{fixture_id}.json"))
}

fn local_fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.json"))
}

fn lower_program(path: &Path) -> WirProgram {
    let json = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read fixture {}: {error}", path.display()));
    let protocol = hir::parse_str(&json).expect("fixture parses");
    let model = protocol.to_ir().expect("fixture converts");
    lower::lower(&model).expect("fixture lowers")
}

fn corpus_program(fixture_id: &str) -> WirProgram {
    lower_program(&fixture_path(fixture_id))
}

fn local_program(name: &str) -> WirProgram {
    lower_program(&local_fixture_path(name))
}

// ── Registry identity ─────────────────────────────────────────────────────────

#[test]
fn registry_first_party_rules_have_unique_stable_ids() {
    let registry = LintRegistry::default();
    let ids: Vec<&str> = registry.rules().map(|meta| meta.id).collect();
    assert!(!ids.is_empty(), "the registry contains first-party rules");
    let unique: std::collections::HashSet<_> = ids.iter().copied().collect();
    assert_eq!(unique.len(), ids.len(), "rule IDs are unique");
    assert!(ids.iter().all(|id| !id.is_empty()), "rule IDs are stable");
}

#[test]
fn rule_metadata_fields_are_non_empty() {
    let registry = LintRegistry::default();
    for meta in registry.rules() {
        assert!(!meta.id.is_empty(), "{}: id must be non-empty", meta.id);
        assert!(
            !meta.summary.is_empty(),
            "{}: summary must be non-empty",
            meta.id
        );
        assert!(
            !meta.rationale.is_empty(),
            "{}: rationale must be non-empty",
            meta.id
        );
        assert!(
            !meta.documentation.is_empty(),
            "{}: documentation must be non-empty",
            meta.id
        );
        assert!(
            !meta.known_limits.is_empty(),
            "{}: known_limits must be non-empty",
            meta.id
        );
        assert!(!meta.tags.is_empty(), "{}: tags must be non-empty", meta.id);
    }
}

#[test]
fn rule_ids_are_canonical_kebab_case() {
    let registry = LintRegistry::default();
    for meta in registry.rules() {
        assert!(
            meta.id
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch == '-'),
            "{}: rule ID must be lowercase-kebab-case",
            meta.id
        );
    }
}

#[test]
fn rule_default_severities_match_known_values() {
    let registry = LintRegistry::default();
    let severities: Vec<(&str, Severity)> = registry
        .rules()
        .map(|meta| (meta.id, meta.default_severity))
        .collect();
    let min_wait = severities
        .iter()
        .find(|(id, _)| *id == "min-wait-loop")
        .unwrap();
    assert_eq!(min_wait.1, Severity::Warning);

    let dup_cond = severities
        .iter()
        .find(|(id, _)| *id == "duplicate-condition")
        .unwrap();
    assert_eq!(dup_cond.1, Severity::Warning);

    let exp_loop = severities
        .iter()
        .find(|(id, _)| *id == "expensive-loop-check")
        .unwrap();
    assert_eq!(exp_loop.1, Severity::Info);

    let ongoing = severities
        .iter()
        .find(|(id, _)| *id == "ongoing-condition-hot-path")
        .unwrap();
    assert_eq!(ongoing.1, Severity::Info);

    let repeated = severities
        .iter()
        .find(|(id, _)| *id == "repeated-value")
        .unwrap();
    assert_eq!(repeated.1, Severity::Warning);

    let no_wait = severities
        .iter()
        .find(|(id, _)| *id == "while-without-wait")
        .unwrap();
    assert_eq!(no_wait.1, Severity::Warning);
}

// ── Evidence classification (#98) ────────────────────────────────────────────

#[test]
fn rule_evidence_classes_are_declared() {
    let registry = LintRegistry::default();
    let evidence: Vec<(&str, EvidenceClass)> = registry
        .rules()
        .map(|meta| (meta.id, meta.evidence))
        .collect();
    assert_eq!(
        evidence_of(&evidence, "min-wait-loop"),
        EvidenceClass::StaticIndicator,
        "the minimum-duration wait is statically known; the frequency impact is an indicator"
    );
    assert_eq!(
        evidence_of(&evidence, "duplicate-condition"),
        EvidenceClass::Exact,
        "a duplicated condition is a structural fact"
    );
    assert_eq!(
        evidence_of(&evidence, "expensive-loop-check"),
        EvidenceClass::Heuristic,
        "the expensive-call list is a documented fixed heuristic"
    );
    assert_eq!(
        evidence_of(&evidence, "ongoing-condition-hot-path"),
        EvidenceClass::Heuristic,
        "ongoing condition identity is exact but the expensive-call list is heuristic"
    );
    assert_eq!(
        evidence_of(&evidence, "repeated-value"),
        EvidenceClass::Exact,
        "a duplicated value in one loop scope is a structural fact"
    );
    assert_eq!(
        evidence_of(&evidence, "while-without-wait"),
        EvidenceClass::StaticIndicator,
        "the missing wait is statically known; the frequency impact is an indicator"
    );
}

fn evidence_of(evidence: &[(&str, EvidenceClass)], id: &str) -> EvidenceClass {
    evidence
        .iter()
        .find(|(rule_id, _)| *rule_id == id)
        .map(|(_, class)| *class)
        .expect("registered rule")
}

#[test]
fn findings_carry_the_evidence_class_of_their_rule() {
    let program = corpus_program("synthetic/control-flow");
    let config = LintConfig::default();
    let registry = LintRegistry::default();

    let findings = registry.run(&program, &config);
    assert!(
        !findings.is_empty(),
        "control-flow must produce at least one finding"
    );
    for finding in &findings {
        let meta = registry
            .rules()
            .find(|meta| meta.id == finding.code)
            .expect("finding code must match a registered rule");
        assert_eq!(
            finding.evidence, meta.evidence,
            "finding '{}' must carry the evidence class of its rule",
            finding.code
        );
    }
}

// ── Default configuration ─────────────────────────────────────────────────────

#[test]
fn default_config_enables_all_rules() {
    let config = LintConfig::default();
    for id in &[
        "min-wait-loop",
        "duplicate-condition",
        "expensive-loop-check",
        "ongoing-condition-hot-path",
        "repeated-value",
        "while-without-wait",
    ] {
        assert!(
            config.is_enabled(id),
            "rule {id} must be enabled by default"
        );
    }
}

#[test]
fn default_config_returns_true_for_unknown_rule_id() {
    let config = LintConfig::default();
    assert!(
        config.is_enabled("nonexistent-rule"),
        "unknown rule IDs are enabled by default (opt-in disabled)"
    );
}

// ── Enable / disable ─────────────────────────────────────────────────────────

#[test]
fn disabled_rule_produces_no_findings() {
    let program = corpus_program("synthetic/control-flow");
    let mut config = LintConfig::default();
    config.disable("min-wait-loop");

    let findings = LintRegistry::default().run(&program, &config);
    assert!(
        findings.iter().all(|f| f.code != "min-wait-loop"),
        "disabled rule must produce no findings"
    );
    // Other rules still run (no findings expected from this fixture for the
    // remaining two rules, but the important thing is the run completed).
}

#[test]
fn disabled_rule_does_not_suppress_other_rules() {
    // expensive-loop fixture fires expensive-loop-check; disabling another
    // rule must leave it unaffected.
    let program = local_program("expensive-loop");
    let mut config = LintConfig::default();
    config.disable("min-wait-loop");
    config.disable("duplicate-condition");

    let findings = LintRegistry::default().run(&program, &config);
    assert!(
        findings.iter().any(|f| f.code == "expensive-loop-check"),
        "expensive-loop-check must still fire when other rules are disabled"
    );
}

#[test]
fn re_enabled_rule_fires_again() {
    let program = corpus_program("synthetic/control-flow");
    let mut config = LintConfig::default();
    config.disable("min-wait-loop");
    config.enable("min-wait-loop");

    let findings = LintRegistry::default().run(&program, &config);
    assert!(
        findings.iter().any(|f| f.code == "min-wait-loop"),
        "re-enabled rule must fire again"
    );
}

#[test]
fn all_rules_disabled_produces_empty_findings() {
    let program = corpus_program("synthetic/control-flow");
    let mut config = LintConfig::default();
    for rule in LintRegistry::default().rules() {
        config.disable(rule.id);
    }

    let findings = LintRegistry::default().run(&program, &config);
    assert!(
        findings.is_empty(),
        "all rules disabled must yield no findings"
    );
}

// ── Severity override ─────────────────────────────────────────────────────────

#[test]
fn severity_override_replaces_finding_severity() {
    // expensive-loop-check defaults to Info; override to Warning.
    let program = local_program("expensive-loop");
    let mut config = LintConfig::default();
    config.set_severity("expensive-loop-check", Severity::Warning);

    let findings = LintRegistry::default().run(&program, &config);
    let exp = findings
        .iter()
        .find(|f| f.code == "expensive-loop-check")
        .expect("expensive-loop-check must fire");
    assert_eq!(
        exp.severity,
        Severity::Warning,
        "severity override must replace the rule default"
    );
}

#[test]
fn severity_override_does_not_affect_other_rules() {
    let program = corpus_program("synthetic/control-flow");
    let mut config = LintConfig::default();
    config.set_severity("expensive-loop-check", Severity::Warning);

    let findings = LintRegistry::default().run(&program, &config);
    for finding in findings.iter().filter(|f| f.code == "min-wait-loop") {
        assert_eq!(
            finding.severity,
            Severity::Warning,
            "min-wait-loop severity must be its own default, not the override"
        );
    }
}

#[test]
fn effective_severity_uses_override_when_set() {
    let registry = LintRegistry::default();
    let meta = registry
        .rules()
        .find(|m| m.id == "expensive-loop-check")
        .unwrap();

    let mut config = LintConfig::default();
    assert_eq!(
        config.effective_severity(meta),
        Severity::Info,
        "no override: must return the default severity"
    );
    config.set_severity("expensive-loop-check", Severity::Warning);
    assert_eq!(
        config.effective_severity(meta),
        Severity::Warning,
        "after override: must return the configured severity"
    );
}

#[test]
fn set_severity_by_name_accepts_cli_spellings_and_rejects_unknown_labels() {
    let registry = LintRegistry::default();
    let meta = registry
        .rules()
        .find(|m| m.id == "expensive-loop-check")
        .unwrap();

    let mut config = LintConfig::default();
    assert!(
        config.set_severity_by_name("expensive-loop-check", "warn"),
        "'warn' is a known severity label"
    );
    assert_eq!(
        config.effective_severity(meta),
        Severity::Warning,
        "the CLI spelling must override the default severity"
    );
    assert!(
        !config.set_severity_by_name("expensive-loop-check", "fatal"),
        "'fatal' is not a known severity label"
    );
    assert_eq!(
        config.effective_severity(meta),
        Severity::Warning,
        "an unknown label must leave the configuration unchanged"
    );
}

#[test]
fn severity_policy_supports_off_warn_and_error() {
    let program = local_program("expensive-loop");
    let mut registry = LintRegistry::default();
    registry
        .load_yaml_str(
            &minimum_wait_yaml("community/minimum-wait"),
            &Catalog::builtin().unwrap(),
        )
        .expect("rule loads for severity policy execution");

    let mut config = LintConfig::default();
    assert!(config.set_severity_by_name("community/minimum-wait", "error"));
    assert_eq!(
        registry
            .run(&program, &config)
            .into_iter()
            .find(|finding| finding.code == "community/minimum-wait")
            .map(|finding| finding.severity),
        Some(Severity::Error)
    );

    assert!(config.set_severity_by_name("community/minimum-wait", "off"));
    assert!(
        registry
            .run(&program, &config)
            .into_iter()
            .all(|finding| finding.code != "community/minimum-wait")
    );
}

// ── Determinism ───────────────────────────────────────────────────────────────

#[test]
fn registry_run_is_deterministic() {
    let program = corpus_program("synthetic/control-flow");
    let config = LintConfig::default();
    let registry = LintRegistry::default();

    let first = registry.run(&program, &config);
    let second = registry.run(&program, &config);
    assert_eq!(
        first.len(),
        second.len(),
        "finding count must be identical across runs"
    );
    for (a, b) in first.iter().zip(second.iter()) {
        assert_eq!(a.code, b.code, "finding codes must be stable");
        assert_eq!(a.severity, b.severity, "finding severities must be stable");
        assert_eq!(a.rule, b.rule, "finding rule IDs must be stable");
        assert_eq!(a.span, b.span, "finding spans must be stable");
    }
}

// ── Rule-ID / finding-code consistency ───────────────────────────────────────

#[test]
fn finding_codes_match_registered_rule_ids() {
    let registry = LintRegistry::default();
    let config = LintConfig::default();
    let program = corpus_program("synthetic/control-flow");
    let registered_ids: Vec<&str> = registry.rules().map(|m| m.id).collect();

    for finding in registry.run(&program, &config) {
        assert!(
            registered_ids.contains(&finding.code.as_str()),
            "finding code '{}' does not match any registered rule ID",
            finding.code
        );
    }
}

#[test]
fn findings_carry_source_spans() {
    let program = corpus_program("synthetic/control-flow");
    let config = LintConfig::default();

    let findings = LintRegistry::default().run(&program, &config);
    for finding in &findings {
        assert!(
            finding.span.is_some(),
            "rule '{}' finding must carry a source span",
            finding.code
        );
    }
}

#[test]
fn declarative_rule_matches_canonical_nested_actions_and_exposes_metadata() {
    let catalog = Catalog::builtin().expect("built-in catalog");
    let mut registry = LintRegistry::default();
    registry
        .load_yaml_str(
            r#"
id: community/minimum-wait
metadata:
  summary: loop contains a minimum wait
  rationale: minimum waits can create high-frequency loops
  documentation: Finds a minimum wait inside a while scope.
  known-limits: This is a structural fact and does not measure runtime cost.
  tags: [performance]
matcher:
  scope: while
  actions:
    - kind: call
      name: Wait
      args:
        - number: 0.1
      count:
        min: 1
"#,
            &catalog,
        )
        .expect("declarative rule loads");

    let findings = registry.run(&local_program("expensive-loop"), &LintConfig::default());
    let finding = findings
        .iter()
        .find(|finding| finding.code == "community/minimum-wait")
        .expect("external rule finds the nested wait action");
    assert_eq!(finding.severity, Severity::Warning);
    assert!(
        finding.span.is_some(),
        "external findings preserve source spans"
    );
    let descriptor = registry
        .descriptors(&LintConfig::default())
        .into_iter()
        .find(|rule| rule.id == "community/minimum-wait")
        .expect("external metadata is queryable");
    assert_eq!(descriptor.kind, "declarative");
    assert_eq!(
        descriptor.rationale,
        "minimum waits can create high-frequency loops"
    );

    let mut canonical_registry = LintRegistry::default();
    canonical_registry
        .load_yaml_str(
            &definition_yaml("community/canonical-wait", "wait"),
            &catalog,
        )
        .expect("canonical Workshop identity loads");
    let canonical_findings = canonical_registry
        .run(&local_program("expensive-loop"), &LintConfig::default())
        .into_iter()
        .filter(|finding| finding.code == "community/canonical-wait")
        .count();
    assert_eq!(
        canonical_findings, 1,
        "canonical and localized spellings match"
    );
}

#[test]
fn declarative_rule_matches_named_parameters_and_numeric_comparisons() {
    let catalog = Catalog::builtin().expect("built-in catalog");
    let mut registry = LintRegistry::default();
    registry
        .load_yaml_str(
            r#"
id: community/wait-duration
metadata:
  summary: loop contains a long wait
  rationale: verify action parameter matching
  documentation: Finds a wait with a duration at least one tenth of a second.
  known-limits: This is a structural fact.
  tags: [performance]
matcher:
  scope: while
  actions:
    - kind: call
      name: Wait
      parameters:
        - name: duration
          comparison:
            operator: ">="
            value:
              number: 0.1
      count:
        min: 1
"#,
            &catalog,
        )
        .expect("named parameter rule loads");

    let findings = registry.run(&local_program("expensive-loop"), &LintConfig::default());
    assert_eq!(
        findings
            .iter()
            .filter(|finding| finding.code == "community/wait-duration")
            .count(),
        1
    );
}

#[test]
fn declarative_conditions_are_limited_to_the_selected_scope() {
    let catalog = Catalog::builtin().expect("built-in catalog");
    let mut registry = LintRegistry::default();
    registry
        .load_yaml_str(
            r#"
id: community/outer-loop-comparison
metadata:
  summary: outer loop has a comparison condition
  rationale: verify scoped condition matching
  documentation: Finds a comparison directly belonging to a while scope.
  known-limits: This is a structural fact.
  tags: [correctness]
matcher:
  scope: while
  conditions:
    call:
      name: "<"
    count:
      min: 1
"#,
            &catalog,
        )
        .expect("scoped condition rule loads");

    let findings = registry.run(&local_program("expensive-loop"), &LintConfig::default());
    assert!(
        findings
            .iter()
            .all(|finding| finding.code != "community/outer-loop-comparison"),
        "a while matcher must not count conditions from nested if scopes"
    );
}

#[test]
fn declarative_rule_rejects_reserved_or_unscoped_ids_and_unknown_spellings() {
    let catalog = Catalog::builtin().expect("built-in catalog");
    let definition = |id: &str, name: &str| {
        format!(
            r#"
id: {id}
metadata:
  summary: summary
  rationale: rationale
  documentation: documentation
  known-limits: limits
  tags: [correctness]
matcher:
  actions:
    - kind: call
      name: {name}
"#
        )
    };
    for id in ["bare-id", "wright/rule", "a/b/c"] {
        let error = registry_error(&definition(id, "Wait"), &catalog);
        assert!(error.to_string().contains("external rule ID"));
    }
    let error = registry_error(
        &definition("community/rule", "NotAWorkshopAction"),
        &catalog,
    );
    assert!(error.to_string().contains("unknown action spelling"));
}

#[test]
fn lint_config_yaml_controls_external_rules_and_bounded_options() {
    let config = LintConfig::from_yaml_str(
        r#"
rules:
  community/minimum-wait:
    enabled: true
    severity: warn
    options:
      max-matches: 2
"#,
    )
    .expect("project lint YAML parses");
    assert!(config.is_enabled("community/minimum-wait"));
    assert_eq!(
        config.options("community/minimum-wait").max_matches,
        Some(2)
    );

    let mut registry = LintRegistry::default();
    registry
        .load_yaml_str(
            &minimum_wait_yaml("community/minimum-wait"),
            &Catalog::builtin().unwrap(),
        )
        .expect("rule loads for config execution");
    let findings = registry.run(&local_program("expensive-loop"), &config);
    assert_eq!(
        findings
            .iter()
            .find(|finding| finding.code == "community/minimum-wait")
            .map(|finding| finding.severity),
        Some(Severity::Warning)
    );

    let invalid_options = LintConfig::from_yaml_str(
        "rules:\n  community/minimum-wait:\n    options:\n      unsupported: true\n",
    );
    assert!(invalid_options.is_err(), "unknown options are rejected");
}

fn minimum_wait_yaml(id: &str) -> String {
    definition_yaml(id, "Wait")
}

fn definition_yaml(id: &str, name: &str) -> String {
    format!(
        r#"
id: {id}
metadata:
  summary: loop contains a minimum wait
  rationale: minimum waits can create high-frequency loops
  documentation: Finds a minimum wait inside a while scope.
  known-limits: This is a structural fact and does not measure runtime cost.
  tags: [performance]
matcher:
  scope: while
  actions:
    - kind: call
      name: {name}
      args:
        - number: 0.1
      count:
        min: 1
"#
    )
}

fn registry_error(yaml: &str, catalog: &Catalog) -> wright_analyzer::registry::RuleRegistryError {
    let mut registry = LintRegistry::default();
    registry
        .load_yaml_str(yaml, catalog)
        .expect_err("rule must reject")
}
