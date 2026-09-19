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

use crate::client::{ClientConfig, JsonRpcClient};
use crate::error::ProviderError;
use crate::process::ChildProcess;
use crate::types::{
    Capabilities, Capability, CheckResult, ClientInfo, CompileResult, Document, DocumentSet,
    InitializeResult, LocationsResult, Position, ProjectEntry, ReconstructResult, RenameResult,
    SymbolsResult, TextEdit, ValidateEditsResult, WorkshopArtifact,
};
use crate::{LPP_DIRECTORY_TARGET_PROTOCOL_VERSION, LPP_PROTOCOL_VERSION};

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

pub type LppRes<T> = Result<T, ProviderError>;

/// The transport-neutral language-provider client surface.
///
/// Implementations are session-oriented: [`initialize`](Self::initialize)
/// must succeed before any other operation, and [`shutdown`](Self::shutdown)
/// ends the session. Every operation is capability-guarded: invoking a
/// method whose capability was not negotiated fails explicitly with
/// `capabilityUnavailable` before anything is sent to the provider.
pub trait LanguageProvider {
    /// Perform the initialize/handshake and capability negotiation.
    fn initialize(&mut self, client_info: Option<&ClientInfo>) -> LppRes<InitializeResult>;

    /// Initialize with LPP 1.1 for provider-owned project loading.
    fn initialize_project_loading(
        &mut self,
        info: Option<&ClientInfo>,
    ) -> LppRes<InitializeResult> {
        let res = self.initialize(info)?;
        Err(ProviderError::ProtocolVersionMismatch {
            supported: vec![res.protocol_version],
            message: "the provider client does not support LPP 1.1 project loading".into(),
        })
    }

    /// Initialize with LPP 1.2 for provider-owned file or directory targets.
    fn initialize_project_target(&mut self, info: Option<&ClientInfo>) -> LppRes<InitializeResult> {
        let res = self.initialize_project_loading(info)?;
        Err(ProviderError::ProtocolVersionMismatch {
            supported: vec![res.protocol_version],
            message: "the provider client does not support LPP 1.2 directory targets".into(),
        })
    }

    /// The negotiated capabilities, after a successful initialize.
    fn capabilities(&self) -> LppRes<&NegotiatedCapabilities>;
    /// `lpp/check`: produce diagnostics for a document set.
    fn check(&mut self, docs: &DocumentSet, root: Option<&str>) -> LppRes<CheckResult>;
    /// `lpp/check` over a provider-owned filesystem project entry.
    fn check_entry(
        &mut self,
        e: &ProjectEntry,
        root: Option<&str>,
        loc: Option<&str>,
    ) -> LppRes<CheckResult> {
        self.check_target(e, root, loc)
    }
    /// `lpp/check` over an LPP 1.2 file or directory target.
    fn check_target(
        &mut self,
        _t: &ProjectEntry,
        _r: Option<&str>,
        _l: Option<&str>,
    ) -> LppRes<CheckResult> {
        unavail("projectLoading", "lpp/check")
    }
    /// `lpp/compile`: compile a document set into one opaque Workshop artifact.
    fn compile(&mut self, docs: &DocumentSet, root: Option<&str>) -> LppRes<CompileResult>;
    /// `lpp/compile` over a provider-owned filesystem project entry.
    fn compile_entry(
        &mut self,
        e: &ProjectEntry,
        root: Option<&str>,
        loc: Option<&str>,
    ) -> LppRes<CompileResult> {
        self.compile_target(e, root, loc)
    }
    /// `lpp/compile` over an LPP 1.2 file or directory target.
    fn compile_target(
        &mut self,
        _t: &ProjectEntry,
        _r: Option<&str>,
        _l: Option<&str>,
    ) -> LppRes<CompileResult> {
        unavail("projectLoading", "lpp/compile")
    }
    /// `lpp/reconstruct`: reconstruct source from a provider-owned artifact.
    fn reconstruct(&mut self, artifact: &WorkshopArtifact) -> LppRes<ReconstructResult>;
    /// `lpp/symbols`: list the symbols declared in a document set.
    fn symbols(&mut self, docs: &DocumentSet, root: Option<&str>) -> LppRes<SymbolsResult>;
    /// `lpp/definition`: resolve the definition at a position.
    fn definition(&mut self, doc: &Document, pos: Position) -> LppRes<LocationsResult>;
    /// `lpp/references`: find references to the symbol at a position.
    fn references(&mut self, doc: &Document, pos: Position, decl: bool) -> LppRes<LocationsResult>;
    /// `lpp/rename`: compute source edits for a semantic rename.
    fn rename(
        &mut self,
        docs: &DocumentSet,
        uri: &str,
        pos: Position,
        name: &str,
        root: Option<&str>,
    ) -> LppRes<RenameResult>;
    /// `lpp/validateEdits`: validate a set of source edits against a document.
    fn validate_edits(&mut self, doc: &Document, edits: &[TextEdit])
    -> LppRes<ValidateEditsResult>;
    /// Graceful termination: send `lpp/shutdown`, close stdin, and wait for exit.
    fn shutdown(&mut self) -> LppRes<()>;
    /// The last observed provider exit code, when the provider exited.
    fn exit_status(&self) -> Option<i32>;
}

