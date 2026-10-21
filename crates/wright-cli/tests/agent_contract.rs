use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use jsonschema::validator_for;
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
    "providerValidateEdit",
    "editStatements",
    "lookup",
    "listEntries",
    "simulate",
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
        json!({"op":"compile","output":concat!(env!("CARGO_TARGET_TMPDIR"), "/agent-contract.ow"),"force":true}),
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
        // ADR-0029: `service_responses` substitutes the loaded input's
        // `inputIdentity`; a declared rule name needs no `snapshot`.
        json!({
            "op":"editStatements",
            "inputIdentity":"",
            "edits":[{
                "rule":"bounded while","list":"actions","index":0,"change":"insert",
                "statements":[{"call":"wait","args":[0.5,{"enum":"Wait","member":"IGNORE_CONDITION"}]}]
            }],
        }),
        json!({
            "op":"lookup",
            "language":"workshop",
            "query":"wait",
            "kind":null,
            "within":null,
            "locale":null,
            "limit":null
        }),
        json!({
            "op":"listEntries",
            "language":"workshop",
            "kind":"event",
            "within":null,
            "locale":null,
            "pageSize":null,
            "cursor":null
        }),
        // ADR-0033: `inputIdentity` preconditions the program to simulate;
        // `seconds` runs a scenario-free simulation under `athena`.
        json!({
            "op":"simulate",
            "inputIdentity":"",
            "sources":null,
            "seconds":0.25,
            "scenario":null
        }),
    ]
}

