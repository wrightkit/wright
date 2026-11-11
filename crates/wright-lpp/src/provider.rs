use std::io::BufWriter;
use std::path::Path;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::client::{ClientConfig, ExchangeObserver, JsonRpcClient};
use crate::error::ProviderError;
use crate::process::ChildProcess;
use crate::types::{
    Capabilities, Capability, CheckResult, ClientInfo, CompileResult, Document, DocumentSet,
    InitializeResult, LocationsResult, LookupParams, LookupResult, Position, ProjectEntry,
    ReconstructResult, RenameResult, SymbolsResult, TextEdit, ValidateEditsResult,
    WorkshopArtifact,
};
use crate::{
    LPP_ARTIFACT_NEGOTIATION_PROTOCOL_VERSION, LPP_DIRECTORY_TARGET_PROTOCOL_VERSION,
    LPP_LOOKUP_PROTOCOL_VERSION, LPP_PROTOCOL_VERSION, protocol_version_at_least,
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

    /// Initialize with LPP 1.1 for provider-owned project loading.
    fn initialize_project_loading(
        &mut self,
        client_info: Option<&ClientInfo>,
    ) -> Result<InitializeResult, ProviderError> {
        let result = self.initialize(client_info)?;
        Err(ProviderError::ProtocolVersionMismatch {
            supported: vec![result.protocol_version],
            message: "the provider client does not support LPP 1.1 project loading".to_string(),
        })
    }

    /// Initialize with LPP 1.2 for provider-owned file or directory targets.
    fn initialize_project_target(
        &mut self,
        client_info: Option<&ClientInfo>,
    ) -> Result<InitializeResult, ProviderError> {
        let result = self.initialize_project_loading(client_info)?;
        Err(ProviderError::ProtocolVersionMismatch {
            supported: vec![result.protocol_version],
            message: "the provider client does not support LPP 1.2 directory targets".to_string(),
        })
    }

    /// Initialize with LPP 1.4 for `lpp/compile` artifact format negotiation.
    fn initialize_artifact_negotiation(
        &mut self,
        _client_info: Option<&ClientInfo>,
    ) -> Result<InitializeResult, ProviderError> {
        Err(ProviderError::ProtocolVersionMismatch {
            supported: Vec::new(),
            message: "the provider client does not support LPP 1.4 artifact negotiation"
                .to_string(),
        })
    }

    /// Initialize with LPP 1.5 for `lpp/lookup` name and signature facts.
    fn initialize_lookup(
        &mut self,
        _client_info: Option<&ClientInfo>,
    ) -> Result<InitializeResult, ProviderError> {
        Err(ProviderError::ProtocolVersionMismatch {
            supported: Vec::new(),
            message: "the provider client does not support LPP 1.5 name lookup".to_string(),
        })
    }

    /// Initialize at an arbitrary negotiated protocol version — the
    /// dispatch the highest-mutual-version retry needs after a
    /// `protocolVersionMismatch` (LPP §19.2, #638). The default maps each
    /// version constant to its named initializer; a version with no named
    /// initializer is a client-side mismatch. Transport implementations
    /// override this to send the version verbatim.
    fn initialize_version(
        &mut self,
        protocol_version: &str,
        client_info: Option<&ClientInfo>,
    ) -> Result<InitializeResult, ProviderError> {
        match protocol_version {
            crate::LPP_PROTOCOL_VERSION => self.initialize(client_info),
            crate::LPP_PROJECT_LOADING_PROTOCOL_VERSION => {
                self.initialize_project_loading(client_info)
            }
            crate::LPP_DIRECTORY_TARGET_PROTOCOL_VERSION => {
                self.initialize_project_target(client_info)
            }
            crate::LPP_ARTIFACT_NEGOTIATION_PROTOCOL_VERSION => {
                self.initialize_artifact_negotiation(client_info)
            }
            crate::LPP_LOOKUP_PROTOCOL_VERSION => self.initialize_lookup(client_info),
            _ => Err(ProviderError::ProtocolVersionMismatch {
                supported: Vec::new(),
                message: format!(
                    "the provider client has no LPP {protocol_version} session initializer"
                ),
            }),
        }
    }

    /// The negotiated capabilities, after a successful initialize.
    fn capabilities(&self) -> Result<&NegotiatedCapabilities, ProviderError>;

    /// `lpp/check`: produce diagnostics for a document set.
    fn check(
        &mut self,
        documents: &DocumentSet,
        project_root: Option<&str>,
    ) -> Result<CheckResult, ProviderError>;

    /// `lpp/check` over a provider-owned filesystem project entry.
    fn check_entry(
        &mut self,
        _entry: &ProjectEntry,
        _project_root: Option<&str>,
        _locale: Option<&str>,
    ) -> Result<CheckResult, ProviderError> {
        capability_unavailable("projectLoading", "lpp/check")
    }

    /// `lpp/check` over an LPP 1.2 file or directory target.
    fn check_target(
        &mut self,
        _target: &ProjectEntry,
        _project_root: Option<&str>,
        _locale: Option<&str>,
    ) -> Result<CheckResult, ProviderError> {
        capability_unavailable("projectLoading", "lpp/check")
    }

    /// `lpp/compile`: compile a document set into one opaque Workshop
    /// artifact.
    fn compile(
        &mut self,
        documents: &DocumentSet,
        project_root: Option<&str>,
    ) -> Result<CompileResult, ProviderError>;

    /// `lpp/compile` over a provider-owned filesystem project entry.
    fn compile_entry(
        &mut self,
        _entry: &ProjectEntry,
        _project_root: Option<&str>,
        _locale: Option<&str>,
    ) -> Result<CompileResult, ProviderError> {
        capability_unavailable("projectLoading", "lpp/compile")
    }

    /// `lpp/compile` over an LPP 1.2 file or directory target.
    fn compile_target(
        &mut self,
        _target: &ProjectEntry,
        _project_root: Option<&str>,
        _locale: Option<&str>,
    ) -> Result<CompileResult, ProviderError> {
        capability_unavailable("projectLoading", "lpp/compile")
    }

    /// `lpp/compile` over a file or directory target, most-preferred
    /// artifact format first (LPP 1.4+).
    fn compile_target_accepting(
        &mut self,
        _target: &ProjectEntry,
        _project_root: Option<&str>,
        _locale: Option<&str>,
        _accepted_artifact_formats: &[&str],
    ) -> Result<CompileResult, ProviderError> {
        capability_unavailable("projectLoading", "lpp/compile")
    }

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

    /// `lpp/lookup` (LPP 1.5): resolve a free-text name guess to the
    /// provider's vocabulary entries. Answered from the provider's language
    /// vocabulary alone — no documents, no loaded project.
    fn lookup(&mut self, _params: &LookupParams) -> Result<LookupResult, ProviderError> {
        capability_unavailable("lookup", "lpp/lookup")
    }

    /// Begin graceful shutdown without waiting for process exit, when supported.
    fn begin_shutdown(&mut self) -> Result<(), ProviderError> {
        Ok(())
    }

    /// Graceful termination: send `lpp/shutdown`, close the provider's
    /// stdin, and wait (bounded) for the provider to exit.
    fn shutdown(&mut self) -> Result<(), ProviderError>;

    /// The last observed provider exit code, when the provider exited.
    fn exit_status(&self) -> Option<i32>;
}