fn unavail<T>(cap: &str, method: &str) -> LppRes<T> {
    Err(ProviderError::lpp(
        crate::error::LppErrorKind::CapabilityUnavailable,
        json!({ "capability": cap, "method": method }),
        format!("capability '{cap}' is not available in this provider client"),
    ))
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

    fn initialize_with_version(
        &mut self,
        protocol_version: &str,
        client_info: Option<&ClientInfo>,
    ) -> Result<InitializeResult, ProviderError> {
        let mut params = json!({ "protocolVersion": protocol_version });
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
        if result.protocol_version != protocol_version {
            self.client.reset_initialize();
            return Err(ProviderError::ProtocolVersionMismatch {
                supported: vec![result.protocol_version.clone()],
                message: format!(
                    "provider negotiated protocol version '{}' but this client requested '{protocol_version}'",
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
    fn call_capability<T: DeserializeOwned>(
        &mut self,
        capability: Capability,
        params: Value,
    ) -> Result<T, ProviderError> {
        self.require_capability(capability)?;
        let method = capability.method();
        parse_result(self.request(method, params)?, method)
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
    fn initialize(&mut self, info: Option<&ClientInfo>) -> LppRes<InitializeResult> {
        self.initialize_with_version(LPP_PROTOCOL_VERSION, info)
    }

    fn initialize_project_loading(
        &mut self,
        info: Option<&ClientInfo>,
    ) -> LppRes<InitializeResult> {
        self.initialize_with_version(crate::LPP_PROJECT_LOADING_PROTOCOL_VERSION, info)
    }

    fn initialize_project_target(&mut self, info: Option<&ClientInfo>) -> LppRes<InitializeResult> {
        self.initialize_with_version(LPP_DIRECTORY_TARGET_PROTOCOL_VERSION, info)
    }

    fn capabilities(&self) -> LppRes<&NegotiatedCapabilities> {
        self.negotiated
            .as_ref()
            .ok_or_else(|| ProviderError::NotInitialized {
                method: "capabilities".into(),
            })
    }

    fn check(&mut self, docs: &DocumentSet, root: Option<&str>) -> LppRes<CheckResult> {
        self.call_capability(Capability::Check, documents_params(docs, root))
    }

    fn check_target(
        &mut self,
        target: &ProjectEntry,
        root: Option<&str>,
        loc: Option<&str>,
    ) -> LppRes<CheckResult> {
        self.require_capability(Capability::ProjectLoading)?;
        self.call_capability(Capability::Check, entry_params(target, root, loc))
    }

    fn compile(&mut self, docs: &DocumentSet, root: Option<&str>) -> LppRes<CompileResult> {
        self.call_capability(Capability::Compile, documents_params(docs, root))
    }

    fn compile_target(
        &mut self,
        target: &ProjectEntry,
        root: Option<&str>,
        loc: Option<&str>,
    ) -> LppRes<CompileResult> {
        self.require_capability(Capability::ProjectLoading)?;
        self.call_capability(Capability::Compile, entry_params(target, root, loc))
    }

    fn reconstruct(&mut self, artifact: &WorkshopArtifact) -> LppRes<ReconstructResult> {
        self.call_capability(Capability::Reconstruct, json!({ "artifact": artifact }))
    }

    fn symbols(&mut self, docs: &DocumentSet, root: Option<&str>) -> LppRes<SymbolsResult> {
        self.call_capability(Capability::Symbols, documents_params(docs, root))
    }

    fn definition(&mut self, doc: &Document, pos: Position) -> LppRes<LocationsResult> {
        self.call_capability(
            Capability::Definition,
            json!({ "document": doc, "position": pos }),
        )
    }

    fn references(&mut self, doc: &Document, pos: Position, decl: bool) -> LppRes<LocationsResult> {
        self.call_capability(
            Capability::References,
            json!({ "document": doc, "position": pos, "includeDeclaration": decl }),
        )
    }

    fn rename(
        &mut self,
        docs: &DocumentSet,
        uri: &str,
        pos: Position,
        name: &str,
        root: Option<&str>,
    ) -> LppRes<RenameResult> {
        let p = opt_param(
            json!({ "documents": docs, "positionDocumentUri": uri, "position": pos, "newName": name }),
            "projectRoot",
            root,
        );
        self.call_capability(Capability::Rename, p)
    }

    fn validate_edits(
        &mut self,
        doc: &Document,
        edits: &[TextEdit],
    ) -> LppRes<ValidateEditsResult> {
        self.call_capability(
            Capability::EditValidation,
            json!({ "document": doc, "edits": edits }),
        )
    }

    fn shutdown(&mut self) -> LppRes<()> {
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

fn opt_param(mut obj: Value, key: &'static str, val: Option<&str>) -> Value {
    if let Some(v) = val {
        obj[key] = json!(v);
    }
    obj
}

fn documents_params(documents: &DocumentSet, project_root: Option<&str>) -> Value {
    opt_param(
        json!({ "documents": documents }),
        "projectRoot",
        project_root,
    )
}

fn entry_params(entry: &ProjectEntry, project_root: Option<&str>, locale: Option<&str>) -> Value {
    let p = opt_param(json!({ "entry": entry }), "projectRoot", project_root);
    opt_param(p, "locale", locale)
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
        let (cmd, args): (&str, &[&str]) = if cfg!(windows) {
            ("ping", &["-n", "60", "127.0.0.1"])
        } else {
            ("sleep", &["30"])
        };
        let mut child = ChildProcess::spawn(
            Path::new(cmd),
            &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
        .expect("dummy child spawns");
        let _ = (child.take_stdin(), child.take_stdout());
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
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    let request = line.trim_end_matches(['\r', '\n']).to_string();
                    let _ = requests_tx.send(request.clone());
                    let FakeStep::Respond(mut response) = step;
                    let id = serde_json::from_str::<Value>(&request)
                        .ok()
                        .and_then(|v| v.get("id").cloned())
                        .unwrap_or(Value::Null);
                    response["id"] = id;
                    let _ = writeln!(writer, "{response}");
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
            (
                StdioLanguageProvider {
                    child: dummy_child(),
                    client,
                    negotiated: None,
                    last_exit_status: None,
                },
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
            if let Ok(line) = self.requests.recv_timeout(Duration::from_millis(100)) {
                panic!("unexpected request reached the provider: {line}");
            }
        }
    }

    fn recv_req(fake: &Fake) -> Value {
        serde_json::from_str(
            &fake
                .requests
                .recv_timeout(Duration::from_millis(250))
                .expect("request"),
        )
        .expect("JSON")
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
                "check": true, "compile": true, "reconstruct": true, "symbols": true,
                "definition": true, "references": true, "rename": true, "editValidation": true,
            }
        })
    }

    fn init_result_version(ver: &str) -> Value {
        let mut result = init_result_json();
        result["protocolVersion"] = json!(ver);
        result["capabilities"]["projectLoading"] = json!(true);
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
    fn initialize_failure_resets_the_session_for_retry() {
        let mut missing = init_result_json();
        missing["capabilities"]
            .as_object_mut()
            .expect("object")
            .remove("compile");
        let (mut p1, f1) = Fake::spawn(vec![
            FakeStep::Respond(ok_response(missing)),
            FakeStep::Respond(ok_response(init_result_json())),
        ]);
        let err1 = p1.initialize(None).expect_err("missing capability field");
        assert_eq!(err1.code(), "provider-malformed");
        assert_eq!(p1.client.phase(), ClientPhase::Fresh);
        assert_eq!(p1.initialize(None).unwrap().protocol_version, "1.0");
        f1.assert_only_requests(2);

        let mut wrong = init_result_json();
        wrong["protocolVersion"] = json!("0.9");
        let (mut p2, f2) = Fake::spawn(vec![
            FakeStep::Respond(ok_response(wrong)),
            FakeStep::Respond(ok_response(init_result_json())),
        ]);
        let err2 = p2.initialize(None).expect_err("echo mismatch");
        assert_eq!(err2.code(), "protocol-version-mismatch");
        assert_eq!(p2.client.phase(), ClientPhase::Fresh);
        p2.initialize(None).expect("retry succeeds");
        f2.assert_only_requests(2);
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
    fn project_loading_and_directory_targets_use_proper_lpp_versions() {
        let (mut p1, f1) = Fake::spawn(vec![
            FakeStep::Respond(ok_response(init_result_version("1.1"))),
            FakeStep::Respond(ok_response(json!({ "documents": [] }))),
            FakeStep::Respond(ok_response(json!({ "diagnostics": [], "artifact": null }))),
        ]);
        p1.initialize_project_loading(None).unwrap();
        let entry = ProjectEntry {
            uri: "file:///project/main.opy".to_string(),
            language_id: "opy".to_string(),
            version: 7,
            kind: crate::types::ProjectTargetKind::File,
        };
        p1.check_entry(&entry, Some("file:///project"), Some("zh-CN"))
            .unwrap();
        p1.compile_entry(&entry, Some("file:///project"), Some("zh-CN"))
            .unwrap();
        assert_eq!(recv_req(&f1)["params"]["protocolVersion"], "1.1");
        let check = recv_req(&f1);
        assert_eq!(check["method"], "lpp/check");
        assert_eq!(check["params"]["locale"], "zh-CN");
        assert_eq!(recv_req(&f1)["method"], "lpp/compile");
        f1.assert_only_requests(0);

        let (mut p2, f2) = Fake::spawn(vec![
            FakeStep::Respond(ok_response(init_result_version("1.2"))),
            FakeStep::Respond(ok_response(json!({ "documents": [] }))),
            FakeStep::Respond(ok_response(json!({ "diagnostics": [], "artifact": null }))),
        ]);
        p2.initialize_project_target(None).unwrap();
        let target = ProjectEntry {
            uri: "file:///project".to_string(),
            language_id: "opy".to_string(),
            version: 7,
            kind: crate::types::ProjectTargetKind::Directory,
        };
        p2.check_target(&target, None, None).unwrap();
        p2.compile_target(&target, None, None).unwrap();
        assert_eq!(recv_req(&f2)["params"]["protocolVersion"], "1.2");
        assert_eq!(recv_req(&f2)["params"]["entry"]["kind"], "directory");
        f2.assert_only_requests(1);
    }
}
