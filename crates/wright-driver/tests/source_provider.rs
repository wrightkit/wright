use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use wright_driver::source_provider::{
    SourceCompilation, SourceLanguage, SourceProvider, SourceProviderError, SourceTarget,
};
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceBackend, SourceKind};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn workshop_fixture(fixture: &str) -> String {
    let oracle = std::fs::read_to_string(
        workspace_root().join(format!("compatibility/fixtures/{fixture}/oracle.json")),
    )
    .expect("fixture oracle");
    serde_json::from_str::<serde_json::Value>(&oracle)
        .expect("oracle JSON")
        .pointer("/compile/workshop")
        .and_then(serde_json::Value::as_str)
        .expect("Workshop artifact")
        .to_string()
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
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("trash")
            .arg(&dir)
            .status()
            .expect("trash command");
        assert!(status.success(), "trash failed for {}", dir.display());
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::fs::remove_dir_all(dir).expect("remove test directory");
    }
}

struct RecordingProvider {
    target: Arc<Mutex<Option<SourceTarget>>>,
    operations: Arc<Mutex<Vec<&'static str>>>,
    check_compilation: Option<SourceCompilation>,
    compilation: Option<SourceCompilation>,
    failure: Option<SourceProviderError>,
}

impl SourceProvider for RecordingProvider {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Opy
    }

    fn check(&mut self, target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError> {
        if self.check_compilation.is_none() {
            return self.compile(target);
        }
        self.operations
            .lock()
            .expect("operation lock")
            .push("check");
        *self.target.lock().expect("target lock") = Some(target.clone());
        if let Some(error) = self.failure.take() {
            return Err(error);
        }
        Ok(self.check_compilation.take().expect("check result"))
    }

    fn compile(&mut self, target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError> {
        self.operations
            .lock()
            .expect("operation lock")
            .push("compile");
        *self.target.lock().expect("target lock") = Some(target.clone());
        if let Some(error) = self.failure.take() {
            return Err(error);
        }
        Ok(self.compilation.take().expect("provider result"))
    }
}

#[test]
fn provider_backend_passes_only_the_selected_entry_and_uses_canonical_workshop_handoff() {
    let (dir, entry) = temp_entry();
    let observed = Arc::new(Mutex::new(None));
    let operations = Arc::new(Mutex::new(Vec::new()));
    let provider = RecordingProvider {
        target: Arc::clone(&observed),
        operations: Arc::clone(&operations),
        check_compilation: None,
        compilation: Some(SourceCompilation::success(workshop_fixture(
            "synthetic/control-flow",
        ))),
        failure: None,
    };
    let config = SessionConfig {
        input: InputSpec::Path(entry.clone()),
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::with_source_provider(config, Box::new(provider))
        .expect("provider session");
    let result = session.compile();
    assert!(result.ok, "provider compile: {:?}", result.diagnostics);
    assert!(result.result.output.is_some());
    let lint = session.lint();
    let findings = lint.result.findings.as_array().expect("finding array");
    assert!(!findings.is_empty(), "fixture supplies a lint finding");
    assert!(findings.iter().all(|finding| {
        finding.pointer("/span/path")
            == Some(&serde_json::Value::String(
                "<provider-artifact>".to_string(),
            ))
    }));
    let target = observed
        .lock()
        .expect("target lock")
        .clone()
        .expect("provider target");
    assert_eq!(target.language, SourceLanguage::Opy);
    assert_eq!(target.entry, entry);
    assert_eq!(target.cwd, std::env::current_dir().expect("cwd"));
    assert_eq!(target.project_root, Some(dir.clone()));
    assert_eq!(
        session.load().expect("cached provider result").provenance,
        wright_driver::Provenance::Unmapped
    );
    assert_eq!(*operations.lock().expect("operation lock"), vec!["compile"]);
    cleanup(dir);
}

#[test]
fn provider_backend_check_uses_the_provider_check_operation() {
    let (dir, entry) = temp_entry();
    let operations = Arc::new(Mutex::new(Vec::new()));
    let provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::clone(&operations),
        check_compilation: Some(SourceCompilation {
            workshop_text: None,
            locale: Some("zh-CN".to_string()),
            provenance: wright_driver::source_provider::SourceProvenance::Unmapped,
            diagnostics: Vec::new(),
        }),
        compilation: Some(SourceCompilation::success("unused")),
        failure: None,
    };
    let config = SessionConfig {
        input: InputSpec::Path(entry),
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::with_source_provider(config, Box::new(provider))
        .expect("provider session");
    let result = session.check();
    assert!(result.ok, "provider check: {:?}", result.diagnostics);
    assert_eq!(*operations.lock().expect("operation lock"), vec!["check"]);
    cleanup(dir);
}

#[test]
fn provider_backend_does_not_reuse_a_check_load_for_compile() {
    let (dir, entry) = temp_entry();
    let operations = Arc::new(Mutex::new(Vec::new()));
    let provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::clone(&operations),
        check_compilation: Some(SourceCompilation {
            workshop_text: None,
            locale: None,
            provenance: wright_driver::source_provider::SourceProvenance::Unmapped,
            diagnostics: Vec::new(),
        }),
        compilation: Some(SourceCompilation::success(workshop_fixture(
            "synthetic/basic-rule",
        ))),
        failure: None,
    };
    let config = SessionConfig {
        input: InputSpec::Path(entry),
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::with_source_provider(config, Box::new(provider))
        .expect("provider session");
    assert!(session.check().ok);
    assert!(session.compile().ok);
    assert_eq!(
        *operations.lock().expect("operation lock"),
        vec!["check", "compile"]
    );
    cleanup(dir);
}

#[test]
fn provider_backend_rejects_stdin_without_fabricating_an_entry() {
    let config = SessionConfig {
        input: InputSpec::Stdin,
        kind: SourceKind::Opy,
        source_backend: SourceBackend::Provider,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::new(config).expect("session");
    let result = session.check();
    assert!(!result.ok);
    assert_eq!(result.exit, 3);
    assert_eq!(result.diagnostics[0].code, "source-provider-unsupported");
}

#[test]
fn unmapped_provider_artifact_errors_do_not_claim_the_opy_entry() {
    let (dir, entry) = temp_entry();
    let provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::new(Mutex::new(Vec::new())),
        check_compilation: None,
        compilation: Some(SourceCompilation::success(
            "rule (\"broken\") {\n    actions {\n        UnknownAction;\n    }\n}\n",
        )),
        failure: None,
    };
    let config = SessionConfig {
        input: InputSpec::Path(entry),
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::with_source_provider(config, Box::new(provider))
        .expect("provider session");
    let result = session.compile();
    assert!(!result.ok);
    assert_eq!(result.diagnostics[0].code, "unknown-action");
    assert_eq!(
        result.diagnostics[0]
            .span
            .as_ref()
            .expect("artifact span")
            .path,
        "<provider-artifact>"
    );
    assert_eq!(
        result.diagnostics[0]
            .source
            .as_ref()
            .expect("artifact origin")
            .kind,
        "provider-artifact"
    );
    cleanup(dir);
}

#[test]
fn provider_backend_does_not_fall_back_when_the_provider_fails() {
    let (dir, entry) = temp_entry();
    let provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::new(Mutex::new(Vec::new())),
        check_compilation: None,
        compilation: None,
        failure: Some(SourceProviderError::Failed {
            code: "provider-exited".to_string(),
            message: "provider exited before compiling the entry".to_string(),
        }),
    };
    let config = SessionConfig {
        input: InputSpec::Path(entry),
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::with_source_provider(config, Box::new(provider))
        .expect("provider session");
    let result = session.check();
    assert!(!result.ok);
    assert_eq!(result.exit, 4);
    assert_eq!(result.diagnostics[0].code, "provider-exited");
    cleanup(dir);
}

#[test]
fn provider_backend_provider_resolution_failure_is_explicit() {
    let (dir, entry) = temp_entry();
    let config = SessionConfig {
        input: InputSpec::Path(entry),
        kind: SourceKind::Opy,
        source_backend: SourceBackend::Provider,
        opy_provider: wright_driver::OpyProviderConfig::with_executable(
            dir.join("missing-provider"),
        ),
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::new(config).expect("session");
    let result = session.check();
    assert!(!result.ok);
    assert_eq!(result.exit, 4);
    assert_eq!(result.diagnostics[0].code, "provider-missing");
    cleanup(dir);
}
