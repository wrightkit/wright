//! `wright agent install --hook codex` and `wright agent hook post-edit
//! --harness codex` (#1109). The payload shape is the one `codex exec`
//! 0.169.0 sent to a `PostToolUse` hook, captured for the Issue.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

/// A git repository holding the control-flow program as `session.ws`.
fn project() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-agent-hook-codex-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/workshop/synthetic/control-flow.ws"),
        dir.join("session.ws"),
    )
    .unwrap();
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&dir)
        .status()
        .unwrap();
    assert!(git.success());
    dir
}

fn install(project: &Path, args: &[&str]) -> Output {
    Command::new(wright())
        .args(["agent", "install", "--hook", "codex", "--no-guide"])
        .args(args)
        .current_dir(project)
        .stdin(Stdio::null())
        .output()
        .expect("wright runs")
}

const GIVEN: [&str; 4] = ["--", "session.ws", "--kind", "workshop"];

fn config(project: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(project.join(".codex/hooks.json")).unwrap()).unwrap()
}

/// The `apply_patch` payload of a Codex `PostToolUse` event.
fn patch_event(project: &Path, patch: &str) -> String {
    json!({
        "hook_event_name": "PostToolUse",
        "cwd": project,
        "tool_name": "apply_patch",
        "tool_input": {"command": format!("*** Begin Patch\n{patch}\n*** End Patch")},
    })
    .to_string()
}

