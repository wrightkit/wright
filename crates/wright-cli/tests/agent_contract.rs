use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use jsonschema::JSONSchema;
use serde_json::{Value, json};
use wright_driver::service::{AGENT_CONTRACT, ToolRequest};

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
        json!({"op":"references","symbol":1}),
        json!({"op":"usage","symbol":1}),
        json!({"op":"cfg","rule":1}),
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
    let requests = requests();
    let request_operations = requests
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
    for request in requests {
        assert!(
            request_schema.is_valid(&request),
            "invalid request: {request}"
        );
        serde_json::from_value::<ToolRequest>(request).expect("request deserializes");
    }

    assert!(response_schema.is_valid(&json!({"result":{"ok":true}})));
    assert!(response_schema.is_valid(&json!({
        "result":{"ok":true},
        "future_metadata":{"version":2}
    })));
    assert!(response_schema.is_valid(&json!({
        "error":{"code":"unsupported","message":"not available","details":{}},
        "future_metadata":{"version":2}
    })));
    assert!(!response_schema.is_valid(&json!({"result":{},"error":{"code":"x","message":"y"}})));
    assert!(!request_schema.is_valid(&json!({"op":"check","unexpected":true})));
}
