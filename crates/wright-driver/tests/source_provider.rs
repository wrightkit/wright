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
fn provider_backend_delegates_directory_discovery_to_the_source_owner() {
    let (dir, _) = temp_entry();
    let target = Arc::new(Mutex::new(None));
    let provider = RecordingProvider {
        target: target.clone(),
        operations: Arc::new(Mutex::new(Vec::new())),
        check_compilation: None,
        compilation: Some(SourceCompilation::success(workshop_fixture(
            "synthetic/basic-rule",
        ))),
        failure: None,
    };
    let config = SessionConfig {
        input: InputSpec::Path(dir.clone()),
        kind: SourceKind::Auto,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::with_source_provider(config, Box::new(provider))
        .expect("provider session");
    let result = session.check();
    assert!(result.ok, "provider check: {:?}", result.diagnostics);
    let selected = target.lock().expect("target lock").clone().expect("target");
    assert_eq!(selected.kind, SourceTargetKind::Directory);
    assert_eq!(selected.entry, dir);
    cleanup(selected.entry);
}

#[test]
fn directory_provider_compile_uses_the_owner_source_identity() {
    let (dir, _) = temp_entry();
    let provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::new(Mutex::new(Vec::new())),
        check_compilation: None,
        compilation: Some(SourceCompilation {
            workshop_text: Some(workshop_fixture("synthetic/basic-rule")),
            locale: None,
            provenance: wright_driver::SourceProvenance::Unmapped,
            diagnostics: Vec::new(),
            source_identity: Some(wright_driver::input_identity("owner-selected source")),
        }),
        failure: None,
    };
    let config = SessionConfig {
        input: InputSpec::Path(dir.clone()),
        kind: SourceKind::Auto,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::with_source_provider(config, Box::new(provider))
        .expect("provider session");
    let result = session.compile();
    assert!(result.ok, "provider compile: {:?}", result.diagnostics);
    assert_eq!(
        result
            .result
            .output
            .expect("compiled output")
            .input_identity,
        wright_driver::input_identity("owner-selected source")
    );
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
            source_identity: None,
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
            source_identity: None,
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
fn provider_backend_lint_and_analyze_use_the_canonical_artifact_without_opy_spans() {
    let (dir, entry) = temp_entry();
    let operations = Arc::new(Mutex::new(Vec::new()));
    let provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::clone(&operations),
        check_compilation: None,
        compilation: Some(SourceCompilation {
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
        }),
        failure: None,
    };
    let config = SessionConfig {
        input: InputSpec::Path(entry),
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::with_source_provider(config, Box::new(provider))
        .expect("provider session");

    let lint = session.lint();
    assert!(lint.ok, "provider lint: {:?}", lint.diagnostics);
    assert_eq!(lint.result.program["origin"]["kind"], "provider-artifact");
    let findings = lint.result.findings.as_array().expect("finding array");
    assert!(!findings.is_empty(), "fixture supplies a lint finding");
    assert!(findings.iter().all(|finding| finding.pointer("/span/path")
        == Some(&serde_json::Value::String(
            "<provider-artifact>".to_string()
        ))));
    assert_eq!(lint.diagnostics[0].code, "owner-warning");
    assert_eq!(
        lint.diagnostics[0]
            .source
            .as_ref()
            .expect("owner source")
            .kind,
        "opy"
    );

    let analyze = session.analyze();
    assert!(analyze.ok, "provider analyze: {:?}", analyze.diagnostics);
    assert_eq!(
        analyze.result.program["origin"]["kind"],
        "provider-artifact"
    );
    assert!(!analyze.result.facts.is_null());
    assert_eq!(*operations.lock().expect("operation lock"), vec!["compile"]);
    cleanup(dir);
}

/// A source map for `text` whose spans point into `authored`, edited by
/// `edit` before decoding, as a provider would return it.
fn mapped_provenance(
    text: &str,
    authored: &Path,
    edit: impl FnOnce(&mut serde_json::Value),
) -> wright_driver::SourceProvenance {
    let catalog = workshop_rs::catalog::Catalog::builtin().expect("catalog");
    let locale = workshop_rs::catalog::Locale::new("en-US");
    let program = workshop_rs::parser::parse_with_context(text, &catalog, &locale, &catalog)
        .expect("fixture parses");
    let json = workshop_rs::MappedText {
        text: text.to_string(),
        map: workshop_rs::SourceMap::extract(&program),
    }
    .to_json();
    let mut artifact: serde_json::Value = serde_json::from_str(&json).expect("mapped JSON");
    artifact["files"] = serde_json::json!([{
        "path": url::Url::from_file_path(authored).expect("file URI").to_string(),
    }]);
    edit(&mut artifact);
    let mapped = workshop_rs::MappedText::from_json(&artifact.to_string()).expect("mapped text");
    wright_driver::SourceProvenance::Mapped(mapped.map)
}

fn mapped_lint_session(
    dir: &Path,
    entry: PathBuf,
    edit: impl FnOnce(&mut serde_json::Value),
) -> CompilerSession {
    let text = workshop_fixture("synthetic/control-flow");
    let provenance = mapped_provenance(&text, &dir.join("main.opy"), edit);
    let provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::new(Mutex::new(Vec::new())),
        check_compilation: None,
        compilation: Some(SourceCompilation {
            workshop_text: Some(text),
            locale: None,
            provenance,
            diagnostics: Vec::new(),
            source_identity: None,
        }),
        failure: None,
    };
    let config = SessionConfig {
        input: InputSpec::Path(entry),
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    };
    CompilerSession::with_source_provider(config, Box::new(provider)).expect("provider session")
}

#[test]
fn mapped_provider_artifact_attributes_findings_to_authored_source() {
    let (dir, entry) = temp_entry();
    let mut session = mapped_lint_session(&dir, entry, |_| {});

    let lint = session.lint();
    assert!(lint.ok, "mapped lint: {:?}", lint.diagnostics);
    assert_eq!(
        session.load().expect("loaded").provenance,
        wright_driver::Provenance::Mapped
    );
    assert_eq!(lint.result.program["origin"]["kind"], "opy");
    let findings = lint.result.findings.as_array().expect("finding array");
    assert!(!findings.is_empty(), "fixture supplies a lint finding");
    assert!(findings.iter().all(|finding| {
        finding.pointer("/span/path") == Some(&serde_json::Value::String("main.opy".to_string()))
            && finding
                .pointer("/span/start/line")
                .and_then(serde_json::Value::as_u64)
                .is_some_and(|line| line > 0)
    }));
    cleanup(dir);
}

#[test]
fn mapped_analyze_locations_resolve_to_authored_source() {
    fn collect(value: &serde_json::Value, spans: &mut Vec<serde_json::Value>) {
        match value {
            serde_json::Value::Object(object) => {
                if object.contains_key("file") && object.contains_key("start") {
                    spans.push(value.clone());
                } else {
                    object.values().for_each(|v| collect(v, spans));
                }
            }
            serde_json::Value::Array(items) => items.iter().for_each(|v| collect(v, spans)),
            _ => {}
        }
    }

    let (dir, entry) = temp_entry();
    let mut session = mapped_lint_session(&dir, entry, |_| {});

    let analyze = session.analyze();
    assert!(analyze.ok, "mapped analyze: {:?}", analyze.diagnostics);
    let mut spans = Vec::new();
    collect(&analyze.result.facts, &mut spans);
    assert!(!spans.is_empty(), "analyze facts carry spans");
    assert!(spans.iter().all(|span| span["path"] == "main.opy"));
    cleanup(dir);
}

/// A conforming pre-1.4 provider allows one `lpp/initialize` per process, so
/// the fallback after a refused 1.4 negotiation must use a restarted process.
#[cfg(unix)]
#[test]
fn pre_lpp_14_provider_is_restarted_before_the_unmapped_fallback() {
    use std::os::unix::fs::PermissionsExt;

    const PROVIDER: &str = r#"#!/usr/bin/env python3
import json, os, sys
artifact = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "artifact.ws")).read()
initialized = False
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
        first = not initialized
        initialized = True
        if not first:
            lpp_error(id, "alreadyInitialized", {}, "already initialized")
        elif request["params"]["protocolVersion"] != "1.1":
            lpp_error(id, "protocolVersionMismatch", {"supportedProtocolVersions": ["1.1"]}, "unsupported")
        else:
            reply(id, {"protocolVersion": "1.1", "serverInfo": {"name": "fake", "version": "0"},
                       "languages": [{"id": "opy", "extensions": ["opy"]}],
                       "capabilities": {"check": True, "compile": True, "reconstruct": False, "symbols": False, "definition": False, "references": False, "rename": False, "editValidation": False, "projectLoading": True}})
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
        workshop_fixture("synthetic/control-flow"),
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

    let lint = session.lint();
    assert!(lint.ok, "fallback lint: {:?}", lint.diagnostics);
    assert_eq!(
        session.load().expect("loaded").provenance,
        wright_driver::Provenance::Unmapped
    );
    cleanup(dir);
}

