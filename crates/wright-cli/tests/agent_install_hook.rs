use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

fn temp_dir() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-agent-hook-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn project() -> PathBuf {
    let dir = temp_dir();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/workshop/synthetic/control-flow.ws"),
        dir.join("session.ws"),
    )
    .unwrap();
    dir
}

fn install(project: &Path, args: &[&str]) -> Output {
    install_for(project, "claude", args)
}

fn install_for(project: &Path, hook: &str, args: &[&str]) -> Output {
    Command::new(wright())
        .args(["agent", "install", "--hook", hook, "--no-guide"])
        .args(args)
        .current_dir(project)
        .stdin(Stdio::null())
        .output()
        .expect("wright runs")
}

fn settings(project: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(project.join(".claude/settings.local.json")).unwrap())
        .unwrap()
}

/// Runs the stored hook command the way Claude Code does: through a shell,
/// with the project directory variable set and the hook JSON on stdin.
fn run_hook(project: &Path, file: &Path, stdin: Option<&str>) -> Output {
    let command = settings(project)["hooks"]["PostToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .replacen("wright", wright(), 1);
    let event = stdin.map(String::from).unwrap_or_else(|| {
        serde_json::json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "Edit",
            "tool_input": {"file_path": file},
            "cwd": project,
        })
        .to_string()
    });
    let mut child = Command::new("sh")
        .args(["-c", &command])
        .env("CLAUDE_PROJECT_DIR", project)
        .current_dir(std::env::temp_dir())
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

