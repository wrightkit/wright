use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{Value, json};

use super::*;
use crate::client::{ClientConfig, ClientPhase};
use crate::types::LookupWithin;

fn dummy_child() -> ChildProcess {
    let (command, args): (&str, Vec<String>) = if cfg!(windows) {
        (
            "ping",
            vec!["-n".to_string(), "60".to_string(), "127.0.0.1".to_string()],
        )
    } else {
        ("sleep", vec!["30".to_string()])
    };
    let mut child = ChildProcess::spawn(Path::new(command), &args).expect("dummy child spawns");
    let _stdin = child.take_stdin();
    let _stdout = child.take_stdout();
    child
}

struct Fake {
    requests: mpsc::Receiver<String>,
    _thread: JoinHandle<()>,
}

impl Fake {
    fn spawn(script: Vec<FakeStep>) -> (StdioLanguageProvider, Fake) {
        let (client_read, provider_write) = os_pipe::pipe().expect("os-pipe");
        let (provider_read, client_write) = os_pipe::pipe().expect("os-pipe");
        let (requests_tx, requests_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut reader = BufReader::new(provider_read);
            let mut writer = provider_write;
            let mut line = String::new();
            for step in script {
                line.clear();
                let read = reader.read_line(&mut line).unwrap_or(0);
                if read == 0 {
                    break;
                }
                let request = line.trim_end_matches(['\r', '\n']).to_string();
                let _ = requests_tx.send(request.clone());
                let FakeStep::Respond(mut response) = step;
                let id = serde_json::from_str::<Value>(&request)
                    .ok()
                    .and_then(|value| value.get("id").cloned())
                    .unwrap_or(Value::Null);
                response["id"] = id;
                let _ = writer.write_all(response.to_string().as_bytes());
                let _ = writer.write_all(b"\n");
                let _ = writer.flush();
            }
        });
        let client = JsonRpcClient::new(
            Box::new(client_read),
            Box::new(BufWriter::new(client_write)),
            ClientConfig {
                request_timeout: Duration::from_secs(5),
            },
        );
        let provider = StdioLanguageProvider {
            child: dummy_child(),
            client,
            negotiated: None,
            last_exit_status: None,
        };
        (
            provider,
            Fake {
                requests: requests_rx,
                _thread: thread,
            },
        )
    }

    fn assert_only_requests(&self, expected: usize) {
        for _ in 0..expected {
            let line = self
                .requests
                .recv_timeout(Duration::from_millis(250))
                .expect("expected request arrives");
            assert!(!line.is_empty(), "request line is non-empty");
        }
        match self.requests.recv_timeout(Duration::from_millis(100)) {
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Ok(line) => panic!("unexpected request reached the provider: {line}"),
            Err(mpsc::RecvTimeoutError::Disconnected) => {}
        }
    }
}

enum FakeStep {
    Respond(Value),
}

fn init_result_json() -> Value {
    json!({
        "protocolVersion": "1.0",
        "serverInfo": { "name": "lpp-mock-provider", "version": "0.1.0" },
        "languages": [ { "id": "x-demo-lang", "extensions": ["xdl"] } ],
        "capabilities": {
            "check": true,
            "compile": true,
            "reconstruct": true,
            "symbols": true,
            "definition": true,
            "references": true,
            "rename": true,
            "editValidation": true,
        }
    })
}

fn init_result_project_loading_json() -> Value {
    let mut result = init_result_json();
    result["protocolVersion"] = json!("1.1");
    result["capabilities"]["projectLoading"] = json!(true);
    result
}

fn init_result_directory_target_json() -> Value {
    let mut result = init_result_project_loading_json();
    result["protocolVersion"] = json!("1.2");
    result
}

fn init_result_artifact_negotiation_json() -> Value {
    let mut result = init_result_project_loading_json();
    result["protocolVersion"] = json!("1.4");
    result
}

fn init_result_lookup_json() -> Value {
    let mut result = init_result_artifact_negotiation_json();
    result["protocolVersion"] = json!("1.5");
    result["capabilities"]["lookup"] = json!(true);
    result
}

fn ok_response(result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": 0, "result": result })
}

fn doc(uri: &str, text: &str) -> Document {
    Document {
        uri: uri.to_string(),
        language_id: "x-demo-lang".to_string(),
        version: 1,
        text: text.to_string(),
    }
}

#[test]
fn initialize_parse_failure_resets_the_session_for_retry() {
    let mut missing = init_result_json();
    missing.as_object_mut().expect("object").remove("languages");
    let (mut provider, fake) = Fake::spawn(vec![
        FakeStep::Respond(ok_response(missing)),
        FakeStep::Respond(ok_response(init_result_json())),
    ]);
    let first = provider
        .initialize(None)
        .expect_err("missing required field");
    assert_eq!(first.code(), "provider-malformed");
    assert_eq!(
        provider.client.phase(),
        ClientPhase::Fresh,
        "session stays restartable"
    );
    let second = provider.initialize(None).expect("retry succeeds");
    assert_eq!(second.protocol_version, "1.0");
    assert_eq!(
        provider.capabilities().expect("negotiated").language_ids(),
        vec!["x-demo-lang"]
    );
    fake.assert_only_requests(2);
}

