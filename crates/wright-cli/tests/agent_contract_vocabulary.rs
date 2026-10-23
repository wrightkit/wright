use std::path::{Path, PathBuf};

use jsonschema::validator_for;
use serde_json::{Value, json};
use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceKind};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn service_response(request: &ToolRequest) -> ToolResponse {
    let input = workspace_root().join("tests/fixtures/workshop/synthetic/control-flow.ws");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("session starts");
    let mut service = ToolService::new(&mut session).expect("service loads the project");
    service.handle(request)
}

#[test]
fn lookup_result_schema_covers_callable_returns_and_variadic() {
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(workspace_root().join("schemas/wright-agent-v1.schema.json"))
            .unwrap(),
    )
    .unwrap();
    let mut document = schema.clone();
    document["$ref"] = Value::String("#/$defs/LookupResult".to_string());
    let result_schema = validator_for(&document).expect("result schema compiles");

    let ToolResponse::Ok { result } = service_response(&ToolRequest::Lookup {
        language: "workshop".to_string(),
        query: Some("array".to_string()),
        kind: None,
        within: None,
        locale: None,
        limit: Some(10),
    }) else {
        panic!("lookup failed");
    };
    let entries = result["entries"].as_array().unwrap();
    let array = entries
        .iter()
        .find(|entry| entry["identity"] == "array")
        .expect("the array value");
    assert_eq!(array["callable"]["variadic"], true);
    assert_eq!(array["callable"]["returns"], "Array");
    assert!(
        result_schema.is_valid(&result),
        "LookupResult rejected a callable carrying returns/variadic: {result}"
    );
}

#[test]
fn list_entries_request_and_result_validate_against_the_committed_schema() {
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(workspace_root().join("schemas/wright-agent-v1.schema.json"))
            .unwrap(),
    )
    .unwrap();
    let mut request = schema.clone();
    request["$ref"] = Value::String("#/$defs/ListEntriesRequest".to_string());
    let request_schema = validator_for(&request).expect("request schema compiles");
    let valid = json!({"op":"listEntries","language":"workshop","kind":"enumMember","within":"Hero","pageSize":500,"cursor":"a.b"});
    assert!(request_schema.is_valid(&valid));
    for invalid in [
        json!({"op":"listEntries","language":"workshop"}),
        json!({"op":"listEntries","language":"workshop","kind":"setting"}),
        json!({"op":"listEntries","language":"workshop","kind":"action","pageSize":0}),
        json!({"op":"listEntries","language":"workshop","kind":"action","pageSize":501}),
        json!({"op":"listEntries","language":"workshop","kind":"action","query":"wait"}),
    ] {
        assert!(!request_schema.is_valid(&invalid), "{invalid}");
    }

    let mut result_document = schema.clone();
    result_document["$ref"] = Value::String("#/$defs/ListEntriesResult".to_string());
    let result_schema = validator_for(&result_document).expect("result schema compiles");
    let ToolResponse::Ok { result } = service_response(&ToolRequest::ListEntries {
        language: "workshop".to_string(),
        kind: "action".to_string(),
        within: None,
        locale: None,
        page_size: Some(10),
        cursor: None,
    }) else {
        panic!("listEntries failed");
    };
    assert!(result["nextCursor"].is_string());
    assert!(
        result_schema.is_valid(&result),
        "ListEntriesResult rejected a live result: {result}"
    );
}
