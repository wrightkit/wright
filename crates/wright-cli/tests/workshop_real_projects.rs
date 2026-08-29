//! Real-project integration coverage for the pinned Workshop corpus.
//!
//! The source artifacts are owned by `workshop-rs`; this test consumes the
//! owner-provided expectation contract and invokes the actual `wright` binary
//! for both public product commands.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use workshop_rs::real_projects::{
    REAL_PROJECT_EXPECTATION as WORKSHOP_EXPECTATION,
    RealProjectResidualExpectation as WorkshopResidualExpectation,
};
use wright_analyzer::registry::LintRegistry;
use wright_driver::workshop_provider::{diagnostic_code, status_for_classification};

fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

#[test]
#[ignore = "requires the released workshop-rs Workshop corpus checkout"]
fn real_projects_run_check_and_lint_through_wright() {
    let artifact_root = std::env::var_os("WRIGHTKIT_WORKSHOP_CORPUS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("WRIGHTKIT_WORKSHOP_CORPUS_DIR must be set for this test"));
    assert!(
        artifact_root.is_absolute() && artifact_root.is_dir(),
        "WRIGHTKIT_WORKSHOP_CORPUS_DIR must be an absolute directory: {}",
        artifact_root.display()
    );

    for case in WORKSHOP_EXPECTATION.cases {
        let path = find_artifact_by_hash(&artifact_root, case.source_sha256, case.id);
        let (check, check_success) = run_json("check", &path);
        let (lint, lint_success) = run_json("lint", &path);

        let expected_diagnostics = case
            .residuals
            .iter()
            .map(expected_diagnostic)
            .collect::<BTreeSet<_>>();
        let actual_diagnostics = check["diagnostics"]
            .as_array()
            .expect("check diagnostics array")
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic["code"]
                        .as_str()
                        .expect("diagnostic code")
                        .to_string(),
                    diagnostic["status"]
                        .as_str()
                        .expect("diagnostic status")
                        .to_string(),
                )
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            actual_diagnostics, expected_diagnostics,
            "{} check diagnostics",
            case.id
        );
        assert_eq!(
            check["ok"],
            serde_json::Value::Bool(expected_diagnostics.is_empty()),
            "{} check status",
            case.id
        );
        assert_eq!(
            check_success,
            expected_diagnostics.is_empty(),
            "{} check exit status",
            case.id
        );
        assert_eq!(check["command"], "check");

        for diagnostic in check["diagnostics"].as_array().unwrap() {
            assert_eq!(
                diagnostic["span"]["path"],
                path.to_string_lossy().as_ref(),
                "{} diagnostics retain source attribution",
                case.id
            );
        }

        assert_eq!(lint["ok"], check["ok"], "{} lint status", case.id);
        assert_eq!(lint_success, check_success, "{} lint exit status", case.id);
        assert_eq!(lint["command"], "lint");
        assert_eq!(lint["result"]["program"]["origin"]["kind"], "workshop");
        assert_eq!(
            lint["result"]["program"]["origin"]["locale"],
            case.locale.to_ascii_lowercase(),
            "{} locale detection",
            case.id
        );
        assert!(
            lint["result"]["program"]["rules"]
                .as_u64()
                .is_some_and(|rules| rules > 0),
            "{} lint must reach the canonical semantic program",
            case.id
        );
        let lint_rules = lint["result"]["rules"]
            .as_array()
            .expect("lint rules array");
        let expected_rule_ids = LintRegistry::default()
            .rules()
            .map(|rule| rule.id)
            .collect::<Vec<_>>();
        let actual_rule_ids = lint_rules
            .iter()
            .map(|rule| rule["id"].as_str().expect("lint rule id"))
            .collect::<Vec<_>>();
        assert_eq!(
            actual_rule_ids, expected_rule_ids,
            "{} lint must report the authoritative default rule registry",
            case.id
        );
        assert!(
            lint_rules
                .iter()
                .all(|rule| rule["enabled"].as_bool() == Some(true)),
            "{} lint must enable every default rule",
            case.id
        );

        let file_name = path
            .file_name()
            .expect("fixture filename")
            .to_string_lossy();
        let findings = lint["result"]["findings"]
            .as_array()
            .expect("lint findings array");
        let review = findings
            .iter()
            .map(|finding| {
                let code = finding["code"].as_str().expect("finding code");
                let evidence = finding["evidence"].as_str().expect("finding evidence");
                assert!(
                    matches!(evidence, "exact" | "heuristic" | "static-indicator"),
                    "{} finding {code} has unknown evidence class {evidence}",
                    case.id
                );
                assert_eq!(
                    finding["span"]["path"],
                    file_name.as_ref(),
                    "{} finding {code} retains source attribution",
                    case.id
                );
                serde_json::json!({
                    "code": code,
                    "severity": finding["severity"],
                    "evidence": evidence,
                    "span": finding["span"],
                })
            })
            .collect::<Vec<_>>();
        println!(
            "WORKSHOP_CORPUS {}",
            serde_json::json!({
                "corpus": workshop_rs::real_projects::REAL_PROJECT_CORPUS_ID,
                "case": case.id,
                "artifact": path,
                "sourceSha256": case.source_sha256,
                "check": {
                    "ok": check["ok"],
                    "diagnostics": check["diagnostics"],
                },
                "lint": {
                    "ok": lint["ok"],
                    "rules": lint["result"]["rules"].as_array().map(Vec::len),
                    "findings": review,
                },
            })
        );
    }
}