#[test]
fn nodes_without_an_authored_origin_stay_explicitly_unmapped() {
    let (dir, entry) = temp_entry();
    let mut session = mapped_lint_session(&dir, entry, |artifact| {
        artifact["spans"]
            .as_array_mut()
            .expect("span list")
            .retain(|node| node["node"] != "action");
    });

    let lint = session.lint();
    assert!(lint.ok, "mapped lint: {:?}", lint.diagnostics);
    let findings = lint.result.findings.as_array().expect("finding array");
    assert!(!findings.is_empty(), "fixture supplies a lint finding");
    assert!(
        findings
            .iter()
            .all(|finding| finding.get("span") == Some(&serde_json::Value::Null)),
        "action findings lose their span instead of borrowing another location: {findings:?}"
    );
    cleanup(dir);
}

#[test]
fn shape_mismatch_falls_back_to_unmapped_findings_with_a_diagnostic() {
    let (dir, entry) = temp_entry();
    let mut session = mapped_lint_session(&dir, entry, |artifact| {
        artifact["shape"]["rules"]
            .as_array_mut()
            .expect("rule shapes")
            .push(serde_json::json!({ "conditions": 0, "actions": 0 }));
    });

    let lint = session.lint();
    assert!(
        lint.ok,
        "mismatched map must not fail lint: {:?}",
        lint.diagnostics
    );
    assert_eq!(
        session.load().expect("loaded").provenance,
        wright_driver::Provenance::Unmapped
    );
    let mismatch = lint
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "source-map-mismatch")
        .expect("mismatch diagnostic");
    assert_eq!(mismatch.severity, wright_driver::Severity::Warning);
    let findings = lint.result.findings.as_array().expect("finding array");
    assert!(!findings.is_empty(), "fixture supplies a lint finding");
    assert!(findings.iter().all(|finding| finding.pointer("/span/path")
        == Some(&serde_json::Value::String(
            "<provider-artifact>".to_string()
        ))));
    cleanup(dir);
}

