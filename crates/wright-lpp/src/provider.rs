//! The transport-neutral provider abstraction and its stdio implementation.
//!
//! [`LanguageProvider`] is the stable seam that ToolService and language
//! services consume: provider capabilities plus source-oriented operations,
//! with no process, framing, or JSON-RPC details exposed. Failures surface
//! as structured [`crate::error::ProviderError`] values; there is no silent
//! fallback when a required capability was not negotiated.
//!
//! [`StdioLanguageProvider`] implements the trait over a spawned provider
//! process (see [`crate::client::JsonRpcClient`] and
//! [`crate::process::ChildProcess`]).

use std::io::BufWriter;
use std::path::Path;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::LPP_PROTOCOL_VERSION;
use crate::client::{ClientConfig, JsonRpcClient};
use crate::error::ProviderError;
use crate::process::ChildProcess;
use crate::types::{
    Capabilities, Capability, CheckResult, ClientInfo, CompileResult, Document, DocumentSet,
    InitializeResult, LocationsResult, Position, ReconstructResult, RenameResult, SymbolsResult,
    TextEdit, ValidateEditsResult, WorkshopArtifact,
};

/// The negotiated result of a successful `lpp/initialize`.
#[derive(Debug, Clone)]
pub struct NegotiatedCapabilities {
    pub protocol_version: String,
    pub server_info: crate::types::ServerInfo,
    pub languages: Vec<crate::types::LanguageInfo>,
    pub capabilities: Capabilities,
}

impl NegotiatedCapabilities {
    /// Whether a capability was negotiated.
    pub fn supports(&self, capability: Capability) -> bool {
        self.capabilities.supports(capability)
    }

    /// Require a capability, refusing explicitly when absent.
    pub fn require(&self, capability: Capability) -> Result<(), ProviderError> {
        self.capabilities.require(capability)
    }

    /// The language ids the provider declared it serves.
    pub fn language_ids(&self) -> Vec<&str> {
        self.languages
            .iter()
            .map(|language| language.id.as_str())
            .collect()
    }
}

/// The transport-neutral language-provider client surface.
///
/// Implementations are session-oriented: [`initialize`](Self::initialize)
/// must succeed before any other operation, and [`shutdown`](Self::shutdown)
/// ends the session. Every operation is capability-guarded: invoking a
/// method whose capability was not negotiated fails explicitly with
/// `capabilityUnavailable` before anything is sent to the provider.
pub trait LanguageProvider {
    /// Perform the initialize/handshake and capability negotiation.
    ///
    /// The protocol version is fixed to `"1.0"` by this client. A provider
    /// that does not support it responds with a `protocolVersionMismatch`
    /// LPP error; a provider that echoes a different version after accepting
    /// fails with `ProtocolVersionMismatch`. Either way the session stays
    /// restartable.
    fn initialize(
        &mut self,
        client_info: Option<&ClientInfo>,
    ) -> Result<InitializeResult, ProviderError>;

    /// The negotiated capabilities, after a successful initialize.
    fn capabilities(&self) -> Result<&NegotiatedCapabilities, ProviderError>;

    /// `lpp/check`: produce diagnostics for a document set.
    fn check(
        &mut self,
        documents: &DocumentSet,
        project_root: Option<&str>,
    ) -> Result<CheckResult, ProviderError>;

    /// `lpp/compile`: compile a document set into one opaque Workshop
    /// artifact.
    fn compile(
        &mut self,
        documents: &DocumentSet,
        project_root: Option<&str>,
    ) -> Result<CompileResult, ProviderError>;

    /// `lpp/reconstruct`: reconstruct source from a provider-owned artifact.
    fn reconstruct(
        &mut self,
        artifact: &WorkshopArtifact,
    ) -> Result<ReconstructResult, ProviderError>;

    /// `lpp/symbols`: list the symbols declared in a document set.
    fn symbols(
        &mut self,
        documents: &DocumentSet,
        project_root: Option<&str>,
    ) -> Result<SymbolsResult, ProviderError>;