#[test]
fn initialize_echoed_version_mismatch_resets_the_session_for_retry() {
    let mut wrong = init_result_json();
    wrong["protocolVersion"] = json!("0.9");
    let (mut provider, fake) = Fake::spawn(vec![
        FakeStep::Respond(ok_response(wrong)),
        FakeStep::Respond(ok_response(init_result_json())),
    ]);
    let first = provider.initialize(None).expect_err("echo mismatch");
    assert_eq!(first.code(), "protocol-version-mismatch");
    assert_eq!(first.supported_protocol_versions(), vec!["0.9"]);
    assert_eq!(provider.client.phase(), ClientPhase::Fresh);
    provider.initialize(None).expect("retry succeeds");
    fake.assert_only_requests(2);
}

#[test]
fn capability_guard_refuses_before_any_wire_traffic() {
    let mut without_compile = init_result_json();
    without_compile["capabilities"]["compile"] = json!(false);
    let (mut provider, fake) = Fake::spawn(vec![
        FakeStep::Respond(ok_response(without_compile)),
        FakeStep::Respond(ok_response(json!({ "documents": [] }))),
    ]);
    provider.initialize(None).expect("initialize");
    let mut documents = DocumentSet::new();
    documents.insert(
        "file:///project/puzzle.xdl".to_string(),
        doc("file:///project/puzzle.xdl", "puzzle clean { ... }"),
    );
    let error = provider
        .compile(&documents, Some("file:///project"))
        .expect_err("compile not negotiated");
    assert_eq!(error.code(), "capability-unavailable");
    let check = provider.check(&documents, None).expect("check works");
    assert!(check.documents.is_empty());
    fake.assert_only_requests(2);
}

#[test]
fn project_loading_initialization_and_entry_requests_use_lpp_11() {
    let (mut provider, fake) = Fake::spawn(vec![
        FakeStep::Respond(ok_response(init_result_project_loading_json())),
        FakeStep::Respond(ok_response(json!({ "documents": [] }))),
        FakeStep::Respond(ok_response(json!({ "diagnostics": [], "artifact": null }))),
    ]);
    provider
        .initialize_project_loading(None)
        .expect("LPP 1.1 initialize");
    let entry = ProjectEntry {
        uri: "file:///project/main.opy".to_string(),
        language_id: "opy".to_string(),
        version: 7,
        kind: crate::types::ProjectTargetKind::File,
    };
    provider
        .check_entry(&entry, Some("file:///project"), Some("zh-CN"))
        .expect("entry check");
    provider
        .compile_entry(&entry, Some("file:///project"), Some("zh-CN"))
        .expect("entry compile");
    let initialize: Value = serde_json::from_str(
        &fake
            .requests
            .recv_timeout(Duration::from_millis(250))
            .expect("initialize request"),
    )
    .expect("initialize JSON");
    assert_eq!(initialize["params"]["protocolVersion"], "1.1");
    let check: Value = serde_json::from_str(
        &fake
            .requests
            .recv_timeout(Duration::from_millis(250))
            .expect("check request"),
    )
    .expect("check JSON");
    assert_eq!(check["method"], "lpp/check");
    assert_eq!(
        check["params"]["entry"],
        serde_json::to_value(&entry).unwrap()
    );
    assert_eq!(check["params"]["projectRoot"], "file:///project");
    assert_eq!(check["params"]["locale"], "zh-CN");
    let compile: Value = serde_json::from_str(
        &fake
            .requests
            .recv_timeout(Duration::from_millis(250))
            .expect("compile request"),
    )
    .expect("compile JSON");
    assert_eq!(compile["method"], "lpp/compile");
    fake.assert_only_requests(0);
}

#[test]
fn directory_target_requests_use_lpp_12_and_preserve_target_kind() {
    let (mut provider, fake) = Fake::spawn(vec![
        FakeStep::Respond(ok_response(init_result_directory_target_json())),
        FakeStep::Respond(ok_response(json!({ "documents": [] }))),
        FakeStep::Respond(ok_response(json!({ "diagnostics": [], "artifact": null }))),
    ]);
    provider
        .initialize_project_target(None)
        .expect("LPP 1.2 initialize");
    let target = ProjectEntry {
        uri: "file:///project".to_string(),
        language_id: "opy".to_string(),
        version: 7,
        kind: crate::types::ProjectTargetKind::Directory,
    };
    provider
        .check_target(&target, None, None)
        .expect("directory check");
    provider
        .compile_target(&target, None, None)
        .expect("directory compile");
    let initialize: Value = serde_json::from_str(
        &fake
            .requests
            .recv_timeout(Duration::from_millis(250))
            .expect("initialize request"),
    )
    .expect("initialize JSON");
    assert_eq!(initialize["params"]["protocolVersion"], "1.2");
    let check: Value = serde_json::from_str(
        &fake
            .requests
            .recv_timeout(Duration::from_millis(250))
            .expect("check request"),
    )
    .expect("check JSON");
    assert_eq!(check["params"]["entry"]["kind"], "directory");
    fake.assert_only_requests(1);
}

