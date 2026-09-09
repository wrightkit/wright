//! Native Workshop input integration tests (#36): localized Workshop text
//! drives the existing rule/symbol/reference/usage/CFG/finding queries and
//! the read-only semantic service, with Workshop-origin metadata and usable
//! source spans.

use std::path::{Path, PathBuf};

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
