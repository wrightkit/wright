//! Native Workshop input integration tests (#36): localized Workshop text
//! drives the existing rule/symbol/reference/usage/CFG/finding queries and
//! the read-only tool interface, with Workshop-origin metadata and usable
//! source spans.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::parser;
use wright_analyzer::service::SemanticService;

fn oracle_path(fixture_id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures")
        .join(fixture_id)
        .join("oracle.json")
}

fn corpus_text(fixture_id: &str) -> String {
    let oracle = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(oracle_path(fixture_id)).unwrap(),
    )
    .unwrap();
    oracle["compile"]["workshop"].as_str().unwrap().to_string()
}

fn workshop_service(fixture_id: &str) -> SemanticService<'static> {
    // SAFETY-free approach: the service borrows the program; keep both in a
    // leaked box for the test scope. The catalog supplies the canonical
    // expected enum domains (e.g. Create HUD Text's Reevaluation argument is
    // HudReeval), resolving bare members that are ambiguous across the
    // catalog's enum domains (#118).
    let text = corpus_text(fixture_id);
    let catalog = Catalog::builtin().unwrap();
    let program = parser::parse_with_context(&text, &catalog, &Locale::new("en-US"), &catalog)
        .unwrap_or_else(|error| panic!("{fixture_id} must parse: {error}"));
    let program = Box::leak(Box::new(program));
    SemanticService::from_workshop(program, "en-US").unwrap()
}

#[test]
fn workshop_input_runs_all_semantic_queries() {
    let service = workshop_service("synthetic/control-flow");
    let requests = [
        r#"{"op":"program"}"#,
        r#"{"op":"listRules"}"#,
        r#"{"op":"getRule","rule":0}"#,
        r#"{"op":"findReferences","symbol":0}"#,
        r#"{"op":"getUsage","symbol":0}"#,
        r#"{"op":"getCfg","rule":1}"#,
        r#"{"op":"getFindings"}"#,
    ];
    for request in requests {
        let response: Value = serde_json::from_str(&service.handle_json(request)).unwrap();
        assert!(
            response.get("error").is_none(),
            "{request} must succeed: {response}"
        );
    }

    // Origin metadata identifies the Workshop source and locale.
    let program_response: Value =
        serde_json::from_str(&service.handle_json(r#"{"op":"program"}"#)).unwrap();
    assert_eq!(program_response["result"]["origin"]["kind"], "workshop");
    assert_eq!(program_response["result"]["origin"]["locale"], "en-us");
}

#[test]
fn workshop_input_analysis_findings_are_available() {
    let service = workshop_service("synthetic/control-flow");
    let response: Value =
        serde_json::from_str(&service.handle_json(r#"{"op":"getFindings"}"#)).unwrap();
    let findings = response["result"].as_array().unwrap();
    assert!(
        findings
            .iter()
            .any(|finding| finding["code"] == "min-wait-loop"),
        "the Workshop-origin bounded-while is a hot loop"
    );
    for finding in findings {
        assert!(
            finding["span"].is_object(),
            "findings must carry Workshop source spans: {finding}"
        );
    }
}

#[test]
fn workshop_input_references_preserve_source_spans() {
    let service = workshop_service("synthetic/declarations-rules");
    // hasStarted is symbol 1 (after score).
    let response: Value =
        serde_json::from_str(&service.handle_json(r#"{"op":"findReferences","symbol":1}"#))
            .unwrap();
    let references = response["result"].as_array().unwrap();
    assert!(
        references
            .iter()
            .any(|reference| reference["kind"] == "write" && reference["span"].is_object()),
        "workshop input references must carry usable spans: {references:?}"
    );
}

#[test]
fn external_tool_serves_workshop_input_with_origin_metadata() {
    let bin = env!("CARGO_BIN_EXE_wright-tool");
    let dir = std::env::temp_dir().join("wright-tool-workshop-test");
    std::fs::create_dir_all(&dir).unwrap();
    let workshop = dir.join("program.txt");
    std::fs::write(&workshop, corpus_text("synthetic/basic-rule")).unwrap();

    let mut child = Command::new(bin)
        .args(["--workshop"])
        .arg(&workshop)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawns");
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, r#"{{"op":"program"}}"#).unwrap();
    writeln!(stdin, r#"{{"op":"listRules"}}"#).unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "wright-tool failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["result"]["origin"]["kind"], "workshop");
    assert_eq!(lines[0]["result"]["origin"]["locale"], "en-us");
    let rules = lines[1]["result"].as_array().unwrap();
    assert_eq!(rules[0]["name"], "setup");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn external_tool_honors_explicit_locale_override() {
    let bin = env!("CARGO_BIN_EXE_wright-tool");
    let dir = std::env::temp_dir().join("wright-tool-workshop-locale");
    std::fs::create_dir_all(&dir).unwrap();
    let workshop = dir.join("program.txt");
    std::fs::write(&workshop, corpus_text("synthetic/basic-rule")).unwrap();

    let mut child = Command::new(bin)
        .arg("--workshop")
        .arg(&workshop)
        .args(["--locale", "en-US"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawns");
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, r#"{{"op":"version"}}"#).unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}
