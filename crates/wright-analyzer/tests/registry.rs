//! Registry contract tests (#97): stable rule identity, metadata, deterministic
//! configuration (enable/disable/severity-override), and execution determinism.

use std::path::{Path, PathBuf};

use wright_analyzer::analysis::Severity;
use wright_analyzer::registry::{LintConfig, LintRegistry};
use wright_core::hir;
use wright_ir::lower;
use wright_ir::wir::Program as WirProgram;

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
fn registry_has_three_first_party_rules_with_stable_ids() {
    let registry = LintRegistry::default();
    let ids: Vec<&str> = registry.rules().map(|meta| meta.id).collect();
    assert_eq!(ids.len(), 3, "exactly three first-party rules in M12");
    assert!(ids.contains(&"min-wait-loop"), "min-wait-loop present");
    assert!(
        ids.contains(&"duplicate-condition"),
        "duplicate-condition present"
    );
    assert!(
        ids.contains(&"expensive-loop-check"),
        "expensive-loop-check present"
    );
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
}

// ── Default configuration ─────────────────────────────────────────────────────

#[test]
fn default_config_enables_all_rules() {
    let config = LintConfig::default();
    for id in &[
        "min-wait-loop",
        "duplicate-condition",
        "expensive-loop-check",
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
    config.disable("min-wait-loop");
    config.disable("duplicate-condition");
    config.disable("expensive-loop-check");

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
            registered_ids.contains(&finding.code),
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
