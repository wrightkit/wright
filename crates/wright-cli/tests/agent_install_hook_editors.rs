//! `wright agent install --hook cursor|vscode` and `wright agent hook post-edit
//! --harness cursor|vscode` (#1110). The payloads follow the harnesses'
//! published references (cursor.com/docs/agent/hooks, the Copilot hooks
//! reference, read 2026-11-23), not a captured session.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

#[derive(Clone, Copy)]
struct Target {
    name: &'static str,
    file: &'static str,
    feedback_key: &'static str,
    activation: &'static str,
}

const TARGETS: [Target; 2] = [
    Target {
        name: "cursor",
        file: ".cursor/hooks.json",
        feedback_key: "additional_context",
        activation: "Hooks output channel",
    },
    Target {
        name: "vscode",
        file: ".github/copilot/settings.local.json",
        feedback_key: "additionalContext",
        activation: "Copilot as the session target",
    },
];
const GIVEN: [&str; 4] = ["--", "session.ws", "--kind", "workshop"];
const BROKEN_RULE: &str = "\nrule(\"x\") { broken\n";

fn project() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-agent-hook-editors-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/workshop/synthetic/control-flow.ws"),
        dir.join("session.ws"),
    )
    .unwrap();
    dir
}

fn wright() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wright"))
}

