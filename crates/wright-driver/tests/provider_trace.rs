#![cfg(unix)]

use std::path::Path;

use serde_json::Value;
use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::{
    CompilerSession, Fact, InputSpec, SessionConfig, SourceBackend, SourceKind, Trace,
};
use wright_lpp::{LanguageProvider, ProviderConfig};

const PROVIDER: &str = r#"
import json, os, pathlib, sys
here = pathlib.Path(__file__).resolve().parent
mode = sys.argv[1]
trace = os.environ.get("WRIGHT_PROVIDER_TRACE_FILE")
(here / "spawn.json").write_text(json.dumps({"argv": sys.argv, "env": dict(os.environ)}))
if trace:
    path = pathlib.Path(trace)
    (here / "trace-path").write_text(trace)
    assert path.is_absolute()
    assert path.stat().st_mode & 0o077 == 0
    if mode == "missing":
        path.rename(here / "unused-trace")
    elif mode == "unreadable":
        path.chmod(0)
for line in sys.stdin:
    request = json.loads(line)
    method = request["method"]
    if method == "lpp/initialize":
        result = {"protocolVersion": request["params"]["protocolVersion"],
                  "serverInfo": {"name": "trace-test", "version": "1"},
                  "languages": [{"id": "opy", "extensions": ["opy"]}],
                  "capabilities": {"compile": True, "projectLoading": True}}
    elif method == "lpp/compile":
        result = {"diagnostics": [], "artifact": {"format": "workshop-rs/text-v1",
                  "content": (here / "artifact.ws").read_text()}}
    else:
        result = None
    if trace and mode in ["valid", "malformed", "malformed-only"]:
        spans = [
            {"id": 0, "parent": None, "name": "provider.request.decode", "startNs": 10, "durationNs": 20, "facts": {}},
            {"id": 1, "parent": None, "name": "provider.phase", "startNs": 40, "durationNs": 900, "facts": {"method": method}},
            {"id": 2, "parent": 1, "name": "provider.source.read", "startNs": 60, "durationNs": 30, "facts": {"files": 3, "bytes": 42}}]
        record = {"schema": "wright-provider-trace/v1", "method": method, "spans": spans}
        with open(trace, "ab") as f:
            if mode in ["malformed", "malformed-only"]:
                f.write(b"not-json\n\xff\n")
                f.write((json.dumps(dict(record, schema="unknown")) + "\n").encode())
                f.write((json.dumps(dict(record, method="lpp/unrelated")) + "\n").encode())
                invalid = dict(spans[0], parent=99)
                f.write((json.dumps(dict(record, spans=[invalid])) + "\n").encode())
            if mode != "malformed-only":
                f.write((json.dumps(record) + "\n").encode())
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
"#;

fn config(dir: &Path, mode: &str) -> SessionConfig {
    std::fs::write(dir.join("provider.py"), PROVIDER).unwrap();
    std::fs::write(dir.join("main.opy"), "# provider-owned input\n").unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/workshop/synthetic/basic-rule.ws");
    std::fs::copy(fixture, dir.join("artifact.ws")).unwrap();
    let mut config = SessionConfig {
        input: InputSpec::Path(dir.join("main.opy")),
        kind: SourceKind::Opy,
        source_backend: SourceBackend::Provider,
        ..SessionConfig::default()
    };
    config
        .providers
        .register(ProviderConfig::new(
            "opy",
            "python3",
            vec![dir.join("provider.py").display().to_string(), mode.into()],
        ))
        .unwrap();
    config
}

