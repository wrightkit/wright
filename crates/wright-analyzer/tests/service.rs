//! Integration tests for the read-only agent/tool interface (#26): the JSON
//! request/response contract exercised in-process.

use std::path::{Path, PathBuf};

use serde_json::Value;

use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::parser;
use workshop_rs::wir::Program as WirProgram;
use wright_analyzer::analysis::Severity;
use wright_analyzer::registry::LintConfig;
use wright_analyzer::service::SemanticService;

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

/// Drive the in-process service over a fixture and return responses keyed by
/// request.
fn in_process_responses(fixture_id: &str, requests: &[&str]) -> Vec<Value> {
    let protocol =
        wright_core::hir::parse_str(&std::fs::read_to_string(fixture_path(fixture_id)).unwrap())
            .unwrap();
    let model = protocol.to_ir().unwrap();
    let program = wright_ir::lower::lower(&model).unwrap();
    let service = SemanticService::new(&program).unwrap();
    requests
        .iter()
        .map(|request| serde_json::from_str(&service.handle_json(request)).unwrap())
        .collect()
}

fn lowered_program(path: &Path) -> WirProgram {
    let protocol = wright_core::hir::parse_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let model = protocol.to_ir().unwrap();
    wright_ir::lower::lower(&model).unwrap()
}

fn real_world_program(fixture_id: &str) -> WirProgram {
    let oracle_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures")
        .join(fixture_id)
        .join("oracle.json");
    let oracle: Value =
        serde_json::from_str(&std::fs::read_to_string(oracle_path).unwrap()).unwrap();
    let workshop = oracle["compile"]["workshop"].as_str().unwrap();
    let catalog = Catalog::builtin().unwrap();
    parser::parse_with_context(workshop, &catalog, &Locale::new("en-US"), &catalog).unwrap()
}

/// Build a service over a lowered program with an explicit lint config.
fn service_with_config(program: &WirProgram, config: LintConfig) -> SemanticService<'_> {
    SemanticService::with_origin_and_config(
        program,
        wright_analyzer::service::Origin {
            kind: "protocol".to_string(),
            locale: None,
        },
        config,
    )
    .unwrap()
}

/// Handle one request over a service and return the parsed JSON response.
fn handle(service: &SemanticService<'_>, request: &str) -> Value {
    serde_json::from_str(&service.handle_json(request)).unwrap()
}

