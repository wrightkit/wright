//! Protocol/client unit tests against a scripted fake provider.

#![allow(clippy::result_large_err)]

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{Value, json};
use wright_lpp::{
    Capabilities, Capability, ClientConfig, ClientPhase, Document, DocumentSet, JsonRpcClient,
    LppErrorKind, Position, ProviderError,
};

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

enum FakeStep {
    Respond(Value),
    RespondRaw(String),
    NoResponse,
    Spontaneous(String),
    Hold(Duration),
    Close,
}

struct FakeProvider {
    requests: mpsc::Receiver<String>,
    handle: Option<JoinHandle<()>>,
}

impl FakeProvider {
    fn spawn(script: Vec<FakeStep>) -> (JsonRpcClient, FakeProvider) {
        FakeProvider::spawn_with_timeout(script, TEST_TIMEOUT)
    }

    fn spawn_with_timeout(
        script: Vec<FakeStep>,
        request_timeout: Duration,
    ) -> (JsonRpcClient, FakeProvider) {
        let (client_read, provider_write) = os_pipe::pipe().expect("os-pipe");
        let (provider_read, client_write) = os_pipe::pipe().expect("os-pipe");
        let (requests_tx, requests_rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            run_script(script, provider_write, provider_read, requests_tx);
        });
        let client = JsonRpcClient::new(
            Box::new(client_read),
            Box::new(BufWriter::new(client_write)),
            ClientConfig { request_timeout },
        );
        (
            client,
            FakeProvider {
                requests: requests_rx,
                handle: Some(handle),
            },
        )
    }

    fn next_request(&self) -> Result<String, RecvTimeoutError> {
        self.requests.recv_timeout(Duration::from_millis(250))
    }

    fn assert_no_request(&self) {
        match self.requests.recv_timeout(Duration::from_millis(100)) {
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {}
            Ok(line) => panic!("unexpected request reached provider: {line}"),
        }
    }

    fn join(self) {
        self.handle.expect("thread").join().expect("joined");
    }
}

fn spawn_initialized(mut steps: Vec<FakeStep>) -> (JsonRpcClient, FakeProvider) {
    let mut script = vec![FakeStep::Respond(ok_response(init_result_json()))];
    script.append(&mut steps);
    let (mut client, fake) = FakeProvider::spawn(script);
    client.initialize(json!({})).expect("initialize");
    (client, fake)
}

fn run_script(
    script: Vec<FakeStep>,
    mut writer: os_pipe::PipeWriter,
    provider_read: os_pipe::PipeReader,
    requests_tx: Sender<String>,
) {
    let mut reader = BufReader::new(provider_read);
    let mut line = String::new();
    for step in script {
        match step {
            FakeStep::Spontaneous(raw) => {
                if write_line(&mut writer, &raw).is_err() {
                    break;
                }
            }
            FakeStep::Hold(duration) => std::thread::sleep(duration),
            FakeStep::Close => break,
            other => {
                line.clear();
                let Some(req) = reader
                    .read_line(&mut line)
                    .ok()
                    .filter(|&n| n > 0)
                    .map(|_| line.trim_end_matches(['\r', '\n']).to_string())
                else {
                    break;
                };
                let _ = requests_tx.send(req.clone());
                let res = match other {
                    FakeStep::Respond(mut val) => {
                        val["id"] = serde_json::from_str::<Value>(&req)
                            .ok()
                            .and_then(|v| v.get("id").cloned())
                            .unwrap_or(Value::Null);
                        Some(val.to_string())
                    }
                    FakeStep::RespondRaw(raw) => Some(raw),
                    _ => None,
                };
                if let Some(payload) = res {
                    if write_line(&mut writer, &payload).is_err() {
                        break;
                    }
                }
            }
        }
    }
}

fn write_line(writer: &mut os_pipe::PipeWriter, line: &str) -> std::io::Result<()> {
    writer.write_all(line.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn init_result_json() -> Value {
    json!({
        "protocolVersion": "1.0",
        "serverInfo": { "name": "lpp-mock-provider", "version": "0.1.0" },
        "languages": [ { "id": "x-demo-lang", "extensions": ["xdl"] } ],
        "capabilities": {
            "check": true, "compile": true, "reconstruct": true, "symbols": true,
            "definition": true, "references": true, "rename": true, "editValidation": true,
        }
    })
}

fn ok_response(result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": 0, "result": result })
}

fn lpp_error(kind: &str, details: Value, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0", "id": 0,
        "error": { "code": -32000, "message": message, "data": { "lpp": { "kind": kind, "details": details } } }
    })
}