fn install(project: &Path, target: Target, args: &[&str]) -> Output {
    wright()
        .args(["agent", "install", "--hook", target.name])
        .args(args)
        .current_dir(project)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn settings(project: &Path, target: Target) -> Value {
    serde_json::from_slice(&std::fs::read(project.join(target.file)).unwrap()).unwrap()
}

fn expect_success(output: &Output) -> String {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn registers_one_flat_hook_and_preserves_foreign_entries() {
    for target in TARGETS {
        let project = project();
        let path = project.join(target.file);
        let dry = install(&project, target, &[&["--dry-run"][..], &GIVEN].concat());
        let stdout = expect_success(&dry);
        assert!(
            stdout.contains("would configure") && stdout.contains(target.file),
            "{stdout}"
        );
        assert_eq!(std::fs::read_dir(&project).unwrap().count(), 1);

        let first = install(&project, target, &[&["--no-guide"][..], &GIVEN].concat());
        let stdout = expect_success(&first);
        let command = format!(
            "wright agent hook post-edit --harness {} -- --kind workshop ./session.ws",
            target.name
        );
        assert!(
            stdout.contains(target.file) && stdout.contains(target.activation),
            "{stdout}"
        );
        assert!(
            stdout.contains(&format!("echo '{{}}' | {command}")),
            "{stdout}"
        );
        let hook = if target.name == "cursor" {
            json!({"type": "command", "command": command, "matcher": "Write", "timeout": 60})
        } else {
            json!({"type": "command", "command": command, "matcher": "create|edit|str_replace_editor|apply_patch", "timeoutSec": 60, "cwd": "."})
        };
        let mut expected = json!({"hooks": {"postToolUse": [hook]}});
        if target.name == "cursor" {
            expected["version"] = json!(1);
        }
        assert_eq!(settings(&project, target), expected, "{}", target.name);

        let foreign = json!({"type": "command", "command": "echo hi"});
        expected["hooks"]["postToolUse"] = json!([foreign, hook]);
        expected["hooks"]["sessionStart"] = json!([foreign]);
        expected["other"] = json!(true);
        let before = expected.to_string();
        std::fs::write(&path, &before).unwrap();

        let again = install(&project, target, &[&["--no-guide"][..], &GIVEN].concat());
        assert!(expect_success(&again).contains("up to date"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

        let changed = [&GIVEN[..], &["--locale", "en-US"]].concat();
        let refused = install(&project, target, &changed);
        assert_eq!(refused.status.code(), Some(1), "{refused:?}");
        assert!(String::from_utf8_lossy(&refused.stderr).contains("pass --force"));
        assert!(refused.stdout.is_empty() && !project.join(".agents").exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

        let dry = install(
            &project,
            target,
            &[&["--force", "--dry-run"][..], &changed].concat(),
        );
        assert!(expect_success(&dry).contains("would configure"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

        let forced = install(
            &project,
            target,
            &[&["--force", "--no-guide"][..], &changed].concat(),
        );
        assert!(expect_success(&forced).contains("replaced"));
        expected["hooks"]["postToolUse"][1]["command"] =
            json!(command.replace("--kind workshop", "--kind workshop --locale en-US"));
        assert_eq!(settings(&project, target), expected, "{}", target.name);
        std::fs::remove_dir_all(project).unwrap();
    }
}

#[test]
fn refuses_invalid_inputs_and_config_without_writing() {
    for target in TARGETS {
        let project = project();
        let missing = install(
            &project,
            target,
            &["--", "missing.ws", "--kind", "workshop"],
        );
        assert_eq!(missing.status.code(), Some(1), "{missing:?}");
        let stderr = String::from_utf8_lossy(&missing.stderr);
        assert!(
            stderr.contains("input-io") && stderr.contains("nothing was written"),
            "{stderr}"
        );
        assert!(
            stderr.contains(&format!("--hook {} --", target.name)),
            "{stderr}"
        );
        let usage = install(
            &project,
            target,
            &["--force", "--", "session.ws", "--kind", "nope"],
        );
        assert_eq!(usage.status.code(), Some(2), "{usage:?}");
        assert_eq!(std::fs::read_dir(&project).unwrap().count(), 1);

        let path = project.join(target.file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bad_configs = vec![(r#"{"hooks":{"postToolUse": {}}}"#, "hooks.postToolUse")];
        if target.name == "cursor" {
            bad_configs.push((r#"{"version":0,"hooks":{}}"#, "positive integer"));
        }
        for (text, diagnostic) in bad_configs {
            std::fs::write(&path, text).unwrap();
            let refused = install(&project, target, &[&["--force"][..], &GIVEN].concat());
            assert_eq!(refused.status.code(), Some(1), "{refused:?}");
            assert!(
                String::from_utf8_lossy(&refused.stderr).contains(diagnostic),
                "{refused:?}"
            );
            assert!(refused.stdout.is_empty() && !project.join(".agents").exists());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        }
        std::fs::remove_dir_all(project).unwrap();
    }
}

fn event(project: &Path, target: Target, args: Value) -> String {
    if target.name == "cursor" {
        json!({
            "hook_event_name": "postToolUse", "tool_name": "Write", "tool_input": args,
            "tool_output": "{}", "tool_use_id": "tool-1", "cwd": project, "duration": 10,
        })
    } else {
        json!({
            "sessionId": "session-1", "timestamp": 1, "cwd": project, "toolName": "edit",
            "toolArgs": args, "toolResult": {"resultType": "success", "textResultForLlm": "Edited"},
        })
    }
    .to_string()
}

fn run_hook(project: &Path, target: Target, event: &str) -> Output {
    let config = settings(project, target);
    let hook = &config["hooks"]["postToolUse"][0];
    let binary_dir = Path::new(env!("CARGO_BIN_EXE_wright")).parent().unwrap();
    let path = std::env::join_paths([binary_dir.to_owned()].into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    let mut child = Command::new("sh")
        .args(["-c", hook["command"].as_str().unwrap()])
        .current_dir(project.join(hook["cwd"].as_str().unwrap_or(".")))
        .env("PATH", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(event.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn reports_broken_programs_without_relying_on_an_edited_path() {
    for target in TARGETS {
        let project = project();
        std::fs::write(project.join("lint policy.toml"), "").unwrap();
        expect_success(&install(
            &project,
            target,
            &[&GIVEN[..], &["--lint-config=lint policy.toml"]].concat(),
        ));
        let no_path = event(&project, target, json!({}));
        let outside = project.join("notes.md");
        std::fs::write(&outside, "An edit outside the configured program.\n").unwrap();
        let outside_path = event(&project, target, json!({"file_path": outside}));
        for payload in [&no_path, &outside_path] {
            let clean = run_hook(&project, target, payload);
            assert!(expect_success(&clean).is_empty(), "{clean:?}");
        }

        let session = project.join("session.ws");
        let original = std::fs::read_to_string(&session).unwrap();
        std::fs::write(&session, format!("{original}{BROKEN_RULE}")).unwrap();
        let member_path = event(&project, target, json!({"file_path": session}));
        for payload in [&member_path, &no_path, &outside_path] {
            let stdout = expect_success(&run_hook(&project, target, payload));
            let result: Value = serde_json::from_str(&stdout).unwrap();
            let context = result[target.feedback_key].as_str().unwrap();
            assert_eq!(result.as_object().unwrap().len(), 1, "{result}");
            assert!(
                context.contains("error unknown-rule-section") && context.contains("'broken'"),
                "{context}"
            );
        }

        let invalid = run_hook(&project, target, "not json");
        assert_eq!(invalid.status.code(), Some(1), "{invalid:?}");
        assert!(invalid.stdout.is_empty());
        assert_eq!(
            String::from_utf8_lossy(&invalid.stderr).trim(),
            "wright: the hook input is not JSON"
        );
        std::fs::remove_dir_all(project).unwrap();
    }
}

#[test]
fn editor_hook_targets_are_discoverable() {
    for args in [
        &["agent", "--help"][..],
        &["agent", "install", "--help"],
        &["agent", "hook", "post-edit", "--help"],
    ] {
        let help = expect_success(&wright().args(args).output().unwrap());
        for target in TARGETS {
            assert!(help.contains(target.name), "{help}");
        }
    }
    let bash = expect_success(&wright().args(["completion", "bash"]).output().unwrap());
    for option in ["--hook", "--harness"] {
        let (_, candidates) = bash.split_once(&format!("{option})\n")).unwrap();
        let body = candidates.split_once(";;").unwrap().0;
        for target in TARGETS {
            assert!(body.contains(target.name), "{option} candidates: {body}");
        }
    }
}

#[test]
fn cursor_keeps_an_existing_config_version_and_adds_one_when_missing() {
    let target = TARGETS[0];
    for (existing, version) in [(r#"{"version":2,"hooks":{}}"#, 2), (r#"{"hooks":{}}"#, 1)] {
        let project = project();
        let path = project.join(target.file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, existing).unwrap();
        expect_success(&install(
            &project,
            target,
            &[&["--no-guide"][..], &GIVEN].concat(),
        ));
        assert_eq!(settings(&project, target)["version"], version);
        std::fs::remove_dir_all(project).unwrap();
    }
}
