//! Native Workshop input integration tests (#36): localized Workshop text
//! drives the existing rule/symbol/reference/usage/CFG/finding queries and
//! the read-only semantic service, with Workshop-origin metadata and usable
//! source spans.

use std::path::{Path, PathBuf};

use serde_json::Value;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::parser;
use wright_analyzer::service::SemanticService;

fn workshop_path(fixture_id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/workshop")
        .join(fixture_id)
        .with_extension("ws")
}

fn fixture_text(fixture_id: &str) -> String {
    std::fs::read_to_string(workshop_path(fixture_id)).unwrap()
}

fn workshop_service(fixture_id: &str) -> SemanticService<'static> {
    // SAFETY-free approach: the service borrows the program; keep both in a
    // leaked box for the test scope. The catalog supplies the canonical
    // expected enum domains (e.g. Create HUD Text's Reevaluation argument is
    // HudReeval), resolving bare members that are ambiguous across the
    // catalog's enum domains (#118).
    let text = fixture_text(fixture_id);
    let catalog = Catalog::builtin().unwrap();
    let program = parser::parse_wir_with_context(&text, &catalog, &Locale::new("en-US"), &catalog)
        .unwrap_or_else(|error| panic!("{fixture_id} must parse: {error}"));
    let program = Box::leak(Box::new(program));
    SemanticService::from_workshop(program, "en-US").unwrap()
}

fn query(service: &SemanticService<'_>, request: Value) -> Value {
    let response: Value =
        serde_json::from_str(&service.handle_json(&serde_json::to_string(&request).unwrap()))
            .unwrap();
    assert!(
        response.get("error").is_none(),
        "query must succeed: {response}"
    );
    response["result"].clone()
}

fn id_for(service: &SemanticService<'_>, request: Value, name: &str) -> u32 {
    query(service, request)
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == name)
        .and_then(|item| item["id"].as_u64())
        .unwrap_or_else(|| panic!("query result has no item named {name}")) as u32
}

#[test]
fn workshop_input_runs_all_semantic_queries() {
    let service = workshop_service("synthetic/control-flow");
    let rule = id_for(
        &service,
        serde_json::json!({"op": "listRules"}),
        "bounded while",
    );
    let symbol = id_for(&service, serde_json::json!({"op": "listSymbols"}), "index");
    let requests = [
        serde_json::json!({"op": "program"}),
        serde_json::json!({"op": "listRules"}),
        serde_json::json!({"op": "getRule", "rule": rule}),
        serde_json::json!({"op": "findReferences", "symbol": symbol}),
        serde_json::json!({"op": "getUsage", "symbol": symbol}),
        serde_json::json!({"op": "getCfg", "rule": rule}),
        serde_json::json!({"op": "getFindings"}),
    ];
    for request in requests {
        query(&service, request);
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
    let symbol = id_for(
        &service,
        serde_json::json!({"op": "listSymbols"}),
        "hasStarted",
    );
    let references = query(
        &service,
        serde_json::json!({"op": "findReferences", "symbol": symbol}),
    );
    let references = references.as_array().unwrap();
    assert!(
        references
            .iter()
            .any(|reference| reference["kind"] == "write" && reference["span"].is_object()),
        "workshop input references must carry usable spans: {references:?}"
    );
}