/// A `LanguageProvider` over a spawned stdio provider process.
pub struct StdioLanguageProvider {
    child: Option<ChildProcess>,
    exiting: Option<JoinHandle<Option<i32>>>,
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
            child: Some(child),
            exiting: None,
            client,
            negotiated: None,
            last_exit_status: None,
        })
    }

    /// Report every later exchange with this process to `observer`.
    pub fn set_exchange_observer(&mut self, observer: Arc<dyn ExchangeObserver>) {
        self.client.set_observer(observer);
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

    fn request(&mut self, method: &str, params: Value) -> Result<Value, ProviderError> {
        match self.client.request(method, params) {
            Ok(value) => Ok(value),
            Err(mut error) => {
                self.enrich_transport_error(&mut error, method);
                Err(error)
            }
        }
    }

    fn enrich_transport_error(&mut self, error: &mut ProviderError, method: &str) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        if matches!(error, ProviderError::Timeout { .. }) {
            // The client refuses every later request; kill the process too.
            child.kill();
            return;
        }
        if !matches!(error.code(), "provider-io" | "provider-exited") {
            return;
        }
        if let Some(status) = child.wait_status(Duration::from_millis(500)) {
            self.last_exit_status = Some(status);
            *error = ProviderError::Exited {
                status: Some(status),
                message: format!(
                    "the LPP provider process exited with status {status} while handling '{method}'"
                ),
            };
        }
    }

    fn shutdown_request(&mut self) -> Result<(), ProviderError> {
        if let Err(mut error) = self.client.shutdown() {
            self.enrich_transport_error(&mut error, "lpp/shutdown");
            return Err(error);
        }
        self.client.close_stdin();
        Ok(())
    }

    fn wait_for_exit(&mut self) {
        if let Some(exiting) = self.exiting.take() {
            self.last_exit_status = exiting.join().expect("provider exit thread panicked");
        }
    }

    fn require_capability(&self, capability: Capability) -> Result<(), ProviderError> {
        match &self.negotiated {
            Some(negotiated) => negotiated.require(capability),
            None => Err(ProviderError::NotInitialized {
                method: capability.method().to_string(),
            }),
        }
    }

    fn call<T: DeserializeOwned>(
        &mut self,
        cap: Capability,
        params: Value,
    ) -> Result<T, ProviderError> {
        self.require_capability(cap)?;
        let method = cap.method();
        let val = self.request(method, params)?;
        parse_result(val, method)
    }
}

