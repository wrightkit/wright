//! Protocol/client unit tests against a scripted fake provider.
//!
//! The fake provider is a thread speaking over real OS pipes (os-pipe), so
//! framing, flushing, correlation, timeouts, protocol violations, and
//! end-of-stream behavior are exercised through the exact same byte
//! transport a spawned provider process uses — without needing a provider
//! binary. The end-to-end suite against the real LPP conformance mock
//! provider lives in `tests/mock_provider.rs`.

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

/// One scripted fake-provider behavior. Every request-consuming step
/// records the request line it read (for later assertions).
enum FakeStep {
    /// Respond to the next request, patching the response id from the
    /// request.
    Respond(Value),
    /// Respond to the next request with a verbatim line (no id patching).
    RespondRaw(String),
    /// Read and ignore the next request (used to provoke timeouts).
    NoResponse,
    /// Write a verbatim line without consuming a request (spontaneous
    /// provider output such as stray blank lines).
    Spontaneous(String),
    /// Keep the output open for a duration without writing (lets a timeout
    /// fire before end-of-stream).
    Hold(Duration),
    /// Close the provider output (end-of-stream to the client).
    Close,
}

/// A scripted fake provider over OS pipes.
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

    /// The next recorded request line, or a timeout when none arrived.
    fn next_request(&self) -> Result<String, RecvTimeoutError> {
        self.requests.recv_timeout(Duration::from_millis(250))
    }

    /// Assert that no further request line arrives within a short window.
    fn assert_no_request(&self) {
        match self.requests.recv_timeout(Duration::from_millis(100)) {
            Err(RecvTimeoutError::Timeout) => {}
            Ok(line) => panic!("unexpected request reached the provider: {line}"),
            Err(RecvTimeoutError::Disconnected) => {}
        }
    }

    /// Join the script thread (the script ends at end-of-stream).
    fn join(self) {
        let handle = self.handle.expect("script thread present");
        handle.join().expect("script thread panicked");
    }
}

fn run_script(
    script: Vec<FakeStep>,
    provider_write: os_pipe::PipeWriter,
    provider_read: os_pipe::PipeReader,
    requests_tx: Sender<String>,
) {
    let mut reader = BufReader::new(provider_read);
    let mut writer = provider_write;
    let mut line = String::new();
    for step in script {
        // Spontaneous and hold steps run without consuming a request.
        match &step {
            FakeStep::Spontaneous(raw) => {
                if write_line(&mut writer, raw).is_err() {
                    break;
                }
                continue;
            }
            FakeStep::Hold(duration) => {
                std::thread::sleep(*duration);
                continue;
            }
            _ => {}
        }
        let request = read_request(&mut reader, &mut line);
        match step {
            FakeStep::Respond(mut response) => {
                let Some(request) = request else { break };
                let _ = requests_tx.send(request.clone());
                response["id"] = request_id(&request);
                if write_line(&mut writer, &response.to_string()).is_err() {
                    break;
                }
            }
            FakeStep::RespondRaw(raw) => {
                let Some(request) = request else { break };
                let _ = requests_tx.send(request);
                if write_line(&mut writer, &raw).is_err() {
                    break;
                }
            }
            FakeStep::NoResponse => {
                let Some(request) = request else { break };
                let _ = requests_tx.send(request);
            }
            FakeStep::Spontaneous(_) | FakeStep::Hold(_) => {
                unreachable!("handled before reading")
            }
            FakeStep::Close => break,
        }
    }
    // Dropping the writer closes the provider output: end-of-stream.
}

fn read_request(reader: &mut BufReader<os_pipe::PipeReader>, line: &mut String) -> Option<String> {
    line.clear();
    match reader.read_line(line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line.trim_end_matches(['\r', '\n']).to_string()),
    }
}

