use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use wright_driver::source_provider::{
    SourceCompilation, SourceLanguage, SourceProvider, SourceProviderError, SourceTarget,
    SourceTargetKind,
};
use wright_driver::{
    CompilerSession, Diagnostic, InputSpec, Origin, SessionConfig, SourceBackend, SourceKind, Stage,
};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn workshop_fixture(fixture: &str) -> String {
    std::fs::read_to_string(
        workspace_root().join(format!("compatibility/fixtures/{fixture}/workshop.ws")),
    )
    .expect("Workshop fixture")
}

struct Fixture {
    dir: PathBuf,
    entry: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "wright-sp-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).expect("temp directory");
        let entry = dir.join("main.opy");
        std::fs::write(&entry, "this is intentionally not native OPY").expect("entry source");
        Self { dir, entry }
    }

    fn config(&self, path: PathBuf, kind: SourceKind) -> SessionConfig {
        SessionConfig {
            input: InputSpec::Path(path),
            kind,
            source_backend: SourceBackend::Provider,
            ..SessionConfig::default()
        }
    }

    fn session_with(&self, provider: RecordingProvider) -> CompilerSession {
        CompilerSession::with_source_provider(
            self.config(self.entry.clone(), SourceKind::Opy),
            Box::new(provider),
        )
        .expect("provider session")
    }

    fn dir_session_with(&self, provider: RecordingProvider) -> CompilerSession {
        CompilerSession::with_source_provider(
            self.config(self.dir.clone(), SourceKind::Auto),
            Box::new(provider),
        )
        .expect("provider session")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct RecordingProvider {
    target: Arc<Mutex<Option<SourceTarget>>>,
    operations: Arc<Mutex<Vec<&'static str>>>,
    check_compilation: Option<SourceCompilation>,
    compilation: Option<SourceCompilation>,
    failure: Option<SourceProviderError>,
}

type RecordingReturn = (
    RecordingProvider,
    Arc<Mutex<Option<SourceTarget>>>,
    Arc<Mutex<Vec<&'static str>>>,
);

impl RecordingProvider {
    fn new() -> RecordingReturn {
        let target = Arc::new(Mutex::new(None));
        let operations = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                target: Arc::clone(&target),
                operations: Arc::clone(&operations),
                check_compilation: None,
                compilation: None,
                failure: None,
            },
            target,
            operations,
        )
    }

    fn with_compilation(compilation: SourceCompilation) -> RecordingReturn {
        let (mut p, target, ops) = Self::new();
        p.compilation = Some(compilation);
        (p, target, ops)
    }
}