fn step_ok(result: Value) -> FakeStep {
    FakeStep::Respond(ok_response(result))
}

fn step_lpp_err(kind: &str, details: Value, message: &str) -> FakeStep {
    FakeStep::Respond(lpp_error(kind, details, message))
}

fn check_req(client: &mut JsonRpcClient) -> Result<Value, ProviderError> {
    client.request("lpp/check", json!({ "documents": {} }))
}

fn assert_poison(raw: &str) {
    let (mut client, fake) = spawn_initialized(vec![FakeStep::RespondRaw(raw.to_string())]);
    assert_eq!(
        check_req(&mut client).unwrap_err().code(),
        "provider-malformed"
    );
    assert_eq!(
        check_req(&mut client).unwrap_err().code(),
        "provider-malformed"
    );
    fake.join();
}

fn assert_malformed_survives(response: Value) {
    let (mut client, fake) = spawn_initialized(vec![
        FakeStep::Respond(response),
        step_ok(json!({ "documents": [] })),
    ]);
    assert_eq!(
        check_req(&mut client).unwrap_err().code(),
        "provider-malformed"
    );
    assert_eq!(
        check_req(&mut client).expect("survives"),
        json!({ "documents": [] })
    );
    fake.join();
}

#[test]
fn initialize_handshake_is_a_single_flushed_line() {
    let (mut client, fake) = FakeProvider::spawn(vec![step_ok(init_result_json())]);
    let result = client
        .initialize(json!({
            "protocolVersion": "1.0",
            "clientInfo": { "name": "wright", "version": "0.2.0" },
        }))
        .expect("initialize succeeds");
    let request: Value =
        serde_json::from_str(&fake.next_request().expect("request")).expect("JSON line");
    assert_eq!(request["jsonrpc"], "2.0");
    assert_eq!(request["method"], "lpp/initialize");
    assert_eq!(request["params"]["protocolVersion"], "1.0");
    assert_eq!(request["params"]["clientInfo"]["name"], "wright");
    assert_eq!(result["protocolVersion"], "1.0");
    assert_eq!(client.phase(), ClientPhase::Ready);
    fake.join();
}

#[test]
fn responses_correlate_by_id_and_accept_crlf() {
    let (mut client, fake) = spawn_initialized(vec![
        FakeStep::Spontaneous("\n".to_string()),
        FakeStep::Spontaneous("\r\n".to_string()),
        step_ok(json!({ "documents": [] })),
    ]);
    assert_eq!(
        check_req(&mut client).expect("check"),
        json!({ "documents": [] })
    );
    let _init = fake.next_request().expect("init");
    let check: Value = serde_json::from_str(&fake.next_request().expect("check")).expect("parses");
    assert_eq!(check["method"], "lpp/check");
    assert_eq!(check["id"], 2);
    fake.join();
}

#[test]
fn timeout_and_late_response_handling() {
    let (mut client, fake) = FakeProvider::spawn_with_timeout(
        vec![
            step_ok(init_result_json()),
            FakeStep::NoResponse,
            FakeStep::Hold(Duration::from_millis(200)),
        ],
        Duration::from_millis(50),
    );
    client.initialize(json!({})).expect("initialize");
    let error = check_req(&mut client).expect_err("timeout");
    assert_eq!(error.code(), "provider-timeout");
    assert_eq!(
        error,
        ProviderError::Timeout {
            method: "lpp/check".into(),
            duration: Duration::from_millis(50)
        }
    );
    fake.join();

    let (mut client, fake) = FakeProvider::spawn_with_timeout(
        vec![
            step_ok(init_result_json()),
            FakeStep::NoResponse,
            FakeStep::RespondRaw(
                "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"documents\":[]}}".to_string(),
            ),
            FakeStep::Spontaneous(
                "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"documents\":[]}}".to_string(),
            ),
        ],
        Duration::from_millis(50),
    );
    client.initialize(json!({})).expect("initialize");
    assert_eq!(
        check_req(&mut client).unwrap_err().code(),
        "provider-timeout"
    );
    assert_eq!(
        check_req(&mut client).expect("second succeeds"),
        json!({ "documents": [] })
    );
    fake.join();
}