/// Set `WRIGHT_BASTION_MAIN` to `src/main.opy` of OWBastion/Bastion at revision
/// c010e1a2d468ec7140f474e334067e5ab8d02d89 and `WRIGHT_OPY_PROVIDER` to an
/// `opy-provider` that emits `workshop-rs/mapped-text-v1` to run the pinned
/// real-project attribution check.
#[test]
fn pinned_bastion_findings_resolve_to_authored_opy_locations() {
    let (Ok(main), Ok(provider)) = (
        std::env::var("WRIGHT_BASTION_MAIN"),
        std::env::var("WRIGHT_OPY_PROVIDER"),
    ) else {
        eprintln!("SKIPPED: WRIGHT_BASTION_MAIN and WRIGHT_OPY_PROVIDER are not set");
        return;
    };
    let main = PathBuf::from(main);
    let root = main.parent().expect("project source root").to_path_buf();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(main),
        kind: SourceKind::Opy,
        source_backend: SourceBackend::Provider,
        opy_provider: wright_driver::OpyProviderConfig::with_executable(PathBuf::from(provider)),
        ..SessionConfig::default()
    })
    .expect("session");

    let lint = session.lint();
    assert!(lint.ok, "Bastion lint: {:?}", lint.diagnostics);
    assert_eq!(
        session.load().expect("loaded").provenance,
        wright_driver::Provenance::Mapped,
        "the provider must hand off a source map"
    );
    let findings = lint.result.findings.as_array().expect("finding array");
    let mut mapped = 0;
    for finding in findings {
        let span = &finding["span"];
        if span.is_null() {
            continue;
        }
        let path = span["path"].as_str().expect("span path");
        assert!(path.ends_with(".opy"), "not an authored path: {path}");
        let source = std::fs::read_to_string(root.join(path)).expect("authored file");
        let line = span["start"]["line"].as_u64().expect("span line") as usize;
        assert!(
            (1..=source.lines().count()).contains(&line),
            "{path}:{line} is outside the authored file"
        );
        mapped += 1;
    }
    assert!(
        mapped > 0,
        "no Bastion finding resolved to an authored location"
    );
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

#[test]
fn provider_backend_inspect_remains_explicitly_unsupported() {
    let (dir, entry) = temp_entry();
    let config = SessionConfig {
        input: InputSpec::Path(entry),
        kind: SourceKind::Opy,
        source_backend: SourceBackend::Provider,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::new(config).expect("session");

    let result = session.inspect();
    assert!(!result.ok);
    assert_eq!(result.exit, 3);
    assert_eq!(result.diagnostics[0].code, "source-provider-unsupported");
    assert!(result.result.program.is_null());
    cleanup(dir);
}