impl Drop for StdioLanguageProvider {
    fn drop(&mut self) {
        // The client (stdin) drops after this, but the provider must not be
        // left running: LPP allows the client to terminate at any time.
        if let Some(child) = &mut self.child {
            child.kill();
        }
        self.wait_for_exit();
    }
}

impl LanguageProvider for StdioLanguageProvider {
    fn initialize(
        &mut self,
        client_info: Option<&ClientInfo>,
    ) -> Result<InitializeResult, ProviderError> {
        self.initialize_with_version(LPP_PROTOCOL_VERSION, client_info)
    }

    fn initialize_project_loading(
        &mut self,
        client_info: Option<&ClientInfo>,
    ) -> Result<InitializeResult, ProviderError> {
        self.initialize_with_version(crate::LPP_PROJECT_LOADING_PROTOCOL_VERSION, client_info)
    }

    fn initialize_project_target(
        &mut self,
        client_info: Option<&ClientInfo>,
    ) -> Result<InitializeResult, ProviderError> {
        self.initialize_with_version(LPP_DIRECTORY_TARGET_PROTOCOL_VERSION, client_info)
    }

    fn initialize_artifact_negotiation(
        &mut self,
        client_info: Option<&ClientInfo>,
    ) -> Result<InitializeResult, ProviderError> {
        self.initialize_with_version(LPP_ARTIFACT_NEGOTIATION_PROTOCOL_VERSION, client_info)
    }

    fn initialize_lookup(
        &mut self,
        client_info: Option<&ClientInfo>,
    ) -> Result<InitializeResult, ProviderError> {
        self.initialize_with_version(LPP_LOOKUP_PROTOCOL_VERSION, client_info)
    }

    fn initialize_version(
        &mut self,
        protocol_version: &str,
        client_info: Option<&ClientInfo>,
    ) -> Result<InitializeResult, ProviderError> {
        self.initialize_with_version(protocol_version, client_info)
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
        self.call(Capability::Check, documents_params(documents, project_root))
    }

    fn check_entry(
        &mut self,
        entry: &ProjectEntry,
        project_root: Option<&str>,
        locale: Option<&str>,
    ) -> Result<CheckResult, ProviderError> {
        self.require_capability(Capability::ProjectLoading)?;
        self.call(Capability::Check, entry_params(entry, project_root, locale))
    }

    fn check_target(
        &mut self,
        target: &ProjectEntry,
        project_root: Option<&str>,
        locale: Option<&str>,
    ) -> Result<CheckResult, ProviderError> {
        self.check_entry(target, project_root, locale)
    }

    fn compile(
        &mut self,
        documents: &DocumentSet,
        project_root: Option<&str>,
    ) -> Result<CompileResult, ProviderError> {
        self.call(
            Capability::Compile,
            documents_params(documents, project_root),
        )
    }

    fn compile_entry(
        &mut self,
        entry: &ProjectEntry,
        project_root: Option<&str>,
        locale: Option<&str>,
    ) -> Result<CompileResult, ProviderError> {
        self.require_capability(Capability::ProjectLoading)?;
        self.call(
            Capability::Compile,
            entry_params(entry, project_root, locale),
        )
    }

    fn compile_target(
        &mut self,
        target: &ProjectEntry,
        project_root: Option<&str>,
        locale: Option<&str>,
    ) -> Result<CompileResult, ProviderError> {
        self.compile_entry(target, project_root, locale)
    }

