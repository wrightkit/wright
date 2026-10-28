use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
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

fn failed(
    code: &str,
    stage: Stage,
    class: wright_driver::ExitClass,
    message: impl Into<String>,
) -> SourceProviderError {
    SourceProviderError::Failed {
        code: code.to_string(),
        stage,
        class,
        message: message.into(),
    }
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
    check_failure: Option<SourceProviderError>,
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
        if let Some(error) = self.check_failure.take() {
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
        check_failure: None,
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
        check_compilation: Some(SourceCompilation {
            workshop_text: None,
            locale: None,
            provenance: wright_driver::SourceProvenance::Unmapped,
            diagnostics: Vec::new(),
            source_identity: None,
        }),
        compilation: Some(SourceCompilation {
            workshop_text: Some(workshop_fixture("synthetic/basic-rule")),
            locale: None,
            provenance: wright_driver::SourceProvenance::Unmapped,
            diagnostics: Vec::new(),
            source_identity: Some(wright_driver::input_identity("owner-selected source")),
        }),
        check_failure: None,
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
        check_failure: None,
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
fn provider_backend_check_falls_back_to_the_check_operation_on_a_check_only_provider() {
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
        compilation: None,
        check_failure: None,
        failure: Some(SourceProviderError::Unsupported {
            message: "this provider has no canonical compile capability".to_string(),
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
    assert!(result.ok, "provider check: {:?}", result.diagnostics);
    assert_eq!(
        *operations.lock().expect("operation lock"),
        vec!["compile", "check"],
        "check compiles first and falls back to the check operation only on a capability decline"
    );
    cleanup(dir);
}

#[test]
fn provider_backend_serves_compile_from_the_check_established_snapshot() {
    let (dir, entry) = temp_entry();
    let operations = Arc::new(Mutex::new(Vec::new()));
    let provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::clone(&operations),
        check_compilation: None,
        compilation: Some(SourceCompilation::success(workshop_fixture(
            "synthetic/basic-rule",
        ))),
        check_failure: None,
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
    assert_eq!(*operations.lock().expect("operation lock"), vec!["compile"]);
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
                source: Some(Origin {
                    kind: "opy".to_string(),
                    locale: None,
                }),
                ..Diagnostic::warning("owner-warning", Stage::Frontend, "owner-side warning")
            }],
            source_identity: None,
        }),
        check_failure: None,
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

fn mapped_provenance(
    text: &str,
    authored: &Path,
    edit: impl FnOnce(&mut serde_json::Value),
) -> wright_driver::SourceProvenance {
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
    edit(&mut artifact);
    let mapped = workshop_rs::MappedText::from_json(&artifact.to_string()).expect("mapped text");
    wright_driver::SourceProvenance::Mapped(mapped.map)
}

fn collect_spans(value: &serde_json::Value, spans: &mut Vec<serde_json::Value>) {
    match value {
        serde_json::Value::Object(object) => {
            if object.contains_key("file") && object.contains_key("start") {
                spans.push(value.clone());
            } else {
                object.values().for_each(|v| collect_spans(v, spans));
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|v| collect_spans(v, spans)),
        _ => {}
    }
}

fn mapped_lint_session(
    dir: &Path,
    entry: PathBuf,
    edit: impl FnOnce(&mut serde_json::Value),
) -> CompilerSession {
    let text = workshop_fixture("synthetic/control-flow");
    std::fs::write(dir.join("main.opy"), &text).expect("member source");
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
        check_failure: None,
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
    let (dir, entry) = temp_entry();
    let mut session = mapped_lint_session(&dir, entry, |_| {});

    let analyze = session.analyze();
    assert!(analyze.ok, "mapped analyze: {:?}", analyze.diagnostics);
    let mut spans = Vec::new();
    collect_spans(&analyze.result.facts, &mut spans);
    assert!(!spans.is_empty(), "analyze facts carry spans");
    assert!(spans.iter().all(|span| span["path"] == "main.opy"));
    cleanup(dir);
}

#[test]
fn mapped_provider_project_reports_its_source_file_count() {
    let (dir, entry) = temp_entry();
    let member = dir.join("member.opy");
    let mut session = mapped_lint_session(&dir, entry, |artifact| {
        artifact["files"]
            .as_array_mut()
            .expect("file table")
            .push(serde_json::json!({
                "path": url::Url::from_file_path(&member).expect("file URI").to_string(),
            }));
    });

    let analyze = session.analyze();
    assert!(analyze.ok, "mapped analyze: {:?}", analyze.diagnostics);
    assert_eq!(analyze.result.program["files"], 2);
    let lint = session.lint();
    assert!(lint.ok, "mapped lint: {:?}", lint.diagnostics);
    assert_eq!(lint.result.program["files"], 2);

    let mut service = ToolService::new(&mut session).expect("service loads");
    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Project) else {
        panic!("project failed");
    };
    assert_eq!(result["files"], 2, "project and program summaries agree");
    cleanup(dir);
}

#[test]
fn mapped_span_outside_retained_source_degrades_extents_not_the_load() {
    let (dir, entry) = temp_entry();
    let mut session = mapped_lint_session(&dir, entry, |artifact| {
        let node = artifact["spans"]
            .as_array_mut()
            .expect("spans")
            .iter_mut()
            .find(|node| node.get("span").is_some_and(|span| span.is_object()))
            .expect("a node carrying a span");
        node["span"]["end"] = serde_json::json!({"line": 9999, "column": 1});
    });

    let lint = session.lint();
    assert!(lint.ok, "mapped lint: {:?}", lint.diagnostics);
    assert_eq!(
        session.load().expect("loaded").provenance,
        wright_driver::Provenance::Mapped
    );
    cleanup(dir);
}

#[test]
fn unreadable_mapped_member_warns_instead_of_silently_dropping() {
    let (dir, entry) = temp_entry();
    let mut session = mapped_lint_session(&dir, entry, |artifact| {
        artifact["files"]
            .as_array_mut()
            .expect("file table")
            .push(serde_json::json!({
                "path": url::Url::from_file_path(dir.join("missing.opy"))
                    .expect("file URI")
                    .to_string(),
            }));
    });

    let lint = session.lint();
    assert!(lint.ok, "mapped lint: {:?}", lint.diagnostics);
    assert!(
        lint.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "source-map-member-unreadable")
    );
    cleanup(dir);
}

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

