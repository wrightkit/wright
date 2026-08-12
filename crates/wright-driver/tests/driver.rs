//! Reusable driver tests (#37/#41): the compiler/session driver serves every
//! core workflow over both Workshop and protocol inputs without the CLI, and
//! CLI and library consumers share this single orchestration path.

use std::path::{Path, PathBuf};

use wright_driver::CompilerSession;
use wright_driver::config::{InputSpec, SessionConfig, SourceKind};
use wright_driver::result::exit;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn fixture(fixture_id: &str) -> PathBuf {
    workspace_root()
        .join("compatibility/fixtures")
        .join(fixture_id)
        .join("oracle.json")
}

fn adapter_fixture(fixture_id: &str) -> PathBuf {
    workspace_root()
        .join("adapter/fixtures")
        .join(format!("{fixture_id}.json"))
}

fn corpus_workshop_text(fixture_id: &str) -> String {
    let oracle = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(fixture(fixture_id)).unwrap(),
    )
    .unwrap();
    oracle["compile"]["workshop"].as_str().unwrap().to_string()
}

fn corpus_source_opy(fixture_id: &str) -> String {
    std::fs::read_to_string(
        workspace_root()
            .join("compatibility/fixtures")
            .join(fixture_id)
            .join("source.opy"),
    )
    .unwrap()
}

fn temp_file(name: &str, content: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-driver-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn workshop_session(text: &str) -> CompilerSession {
    let path = temp_file("program.txt", text);
    CompilerSession::new(SessionConfig::from_path(path)).unwrap()
}

#[test]
fn workshop_compile_emits_corpus_text() {
    let text = corpus_workshop_text("synthetic/basic-rule");
    let mut session = workshop_session(&text);
    let envelope = session.compile();
    assert!(
        envelope.ok,
        "compile must succeed: {:?}",
        envelope.diagnostics
    );
    let output = envelope.result.output.expect("compiled output");
    assert_eq!(output.locale, "en-us");
    assert_eq!(output.input_identity.len(), 64);
    assert_eq!(output.sha256.len(), 64);
    assert!(
        output.text.contains("Disable Inspector Recording"),
        "emitted text: {}",
        output.text
    );
    // Compilation to a file path uses the same driver path as stdout.
    let out_path = temp_file("out.txt", "");
    let mut session = workshop_session(&text);
    session.config.output = Some(out_path.clone());
    let envelope = session.compile();
    assert!(envelope.ok);
    let stored = std::fs::read_to_string(&out_path).unwrap();
    assert_eq!(stored, output.text);
    let _ = std::fs::remove_dir_all(out_path.parent().unwrap());
}

#[test]
fn workshop_check_surfaces_analysis_findings_as_diagnostics() {
    let text = corpus_workshop_text("synthetic/control-flow");
    let mut session = workshop_session(&text);
    let envelope = session.check();
    assert!(envelope.ok, "check passes with warnings only");
    assert!(
        envelope
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "min-wait-loop"),
        "check must attach analysis findings: {:?}",
        envelope.diagnostics
    );
    assert_eq!(envelope.exit, exit::SUCCESS);
}

#[test]
fn workshop_analyze_reports_program_and_findings() {
    let text = corpus_workshop_text("synthetic/control-flow");
    let mut session = workshop_session(&text);
    let envelope = session.analyze();
    assert!(envelope.ok);
    assert_eq!(envelope.result.program["origin"]["kind"], "workshop");
    assert_eq!(envelope.result.program["origin"]["locale"], "en-us");
    assert_eq!(envelope.result.program["rules"], 2);
    let findings = envelope.result.findings.as_array().unwrap();
    assert!(
        findings
            .iter()
            .any(|finding| finding["code"] == "min-wait-loop"),
        "findings: {findings:?}"
    );
}

