//! The source-provider tests that drive a scripted `LanguageProvider`
//! directly or spawn an LPP provider process — split from
//! `source_provider.rs` under the size limit.

use std::path::PathBuf;

use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::source_provider::{SourceLanguage, SourceProvider, SourceTarget};
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceBackend, SourceKind, Stage};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn workshop_fixture(fixture: &str) -> String {
    std::fs::read_to_string(workspace_root().join(format!("tests/fixtures/workshop/{fixture}.ws")))
        .expect("Workshop fixture")
}

fn temp_entry() -> (PathBuf, PathBuf) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-source-provider-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).expect("temp directory");
    let entry = dir.join("main.opy");
    std::fs::write(&entry, "this is intentionally not native OPY").expect("entry source");
    (dir, entry)
}

fn cleanup(dir: PathBuf) {
    std::fs::remove_dir_all(dir).expect("remove test directory");
}
struct FailingLppProvider {
    failure: wright_lpp::ProviderError,
    capabilities_failure: Option<wright_lpp::ProviderError>,
}

impl wright_lpp::LanguageProvider for FailingLppProvider {
    fn initialize(
        &mut self,
        _client_info: Option<&wright_lpp::ClientInfo>,
    ) -> Result<wright_lpp::InitializeResult, wright_lpp::ProviderError> {
        unreachable!()
    }
    fn capabilities(
        &self,
    ) -> Result<&wright_lpp::NegotiatedCapabilities, wright_lpp::ProviderError> {
        match &self.capabilities_failure {
            Some(error) => Err(error.clone()),
            None => unreachable!(),
        }
    }
    fn check(
        &mut self,
        _documents: &wright_lpp::DocumentSet,
        _project_root: Option<&str>,
    ) -> Result<wright_lpp::CheckResult, wright_lpp::ProviderError> {
        unreachable!()
    }
    fn check_entry(
        &mut self,
        _entry: &wright_lpp::ProjectEntry,
        _project_root: Option<&str>,
        _locale: Option<&str>,
    ) -> Result<wright_lpp::CheckResult, wright_lpp::ProviderError> {
        Err(self.failure.clone())
    }
    fn compile(
        &mut self,
        _documents: &wright_lpp::DocumentSet,
        _project_root: Option<&str>,
    ) -> Result<wright_lpp::CompileResult, wright_lpp::ProviderError> {
        unreachable!()
    }
    fn compile_entry(
        &mut self,
        _entry: &wright_lpp::ProjectEntry,
        _project_root: Option<&str>,
        _locale: Option<&str>,
    ) -> Result<wright_lpp::CompileResult, wright_lpp::ProviderError> {
        Err(self.failure.clone())
    }
    fn reconstruct(
        &mut self,
        _artifact: &wright_lpp::WorkshopArtifact,
    ) -> Result<wright_lpp::ReconstructResult, wright_lpp::ProviderError> {
        unreachable!()
    }
    fn symbols(
        &mut self,
        _documents: &wright_lpp::DocumentSet,
        _project_root: Option<&str>,
    ) -> Result<wright_lpp::SymbolsResult, wright_lpp::ProviderError> {
        unreachable!()
    }
    fn definition(
        &mut self,
        _document: &wright_lpp::Document,
        _position: wright_lpp::Position,
    ) -> Result<wright_lpp::LocationsResult, wright_lpp::ProviderError> {
        unreachable!()
    }
    fn references(
        &mut self,
        _document: &wright_lpp::Document,
        _position: wright_lpp::Position,
        _include_declaration: bool,
    ) -> Result<wright_lpp::LocationsResult, wright_lpp::ProviderError> {
        unreachable!()
    }
    fn rename(
        &mut self,
        _documents: &wright_lpp::DocumentSet,
        _position_document_uri: &str,
        _position: wright_lpp::Position,
        _new_name: &str,
        _project_root: Option<&str>,
    ) -> Result<wright_lpp::RenameResult, wright_lpp::ProviderError> {
        unreachable!()
    }
    fn validate_edits(
        &mut self,
        _document: &wright_lpp::Document,
        _edits: &[wright_lpp::TextEdit],
    ) -> Result<wright_lpp::ValidateEditsResult, wright_lpp::ProviderError> {
        unreachable!()
    }
    fn shutdown(&mut self) -> Result<(), wright_lpp::ProviderError> {
        Ok(())
    }
    fn exit_status(&self) -> Option<i32> {
        None
    }
}

