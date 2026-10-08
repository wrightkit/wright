//! Validated lint fixes (#556): `duplicate-condition` and `repeated-value`
//! findings carry a materialized `EditTransaction` on both the session and
//! ToolService surfaces; applying it through the existing edit path removes
//! the finding and keeps `check` clean, and a stale source refuses with no
//! write.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use wright_driver::edit::{EditTransaction, write_previews};
use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::{CompilerSession, FindingSelection, InputSpec, SessionConfig, SourceKind};

const SOURCE: &str = r#"variables {
    global:
        0: index
        1: other
}
rule ("duplicates") {
    event {
        Ongoing - Global;
    }
    actions {
        If(Compare(Global.index, ==, 0));
            Set Global Variable(other, 1);
        Else If(Compare(Global.index, ==, 1));
            Set Global Variable(other, 2);
        Else If(Compare(Global.index, ==, 0));
            Set Global Variable(other, 3);
        Else;
            Set Global Variable(other, 4);
        End;
        While(Compare(Global.index, <, 3));
            Set Global Variable(other, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            Set Global Variable(index, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            Wait(0.016, Ignore Condition);
        End;
    }
}
"#;

fn temp_workshop(text: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-lint-fix-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("program.ws");
    std::fs::write(&path, text).unwrap();
    path
}

fn make_session(path: PathBuf) -> CompilerSession {
    CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap()
}

fn fixed(findings: &serde_json::Value, code: &str) -> serde_json::Value {
    findings
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["code"] == code)
        .and_then(|finding| finding.get("fix"))
        .cloned()
        .unwrap_or_else(|| panic!("{code} carries a fix"))
}

#[test]
fn lint_findings_carry_fix_transactions_on_session_and_service() {
    let path = temp_workshop(SOURCE);
    let mut session = make_session(path);
    let envelope = session.lint();
    assert!(envelope.ok, "lint succeeds: {:?}", envelope.diagnostics);

    let duplicate = fixed(&envelope.result.findings, "duplicate-condition");
    assert_eq!(duplicate["kind"], "remove-dead-branch");
    assert_eq!(
        duplicate["transaction"]["edits"].as_array().unwrap().len(),
        1
    );

    let repeated = fixed(&envelope.result.findings, "repeated-value");
    assert_eq!(repeated["kind"], "evaluate-once");
    assert_eq!(
        repeated["transaction"]["edits"].as_array().unwrap().len(),
        2,
        "both duplicated occurrences wrap"
    );

    // The agent surfaces return the same fix payloads as the session.
    let mut service = ToolService::new(&mut session).unwrap();
    let lint = match service.handle(&ToolRequest::Lint {
        selection: FindingSelection::default(),
        brief: false,
    }) {
        ToolResponse::Ok { result } => result,
        ToolResponse::Error { error } => panic!("lint op: {error:?}"),
    };
    let findings = match service.handle(&ToolRequest::Findings(FindingSelection::default())) {
        ToolResponse::Ok { result } => result,
        ToolResponse::Error { error } => panic!("findings op: {error:?}"),
    };
    assert_eq!(
        fixed(&lint["findings"], "duplicate-condition"),
        duplicate,
        "the lint operation returns the session's fix"
    );
    assert_eq!(
        fixed(&findings, "duplicate-condition"),
        duplicate,
        "the findings operation returns the same fix"
    );
    assert_eq!(
        fixed(&findings, "repeated-value"),
        repeated,
        "the findings operation returns the same fix"
    );
}

#[test]
fn lint_fix_preview_validates_and_write_applies_to_completion() {
    let path = temp_workshop(SOURCE);
    let mut session = make_session(path.clone());

    let preview = session.lint_fix(false);
    assert!(preview.ok);
    let fixes = preview.result.fixes.expect("fixes are reported");
    assert_eq!(fixes.len(), 2);
    assert!(
        fixes
            .iter()
            .all(|fix| fix.status == wright_driver::result::LintFixStatus::Preview),
        "a preview run validates every fix without writing: {fixes:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        SOURCE,
        "preview writes nothing"
    );

    let mut session = make_session(path.clone());
    let written = session.lint_fix(true);
    assert!(written.ok, "{:?}", written.diagnostics);
    let fixes = written.result.fixes.unwrap();
    assert_eq!(
        fixes
            .iter()
            .filter(|fix| fix.status == wright_driver::result::LintFixStatus::Applied)
            .count(),
        2
    );

    let applied = std::fs::read_to_string(&path).unwrap();
    assert!(!applied.contains("Else If(Compare(Global.index, ==, 0));"));
    assert!(!applied.contains("Set Global Variable(other, 3)"));
    assert_eq!(
        applied
            .matches("Evaluate Once(Distance Between(Position Of(Event Player), Vector(0, 0, 0)))")
            .count(),
        2,
        "each occurrence wraps the complete expression: {applied}"
    );

    // The resolved findings are gone and the input still checks clean.
    let mut session = make_session(path);
    let relint = session.lint();
    assert!(
        relint
            .result
            .findings
            .as_array()
            .unwrap()
            .iter()
            .all(|finding| !matches!(
                finding["code"].as_str(),
                Some("duplicate-condition") | Some("repeated-value")
            )),
        "applied fixes remove their findings: {}",
        relint.result.findings
    );
    let checked = session.check();
    assert!(checked.ok, "{:?}", checked.diagnostics);
}

#[test]
fn a_fix_refuses_a_stale_source_and_writes_nothing() {
    let path = temp_workshop(SOURCE);
    let mut session = make_session(path.clone());
    let envelope = session.lint();
    let fix = fixed(&envelope.result.findings, "duplicate-condition");
    let transaction: EditTransaction =
        serde_json::from_value(fix["transaction"].clone()).expect("fix carries a transaction");

    // The source moves after the fix was planned.
    let edited = SOURCE.replacen("other, 1", "other, 9", 1);
    std::fs::write(&path, &edited).unwrap();

    let validation = session.validate_edit_transaction(None, &transaction);
    assert!(!validation.ok);
    assert_eq!(validation.diagnostics[0].code, "edit-stale-source");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        edited,
        "a refused fix writes nothing"
    );

    // The write path checks the same precondition again before touching
    // disk: previews materialized while the source was current refuse once
    // the file moves.
    let path = temp_workshop(SOURCE);
    let mut session = make_session(path.clone());
    let envelope = session.lint();
    let fix = fixed(&envelope.result.findings, "duplicate-condition");
    let transaction: EditTransaction =
        serde_json::from_value(fix["transaction"].clone()).expect("fix carries a transaction");
    let validation = session.validate_edit_transaction(None, &transaction);
    assert!(validation.ok, "a fresh transaction validates");
    let previews = validation.preview.unwrap_or_default();

    let moved = SOURCE.replacen("other, 1", "other, 9", 1);
    std::fs::write(&path, &moved).unwrap();
    let error = write_previews(&previews, &transaction).unwrap_err();
    assert_eq!(error.code, "edit-stale-source");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), moved);
}

#[test]
fn lint_fix_write_refuses_a_caller_held_text_input() {
    // A `Text` session's buffer is its source: writing the named file and
    // reloading resolves the unchanged buffer, so the batch would apply one
    // fix then fail its own stale-source precondition. Refuse before the
    // first write instead.
    let path = temp_workshop(SOURCE);
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Text {
            text: SOURCE.to_string(),
            path: Some(path.clone()),
        },
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();

    let preview = session.lint_fix(false);
    assert!(preview.ok, "preview still runs on the buffer");

    let written = session.lint_fix(true);
    assert!(!written.ok);
    assert!(
        written
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "edit-input-stdin"),
        "the write refuses a non-path input: {:?}",
        written.diagnostics
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        SOURCE,
        "the refusal writes nothing"
    );

    // rename --write carries the same boundary.
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Text {
            text: SOURCE.to_string(),
            path: Some(path.clone()),
        },
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let renamed = session.rename("index", "renamed", true);
    assert!(
        renamed
            .diagnostics
            .iter()
            .any(|d| d.code == "edit-input-stdin"),
        "rename --write refuses a non-path input: {:?}",
        renamed.diagnostics
    );
    assert!(renamed.result.written.is_empty(), "nothing is written");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), SOURCE);
}