fn run_json(command: &str, path: &Path) -> (serde_json::Value, bool) {
    let output = Command::new(wright())
        .args([command, "--kind", "workshop"])
        .arg(path)
        .args(["-f", "json"])
        .output()
        .unwrap_or_else(|error| panic!("wright {command} failed to start: {error}"));
    match serde_json::from_slice(&output.stdout) {
        Ok(value) => (value, output.status.success()),
        Err(error) => panic!(
            "wright {command} returned invalid JSON (exit {}): {error}; stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

fn expected_diagnostic(residual: &WorkshopResidualExpectation) -> (String, String) {
    (
        diagnostic_code(residual_kind_code(residual.kind), residual.identity),
        serde_json::to_value(status_for_classification(residual.classification))
            .expect("provider status serializes")
            .as_str()
            .expect("provider status is a string")
            .to_string(),
    )
}

fn residual_kind_code(kind: workshop_rs::semantic::IncompletenessKind) -> &'static str {
    match kind {
        workshop_rs::semantic::IncompletenessKind::RawSetting => "raw-setting",
        workshop_rs::semantic::IncompletenessKind::UnknownAction => "unknown-action",
        workshop_rs::semantic::IncompletenessKind::UnknownValue => "unknown-value",
        workshop_rs::semantic::IncompletenessKind::OpaqueAction => "opaque-action",
    }
}

fn find_artifact_by_hash(root: &Path, expected_hash: &str, case_id: &str) -> PathBuf {
    let mut matches = Vec::new();
    collect_artifact_matches(root, expected_hash, &mut matches);
    assert_eq!(
        matches.len(),
        1,
        "{case_id} must resolve exactly one artifact by owner-provided SHA-256, found {}",
        matches.len()
    );
    matches.pop().expect("one artifact match")
}

fn collect_artifact_matches(root: &Path, expected_hash: &str, matches: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(root).unwrap_or_else(|error| {
        panic!(
            "cannot read Workshop corpus directory {}: {error}",
            root.display()
        )
    }) {
        let entry = entry.expect("read Workshop corpus directory entry");
        let path = entry.path();
        let file_type = entry.file_type().expect("inspect Workshop corpus entry");
        if file_type.is_dir() {
            collect_artifact_matches(&path, expected_hash, matches);
        } else if file_type.is_file() {
            let bytes = std::fs::read(&path).unwrap_or_else(|error| {
                panic!(
                    "cannot read Workshop corpus artifact {}: {error}",
                    path.display()
                )
            });
            if wright_driver::sha256_hex(&bytes) == expected_hash {
                matches.push(path);
            }
        }
    }
}
