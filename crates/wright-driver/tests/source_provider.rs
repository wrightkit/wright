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

/// #617: on a provider that refuses `compile`, `check` still executes the
/// provider's check operation alone — the documented check-only path.
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
        vec!["check", "compile"]
    );
    cleanup(dir);
}

/// #617: a compile-capable provider's `check` calls `check` then `compile`
/// once; the canonical snapshot the check load established serves the later
/// `compile` from cache — no second provider round-trip.
#[test]
fn provider_backend_serves_compile_from_the_check_established_snapshot() {
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
    assert_eq!(
        *operations.lock().expect("operation lock"),
        vec!["check", "compile"],
        "check compiles once for its canonical snapshot; compile reuses it"
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

/// A source map for `text` whose spans point into `authored`, edited by
/// `edit` before decoding, as a provider would return it.
fn mapped_provenance(
    text: &str,
    authored: &Path,
    edit: impl FnOnce(&mut serde_json::Value),
) -> wright_driver::SourceProvenance {
    let catalog = wright_analyzer::catalog::builtin().expect("catalog");
    let locale = workshop_rs::catalog::Locale::new("en-US");
    let program = workshop_rs::parser::parse_with_context(text, &catalog, &locale, &*catalog)
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

/// Every span object (`file`, `start`, `end`) nested anywhere in `value`.
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
    // The fake map's extracted spans carry this text's coordinates; write it
    // as the member so retained-source validation sees in-range positions.
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

/// A mapped provider program's file table entries carry no retained source
/// text, so the analyzer's document count reports 0 for a real source
/// project. `program.files` reports the loaded source-file table instead,
/// matching `project` (#536).
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

    // `analyze` and `lint` compose the same `program` summary `inspect`
    // reports.
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

/// A provider map may carry a span that does not resolve inside the
/// retained authored source (e.g. a position recorded against an expanded
/// spelling rather than the authored line). `workshop-rs` prunes spans
/// that do not resolve in the attached source (1.13), so the run still
/// serves and mapped positions survive instead of failing the load
/// (#583, #626).
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

/// A member the provider map names but no readable disk file serves no
/// retained source; the load warns instead of silently dropping it (#583).
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

/// `lint --fix` on a provider-mapped program materializes the dead-branch
/// removal against the authored OPY member — marker action spans give the
/// branch extent — and validates the transaction through the provider edit
/// pipeline (`lpp/validateEdits` + `lpp/check`) instead of the raw Workshop
/// reparse (#583). `--write` applies it and re-lints clean.
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

    // The authored OPY member the fake provider's source map points into.
    const OPY: &str = "globalvar total\n\nrule \"dup\":\n    @Event global\n    if total == 0:\n        total = 1\n    elif total == 0:\n        total = 3\n    else:\n        total = 4\n";
    const FIXED_OPY: &str = "globalvar total\n\nrule \"dup\":\n    @Event global\n    if total == 0:\n        total = 1\n    else:\n        total = 4\n";

    // The canonical artifact for each member state: the duplicate `elif`
    // chain before the fix, the `else`-only chain after it — what a real
    // provider emits when it recompiles the edited source.
    const ARTIFACT: &str = "variables {\n    global:\n        0: total\n}\nrule (\"dup\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        If(Compare(Global.total, ==, 0));\n            Set Global Variable(total, 1);\n        Else If(Compare(Global.total, ==, 0));\n            Set Global Variable(total, 3);\n        Else;\n            Set Global Variable(total, 4);\n        End;\n    }\n}\n";
    const ARTIFACT_FIXED: &str = "variables {\n    global:\n        0: total\n}\nrule (\"dup\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        If(Compare(Global.total, ==, 0));\n            Set Global Variable(total, 1);\n        Else;\n            Set Global Variable(total, 4);\n        End;\n    }\n}\n";

    /// Marker action spans the way the owning frontend records them (#583):
    /// the `if` keyword anchors the chain head, `elif`/`else` keywords mark
    /// their branches, and the chain `End` sits at the dedent/EOF boundary.
    fn marker_map(text: &str, authored: &Path) -> wright_driver::SourceProvenance {
        mapped_provenance(text, authored, |artifact| {
            let spans = artifact["spans"].as_array_mut().expect("span list");
            // Every extracted entry carries Workshop-artifact coordinates;
            // a fake provider's map authors only the marker/condition spans
            // this test declares (`apply` rejects position-less entries).
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
            // The dead `elif` condition's authored extent — the finding's
            // span anchor and the fix's file resolution.
            spans.push(serde_json::json!({
                "node": "action_argument", "rule": 0, "action": 2, "argument": 0,
                "span": span(7, 10, 7, 20),
            }));
        })
    }

    // Re-derives its artifact from the member's current text: the edited
    // member compiles to the `elif`-free chain on the post-write reload.
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

/// A provider that can check but refuses compile — the "check-only provider
/// path" #591 requires `check` to execute on without a compile prewarm.
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

/// A provider that re-compiles on every call — the repeatable counterpart
/// of `RecordingProvider`, which takes its scripted result once. Its
/// operations log lets a test distinguish which provider operation ran.
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

/// #617: `check` gates an agent on the same canonical validation `compile`
/// runs — the issue's reproducer class, where the provider's check op
/// reports clean but its compile reports a `catalog-validation` error, must
/// fail `check` with `compile`'s code and span.
#[test]
fn check_reports_the_catalog_validation_errors_compile_reports() {
    let (dir, entry) = temp_entry();
    let catalog_error = Diagnostic {
        code: "catalog-validation".to_string(),
        stage: Stage::Frontend,
        severity: wright_driver::Severity::Error,
        message:
            "action 'playEffect' argument 4 must have semantic type 'Vector|Player', got Array"
                .to_string(),
        status: None,
        span: Some(wright_driver::SourceSpan {
            file: 0,
            path: entry.display().to_string(),
            start: wright_driver::Position { line: 10, col: 85 },
            end: wright_driver::Position { line: 10, col: 128 },
        }),
        source: None,
    };
    let provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::new(Mutex::new(Vec::new())),
        // The provider's own check op reports clean — the divergence the
        // issue reproduces.
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
            diagnostics: vec![catalog_error],
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

    // A second session on the same provider shape: `compile` reports the
    // identical code and span.
    let (dir2, entry2) = temp_entry();
    let compile_provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::new(Mutex::new(Vec::new())),
        check_compilation: None,
        compilation: Some(SourceCompilation {
            workshop_text: Some(workshop_fixture("synthetic/basic-rule")),
            locale: Some("en-US".to_string()),
            provenance: wright_driver::SourceProvenance::Unmapped,
            diagnostics: vec![Diagnostic {
                code: "catalog-validation".to_string(),
                stage: Stage::Frontend,
                severity: wright_driver::Severity::Error,
                message: "action 'playEffect' argument 4 must have semantic type 'Vector|Player', got Array"
                    .to_string(),
                status: None,
                span: Some(wright_driver::SourceSpan {
                    file: 0,
                    path: entry.display().to_string(),
                    start: wright_driver::Position { line: 10, col: 85 },
                    end: wright_driver::Position { line: 10, col: 128 },
                }),
                source: None,
            }],
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

/// #617: over every committed canonical-artifact fixture, `check` and
/// `compile` agree on whether the run reports an error.
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

/// #591: `check` and `compile` are workflows, not snapshot queries — the
/// service must run them through the operation-aware provider calls without
/// first requiring a canonical compile result.
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

    // A service over a check-only provider starts with no snapshot — the
    // construction never compiles just to warm one.
    assert!(service.loaded().is_none());
    assert_eq!(service.reload_count(), 0);

    // `check` executes the provider's check operation, then asks for the
    // canonical artifact (#617); the provider's compile refusal keeps it on
    // the check-only path.
    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Check { all: true }) else {
        panic!("check must return the workflow envelope")
    };
    assert_eq!(result["command"], "check");
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(*operations.lock().expect("ops"), vec!["check", "compile"]);

    // The direct method is the same workflow: check again, still no
    // compile-only provider call just to establish a query snapshot.
    assert!(service.check().ok);
    assert_eq!(
        *operations.lock().expect("ops"),
        vec!["check", "compile", "check", "compile"]
    );

    // Canonical queries still refuse explicitly — no compile capability
    // means no snapshot, not a placeholder program.
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
        vec!["check", "compile", "check", "compile"],
        "snapshot queries refuse without asking the provider to compile"
    );

    // `compile` is the provider's own refusal, in the envelope — not the
    // service's snapshot precondition.
    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Compile) else {
        panic!("compile must return the workflow envelope")
    };
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(
        result["diagnostics"][0]["code"], "source-provider-unsupported",
        "the provider's compile refusal rides the envelope: {result}"
    );
    assert_eq!(
        *operations.lock().expect("ops"),
        vec!["check", "compile", "check", "compile", "compile"]
    );
    cleanup(dir);
}

/// #591: a successful `compile` establishes the snapshot later queries read,
/// and an observed disk change re-runs the operation-aware load — recovery
/// is the same workflow, never a re-prewarm requirement.
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

    // A fresh service holds no snapshot until a workflow establishes one.
    assert!(service.loaded().is_none());

    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Compile) else {
        panic!("compile must return the workflow envelope")
    };
    assert_eq!(result["ok"], true, "{result}");
    assert!(service.loaded().is_some(), "compile supplied the snapshot");

    // The adopted snapshot serves queries — no second provider compile.
    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Project) else {
        panic!("project reads the established snapshot")
    };
    assert!(result["rules"].as_u64().unwrap() > 0, "{result}");
    assert_eq!(*operations.lock().expect("ops"), vec!["compile"]);

    // A disk change under the session invalidates the snapshot; `compile`
    // re-resolves through the provider's own operation and re-establishes it.
    std::fs::write(&entry, "// edited project source").expect("edit entry");
    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Check { all: true }) else {
        panic!("check must return the workflow envelope")
    };
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(
        *operations.lock().expect("ops"),
        vec!["compile", "check", "compile"],
        "an observed change re-checks instead of serving the stale program"
    );
    assert!(
        service.loaded().is_some(),
        "a compile-capable provider check supplies the canonical snapshot (#617)"
    );
    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Compile) else {
        panic!("compile must return the workflow envelope")
    };
    assert_eq!(result["ok"], true, "{result}");
    assert!(
        service.loaded().is_some(),
        "compile re-supplied the snapshot"
    );
    assert_eq!(
        *operations.lock().expect("ops"),
        vec!["compile", "check", "compile"]
    );
    cleanup(dir);
}

