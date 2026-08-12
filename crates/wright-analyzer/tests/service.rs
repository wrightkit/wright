//! Integration tests for the read-only agent/tool interface (#26): the JSON
//! request/response contract, exercised in-process and through the actual
//! `wright-tool` binary as an external consumer boundary.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

fn fixture_path(fixture_id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../adapter/fixtures")
        .join(format!("{fixture_id}.json"))
}

/// Drive the in-process service over a fixture and return responses keyed by
/// request.
fn in_process_responses(fixture_id: &str, requests: &[&str]) -> Vec<Value> {
    let protocol =
        wright_core::hir::parse_str(&std::fs::read_to_string(fixture_path(fixture_id)).unwrap())
            .unwrap();
    let model = protocol.to_ir().unwrap();
    let program = wright_ir::lower::lower(&model).unwrap();
    let service = wright_analyzer::service::SemanticService::new(&program).unwrap();
    requests
        .iter()
        .map(|request| serde_json::from_str(&service.handle_json(request)).unwrap())
        .collect()
}

#[test]
fn version_reports_identity_and_capabilities() {
    let responses = in_process_responses("synthetic/control-flow", &[r#"{"op":"version"}"#]);
    let result = &responses[0]["result"];
    assert_eq!(result["name"], "wright-tool");
    assert_eq!(result["version"], "0.1.0");
    let capabilities: Vec<&str> = result["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert!(capabilities.contains(&"findings"));
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
    assert_eq!(rules[0]["name"], "player starts");
    assert_eq!(rules[1]["name"], "Subroutine showStatus");

    let rule = &responses[1]["result"];
    assert_eq!(rule["name"], "Subroutine showStatus");
    assert_eq!(rule["event"], "subroutine:showStatus");
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
    assert_eq!(symbol["name"], "player starts");
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

#[test]
fn external_process_inspects_a_compiled_program_without_parsing_source() {
    // The acceptance boundary: an external process drives `wright-tool` over
    // a compiled program and inspects semantic relationships.
    let bin = env!("CARGO_BIN_EXE_wright-tool");
    let requests = [
        r#"{"op":"version"}"#,
        r#"{"op":"program"}"#,
        r#"{"op":"listRules"}"#,
        r#"{"op":"getRule","rule":1}"#,
        r#"{"op":"listSymbols"}"#,
        r#"{"op":"findReferences","symbol":0}"#,
        r#"{"op":"getUsage","symbol":0}"#,
        r#"{"op":"getCfg","rule":1}"#,
        r#"{"op":"getFindings"}"#,
    ];

    let stdout = run_tool(bin, "synthetic/control-flow", &requests);
    let lines: Vec<Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).expect("each response is JSON"))
        .collect();
    assert_eq!(lines.len(), requests.len());

    assert_eq!(lines[0]["result"]["name"], "wright-tool");
    assert_eq!(lines[1]["result"]["rules"], 2);
    let rules = lines[2]["result"].as_array().unwrap();
    assert_eq!(rules[0]["name"], "control flow");
    assert_eq!(lines[3]["result"]["name"], "bounded while");
    assert!(
        lines[4]["result"].as_array().unwrap().len() >= 3,
        "symbols exist"
    );
    assert!(
        lines[5]["result"].as_array().unwrap().len() >= 4,
        "index references exist"
    );
    assert_eq!(lines[6]["result"]["reads"], 6);
    assert!(!lines[7]["result"]["blocks"].as_array().unwrap().is_empty());
    assert!(
        lines[8]["result"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["code"] == "min-wait-loop")
    );

    // Deterministic across processes.
    let second = run_tool(bin, "synthetic/control-flow", &requests);
    assert_eq!(stdout, second, "external responses must be deterministic");
}

#[test]
fn external_process_rejects_missing_program_argument() {
    let bin = env!("CARGO_BIN_EXE_wright-tool");
    let output = Command::new(bin)
        .stdin(Stdio::null())
        .output()
        .expect("spawns");
    assert_eq!(output.status.code(), Some(2));
}

fn run_tool(bin: &str, fixture_id: &str, requests: &[&str]) -> String {
    let mut child = Command::new(bin)
        .arg("--program")
        .arg(fixture_path(fixture_id))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wright-tool spawns");
    let mut stdin = child.stdin.take().expect("stdin");
    for request in requests {
        writeln!(stdin, "{request}").expect("write request");
    }
    drop(stdin);
    let output = child.wait_with_output().expect("wright-tool runs");
    assert!(
        output.status.success(),
        "wright-tool failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("stdout is UTF-8")
}