fn write_line(writer: &mut os_pipe::PipeWriter, line: &str) -> std::io::Result<()> {
    writer.write_all(line.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

/// The request line the recorded message belongs to (for id patching).
fn request_id(request: &str) -> Value {
    serde_json::from_str::<Value>(request)
        .ok()
        .and_then(|value| value.get("id").cloned())
        .unwrap_or(Value::Null)
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

fn ok_response(result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": 0, "result": result })
}

fn lpp_error(kind: &str, details: Value, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 0,
        "error": {
            "code": -32000,
            "message": message,
            "data": { "lpp": { "kind": kind, "details": details } },
        }
    })
}

// ---------------------------------------------------------------------------
// Framing and correlation
// ---------------------------------------------------------------------------

#[test]
fn initialize_handshake_is_a_single_flushed_line() {
    let (mut client, fake) =
        FakeProvider::spawn(vec![FakeStep::Respond(ok_response(init_result_json()))]);
    let result = client
        .initialize(json!({
            "protocolVersion": "1.0",
            "clientInfo": { "name": "wright", "version": "0.2.0" },
        }))
        .expect("initialize succeeds");
    let request: Value = serde_json::from_str(&fake.next_request().expect("request arrives"))
        .expect("request is one JSON line");
    assert_eq!(request["jsonrpc"], "2.0");
    assert_eq!(request["method"], "lpp/initialize");
    assert_eq!(request["params"]["protocolVersion"], "1.0");
    assert_eq!(request["params"]["clientInfo"]["name"], "wright");
    assert_eq!(result["protocolVersion"], "1.0");
    assert_eq!(client.phase(), ClientPhase::Ready);
    fake.join();
}

#[test]
fn responses_correlate_by_id() {
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(ok_response(init_result_json())),
        FakeStep::Respond(ok_response(json!({ "documents": [] }))),
    ]);
    client.initialize(json!({})).expect("initialize");
    let result = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect("check");
    assert_eq!(result, json!({ "documents": [] }));
    let _init = fake.next_request().expect("initialize request arrives");
    let check = fake.next_request().expect("check request arrives");
    let check: Value = serde_json::from_str(&check).expect("check request parses");
    assert_eq!(check["method"], "lpp/check");
    assert_eq!(check["id"], 2, "correlation ids increase per request");
    fake.join();
}

#[test]
fn empty_lines_and_crlf_are_accepted() {
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(ok_response(init_result_json())),
        FakeStep::Spontaneous("\n".to_string()),
        FakeStep::Spontaneous("\r\n".to_string()),
        FakeStep::Respond(ok_response(json!({ "documents": [] }))),
    ]);
    client.initialize(json!({})).expect("initialize");
    let result = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect("check");
    assert_eq!(result, json!({ "documents": [] }));
    fake.join();
}

// ---------------------------------------------------------------------------
// Timeouts and late responses
// ---------------------------------------------------------------------------

#[test]
fn missing_response_times_out_deterministically() {
    let (mut client, fake) = FakeProvider::spawn_with_timeout(
        vec![
            FakeStep::Respond(ok_response(init_result_json())),
            FakeStep::NoResponse,
            FakeStep::Hold(Duration::from_millis(200)),
        ],
        Duration::from_millis(50),
    );
    client.initialize(json!({})).expect("initialize");
    let error = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect_err("check times out");
    assert_eq!(error.code(), "provider-timeout");
    assert_eq!(
        error,
        ProviderError::Timeout {
            method: "lpp/check".to_string(),
            duration: Duration::from_millis(50),
        }
    );
    fake.join();
}

#[test]
fn late_response_after_timeout_is_ignored_and_session_survives() {
    // Request ids are deterministic (1 = initialize, 2 = first check, 3 =
    // second check). The late response for the timed-out request (id 2)
    // must be ignored, not treated as a protocol violation.
    let (mut client, fake) = FakeProvider::spawn_with_timeout(
        vec![
            FakeStep::Respond(ok_response(init_result_json())),
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
    let first = client.request("lpp/check", json!({ "documents": {} }));
    assert_eq!(
        first.expect_err("first check times out").code(),
        "provider-timeout"
    );
    let second = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect("second check succeeds after a late response");
    assert_eq!(second, json!({ "documents": [] }));
    fake.join();
}

// ---------------------------------------------------------------------------
// End-of-stream and provider failure
// ---------------------------------------------------------------------------

#[test]
fn eof_fails_pending_request_and_records_exit() {
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(ok_response(init_result_json())),
        FakeStep::Close,
    ]);
    client.initialize(json!({})).expect("initialize");
    let error = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect_err("provider output closed");
    assert_eq!(error.code(), "provider-exited");
    let again = client.request("lpp/check", json!({ "documents": {} }));
    assert_eq!(
        again.expect_err("session is dead").code(),
        "provider-exited"
    );
    fake.join();
}