#[test]
fn provider_spans_keep_request_offsets_nesting_and_incremental_reads() {
    for mode in ["valid", "malformed"] {
        let dir = tempfile::tempdir().unwrap();
        let trace = Trace::enabled();
        let root = trace.span("compile");
        let mut session =
            CompilerSession::new_traced(config(dir.path(), mode), trace.clone()).unwrap();
        let outcome = session.compile();
        assert!(outcome.ok, "{mode}: {:?}", outcome.diagnostics);
        {
            let mut service = ToolService::new(&mut session).unwrap();
            std::fs::write(dir.path().join("main.opy"), "# changed provider input\n").unwrap();
            assert!(matches!(
                service.handle(&ToolRequest::Project),
                ToolResponse::Ok { .. }
            ));
            assert_eq!(service.reload_count(), 1);
        }
        drop(session);
        drop(root);
        let trace_path = std::fs::read_to_string(dir.path().join("trace-path")).unwrap();
        assert!(
            !Path::new(&trace_path).exists(),
            "session removes its trace file"
        );
        let report = trace.report().unwrap();
        let requests: Vec<_> = report
            .spans
            .iter()
            .filter(|s| s.name == "lpp.request")
            .collect();
        assert_eq!(requests.len(), 4, "initialize, two compiles, shutdown");
        for request in requests {
            let children: Vec<_> = report
                .spans
                .iter()
                .filter(|s| s.parent == Some(request.id))
                .collect();
            assert_eq!(
                children.len(),
                2,
                "{mode}: no old or malformed records replayed"
            );
            assert_eq!(children[0].name, "provider.request.decode");
            assert_eq!(children[0].start_ns, request.start_ns + 10);
            assert_eq!(children[0].duration_ns, 20);
            let phase = children[1];
            assert_eq!(phase.name, "provider.phase");
            assert_eq!(phase.start_ns, request.start_ns + 40);
            assert_eq!(phase.duration_ns, 900);
            assert_eq!(
                phase.facts,
                vec![(
                    "method".into(),
                    request
                        .facts
                        .iter()
                        .find(|(key, _)| key == "method")
                        .unwrap()
                        .1
                        .clone()
                )]
            );
            let nested = report
                .spans
                .iter()
                .find(|s| s.parent == Some(phase.id))
                .unwrap();
            assert_eq!(nested.name, "provider.source.read");
            assert_eq!(nested.start_ns, request.start_ns + 60);
            assert_eq!(nested.duration_ns, 30);
            assert!(nested.facts.contains(&("files".into(), Fact::Count(3))));
            assert!(nested.facts.contains(&("bytes".into(), Fact::Count(42))));
        }
    }
}

#[test]
fn absent_or_unreadable_provider_data_preserves_compile_and_other_spans() {
    for mode in ["no-write", "missing", "unreadable", "malformed-only"] {
        let dir = tempfile::tempdir().unwrap();
        let trace = Trace::enabled();
        let mut session =
            CompilerSession::new_traced(config(dir.path(), mode), trace.clone()).unwrap();
        let outcome = session.compile();
        assert!(outcome.ok, "{mode}: {:?}", outcome.diagnostics);
        drop(session);
        let report = trace.report().unwrap();
        assert!(report.spans.iter().any(|s| s.name == "lpp.request"));
        assert!(report.spans.iter().any(|s| s.name == "parse"));
        assert!(!report.spans.iter().any(|s| s.name == "provider.phase"));
        let trace_path = std::fs::read_to_string(dir.path().join("trace-path")).unwrap();
        assert!(!Path::new(&trace_path).exists());
    }
}

fn spawn_log(dir: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join("spawn.json")).unwrap()).unwrap()
}

#[test]
fn disabled_trace_preserves_provider_arguments_and_environment() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path(), "no-write");
    let mut baseline = config.providers.spawn("opy").unwrap();
    baseline.initialize_version("1.7", None).unwrap();
    baseline.shutdown().unwrap();
    drop(baseline);
    let expected = spawn_log(dir.path());
    let trace = Trace::disabled();
    let mut session = CompilerSession::new_traced(config, trace.clone()).unwrap();
    let outcome = session.compile();
    assert!(outcome.ok, "{:?}", outcome.diagnostics);
    drop(session);
    assert_eq!(spawn_log(dir.path()), expected);
    assert!(trace.report().is_none());
}