#[cfg(unix)]
#[test]
fn mapped_lint_fix_validates_and_writes_through_the_provider() {
    use std::os::unix::fs::PermissionsExt;

    const PROVIDER: &str = r#"#!/usr/bin/env python3
import json, sys
def reply(id, result=None, error=None):
    message = {"jsonrpc": "2.0", "id": id}
    message.update({"error": error} if error else {"result": result})
    print(json.dumps(message), flush=True)
for line in sys.stdin:
    request = json.loads(line)
    id, method = request["id"], request["method"]
    if method == "lpp/initialize":
        reply(id, {"protocolVersion": "1.0", "serverInfo": {"name": "fake", "version": "0"},
                   "languages": [{"id": "opy", "extensions": ["opy"]}],
                   "capabilities": {"check": True, "compile": False, "reconstruct": False, "symbols": False, "definition": False, "references": False, "rename": False, "editValidation": True, "projectLoading": False}})
    elif method == "lpp/validateEdits":
        reply(id, {"valid": True, "version": request["params"]["document"]["version"]})
    elif method == "lpp/check":
        reply(id, {"documents": []})
    else:
        reply(id, {})
"#;

    const OPY: &str = "globalvar total\n\nrule \"dup\":\n    @Event global\n    if total == 0:\n        total = 1\n    elif total == 0:\n        total = 3\n    else:\n        total = 4\n";
    const FIXED_OPY: &str = "globalvar total\n\nrule \"dup\":\n    @Event global\n    if total == 0:\n        total = 1\n    else:\n        total = 4\n";

    const ARTIFACT: &str = "variables {\n    global:\n        0: total\n}\nrule (\"dup\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        If(Compare(Global.total, ==, 0));\n            Set Global Variable(total, 1);\n        Else If(Compare(Global.total, ==, 0));\n            Set Global Variable(total, 3);\n        Else;\n            Set Global Variable(total, 4);\n        End;\n    }\n}\n";
    const ARTIFACT_FIXED: &str = "variables {\n    global:\n        0: total\n}\nrule (\"dup\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        If(Compare(Global.total, ==, 0));\n            Set Global Variable(total, 1);\n        Else;\n            Set Global Variable(total, 4);\n        End;\n    }\n}\n";

    fn marker_map(text: &str, authored: &Path) -> wright_driver::SourceProvenance {
        mapped_provenance(text, authored, |artifact| {
            let spans = artifact["spans"].as_array_mut().expect("span list");
            spans.clear();
            let span = |sl: u32, sc: u32, el: u32, ec: u32| {
                serde_json::json!({
                    "file": 0,
                    "start": {"line": sl, "column": sc},
                    "end": {"line": el, "column": ec},
                })
            };
            for (action, sl, sc, el, ec) in [
                (0usize, 5, 5, 5, 7),
                (2, 7, 5, 7, 9),
                (4, 9, 5, 9, 9),
                (6, 11, 1, 11, 1),
            ] {
                spans.push(serde_json::json!({
                    "node": "action", "rule": 0, "action": action,
                    "span": span(sl, sc, el, ec),
                }));
            }
            spans.push(serde_json::json!({
                "node": "action_argument", "rule": 0, "action": 2, "argument": 0,
                "span": span(7, 10, 7, 20),
            }));
        })
    }

    struct FixProvider {
        entry: PathBuf,
    }

    impl SourceProvider for FixProvider {
        fn language(&self) -> SourceLanguage {
            SourceLanguage::Opy
        }

        fn compile(
            &mut self,
            _target: &SourceTarget,
        ) -> Result<SourceCompilation, SourceProviderError> {
            let edited = !std::fs::read_to_string(&self.entry)
                .unwrap()
                .contains("elif");
            Ok(SourceCompilation {
                workshop_text: Some(if edited { ARTIFACT_FIXED } else { ARTIFACT }.to_string()),
                locale: None,
                provenance: if edited {
                    wright_driver::SourceProvenance::Unmapped
                } else {
                    marker_map(ARTIFACT, &self.entry)
                },
                diagnostics: Vec::new(),
                source_identity: None,
            })
        }
    }

    let (dir, entry) = temp_entry();
    std::fs::write(&entry, OPY).expect("entry source");
    let script = dir.join("fake-provider");
    std::fs::write(&script, PROVIDER).expect("provider script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");

    let mut session = CompilerSession::with_source_provider(
        SessionConfig {
            input: InputSpec::Path(entry.clone()),
            kind: SourceKind::Opy,
            opy_provider: wright_driver::OpyProviderConfig::with_executable(script),
            ..SessionConfig::default()
        },
        Box::new(FixProvider {
            entry: entry.clone(),
        }),
    )
    .expect("provider session");

    let preview = session.lint_fix(false);
    assert!(preview.ok, "preview lint: {:?}", preview.diagnostics);
    let outcomes = preview.result.fixes.expect("fix outcomes");
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    let outcome = &outcomes[0];
    assert_eq!(
        outcome.status,
        wright_driver::result::LintFixStatus::Preview
    );
    assert_eq!(outcome.code, "duplicate-condition");
    let rendered = outcome.preview.as_ref().expect("validated preview");
    assert_eq!(rendered.len(), 1);
    assert_eq!(rendered[0].new_text, FIXED_OPY);
    assert_eq!(rendered[0].original, OPY);
    assert_eq!(
        rendered[0].source,
        entry.display().to_string(),
        "the preview names the authored member, not the artifact"
    );
    assert_eq!(
        std::fs::read_to_string(&entry).unwrap(),
        OPY,
        "a preview run writes nothing"
    );

    let written = session.lint_fix(true);
    assert!(written.ok, "write lint: {:?}", written.diagnostics);
    assert!(
        written
            .result
            .fixes
            .as_ref()
            .expect("fix outcomes")
            .iter()
            .any(|outcome| outcome.status == wright_driver::result::LintFixStatus::Applied),
        "the dead-branch fix applies: {:?}",
        written.result.fixes
    );
    assert_eq!(std::fs::read_to_string(&entry).unwrap(), FIXED_OPY);
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
    let mut spans = Vec::new();
    collect_spans(&lint.result.findings, &mut spans);
    let lint_spans = spans.len();
    let analyze = session.analyze();
    assert!(analyze.ok, "Bastion analyze: {:?}", analyze.diagnostics);
    collect_spans(&analyze.result.facts, &mut spans);
    assert!(
        lint_spans > 0,
        "no Bastion finding resolved to an authored location"
    );
    assert!(
        spans.len() > lint_spans,
        "no Bastion analyze location resolved to an authored location"
    );
    for span in &spans {
        let path = span["path"].as_str().expect("span path");
        assert!(path.ends_with(".opy"), "not an authored path: {path}");
        let source = std::fs::read_to_string(root.join(path)).expect("authored file");
        let line = span["start"]["line"].as_u64().expect("span line") as usize;
        assert!(
            (1..=source.lines().count()).contains(&line),
            "{path}:{line} is outside the authored file"
        );
    }
}