// ---------------------------------------------------------------------------
// Protocol violations (session poisoning)
// ---------------------------------------------------------------------------

#[test]
fn malformed_json_poisons_the_session() {
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(ok_response(init_result_json())),
        FakeStep::RespondRaw("this is not json {".to_string()),
    ]);
    client.initialize(json!({})).expect("initialize");
    let error = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect_err("malformed");
    assert_eq!(error.code(), "provider-malformed");
    let again = client.request("lpp/check", json!({ "documents": {} }));
    assert_eq!(
        again.expect_err("session poisoned").code(),
        "provider-malformed"
    );
    fake.join();
}

#[test]
fn batch_message_poisons_the_session() {
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(ok_response(init_result_json())),
        FakeStep::RespondRaw("[1, 2]".to_string()),
    ]);
    client.initialize(json!({})).expect("initialize");
    let error = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect_err("batch");
    assert_eq!(error.code(), "provider-malformed");
    fake.join();
}

#[test]
fn notification_message_poisons_the_session() {
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(ok_response(init_result_json())),
        FakeStep::RespondRaw(
            "{\"jsonrpc\":\"2.0\",\"method\":\"lpp/check\",\"params\":{}}".to_string(),
        ),
    ]);
    client.initialize(json!({})).expect("initialize");
    let error = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect_err("notification");
    assert_eq!(error.code(), "provider-malformed");
    fake.join();
}

#[test]
fn unmatched_response_id_poisons_the_session() {
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(ok_response(init_result_json())),
        FakeStep::RespondRaw("{\"jsonrpc\":\"2.0\",\"id\":999,\"result\":null}".to_string()),
    ]);
    client.initialize(json!({})).expect("initialize");
    let error = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect_err("unmatched");
    assert_eq!(error.code(), "provider-malformed");
    fake.join();
}

#[test]
fn wrong_jsonrpc_version_poisons_the_session() {
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(ok_response(init_result_json())),
        FakeStep::RespondRaw("{\"jsonrpc\":\"1.0\",\"id\":2,\"result\":null}".to_string()),
    ]);
    client.initialize(json!({})).expect("initialize");
    let error = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect_err("version");
    assert_eq!(error.code(), "provider-malformed");
    fake.join();
}

// ---------------------------------------------------------------------------
// Malformed payloads (request-scoped, session survives)
// ---------------------------------------------------------------------------

#[test]
fn both_result_and_error_fails_only_that_request() {
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(ok_response(init_result_json())),
        FakeStep::Respond(
            json!({ "jsonrpc": "2.0", "id": 0, "result": null, "error": { "code": -32603, "message": "boom" } }),
        ),
        FakeStep::Respond(ok_response(json!({ "documents": [] }))),
    ]);
    client.initialize(json!({})).expect("initialize");
    let error = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect_err("both");
    assert_eq!(error.code(), "provider-malformed");
    let ok = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect("session survives");
    assert_eq!(ok, json!({ "documents": [] }));
    fake.join();
}

#[test]
fn neither_result_nor_error_fails_only_that_request() {
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(ok_response(init_result_json())),
        FakeStep::Respond(json!({ "jsonrpc": "2.0", "id": 0, "data": {} })),
        FakeStep::Respond(ok_response(json!({ "documents": [] }))),
    ]);
    client.initialize(json!({})).expect("initialize");
    let error = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect_err("neither");
    assert_eq!(error.code(), "provider-malformed");
    let ok = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect("session survives");
    assert_eq!(ok, json!({ "documents": [] }));
    fake.join();
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

