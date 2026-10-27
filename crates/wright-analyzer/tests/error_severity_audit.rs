//! Error-severity audit enforcement (#1010): any `error`-severity lint
//! finding yields `ok: false`/`exit: 1`, and lint correctness is not
//! guaranteed, so `error` is reserved for findings confirmed by runtime
//! evaluation — only a `runtime-validated` rule may emit it.

use wright_analyzer::registry::LintRegistry;

fn first_party_rule(evidence: Option<&str>) -> String {
    let evidence = evidence
        .map(|evidence| format!("  evidence: {evidence}\n"))
        .unwrap_or_default();
    format!(
        "id: wright/audited\nmetadata:\n  summary: s\n  rationale: r\n  documentation: d\n  known-limits: k\n  tags: []\n{evidence}  severity: error\nmatcher: {{}}\n"
    )
}

#[test]
fn first_party_error_severity_requires_runtime_validated_evidence() {
    let mut registry = LintRegistry::default();
    for (evidence, accepted) in [
        (Some("runtime-validated"), true),
        (Some("exact"), false),
        (Some("static-indicator"), false),
        (Some("heuristic"), false),
        (None, false),
    ] {
        let result = registry.load_first_party_yaml_str(&first_party_rule(evidence));
        assert_eq!(
            result.is_ok(),
            accepted,
            "{evidence:?} with severity error: {result:?}"
        );
        if result.is_ok() {
            // Re-load under a fresh registry for the next rejected case.
            registry = LintRegistry::default();
        }
    }
    // Non-error severities remain unrestricted.
    let mut registry = LintRegistry::default();
    registry
        .load_first_party_yaml_str(
            "id: wright/audited-warn\nmetadata:\n  summary: s\n  rationale: r\n  documentation: d\n  known-limits: k\n  tags: []\n  evidence: heuristic\n  severity: warning\nmatcher: {}\n",
        )
        .unwrap();
}