#[test]
fn lpp_source_provider_keeps_refusal_code_and_request_context() {
    let (dir, entry) = temp_entry();
    let mut provider = wright_driver::source_provider::LppSourceProvider::new(
        Box::new(FailingLppProvider {
            failure: wright_lpp::ProviderError::lpp(
                wright_lpp::LppErrorKind::Refusal,
                serde_json::json!({"refusalCode": "compile.tooLarge"}),
                "the project exceeds the provider limit",
            ),
            capabilities_failure: None,
        }),
        None,
    );
    let error = provider
        .check(&SourceTarget::new(SourceLanguage::Opy, &entry, &dir))
        .expect_err("the provider refused");
    assert_eq!(error.code(), "provider-refusal-compile.tooLarge");
    let diagnostic = error.diagnostic();
    assert_eq!(diagnostic.stage, Stage::Frontend);
    assert!(
        diagnostic.message.contains("check"),
        "the operation is named: {}",
        diagnostic.message
    );
    assert!(
        diagnostic.message.contains(&entry.display().to_string()),
        "the entry is named: {}",
        diagnostic.message
    );
    assert!(
        diagnostic
            .message
            .contains("the project exceeds the provider limit"),
        "the provider reason survives: {}",
        diagnostic.message
    );
    cleanup(dir);
}

#[test]
fn lpp_source_provider_timeout_is_internal_with_typed_detail_in_text() {
    let (dir, entry) = temp_entry();
    let mut provider = wright_driver::source_provider::LppSourceProvider::new(
        Box::new(FailingLppProvider {
            failure: wright_lpp::ProviderError::Timeout {
                method: "lpp/check".to_string(),
                duration: std::time::Duration::from_millis(1500),
            },
            capabilities_failure: None,
        }),
        None,
    );
    let error = provider
        .check(&SourceTarget::new(SourceLanguage::Opy, &entry, &dir))
        .expect_err("the provider timed out");
    assert_eq!(error.code(), "provider-timeout");
    assert_eq!(error.diagnostic().stage, Stage::Internal);
    let message = error.to_string();
    assert!(message.contains("'lpp/check'"), "{message}");
    assert!(message.contains("1500ms"), "{message}");
    cleanup(dir);
}

#[test]
fn lpp_source_provider_propagates_a_capability_query_failure() {
    let (dir, entry) = temp_entry();
    let mut provider = wright_driver::source_provider::LppSourceProvider::new(
        Box::new(FailingLppProvider {
            failure: wright_lpp::ProviderError::lpp(
                wright_lpp::LppErrorKind::Refusal,
                serde_json::json!({"refusalCode": "compile.unreachable"}),
                "the compile request must not be reached",
            ),
            capabilities_failure: Some(wright_lpp::ProviderError::NotInitialized {
                method: "capabilities".to_string(),
            }),
        }),
        None,
    );
    let error = provider
        .compile(&SourceTarget::new(SourceLanguage::Opy, &entry, &dir))
        .expect_err("the capability query failed");
    assert_eq!(error.code(), "provider-not-initialized");
    assert_eq!(error.diagnostic().stage, Stage::Internal);
    let message = error.to_string();
    assert!(
        message.contains("compile"),
        "the operation is named: {message}"
    );
    assert!(
        message.contains(&entry.display().to_string()),
        "the entry is named: {message}"
    );
    cleanup(dir);
}