struct CheckOnlyProvider {
    operations: Arc<Mutex<Vec<&'static str>>>,
}

impl SourceProvider for CheckOnlyProvider {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Opy
    }

    fn check(&mut self, _target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError> {
        self.operations
            .lock()
            .expect("operation lock")
            .push("check");
        Ok(SourceCompilation {
            workshop_text: None,
            locale: Some("en-US".to_string()),
            provenance: wright_driver::SourceProvenance::Unmapped,
            diagnostics: Vec::new(),
            source_identity: None,
        })
    }

    fn compile(
        &mut self,
        _target: &SourceTarget,
    ) -> Result<SourceCompilation, SourceProviderError> {
        self.operations
            .lock()
            .expect("operation lock")
            .push("compile");
        Err(SourceProviderError::Unsupported {
            message: "this provider has no canonical compile capability".to_string(),
        })
    }
}

struct ReloadableProvider {
    operations: Arc<Mutex<Vec<&'static str>>>,
    workshop_text: String,
}

impl SourceProvider for ReloadableProvider {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Opy
    }

    fn check(&mut self, _target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError> {
        self.operations
            .lock()
            .expect("operation lock")
            .push("check");
        Ok(SourceCompilation {
            workshop_text: None,
            locale: Some("en-US".to_string()),
            provenance: wright_driver::SourceProvenance::Unmapped,
            diagnostics: Vec::new(),
            source_identity: None,
        })
    }

    fn compile(
        &mut self,
        _target: &SourceTarget,
    ) -> Result<SourceCompilation, SourceProviderError> {
        self.operations
            .lock()
            .expect("operation lock")
            .push("compile");
        Ok(SourceCompilation::success(self.workshop_text.clone()))
    }
}