#[test]
fn accepted_artifact_formats_need_lpp_14_or_later() {
    let target = ProjectEntry {
        uri: "file:///project/main.opy".to_string(),
        language_id: "opy".to_string(),
        version: 1,
        kind: crate::types::ProjectTargetKind::File,
    };
    let (mut provider, fake) = Fake::spawn(vec![
        FakeStep::Respond(ok_response(init_result_artifact_negotiation_json())),
        FakeStep::Respond(ok_response(json!({ "diagnostics": [], "artifact": null }))),
    ]);
    provider
        .initialize_artifact_negotiation(None)
        .expect("LPP 1.4 initialize");
    provider
        .compile_target_accepting(
            &target,
            None,
            None,
            &["workshop-rs/mapped-text-v1", "workshop-rs/text-v1"],
        )
        .expect("negotiated compile");
    let initialize: Value = serde_json::from_str(
        &fake
            .requests
            .recv_timeout(Duration::from_millis(250))
            .expect("initialize request"),
    )
    .expect("initialize JSON");
    assert_eq!(initialize["params"]["protocolVersion"], "1.4");
    let compile: Value = serde_json::from_str(
        &fake
            .requests
            .recv_timeout(Duration::from_millis(250))
            .expect("compile request"),
    )
    .expect("compile JSON");
    assert_eq!(
        compile["params"]["acceptedArtifactFormats"],
        json!(["workshop-rs/mapped-text-v1", "workshop-rs/text-v1"])
    );

    let (mut older, fake) = Fake::spawn(vec![FakeStep::Respond(ok_response(
        init_result_directory_target_json(),
    ))]);
    older
        .initialize_project_target(None)
        .expect("LPP 1.2 initialize");
    let error = older
        .compile_target_accepting(&target, None, None, &["workshop-rs/text-v1"])
        .expect_err("pre-1.4 session");
    assert_eq!(error.supported_protocol_versions(), vec!["1.2"]);
    fake.assert_only_requests(1);
}

#[test]
fn lookup_uses_lpp_15_and_requires_the_capability() {
    let (mut provider, fake) = Fake::spawn(vec![
        FakeStep::Respond(ok_response(init_result_lookup_json())),
        FakeStep::Respond(ok_response(json!({
            "entries": [{
                "identity": "x-demo:operator/multiply",
                "kind": "operator",
                "spelling": "*",
                "displayName": "multiply",
                "callable": { "parameters": [
                    { "name": "arg", "type": "integer", "required": true }
                ] },
            }]
        }))),
    ]);
    provider
        .initialize_lookup(None)
        .expect("LPP 1.5 initialize");
    let params = LookupParams {
        language_id: "x-demo-lang".to_string(),
        query: Some("multiply".to_string()),
        kind: None,
        within: Some(LookupWithin {
            kind: "enum".to_string(),
            value: "x-demo:enum/operator".to_string(),
        }),
        locale: None,
        limit: Some(3),
    };
    let result = provider.lookup(&params).expect("lookup result");
    assert_eq!(result.entries.len(), 1);
    let initialize: Value = serde_json::from_str(
        &fake
            .requests
            .recv_timeout(Duration::from_millis(250))
            .expect("initialize request"),
    )
    .expect("initialize JSON");
    assert_eq!(initialize["params"]["protocolVersion"], "1.5");
    let request: Value = serde_json::from_str(
        &fake
            .requests
            .recv_timeout(Duration::from_millis(250))
            .expect("lookup request"),
    )
    .expect("lookup JSON");
    assert_eq!(request["method"], "lpp/lookup");
    assert_eq!(request["params"]["languageId"], "x-demo-lang");
    assert_eq!(request["params"]["query"], "multiply");
    assert_eq!(
        request["params"]["within"],
        json!({ "kind": "enum", "value": "x-demo:enum/operator" })
    );
    assert_eq!(request["params"]["limit"], 3);
    fake.assert_only_requests(0);

    let (mut older, fake) = Fake::spawn(vec![FakeStep::Respond(ok_response(
        init_result_artifact_negotiation_json(),
    ))]);
    older
        .initialize_artifact_negotiation(None)
        .expect("LPP 1.4 initialize");
    let error = older.lookup(&params).expect_err("pre-1.5 session");
    assert_eq!(error.code(), "capability-unavailable");
    fake.assert_only_requests(1);
}