#[cfg(unix)]
#[test]
fn a_check_only_lpp_provider_keeps_the_check_only_path() {
    use std::os::unix::fs::PermissionsExt;

    const PROVIDER: &str = r#"#!/usr/bin/env python3
import json, sys, pathlib
entry = pathlib.Path(__file__).resolve().parent.joinpath("main.opy").as_uri()
def reply(id, result=None, error=None):
    message = {"jsonrpc": "2.0", "id": id}
    message.update({"error": error} if error else {"result": result})
    print(json.dumps(message), flush=True)
def lpp_error(id, kind, details, text):
    reply(id, error={"code": -32000, "message": text, "data": {"lpp": {"kind": kind, "details": details}}})
for line in sys.stdin:
    request = json.loads(line)
    id, method = request["id"], request["method"]
    if method == "lpp/initialize":
        if request["params"]["protocolVersion"] != "1.1":
            lpp_error(id, "protocolVersionMismatch", {"supportedProtocolVersions": ["1.1"]}, "unsupported")
        else:
            reply(id, {"protocolVersion": "1.1", "serverInfo": {"name": "fake", "version": "0"},
                       "languages": [{"id": "opy", "extensions": ["opy"]}],
                       "capabilities": {"check": True, "projectLoading": True}})
    elif method == "lpp/check":
        reply(id, {"documents": [{"uri": entry, "version": 0, "diagnostics": [
            {"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 4}},
             "severity": "warning", "code": "opy-lint", "message": "provider warning"}]}]})
    else:
        reply(id, {})
"#;

    let (dir, entry) = temp_entry();
    let script = dir.join("fake-provider");
    std::fs::write(&script, PROVIDER).expect("provider script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(entry),
        kind: SourceKind::Opy,
        source_backend: SourceBackend::Provider,
        opy_provider: wright_driver::OpyProviderConfig::with_executable(script),
        ..SessionConfig::default()
    })
    .expect("session");

    let check = session.check();
    assert!(
        check.ok,
        "check-only provider check: {:?}",
        check.diagnostics
    );
    assert!(
        check.diagnostics.iter().any(|d| d.code == "opy-lint"),
        "the provider's check diagnostic survives the degrade arm: {check:?}"
    );
    cleanup(dir);
}

#[cfg(unix)]
#[test]
fn a_version_mismatch_retries_at_the_highest_mutual_version() {
    use std::os::unix::fs::PermissionsExt;

    const PROVIDER: &str = r#"#!/usr/bin/env python3
import json, sys, os, pathlib
log = pathlib.Path(__file__).resolve().parent.joinpath("init-log")
artifact = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "artifact.ws")).read()
supported = ["1.0", "1.1", "1.3"]
def reply(id, result=None, error=None):
    message = {"jsonrpc": "2.0", "id": id}
    message.update({"error": error} if error else {"result": result})
    print(json.dumps(message), flush=True)
def lpp_error(id, kind, details, text):
    reply(id, error={"code": -32000, "message": text, "data": {"lpp": {"kind": kind, "details": details}}})
for line in sys.stdin:
    request = json.loads(line)
    id, method = request["id"], request["method"]
    if method == "lpp/initialize":
        params = request["params"]
        with open(log, "a") as f:
            f.write(json.dumps({"version": params["protocolVersion"],
                                "client": params["clientInfo"]["name"]}) + "\n")
        if params["protocolVersion"] not in supported:
            lpp_error(id, "protocolVersionMismatch", {"supportedProtocolVersions": supported}, "unsupported")
        else:
            reply(id, {"protocolVersion": params["protocolVersion"], "serverInfo": {"name": "fake", "version": "0"},
                       "languages": [{"id": "opy", "extensions": ["opy"]}],
                       "capabilities": {"check": True, "compile": True, "projectLoading": True}})
    elif method == "lpp/compile":
        reply(id, {"diagnostics": [], "artifact": {"format": "workshop-rs/text-v1", "content": artifact}})
    else:
        reply(id, {})
"#;

    let (dir, entry) = temp_entry();
    let script = dir.join("fake-provider");
    std::fs::write(&script, PROVIDER).expect("provider script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    std::fs::write(
        dir.join("artifact.ws"),
        workshop_fixture("synthetic/basic-rule"),
    )
    .expect("artifact");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(entry),
        kind: SourceKind::Opy,
        source_backend: SourceBackend::Provider,
        opy_provider: wright_driver::OpyProviderConfig::with_executable(script),
        ..SessionConfig::default()
    })
    .expect("session");

    let compile = session.compile();
    assert!(compile.ok, "negotiated compile: {:?}", compile.diagnostics);
    let attempts: Vec<serde_json::Value> = std::fs::read_to_string(dir.join("init-log"))
        .expect("init log")
        .lines()
        .map(|line| serde_json::from_str(line).expect("log line"))
        .collect();
    assert_eq!(
        attempts,
        vec![
            serde_json::json!({"version": "1.7", "client": "wright"}),
            serde_json::json!({"version": "1.3", "client": "wright"}),
        ],
        "the retry asks for the highest mutual version, not the next fixed rung"
    );
    cleanup(dir);
}

