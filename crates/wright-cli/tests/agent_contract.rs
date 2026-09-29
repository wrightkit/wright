use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use jsonschema::JSONSchema;
use serde_json::{Value, json};
use wright_driver::service::{AGENT_CONTRACT, ToolRequest, ToolResponse, ToolService};
use wright_driver::{CompilerSession, FindingSelection, InputSpec, SessionConfig, SourceKind};

const EXPECTED_V1_OPERATIONS: &[&str] = &[
    "capabilities",
    "compile",
    "check",
    "analyze",
    "inspect",
    "project",
    "rules",
    "symbols",
    "references",
    "usage",
    "cfg",
    "findings",
    "persistentObjects",
    "lint",
    "lintRules",
    "callGraph",
    "costEstimate",
    "targetMetadata",
    "validateEditTransaction",
    "semanticRename",
    "providerSemanticRename",
    "providerValidateEdit",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn capabilities() -> Value {
    let input = workspace_root().join("tests/fixtures/workshop/synthetic/basic-rule.ws");
    let mut child = Command::new(env!("CARGO_BIN_EXE_wright"))
        .args(["serve", "--transport", "stdio"])
        .arg(input)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("wright serve starts");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"op\":\"capabilities\"}\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    serde_json::from_slice::<Value>(output.stdout.split(|byte| *byte == b'\n').next().unwrap())
        .unwrap()["result"]
        .clone()
}

fn requests() -> Vec<Value> {
    vec![
        json!({"op":"capabilities"}),
        json!({"op":"compile"}),
        json!({"op":"check"}),
        json!({"op":"analyze"}),
        json!({"op":"inspect"}),
        json!({"op":"project"}),
        json!({"op":"rules"}),
        json!({"op":"symbols","kind":null}),
        // #429: symbol/rule addresses accept the declared name as well as the
        // numeric id; `service_responses` substitutes discovered ids.
        json!({"op":"references","symbol":"name"}),
        json!({"op":"usage","symbol":"name"}),
        json!({"op":"cfg","rule":"name"}),
        json!({"op":"findings"}),
        json!({"op":"persistentObjects"}),
        json!({"op":"lint"}),
        json!({"op":"lintRules"}),
        json!({"op":"callGraph"}),
        json!({"op":"costEstimate"}),
        json!({"op":"targetMetadata"}),
        json!({
            "op":"validateEditTransaction",
            "sources":{"file:///project.ws":"rule(\"Example\") {}"},
            "transaction":{"edits":[{
                "kind":"edit",
                "source":"file:///project.ws",
                "source_identity":"sha256:example",
                "range":{"start_line":1,"start_col":1,"end_line":1,"end_col":1},
                "new_text":""
            }]}
        }),
        json!({
            "op":"semanticRename",
            "sources":{"file:///project.ws":"rule(\"Example\") {}"},
            "target":{"source":"file:///project.ws","line":1,"col":1,"to":"Renamed"}
        }),
        json!({
            "op":"providerSemanticRename",
            "language_id":"opy",
            "documents":{"file:///project.opy":{
                "uri":"file:///project.opy","languageId":"opy","version":1,"text":"rule(\"Example\") {}"
            }},
            "position_document_uri":"file:///project.opy",
            "position":{"line":0,"character":0},
            "new_name":"Renamed",
            "project_root":null,
            "sources":{"file:///project.opy":"rule(\"Example\") {}"}
        }),
        json!({
            "op":"providerValidateEdit",
            "language_id":"opy",
            "documents":{"file:///project.opy":{
                "uri":"file:///project.opy","languageId":"opy","version":1,"text":"rule(\"Example\") {}"
            }},
            "transaction":{"edits":[{
                "kind":"edit",
                "source":"file:///project.opy",
                "source_identity":"sha256:example",
                "range":{"start_line":1,"start_col":1,"end_line":1,"end_col":1},
                "new_text":""
            }]},
            "sources":{"file:///project.opy":"rule(\"Example\") {}"},
            "project_root":null
        }),
    ]
}

fn service_responses() -> Vec<(String, Value)> {
    let input = workspace_root().join("tests/fixtures/workshop/synthetic/control-flow.ws");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("session starts");
    let mut service = ToolService::new(&mut session).expect("service loads the project");

    let rule = match service.handle(&ToolRequest::Rules) {
        ToolResponse::Ok { result } => result[0]["id"].as_u64().unwrap(),
        ToolResponse::Error { error } => panic!("rules failed: {error:?}"),
    };
    let symbol = match service.handle(&ToolRequest::Symbols { kind: None }) {
        ToolResponse::Ok { result } => result[0]["id"].as_u64().unwrap(),
        ToolResponse::Error { error } => panic!("symbols failed: {error:?}"),
    };

    requests()
        .into_iter()
        .map(|mut request| {
            let operation = request["op"].as_str().unwrap().to_string();
            match operation.as_str() {
                "references" | "usage" => request["symbol"] = json!(symbol),
                "cfg" => request["rule"] = json!(rule),
                _ => {}
            }
            let parsed =
                serde_json::from_value::<ToolRequest>(request).expect("request deserializes");
            let response = service.handle(&parsed);
            let ToolResponse::Ok { result } = response else {
                panic!("{operation} returned a service error: {response:?}");
            };
            (operation, result)
        })
        .collect()
}

#[test]
fn agent_v1_schema_covers_every_advertised_request_and_response() {
    let root = workspace_root();
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("schemas/wright-agent-v1.schema.json")).unwrap(),
    )
    .unwrap();
    let request_schema = JSONSchema::compile(&schema).expect("request schema compiles");
    let mut response_document = schema.clone();
    response_document["$ref"] = Value::String("#/$defs/ToolResponse".to_string());
    let response_schema =
        JSONSchema::compile(&response_document).expect("response schema compiles");
    let mut capabilities_document = schema.clone();
    capabilities_document["$ref"] = Value::String("#/$defs/Capabilities".to_string());
    let capabilities_schema =
        JSONSchema::compile(&capabilities_document).expect("capability schema compiles");
    let mut edit_range_document = schema.clone();
    edit_range_document["$ref"] = Value::String("#/$defs/EditRange".to_string());
    let edit_range_schema =
        JSONSchema::compile(&edit_range_document).expect("range schema compiles");

    assert_eq!(AGENT_CONTRACT, "wright-agent/v1");
    assert_eq!(
        schema["$id"],
        "https://wrightkit.dev/schemas/wright-agent-v1.schema.json"
    );
    let current = capabilities();
    assert_eq!(current["agent_contract"], AGENT_CONTRACT);
    let advertised = current["operations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|operation| operation.as_str().unwrap().to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        schema["$defs"]["Capabilities"]["properties"]["operations"]["items"]["type"],
        "string"
    );
    let request_examples = requests();
    let request_operations = request_examples
        .iter()
        .map(|request| request["op"].as_str().unwrap().to_string())
        .collect::<BTreeSet<_>>();

    assert_eq!(advertised, request_operations);
    assert_eq!(
        advertised,
        EXPECTED_V1_OPERATIONS
            .iter()
            .map(|operation| (*operation).to_string())
            .collect()
    );
    assert!(capabilities_schema.is_valid(&current));
    let mut future_capabilities = current.clone();
    future_capabilities["operations"]
        .as_array_mut()
        .unwrap()
        .push(json!("futureOperation"));
    assert!(capabilities_schema.is_valid(&future_capabilities));
    for request in request_examples {
        assert!(
            request_schema.is_valid(&request),
            "invalid request: {request}"
        );
        serde_json::from_value::<ToolRequest>(request).expect("request deserializes");
    }

    // #429: both address spellings validate and deserialize; other types are
    // rejected rather than coerced.
    for request in [
        json!({"op":"references","symbol":0}),
        json!({"op":"references","symbol":"counter"}),
        json!({"op":"usage","symbol":1}),
        json!({"op":"usage","symbol":"counter"}),
        json!({"op":"cfg","rule":0}),
        json!({"op":"cfg","rule":"intro"}),
    ] {
        assert!(
            request_schema.is_valid(&request),
            "invalid request: {request}"
        );
        serde_json::from_value::<ToolRequest>(request).expect("request deserializes");
    }
    for request in [
        json!({"op":"references","symbol":true}),
        json!({"op":"usage","symbol":1.5}),
        json!({"op":"cfg","rule":null}),
    ] {
        assert!(
            !request_schema.is_valid(&request),
            "schema accepted a non-id/name address: {request}"
        );
    }

    let capabilities_response = json!({"result":current});
    assert!(response_schema.is_valid(&capabilities_response));
    assert!(response_schema.is_valid(&json!({
        "result":current,
        "future_metadata":{"version":2}
    })));
    assert!(response_schema.is_valid(&json!({
        "error":{"code":"unsupported","message":"not available","details":{}},
        "future_metadata":{"version":2}
    })));
    assert!(!response_schema.is_valid(&json!({"result":{},"error":{"code":"x","message":"y"}})));
    assert!(!request_schema.is_valid(&json!({"op":"check","unexpected":true})));

    let operation_results = [
        ("capabilities", "CapabilitiesResult"),
        ("compile", "CompileResult"),
        ("check", "CheckResult"),
        ("analyze", "AnalyzeResult"),
        ("inspect", "InspectResult"),
        ("project", "ProjectResult"),
        ("rules", "RulesResult"),
        ("symbols", "SymbolsResult"),
        ("references", "ReferencesResult"),
        ("usage", "UsageResult"),
        ("cfg", "CfgResult"),
        ("findings", "FindingsResult"),
        ("persistentObjects", "PersistentObjectsResult"),
        ("lint", "LintResult"),
        ("lintRules", "LintRulesResult"),
        ("callGraph", "CallGraphResult"),
        ("costEstimate", "CostEstimateResult"),
        ("targetMetadata", "TargetMetadataResult"),
        ("validateEditTransaction", "ValidateEditTransactionResult"),
        ("semanticRename", "SemanticRenameResult"),
        ("providerSemanticRename", "ProviderSemanticRenameResult"),
        ("providerValidateEdit", "ProviderValidateEditResult"),
    ];
    let live_responses = service_responses();
    assert_eq!(live_responses.len(), operation_results.len());
    for ((operation, result), (expected_operation, definition)) in
        live_responses.iter().zip(operation_results)
    {
        assert_eq!(operation, expected_operation);
        let response = json!({"result":result});
        assert!(
            response_schema.is_valid(&response),
            "ToolResponse schema rejected {operation}: {response}"
        );
        let mut operation_document = schema.clone();
        operation_document["$ref"] = Value::String(format!("#/$defs/{definition}"));
        let operation_schema =
            JSONSchema::compile(&operation_document).expect("operation result schema compiles");
        assert!(
            operation_schema.is_valid(result),
            "{operation} result does not match {definition}: {result}"
        );
    }

    let valid_range = json!({"start_line":1,"start_col":1,"end_line":1,"end_col":1});
    assert!(edit_range_schema.is_valid(&valid_range));
    for coordinate in ["start_line", "start_col", "end_line", "end_col"] {
        let mut zero_range = valid_range.clone();
        zero_range[coordinate] = json!(0);
        assert!(
            !edit_range_schema.is_valid(&zero_range),
            "EditRange accepted 0 for {coordinate}"
        );
    }
}

#[test]
fn cli_and_agent_selections_return_the_same_set() {
    // #430: the CLI flags and the agent request fields drive one driver-side
    // selection, so both surfaces must return the same selected findings.
    let input = workspace_root().join("tests/fixtures/workshop/real-world/overpy-cake.ws");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("session starts");
    let mut service = ToolService::new(&mut session).expect("service loads");
    let agent = match service.handle(&ToolRequest::Lint(FindingSelection {
        rule: Some("repeated-value".to_string()),
        max: Some(2),
        ..FindingSelection::default()
    })) {
        ToolResponse::Ok { result } => result,
        ToolResponse::Error { error } => panic!("lint selection failed: {error:?}"),
    };

    let output = Command::new(env!("CARGO_BIN_EXE_wright"))
        .args([
            "lint",
            input.to_str().unwrap(),
            "--rule-id",
            "repeated-value",
            "--max",
            "2",
            "-f",
            "json",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("wright lint runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cli = serde_json::from_slice::<Value>(&output.stdout).unwrap()["result"].clone();
    assert_eq!(cli["findings"], agent["findings"], "same selected set");
    assert_eq!(cli["selection"], agent["selection"], "same truncation");
    assert_eq!(agent["selection"], json!({"total": 10, "withheld": 7}));
}