#[test]
fn check_reports_the_catalog_validation_errors_compile_reports() {
    let (dir, entry) = temp_entry();
    let catalog_error = Diagnostic {
        span: Some(wright_driver::SourceSpan {
            file: 0,
            path: entry.display().to_string(),
            start: wright_driver::Position { line: 10, col: 85 },
            end: wright_driver::Position { line: 10, col: 128 },
        }),
        ..Diagnostic::error(
            "catalog-validation",
            Stage::Frontend,
            "action 'playEffect' argument 4 must have semantic type 'Vector|Player', got Array",
        )
    };
    let provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::new(Mutex::new(Vec::new())),
        check_compilation: Some(SourceCompilation {
            workshop_text: None,
            locale: Some("en-US".to_string()),
            provenance: wright_driver::SourceProvenance::Unmapped,
            diagnostics: Vec::new(),
            source_identity: None,
        }),
        compilation: Some(SourceCompilation {
            workshop_text: Some(workshop_fixture("synthetic/basic-rule")),
            locale: Some("en-US".to_string()),
            provenance: wright_driver::SourceProvenance::Unmapped,
            diagnostics: vec![catalog_error.clone()],
            source_identity: None,
        }),
        check_failure: None,
        failure: None,
    };
    let config = SessionConfig {
        input: InputSpec::Path(entry.clone()),
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::with_source_provider(config, Box::new(provider))
        .expect("provider session");
    let check = session.check();
    assert!(
        !check.ok,
        "check must fail like compile on a catalog-invalid artifact: {check:?}"
    );
    let check_findings: Vec<_> = check
        .diagnostics
        .iter()
        .map(|d| (d.code.as_str(), d.span.clone()))
        .collect();
    assert_eq!(
        check_findings.len(),
        1,
        "exactly compile's error, not a synthetic one: {check_findings:?}"
    );
    assert_eq!(check_findings[0].0, "catalog-validation");
    let span = check_findings[0].1.as_ref().expect("error span");
    assert_eq!((span.start.line, span.start.col), (10, 85));

    let (dir2, entry2) = temp_entry();
    let compile_provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::new(Mutex::new(Vec::new())),
        check_compilation: None,
        compilation: Some(SourceCompilation {
            workshop_text: Some(workshop_fixture("synthetic/basic-rule")),
            locale: Some("en-US".to_string()),
            provenance: wright_driver::SourceProvenance::Unmapped,
            diagnostics: vec![catalog_error.clone()],
            source_identity: None,
        }),
        check_failure: None,
        failure: None,
    };
    let config = SessionConfig {
        input: InputSpec::Path(entry2),
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    };
    let mut compile_session =
        CompilerSession::with_source_provider(config, Box::new(compile_provider))
            .expect("provider session");
    let compile = compile_session.compile();
    assert!(!compile.ok, "compile fails on the same input: {compile:?}");
    assert_eq!(
        check_findings,
        compile
            .diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.span.clone()))
            .collect::<Vec<_>>(),
        "check and compile must report the same code and span"
    );
    cleanup(dir);
    cleanup(dir2);
}