#[cfg(unix)]
#[test]
fn a_compile_inputs_declaration_bounds_the_snapshot_fingerprint() {
    use std::os::unix::fs::PermissionsExt;

    const PROVIDER: &str = r#"#!/usr/bin/env python3
import json, sys, os, pathlib
base = pathlib.Path(os.path.abspath(__file__)).parent
log = base.joinpath("init-log")
artifact = open(base.joinpath("artifact.ws")).read()
def reply(id, result=None, error=None):
    message = {"jsonrpc": "2.0", "id": id}
    message.update({"error": error} if error else {"result": result})
    print(json.dumps(message), flush=True)
for line in sys.stdin:
    request = json.loads(line)
    id, method = request["id"], request["method"]
    if method == "lpp/initialize":
        with open(log, "a") as f:
            f.write(json.dumps({"version": request["params"]["protocolVersion"]}) + "\n")
        reply(id, {"protocolVersion": request["params"]["protocolVersion"],
                   "serverInfo": {"name": "fake", "version": "0"},
                   "languages": [{"id": "opy", "extensions": ["opy"]}],
                   "capabilities": {"check": True, "compile": True, "projectLoading": True,
                                    "compileInputs": True}})
    elif method == "lpp/compile":
        reply(id, {"diagnostics": [],
                   "compileInputs": [base.joinpath("main.opy").as_uri(),
                                      base.joinpath("hook.js").as_uri()],
                   "artifact": {"format": "workshop-rs/text-v1", "content": artifact}})
    else:
        reply(id, {})
"#;

    let (dir, entry) = temp_entry();
    let script = dir.join("hook.js");
    let unrelated = dir.join("notes.md");
    std::fs::write(&script, "// postCompileHook input").expect("script input");
    std::fs::write(&unrelated, "# project notes").expect("unrelated file");
    let provider_script = dir.join("fake-provider");
    std::fs::write(&provider_script, PROVIDER).expect("provider script");
    std::fs::set_permissions(&provider_script, std::fs::Permissions::from_mode(0o755))
        .expect("chmod");
    std::fs::write(
        dir.join("artifact.ws"),
        workshop_fixture("synthetic/basic-rule"),
    )
    .expect("artifact");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(entry),
        kind: SourceKind::Opy,
        source_backend: SourceBackend::Provider,
        opy_provider: wright_driver::OpyProviderConfig::with_executable(provider_script),
        ..SessionConfig::default()
    })
    .expect("session");

    let compile = session.compile();
    assert!(
        compile.ok,
        "the declaring provider compiles: {:?}",
        compile.diagnostics
    );
    let attempts: Vec<serde_json::Value> = std::fs::read_to_string(dir.join("init-log"))
        .expect("init log")
        .lines()
        .map(|line| serde_json::from_str(line).expect("log line"))
        .collect();
    assert_eq!(
        attempts,
        vec![serde_json::json!({"version": "1.7"})],
        "the compile-input session negotiates LPP 1.7"
    );
    let mut service = ToolService::new(&mut session).expect("service loads");
    let ToolResponse::Ok { .. } = service.handle(&ToolRequest::Project) else {
        panic!("project reads the established snapshot")
    };
    assert_eq!(service.reload_count(), 0);

    std::fs::write(&unrelated, "# edited project notes").expect("edit unrelated");
    let ToolResponse::Ok { .. } = service.handle(&ToolRequest::Project) else {
        panic!("an undeclared file leaves the snapshot fresh")
    };
    assert_eq!(
        service.reload_count(),
        0,
        "a file outside the wire-declared input set is not observed (#1005)"
    );

    std::fs::write(&script, "// an edited compile-time input").expect("edit script");
    let ToolResponse::Ok { .. } = service.handle(&ToolRequest::Project) else {
        panic!("a declared input edit reloads into a fresh snapshot")
    };
    assert_eq!(
        service.reload_count(),
        1,
        "a declared compile-time input change invalidates the snapshot once"
    );
    cleanup(dir);
}