/// #591: `handle` check/compile and the direct service methods deliver the
/// same workflow outcome over the same provider session.
#[test]
fn service_check_and_compile_match_the_direct_workflow_methods() {
    for via_request in [true, false] {
        let (dir, entry) = temp_entry();
        let operations = Arc::new(Mutex::new(Vec::new()));
        let provider = ReloadableProvider {
            operations: Arc::clone(&operations),
            workshop_text: workshop_fixture("synthetic/basic-rule"),
        };
        let config = SessionConfig {
            input: InputSpec::Path(entry.clone()),
            kind: SourceKind::Opy,
            ..SessionConfig::default()
        };
        let mut session = CompilerSession::with_source_provider(config, Box::new(provider))
            .expect("provider session");
        let mut service = ToolService::new(&mut session).expect("service constructs");

        let check = if via_request {
            let ToolResponse::Ok { result } = service.handle(&ToolRequest::Check { all: true })
            else {
                panic!("check must return the workflow envelope")
            };
            result
        } else {
            serde_json::to_value(service.check()).expect("envelope serializes")
        };
        assert_eq!(check["command"], "check");
        assert_eq!(check["ok"], true, "{check}");
        // `check` consults the provider's check operation and its compile for
        // canonical validation (#617).
        assert_eq!(*operations.lock().expect("ops"), vec!["check", "compile"]);

        let compile = if via_request {
            let ToolResponse::Ok { result } = service.handle(&ToolRequest::Compile) else {
                panic!("compile must return the workflow envelope")
            };
            result
        } else {
            serde_json::to_value(service.compile()).expect("envelope serializes")
        };
        assert_eq!(compile["command"], "compile");
        assert_eq!(compile["ok"], true, "{compile}");
        // The `check`-established canonical snapshot serves `compile` — no
        // second provider round-trip.
        assert_eq!(*operations.lock().expect("ops"), vec!["check", "compile"]);
        cleanup(dir);
    }
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

/// #555: caller-held text is not a provider input either — the provider
/// resolves the project from disk itself, so it cannot answer for a buffer
/// only the caller holds.
#[test]
fn provider_backend_rejects_text_input_without_fabricating_an_entry() {
    let config = SessionConfig {
        input: InputSpec::Text {
            text: "globalvar score = 0\n".to_string(),
            path: Some(PathBuf::from("entry.opy")),
        },
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
fn provider_backend_does_not_fall_back_when_the_provider_fails() {
    let (dir, entry) = temp_entry();
    let provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::new(Mutex::new(Vec::new())),
        check_compilation: None,
        compilation: None,
        check_failure: None,
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

/// #569: a provider refusal surfaces at the frontend stage (exit 1) with
/// the refusal code machine-readable in the diagnostic code — it is a
/// deliberate decline of the request, not an internal failure.
#[test]
fn provider_refusal_is_a_frontend_failure_with_its_refusal_code() {
    let (dir, entry) = temp_entry();
    let provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::new(Mutex::new(Vec::new())),
        check_compilation: None,
        compilation: None,
        check_failure: None,
        failure: Some(SourceProviderError::Failed {
            code: "provider-refusal-compile.tooLarge".to_string(),
            message: "the compile request failed: the provider refused (compile.tooLarge)"
                .to_string(),
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
    assert_eq!(result.exit, 1);
    assert_eq!(
        result.diagnostics[0].code,
        "provider-refusal-compile.tooLarge"
    );
    assert_eq!(result.diagnostics[0].stage, Stage::Frontend);
    cleanup(dir);
}

/// #569: a negotiated-capability gap is a recognized but unsupported
/// operation (exit 3), not a source error and not an internal failure.
#[test]
fn provider_capability_unavailable_exits_as_unsupported() {
    let (dir, entry) = temp_entry();
    let provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::new(Mutex::new(Vec::new())),
        check_compilation: None,
        compilation: None,
        check_failure: None,
        failure: Some(SourceProviderError::Failed {
            code: "capability-unavailable".to_string(),
            message: "the check request failed: capability 'projectLoading' was not negotiated"
                .to_string(),
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
    assert_eq!(result.exit, 3);
    assert_eq!(result.diagnostics[0].code, "capability-unavailable");
    assert_eq!(result.diagnostics[0].stage, Stage::Frontend);
    cleanup(dir);
}

/// #569: owner-side project loading failures are source-side request
/// failures (exit 1), not internal errors.
#[test]
fn provider_project_load_failure_is_a_source_side_failure() {
    let (dir, entry) = temp_entry();
    let provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::new(Mutex::new(Vec::new())),
        check_compilation: None,
        compilation: None,
        check_failure: None,
        failure: Some(SourceProviderError::Failed {
            code: "project-load-failed".to_string(),
            message: "the check request failed: a required source file could not be loaded"
                .to_string(),
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
    assert_eq!(result.exit, 1);
    assert_eq!(result.diagnostics[0].code, "project-load-failed");
    cleanup(dir);
}

/// #569: transport-level failures (timeout, exit, malformed traffic) stay
/// internal (exit 4) and keep their structured codes.
#[test]
fn provider_transport_failures_stay_internal() {
    for code in ["provider-timeout", "provider-exited", "provider-malformed"] {
        let (dir, entry) = temp_entry();
        let provider = RecordingProvider {
            target: Arc::new(Mutex::new(None)),
            operations: Arc::new(Mutex::new(Vec::new())),
            check_compilation: None,
            compilation: None,
            check_failure: None,
            failure: Some(SourceProviderError::Failed {
                code: code.to_string(),
                message: format!("the compile request failed: {code}"),
            }),
        };
        let config = SessionConfig {
            input: InputSpec::Path(entry),
            kind: SourceKind::Opy,
            ..SessionConfig::default()
        };
        let mut session = CompilerSession::with_source_provider(config, Box::new(provider))
            .expect("provider session");
        let result = session.compile();
        assert!(!result.ok, "{code}");
        assert_eq!(result.exit, 4, "{code}");
        assert_eq!(result.diagnostics[0].code, code);
        assert_eq!(result.diagnostics[0].stage, Stage::Internal);
        cleanup(dir);
    }
}

/// #569: a Wright-originated refusal (`session-config-changed`) keeps its
/// verbatim code and discovery stage on this path too, matching the direct
/// workflow refusal.
#[test]
fn wright_originated_refusal_keeps_its_verbatim_code() {
    let (dir, entry) = temp_entry();
    let provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::new(Mutex::new(Vec::new())),
        check_compilation: None,
        compilation: None,
        check_failure: None,
        failure: Some(SourceProviderError::Failed {
            code: "session-config-changed".to_string(),
            message: "the check request failed: session configuration is fixed".to_string(),
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
    assert_eq!(result.exit, 1);
    assert_eq!(result.diagnostics[0].code, "session-config-changed");
    assert_eq!(result.diagnostics[0].stage, Stage::Discovery);
    cleanup(dir);
}

// ---------------------------------------------------------------------
// #569: the real `LppSourceProvider` maps a typed `ProviderError` to a
// classified `SourceProviderError` carrying the operation/entry context.
// ---------------------------------------------------------------------

/// A `LanguageProvider` that fails the entry-scoped requests with a scripted
/// typed error; every other method is unreachable on these paths. When
/// `capabilities_failure` is `Some`, `capabilities()` fails with that error
/// instead of being unreachable.
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

/// #569: an LPP refusal reaches the `SourceProvider` consumer as a
/// classified frontend failure that still names its `refusalCode`, with the
/// failing operation and entry in the message.
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

/// #569: a timeout stays internal and keeps the method/duration detail in
/// the message of the classified error.
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

/// #569: a capability gap stays `capability-unavailable` (unsupported), a
/// process exit and malformed traffic stay internal — each class keeps a
/// distinct code a consumer can branch on without parsing `message`.
#[test]
fn lpp_source_provider_distinguishes_failure_classes() {
    for (failure, code) in [
        (
            wright_lpp::ProviderError::lpp(
                wright_lpp::LppErrorKind::CapabilityUnavailable,
                serde_json::json!({"capability": "projectLoading", "method": "lpp/check"}),
                "capability was not negotiated",
            ),
            "capability-unavailable",
        ),
        (
            wright_lpp::ProviderError::Exited {
                status: Some(9),
                message: "the provider exited".to_string(),
            },
            "provider-exited",
        ),
        (
            wright_lpp::ProviderError::Malformed {
                detail: "missing result".to_string(),
            },
            "provider-malformed",
        ),
    ] {
        let (dir, entry) = temp_entry();
        let mut provider = wright_driver::source_provider::LppSourceProvider::new(
            Box::new(FailingLppProvider {
                failure: failure.clone(),
                capabilities_failure: None,
            }),
            None,
        );
        let error = provider
            .check(&SourceTarget::new(SourceLanguage::Opy, &entry, &dir))
            .err()
            .unwrap_or_else(|| panic!("{failure:?} produced no error"));
        assert_eq!(error.code(), code, "{failure:?} lost its classification");
        cleanup(dir);
    }
}

/// #571: a failed capability query is a provider failure, not a legacy
/// negotiation signal — `compile` propagates the typed error with its
/// classification instead of silently dispatching the unnegotiated
/// `lpp/compile` request. `failure` answers `compile_entry`, so a distinct
/// capabilities failure reaching the caller proves no compile request was
/// attempted.
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

/// #655: `inspect` and its query subcommands load provider-backed input
/// through the provider's compile operation — the same operation-aware load
/// `lint`/`analyze` run — so a compile-capable provider serves them, and one
/// provider compile feeds every later query.
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
    let rules = session.rules(None, None, None, None, true);
    assert!(
        rules.ok,
        "provider rules --content: {:?}",
        rules.diagnostics
    );
    // The first compile established the canonical snapshot; every query
    // after it reads the cache instead of re-compiling.
    assert_eq!(*operations.lock().expect("ops"), vec!["compile"]);
    cleanup(dir);
}

/// #655: a provider that declines its compile operation reports its own
/// limitation — `inspect` relays that decline rather than refusing at the
/// command level.
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
    // The compile operation was asked once; the refusal is the provider's.
    assert_eq!(*operations.lock().expect("ops"), vec!["compile"]);
    cleanup(dir);
}

/// ADR-0024 decision 4: until the provider advertises the names capability,
/// an OverPy program serves its canonical content with `unavailable`
/// naming `opy-rs` instead of a `names` map. The repel fixture stands in
/// for the compiled OverPy form; assertions read the content, never text.
#[test]
fn opy_rules_content_reports_names_unavailable() {
    let (dir, entry) = temp_entry();
    let provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::new(Mutex::new(Vec::new())),
        check_compilation: None,
        compilation: Some(SourceCompilation::success(
            std::fs::read_to_string(
                workspace_root().join("benchmarks/agent/fixtures/program-calls/repel.en-US.ow"),
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
    // A compile establishes the canonical snapshot semantic queries serve.
    let compile = session.compile();
    assert!(compile.ok, "provider compile: {:?}", compile.diagnostics);
    let mut service = ToolService::new(&mut session).expect("service loads");
    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Rules {
        name: None,
        rule: None,
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

/// #638: the first `check`/`compile` drains the session's diagnostic buffer
/// into the load-time envelope; the cached `Loaded` keeps its load
/// diagnostics so an identical repeated request reports the same set instead
/// of `diagnostics: []` without calling the provider.
#[test]
fn a_repeated_check_reports_the_same_provider_diagnostics() {
    let (dir, entry) = temp_entry();
    let operations = Arc::new(Mutex::new(Vec::new()));
    let provider_diagnostic = |code: &str| Diagnostic {
        code: code.to_string(),
        stage: Stage::Frontend,
        severity: wright_driver::Severity::Warning,
        message: format!("{code} message"),
        status: None,
        span: None,
        source: None,
    };
    let provider = RecordingProvider {
        target: Arc::new(Mutex::new(None)),
        operations: Arc::clone(&operations),
        check_compilation: Some(SourceCompilation {
            workshop_text: None,
            locale: None,
            provenance: wright_driver::SourceProvenance::Unmapped,
            diagnostics: vec![provider_diagnostic("check-warning")],
            source_identity: None,
        }),
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
            .any(|d| d.code == "check-warning" || d.code == "compile-warning"),
        "the repeated check keeps the provider diagnostics: {second:?}"
    );
    assert_eq!(
        *operations.lock().expect("operation lock"),
        vec!["check", "compile"],
        "the repeated check is served from the cached load"
    );
    cleanup(dir);
}

/// #638: an LPP provider without the compile capability declines
/// `lpp/compile` through `capability-unavailable`, not `Unsupported` —
/// `check` must still keep the check-only path and report the provider's
/// own diagnostics.
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

/// #638: the version-mismatch retry asks for the highest mutually supported
/// version from `supportedProtocolVersions` (LPP §19.2) rather than a fixed
/// next rung — and every initialize path sends `clientInfo.name: "wright"`.
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
            serde_json::json!({"version": "1.4", "client": "wright"}),
            serde_json::json!({"version": "1.3", "client": "wright"}),
        ],
        "the retry asks for the highest mutual version, not the next fixed rung"
    );
    cleanup(dir);
}

/// #638: a directory target loads through the LPP 1.3 source-identity
/// contract — a negotiated session that does not advertise `sourceIdentity`
/// is refused with a typed capability diagnostic rather than silently
/// proceeding without a canonical identity.
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