#[test]
fn standard_jsonrpc_error_is_surfaced() {
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(ok_response(init_result_json())),
        FakeStep::Respond(
            json!({ "jsonrpc": "2.0", "id": 0, "error": { "code": -32601, "message": "Method not found" } }),
        ),
    ]);
    client.initialize(json!({})).expect("initialize");
    let error = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect_err("jsonrpc");
    assert_eq!(
        error,
        ProviderError::JsonRpc {
            code: -32601,
            message: "Method not found".to_string(),
        }
    );
    fake.join();
}

#[test]
fn lpp_refusal_is_typed_and_surfaces_refusal_code() {
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(ok_response(init_result_json())),
        FakeStep::Respond(lpp_error(
            "refusal",
            json!({ "refusalCode": "rename.noSymbolAtPosition", "uri": "file:///project/puzzle.xdl" }),
            "rename refused: no symbol at position",
        )),
    ]);
    client.initialize(json!({})).expect("initialize");
    let error = client
        .request("lpp/rename", json!({}))
        .expect_err("refusal");
    assert_eq!(error.code(), "refusal");
    assert_eq!(error.refusal_code(), Some("rename.noSymbolAtPosition"));
    let ProviderError::Lpp(lpp) = error else {
        panic!("expected a typed LPP error");
    };
    assert_eq!(lpp.kind, LppErrorKind::Refusal);
    fake.join();
}

#[test]
fn capability_unavailable_lpp_error_is_typed() {
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(ok_response(init_result_json())),
        FakeStep::Respond(lpp_error(
            "capabilityUnavailable",
            json!({ "capability": "compile", "method": "lpp/compile" }),
            "capability 'compile' is not available",
        )),
    ]);
    client.initialize(json!({})).expect("initialize");
    let error = client
        .request("lpp/compile", json!({}))
        .expect_err("capability");
    assert_eq!(error.code(), "capability-unavailable");
    let ProviderError::Lpp(lpp) = &error else {
        panic!("expected a typed LPP error");
    };
    assert_eq!(lpp.kind, LppErrorKind::CapabilityUnavailable);
    assert_eq!(lpp.capability(), Some("compile"));
    assert_eq!(lpp.method(), Some("lpp/compile"));
    fake.join();
}

#[test]
fn unknown_lpp_error_kind_is_preserved_opaque() {
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(ok_response(init_result_json())),
        FakeStep::Respond(lpp_error(
            "brandNewKind",
            json!({ "whatever": true }),
            "a future error kind",
        )),
    ]);
    client.initialize(json!({})).expect("initialize");
    let error = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect_err("unknown kind");
    assert_eq!(error.code(), "lpp-error");
    let ProviderError::Lpp(lpp) = &error else {
        panic!("expected a typed LPP error");
    };
    assert_eq!(lpp.kind, LppErrorKind::Unknown("brandNewKind".to_string()));
    fake.join();
}

#[test]
fn lpp_code_without_lpp_data_is_a_malformed_payload() {
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(ok_response(init_result_json())),
        FakeStep::Respond(
            json!({ "jsonrpc": "2.0", "id": 0, "error": { "code": -32000, "message": "naked" } }),
        ),
        FakeStep::Respond(ok_response(json!({ "documents": [] }))),
    ]);
    client.initialize(json!({})).expect("initialize");
    let error = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect_err("naked lpp code");
    assert_eq!(error.code(), "provider-malformed");
    let ok = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect("session survives");
    assert_eq!(ok, json!({ "documents": [] }));
    fake.join();
}

// ---------------------------------------------------------------------------
// Session phase guards
// ---------------------------------------------------------------------------

#[test]
fn request_before_initialize_is_refused_without_wire_traffic() {
    let (mut client, fake) = FakeProvider::spawn(vec![]);
    let error = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect_err("fresh");
    assert_eq!(error.code(), "provider-not-initialized");
    fake.assert_no_request();
    fake.join();
}