impl SourceProvider for RecordingProvider {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Opy
    }

    fn check(&mut self, target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError> {
        if self.check_compilation.is_none() {
            return self.compile(target);
        }
        self.operations.lock().unwrap().push("check");
        *self.target.lock().unwrap() = Some(target.clone());
        self.failure
            .take()
            .map(Err)
            .unwrap_or_else(|| Ok(self.check_compilation.clone().unwrap()))
    }

    fn compile(&mut self, target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError> {
        self.operations.lock().unwrap().push("compile");
        *self.target.lock().unwrap() = Some(target.clone());
        self.failure
            .take()
            .map(Err)
            .unwrap_or_else(|| Ok(self.compilation.clone().unwrap()))
    }
}

#[test]
fn provider_backend_passes_only_the_selected_entry_and_uses_canonical_workshop_handoff() {
    let fix = Fixture::new();
    let (provider, observed, operations) = RecordingProvider::with_compilation(
        SourceCompilation::success(workshop_fixture("synthetic/control-flow")),
    );
    let mut session = fix.session_with(provider);
    let result = session.compile();
    assert!(result.ok, "provider compile: {:?}", result.diagnostics);
    assert!(result.result.output.is_some());
    let lint = session.lint();
    let findings = lint.result.findings.as_array().expect("finding array");
    assert!(!findings.is_empty(), "fixture supplies a lint finding");
    assert!(findings.iter().all(|f| f.pointer("/span/path")
        == Some(&serde_json::Value::String(
            "<provider-artifact>".to_string()
        ))));
    let target = observed.lock().unwrap().clone().expect("provider target");
    assert_eq!(target.language, SourceLanguage::Opy);
    assert_eq!(target.entry, fix.entry);
    assert_eq!(target.cwd, std::env::current_dir().unwrap());
    assert_eq!(target.project_root, Some(fix.dir.clone()));
    assert_eq!(
        session.load().unwrap().provenance,
        wright_driver::Provenance::Unmapped
    );
    assert_eq!(*operations.lock().unwrap(), vec!["compile"]);
}

#[test]
fn directory_provider_operations() {
    let fix = Fixture::new();
    let (provider, target, _) = RecordingProvider::with_compilation(SourceCompilation {
        workshop_text: Some(workshop_fixture("synthetic/basic-rule")),
        locale: None,
        provenance: wright_driver::SourceProvenance::Unmapped,
        diagnostics: Vec::new(),
        source_identity: Some(wright_driver::input_identity("owner-selected source")),
    });
    let mut session = fix.dir_session_with(provider);
    let result = session.check();
    assert!(result.ok, "provider check: {:?}", result.diagnostics);
    let selected = target.lock().unwrap().clone().expect("target");
    assert_eq!(selected.kind, SourceTargetKind::Directory);
    assert_eq!(selected.entry, fix.dir);

    let compile_result = session.compile();
    assert!(
        compile_result.ok,
        "provider compile: {:?}",
        compile_result.diagnostics
    );
    assert_eq!(
        compile_result
            .result
            .output
            .expect("compiled output")
            .input_identity,
        wright_driver::input_identity("owner-selected source")
    );
}

#[test]
fn provider_backend_check_and_compile_operations() {
    let fix = Fixture::new();
    let (mut provider, _, operations) = RecordingProvider::new();
    provider.check_compilation = Some(SourceCompilation {
        workshop_text: None,
        locale: Some("zh-CN".to_string()),
        provenance: wright_driver::source_provider::SourceProvenance::Unmapped,
        diagnostics: Vec::new(),
        source_identity: None,
    });
    provider.compilation = Some(SourceCompilation::success(workshop_fixture(
        "synthetic/basic-rule",
    )));
    let mut session = fix.session_with(provider);
    assert!(session.check().ok);
    assert_eq!(*operations.lock().unwrap(), vec!["check"]);
    assert!(session.compile().ok);
    assert_eq!(*operations.lock().unwrap(), vec!["check", "compile"]);
}

#[test]
fn provider_backend_lint_analyze_and_unmapped_errors() {
    let fix = Fixture::new();
    let (provider, _, operations) = RecordingProvider::with_compilation(SourceCompilation {
        workshop_text: Some(workshop_fixture("synthetic/control-flow")),
        locale: None,
        provenance: wright_driver::SourceProvenance::Unmapped,
        diagnostics: vec![Diagnostic {
            code: "owner-warning".to_string(),
            stage: Stage::Frontend,
            severity: wright_driver::Severity::Warning,
            message: "owner-side warning".to_string(),
            status: None,
            span: None,
            source: Some(Origin {
                kind: "opy".to_string(),
                locale: None,
            }),
        }],
        source_identity: None,
    });
    let mut session = fix.session_with(provider);

    let lint = session.lint();
    assert!(lint.ok, "provider lint: {:?}", lint.diagnostics);
    assert_eq!(lint.result.program["origin"]["kind"], "provider-artifact");
    let findings = lint.result.findings.as_array().expect("finding array");
    assert!(!findings.is_empty(), "fixture supplies a lint finding");
    assert!(findings.iter().all(|f| f.pointer("/span/path")
        == Some(&serde_json::Value::String(
            "<provider-artifact>".to_string()
        ))));
    assert_eq!(lint.diagnostics[0].code, "owner-warning");
    assert_eq!(lint.diagnostics[0].source.as_ref().unwrap().kind, "opy");

    let analyze = session.analyze();
    assert!(analyze.ok && !analyze.result.facts.is_null());
    assert_eq!(*operations.lock().unwrap(), vec!["compile"]);

    let (broken_p, _, _) = RecordingProvider::with_compilation(SourceCompilation::success(
        "rule (\"broken\") {\n    actions {\n        UnknownAction;\n    }\n}\n",
    ));
    let mut broken_session = fix.session_with(broken_p);
    let result = broken_session.compile();
    assert!(!result.ok);
    assert_eq!(result.diagnostics[0].code, "unknown-action");
    assert_eq!(
        result.diagnostics[0].span.as_ref().unwrap().path,
        "<provider-artifact>"
    );
    assert_eq!(
        result.diagnostics[0].source.as_ref().unwrap().kind,
        "provider-artifact"
    );
}

#[test]
fn provider_backend_refusals_and_failures() {
    let fix = Fixture::new();

    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Stdin,
        kind: SourceKind::Opy,
        source_backend: SourceBackend::Provider,
        ..SessionConfig::default()
    })
    .unwrap();
    let result = session.check();
    assert!(
        !result.ok
            && result.exit == 3
            && result.diagnostics[0].code == "source-provider-unsupported"
    );

    let mut session = CompilerSession::new(fix.config(fix.entry.clone(), SourceKind::Opy)).unwrap();
    let result = session.inspect();
    assert!(
        !result.ok
            && result.exit == 3
            && result.diagnostics[0].code == "source-provider-unsupported"
            && result.result.program.is_null()
    );

    let mut session = CompilerSession::new(SessionConfig {
        opy_provider: wright_driver::OpyProviderConfig::with_executable(
            fix.dir.join("missing-provider"),
        ),
        ..fix.config(fix.entry.clone(), SourceKind::Opy)
    })
    .unwrap();
    let result = session.check();
    assert!(!result.ok && result.exit == 4 && result.diagnostics[0].code == "provider-missing");

    let (mut provider, _, _) = RecordingProvider::new();
    provider.failure = Some(SourceProviderError::Failed {
        code: "provider-exited".to_string(),
        message: "provider exited before compiling the entry".to_string(),
    });
    let mut session = fix.session_with(provider);
    let result = session.check();
    assert!(!result.ok && result.exit == 4 && result.diagnostics[0].code == "provider-exited");
}