    fn compile_target_accepting(
        &mut self,
        target: &ProjectEntry,
        project_root: Option<&str>,
        locale: Option<&str>,
        accepted_artifact_formats: &[&str],
    ) -> Result<CompileResult, ProviderError> {
        self.require_capability(Capability::ProjectLoading)?;
        let negotiated = self.capabilities()?.protocol_version.clone();
        if !protocol_version_at_least(&negotiated, LPP_ARTIFACT_NEGOTIATION_PROTOCOL_VERSION) {
            return Err(ProviderError::ProtocolVersionMismatch {
                supported: vec![negotiated],
                message: "acceptedArtifactFormats requires LPP 1.4 or later".to_string(),
            });
        }
        let mut params = entry_params(target, project_root, locale);
        params["acceptedArtifactFormats"] = json!(accepted_artifact_formats);
        self.call(Capability::Compile, params)
    }

    fn reconstruct(
        &mut self,
        artifact: &WorkshopArtifact,
    ) -> Result<ReconstructResult, ProviderError> {
        self.call(Capability::Reconstruct, json!({ "artifact": artifact }))
    }

    fn symbols(
        &mut self,
        documents: &DocumentSet,
        project_root: Option<&str>,
    ) -> Result<SymbolsResult, ProviderError> {
        self.call(
            Capability::Symbols,
            documents_params(documents, project_root),
        )
    }

    fn definition(
        &mut self,
        document: &Document,
        position: Position,
    ) -> Result<LocationsResult, ProviderError> {
        self.call(
            Capability::Definition,
            json!({ "document": document, "position": position }),
        )
    }

    fn references(
        &mut self,
        document: &Document,
        position: Position,
        include_declaration: bool,
    ) -> Result<LocationsResult, ProviderError> {
        self.call(
            Capability::References,
            json!({
                "document": document,
                "position": position,
                "includeDeclaration": include_declaration,
            }),
        )
    }

    fn rename(
        &mut self,
        documents: &DocumentSet,
        position_document_uri: &str,
        position: Position,
        new_name: &str,
        project_root: Option<&str>,
    ) -> Result<RenameResult, ProviderError> {
        let mut params = json!({
            "documents": documents,
            "positionDocumentUri": position_document_uri,
            "position": position,
            "newName": new_name,
        });
        if let Some(root) = project_root {
            params["projectRoot"] = json!(root);
        }
        self.call(Capability::Rename, params)
    }

    fn validate_edits(
        &mut self,
        document: &Document,
        edits: &[TextEdit],
    ) -> Result<ValidateEditsResult, ProviderError> {
        self.call(
            Capability::EditValidation,
            json!({
                "document": document,
                "edits": edits,
            }),
        )
    }

    fn lookup(&mut self, params: &LookupParams) -> Result<LookupResult, ProviderError> {
        self.call(
            Capability::Lookup,
            serde_json::to_value(params).expect("lookup params serialize"),
        )
    }

    fn begin_shutdown(&mut self) -> Result<(), ProviderError> {
        self.shutdown_request()?;
        let mut child = self.child.take().expect("provider process is running");
        self.exiting = Some(std::thread::spawn(move || child.terminate()));
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), ProviderError> {
        if self.exiting.is_some() {
            self.wait_for_exit();
        } else {
            self.shutdown_request()?;
            self.last_exit_status = self
                .child
                .as_mut()
                .expect("provider process is running")
                .terminate();
        }
        Ok(())
    }

    fn exit_status(&self) -> Option<i32> {
        self.last_exit_status
    }
}

fn documents_params(documents: &DocumentSet, project_root: Option<&str>) -> Value {
    let mut params =
        json!({ "documents": serde_json::to_value(documents).expect("document set serializes") });
    if let Some(root) = project_root {
        params["projectRoot"] = json!(root);
    }
    params
}

fn entry_params(entry: &ProjectEntry, project_root: Option<&str>, locale: Option<&str>) -> Value {
    let mut params =
        json!({ "entry": serde_json::to_value(entry).expect("project entry serializes") });
    if let Some(root) = project_root {
        params["projectRoot"] = json!(root);
    }
    if let Some(locale) = locale {
        params["locale"] = json!(locale);
    }
    params
}

fn capability_unavailable<T>(capability: &str, method: &str) -> Result<T, ProviderError> {
    Err(ProviderError::lpp(
        crate::error::LppErrorKind::CapabilityUnavailable,
        json!({ "capability": capability, "method": method }),
        format!("capability '{capability}' is not available in this provider client"),
    ))
}

fn parse_result<T: DeserializeOwned>(value: Value, method: &str) -> Result<T, ProviderError> {
    serde_json::from_value(value).map_err(|error| ProviderError::Malformed {
        detail: format!("{method} result is not a valid LPP v1 response: {error}"),
    })
}

#[cfg(test)]
mod tests;