/// Runs the stored command the way Codex does: through a shell in the
/// session directory, with the event JSON on stdin.
fn run_hook(project: &Path, event: &str) -> Output {
    let command = config(project)["hooks"]["PostToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .replacen("wright", wright(), 1);
    let mut child = Command::new("sh")
        .args(["-c", &command])
        .current_dir(project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("shell runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(event.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn context(output: &Output) -> String {
    let answer: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    assert_eq!(answer["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    answer["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .to_string()
}

fn break_program(project: &Path) -> String {
    let session = project.join("session.ws");
    let original = std::fs::read_to_string(&session).unwrap();
    std::fs::write(&session, format!("{original}\nrule(\"x\") {{ broken\n")).unwrap();
    original
}

#[test]
fn install_writes_one_entry_idempotently_and_preserves_other_hooks() {
    let project = project();
    let dry = install(&project, &[&["--dry-run"][..], &GIVEN].concat());
    assert!(dry.status.success());
    assert!(!project.join(".codex").exists());

    let foreign = json!({
        "other": 1,
        "hooks": {"PostToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "echo hi"}]}]},
    });
    std::fs::create_dir_all(project.join(".codex")).unwrap();
    std::fs::write(project.join(".codex/hooks.json"), foreign.to_string()).unwrap();
    assert!(install(&project, &GIVEN).status.success());
    let first = std::fs::read(project.join(".codex/hooks.json")).unwrap();
    let installed = config(&project);
    assert_eq!(installed["other"], 1);
    let groups = installed["hooks"]["PostToolUse"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0], foreign["hooks"]["PostToolUse"][0]);
    assert_eq!(groups[1]["matcher"], "apply_patch");
    assert_eq!(groups[1]["hooks"][0]["type"], "command");
    assert_eq!(groups[1]["hooks"][0]["timeout"], 60);
    assert_eq!(
        groups[1]["hooks"][0]["command"],
        "wright agent hook post-edit --harness codex -- --kind workshop \"$(git rev-parse --show-toplevel)/session.ws\""
    );

    let again = install(&project, &GIVEN);
    assert!(String::from_utf8_lossy(&again.stdout).contains("up to date"));
    assert_eq!(
        std::fs::read(project.join(".codex/hooks.json")).unwrap(),
        first
    );

    let differing = [
        "--",
        "session.ws",
        "--kind",
        "workshop",
        "--locale",
        "en-US",
    ];
    let refused = install(&project, &differing);
    assert_eq!(refused.status.code(), Some(1));
    assert_eq!(
        std::fs::read(project.join(".codex/hooks.json")).unwrap(),
        first
    );
    let forced = install(&project, &[&["--force"][..], &differing].concat());
    assert!(forced.status.success());
    let text = config(&project).to_string();
    assert!(text.contains("echo hi") && text.contains("--locale en-US"));
    assert_eq!(text.matches("wright agent hook post-edit").count(), 1);
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn a_refused_install_writes_nothing() {
    let project = project();
    let missing = install(&project, &["--", "nope.ws", "--kind", "workshop"]);
    assert_eq!(missing.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(
        stderr.contains("--hook codex --") && stderr.contains("input-io"),
        "{stderr}"
    );
    assert!(!project.join(".codex").exists());
    let usage = install(&project, &["--", "session.ws", "--kind", "nope"]);
    assert_eq!(usage.status.code(), Some(2));
    assert!(!project.join(".codex").exists());

    let wrong_shape = json!({"hooks": {"PostToolUse": {}}});
    std::fs::create_dir_all(project.join(".codex")).unwrap();
    std::fs::write(project.join(".codex/hooks.json"), wrong_shape.to_string()).unwrap();
    assert_eq!(install(&project, &GIVEN).status.code(), Some(1));
    assert_eq!(config(&project), wrong_shape);
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn post_edit_gives_the_model_the_errors_as_additional_context() {
    let project = project();
    assert!(install(&project, &GIVEN).status.success());
    let session = project.join("session.ws");
    let patch = |file: &str| patch_event(&project, &format!("*** Update File: {file}\n@@\n-x\n+y"));

    let clean = run_hook(&project, &patch(session.to_str().unwrap()));
    assert_eq!(clean.status.code(), Some(0));
    assert!(clean.stdout.is_empty() && clean.stderr.is_empty());

    let original = break_program(&project);
    for file in [session.to_str().unwrap(), "session.ws"] {
        let broken = run_hook(&project, &patch(file));
        assert_eq!(broken.status.code(), Some(0), "{file}");
        assert!(broken.stderr.is_empty());
        assert!(
            context(&broken).contains("error unknown-rule-section"),
            "{file}"
        );
    }

    let both = run_hook(
        &project,
        &patch_event(
            &project,
            "*** Add File: notes.md\n+x\n*** Update File: session.ws\n@@\n-x\n+y",
        ),
    );
    assert!(context(&both).contains("unknown-rule-section"));

    std::fs::write(&session, original).unwrap();
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn post_edit_ignores_files_outside_a_program_that_has_errors() {
    let project = project();
    let given = [&GIVEN[..], &["--rule-severity", "min-wait-loop:error"]].concat();
    assert!(install(&project, &given).status.success());
    let patch = |file: &str| patch_event(&project, &format!("*** Update File: {file}\n@@\n-x\n+y"));
    let member = run_hook(&project, &patch("session.ws"));
    assert_eq!(member.status.code(), Some(0));
    assert!(context(&member).contains("min-wait-loop"));

    let outside = run_hook(&project, &patch("notes.md"));
    assert_eq!(outside.status.code(), Some(0));
    assert!(outside.stdout.is_empty() && outside.stderr.is_empty());
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn post_edit_without_a_readable_path_checks_the_whole_program() {
    let project = project();
    assert!(install(&project, &GIVEN).status.success());
    let unreadable = json!({"cwd": project, "tool_name": "apply_patch", "tool_input": {}});
    let clean = run_hook(&project, &unreadable.to_string());
    assert_eq!(clean.status.code(), Some(0));
    assert!(clean.stdout.is_empty() && clean.stderr.is_empty());

    break_program(&project);
    let broken = run_hook(&project, &unreadable.to_string());
    assert_eq!(broken.status.code(), Some(0));
    assert!(context(&broken).contains("unknown-rule-section"));

    let garbled = run_hook(&project, "not json");
    assert_eq!(garbled.status.code(), Some(1));
    assert!(garbled.stdout.is_empty());
    let _ = std::fs::remove_dir_all(&project);
}