#[test]
fn eof_fails_pending_request_and_records_exit() {
    let (mut client, fake) = spawn_initialized(vec![FakeStep::Close]);
    assert_eq!(
        check_req(&mut client).expect_err("closed").code(),
        "provider-exited"
    );
    assert_eq!(
        check_req(&mut client).expect_err("session is dead").code(),
        "provider-exited"
    );
    fake.join();
}

#[test]
fn protocol_violations_poison_the_session() {
    for raw in [
        "this is not json {",
        "[1, 2]",
        "{\"jsonrpc\":\"2.0\",\"method\":\"lpp/check\",\"params\":{}}",
        "{\"jsonrpc\":\"2.0\",\"id\":999,\"result\":null}",
        "{\"jsonrpc\":\"1.0\",\"id\":2,\"result\":null}",
    ] {
        assert_poison(raw);
    }
}

#[test]
fn response_errors_and_malformed_payloads() {
    for malformed in [
        json!({ "jsonrpc": "2.0", "id": 0, "result": null, "error": { "code": -32603, "message": "boom" } }),
        json!({ "jsonrpc": "2.0", "id": 0, "data": {} }),
        json!({ "jsonrpc": "2.0", "id": 0, "error": { "code": -32000, "message": "naked" } }),
    ] {
        assert_malformed_survives(malformed);
    }

    let (mut client, fake) = spawn_initialized(vec![FakeStep::Respond(json!({
        "jsonrpc": "2.0", "id": 0, "error": { "code": -32601, "message": "Method not found" }
    }))]);
    assert_eq!(
        check_req(&mut client).expect_err("jsonrpc"),
        ProviderError::JsonRpc {
            code: -32601,
            message: "Method not found".to_string()
        }
    );
    fake.join();
}

#[test]
fn typed_lpp_errors() {
    let (mut client, fake) = spawn_initialized(vec![
        step_lpp_err(
            "refusal",
            json!({ "refusalCode": "rename.noSymbolAtPosition", "uri": "file:///project/puzzle.xdl" }),
            "rename refused: no symbol at position",
        ),
        step_lpp_err(
            "capabilityUnavailable",
            json!({ "capability": "compile", "method": "lpp/compile" }),
            "capability 'compile' is not available",
        ),
        step_lpp_err(
            "brandNewKind",
            json!({ "whatever": true }),
            "a future error kind",
        ),
    ]);

    let err = client
        .request("lpp/rename", json!({}))
        .expect_err("refusal");
    assert_eq!(err.code(), "refusal");
    assert_eq!(err.refusal_code(), Some("rename.noSymbolAtPosition"));
    assert!(matches!(err, ProviderError::Lpp(ref lpp) if lpp.kind == LppErrorKind::Refusal));

    let err = client
        .request("lpp/compile", json!({}))
        .expect_err("capability");
    assert_eq!(err.code(), "capability-unavailable");
    let ProviderError::Lpp(lpp) = &err else {
        panic!("expected typed LPP error")
    };
    assert_eq!(lpp.kind, LppErrorKind::CapabilityUnavailable);
    assert_eq!(lpp.capability(), Some("compile"));
    assert_eq!(lpp.method(), Some("lpp/compile"));

    let err = check_req(&mut client).expect_err("unknown");
    assert_eq!(err.code(), "lpp-error");
    assert!(
        matches!(err, ProviderError::Lpp(lpp) if lpp.kind == LppErrorKind::Unknown("brandNewKind".into()))
    );

    fake.join();
}

#[test]
fn lifecycle_phase_transitions_and_refusals() {
    let (mut client, fake) = FakeProvider::spawn(vec![]);
    assert_eq!(
        check_req(&mut client).expect_err("fresh").code(),
        "provider-not-initialized"
    );
    assert_eq!(
        client.shutdown().expect_err("fresh").code(),
        "provider-not-initialized"
    );
    fake.assert_no_request();
    fake.join();

    let (mut client, fake) = spawn_initialized(vec![
        step_ok(json!({ "oops": true })),
        step_ok(json!({ "documents": [] })),
        step_ok(Value::Null),
    ]);
    assert_eq!(
        client.initialize(json!({})).expect_err("second").code(),
        "provider-already-initialized"
    );
    assert_eq!(
        client.shutdown().expect_err("non-null").code(),
        "provider-malformed"
    );
    assert_eq!(
        check_req(&mut client).expect("session survives"),
        json!({ "documents": [] })
    );
    client.shutdown().expect("shutdown");
    assert_eq!(client.phase(), ClientPhase::ShutDown);
    assert_eq!(
        check_req(&mut client).expect_err("shut down").code(),
        "provider-shutdown"
    );
    fake.join();
}