    /// `lpp/definition`: resolve the definition at a position.
    fn definition(
        &mut self,
        document: &Document,
        position: Position,
    ) -> Result<LocationsResult, ProviderError>;

    /// `lpp/references`: find references to the symbol at a position.
    fn references(
        &mut self,
        document: &Document,
        position: Position,
        include_declaration: bool,
    ) -> Result<LocationsResult, ProviderError>;

    /// `lpp/rename`: compute source edits for a semantic rename.
    fn rename(
        &mut self,
        documents: &DocumentSet,
        position_document_uri: &str,
        position: Position,
        new_name: &str,
        project_root: Option<&str>,
    ) -> Result<RenameResult, ProviderError>;

    /// `lpp/validateEdits`: validate a set of source edits against a
    /// document.
    fn validate_edits(
        &mut self,
        document: &Document,
        edits: &[TextEdit],
    ) -> Result<ValidateEditsResult, ProviderError>;

    /// Graceful termination: send `lpp/shutdown`, close the provider's
    /// stdin, and wait (bounded) for the provider to exit.
    fn shutdown(&mut self) -> Result<(), ProviderError>;

    /// The last observed provider exit code, when the provider exited.
    fn exit_status(&self) -> Option<i32>;
}

/// A `LanguageProvider` over a spawned stdio provider process.
pub struct StdioLanguageProvider {
    child: ChildProcess,
    client: JsonRpcClient,
    negotiated: Option<NegotiatedCapabilities>,
    last_exit_status: Option<i32>,
}

impl StdioLanguageProvider {
    /// Spawn `command` as a long-running stdio provider process.
    pub fn spawn(
        command: &Path,
        args: &[String],
        request_timeout: Duration,
    ) -> Result<StdioLanguageProvider, ProviderError> {
        let mut child = ChildProcess::spawn(command, args)?;
        let stdin = child.take_stdin();
        let stdout = child.take_stdout();
        let client = JsonRpcClient::new(
            Box::new(stdout),
            Box::new(BufWriter::new(stdin)),
            ClientConfig { request_timeout },
        );
        Ok(StdioLanguageProvider {
            child,
            client,
            negotiated: None,
            last_exit_status: None,
        })
    }

    /// Send a request through the client, enriching transport failures with
    /// the observed process status.
    fn request(&mut self, method: &str, params: Value) -> Result<Value, ProviderError> {
        match self.client.request(method, params) {
            Ok(value) => Ok(value),
            Err(mut error) => {
                self.enrich_transport_error(&mut error, method);
                Err(error)
            }
        }
    }