#[test]
fn check_and_compile_agree_on_error_presence_across_the_artifact_corpus() {
    let corpus = workspace_root().join("tests/fixtures/workshop");
    let mut fixtures: Vec<PathBuf> = Vec::new();
    let mut stack = vec![corpus];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("fixture directory") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "ws") {
                fixtures.push(path);
            }
        }
    }
    fixtures.sort();
    assert!(!fixtures.is_empty(), "the fixture corpus must not be empty");

    for fixture in fixtures {
        let (dir, entry) = temp_entry();
        let artifact = std::fs::read_to_string(&fixture).expect("fixture text");
        let provider = ReloadableProvider {
            operations: Arc::new(Mutex::new(Vec::new())),
            workshop_text: artifact,
        };
        let config = SessionConfig {
            input: InputSpec::Path(entry),
            kind: SourceKind::Opy,
            ..SessionConfig::default()
        };
        let mut session = CompilerSession::with_source_provider(config, Box::new(provider))
            .expect("provider session");
        let check = session.check();
        let compile = session.compile();
        assert_eq!(
            check.ok,
            compile.ok,
            "{}: check and compile disagree (check ok={}, compile ok={})",
            fixture.display(),
            check.ok,
            compile.ok
        );
        let check_errors: Vec<_> = check
            .diagnostics
            .iter()
            .filter(|d| d.severity == wright_driver::Severity::Error)
            .map(|d| (d.code.as_str(), d.span.clone()))
            .collect();
        let compile_errors: Vec<_> = compile
            .diagnostics
            .iter()
            .filter(|d| d.severity == wright_driver::Severity::Error)
            .map(|d| (d.code.as_str(), d.span.clone()))
            .collect();
        assert_eq!(
            check_errors,
            compile_errors,
            "{}: check and compile report different errors",
            fixture.display()
        );
        cleanup(dir);
    }
}