#[cfg(unix)]
#[test]
fn an_unadvertised_compile_inputs_field_keeps_the_wide_fingerprint() {
    use std::os::unix::fs::PermissionsExt;

    const PROVIDER: &str = r#"#!/usr/bin/env python3
import json, sys, os, pathlib
base = pathlib.Path(os.path.abspath(__file__)).parent
artifact = open(base.joinpath("artifact.ws")).read()
def reply(id, result=None, error=None):
    message = {"jsonrpc": "2.0", "id": id}
    message.update({"error": error} if error else {"result": result})
    print(json.dumps(message), flush=True)
for line in sys.stdin:
    request = json.loads(line)
    id, method = request["id"], request["method"]
    if method == "lpp/initialize":
        reply(id, {"protocolVersion": request["params"]["protocolVersion"],
                   "serverInfo": {"name": "fake", "version": "0"},
                   "languages": [{"id": "opy", "extensions": ["opy"]}],
                   "capabilities": {"check": True, "compile": True, "projectLoading": True}})
    elif method == "lpp/compile":
        reply(id, {"diagnostics": [],
                   "compileInputs": [base.joinpath("main.opy").as_uri()],
                   "artifact": {"format": "workshop-rs/text-v1", "content": artifact}})
    else:
        reply(id, {})
"#;

    let (dir, entry) = temp_entry();
    let unrelated = dir.join("notes.md");
    std::fs::write(&unrelated, "# project notes").expect("unrelated file");
    let provider_script = dir.join("fake-provider");
    std::fs::write(&provider_script, PROVIDER).expect("provider script");
    std::fs::set_permissions(&provider_script, std::fs::Permissions::from_mode(0o755))
        .expect("chmod");
    std::fs::write(
        dir.join("artifact.ws"),
        workshop_fixture("synthetic/basic-rule"),
    )
    .expect("artifact");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(entry),
        kind: SourceKind::Opy,
        source_backend: SourceBackend::Provider,
        opy_provider: wright_driver::OpyProviderConfig::with_executable(provider_script),
        ..SessionConfig::default()
    })
    .expect("session");

    // The provider answers `compileInputs` without advertising the
    // capability: the declaration is not trusted, so freshness keeps the
    // whole-tree fingerprint and an undeclared file still invalidates.
    let compile = session.compile();
    assert!(compile.ok, "compile: {:?}", compile.diagnostics);
    let mut service = ToolService::new(&mut session).expect("service loads");
    let ToolResponse::Ok { .. } = service.handle(&ToolRequest::Project) else {
        panic!("project reads the established snapshot")
    };

    std::fs::write(&unrelated, "# edited project notes").expect("edit unrelated");
    let ToolResponse::Ok { .. } = service.handle(&ToolRequest::Project) else {
        panic!("an undeclared-file edit reloads under the wide fingerprint")
    };
    assert_eq!(
        service.reload_count(),
        1,
        "an unadvertised declaration is ignored (#1005)"
    );
    cleanup(dir);
}

#[cfg(unix)]
#[test]
fn a_directory_target_requires_the_source_identity_capability() {
    use std::os::unix::fs::PermissionsExt;

    const PROVIDER: &str = r#"#!/usr/bin/env python3
import json, sys, os
artifact = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "artifact.ws")).read()
def reply(id, result=None, error=None):
    message = {"jsonrpc": "2.0", "id": id}
    message.update({"error": error} if error else {"result": result})
    print(json.dumps(message), flush=True)
for line in sys.stdin:
    request = json.loads(line)
    id, method = request["id"], request["method"]
    if method == "lpp/initialize":
        reply(id, {"protocolVersion": request["params"]["protocolVersion"], "serverInfo": {"name": "fake", "version": "0"},
                   "languages": [{"id": "opy", "extensions": ["opy"]}],
                   "capabilities": {"check": True, "compile": True, "projectLoading": True}})
    elif method == "lpp/compile":
        reply(id, {"diagnostics": [], "sourceIdentity": "0" * 64,
                   "artifact": {"format": "workshop-rs/text-v1", "content": artifact}})
    else:
        reply(id, {})
"#;

    let (dir, _entry) = temp_entry();
    let script = dir.join("fake-provider");
    std::fs::write(&script, PROVIDER).expect("provider script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    std::fs::write(
        dir.join("artifact.ws"),
        workshop_fixture("synthetic/basic-rule"),
    )
    .expect("artifact");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(dir.clone()),
        kind: SourceKind::Opy,
        source_backend: SourceBackend::Provider,
        opy_provider: wright_driver::OpyProviderConfig::with_executable(script),
        ..SessionConfig::default()
    })
    .expect("session");

    let check = session.check();
    assert!(
        !check.ok,
        "a sourceIdentity-less directory session must refuse"
    );
    assert!(
        check
            .diagnostics
            .iter()
            .any(|d| d.code == "capability-unavailable"),
        "the refusal is the typed capability diagnostic: {check:?}"
    );
    cleanup(dir);
}