#[test]
fn workshop_inspect_returns_structural_model() {
    let text = corpus_workshop_text("synthetic/declarations-rules");
    let mut session = workshop_session(&text);
    let envelope = session.inspect();
    assert!(envelope.ok);
    let rules = envelope.result.rules.as_array().unwrap();
    assert!(!rules.is_empty(), "inspect lists rules");
    let symbols = envelope.result.symbols.as_array().unwrap();
    assert!(!symbols.is_empty(), "inspect lists symbols");
    let references = envelope.result.references.as_array().unwrap();
    assert_eq!(references.len(), symbols.len(), "references per symbol");
}

#[test]
fn protocol_input_runs_all_workflows() {
    for fixture_id in [
        "synthetic/basic-rule",
        "synthetic/control-flow",
        "synthetic/declarations-rules",
    ] {
        let path = adapter_fixture(fixture_id);
        let mut session = CompilerSession::new(SessionConfig::from_path(path)).unwrap();
        assert!(session.check().ok, "{fixture_id} check");
        assert!(session.analyze().ok, "{fixture_id} analyze");
        let inspect = session.inspect();
        assert!(inspect.ok, "{fixture_id} inspect");
        assert_eq!(inspect.result.program["origin"]["kind"], "protocol");
    }
}

#[test]
fn opy_input_compiles_through_the_native_frontend() {
    let source = corpus_source_opy("synthetic/basic-rule");
    let path = temp_file("basic-rule.opy", &source);
    let mut session = CompilerSession::new(SessionConfig::from_path(path.clone())).unwrap();
    let envelope = session.compile();
    assert!(envelope.ok, "opy compile: {:?}", envelope.diagnostics);
    let output = envelope.result.output.expect("output");
    let oracle = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(fixture("synthetic/basic-rule")).unwrap(),
    )
    .unwrap();
    let expected = oracle["compile"]["workshop"].as_str().unwrap();
    assert_eq!(
        output.text.trim(),
        expected.trim(),
        "the native .opy path must reproduce the oracle Workshop text"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn malformed_workshop_input_fails_structurally() {
    // Enough locale evidence to pass detection, then a syntax error.
    let mut session =
        workshop_session("rule (\"broken\") { event { Ongoing - Global; } actions { If(True); }");
    let envelope = session.check();
    assert!(!envelope.ok);
    assert_eq!(envelope.exit, exit::SOURCE_ERROR);
    let diagnostic = &envelope.diagnostics[0];
    assert!(diagnostic.span.is_some(), "parse errors carry spans");
    assert_eq!(diagnostic.source.as_ref().unwrap().kind, "workshop");
}

#[test]
fn explicit_locale_override_wins() {
    let text = corpus_workshop_text("synthetic/basic-rule");
    let path = temp_file("program.txt", &text);
    let mut session = CompilerSession::new(SessionConfig::from_path(path.clone())).unwrap();
    session.config.locale = Some("en-US".to_string());
    let envelope = session.check();
    assert!(envelope.ok, "explicit locale: {:?}", envelope.diagnostics);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn stdin_opy_is_a_supported_kind() {
    // `.opy` on stdin is no longer rejected: the native frontend owns it.
    // (The driver reads the process stdin at load time; an empty stdin fails
    // with `stdin-empty`, proving the kind check no longer blocks `.opy`.)
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Stdin,
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    })
    .unwrap();
    let envelope = session.check();
    assert!(
        !envelope
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "stdin-opy-unsupported"),
        "stdin .opy is supported by the native frontend"
    );
}

#[test]
fn loading_is_idempotent() {
    let text = corpus_workshop_text("synthetic/basic-rule");
    let mut session = workshop_session(&text);
    let first = session.load().unwrap();
    let identity = first.input.identity.clone();
    let second = session.load().unwrap();
    assert_eq!(second.input.identity, identity);
    assert_eq!(second.program.rules.len(), 1);
}

#[test]
fn compiled_output_is_deterministic() {
    let text = corpus_workshop_text("synthetic/declarations-rules");
    let mut first = workshop_session(&text);
    let mut second = workshop_session(&text);
    let a = first.compile().result.output.unwrap();
    let b = second.compile().result.output.unwrap();
    assert_eq!(a.text, b.text);
    assert_eq!(a.sha256, b.sha256);
    assert_eq!(a.input_identity, b.input_identity);
}