fn service_responses() -> Vec<(String, Value)> {
    let input = workspace_root().join("tests/fixtures/workshop/synthetic/control-flow.ws");
    let input_identity =
        wright_driver::input_identity(&std::fs::read_to_string(&input).expect("fixture text"));
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("session starts");
    let mut service = ToolService::new(&mut session).expect("service loads the project");

    let (rule, generation) = match service.handle(&ToolRequest::Rules {
        name: None,
        file: None,
        max: None,
        rule: None,
        snapshot: None,
        content: false,
        all: false,
    }) {
        ToolResponse::Ok { result } => (
            result["rules"][0]["id"].as_u64().unwrap(),
            result["snapshot"].as_u64().unwrap(),
        ),
        ToolResponse::Error { error } => panic!("rules failed: {error:?}"),
    };
    let symbol = match service.handle(&ToolRequest::Symbols {
        kind: None,
        file: None,
        max: None,
        all: false,
    }) {
        ToolResponse::Ok { result } => result["symbols"][0]["id"].as_u64().unwrap(),
        ToolResponse::Error { error } => panic!("symbols failed: {error:?}"),
    };

    requests()
        .into_iter()
        .map(|mut request| {
            let operation = request["op"].as_str().unwrap().to_string();
            match operation.as_str() {
                // #595: a numeric address carries the generation that issued
                // it — the `snapshot` the id-reporting response returned.
                "references" | "usage" => {
                    request["symbol"] = json!(symbol);
                    request["snapshot"] = json!(generation);
                }
                "cfg" => {
                    request["rule"] = json!(rule);
                    request["snapshot"] = json!(generation);
                }
                // ADR-0029/ADR-0033: `inputIdentity` preconditions the
                // program the statement addresses were read from, or the
                // program `simulate` runs.
                "editStatements" | "simulate" => {
                    request["inputIdentity"] = json!(input_identity);
                }
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
fn agent_v1_schema_covers_every_advertised_request_and_response() {
    let root = workspace_root();
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("schemas/wright-agent-v1.schema.json")).unwrap(),
    )
    .unwrap();
    let request_schema = validator_for(&schema).expect("request schema compiles");
    let mut response_document = schema.clone();
    response_document["$ref"] = Value::String("#/$defs/ToolResponse".to_string());
    let response_schema = validator_for(&response_document).expect("response schema compiles");
    let mut capabilities_document = schema.clone();
    capabilities_document["$ref"] = Value::String("#/$defs/Capabilities".to_string());
    let capabilities_schema =
        validator_for(&capabilities_document).expect("capability schema compiles");
    let mut edit_range_document = schema.clone();
    edit_range_document["$ref"] = Value::String("#/$defs/EditRange".to_string());
    let edit_range_schema = validator_for(&edit_range_document).expect("range schema compiles");

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

    // #429 + #595: both address spellings validate and deserialize; a
    // numeric address requires its issuing `snapshot` while a declared name
    // ignores it. Other types are rejected rather than coerced.
    for request in [
        json!({"op":"references","symbol":0,"snapshot":1}),
        json!({"op":"references","symbol":"counter"}),
        json!({"op":"references","symbol":"counter","snapshot":1}),
        json!({"op":"usage","symbol":1,"snapshot":1}),
        json!({"op":"usage","symbol":"counter"}),
        json!({"op":"cfg","rule":0,"snapshot":1}),
        json!({"op":"cfg","rule":"intro"}),
        json!({"op":"rules","rule":0,"snapshot":1}),
        json!({"op":"rules","rule":"intro"}),
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
        // #595: a numeric address without its issuing snapshot is invalid —
        // and even where a request still parses, the service refuses it
        // `stale-id`.
        json!({"op":"references","symbol":0}),
        json!({"op":"usage","symbol":1}),
        json!({"op":"cfg","rule":0}),
        json!({"op":"rules","rule":0}),
        // ADR-0035: `name` is removed from `rules` — the schema no longer
        // accepts it, and the service refuses a request carrying it
        // `malformed-request` (the deserialization sentinel lets the
        // refusal come from the service on every transport).
        json!({"op":"rules","name":"intro"}),
    ] {
        assert!(
            !request_schema.is_valid(&request),
            "schema accepted a non-id/name address or a snapshot-less id: {request}"
        );
    }

    // #472: `sources` is optional on the raw edit operations — a request
    // without it, or with an explicit null, validates and deserializes; the
    // service then reads the files the request names from disk.
    for request in [
        json!({"op":"validateEditTransaction","transaction":{"edits":[{
            "kind":"edit","source":"a.ws","source_identity":"sha256:x",
            "range":{"start_line":1,"start_col":1,"end_line":1,"end_col":1},
            "new_text":"x"}]}}),
        json!({"op":"validateEditTransaction","sources":null,"transaction":{"edits":[{
            "kind":"edit","source":"a.ws","source_identity":"sha256:x",
            "range":{"start_line":1,"start_col":1,"end_line":1,"end_col":1},
            "new_text":"x"}]}}),
    ] {
        assert!(
            request_schema.is_valid(&request),
            "invalid request: {request}"
        );
        serde_json::from_value::<ToolRequest>(request).expect("request deserializes");
    }

    // #548: `documents`/`sources` are optional on the provider operations —
    // a request without them, or with explicit nulls, validates and
    // deserializes; the service then derives the set from the loaded project.
    for request in [
        json!({"op":"providerValidateEdit","language_id":"opy","transaction":{"edits":[{
            "kind":"edit","source":"file:///main.opy","source_identity":"sha256:x",
            "range":{"start_line":1,"start_col":1,"end_line":1,"end_col":1},
            "new_text":"x"}]}}),
        json!({"op":"providerValidateEdit","language_id":"opy","documents":null,"sources":null,
            "transaction":{"edits":[{
            "kind":"edit","source":"file:///main.opy","source_identity":"sha256:x",
            "range":{"start_line":1,"start_col":1,"end_line":1,"end_col":1},
            "new_text":"x"}]}}),
    ] {
        assert!(
            request_schema.is_valid(&request),
            "invalid request: {request}"
        );
        serde_json::from_value::<ToolRequest>(request).expect("request deserializes");
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

    // #532: `capabilities.result_schemas` names the committed-schema definition
    // describing each advertised operation's result. An advertised operation
    // without a result schema fails here.
    let result_schemas = current["result_schemas"].as_object().unwrap();
    assert_eq!(
        result_schemas.keys().cloned().collect::<BTreeSet<String>>(),
        advertised
    );
    let live_responses = service_responses();
    assert_eq!(live_responses.len(), result_schemas.len());
    for (operation, result) in &live_responses {
        let reference = result_schemas[operation]
            .as_str()
            .unwrap_or_else(|| panic!("{operation} has no result schema"));
        let definition = reference
            .strip_prefix("#/$defs/")
            .unwrap_or_else(|| panic!("{operation} result schema is not a local ref: {reference}"));
        assert!(
            schema["$defs"].get(definition).is_some(),
            "{operation} result schema {reference} has no definition"
        );
        let response = json!({"result":result});
        assert!(
            response_schema.is_valid(&response),
            "ToolResponse schema rejected {operation}: {response}"
        );
        let mut operation_document = schema.clone();
        operation_document["$ref"] = Value::String(reference.to_string());
        let operation_schema =
            validator_for(&operation_document).expect("operation result schema compiles");
        assert!(
            operation_schema.is_valid(result),
            "{operation} result does not match {reference}: {result}"
        );
    }

    // #532: `all` validates and deserializes on analyze/inspect/lint, and a
    // all result validates against the same operation result schema.
    for request in [
        json!({"op":"analyze","all":true}),
        json!({"op":"inspect","all":true}),
        json!({"op":"lint","all":true}),
        json!({"op":"lint","all":false,"severity":"warning","max":3}),
    ] {
        assert!(
            request_schema.is_valid(&request),
            "invalid request: {request}"
        );
        let parsed =
            serde_json::from_value::<ToolRequest>(request.clone()).expect("request deserializes");
        let ToolResponse::Ok { result } = service_response(&parsed) else {
            panic!("all request failed: {request}");
        };
        let reference = result_schemas[request["op"].as_str().unwrap()]
            .as_str()
            .unwrap();
        let mut operation_document = schema.clone();
        operation_document["$ref"] = Value::String(reference.to_string());
        let operation_schema =
            validator_for(&operation_document).expect("operation result schema compiles");
        assert!(
            operation_schema.is_valid(&result),
            "{} all result does not match {reference}: {result}",
            request["op"]
        );
    }
    assert!(!request_schema.is_valid(&json!({"op":"lint","all":"yes"})));
    assert!(request_schema.is_valid(&json!({"op":"check","all":true})));

    // #783: `limit` declares 1..=10 in the request schema. An out-of-range
    // value fails schema validation, still deserializes (the wire type is
    // an integer), and the service refuses it `malformed-request` — the
    // same code a transport assigns a malformed request.
    for request in [
        json!({"op":"lookup","language":"workshop","limit":0}),
        json!({"op":"lookup","language":"workshop","limit":11}),
    ] {
        assert!(
            !request_schema.is_valid(&request),
            "schema accepted an out-of-range limit: {request}"
        );
        let parsed =
            serde_json::from_value::<ToolRequest>(request.clone()).expect("request deserializes");
        let ToolResponse::Error { error } = service_response(&parsed) else {
            panic!("{request} expected a malformed-request refusal");
        };
        assert_eq!(error.code, "malformed-request", "{request}: {error:?}");
    }
    assert!(request_schema.is_valid(&json!({"op":"lookup","language":"workshop","limit":10})));

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
    let agent = match service.handle(&ToolRequest::Lint {
        selection: FindingSelection {
            rule: Some("repeated-value".to_string()),
            max: Some(2),
            ..FindingSelection::default()
        },
        all: false,
    }) {
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
    assert_eq!(
        agent["selection"],
        json!({"total": 10, "withheld": 7, "byCode": {"repeated-value": 9}})
    );

    // #531: the semantic-query flags select the same set the agent request
    // fields do, through the same driver-side selection.
    let agent = match service.handle(&ToolRequest::Symbols {
        kind: Some("rule".to_string()),
        file: None,
        max: Some(2),
        all: false,
    }) {
        ToolResponse::Ok { result } => result,
        ToolResponse::Error { error } => panic!("symbols selection failed: {error:?}"),
    };
    let output = Command::new(env!("CARGO_BIN_EXE_wright"))
        .args([
            "inspect",
            "symbols",
            input.to_str().unwrap(),
            "--only",
            "rule",
            "--max",
            "2",
            "-f",
            "json",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("wright inspect symbols runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cli = serde_json::from_slice::<Value>(&output.stdout).unwrap()["result"].clone();
    assert_eq!(cli["symbols"], agent["symbols"], "same selected set");
    assert_eq!(cli["selection"], agent["selection"], "same truncation");

    // ADR-0024: `inspect rules` serves the same driver model `rules`
    // serves, including `content` and `rule` — each flag maps to the
    // request field of the same name. A fixture without ambiguous enums:
    // `rule-content-v1` cannot express `__ambiguous_enum`, so content
    // fails whole-call there by design.
    let rules_input =
        workspace_root().join("tests/fixtures/workshop/synthetic/declarations-rules.ws");
    let mut rules_session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(rules_input.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("rules session starts");
    let mut rules_service = ToolService::new(&mut rules_session).expect("rules service loads");
    for (request, args) in [
        (
            ToolRequest::Rules {
                name: None,
                rule: None,
                snapshot: None,
                file: None,
                max: Some(1),
                content: true,
                all: false,
            },
            vec!["--content", "--max", "1"],
        ),
        (
            ToolRequest::Rules {
                name: None,
                rule: Some("player starts".into()),
                snapshot: None,
                file: None,
                max: None,
                content: true,
                all: false,
            },
            vec!["--content", "--rule", "player starts"],
        ),
    ] {
        let agent = match rules_service.handle(&request) {
            ToolResponse::Ok { result } => result,
            ToolResponse::Error { error } => panic!("{request:?} failed: {error:?}"),
        };
        let output = Command::new(env!("CARGO_BIN_EXE_wright"))
            .args(["inspect", "rules"])
            .arg(rules_input.to_str().unwrap())
            .args(&args)
            .args(["-f", "json"])
            .stdin(Stdio::null())
            .output()
            .expect("wright inspect rules runs");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let cli = serde_json::from_slice::<Value>(&output.stdout).unwrap()["result"].clone();
        assert_eq!(cli, agent, "{request:?} differs between CLI and service");
    }

    // #532: `--all` and the `all` request field produce the same all
    // payload for the same input — the agent and CLI surfaces share one
    // driver-side form.
    for (request, args, envelope) in [
        (
            ToolRequest::Lint {
                selection: FindingSelection::default(),
                all: false,
            },
            vec!["lint"],
            false,
        ),
        (
            ToolRequest::Analyze {
                selection: Default::default(),
                all: false,
            },
            vec!["analyze"],
            true,
        ),
        (ToolRequest::Inspect { all: false }, vec!["inspect"], true),
    ] {
        let agent = match service.handle(&request) {
            ToolResponse::Ok { mut result } => {
                if !envelope {
                    result.as_object_mut().unwrap().remove("ok");
                    result.as_object_mut().unwrap().remove("exit");
                    // #595: the service stamps its serving `snapshot`;
                    // the one-shot `lint` command has no session
                    // generation and reports none.
                    result.as_object_mut().unwrap().remove("snapshot");
                }
                if envelope {
                    result["result"].clone()
                } else {
                    result
                }
            }
            ToolResponse::Error { error } => panic!("{request:?} failed: {error:?}"),
        };
        let output = Command::new(env!("CARGO_BIN_EXE_wright"))
            .args(args)
            .arg(input.to_str().unwrap())
            .args(["-f", "json"])
            .stdin(Stdio::null())
            .output()
            .expect("all command runs");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let cli = serde_json::from_slice::<Value>(&output.stdout).unwrap()["result"].clone();
        assert_eq!(cli, agent, "{request:?} differs between CLI and service");
    }
}

#[test]
fn rule_content_conforms_to_the_owner_schema() {
    let input = workspace_root().join("tests/fixtures/workshop/synthetic/declarations-rules.ws");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("session starts");
    let mut service = ToolService::new(&mut session).expect("service loads the project");
    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Rules {
        name: None,
        rule: None,
        snapshot: None,
        file: None,
        max: None,
        content: true,
        all: false,
    }) else {
        panic!("rules with content failed");
    };
    let owner: Value =
        serde_json::from_str(workshop_rs::rules::content::RULE_CONTENT_V1_SCHEMA).unwrap();
    let owner = validator_for(&owner).expect("owner schema compiles");
    let wright: Value = serde_json::from_str(
        &std::fs::read_to_string(workspace_root().join("schemas/wright-agent-v1.schema.json"))
            .unwrap(),
    )
    .unwrap();
    let mut response = wright.clone();
    response["$ref"] = json!("#/$defs/RulesResult");
    let response = validator_for(&response).expect("response schema compiles");
    assert!(response.is_valid(&result), "{result}");
    for rule in result["rules"].as_array().unwrap() {
        // Wright adds `span` to each statement; the owner's document has none.
        let mut content = rule["content"].clone();
        for key in ["conditions", "actions"] {
            for statement in content[key].as_array_mut().unwrap() {
                statement.as_object_mut().unwrap().remove("span");
            }
        }
        assert!(owner.is_valid(&content), "{content}");
    }
}