#[test]
fn initialize_error_response_keeps_session_fresh_for_retry() {
    for step in [
        step_lpp_err(
            "protocolVersionMismatch",
            json!({ "supportedProtocolVersions": ["1.0"] }),
            "unsupported protocol version 0.9",
        ),
        step_lpp_err(
            "invalidRequest",
            json!({ "reason": "alreadyInitialized" }),
            "invalid request: already initialized",
        ),
        FakeStep::Respond(
            json!({ "jsonrpc": "2.0", "id": 0, "result": null, "error": { "code": -32603, "message": "boom" } }),
        ),
    ] {
        let (mut client, fake) = FakeProvider::spawn(vec![step, step_ok(init_result_json())]);
        let err = client.initialize(json!({})).expect_err("init failure");
        assert!(matches!(
            err.code(),
            "protocol-version-mismatch" | "invalid-request" | "provider-malformed"
        ));
        if err.code() == "protocol-version-mismatch" {
            assert_eq!(err.supported_protocol_versions(), vec!["1.0"]);
        }
        assert_eq!(client.phase(), ClientPhase::Fresh);
        assert_eq!(
            client.initialize(json!({})).expect("retry")["protocolVersion"],
            "1.0"
        );
        fake.join();
    }
}

#[test]
fn capabilities_and_serialization() {
    let caps = Capabilities {
        check: true,
        compile: false,
        project_loading: false,
        reconstruct: true,
        symbols: true,
        definition: true,
        references: true,
        rename: true,
        edit_validation: true,
    };
    assert!(caps.require(Capability::Check).is_ok());
    let error = caps
        .require(Capability::Compile)
        .expect_err("not negotiated");
    assert_eq!(error.code(), "capability-unavailable");
    let ProviderError::Lpp(lpp) = &error else {
        panic!("expected typed LPP error")
    };
    assert_eq!(lpp.capability(), Some("compile"));
    assert_eq!(lpp.method(), Some("lpp/compile"));
    assert_eq!(
        caps.supported(),
        vec![
            Capability::Check,
            Capability::Reconstruct,
            Capability::Symbols,
            Capability::Definition,
            Capability::References,
            Capability::Rename,
            Capability::EditValidation
        ]
    );

    for (cap, id, method) in [
        (Capability::Check, "check", "lpp/check"),
        (Capability::Compile, "compile", "lpp/compile"),
        (Capability::ProjectLoading, "projectLoading", "lpp/check"),
        (Capability::Reconstruct, "reconstruct", "lpp/reconstruct"),
        (Capability::Symbols, "symbols", "lpp/symbols"),
        (Capability::Definition, "definition", "lpp/definition"),
        (Capability::References, "references", "lpp/references"),
        (Capability::Rename, "rename", "lpp/rename"),
        (
            Capability::EditValidation,
            "editValidation",
            "lpp/validateEdits",
        ),
    ] {
        assert_eq!(cap.as_str(), id);
        assert_eq!(cap.method(), method);
        assert_eq!(Capability::parse(id), Some(cap));
    }
    assert_eq!(Capability::parse("bogus"), None);

    let mut init = init_result_json();
    assert!(
        serde_json::from_value::<wright_lpp::InitializeResult>(init.clone())
            .unwrap()
            .capabilities
            .supports(Capability::Compile)
    );
    init["capabilities"]
        .as_object_mut()
        .unwrap()
        .remove("compile");
    assert!(serde_json::from_value::<wright_lpp::InitializeResult>(init).is_err());

    let uri = "file:///project/puzzle.xdl";
    let documents = DocumentSet::from([(
        uri.into(),
        Document {
            uri: uri.into(),
            language_id: "x-demo-lang".into(),
            version: 3,
            text: "puzzle clean { ... }".into(),
        },
    )]);
    let value = serde_json::to_value(&documents).expect("serializes");
    assert_eq!(value[uri]["languageId"], "x-demo-lang");
    assert_eq!(value[uri]["version"], 3);
    assert_eq!(value[uri]["text"], "puzzle clean { ... }");
    assert_eq!(
        serde_json::to_value(Position {
            line: 4,
            character: 6
        })
        .unwrap(),
        json!({ "line": 4, "character": 6 })
    );
}