    /// When a request failed through a transport error (I/O or exit), the
    /// provider process is likely dead: observe its status (bounded) and
    /// report a deterministic `Exited` failure carrying the exit code.
    fn enrich_transport_error(&mut self, error: &mut ProviderError, method: &str) {
        if !matches!(error.code(), "provider-io" | "provider-exited") {
            return;
        }
        let deadline = std::time::Instant::now() + Duration::from_millis(500);
        loop {
            if let Some(status) = self.child.try_status() {
                self.last_exit_status = Some(status);
                *error = ProviderError::Exited {
                    status: Some(status),
                    message: format!(
                        "the LPP provider process exited with status {status} while handling '{method}'"
                    ),
                };
                return;
            }
            if std::time::Instant::now() >= deadline {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The capability guard: refuse explicitly when the capability was not
    /// negotiated (or when the session is not initialized).
    fn require_capability(&self, capability: Capability) -> Result<(), ProviderError> {
        match &self.negotiated {
            Some(negotiated) => negotiated.require(capability),
            None => Err(ProviderError::NotInitialized {
                method: capability.method().to_string(),
            }),
        }
    }
}

impl Drop for StdioLanguageProvider {
    fn drop(&mut self) {
        // The client (stdin) drops after this, but the provider must not be
        // left running: LPP allows the client to terminate at any time.
        self.child.kill();
    }
}

impl LanguageProvider for StdioLanguageProvider {
    fn initialize(
        &mut self,
        client_info: Option<&ClientInfo>,
    ) -> Result<InitializeResult, ProviderError> {
        let mut params = json!({ "protocolVersion": LPP_PROTOCOL_VERSION });
        if let Some(info) = client_info {
            params["clientInfo"] = serde_json::to_value(info).expect("client info serializes");
        }
        let value = match self.client.initialize(params) {
            Ok(value) => value,
            Err(mut error) => {
                self.enrich_transport_error(&mut error, "lpp/initialize");
                return Err(error);
            }
        };
        let result: InitializeResult = match serde_json::from_value(value) {
            Ok(result) => result,
            Err(error) => {
                self.client.reset_initialize();
                return Err(ProviderError::Malformed {
                    detail: format!(
                        "lpp/initialize result is not a valid LPP v1 response: {error}"
                    ),
                });
            }
        };
        if result.protocol_version != LPP_PROTOCOL_VERSION {
            self.client.reset_initialize();
            return Err(ProviderError::ProtocolVersionMismatch {
                supported: vec![result.protocol_version.clone()],
                message: format!(
                    "provider negotiated protocol version '{}' but this client only speaks '{LPP_PROTOCOL_VERSION}'",
                    result.protocol_version
                ),
            });
        }
        self.negotiated = Some(NegotiatedCapabilities {
            protocol_version: result.protocol_version.clone(),
            server_info: result.server_info.clone(),
            languages: result.languages.clone(),
            capabilities: result.capabilities.clone(),
        });
        Ok(result)
    }

    fn capabilities(&self) -> Result<&NegotiatedCapabilities, ProviderError> {
        self.negotiated
            .as_ref()
            .ok_or_else(|| ProviderError::NotInitialized {
                method: "capabilities".to_string(),
            })
    }

    fn check(
        &mut self,
        documents: &DocumentSet,
        project_root: Option<&str>,
    ) -> Result<CheckResult, ProviderError> {
        self.require_capability(Capability::Check)?;
        let value = self.request("lpp/check", documents_params(documents, project_root))?;
        parse_result(value, "lpp/check")
    }

    fn compile(
        &mut self,
        documents: &DocumentSet,
        project_root: Option<&str>,
    ) -> Result<CompileResult, ProviderError> {
        self.require_capability(Capability::Compile)?;
        let value = self.request("lpp/compile", documents_params(documents, project_root))?;
        parse_result(value, "lpp/compile")
    }

    fn reconstruct(
        &mut self,
        artifact: &WorkshopArtifact,
    ) -> Result<ReconstructResult, ProviderError> {
        self.require_capability(Capability::Reconstruct)?;
        let params =
            json!({ "artifact": serde_json::to_value(artifact).expect("artifact serializes") });
        let value = self.request("lpp/reconstruct", params)?;
        parse_result(value, "lpp/reconstruct")
    }

    fn symbols(
        &mut self,
        documents: &DocumentSet,
        project_root: Option<&str>,
    ) -> Result<SymbolsResult, ProviderError> {
        self.require_capability(Capability::Symbols)?;
        let value = self.request("lpp/symbols", documents_params(documents, project_root))?;
        parse_result(value, "lpp/symbols")
    }

    fn definition(
        &mut self,
        document: &Document,
        position: Position,
    ) -> Result<LocationsResult, ProviderError> {
        self.require_capability(Capability::Definition)?;
        let params = json!({
            "document": serde_json::to_value(document).expect("document serializes"),
            "position": serde_json::to_value(position).expect("position serializes"),
        });
        let value = self.request("lpp/definition", params)?;
        parse_result(value, "lpp/definition")
    }

    fn references(
        &mut self,
        document: &Document,
        position: Position,
        include_declaration: bool,
    ) -> Result<LocationsResult, ProviderError> {
        self.require_capability(Capability::References)?;
        let params = json!({
            "document": serde_json::to_value(document).expect("document serializes"),
            "position": serde_json::to_value(position).expect("position serializes"),
            "includeDeclaration": include_declaration,
        });
        let value = self.request("lpp/references", params)?;
        parse_result(value, "lpp/references")
    }

    fn rename(
        &mut self,
        documents: &DocumentSet,
        position_document_uri: &str,
        position: Position,
        new_name: &str,
        project_root: Option<&str>,
    ) -> Result<RenameResult, ProviderError> {
        self.require_capability(Capability::Rename)?;
        let mut params = json!({
            "documents": serde_json::to_value(documents).expect("document set serializes"),
            "positionDocumentUri": position_document_uri,
            "position": serde_json::to_value(position).expect("position serializes"),
            "newName": new_name,
        });
        if let Some(root) = project_root {
            params["projectRoot"] = json!(root);
        }
        let value = self.request("lpp/rename", params)?;
        parse_result(value, "lpp/rename")
    }

    fn validate_edits(
        &mut self,
        document: &Document,
        edits: &[TextEdit],
    ) -> Result<ValidateEditsResult, ProviderError> {
        self.require_capability(Capability::EditValidation)?;
        let params = json!({
            "document": serde_json::to_value(document).expect("document serializes"),
            "edits": serde_json::to_value(edits).expect("edits serialize"),
        });
        let value = self.request("lpp/validateEdits", params)?;
        parse_result(value, "lpp/validateEdits")
    }

    fn shutdown(&mut self) -> Result<(), ProviderError> {
        if let Err(mut error) = self.client.shutdown() {
            self.enrich_transport_error(&mut error, "lpp/shutdown");
            return Err(error);
        }
        self.client.close_stdin();
        self.last_exit_status = self.child.terminate();
        Ok(())
    }

    fn exit_status(&self) -> Option<i32> {
        self.last_exit_status
    }
}

/// The common `documents`/`projectRoot` parameter shape.
fn documents_params(documents: &DocumentSet, project_root: Option<&str>) -> Value {
    let mut params =
        json!({ "documents": serde_json::to_value(documents).expect("document set serializes") });
    if let Some(root) = project_root {
        params["projectRoot"] = json!(root);
    }
    params
}

/// Parse a typed result, converting shape failures into a deterministic
/// malformed-response error.
fn parse_result<T: DeserializeOwned>(value: Value, method: &str) -> Result<T, ProviderError> {
    serde_json::from_value(value).map_err(|error| ProviderError::Malformed {
        detail: format!("{method} result is not a valid LPP v1 response: {error}"),
    })
}

#[cfg(test)]
mod tests {
    //! Provider-layer tests with a scripted fake transport. These cover
    //! initialize strictness and capability guards that a conforming
    //! provider (the conformance mock) can never trigger on its own; the
    //! end-to-end suite exercises the real binary.

    use std::io::{BufRead, BufReader, BufWriter, Write};
    use std::path::Path;
    use std::sync::mpsc;
    use std::thread::JoinHandle;
    use std::time::Duration;

    use serde_json::{Value, json};

    use super::*;
    use crate::client::{ClientConfig, ClientPhase};

    /// A placeholder child whose pipes are taken and discarded; only the
    /// exit-status surface is used by these tests.
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

        /// Drain exactly `expected` request lines, then assert that nothing
        /// further reaches the provider (no unexpected wire traffic).
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
        missing["capabilities"]
            .as_object_mut()
            .expect("object")
            .remove("compile");
        let (mut provider, fake) = Fake::spawn(vec![
            FakeStep::Respond(ok_response(missing)),
            FakeStep::Respond(ok_response(init_result_json())),
        ]);
        let first = provider
            .initialize(None)
            .expect_err("missing capability field");
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
        // The session is healthy and check still works end to end.
        let check = provider.check(&documents, None).expect("check works");
        assert!(check.documents.is_empty());
        fake.assert_only_requests(2);
    }
}
