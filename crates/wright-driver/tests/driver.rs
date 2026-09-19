//! Driver contract tests after the OPY provider cutover.
//!
//! Raw Workshop remains an in-process product path. OPY source behavior is
//! exercised at the provider boundary; these tests ensure a missing provider
//! cannot silently select a removed static frontend.

use std::path::{Path, PathBuf};

use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceBackend, SourceKind};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn legacy_protocol() -> &'static str {
    r#"{"protocol":{"name":"wright/opy-hir","version":"1.1.0"}}"#
}

fn workshop_fixture(id: &str) -> PathBuf {
    workspace_root()
        .join("tests/fixtures/workshop")
        .join(id)
        .with_extension("ws")
}

#[test]
fn workshop_runs_all_product_workflows() {
    let path = workshop_fixture("synthetic/control-flow");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("session creates");

    assert!(session.check().ok, "check: {:?}", session.diagnostics());
    assert!(session.compile().ok, "compile: {:?}", session.diagnostics());
    assert!(session.analyze().ok, "analyze: {:?}", session.diagnostics());
    assert!(session.lint().ok, "lint: {:?}", session.diagnostics());
    assert!(session.inspect().ok, "inspect: {:?}", session.diagnostics());
}

#[test]
fn workshop_compile_is_deterministic_and_idempotent() {
    let path = workshop_fixture("synthetic/basic-rule");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("session creates");
    let first = session.compile();
    let second = session.compile();
    assert!(
        first.ok && second.ok,
        "compile diagnostics: {:?}",
        session.diagnostics()
    );
    assert_eq!(
        first.result.output.as_ref().map(|output| &output.text),
        second.result.output.as_ref().map(|output| &output.text)
    );
}

#[test]
fn legacy_protocol_input_is_refused_without_hir_lowering() {
    let path = workspace_root()
        .join("target")
        .join(format!("wright-driver-{}", std::process::id()))
        .join("legacy.json");
    std::fs::create_dir_all(path.parent().unwrap()).expect("fixture directory creates");
    std::fs::write(&path, legacy_protocol()).expect("legacy fixture writes");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Auto,
        ..SessionConfig::default()
    })
    .expect("session creates");
    let result = session.check();
    assert!(!result.ok);
    assert_eq!(result.diagnostics[0].code, "input-kind-unsupported");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn opy_source_never_falls_back_to_a_static_frontend() {
    let path = workspace_root().join("tests/fixtures/opy/basic-rule.opy");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path),
        kind: SourceKind::Opy,
        source_backend: SourceBackend::Native,
        ..SessionConfig::default()
    })
    .expect("session creates");
    let result = session.check();
    assert!(!result.ok);
    assert_eq!(result.diagnostics[0].code, "source-provider-unavailable");
}