#[test]
fn version_reports_identity_and_capabilities() {
    let responses = in_process_responses("synthetic/control-flow", &[r#"{"op":"version"}"#]);
    let result = &responses[0]["result"];
    assert_eq!(result["name"], "wright-tool");
    assert_eq!(result["version"], env!("CARGO_PKG_VERSION"));
    let capabilities: Vec<&str> = result["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert!(capabilities.contains(&"findings"));
    assert!(capabilities.contains(&"persistentObjects"));
}

#[test]
fn persistent_object_query_exposes_reevaluation_without_emitting_lints() {
    let responses = in_process_responses(
        "synthetic/control-flow",
        &[
            r#"{"op":"getPersistentObjects"}"#,
            r#"{"op":"getFindings"}"#,
        ],
    );
    let objects = responses[0]["result"].as_array().unwrap();
    assert!(!objects.is_empty(), "fixture creates HUD text");
    let object = &objects[0];
    assert_eq!(object["kind"], "hud-text");
    assert_eq!(object["visibility"], "all-players");
    assert_eq!(object["reevaluation"]["domain"], "HudReeval");
    assert_eq!(object["reevaluation"]["mode"], "VISIBILITY_AND_STRING");
    assert!(object["span"].is_object(), "facts preserve provenance");
    assert!(
        responses[1]["result"]
            .as_array()
            .unwrap()
            .iter()
            .all(|finding| finding["code"] != "persistent-object-lifecycle")
    );
}

#[test]
fn persistent_object_query_keeps_real_project_observations_out_of_lints() {
    let program = real_world_program("real-world/overpy-broken-weapons");
    let service = SemanticService::from_workshop(&program, "en-US").unwrap();
    let objects = handle(&service, r#"{"op":"getPersistentObjects"}"#);
    let objects = objects["result"].as_array().unwrap();
    let object = objects
        .iter()
        .find(|object| object["kind"] == "hud-text")
        .expect("the real Workshop project creates persistent HUD text");
    assert!(
        object["span"].is_object(),
        "creation provenance is preserved"
    );
    assert_eq!(object["executionScope"], "per-player");
    assert_eq!(object["visibility"], "all-players");
    let findings = handle(&service, r#"{"op":"getFindings"}"#);
    assert!(
        findings["result"]
            .as_array()
            .unwrap()
            .iter()
            .all(|finding| finding["code"] != "persistent-object-lifecycle")
    );
}

#[test]
fn program_summary_is_structured_and_deterministic() {
    let responses = in_process_responses(
        "synthetic/declarations-rules",
        &[r#"{"op":"program"}"#, r#"{"op":"program"}"#],
    );
    assert_eq!(
        responses[0], responses[1],
        "responses must be deterministic"
    );
    let result = &responses[0]["result"];
    assert_eq!(result["rules"], 2);
    assert_eq!(result["globalVariables"], 1);
    assert_eq!(result["playerVariables"], 1);
    assert_eq!(result["subroutines"], 1);
}

#[test]
fn rule_lookup_and_symbol_queries_return_stable_ids() {
    let responses = in_process_responses(
        "synthetic/declarations-rules",
        &[
            r#"{"op":"listRules"}"#,
            r#"{"op":"getRule","rule":1}"#,
            r#"{"op":"listSymbols"}"#,
            r#"{"op":"getSymbol","symbol":3}"#,
        ],
    );
    let rules = responses[0]["result"].as_array().unwrap();
    assert_eq!(rules.len(), 2);
    // Reference ordering: subroutine definition rules come first.
    assert_eq!(rules[0]["name"], "Subroutine showStatus");
    assert_eq!(rules[1]["name"], "player starts");

    let rule = &responses[1]["result"];
    assert_eq!(rule["name"], "player starts");
    assert!(rule["span"].is_object(), "rule lookup must include a span");

    let symbols = responses[2]["result"].as_array().unwrap();
    assert_eq!(
        symbols.len(),
        5,
        "1 global + 1 player + 1 subroutine + 2 rules"
    );
    assert_eq!(symbols[0]["kind"], "globalVariable");
    assert_eq!(symbols[0]["name"], "score");

    let symbol = &responses[3]["result"];
    assert_eq!(symbol["kind"], "rule");
    // Reference ordering: the subroutine definition rule precedes normal
    // rules, so its symbol precedes "player starts".
    assert_eq!(symbol["name"], "Subroutine showStatus");
}

#[test]
fn find_references_and_usage_are_linked_to_locations() {
    // showStatus is symbol 2 (after score and hasStarted).
    let responses = in_process_responses(
        "synthetic/declarations-rules",
        &[
            r#"{"op":"findReferences","symbol":2}"#,
            r#"{"op":"getUsage","symbol":2}"#,
        ],
    );
    let references = responses[0]["result"].as_array().unwrap();
    assert_eq!(references.len(), 3, "declaration + call + definition");
    let kinds: Vec<&str> = references
        .iter()
        .map(|reference| reference["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"call"));
    assert!(kinds.contains(&"definition"));
    assert!(
        references
            .iter()
            .any(|reference| reference["span"].is_object()),
        "references must preserve source locations"
    );

    let usage = &responses[1]["result"];
    assert_eq!(usage["calls"], 1);
    assert_eq!(usage["rules"], 2);
}

#[test]
fn cfg_inspection_is_deterministic() {
    let responses = in_process_responses(
        "synthetic/control-flow",
        &[r#"{"op":"getCfg","rule":1}"#, r#"{"op":"getCfg","rule":1}"#],
    );
    assert_eq!(responses[0], responses[1]);
    let result = &responses[0]["result"];
    let blocks = result["blocks"].as_array().unwrap();
    assert!(!blocks.is_empty());
    assert!(
        blocks.iter().any(|block| block["kind"] == "while"),
        "bounded while must appear as a while block"
    );
    assert!(
        blocks.iter().any(|block| block["waits"] == true),
        "the wait action must flag a block"
    );
}

#[test]
fn findings_are_returned_with_codes_and_spans() {
    let responses = in_process_responses("synthetic/control-flow", &[r#"{"op":"getFindings"}"#]);
    let findings = responses[0]["result"].as_array().unwrap();
    assert!(
        findings
            .iter()
            .any(|finding| finding["code"] == "min-wait-loop"),
        "control-flow has a hot loop"
    );
    for finding in findings {
        assert!(
            finding["span"].is_object(),
            "{} must carry a span",
            finding["code"]
        );
        assert!(
            finding["evidence"].is_string(),
            "{} must carry an evidence class",
            finding["code"]
        );
    }
}

#[test]
fn while_without_wait_findings_carry_boundedness_in_json() {
    // The machine-readable boundedness surface (issue #103): `while-without-wait`
    // findings expose the kebab-case class, every other finding exposes null.
    let program = lowered_program(&local_fixture_path("no-yield-bounded"));
    let service = SemanticService::new(&program).unwrap();
    let findings = handle(&service, r#"{"op":"getFindings"}"#);
    let findings = findings["result"].as_array().unwrap();
    let no_wait: Vec<&Value> = findings
        .iter()
        .filter(|finding| finding["code"] == "while-without-wait")
        .collect();
    assert_eq!(
        no_wait.len(),
        5,
        "all bounded counter loops in the fixture fire"
    );
    for finding in no_wait {
        assert_eq!(
            finding["boundedness"], "statically-bounded",
            "the boundedness field must serialize as the kebab-case class"
        );
        assert_eq!(finding["severity"], "info");
    }
    for finding in findings
        .iter()
        .filter(|finding| finding["code"] != "while-without-wait")
    {
        assert!(
            finding["boundedness"].is_null(),
            "non-while findings carry boundedness: null"
        );
    }
}

// ── Lint rules and configuration (#98) ───────────────────────────────────────

#[test]
fn lint_rules_reports_rule_metadata_and_effective_config() {
    let responses = in_process_responses(
        "synthetic/control-flow",
        &[r#"{"op":"lintRules"}"#, r#"{"op":"lintRules"}"#],
    );
    assert_eq!(
        responses[0], responses[1],
        "lintRules must be deterministic across calls"
    );
    let result = &responses[0]["result"];
    let rules = result["rules"].as_array().unwrap();
    assert!(!rules.is_empty(), "first-party rules are reported");
    for rule in rules {
        assert!(rule["id"].is_string(), "rules carry stable ids");
        assert!(rule["defaultSeverity"].is_string());
        assert!(rule["effectiveSeverity"].is_string());
        assert_eq!(
            rule["enabled"], true,
            "the default config enables every rule"
        );
        assert!(
            rule["evidence"].is_string(),
            "rules carry an evidence class"
        );
        assert!(
            rule["knownLimits"].is_string(),
            "rules carry documented known limits"
        );
        assert!(rule["tags"].is_array(), "rules carry tags");
    }
    let config_rules = result["config"]["rules"].as_object().unwrap();
    assert_eq!(
        config_rules.len(),
        rules.len(),
        "the config summary covers every rule"
    );
    for rule in rules {
        let id = rule["id"].as_str().unwrap();
        let entry = &config_rules[id];
        assert_eq!(entry["enabled"], true);
        assert_eq!(
            entry["severity"], rule["effectiveSeverity"],
            "config severity agrees with the rule's effective severity"
        );
    }
}

#[test]
fn lint_config_is_applied_to_findings_and_rules() {
    let mut config = LintConfig::default();
    config.disable("min-wait-loop");
    config.set_severity("expensive-loop-check", Severity::Warning);

    // Control-flow fires min-wait-loop: the disabled rule must produce no
    // findings, and lintRules must reflect the configuration.
    let program = lowered_program(&fixture_path("synthetic/control-flow"));
    let service = service_with_config(&program, config.clone());
    let findings = handle(&service, r#"{"op":"getFindings"}"#);
    let findings = findings["result"].as_array().unwrap();
    assert!(
        findings
            .iter()
            .all(|finding| finding["code"] != "min-wait-loop"),
        "the disabled rule must produce no findings"
    );
    let rules = handle(&service, r#"{"op":"lintRules"}"#);
    let rules = rules["result"]["rules"].as_array().unwrap();
    let min_wait = rules
        .iter()
        .find(|rule| rule["id"] == "min-wait-loop")
        .unwrap();
    assert_eq!(
        min_wait["enabled"], false,
        "lintRules reports the disabled rule"
    );
    let exp_loop = rules
        .iter()
        .find(|rule| rule["id"] == "expensive-loop-check")
        .unwrap();
    assert_eq!(
        exp_loop["effectiveSeverity"], "warning",
        "lintRules reports the severity override"
    );

    // The expensive-loop fixture fires expensive-loop-check: its findings
    // must carry the overridden severity.
    let program = lowered_program(&local_fixture_path("expensive-loop"));
    let service = service_with_config(&program, config);
    let findings = handle(&service, r#"{"op":"getFindings"}"#);
    let findings = findings["result"].as_array().unwrap();
    let exp_findings: Vec<&Value> = findings
        .iter()
        .filter(|finding| finding["code"] == "expensive-loop-check")
        .collect();
    assert!(
        !exp_findings.is_empty(),
        "the expensive-loop fixture fires the rule"
    );
    for finding in exp_findings {
        assert_eq!(
            finding["severity"], "warning",
            "findings carry the configured severity"
        );
    }
}

#[test]
fn errors_are_structured() {
    let responses = in_process_responses(
        "synthetic/basic-rule",
        &[
            r#"{"op":"getRule","rule":99}"#,
            r#"{"op":"getUsage","symbol":99}"#,
            r#"{"op":"bogus"}"#,
        ],
    );
    for response in &responses {
        assert!(
            response.get("error").is_some(),
            "expected an error response, got {response}"
        );
        let error = &response["error"];
        assert!(error["code"].is_string());
        assert!(error["message"].is_string());
    }
}