#[test]
fn double_initialize_is_refused() {
    let (mut client, fake) =
        FakeProvider::spawn(vec![FakeStep::Respond(ok_response(init_result_json()))]);
    client.initialize(json!({})).expect("initialize");
    let error = client.initialize(json!({})).expect_err("second initialize");
    assert_eq!(error.code(), "provider-already-initialized");
    fake.join();
}

#[test]
fn shutdown_before_initialize_is_refused() {
    let (mut client, fake) = FakeProvider::spawn(vec![]);
    let error = client.shutdown().expect_err("fresh shutdown");
    assert_eq!(error.code(), "provider-not-initialized");
    fake.join();
}

#[test]
fn requests_after_shutdown_are_refused() {
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(ok_response(init_result_json())),
        FakeStep::Respond(ok_response(Value::Null)),
    ]);
    client.initialize(json!({})).expect("initialize");
    client.shutdown().expect("shutdown");
    assert_eq!(client.phase(), ClientPhase::ShutDown);
    let error = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect_err("shut down");
    assert_eq!(error.code(), "provider-shutdown");
    fake.join();
}

#[test]
fn non_null_shutdown_result_fails_the_shutdown() {
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(ok_response(init_result_json())),
        FakeStep::Respond(ok_response(json!({ "oops": true }))),
        FakeStep::Respond(ok_response(json!({ "documents": [] }))),
    ]);
    client.initialize(json!({})).expect("initialize");
    let error = client.shutdown().expect_err("non-null result");
    assert_eq!(error.code(), "provider-malformed");
    // The session stays usable after a failed shutdown.
    let ok = client
        .request("lpp/check", json!({ "documents": {} }))
        .expect("session survives");
    assert_eq!(ok, json!({ "documents": [] }));
    fake.join();
}

// ---------------------------------------------------------------------------
// Initialize failure paths keep the session restartable
// ---------------------------------------------------------------------------

#[test]
fn protocol_version_mismatch_keeps_session_fresh_for_retry() {
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(lpp_error(
            "protocolVersionMismatch",
            json!({ "supportedProtocolVersions": ["1.0"] }),
            "unsupported protocol version 0.9",
        )),
        FakeStep::Respond(ok_response(init_result_json())),
    ]);
    let first = client.initialize(json!({})).expect_err("mismatch");
    assert_eq!(first.code(), "protocol-version-mismatch");
    assert_eq!(first.supported_protocol_versions(), vec!["1.0"]);
    let second = client
        .initialize(json!({}))
        .expect("retry with a supported version");
    assert_eq!(second["protocolVersion"], "1.0");
    fake.join();
}

#[test]
fn initialize_error_response_keeps_session_fresh_for_retry() {
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(lpp_error(
            "invalidRequest",
            json!({ "reason": "alreadyInitialized" }),
            "invalid request: already initialized",
        )),
        FakeStep::Respond(ok_response(init_result_json())),
    ]);
    let first = client
        .initialize(json!({}))
        .expect_err("initialization error");
    assert_eq!(first.code(), "invalid-request");
    let second = client.initialize(json!({})).expect("retry succeeds");
    assert_eq!(second["protocolVersion"], "1.0");
    fake.join();
}

#[test]
fn malformed_initialize_response_fails_the_initialize_only() {
    // Both result and error: the payload is attributable to the initialize
    // request, so the failure is request-scoped and the session stays fresh.
    let (mut client, fake) = FakeProvider::spawn(vec![
        FakeStep::Respond(
            json!({ "jsonrpc": "2.0", "id": 0, "result": null, "error": { "code": -32603, "message": "boom" } }),
        ),
        FakeStep::Respond(ok_response(init_result_json())),
    ]);
    let first = client
        .initialize(json!({}))
        .expect_err("malformed initialize");
    assert_eq!(first.code(), "provider-malformed");
    assert_eq!(client.phase(), ClientPhase::Fresh);
    let second = client.initialize(json!({})).expect("retry succeeds");
    assert_eq!(second["protocolVersion"], "1.0");
    fake.join();
}

// ---------------------------------------------------------------------------
// Capability model
// ---------------------------------------------------------------------------