#[test]
fn service_check_and_compile_run_without_a_prewarmed_snapshot() {
    let (dir, entry) = temp_entry();
    let operations = Arc::new(Mutex::new(Vec::new()));
    let provider = CheckOnlyProvider {
        operations: Arc::clone(&operations),
    };
    let config = SessionConfig {
        input: InputSpec::Path(entry.clone()),
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::with_source_provider(config, Box::new(provider))
        .expect("provider session");
    let mut service = ToolService::new(&mut session).expect("service constructs");

    assert!(service.loaded().is_none());
    assert_eq!(service.reload_count(), 0);

    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Check { all: true }) else {
        panic!("check must return the workflow envelope")
    };
    assert_eq!(result["command"], "check");
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(
        *operations.lock().expect("ops"),
        vec!["compile", "check"],
        "check compiles first and falls back to the check operation on a capability decline (#1005)"
    );

    assert!(service.check().ok);
    assert_eq!(
        *operations.lock().expect("ops"),
        vec!["compile", "check", "compile", "check"]
    );

    for request in [
        ToolRequest::Project,
        ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::LintRules,
    ] {
        let ToolResponse::Error { error } = service.handle(&request) else {
            panic!("{request:?} must refuse on the check-only path")
        };
        assert_eq!(error.code, "source-provider-unsupported", "{request:?}");
    }
    assert_eq!(
        *operations.lock().expect("ops"),
        vec!["compile", "check", "compile", "check"],
        "snapshot queries refuse without asking the provider to compile"
    );

    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Compile {
        output: None,
        force: false,
        to: None,
    }) else {
        panic!("compile must return the workflow envelope")
    };
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(
        result["diagnostics"][0]["code"], "source-provider-unsupported",
        "the provider's compile refusal rides the envelope: {result}"
    );
    assert_eq!(
        *operations.lock().expect("ops"),
        vec!["compile", "check", "compile", "check", "compile"]
    );
    cleanup(dir);
}

#[test]
fn service_compile_establishes_the_snapshot_and_recovers_after_reload() {
    let (dir, entry) = temp_entry();
    let operations = Arc::new(Mutex::new(Vec::new()));
    let provider = ReloadableProvider {
        operations: Arc::clone(&operations),
        workshop_text: workshop_fixture("synthetic/control-flow"),
    };
    let config = SessionConfig {
        input: InputSpec::Path(entry.clone()),
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::with_source_provider(config, Box::new(provider))
        .expect("provider session");
    let mut service = ToolService::new(&mut session).expect("service constructs");

    assert!(service.loaded().is_none());

    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Compile {
        output: Some(dir.join("program.ow")),
        force: true,
        to: None,
    }) else {
        panic!("compile must return the workflow envelope")
    };
    assert_eq!(result["ok"], true, "{result}");
    assert!(service.loaded().is_some(), "compile supplied the snapshot");

    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Project) else {
        panic!("project reads the established snapshot")
    };
    assert!(result["rules"].as_u64().unwrap() > 0, "{result}");
    assert_eq!(*operations.lock().expect("ops"), vec!["compile"]);

    std::fs::write(&entry, "// edited project source").expect("edit entry");
    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Check { all: true }) else {
        panic!("check must return the workflow envelope")
    };
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(
        *operations.lock().expect("ops"),
        vec!["compile", "compile"],
        "an observed change re-compiles instead of serving the stale program"
    );
    assert!(
        service.loaded().is_some(),
        "a compile-capable provider check supplies the canonical snapshot (#617)"
    );
    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Compile {
        output: Some(dir.join("program.ow")),
        force: true,
        to: None,
    }) else {
        panic!("compile must return the workflow envelope")
    };
    assert_eq!(result["ok"], true, "{result}");
    assert!(
        service.loaded().is_some(),
        "compile re-supplied the snapshot"
    );
    assert_eq!(*operations.lock().expect("ops"), vec!["compile", "compile"]);
    cleanup(dir);
}

