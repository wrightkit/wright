//! The provider-backed request-count and freshness contracts (#1005):
//! `check` issues one provider compile per run, and a mapped snapshot's
//! freshness observes the whole input tree — including the compile-time
//! `.js` inputs the mapped `files` table does not yet declare
//! (opy-rs#654) — so a non-source edit still invalidates it.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::source_provider::{
    SourceCompilation, SourceLanguage, SourceProvider, SourceProviderError, SourceTarget,
};
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceKind};

fn workspace_root() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn workshop_fixture(fixture: &str) -> String {
    std::fs::read_to_string(workspace_root().join(format!("tests/fixtures/workshop/{fixture}.ws")))
        .expect("Workshop fixture")
}

fn temp_entry() -> (PathBuf, PathBuf) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-source-provider-freshness-{}-{}",
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

/// A provider that serves the same compilation on every call — the
/// request-count and freshness tests need a repeatable compile across
/// reloads.
struct CloningProvider {
    operations: Arc<Mutex<Vec<&'static str>>>,
    compilation: SourceCompilation,
}

impl SourceProvider for CloningProvider {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Opy
    }

    fn check(&mut self, target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError> {
        self.compile(target)
    }

    fn compile(
        &mut self,
        _target: &SourceTarget,
    ) -> Result<SourceCompilation, SourceProviderError> {
        self.operations
            .lock()
            .expect("operation lock")
            .push("compile");
        Ok(self.compilation.clone())
    }
}

fn mapped_provenance(text: &str, authored: &std::path::Path) -> wright_driver::SourceProvenance {
    let catalog = workshop_rs::catalog::Catalog::builtin_ref().expect("catalog");
    let locale = workshop_rs::catalog::Locale::new("en-US");
    let program = workshop_rs::parser::parse_with_context(text, catalog, &locale, catalog)
        .expect("fixture parses");
    let json =
        workshop_rs::MappedText::new(text, workshop_rs::SourceMap::extract(&program)).to_json();
    let mut artifact: serde_json::Value = serde_json::from_str(&json).expect("mapped JSON");
    artifact["files"] = serde_json::json!([{
        "path": url::Url::from_file_path(authored).expect("file URI").to_string(),
    }]);
    let mapped = workshop_rs::MappedText::from_json(&artifact.to_string()).expect("mapped text");
    wright_driver::SourceProvenance::Mapped(mapped.map)
}

#[test]
fn provider_backend_check_issues_one_compile_per_run() {
    let (dir, entry) = temp_entry();
    let operations = Arc::new(Mutex::new(Vec::new()));
    let provider = CloningProvider {
        operations: Arc::clone(&operations),
        compilation: SourceCompilation::success(workshop_fixture("synthetic/basic-rule")),
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
    assert_eq!(
        *operations.lock().expect("operation lock"),
        vec!["compile"],
        "check makes one provider compile per run (#1005)"
    );
    cleanup(dir);
}

#[test]
fn provider_backend_check_serves_compile_from_its_own_snapshot() {
    let (dir, entry) = temp_entry();
    let operations = Arc::new(Mutex::new(Vec::new()));
    let provider = CloningProvider {
        operations: Arc::clone(&operations),
        compilation: SourceCompilation::success(workshop_fixture("synthetic/basic-rule")),
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
        vec!["compile"],
        "check's compile establishes the canonical snapshot; compile reuses it"
    );
    cleanup(dir);
}

#[test]
fn a_mapped_project_still_observes_files_the_member_list_omits() {
    let (dir, entry) = temp_entry();
    // The mapped `files` table lists source members only; a `.js` hook
    // consumed at compile time is an input the member list omits until
    // opy-rs registers it (opy-rs#654), so the snapshot keeps the
    // whole-tree fingerprint and a script edit still invalidates it.
    let script = dir.join("hook.js");
    std::fs::write(&script, "// postCompileHook input").expect("script input");
    let text = workshop_fixture("synthetic/control-flow");
    std::fs::write(dir.join("main.opy"), &text).expect("member source");
    let provenance = mapped_provenance(&text, &dir.join("main.opy"));
    let provider = CloningProvider {
        operations: Arc::new(Mutex::new(Vec::new())),
        compilation: SourceCompilation {
            workshop_text: Some(text),
            locale: None,
            provenance,
            diagnostics: Vec::new(),
            source_identity: None,
        },
    };
    let config = SessionConfig {
        input: InputSpec::Path(entry),
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::with_source_provider(config, Box::new(provider))
        .expect("provider session");
    assert!(
        session.compile().ok,
        "the mapped provider load establishes a snapshot"
    );
    let mut service = ToolService::new(&mut session).expect("service loads");

    for _ in 0..3 {
        let ToolResponse::Ok { .. } = service.handle(&ToolRequest::Project) else {
            panic!("project reads the established snapshot")
        };
    }
    assert_eq!(service.reload_count(), 0);

    std::fs::write(&script, "// an edited compile-time input").expect("edit script");
    let ToolResponse::Ok { .. } = service.handle(&ToolRequest::Project) else {
        panic!("a compile-time input edit reloads into a fresh snapshot")
    };
    assert_eq!(
        service.reload_count(),
        1,
        "an undeclared input change invalidates the mapped snapshot once (#1005)"
    );

    std::fs::write(dir.join("main.opy"), "// an edited project member").expect("edit member");
    let ToolResponse::Ok { .. } = service.handle(&ToolRequest::Project) else {
        panic!("a member edit reloads into a fresh snapshot")
    };
    assert_eq!(
        service.reload_count(),
        2,
        "a member change invalidates the snapshot exactly once"
    );
    cleanup(dir);
}