#[test]
fn install_writes_one_entry_idempotently_and_preserves_other_hooks() {
    let project = project();
    let given = ["--", "session.ws", "--kind", "workshop"];
    let dry = install(&project, &[&["--dry-run"][..], &given].concat());
    assert!(dry.status.success());
    assert!(!project.join(".claude").exists());

    let foreign = serde_json::json!({
        "permissions": {"allow": ["Bash(ls)"]},
        "hooks": {"PostToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "echo hi"}]}]},
    });
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    std::fs::write(
        project.join(".claude/settings.local.json"),
        foreign.to_string(),
    )
    .unwrap();
    assert!(install(&project, &given).status.success());
    let first = std::fs::read(project.join(".claude/settings.local.json")).unwrap();
    let installed = settings(&project);
    assert_eq!(installed["permissions"], foreign["permissions"]);
    let groups = installed["hooks"]["PostToolUse"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0], foreign["hooks"]["PostToolUse"][0]);
    assert_eq!(groups[1]["matcher"], "Edit|Write");
    assert_eq!(groups[1]["hooks"][0]["type"], "command");
    assert_eq!(groups[1]["hooks"][0]["timeout"], 60);
    assert_eq!(
        groups[1]["hooks"][0]["command"],
        "wright agent hook post-edit -- --kind workshop \"${CLAUDE_PROJECT_DIR:-.}/session.ws\""
    );

    let again = install(&project, &given);
    assert!(String::from_utf8_lossy(&again.stdout).contains("up to date"));
    assert_eq!(
        std::fs::read(project.join(".claude/settings.local.json")).unwrap(),
        first
    );

    let other = install(
        &project,
        &[
            "--",
            "session.ws",
            "--kind",
            "workshop",
            "--locale",
            "en-US",
        ],
    );
    assert_eq!(other.status.code(), Some(1));
    assert_eq!(
        std::fs::read(project.join(".claude/settings.local.json")).unwrap(),
        first
    );
    let forced = install(
        &project,
        &[
            "--force",
            "--",
            "session.ws",
            "--kind",
            "workshop",
            "--locale",
            "en-US",
        ],
    );
    assert!(forced.status.success());
    assert_eq!(
        settings(&project)["hooks"]["PostToolUse"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn install_refuses_a_program_that_does_not_load_unless_forced() {
    let project = project();
    let missing = install(&project, &["--", "nope.ws", "--kind", "workshop"]);
    assert_eq!(missing.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(
        stderr.contains("input-io") && stderr.contains("-- "),
        "{stderr}"
    );
    assert!(!project.join(".claude").exists());
    let ostw = install(&project, &["--", "session.ws", "--kind", "ostw"]);
    assert_eq!(ostw.status.code(), Some(1));
    assert!(!project.join(".claude").exists());
    let usage = install(&project, &["--", "session.ws", "--kind", "nope"]);
    assert_eq!(usage.status.code(), Some(2));
    let forced_usage = install(&project, &["--force", "--", "session.ws", "--kind", "nope"]);
    assert_eq!(forced_usage.status.code(), Some(2));
    assert!(!project.join(".claude").exists());
    let forced = install(
        &project,
        &["--force", "--", "nope.ws", "--kind", "workshop"],
    );
    assert!(forced.status.success());
    assert!(project.join(".claude/settings.local.json").is_file());
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn a_locale_failure_names_the_locale_option_and_success_prints_next_steps() {
    let project = temp_dir();
    std::fs::write(
        project.join("session.ws"),
        "rule(\"x\") {\n\tevent {\n\t\tOngoing - Global;\n\t}\n\tconditions {\n\t\tCurrent Map == Map(格里姆火山);\n\t}\n\tactions {\n\t\tWait(1, Ignore Condition);\n\t}\n}\n",
    )
    .unwrap();
    let refused = install(&project, &["--", "session.ws", "--kind", "workshop"]);
    assert_eq!(refused.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("nothing was written")
            && stderr.contains("-- session.ws --kind workshop --locale zh-CN"),
        "{stderr}"
    );
    assert!(!project.join(".claude").exists());
    let args = [
        "--",
        "session.ws",
        "--kind",
        "workshop",
        "--locale",
        "zh-CN",
    ];
    let installed = install(&project, &args);
    assert!(installed.status.success());
    let stdout = String::from_utf8_lossy(&installed.stdout);
    assert!(
        stdout.contains("next steps:")
            && stdout.contains("verify: run `echo '{}' | wright agent hook post-edit"),
        "{stdout}"
    );
    let _ = std::fs::remove_dir_all(&project);
}

const LOCALE_RULE: &str = "rule(\"x\") {\n\tevent {\n\t\tOngoing - Global;\n\t}\n\tconditions {\n\t\tCurrent Map == Map(格里姆火山);\n\t}\n\tactions {\n\t\tWait(1, Ignore Condition);\n\t}\n}\n";

/// Refuses an install that needs `zh-CN`, then runs the printed retry
/// literally through `sh` and checks what it stored.
fn assert_retry_runs(hook: &str, config: &str, anchor: &str, name: &str, kind: &str) {
    let project = temp_dir();
    if kind == "opy" {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/opy/locale-hook");
        for entry in std::fs::read_dir(&fixture).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), project.join(entry.file_name())).unwrap();
        }
        std::fs::rename(project.join("main.opy"), project.join(name)).unwrap();
    } else {
        std::fs::write(project.join(name), LOCALE_RULE).unwrap();
    }
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&project)
        .status()
        .unwrap();
    assert!(git.success());
    let refused = install_for(&project, hook, &["--", name, "--kind", kind]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(!project.join(config).exists());
    let stderr = String::from_utf8_lossy(&refused.stderr);
    let retry = stderr
        .split('`')
        .find(|part| part.starts_with("wright agent install"))
        .unwrap_or_else(|| panic!("no retry in {stderr}"));
    let retry = retry.replacen("wright", &format!("'{}'", wright()), 1);
    let ran = Command::new("sh")
        .args(["-c", &retry])
        .current_dir(&project)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "{retry}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let text = std::fs::read_to_string(project.join(config)).unwrap();
    let stored: serde_json::Value = serde_json::from_str(&text).unwrap();
    let command = stored.to_string();
    assert!(command.contains("--locale zh-CN"), "{text}");
    let anchored = format!("{anchor}/{name}");
    assert!(command.contains(&anchored), "{anchored} not in {text}");
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn the_printed_locale_retry_runs_in_a_shell_for_an_input_with_a_space_or_quote() {
    let claude = ".claude/settings.local.json";
    let claude_anchor = "${CLAUDE_PROJECT_DIR:-.}";
    let codex_anchor = "$(git rev-parse --show-toplevel)";
    assert_retry_runs("claude", claude, claude_anchor, "main mode.ws", "workshop");
    assert_retry_runs("claude", claude, claude_anchor, "it's.ws", "workshop");
    assert_retry_runs(
        "codex",
        ".codex/hooks.json",
        codex_anchor,
        "main mode.ws",
        "workshop",
    );
}

#[test]
fn the_printed_locale_retry_runs_in_a_shell_for_an_opy_input_with_a_space() {
    let installed = wright_driver::OpyProviderConfig::default()
        .installed()
        .expect("local provider inventory");
    assert!(
        installed.is_some() || std::env::var_os("CI").is_none(),
        "CI must install the OPY provider before this suite"
    );
    if installed.is_none() {
        eprintln!("SKIPPED: no installed OPY provider");
        return;
    }
    assert_retry_runs(
        "claude",
        ".claude/settings.local.json",
        "${CLAUDE_PROJECT_DIR:-.}",
        "main mode.opy",
        "opy",
    );
}

#[test]
fn force_replaces_only_the_wright_hook_object() {
    let project = project();
    let given = ["--", "session.ws", "--kind", "workshop"];
    assert!(install(&project, &given).status.success());
    let mut config = settings(&project);
    let group = &mut config["hooks"]["PostToolUse"][0]["hooks"];
    group
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"type": "command", "command": "my-formatter"}));
    std::fs::write(
        project.join(".claude/settings.local.json"),
        config.to_string(),
    )
    .unwrap();
    let again = install(&project, &given);
    assert!(String::from_utf8_lossy(&again.stdout).contains("up to date"));
    let forced = install(
        &project,
        &[
            "--force",
            "--",
            "session.ws",
            "--kind",
            "workshop",
            "--locale",
            "en-US",
        ],
    );
    assert!(forced.status.success());
    let text = settings(&project).to_string();
    assert!(text.contains("my-formatter"), "{text}");
    assert_eq!(text.matches("wright agent hook post-edit").count(), 1);
    assert!(text.contains("--locale en-US"));
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn a_refused_hook_leaves_the_mcp_entry_unwritten() {
    let project = project();
    let output = Command::new(wright())
        .args([
            "agent",
            "install",
            "--no-guide",
            "--mcp",
            "claude",
            "--hook",
            "claude",
        ])
        .args(["--", "session.ws", "--kind", "workshop"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(output.status.success());
    std::fs::remove_file(project.join(".mcp.json")).unwrap();
    let foreign = serde_json::json!({"hooks": {"PostToolUse": [{"hooks": [
        {"type": "command", "command": "wright agent hook post-edit -- other.ws"}
    ]}]}});
    std::fs::write(
        project.join(".claude/settings.local.json"),
        foreign.to_string(),
    )
    .unwrap();
    let refused = Command::new(wright())
        .args([
            "agent",
            "install",
            "--no-guide",
            "--mcp",
            "claude",
            "--hook",
            "claude",
        ])
        .args(["--", "session.ws", "--kind", "workshop"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(1));
    assert!(!project.join(".mcp.json").exists());
    assert_eq!(settings(&project), foreign);
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn post_edit_reports_errors_to_the_model_and_nothing_otherwise() {
    let project = project();
    let session = project.join("session.ws");
    assert!(
        install(&project, &["--", "session.ws", "--kind", "workshop"])
            .status
            .success()
    );

    let clean = run_hook(&project, &session, None);
    assert_eq!(clean.status.code(), Some(0));
    assert!(clean.stdout.is_empty() && clean.stderr.is_empty());

    let original = std::fs::read_to_string(&session).unwrap();
    std::fs::write(&session, format!("{original}\nrule(\"x\") {{ broken\n")).unwrap();
    let broken = run_hook(&project, &session, None);
    assert_eq!(broken.status.code(), Some(2));
    assert!(broken.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&broken.stderr);
    assert!(stderr.contains("error unknown-rule-section"), "{stderr}");

    std::fs::write(&session, original).unwrap();
    let missing = run_hook(&project, &session, Some(r#"{"tool_input":{}}"#));
    assert_eq!(missing.status.code(), Some(0));
    let garbled = run_hook(&project, &session, Some("not json"));
    assert_eq!(garbled.status.code(), Some(1));
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn post_edit_ignores_a_file_outside_a_program_that_has_errors() {
    let project = project();
    assert!(
        install(
            &project,
            &[
                "--",
                "session.ws",
                "--kind",
                "workshop",
                "--rule-severity",
                "min-wait-loop:error"
            ]
        )
        .status
        .success()
    );
    let session = project.join("session.ws");
    let member = run_hook(&project, &session, None);
    assert_eq!(member.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&member.stderr).contains("min-wait-loop"));

    let outside = project.join("notes.md");
    std::fs::write(&outside, "x").unwrap();
    let ignored = run_hook(&project, &outside, None);
    assert_eq!(ignored.status.code(), Some(0));
    assert!(ignored.stdout.is_empty() && ignored.stderr.is_empty());
    let _ = std::fs::remove_dir_all(&project);
}