#[test]
fn provider_backend_rejects_unpathed_input_without_fabricating_an_entry() {
    for input in [
        InputSpec::Stdin,
        InputSpec::Text {
            text: "globalvar score = 0\n".to_string(),
            path: Some(PathBuf::from("entry.opy")),
        },
    ] {
        let config = SessionConfig {
            input,
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
        check_failure: None,
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
fn provider_failures_keep_their_code_stage_and_exit_class() {
    use wright_driver::ExitClass;
    for (code, stage, class, exit) in [
        (
            "provider-refusal-compile.tooLarge",
            Stage::Frontend,
            ExitClass::SourceError,
            1,
        ),
        (
            "project-load-failed",
            Stage::Frontend,
            ExitClass::SourceError,
            1,
        ),
        (
            "session-config-changed",
            Stage::Discovery,
            ExitClass::SourceError,
            1,
        ),
        (
            "capability-unavailable",
            Stage::Frontend,
            ExitClass::Unsupported,
            3,
        ),
        ("provider-timeout", Stage::Internal, ExitClass::Internal, 4),
        ("provider-exited", Stage::Internal, ExitClass::Internal, 4),
        (
            "provider-malformed",
            Stage::Internal,
            ExitClass::Internal,
            4,
        ),
    ] {
        let (dir, entry) = temp_entry();
        let provider = RecordingProvider {
            target: Arc::new(Mutex::new(None)),
            operations: Arc::new(Mutex::new(Vec::new())),
            check_compilation: Some(SourceCompilation {
                workshop_text: None,
                locale: None,
                provenance: wright_driver::SourceProvenance::Unmapped,
                diagnostics: Vec::new(),
                source_identity: None,
            }),
            compilation: None,
            check_failure: Some(failed(code, stage, class, format!("failed: {code}"))),
            failure: Some(failed(code, stage, class, format!("failed: {code}"))),
        };
        let config = SessionConfig {
            input: InputSpec::Path(entry),
            kind: SourceKind::Opy,
            ..SessionConfig::default()
        };
        let mut session = CompilerSession::with_source_provider(config, Box::new(provider))
            .expect("provider session");
        let result = session.check();
        assert!(!result.ok, "{code}");
        assert_eq!(result.exit, exit, "{code}");
        assert_eq!(result.diagnostics[0].code, code);
        assert_eq!(result.diagnostics[0].stage, stage, "{code}");
        cleanup(dir);
    }
}

#[test]
fn provider_backend_inspect_and_queries_run_the_compile_load() {
    let (dir, entry) = temp_entry();
    let operations = Arc::new(Mutex::new(Vec::new()));
    let provider = ReloadableProvider {
        operations: Arc::clone(&operations),
        workshop_text: workshop_fixture("synthetic/basic-rule"),
    };
    let config = SessionConfig {
        input: InputSpec::Path(entry),
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::with_source_provider(config, Box::new(provider))
        .expect("provider session");

    let inspect = session.inspect();
    assert!(inspect.ok, "provider inspect: {:?}", inspect.diagnostics);
    let rules = inspect.result.rules.as_array().expect("rule list");
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["name"], "setup");

    let symbols = session.symbols(None, None, None);
    assert!(symbols.ok, "provider symbols: {:?}", symbols.diagnostics);
    let rules = session.rules(None, None, None, true);
    assert!(
        rules.ok,
        "provider rules --content: {:?}",
        rules.diagnostics
    );
    assert_eq!(*operations.lock().expect("ops"), vec!["compile"]);
    cleanup(dir);
}

#[test]
fn check_only_provider_inspect_reports_the_provider_compile_decline() {
    let (dir, entry) = temp_entry();
    let operations = Arc::new(Mutex::new(Vec::new()));
    let provider = CheckOnlyProvider {
        operations: Arc::clone(&operations),
    };
    let config = SessionConfig {
        input: InputSpec::Path(entry),
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::with_source_provider(config, Box::new(provider))
        .expect("provider session");

    let result = session.inspect();
    assert!(!result.ok);
    assert_eq!(result.exit, 3);
    let diagnostic = &result.diagnostics[0];
    assert_eq!(diagnostic.code, "source-provider-unsupported");
    assert!(
        diagnostic
            .message
            .contains("no canonical compile capability"),
        "the provider's own decline message surfaces: {diagnostic:?}"
    );
    assert!(result.result.program.is_null());
    assert_eq!(*operations.lock().expect("ops"), vec!["compile"]);
    cleanup(dir);
}

#[test]
fn opy_rules_content_reports_names_unavailable() {
    let (dir, entry) = temp_entry();
    let provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::new(Mutex::new(Vec::new())),
        check_compilation: None,
        compilation: Some(SourceCompilation::success(
            std::fs::read_to_string(
                workspace_root()
                    .join("tests/fixtures/workshop/synthetic/program-calls-repel.en-US.ow"),
            )
            .expect("repel fixture"),
        )),
        check_failure: None,
        failure: None,
    };
    let mut session = CompilerSession::with_source_provider(
        SessionConfig {
            input: InputSpec::Path(entry.clone()),
            kind: SourceKind::Opy,
            ..SessionConfig::default()
        },
        Box::new(provider),
    )
    .expect("provider session");
    let compile = session.compile();
    assert!(compile.ok, "provider compile: {:?}", compile.diagnostics);
    let mut service = ToolService::new(&mut session).expect("service loads");
    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Rules {
        name: None,
        rule: None,
        snapshot: None,
        file: None,
        max: None,
        content: true,

        all: true,
    }) else {
        panic!("rules with content failed");
    };
    assert_eq!(
        result["unavailable"],
        serde_json::json!({"names": "opy-rs"})
    );
    assert!(result.get("names").is_none());

    let rules = result["rules"].as_array().expect("rules");
    assert_eq!(rules.len(), 1);
    let actions = rules[0]["content"]["actions"].as_array().unwrap();
    let impulses = actions
        .iter()
        .filter(|action| {
            action["call"] == "applyImpulse" && action["args"][2].as_f64() == Some(150.0)
        })
        .count();
    assert_eq!(impulses, 2, "two strength-150 impulses: {actions:?}");
    let stunned = serde_json::json!({"enum": "Status", "member": "STUNNED"});
    let stuns = actions
        .iter()
        .filter(|action| {
            action["call"] == "setStatusEffect"
                && action["args"][2] == stunned
                && action["args"][3].as_f64() == Some(0.75)
        })
        .count();
    assert_eq!(stuns, 2, "two 0.75s stuns: {actions:?}");
    cleanup(dir);
}

#[test]
fn a_repeated_check_reports_the_same_provider_diagnostics() {
    let (dir, entry) = temp_entry();
    let operations = Arc::new(Mutex::new(Vec::new()));
    let provider_diagnostic =
        |code: &str| Diagnostic::warning(code, Stage::Frontend, format!("{code} message"));
    let provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::clone(&operations),
        check_compilation: None,
        compilation: Some(SourceCompilation {
            workshop_text: Some(workshop_fixture("synthetic/basic-rule")),
            locale: None,
            provenance: wright_driver::SourceProvenance::Unmapped,
            diagnostics: vec![provider_diagnostic("compile-warning")],
            source_identity: None,
        }),
        check_failure: None,
        failure: None,
    };
    let mut session = CompilerSession::with_source_provider(
        SessionConfig {
            input: InputSpec::Path(entry),
            kind: SourceKind::Opy,
            ..SessionConfig::default()
        },
        Box::new(provider),
    )
    .expect("provider session");

    let first = session.check();
    assert!(first.ok, "first check: {:?}", first.diagnostics);
    let second = session.check();
    assert!(second.ok, "repeated check: {:?}", second.diagnostics);
    let codes = |diagnostics: &[Diagnostic]| {
        diagnostics
            .iter()
            .map(|d| d.code.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(codes(&first.diagnostics), codes(&second.diagnostics));
    assert!(
        second
            .diagnostics
            .iter()
            .any(|d| d.code == "compile-warning"),
        "the repeated check keeps the provider diagnostics: {second:?}"
    );
    assert_eq!(
        *operations.lock().expect("operation lock"),
        vec!["compile"],
        "the repeated check is served from the cached load"
    );
    cleanup(dir);
}