#[test]
fn capability_require_refuses_unnegotiated_capabilities() {
    let capabilities = Capabilities {
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
    assert!(capabilities.require(Capability::Check).is_ok());
    let error = capabilities
        .require(Capability::Compile)
        .expect_err("not negotiated");
    assert_eq!(error.code(), "capability-unavailable");
    let ProviderError::Lpp(lpp) = &error else {
        panic!("expected a typed LPP error");
    };
    assert_eq!(lpp.capability(), Some("compile"));
    assert_eq!(lpp.method(), Some("lpp/compile"));
    assert_eq!(
        capabilities.supported(),
        vec![
            Capability::Check,
            Capability::Reconstruct,
            Capability::Symbols,
            Capability::Definition,
            Capability::References,
            Capability::Rename,
            Capability::EditValidation,
        ]
    );
}

#[test]
fn capability_ids_and_methods_match_the_spec_table() {
    assert_eq!(Capability::Check.as_str(), "check");
    assert_eq!(Capability::Compile.as_str(), "compile");
    assert_eq!(Capability::ProjectLoading.as_str(), "projectLoading");
    assert_eq!(Capability::Reconstruct.as_str(), "reconstruct");
    assert_eq!(Capability::Symbols.as_str(), "symbols");
    assert_eq!(Capability::Definition.as_str(), "definition");
    assert_eq!(Capability::References.as_str(), "references");
    assert_eq!(Capability::Rename.as_str(), "rename");
    assert_eq!(Capability::EditValidation.as_str(), "editValidation");
    assert_eq!(Capability::Check.method(), "lpp/check");
    assert_eq!(Capability::Compile.method(), "lpp/compile");
    assert_eq!(Capability::ProjectLoading.method(), "lpp/check");
    assert_eq!(Capability::Reconstruct.method(), "lpp/reconstruct");
    assert_eq!(Capability::Symbols.method(), "lpp/symbols");
    assert_eq!(Capability::Definition.method(), "lpp/definition");
    assert_eq!(Capability::References.method(), "lpp/references");
    assert_eq!(Capability::Rename.method(), "lpp/rename");
    assert_eq!(Capability::EditValidation.method(), "lpp/validateEdits");
    assert_eq!(
        Capability::parse("editValidation"),
        Some(Capability::EditValidation)
    );
    assert_eq!(
        Capability::parse("projectLoading"),
        Some(Capability::ProjectLoading)
    );
    assert_eq!(Capability::parse("bogus"), None);
}

#[test]
fn initialize_result_requires_all_capability_fields() {
    let complete: Value = init_result_json();
    let parsed: wright_lpp::InitializeResult =
        serde_json::from_value(complete).expect("all eight capability fields present");
    assert!(parsed.capabilities.supports(Capability::Compile));

    let mut missing = init_result_json();
    missing["capabilities"]
        .as_object_mut()
        .expect("object")
        .remove("compile");
    let error = serde_json::from_value::<wright_lpp::InitializeResult>(missing);
    assert!(
        error.is_err(),
        "a missing capability field must fail deserialization"
    );
}

#[test]
fn document_set_serializes_to_the_wire_shape() {
    let mut documents: DocumentSet = DocumentSet::new();
    documents.insert(
        "file:///project/puzzle.xdl".to_string(),
        Document {
            uri: "file:///project/puzzle.xdl".to_string(),
            language_id: "x-demo-lang".to_string(),
            version: 3,
            text: "puzzle clean { ... }".to_string(),
        },
    );
    let value = serde_json::to_value(&documents).expect("serializes");
    assert_eq!(
        value["file:///project/puzzle.xdl"]["languageId"],
        "x-demo-lang"
    );
    assert_eq!(value["file:///project/puzzle.xdl"]["version"], 3);
    assert_eq!(
        value["file:///project/puzzle.xdl"]["text"],
        "puzzle clean { ... }"
    );
}

#[test]
fn positions_use_zero_based_lsp_conventions() {
    let position = Position {
        line: 4,
        character: 6,
    };
    let value = serde_json::to_value(position).expect("serializes");
    assert_eq!(value, json!({ "line": 4, "character": 6 }));
}
