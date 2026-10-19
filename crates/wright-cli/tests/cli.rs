use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn corpus_workshop(fixture_id: &str) -> String {
    std::fs::read_to_string(
        workspace_root()
            .join("tests/fixtures/workshop")
            .join(fixture_id)
            .with_extension("ws"),
    )
    .unwrap()
}

fn temp_file(name: &str, content: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-cli-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn temp_dir() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-cli-dir-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> std::process::Output {
    run_with_env(args, &[])
}

fn run_with_env(args: &[&str], variables: &[(&str, &str)]) -> std::process::Output {
    let mut command = Command::new(wright());
    command
        .args(args)
        .stdin(Stdio::null())
        .env_remove("CI")
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITHUB_STEP_SUMMARY")
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR");
    for (name, value) in variables {
        command.env(name, value);
    }
    command.output().expect("wright runs")
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run_in_tty(args: &[&str]) -> std::process::Output {
    #[cfg(target_os = "linux")]
    let command = std::iter::once(wright())
        .chain(args.iter().copied())
        .map(|arg| {
            if arg
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || "-_/.:".contains(ch))
            {
                arg.to_string()
            } else {
                format!("'{}'", arg.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mut script = Command::new("script");
    #[cfg(target_os = "linux")]
    script.args(["-qefc", &command, "/dev/null"]);
    #[cfg(target_os = "macos")]
    script.args(["-q", "/dev/null", wright()]).args(args);
    script
        .env("TERM", "xterm")
        .output()
        .expect("script is available")
}

fn run_with_stdin(args: &[&str], stdin: &str) -> std::process::Output {
    let mut child = Command::new(wright())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wright runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn parse_json(output: &[u8]) -> serde_json::Value {
    serde_json::from_slice(output).expect("stdout is one JSON envelope")
}

fn command_result(output: &std::process::Output) -> String {
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn compile_over_workshop_file_emits_correct_text() {
    let path = temp_file("basic.txt", &corpus_workshop("synthetic/basic-rule"));
    let output = run(&["compile", path.to_str().unwrap(), "-o", "-"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty(), "stderr clean on success");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Disable Inspector Recording"), "{stdout}");
    assert!(stdout.contains("Ongoing - Global"));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// A program over the Overwatch client element limit (#488) still emits its
/// artifact (stdout with `-o -`), with the import-limit warning on stderr;
/// in JSON mode the warning rides inside the envelope's diagnostics.
#[test]
fn compile_over_element_limit_warns_and_still_emits() {
    let array = (0..17_000).map(|_| "1").collect::<Vec<_>>().join(", ");
    let source = format!(
        "variables {{\n    global:\n        0: values\n}}\n\nrule (\"fill\") {{\n    event {{\n        Ongoing - Global;\n    }}\n    actions {{\n        Set Global Variable(values, Array({array}));\n    }}\n}}\n"
    );
    let path = temp_file("over-limit.txt", &source);

    let output = run(&["compile", path.to_str().unwrap(), "-o", "-"]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Set Global Variable"),
        "the artifact still reaches stdout: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("warning[target-element-limit]"),
        "the limit warning reaches stderr: {stderr}"
    );
    assert!(stderr.contains("32768"), "{stderr}");

    let out_path = path.with_extension("ow");
    let json = run(&[
        "compile",
        path.to_str().unwrap(),
        "-o",
        out_path.to_str().unwrap(),
        "-f",
        "json",
    ]);
    assert!(json.status.success(), "{}", command_result(&json));
    assert!(
        json.stderr.is_empty(),
        "JSON mode keeps stderr clean: {}",
        String::from_utf8_lossy(&json.stderr)
    );
    let envelope = parse_json(&json.stdout);
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["exit"], 0);
    assert!(
        std::fs::read_to_string(&out_path)
            .unwrap()
            .contains("Set Global Variable"),
        "the artifact still reaches the file"
    );
    let warning = envelope["diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .iter()
        .find(|d| d["code"] == "target-element-limit")
        .expect("the envelope reports the limit warning");
    assert_eq!(warning["severity"], "warning");
    assert_eq!(warning["stage"], "emission");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

fn run_in(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(wright())
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .env_remove("CI")
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITHUB_STEP_SUMMARY")
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR")
        .output()
        .expect("wright runs")
}

fn sha256_of(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

/// ADR-0025: `compile` writes `<entry stem>.ow` in the current directory and
/// reports only the verdict, the path and the digest; no surface carries the
/// program text.
#[test]
fn compile_writes_the_default_file_and_reports_status_only() {
    let dir = temp_dir();
    std::fs::write(dir.join("mode.ws"), corpus_workshop("synthetic/basic-rule")).unwrap();

    let text = run_in(&dir, &["compile", "mode.ws"]);
    assert_eq!(text.status.code(), Some(0), "{}", command_result(&text));
    let written = dir.join("mode.ow");
    assert!(
        std::fs::read_to_string(&written)
            .unwrap()
            .contains("Disable Inspector Recording")
    );
    let stdout = String::from_utf8_lossy(&text.stdout);
    assert!(stdout.contains("PASS compile"), "{stdout}");
    assert!(stdout.contains("wrote mode.ow"), "{stdout}");
    assert!(stdout.contains(&sha256_of(&written)), "{stdout}");
    assert!(!stdout.contains("Disable Inspector Recording"), "{stdout}");

    let json = run_in(&dir, &["compile", "mode.ws", "--force", "-f", "json"]);
    assert_eq!(json.status.code(), Some(0), "{}", command_result(&json));
    assert!(json.stderr.is_empty(), "JSON mode keeps stderr clean");
    let envelope = parse_json(&json.stdout);
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["wright"]["contract"], "wright-result/v1");
    let output = &envelope["result"]["output"];
    assert_eq!(output["written_to"], "mode.ow");
    assert_eq!(output["sha256"], sha256_of(&written));
    assert!(output.get("text").is_none(), "no program text: {output}");
    assert!(
        !String::from_utf8_lossy(&json.stdout).contains("Disable Inspector Recording"),
        "no program text anywhere in the envelope"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compile_o_path_writes_there_and_failure_writes_nothing() {
    let dir = temp_dir();
    std::fs::write(dir.join("mode.ws"), corpus_workshop("synthetic/basic-rule")).unwrap();
    let output = run_in(&dir, &["compile", "mode.ws", "-o", "elsewhere.txt"]);
    assert_eq!(output.status.code(), Some(0), "{}", command_result(&output));
    assert!(dir.join("elsewhere.txt").exists());
    assert!(!dir.join("mode.ow").exists());

    std::fs::write(
        dir.join("broken.ws"),
        "rule (\"x\") { event { Ongoing - Global; }",
    )
    .unwrap();
    let failed = run_in(&dir, &["compile", "broken.ws"]);
    assert_eq!(failed.status.code(), Some(1), "{}", command_result(&failed));
    assert!(
        !dir.join("broken.ow").exists(),
        "a failed compile writes no file"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compile_refuses_an_existing_file_without_a_terminal_and_force_replaces_it() {
    let dir = temp_dir();
    std::fs::write(dir.join("mode.ws"), corpus_workshop("synthetic/basic-rule")).unwrap();
    std::fs::write(dir.join("mode.ow"), "keep me").unwrap();

    let refused = run_in(&dir, &["compile", "mode.ws"]);
    assert_eq!(
        refused.status.code(),
        Some(2),
        "{}",
        command_result(&refused)
    );
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("--force"),
        "{}",
        command_result(&refused)
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("mode.ow")).unwrap(),
        "keep me"
    );

    let forced = run_in(&dir, &["compile", "mode.ws", "--force"]);
    assert_eq!(forced.status.code(), Some(0), "{}", command_result(&forced));
    assert!(
        std::fs::read_to_string(dir.join("mode.ow"))
            .unwrap()
            .contains("Disable Inspector Recording")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compile_never_overwrites_its_input() {
    let dir = temp_dir();
    let source = corpus_workshop("synthetic/basic-rule");
    std::fs::write(dir.join("mode.ws"), &source).unwrap();
    for extra in [&[][..], &["--force"][..]] {
        for target in [
            "mode.ws",
            "./mode.ws",
            dir.join("mode.ws").to_str().unwrap(),
        ] {
            let mut args = vec!["compile", "mode.ws", "-o", target];
            args.extend_from_slice(extra);
            let output = run_in(&dir, &args);
            assert_eq!(output.status.code(), Some(2), "{}", command_result(&output));
            assert_eq!(
                std::fs::read_to_string(dir.join("mode.ws")).unwrap(),
                source
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compile_o_dash_prints_only_the_program() {
    let dir = temp_dir();
    std::fs::write(dir.join("mode.ws"), corpus_workshop("synthetic/basic-rule")).unwrap();
    let output = run_in(&dir, &["compile", "mode.ws", "-o", "-"]);
    assert_eq!(output.status.code(), Some(0), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("rule ("), "{stdout}");
    assert!(!stdout.contains("PASS") && !stdout.contains("sha256"));
    assert!(output.stderr.is_empty());
    assert!(!dir.join("mode.ow").exists());

    let json = run_in(&dir, &["compile", "mode.ws", "-o", "-", "-f", "json"]);
    assert_eq!(json.status.code(), Some(2), "{}", command_result(&json));
    let _ = std::fs::remove_dir_all(&dir);
}

/// wright#759: `--to` selects the Workshop output locale independently of
/// `--locale` (the input override) — a zh-CN program re-emits as en-US and
/// the envelope reports the emitted locale, not the input's.
#[test]
fn compile_to_emits_the_target_locale_independently_of_the_input() {
    let dir = temp_dir();
    // A program whose locale evidence is unambiguously zh-CN (structural
    // keywords carry no mapping ambiguity between these two locales).
    let zh = "变量 {\n    全局:\n        0: index\n}\n\n规则 (\"first\") {\n    事件 {\n        持续 - 全局;\n    }\n    动作 {\n        设置全局变量(index, 1);\n    }\n}\n";
    std::fs::write(dir.join("mode.ws"), zh).unwrap();

    let output = run_in(
        &dir,
        &["compile", "mode.ws", "--to", "en-US", "-o", "en.ow"],
    );
    assert_eq!(output.status.code(), Some(0), "{}", command_result(&output));
    let text = std::fs::read_to_string(dir.join("en.ow")).unwrap();
    assert!(text.contains("variables"), "en-US output: {text}");
    assert!(text.contains("Ongoing - Global"), "{text}");
    assert!(!text.contains('规'), "no zh-CN spellings remain: {text}");

    let json = run_in(
        &dir,
        &[
            "compile", "mode.ws", "--to", "en-US", "-o", "en2.ow", "--force", "-f", "json",
        ],
    );
    let envelope = parse_json(&json.stdout);
    assert_eq!(envelope["result"]["output"]["locale"], "en-us");

    // `--locale` stays the input override: telling Wright the zh-CN input
    // is en-US still fails like it did before `--to` existed.
    let wrong = run_in(
        &dir,
        &[
            "compile", "mode.ws", "--locale", "en-US", "--to", "zh-CN", "-o", "-",
        ],
    );
    assert_eq!(wrong.status.code(), Some(1), "{}", command_result(&wrong));
    let _ = std::fs::remove_dir_all(&dir);
}

/// wright#759: an undeclared `--to` locale is a usage error naming the
/// declared locales, and a declared locale missing mappings for the
/// program fails `missing-mapping` with no partial output file.
#[test]
fn compile_to_rejects_unknown_and_unmapped_target_locales() {
    let dir = temp_dir();
    std::fs::write(dir.join("mode.ws"), corpus_workshop("synthetic/basic-rule")).unwrap();

    let unknown = run_in(
        &dir,
        &["compile", "mode.ws", "--to", "xx-YY", "-o", "out.ow"],
    );
    assert_eq!(
        unknown.status.code(),
        Some(2),
        "{}",
        command_result(&unknown)
    );
    assert!(
        String::from_utf8_lossy(&unknown.stderr).contains("invalid-locale"),
        "{}",
        command_result(&unknown)
    );
    assert!(!dir.join("out.ow").exists());

    // `EventTeam.ALL` (used by `declarations-rules`) has no tr-TR mapping.
    std::fs::write(
        dir.join("teams.ws"),
        corpus_workshop("synthetic/declarations-rules"),
    )
    .unwrap();
    let missing = run_in(
        &dir,
        &["compile", "teams.ws", "--to", "tr-TR", "-o", "teams.ow"],
    );
    assert_eq!(
        missing.status.code(),
        Some(1),
        "{}",
        command_result(&missing)
    );
    let stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(stderr.contains("missing-mapping"), "{stderr}");
    assert!(
        stderr.contains("tr-tr"),
        "the target locale is named: {stderr}"
    );
    assert!(
        !dir.join("teams.ow").exists(),
        "a missing mapping writes no partial file"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn check_over_clean_input_exits_zero() {
    let path = temp_file("basic.txt", &corpus_workshop("synthetic/basic-rule"));
    let output = run(&["check", path.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("PASS check"));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn terminal_renderer_uses_command_specific_hierarchy() {
    let path = temp_file("flow.txt", &corpus_workshop("synthetic/control-flow"));
    for (command, heading, detail) in [
        ("check", "PASS check", "diagnostic(s)"),
        ("lint", "WARN lint", "Lint findings"),
        ("analyze", "WARN analyze", "Program overview"),
        ("inspect", "PASS inspect", "Detail commands"),
    ] {
        let output = run(&[
            command,
            path.to_str().unwrap(),
            "--renderer",
            "terminal",
            "--color",
            "never",
        ]);
        assert!(output.status.code().is_some(), "{command} exited by signal");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains(heading), "{command}: {stdout}");
        assert!(stdout.contains(detail), "{command}: {stdout}");
    }
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn analyze_real_project_report_is_bounded_and_ranked() {
    let path = temp_file(
        "pixelart.txt",
        &corpus_workshop("real-world/overpy-pixelart"),
    );
    let output = run(&[
        "analyze",
        path.to_str().unwrap(),
        "--renderer",
        "terminal",
        "--color",
        "never",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Program overview"));
    assert!(stdout.contains("Workshop cost"));
    assert!(stdout.contains("element(s) [exact]"));
    assert!(stdout.contains("Hotspots"));
    assert!(stdout.contains("Complexity"));
    assert!(stdout.contains("Performance and stability risks"));
    assert!(stdout.contains("State and coupling"));
    assert!(stdout.contains("[static]"));
    assert!(
        stdout.lines().count() <= 40,
        "report is not bounded:\n{stdout}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn analyze_low_cost_program_reports_cost_without_risks() {
    let path = temp_file("basic.txt", &corpus_workshop("synthetic/basic-rule"));
    let output = run(&[
        "analyze",
        path.to_str().unwrap(),
        "--renderer",
        "terminal",
        "--color",
        "never",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Workshop cost"), "{stdout}");
    assert!(stdout.contains("[exact]"), "{stdout}");
    assert!(
        stdout.contains("Performance and stability risks"),
        "{stdout}"
    );
    let risks = stdout
        .split("Performance and stability risks")
        .nth(1)
        .expect("risks section");
    let risks = risks.split("State and coupling").next().unwrap();
    assert!(risks.contains("none"), "{stdout}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn check_over_malformed_input_exits_one_with_structured_diagnostics() {
    // Enough locale evidence to pass detection, then a syntax error.
    let path = temp_file(
        "broken.txt",
        "rule (\"x\") { event { Ongoing - Global; } actions { If(True); }",
    );
    let output = run(&["check", path.to_str().unwrap(), "-f", "json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty(), "JSON mode: no stderr");
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["exit"], 1);
    let diagnostic = &envelope["diagnostics"][0];
    assert!(diagnostic["code"].is_string());
    assert_eq!(diagnostic["severity"], "error");
    assert!(diagnostic["span"].is_object(), "diagnostics carry spans");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn check_excludes_configurable_lint_findings() {
    let path = temp_file("flow.txt", &corpus_workshop("synthetic/control-flow"));
    let output = run(&["check", path.to_str().unwrap(), "-f", "json"]);
    assert_eq!(output.status.code(), Some(0), "warnings do not fail check");
    let envelope = parse_json(&output.stdout);
    assert!(
        envelope["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .all(|diagnostic| diagnostic["code"] != "min-wait-loop")
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

// Two `raw-setting` warnings plus one `unknown-value` error exercise the
// blocking/non-blocking split and the errors-first ordering at once.

const MIXED_CHECK_SOURCE: &str = r#"settings {
    customGroup {
        mySetting: 1
        otherSetting: 2
    }
}
rule ("x") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(A, 1);
        Set Global Variable(B, FutureValueThing(2));
    }
}
"#;

#[test]
fn check_text_leads_with_verdict_and_blocking_count() {
    let path = temp_file("mixed.txt", MIXED_CHECK_SOURCE);
    let output = run(&[
        "check",
        path.to_str().unwrap(),
        "--renderer",
        "plain",
        "--color",
        "never",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    assert_eq!(lines.next(), Some("ERROR check"), "{stdout}");
    let counts = lines.next().unwrap_or_default().to_string();
    assert!(counts.contains("1 error(s)"), "{stdout}");
    assert!(counts.contains("2 warning(s)"), "{stdout}");
    assert_eq!(lines.next(), Some("  1 file(s) affected"), "{stdout}");
    assert!(lines.next().is_none(), "the footer stays compact: {stdout}");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let error_at = stderr.find("error[").expect("an error diagnostic");
    let warning_at = stderr.find("warning[").expect("a warning diagnostic");
    assert!(
        error_at < warning_at,
        "blocking errors precede warnings: {stderr}"
    );
    assert!(stderr.contains(" --> "), "a mapped location: {stderr}");
    assert!(
        stderr.contains("Set Global Variable(B, FutureValueThing(2));"),
        "the source context sits with the diagnostic: {stderr}"
    );
    assert!(
        stderr.contains("= analysis"),
        "stage stays secondary: {stderr}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn terminal_renderer_marks_independent_diagnostics() {
    // The terminal layout hangs the whole report on one rail from the
    // verdict to the footer (#718); the reported set, order, and content
    // are unchanged.
    let path = temp_file("mixed.txt", MIXED_CHECK_SOURCE);
    let output = run(&[
        "check",
        path.to_str().unwrap(),
        "--renderer",
        "terminal",
        "--color",
        "never",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("├─ error["), "{stderr}");
    assert!(stderr.contains("├─ warning["), "{stderr}");
    assert!(
        stderr.contains("│  --> ") && stderr.contains("│  = "),
        "locations and secondary metadata hang on the rail: {stderr}"
    );
    assert!(
        stderr.contains("│\n├─ warning["),
        "a blank rail line separates independent diagnostics: {stderr}"
    );
    assert!(
        stderr.contains("Set Global Variable(B, FutureValueThing(2));"),
        "the source context renders inside the rail: {stderr}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("╭─ ERROR check\n│  "), "{stdout}");
    assert!(
        stdout
            .trim_end()
            .rsplit('\n')
            .next()
            .unwrap()
            .starts_with("╰─ "),
        "the footer closes the rail: {stdout}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn plain_renderer_keeps_the_undecorated_diagnostic_layout() {
    // `plain` is the deterministic, undecorated contract (#703): no box
    // connectors, same lines as before the terminal refinement.
    let path = temp_file("mixed.txt", MIXED_CHECK_SOURCE);
    let output = run(&[
        "check",
        path.to_str().unwrap(),
        "--renderer",
        "plain",
        "--color",
        "never",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains('├') && !stderr.contains('╰'),
        "plain stays undecorated: {stderr}"
    );
    assert!(stderr.contains("error["), "{stderr}");
    assert!(stderr.contains("\n  --> "), "{stderr}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn terminal_renderer_marks_lint_findings_and_keeps_compact_results_flat() {
    let path = temp_file("flow.txt", &corpus_workshop("synthetic/control-flow"));
    let output = run(&[
        "lint",
        path.to_str().unwrap(),
        "--renderer",
        "terminal",
        "--color",
        "never",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("╭─ WARN lint\n"), "{stdout}");
    assert!(
        stdout.contains("├─ warning["),
        "the finding block hangs on the rail: {stdout}"
    );
    assert!(
        stdout.contains("│  --> "),
        "the location hangs under the finding: {stdout}"
    );
    assert!(
        stdout
            .trim_end()
            .rsplit('\n')
            .next()
            .unwrap()
            .starts_with("╰─ "),
        "the footer closes the rail: {stdout}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());

    // A bare verdict with nothing to group stays flat — no decorative rail
    // is forced onto compact output.
    let path = temp_file("basic.txt", &corpus_workshop("synthetic/basic-rule"));
    let output = run(&[
        "check",
        path.to_str().unwrap(),
        "--renderer",
        "terminal",
        "--color",
        "never",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "PASS check\n  0 diagnostic(s)\n", "{stdout}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn only_the_terminal_renderer_draws_the_rail() {
    // The rail is a terminal detail (#718): plain, CI, GitHub Actions, and
    // JSON keep their undecorated layout for a shape the terminal frames.
    let path = temp_file("flow.txt", &corpus_workshop("synthetic/control-flow"));
    let path = path.to_str().unwrap();
    let terminal = run(&["lint", path, "--renderer", "terminal", "--color", "never"]);
    let terminal = String::from_utf8_lossy(&terminal.stdout).into_owned();
    assert!(terminal.contains('╭'), "{terminal}");

    type Env<'a> = &'a [(&'a str, &'a str)];
    let undecorated: [(&[&str], Env); 5] = [
        (&["--renderer", "plain"], &[]),
        (&[], &[]),
        (&[], &[("CI", "true")]),
        (&[], &[("GITHUB_ACTIONS", "true")]),
        (&["--format", "json"], &[]),
    ];
    for (flags, env) in undecorated {
        let mut args = vec!["lint", path];
        args.extend_from_slice(flags);
        let output = run_with_env(&args, env);
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !text.contains(['╭', '├', '╰', '│', '\u{1}', '\u{2}', '\u{3}']),
            "{flags:?} {env:?} is decorated: {text}"
        );
    }
    let _ = std::fs::remove_dir_all(std::path::Path::new(path).parent().unwrap());
}

#[test]
fn terminal_renderer_never_leaks_rail_markers() {
    // An artifact-stream command prints its diagnostics outside the framed
    // report (#718); no internal marker byte may reach either stream, for
    // any decorated shape.
    let path = temp_file("flow.txt", &corpus_workshop("synthetic/control-flow"));
    let path = path.to_str().unwrap();
    let invocations: [&[&str]; 4] = [
        &["convert", "--target", "opy", path],
        &["check", path],
        &["lint", path],
        &["analyze", path],
    ];
    for args in invocations {
        let mut args = args.to_vec();
        args.extend(["--renderer", "terminal", "--color", "never"]);
        let output = run(&args);
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !text.contains(['\u{1}', '\u{2}', '\u{3}']),
            "{args:?} leaked a rail marker: {text:?}"
        );
    }
    let _ = std::fs::remove_dir_all(std::path::Path::new(path).parent().unwrap());
}

#[test]
fn terminal_renderer_marks_analyze_risks_and_cfg_blocks() {
    let path = temp_file("flow.txt", &corpus_workshop("synthetic/control-flow"));
    let output = run(&[
        "analyze",
        path.to_str().unwrap(),
        "--renderer",
        "terminal",
        "--color",
        "never",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("├─ warning[min-wait-loop]"),
        "the risk item hangs on the rail inside its section: {stdout}"
    );
    assert!(stdout.ends_with("╰─\n"), "{stdout}");

    let output = run(&[
        "inspect",
        "cfg",
        "bounded while",
        path.to_str().unwrap(),
        "--renderer",
        "terminal",
        "--color",
        "never",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("╭─ PASS cfg\n"), "{stdout}");
    assert!(stdout.contains("├─ block "), "{stdout}");
    assert!(
        stdout.contains("│  -> "),
        "successors hang under their block: {stdout}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn check_unmapped_locations_are_not_fabricated() {
    // `<stdin>` is a pseudo-path: the position is honest metadata, never a
    // file-style `-->` location.
    let output = run_with_stdin(
        &["check", "-", "--renderer", "plain", "--color", "never"],
        "rule (\"x\") { event { Ongoing - Global; } actions { If(True); }",
    );
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("--> <stdin>"), "{stderr}");
    assert!(
        stderr.contains("= at <stdin>:1:63"),
        "the unmapped position stays secondary metadata: {stderr}"
    );
}

#[test]
fn check_success_renders_a_compact_deterministic_summary() {
    let path = temp_file("basic.txt", &corpus_workshop("synthetic/basic-rule"));
    let output = run(&[
        "check",
        path.to_str().unwrap(),
        "--renderer",
        "plain",
        "--color",
        "never",
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout, "PASS check\n  0 diagnostic(s)\n",
        "a clean check is a two-line summary on the deterministic plain surface"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn check_json_prioritizes_blocking_diagnostics_like_text() {
    let path = temp_file("mixed.txt", MIXED_CHECK_SOURCE);
    let output = run(&["check", path.to_str().unwrap(), "-f", "json"]);
    assert_eq!(output.status.code(), Some(1));
    let envelope = parse_json(&output.stdout);
    let severities: Vec<&str> = envelope["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| diagnostic["severity"].as_str().unwrap())
        .collect();
    assert_eq!(severities, ["error", "warning", "warning"]);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn check_tty_clears_progress_then_renders_verdict_and_elapsed() {
    let path = temp_file("flow.txt", &corpus_workshop("synthetic/control-flow"));
    let output = run_in_tty(&[
        "check",
        path.to_str().unwrap(),
        "--renderer",
        "terminal",
        "--color",
        "never",
    ]);
    assert!(output.status.success());
    let mut transcript = output.stdout;
    transcript.extend_from_slice(&output.stderr);
    let transcript = String::from_utf8_lossy(&transcript);
    let verdict = transcript
        .find("PASS check")
        .expect("the final verdict renders");
    assert!(
        transcript[..verdict].contains("\x1b[2K"),
        "transient progress is cleared before the verdict: {transcript}"
    );
    assert!(
        !transcript[verdict..].contains("Resolving input"),
        "progress labels never compete with the verdict: {transcript}"
    );
    assert!(
        transcript[verdict..].contains(" ms"),
        "the interactive footer carries elapsed time: {transcript}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn analyze_over_workshop_input_reports_semantic_facts() {
    let path = temp_file("flow.txt", &corpus_workshop("synthetic/control-flow"));
    let output = run(&["analyze", path.to_str().unwrap(), "-f", "json"]);
    assert!(output.status.success());
    let envelope = parse_json(&output.stdout);
    assert!(envelope["result"]["program"]["findings"].is_null());
    assert!(
        !envelope["result"]["variables"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !envelope["result"]["hotspots"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        envelope["result"]["persistentObjects"].is_object(),
        "analyze reports persistent-object facts (#429)"
    );
    let cost = &envelope["result"]["cost"];
    assert!(
        cost["elementCount"].as_u64().is_some_and(|total| total > 0),
        "analyze reports the canonical element count (#445): {cost}"
    );
    assert!(
        cost["counts"]["actions"].as_u64().is_some(),
        "analyze reports structural counts (#445): {cost}"
    );
    let rule = &envelope["result"]["hotspots"][0];
    assert!(
        rule["elements"].as_u64().is_some(),
        "analyze attributes element cost to rules (#445): {rule}"
    );
    assert!(
        envelope["result"]["risks"].is_array(),
        "analyze reports performance/stability risk facts (#445)"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn inspect_over_workshop_input_lists_rules_and_symbols() {
    let path = temp_file("decl.txt", &corpus_workshop("synthetic/declarations-rules"));
    let output = run(&["inspect", path.to_str().unwrap(), "-f", "json"]);
    assert!(output.status.success());
    let envelope = parse_json(&output.stdout);
    assert!(!envelope["result"]["rules"].as_array().unwrap().is_empty());
    assert!(envelope["result"]["symbols"].is_null());
    assert!(envelope["result"]["counts"]["symbols"].as_u64().unwrap() > 0);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn lint_over_workshop_input_reports_findings_in_text_and_json() {
    let path = temp_file("flow.txt", &corpus_workshop("synthetic/control-flow"));
    let output = run(&["lint", path.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("WARN lint"), "summary line: {stdout}");
    assert!(stdout.contains("min-wait-loop"), "findings: {stdout}");
    assert!(
        stdout.contains("evidence:"),
        "text mode exposes the evidence class: {stdout}"
    );
    let output = run(&["lint", path.to_str().unwrap(), "-f", "json"]);
    assert!(output.status.success());
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["command"], "lint");
    assert_eq!(envelope["ok"], true);
    let findings = envelope["result"]["findings"].as_array().unwrap();
    assert!(!findings.is_empty(), "control-flow produces findings");
    for finding in findings {
        assert!(finding["evidence"].is_string(), "findings carry evidence");
        assert!(
            finding["location"].is_string(),
            "finding spans carry the resolved path"
        );
    }
    assert!(envelope["result"]["rules"].is_null());
    assert!(envelope["result"]["config"].is_null());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

const FIXABLE_WORKSHOP: &str = r#"variables {
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
        Else If(Compare(Global.index, ==, 0));
            Set Global Variable(other, 2);
        End;
        While(Compare(Global.index, <, 3));
            Set Global Variable(other, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            Set Global Variable(index, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            Wait(0.016, Ignore Condition);
        End;
    }
}
"#;

#[test]
fn lint_fix_previews_and_writes_validated_fixes() {
    let path = temp_file("fixable.ws", FIXABLE_WORKSHOP);
    let path = path.to_str().unwrap();

    let output = run(&["lint", path, "--fix", "-f", "json"]);
    assert!(output.status.success(), "{}", command_result(&output));
    let findings = parse_json(&output.stdout)["result"]["findings"]
        .as_array()
        .unwrap()
        .clone();
    for code in ["duplicate-condition", "repeated-value"] {
        let fix = findings
            .iter()
            .find(|finding| finding["code"] == code)
            .and_then(|finding| finding.get("fix"))
            .unwrap_or_else(|| panic!("{code} carries a fix"));
        assert!(
            fix["transaction"]["edits"]
                .as_array()
                .is_some_and(|edits| !edits.is_empty()),
            "the fix carries a source-edit transaction: {fix}"
        );
    }

    let output = run(&["lint", path, "--fix"]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("fix[duplicate-condition]"), "{stdout}");
    assert!(stdout.contains("fix[repeated-value]"), "{stdout}");
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        FIXABLE_WORKSHOP,
        "preview writes nothing"
    );

    let output = run(&["lint", path, "--fix", "--write"]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("applied[duplicate-condition]"), "{stdout}");
    assert!(stdout.contains("applied[repeated-value]"), "{stdout}");
    let applied = std::fs::read_to_string(path).unwrap();
    assert!(
        !applied.contains("Else If(Compare(Global.index, ==, 0));"),
        "the dead branch is gone: {applied}"
    );
    assert_eq!(
        applied
            .matches("Evaluate Once(Distance Between(Position Of(Event Player), Vector(0, 0, 0)))")
            .count(),
        2,
        "each duplicated occurrence wraps: {applied}"
    );
    let output = run(&["check", path]);
    assert!(
        output.status.success(),
        "check passes after the fixes: {}",
        command_result(&output)
    );
    let output = run(&["lint", path, "-f", "json"]);
    let remaining = parse_json(&output.stdout)["result"]["findings"]
        .as_array()
        .unwrap()
        .clone();
    assert!(
        remaining.iter().all(|finding| !matches!(
            finding["code"].as_str(),
            Some("duplicate-condition") | Some("repeated-value")
        )),
        "applied fixes remove their findings: {remaining:?}"
    );
    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

#[test]
fn lint_fix_refuses_stdin_without_rereading_it() {
    // stdin has no reloadable source: fix preview refuses each finding's
    // transaction with `edit-input-stdin` instead of resolving stdin a
    // second time (EOF on a pipe, a block on a terminal), and `--write`
    // refuses the input up front.
    let output = run_with_stdin(&["lint", "-", "--fix", "-f", "json"], FIXABLE_WORKSHOP);
    assert!(output.status.success(), "{}", command_result(&output));
    let envelope = parse_json(&output.stdout);
    let fixes = envelope["result"]["fixes"].as_array().unwrap();
    assert!(!fixes.is_empty(), "fix outcomes are reported: {envelope}");
    for fix in fixes {
        assert_eq!(fix["status"], "refused", "{fix}");
        assert!(
            fix["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| diagnostic["code"] == "edit-input-stdin"),
            "each fix refuses with edit-input-stdin: {fix}"
        );
    }

    let output = run_with_stdin(
        &["lint", "-", "--fix", "--write", "-f", "json"],
        FIXABLE_WORKSHOP,
    );
    assert!(!output.status.success(), "{}", command_result(&output));
    let envelope = parse_json(&output.stdout);
    assert!(
        envelope["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic["code"] == "edit-input-stdin"),
        "write refuses stdin up front: {envelope}"
    );
}

#[test]
fn span_path_is_consistent_across_input_spellings() {
    // Lint resolves the same root-relative `span.path` for the absolute,
    // bare-name (cwd), and dir-relative spellings of the same Workshop file.
    // Each subprocess gets its own cwd, so the bare-name spelling is
    // exercised end-to-end exactly as in the issue.
    let dir = temp_dir();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(
        dir.join("sub").join("loop.txt"),
        corpus_workshop("synthetic/control-flow"),
    )
    .unwrap();

    let absolute = run(&[
        "lint",
        dir.join("sub").join("loop.txt").to_str().unwrap(),
        "-f",
        "json",
    ]);
    assert!(absolute.status.success(), "{}", command_result(&absolute));
    let absolute_path = parse_json(&absolute.stdout)["result"]["findings"][0]["location"]
        .as_str()
        .unwrap()
        .to_string();

    let bare = Command::new(wright())
        .args(["lint", "loop.txt", "-f", "json"])
        .current_dir(dir.join("sub"))
        .stdin(Stdio::null())
        .output()
        .expect("wright runs");
    assert!(bare.status.success(), "{}", command_result(&bare));
    let bare_path = parse_json(&bare.stdout)["result"]["findings"][0]["location"]
        .as_str()
        .unwrap()
        .to_string();

    let relative = Command::new(wright())
        .args(["lint", "sub/loop.txt", "-f", "json"])
        .current_dir(&dir)
        .stdin(Stdio::null())
        .output()
        .expect("wright runs");
    assert!(relative.status.success(), "{}", command_result(&relative));
    let relative_path = parse_json(&relative.stdout)["result"]["findings"][0]["location"]
        .as_str()
        .unwrap()
        .to_string();

    assert!(absolute_path.starts_with("loop.txt:"));
    assert_eq!(
        bare_path, absolute_path,
        "the bare-name (cwd) spelling must agree with the absolute spelling"
    );
    assert_eq!(
        relative_path, absolute_path,
        "the dir-relative spelling must agree with the absolute spelling"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lint_rule_flags_control_findings() {
    let path = temp_file("flow.txt", &corpus_workshop("synthetic/control-flow"));
    let output = run(&[
        "lint",
        path.to_str().unwrap(),
        "--disable-rule",
        "min-wait-loop",
        "-f",
        "json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = parse_json(&output.stdout);
    let findings = envelope["result"]["findings"].as_array().unwrap();
    assert!(
        findings
            .iter()
            .all(|finding| finding["code"] != "min-wait-loop"),
        "the disabled rule must produce no findings"
    );
    // --rule-severity overrides the effective severity of a rule. The
    // control-flow fixture produces no expensive-loop-check findings, so
    // the assertion is on the rules metadata.
    let output = run(&[
        "lint",
        path.to_str().unwrap(),
        "--rule-severity",
        "min-wait-loop:error",
        "-f",
        "json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = parse_json(&output.stdout);
    assert!(
        envelope["result"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["code"] == "min-wait-loop" && finding["severity"] == "error")
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn lint_lists_a_local_yaml_rule_loaded_with_the_rule_flag() {
    let path = temp_file("flow.txt", &corpus_workshop("synthetic/control-flow"));
    let rule_file = temp_file(
        "cli-smoke.yaml",
        "id: community/cli-smoke\nmetadata:\n  summary: summary\n  rationale: rationale\n  documentation: documentation\n  known-limits: limits\n  tags: []\nmatcher: {}\n",
    );
    let output = run(&[
        "lint",
        path.to_str().unwrap(),
        "--rule",
        rule_file.to_str().unwrap(),
        "-f",
        "json",
    ]);
    assert!(output.status.success(), "{}", command_result(&output));
    let envelope = parse_json(&output.stdout);
    assert!(
        envelope["result"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["code"] == "community/cli-smoke")
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
    let _ = std::fs::remove_dir_all(rule_file.parent().unwrap());
}

#[test]
fn lint_flags_are_usage_errors_for_other_commands() {
    for flags in [
        &["check", "--disable-rule", "min-wait-loop"][..],
        &["analyze", "--rule-severity", "min-wait-loop:info"][..],
    ] {
        let output = run(flags);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{flags:?} must be a usage error"
        );
        assert!(output.stdout.is_empty(), "usage errors write stderr only");
    }
}

// `real-world/overpy-cake.ws` produces 10 findings: 9 identical
// `repeated-value` warnings and 1 `min-wait-loop` warning.

#[test]
fn lint_selection_filters_findings_and_reports_withheld() {
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let path = path.to_str().unwrap();

    let output = run(&[
        "lint",
        path,
        "--rule-id",
        "repeated-value",
        "--max",
        "2",
        "-f",
        "json",
    ]);
    assert!(output.status.success(), "{}", command_result(&output));
    let envelope = parse_json(&output.stdout);
    let findings = envelope["result"]["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 2);
    assert!(findings.iter().all(|f| f["code"] == "repeated-value"));
    assert_eq!(envelope["result"]["selection"]["total"], 10);
    assert_eq!(envelope["result"]["selection"]["withheld"], 7);

    // --severity is a threshold: `error` selects none of the warning findings
    // while the envelope records the full set.
    let output = run(&["lint", path, "--severity", "error", "-f", "json"]);
    assert!(output.status.success());
    let envelope = parse_json(&output.stdout);
    assert!(
        envelope["result"]["findings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(envelope["result"]["selection"]["total"], 10);
    assert_eq!(envelope["result"]["selection"]["withheld"], 0);

    // --file selects by file identity: the reported span.path is
    // root-relative, and the absolute input path exactly as passed selects
    // the same file.
    let reported_path =
        parse_json(&run(&["lint", path, "-f", "json"]).stdout)["result"]["findings"][0]["location"]
            .as_str()
            .unwrap()
            .to_string();
    let reported_path = reported_path.rsplitn(3, ':').last().unwrap();
    let output = run(&["lint", path, "--file", reported_path, "-f", "json"]);
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["result"]["findings"].as_array().unwrap().len(), 10);
    let output = run(&["lint", path, "--file", path, "-f", "json"]);
    let envelope = parse_json(&output.stdout);
    assert_eq!(
        envelope["result"]["findings"].as_array().unwrap().len(),
        10,
        "the input path as passed resolves to the reported root-relative span.path"
    );
    let output = run(&["lint", path, "--file", "other.ws", "-f", "json"]);
    let envelope = parse_json(&output.stdout);
    assert!(
        envelope["result"]["findings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(envelope["result"]["selection"]["total"], 10);

    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

#[test]
fn omitted_selection_reproduces_the_unfiltered_envelope() {
    // Omitting every selection option reproduces the current output:
    // complete findings/diagnostics arrays and no `selection` member (#430).
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let output = run(&["lint", path.to_str().unwrap(), "-f", "json"]);
    assert!(output.status.success());
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["result"]["findings"].as_array().unwrap().len(), 10);
    assert!(envelope["result"].get("selection").is_none());
    assert!(envelope.get("selection").is_none());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn selection_never_changes_the_verdict_or_exit_code() {
    // The ablation guard of #430: if exit codes were computed from the
    // selected set instead of the full set, filtering an error out of view
    // would flip a failing check to exit 0.
    let broken = temp_file(
        "broken.txt",
        "rule (\"x\") { event { Ongoing - Global; } actions { If(True); }",
    );
    let baseline = run(&["check", broken.to_str().unwrap(), "-f", "json"]);
    assert_eq!(baseline.status.code(), Some(1));
    let full = parse_json(&baseline.stdout)["diagnostics"]
        .as_array()
        .unwrap()
        .len();
    assert!(full > 0);

    let output = run(&[
        "check",
        broken.to_str().unwrap(),
        "--rule-id",
        "min-wait-loop",
        "-f",
        "json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "selection must not turn a failing project into exit 0"
    );
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["exit"], 1);
    assert!(
        envelope["diagnostics"].as_array().unwrap().is_empty(),
        "the error diagnostic is selected out of the report"
    );
    assert_eq!(
        envelope["selection"]["total"].as_u64().unwrap() as usize,
        full
    );

    let cake = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let output = run(&["lint", cake.to_str().unwrap(), "--severity", "error"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("WARN lint"), "{stdout}");
    assert!(stdout.contains("10 finding(s)"), "{stdout}");

    let _ = std::fs::remove_dir_all(broken.parent().unwrap());
    let _ = std::fs::remove_dir_all(cake.parent().unwrap());
}

#[test]
fn lint_max_reports_the_withheld_count_in_text_and_json() {
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let output = run(&["lint", path.to_str().unwrap(), "--max", "3"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("7 finding(s) withheld"), "{stdout}");
    assert!(stdout.contains("10 finding(s)"), "{stdout}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn essential_reports_bound_lists_and_expose_expansion() {
    let rules = (0..21).map(|i|format!("rule (\"Rule{i}\") {{ event {{ Ongoing - Global; }} actions {{ Wait(1, Ignore Condition); }} }}")).collect::<Vec<_>>().join("\n");
    let path = temp_file("many-rules.ws", &rules);
    let result =
        parse_json(&run(&["inspect", path.to_str().unwrap(), "-f", "json"]).stdout)["result"]
            .clone();
    assert_eq!(result["rules"].as_array().unwrap().len(), 20);
    assert_eq!(result["truncation"]["rules"]["total"], 21);
    assert!(
        result["truncation"]["rules"]["expand"]
            .as_str()
            .unwrap()
            .contains("--all")
    );
    let output = run(&["inspect", path.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("showing 20 of 21"));
    let full = parse_json(&run(&["inspect", path.to_str().unwrap(), "--all", "-f", "json"]).stdout)
        ["result"]
        .clone();
    assert_eq!(full["rules"].as_array().unwrap().len(), 21);
    assert!(full.get("symbols").is_none());
    assert!(
        !run(&["inspect", path.to_str().unwrap(), "--brief"])
            .status
            .success()
    );
    for query in ["rules", "symbols"] {
        let json = run(&["inspect", query, path.to_str().unwrap(), "-f", "json"]);
        let value = parse_json(&json.stdout)["result"].clone();
        let list = value
            .as_array()
            .or_else(|| value[query].as_array())
            .or_else(|| value["items"].as_array())
            .unwrap();
        assert_eq!(list.len(), 20);
        assert!(
            value["truncation"]
                .as_object()
                .unwrap()
                .values()
                .all(|note| note["total"] == 21
                    && note["expand"].as_str().unwrap().contains("--all"))
        );
        let text = run(&["inspect", query, path.to_str().unwrap()]);
        assert!(String::from_utf8_lossy(&text.stdout).contains("showing 20 of 21"));
        let all = parse_json(
            &run(&[
                "inspect",
                query,
                path.to_str().unwrap(),
                "--all",
                "-f",
                "json",
            ])
            .stdout,
        )["result"]
            .clone();
        let list = all
            .as_array()
            .or_else(|| all[query].as_array())
            .or_else(|| all["items"].as_array())
            .unwrap();
        assert_eq!(list.len(), 21);
    }
    let content = parse_json(
        &run(&[
            "inspect",
            "rules",
            path.to_str().unwrap(),
            "--content",
            "--all",
            "-f",
            "json",
        ])
        .stdout,
    )["result"]
        .clone();
    assert_eq!(content["rules"].as_array().unwrap().len(), 21);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
/// A program whose only risk is the wait-less `While(True)` warning — the
/// smallest input that separates the verdict paths #635 joined.
const WAITLESS_LOOP: &str = "rule (\"r\") {
    event {
        Ongoing - Global;
    }
    actions {
        While(True);
            Set Global Variable(A, 1);
        End;
    }
}
";

#[test]
fn analyze_default_and_all_report_the_same_verdict() {
    let path = temp_file("waitless.txt", WAITLESS_LOOP);
    for args in [
        vec!["analyze", path.to_str().unwrap()],
        vec!["analyze", path.to_str().unwrap(), "--all"],
    ] {
        let output = run(&args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("WARN analyze"), "{args:?}: {stdout}");
    }

    // The same parity holds under a selection: `lint --severity error`
    // reports an empty selected set but the full set's WARN verdict, and the
    // expanded form must agree rather than judging the filtered counts.
    let lint = run(&["lint", path.to_str().unwrap(), "--severity", "error"]);
    let lint_stdout = String::from_utf8_lossy(&lint.stdout);
    assert!(lint_stdout.contains("WARN lint"), "{lint_stdout}");
    let expanded = run(&[
        "lint",
        path.to_str().unwrap(),
        "--severity",
        "error",
        "--all",
    ]);
    let expanded_stdout = String::from_utf8_lossy(&expanded.stdout);
    assert!(expanded_stdout.contains("WARN lint"), "{expanded_stdout}");

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn analyze_selection_narrows_facts_risks() {
    // `severity`/`rule`/`file`/`max` narrow `facts.risks` and the JSON
    // `selection` reports the true total — never a 0/0 beside non-empty
    // risks.
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));

    let output = run(&[
        "analyze",
        path.to_str().unwrap(),
        "--rule-id",
        "repeated-value",
        "--max",
        "2",
        "-f",
        "json",
    ]);
    assert!(output.status.success());
    let envelope = parse_json(&output.stdout);
    let risks = envelope["result"]["risks"].as_array().unwrap();
    assert_eq!(risks.len(), 2);
    assert!(risks.iter().all(|risk| risk["code"] == "repeated-value"));
    assert_eq!(envelope["result"]["selection"]["total"], 10);
    assert_eq!(envelope["result"]["selection"]["withheld"], 7);

    let output = run(&[
        "analyze",
        path.to_str().unwrap(),
        "--severity",
        "error",
        "-f",
        "json",
    ]);
    let envelope = parse_json(&output.stdout);
    assert!(envelope["result"]["risks"].as_array().unwrap().is_empty());
    assert_eq!(envelope["result"]["selection"]["total"], 10);

    let output = run(&["analyze", path.to_str().unwrap(), "--severity", "error"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("WARN analyze"), "{stdout}");
    let output = run(&["analyze", path.to_str().unwrap(), "--max", "2"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("8 finding(s) withheld"), "{stdout}");

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn inspect_json_resolves_span_paths() {
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let output = run(&["inspect", "rules", path.to_str().unwrap(), "-f", "json"]);
    assert!(output.status.success());
    let result = parse_json(&output.stdout)["result"].clone();

    fn collect(value: &serde_json::Value, spans: &mut Vec<serde_json::Value>) {
        match value {
            serde_json::Value::Object(object) => {
                if let Some(span) = object.get("span") {
                    if span.is_object() {
                        spans.push(span.clone());
                    }
                }
                object.values().for_each(|value| collect(value, spans));
            }
            serde_json::Value::Array(items) => {
                items.iter().for_each(|value| collect(value, spans));
            }
            _ => {}
        }
    }
    let mut spans = Vec::new();
    collect(&result, &mut spans);
    assert!(!spans.is_empty(), "the fixture carries spans");
    assert!(
        spans
            .iter()
            .all(|span| span["path"].as_str().is_some_and(|p| !p.is_empty())),
        "every span carries a resolved path: {spans:?}"
    );

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn inspect_cost_verdict_and_file_selection() {
    // Cost findings carry severities, so `inspect cost` judges them like
    // lint findings — the wait-less loop's warning yields WARN.
    let path = temp_file("waitless.txt", WAITLESS_LOOP);
    let output = run(&["inspect", "cost", path.to_str().unwrap()]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("WARN cost"), "{stdout}");

    // Cost findings carry resolved source locations (#722), so `--file`
    // narrows the reported set like `lint` — a matching file keeps its
    // finding and a non-matching one reports an empty set.
    let output = run(&[
        "inspect",
        "cost",
        path.to_str().unwrap(),
        "--file",
        "waitless.txt",
        "-f",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(0));
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["result"]["findings"].as_array().unwrap().len(), 1);
    assert_eq!(envelope["result"]["selection"]["total"], 1);

    let output = run(&[
        "inspect",
        "cost",
        path.to_str().unwrap(),
        "--file",
        "other-file.txt",
        "-f",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(0));
    let envelope = parse_json(&output.stdout);
    assert!(
        envelope["result"]["findings"]
            .as_array()
            .unwrap()
            .is_empty(),
        "a non-matching file empties the reported set: {}",
        envelope["result"]
    );

    let output = run(&[
        "inspect",
        "cost",
        path.to_str().unwrap(),
        "--severity",
        "error",
        "-f",
        "json",
    ]);
    let envelope = parse_json(&output.stdout);
    assert!(
        envelope["result"]["findings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(envelope["result"]["selection"]["total"], 1);
    let output = run(&[
        "inspect",
        "cost",
        path.to_str().unwrap(),
        "--severity",
        "error",
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("WARN cost"), "{stdout}");

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn analyze_github_renderer_emits_risk_annotations() {
    let path = temp_file("waitless.txt", WAITLESS_LOOP);
    let output = run_with_env(
        &[
            "analyze",
            path.to_str().unwrap(),
            "--renderer",
            "github-actions",
        ],
        &[("GITHUB_ACTIONS", "true")],
    );
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("::warning"), "{stderr}");
    assert!(stderr.contains("title=while-without-wait"), "{stderr}");
    assert!(stderr.contains("WARN analyze"), "{stderr}");

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn lint_text_collapses_identical_findings_into_one_entry() {
    // Two loops duplicating the same expression raise two findings that share
    // their rule id and message (#735 names the expression); they collapse into
    // one entry listing both locations.
    let path = temp_file(
        "collapse.txt",
        "rule (\"r\") {
    event {
        Ongoing - Global;
    }
    actions {
        While(True);
            Set Global Variable(A, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            Set Global Variable(B, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            Wait(0.016, Ignore Condition);
        End;
        While(True);
            Set Global Variable(A, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            Set Global Variable(B, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            Wait(0.016, Ignore Condition);
        End;
    }
}
",
    );
    let output = run(&["lint", path.to_str().unwrap()]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("4 warning(s)"), "{stdout}");
    assert_eq!(
        stdout.matches("[repeated-value]").count(),
        1,
        "repeated findings collapse into one entry: {stdout}"
    );
    assert!(
        stdout.contains(
            "warning[repeated-value] (2 findings): `distance(getPosition(eventPlayer), vector(0, 0, 0))` is evaluated 2 times"
        ),
        "{stdout}"
    );
    assert!(
        stdout.contains("warning[min-wait-loop] (2 findings)"),
        "{stdout}"
    );
    assert_eq!(
        stdout.matches(" --> ").count(),
        8,
        "two grouped locations per warning entry plus four expensive-loop-check locations: {stdout}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn lint_text_leads_with_the_verdict_and_orders_by_action_priority() {
    // One rule raised to error severity produces a mixed-severity result:
    // the verdict leads, the error entry renders before warning entries, and
    // the complete result set is unchanged.
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let output = run(&[
        "lint",
        path.to_str().unwrap(),
        "--rule-severity",
        "min-wait-loop:error",
        "--renderer",
        "terminal",
        "--color",
        "never",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let verdict = stdout.find("ERROR lint").expect("verdict: {stdout}");
    let counts = stdout
        .find("1 error(s), 9 warning(s)")
        .expect("severity counts: {stdout}");
    let findings = stdout.find("Lint findings").expect("section: {stdout}");
    let error = stdout
        .find("error[min-wait-loop]")
        .expect("error entry: {stdout}");
    let warning = stdout
        .find("warning[repeated-value]")
        .expect("warning entry: {stdout}");
    assert!(verdict < counts && counts < findings, "{stdout}");
    assert!(findings < error && error < warning, "{stdout}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn lint_text_orders_all_severity_bands_by_action_priority() {
    // An input yielding error, warning, and info findings renders the bands
    // in action order regardless of the driver's reported order.
    let path = temp_file(
        "mixed.txt",
        "variables {\n\
         \tglobal:\n\
         \t\t0: index\n\
         }\n\
         rule (\"hot loop\") {\n\
         \tevent {\n\
         \t\tOngoing - Global;\n\
         \t}\n\
         \tconditions {\n\
         \t\tDistance Between(Vector(0, 0, 0), Vector(1, 1, 1)) < 100;\n\
         \t}\n\
         \tactions {\n\
         \t\tWhile(Compare(Global.index, <, 10));\n\
         \t\t\tWait(0.016, Ignore Condition);\n\
         \t\t\tModify Global Variable(index, Add, 1);\n\
         \t\tEnd;\n\
         \t}\n\
         }\n\
         rule (\"no wait\") {\n\
         \tevent {\n\
         \t\tOngoing - Global;\n\
         \t}\n\
         \tactions {\n\
         \t\tWhile(True);\n\
         \t\t\tModify Global Variable(index, Add, 1);\n\
         \t\tEnd;\n\
         \t}\n\
         }\n",
    );
    let output = run(&[
        "lint",
        path.to_str().unwrap(),
        "--rule-severity",
        "min-wait-loop:error",
        "--renderer",
        "terminal",
        "--color",
        "never",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("1 error(s), 1 warning(s), 1 info finding(s)"),
        "{stdout}"
    );
    let error = stdout
        .find("error[min-wait-loop]")
        .expect("error entry: {stdout}");
    let warning = stdout
        .find("warning[while-without-wait]")
        .expect("warning entry: {stdout}");
    let info = stdout
        .find("info[ongoing-condition-hot-path]")
        .expect("info entry: {stdout}");
    assert!(error < warning && warning < info, "{stdout}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn lint_text_presents_a_finding_with_location_context_and_secondary_metadata() {
    let path = temp_file("flow.txt", &corpus_workshop("synthetic/control-flow"));
    let output = run(&["lint", path.to_str().unwrap()]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("WARN lint"), "{stdout}");
    assert!(stdout.contains("1 warning(s)"), "{stdout}");
    let message = stdout
        .find("warning[min-wait-loop]: loop body waits")
        .expect("primary line: {stdout}");
    let location = stdout.find(" --> ").expect("location: {stdout}");
    let frame = stdout.find(" | 11 | ").expect("source frame: {stdout}");
    let evidence = stdout
        .find("= evidence: static-indicator")
        .expect("notes: {stdout}");
    assert!(
        message < location && location < frame && frame < evidence,
        "{stdout}"
    );
    assert!(stdout.contains("1 file(s) affected"), "{stdout}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn lint_text_resolves_locations_under_a_subdirectory_input_root() {
    // `span.path` is reported root-relative; the human view resolves it under
    // the input root so a subdirectory input still gets an actionable
    // location and a source frame instead of a bare file name.
    let dir = temp_dir();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let path = dir.join("sub").join("flow.txt");
    std::fs::write(&path, corpus_workshop("synthetic/control-flow")).unwrap();
    let output = run(&["lint", path.to_str().unwrap()]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!("{}:11:9", path.display())),
        "the location resolves under the input root: {stdout}"
    );
    assert!(stdout.contains(" | 11 | "), "source frame: {stdout}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lint_text_clean_input_reports_a_pass_verdict_and_no_findings() {
    let path = temp_file("basic.txt", &corpus_workshop("synthetic/basic-rule"));
    let output = run(&["lint", path.to_str().unwrap()]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PASS lint"), "{stdout}");
    assert!(stdout.contains("0 finding(s)"), "{stdout}");
    assert!(stdout.contains("Lint findings\n  none"), "{stdout}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn an_unknown_rule_id_in_a_selection_is_a_usage_error() {
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    for command in ["lint", "check", "analyze"] {
        let output = run(&[command, path.to_str().unwrap(), "--rule-id", "not-a-rule"]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{command} --rule-id not-a-rule must be a usage error"
        );
        assert!(output.stdout.is_empty(), "usage errors write stderr only");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("not-a-rule"),
            "the message names the unknown id"
        );
    }
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn an_unknown_rule_id_in_the_lint_policy_is_a_usage_error() {
    // `--disable-rule`, `--rule-severity`, and lint-config `rules:` keys all
    // reject ids the registry does not know — a typo must not silently leave
    // the intended rule enabled (#640).
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let config = path.parent().unwrap().join("lint.yaml");
    std::fs::write(&config, "rules:\n  not-a-rule:\n    enabled: false\n").unwrap();
    let cases: Vec<Vec<String>> = vec![
        vec!["--disable-rule".into(), "not-a-rule".into()],
        vec!["--rule-severity".into(), "not-a-rule:error".into()],
        vec![
            "--lint-config".into(),
            config.to_string_lossy().into_owned(),
        ],
    ];
    for flags in cases {
        let mut args = vec!["lint".to_string(), path.to_string_lossy().into_owned()];
        args.extend(flags);
        let output = run(&args.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(
            output.status.code(),
            Some(2),
            "lint {args:?} must be a usage error"
        );
        assert!(output.stdout.is_empty(), "usage errors write stderr only");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("not-a-rule"),
            "the message names the unknown id"
        );
    }
    // A lint-config key for a rule loaded through --rule is known (#640).
    let rule_file = path.parent().unwrap().join("smoke.yaml");
    std::fs::write(
        &rule_file,
        "id: community/policy-smoke\nmetadata:\n  summary: summary\n  rationale: rationale\n  documentation: documentation\n  known-limits: limits\n  tags: []\nmatcher: {}\n",
    )
    .unwrap();
    std::fs::write(
        &config,
        "rules:\n  community/policy-smoke:\n    enabled: false\n",
    )
    .unwrap();
    let output = run(&[
        "lint",
        path.to_str().unwrap(),
        "--rule",
        rule_file.to_str().unwrap(),
        "--lint-config",
        config.to_str().unwrap(),
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "a lint-config key for a loaded --rule id is known: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn stdin_workshop_works_and_legacy_protocol_is_refused() {
    let output = run_with_stdin(&["check", "-"], &corpus_workshop("synthetic/basic-rule"));
    assert_eq!(output.status.code(), Some(0));

    // Legacy protocol JSON is recognized but no longer parsed by Wright.
    let protocol = r#"{"protocol":{"name":"wright/opy-hir","version":"1.1.0"}}"#;
    let output = run_with_stdin(&["check", "-"], protocol);
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdin protocol: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("input-kind-unsupported"));
}

#[test]
fn bom_prefixed_workshop_behaves_like_the_plain_file() {
    // #636: a U+FEFF byte-order mark is a file-encoding artifact, not
    // source text — check and compile see the BOM-less program.
    let bom = format!("\u{FEFF}{}", corpus_workshop("synthetic/basic-rule"));
    let plain = temp_file("plain.ws", &corpus_workshop("synthetic/basic-rule"));
    let bommed = temp_file("bom.ws", &bom);

    let output = run(&["check", bommed.to_str().unwrap()]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PASS check"), "{stdout}");

    let bommed_out = run(&["compile", bommed.to_str().unwrap(), "-o", "-"]);
    let plain_out = run(&["compile", plain.to_str().unwrap(), "-o", "-"]);
    assert!(bommed_out.status.success() && plain_out.status.success());
    assert_eq!(bommed_out.stdout, plain_out.stdout);

    // A BOM-prefixed Workshop stdin resolves the same way; a BOM-prefixed
    // JSON stdin is still protocol input, not Workshop text.
    let output = run_with_stdin(&["check", "-"], &bom);
    assert_eq!(output.status.code(), Some(0));
    let output = run_with_stdin(&["check", "-"], "\u{FEFF}{\"protocol\":{}}");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("input-kind-unsupported"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = std::fs::remove_dir_all(bommed.parent().unwrap());
    let _ = std::fs::remove_dir_all(plain.parent().unwrap());
}

#[test]
fn stdin_opy_requires_an_entry_for_provider_workflows() {
    let source =
        std::fs::read_to_string(workspace_root().join("tests/fixtures/opy/basic-rule.opy"))
            .unwrap();
    let output = run_with_stdin(&["compile", "-", "--kind", "opy", "-f", "json"], &source);
    assert_eq!(output.status.code(), Some(3));
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["ok"], false);
    assert_eq!(
        envelope["diagnostics"][0]["code"],
        "source-provider-unsupported"
    );
}

#[test]
fn unknown_extension_is_ambiguous_and_fails_explicitly() {
    let path = temp_file("mystery.data", "whatever");
    let output = run(&["check", path.to_str().unwrap(), "-f", "json"]);
    assert_eq!(output.status.code(), Some(1));
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["diagnostics"][0]["code"], "input-kind-unknown");
    assert!(
        envelope["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("--kind"),
        "ambiguous input guidance is actionable"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// #725 / ADR-0028: `--profile` applies to Workshop input only — a
/// non-Workshop input is a usage error (exit 2) on compile and inspect,
/// `aggressive` is rejected as an unknown value, and serve rejects the
/// combination at startup when `--kind` fixes the type and per request
/// when the kind resolves from the input.
#[test]
fn profile_applies_to_workshop_input_only() {
    let opy = std::fs::read_to_string(workspace_root().join("tests/fixtures/opy/basic-rule.opy"))
        .unwrap();
    let opy_path = temp_file("profile-fixture.opy", &opy);
    let ostw_path = temp_file("profile-fixture.ostw", "anything");
    let workshop_path = temp_file(
        "profile-fixture.ws",
        &corpus_workshop("synthetic/control-flow"),
    );

    // OPY and OSTW inputs with a profile are a usage error, by extension
    // and by explicit --kind alike.
    for (input, extra) in [
        (opy_path.to_str().unwrap().to_string(), vec![]),
        (ostw_path.to_str().unwrap().to_string(), vec![]),
        (
            opy_path.to_str().unwrap().to_string(),
            vec!["--kind", "opy"],
        ),
        (
            ostw_path.to_str().unwrap().to_string(),
            vec!["--kind", "ostw"],
        ),
    ] {
        for command in ["compile", "check", "inspect"] {
            let mut args: Vec<&str> = if command == "inspect" {
                vec!["inspect", "cost", &input, "--profile", "compat"]
            } else if command == "compile" {
                vec!["compile", &input, "--profile", "compat", "-o", "-"]
            } else {
                vec![command, &input, "--profile", "compat"]
            };
            args.extend(extra.iter());
            let output = run(&args);
            assert_eq!(
                output.status.code(),
                Some(2),
                "{args:?}: {}",
                command_result(&output)
            );
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("Workshop")
                    || String::from_utf8_lossy(&output.stdout).contains("Workshop"),
                "{args:?} names the Workshop-only rule"
            );
        }
    }

    // `aggressive` is rejected as an unknown value; the error lists the
    // surviving spellings.
    let aggressive = run(&[
        "check",
        workshop_path.to_str().unwrap(),
        "--profile",
        "aggressive",
    ]);
    assert_eq!(aggressive.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&aggressive.stderr);
    assert!(
        stderr.contains("off") && stderr.contains("compat"),
        "the error lists the surviving spellings: {stderr}"
    );
    assert!(
        !stderr.contains("values: off, compat, aggressive"),
        "aggressive is no longer a possible value: {stderr}"
    );

    // A Workshop input with `compat` still folds as before.
    let workshop = run(&[
        "check",
        workshop_path.to_str().unwrap(),
        "--profile",
        "compat",
    ]);
    assert!(workshop.status.success(), "{}", command_result(&workshop));

    // `serve --kind opy --profile compat` is a startup usage error.
    let serve = run(&[
        "serve",
        "--transport",
        "stdio",
        "--kind",
        "opy",
        "--profile",
        "compat",
        opy_path.to_str().unwrap(),
    ]);
    assert_eq!(serve.status.code(), Some(2), "{}", command_result(&serve));
    assert!(
        String::from_utf8_lossy(&serve.stderr).contains("Workshop"),
        "{}",
        String::from_utf8_lossy(&serve.stderr)
    );

    let _ = std::fs::remove_dir_all(opy_path.parent().unwrap());
    let _ = std::fs::remove_dir_all(ostw_path.parent().unwrap());
    let _ = std::fs::remove_dir_all(workshop_path.parent().unwrap());
}

#[test]
fn unknown_flag_is_a_usage_error_exit_two() {
    let output = run(&["check", "--frobnicate"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "usage errors write stderr only");
    assert!(!output.stderr.is_empty());
}

#[test]
fn json_output_is_deterministic_across_runs() {
    let path = temp_file("flow.txt", &corpus_workshop("synthetic/control-flow"));
    let first = run(&["analyze", path.to_str().unwrap(), "-f", "json"]);
    let second = run(&["analyze", path.to_str().unwrap(), "-f", "json"]);
    assert_eq!(
        first.stdout, second.stdout,
        "JSON output must be byte-deterministic"
    );
    assert!(!first.stdout.is_empty());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn stdout_stderr_separation_holds_in_both_modes() {
    let path = temp_file("basic.txt", &corpus_workshop("synthetic/basic-rule"));
    let output = run(&["check", path.to_str().unwrap()]);
    assert!(output.stderr.is_empty());
    assert!(String::from_utf8_lossy(&output.stdout).contains("PASS check"));
    let output = run(&["check", path.to_str().unwrap(), "-f", "json"]);
    assert!(output.stderr.is_empty());
    parse_json(&output.stdout);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn tty_progress_stops_and_clears_before_final_render() {
    let path = workspace_root().join("tests/fixtures/opy/basic-rule.opy");
    let output = run_in_tty(&[
        "analyze",
        path.to_str().unwrap(),
        "--kind",
        "opy",
        "--renderer",
        "terminal",
        "--color",
        "never",
    ]);
    assert!(output.status.success());
    let mut transcript = output.stdout;
    transcript.extend_from_slice(&output.stderr);
    let transcript = String::from_utf8_lossy(&transcript);
    let final_render = transcript
        .find("PASS analyze")
        .expect("final analyze render is present");
    let before_final = &transcript[..final_render];
    assert!(before_final.contains("Starting workflow"));
    assert!(before_final.contains("Resolving input"));
    assert!(before_final.contains("Parsing"));
    assert!(before_final.contains("Resolving semantics"));
    let cleared = before_final
        .rfind("\x1b[2K")
        .expect("activity line is cleared before final render");
    assert!(cleared < final_render);
    assert!(!transcript.contains("working…PASS"));

    let lint = run_in_tty(&[
        "lint",
        path.to_str().unwrap(),
        "--kind",
        "opy",
        "--renderer",
        "terminal",
        "--color",
        "never",
    ]);
    assert!(lint.status.success());
    let mut lint_transcript = lint.stdout;
    lint_transcript.extend_from_slice(&lint.stderr);
    let lint_transcript = String::from_utf8_lossy(&lint_transcript);
    assert!(lint_transcript.contains("Running lint rules"));
    assert!(lint_transcript.contains(" rules…"));
}

#[test]
fn non_interactive_renderers_have_no_progress_artifacts() {
    let source =
        std::fs::read_to_string(workspace_root().join("tests/fixtures/opy/basic-rule.opy"))
            .unwrap();
    let path = temp_file("basic.opy", &source);
    for renderer in ["plain", "github-actions"] {
        let output = run_with_env(
            &[
                "analyze",
                path.to_str().unwrap(),
                "--renderer",
                renderer,
                "--color",
                "always",
            ],
            &[("GITHUB_ACTIONS", "true")],
        );
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !combined.contains("Resolving input"),
            "{renderer}: {combined}"
        );
        assert!(
            !combined.contains("Running lint rules"),
            "{renderer}: {combined}"
        );
        assert!(!combined.contains("⠋"), "{renderer}: {combined}");
    }
    let json = run(&["analyze", path.to_str().unwrap(), "--format", "json"]);
    assert!(json.status.success());
    assert!(json.stderr.is_empty());
    assert!(!String::from_utf8_lossy(&json.stdout).contains("Resolving input"));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn explicit_locale_override_is_accepted() {
    let path = temp_file("basic.txt", &corpus_workshop("synthetic/basic-rule"));
    let output = run(&["check", path.to_str().unwrap(), "--locale", "en-US"]);
    assert_eq!(output.status.code(), Some(0));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn version_and_help_are_documented_contract_surfaces() {
    let output = run(&["--version"]);
    assert!(output.status.success());
    let banner = String::from_utf8_lossy(&output.stdout);
    assert!(banner.starts_with("wright "), "{banner}");
    assert!(
        banner.contains(env!("CARGO_PKG_VERSION")),
        "banner does not report the implementation version: {banner}"
    );
    assert!(banner.contains("wright-driver"), "{banner}");

    let output = run(&["--help"]);
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    for command in [
        "compile", "convert", "check", "analyze", "lint", "inspect", "agent", "lookup",
    ] {
        assert!(help.contains(command), "help documents {command}");
    }

    // #439: semantic queries live under `inspect`, not the top level.
    let output = run(&["inspect", "--help"]);
    assert!(output.status.success());
    let inspect_help = String::from_utf8_lossy(&output.stdout);
    for subcommand in ["symbols", "refs", "cfg", "callgraph", "cost"] {
        assert!(
            inspect_help.contains(subcommand),
            "inspect help documents {subcommand}"
        );
    }
    for option in [
        "--kind",
        "--target",
        "--locale",
        "--root",
        "--profile",
        "--format",
        "--renderer",
        "--color",
        "--disable-rule",
        "--rule-severity",
    ] {
        assert!(help.contains(option), "top-level help documents {option}");
    }
    assert!(help.contains("EXIT CODES"));

    // The consolidated maintenance surface (#439) retires the dedicated
    // meta/provider commands; `--help` and `--version` are canonical.
    for removed in ["version", "help", "provider"] {
        let output = run(&[removed]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{removed} is a usage error now"
        );
    }
}

#[test]
fn completion_is_generated_for_all_supported_shells() {
    for shell in ["bash", "zsh", "fish", "powershell", "pwsh"] {
        let output = run(&["completion", shell]);
        assert!(output.status.success(), "{shell}: {:?}", output.status);
        assert!(output.stderr.is_empty(), "{shell}: stderr is not clean");
        let completion = String::from_utf8_lossy(&output.stdout);
        assert!(completion.contains("compile"), "{shell}: {completion}");
        assert!(completion.contains("renderer"), "{shell}: {completion}");
        assert!(completion.contains("color"), "{shell}: {completion}");
    }
}

#[test]
fn completion_without_arguments_is_usage_error() {
    let output = run(&["completion"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("specify a shell") && stderr.contains("install"));
}

#[test]
fn completion_install_explicit_shell_and_dir() {
    let dir = temp_dir();
    let output = run(&[
        "completion",
        "install",
        "zsh",
        "--dir",
        dir.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("installed zsh completion"), "{stdout}");
    let target = dir.join("_wright");
    assert!(target.is_file());
    let content = std::fs::read_to_string(&target).unwrap();
    assert!(content.contains("#compdef wright") || content.contains("wright"));

    let output2 = run(&[
        "completion",
        "install",
        "zsh",
        "--dir",
        dir.to_str().unwrap(),
    ]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("is already up to date"), "{stdout2}");

    let output3 = run(&[
        "completion",
        "install",
        "zsh",
        "--dir",
        dir.to_str().unwrap(),
        "--force",
    ]);
    assert!(output3.status.success());
    let stdout3 = String::from_utf8_lossy(&output3.stdout);
    assert!(stdout3.contains("updated zsh completion"), "{stdout3}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn completion_shell_positional_before_install_selects_that_shell() {
    // `wright completion bash install` accepts `bash` ahead of the
    // subcommand; it must install the bash completion, not the detected
    // shell's (#641).
    let dir = temp_dir();
    let output = Command::new(wright())
        .args([
            "completion",
            "bash",
            "install",
            "--dir",
            dir.to_str().unwrap(),
        ])
        .env("WRIGHT_SHELL", "")
        .env("SHELL", "/bin/zsh")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("installed bash completion"),
        "the pre-subcommand positional wins over SHELL detection: {stdout}"
    );
    assert!(dir.join("wright").is_file(), "{stdout}");

    // An explicit install positional still wins over the outer one.
    let output = Command::new(wright())
        .args([
            "completion",
            "bash",
            "install",
            "fish",
            "--dir",
            dir.to_str().unwrap(),
        ])
        .env("WRIGHT_SHELL", "")
        .env("SHELL", "/bin/zsh")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("installed fish completion"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn agent_install_writes_the_guide_and_refreshes_it() {
    let dir = temp_dir();
    let target = dir.join("wright");
    let install = |extra: &[&str]| {
        let mut argv = vec!["agent", "install", "--dest", dir.to_str().unwrap()];
        argv.extend_from_slice(extra);
        run(&argv)
    };

    let output = install(&[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("installed"));
    assert!(target.join("SKILL.md").is_file());
    assert!(target.join("references/language-notes.md").is_file());
    let record: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(target.join("BUILD.json")).unwrap()).unwrap();
    assert_eq!(record["installedBy"], "wright agent install");

    let output = install(&[]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("up to date"));

    // A foreign directory refuses (exit 1) unless --force replaces it.
    std::fs::write(target.join("BUILD.json"), "{}").unwrap();
    let output = install(&[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--force"));
    assert!(install(&["--force"]).status.success());

    let _ = std::fs::remove_dir_all(&dir);
    let output = install(&["--dry-run"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("would install"));
    assert!(!target.exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn agent_tools_emits_messages_and_json_schema_forms_deterministically() {
    let messages = run(&["agent", "tools"]);
    assert!(
        messages.status.success(),
        "{}",
        String::from_utf8_lossy(&messages.stderr)
    );
    let doc: serde_json::Value = serde_json::from_slice(&messages.stdout).unwrap();
    assert_eq!(doc["contract"], "wright-agent/v1");
    let tools = doc["tools"].as_array().unwrap();
    assert!(!tools.is_empty());
    for tool in tools {
        assert!(tool["name"].as_str().unwrap().starts_with("wright_"));
        assert!(tool["description"].as_str().unwrap().contains("Result:"));
        assert_eq!(tool["input_schema"]["type"], "object");
        assert_eq!(
            tool["allowed_callers"],
            serde_json::json!(["code_execution_20260120"])
        );
    }
    assert_eq!(run(&["agent", "tools"]).stdout, messages.stdout);

    let plain = run(&["agent", "tools", "--format", "json-schema"]);
    assert!(plain.status.success());
    let doc: serde_json::Value = serde_json::from_slice(&plain.stdout).unwrap();
    assert_eq!(doc["contract"], "wright-agent/v1");
    let schemas = doc["schemas"].as_object().unwrap();
    assert_eq!(schemas.len(), tools.len());
    for (op, schema) in schemas {
        assert_eq!(schema["type"], "object", "{op}");
        assert!(schema["description"].is_string(), "{op}");
    }
}

#[test]
fn completion_install_dry_run() {
    let dir = temp_dir();
    let output = run(&[
        "completion",
        "install",
        "bash",
        "--dir",
        dir.to_str().unwrap(),
        "--dry-run",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("would install bash completion"), "{stdout}");
    assert!(
        !dir.join("wright").exists(),
        "dry-run must not create files"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn completion_install_detection_via_env() {
    let dir = temp_dir();
    let output = run_with_env(
        &["completion", "install"],
        &[
            ("WRIGHT_SHELL", "fish"),
            ("WRIGHT_COMPLETION_DIR", dir.to_str().unwrap()),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("installed fish completion"), "{stdout}");
    assert!(dir.join("wright.fish").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn completion_install_all_flag() {
    let dir = temp_dir();
    let output = run(&[
        "completion",
        "install",
        "--all",
        "--dir",
        dir.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("installed bash completion"), "{stdout}");
    assert!(stdout.contains("installed zsh completion"), "{stdout}");
    assert!(stdout.contains("installed fish completion"), "{stdout}");
    assert!(
        stdout.contains("installed powershell completion"),
        "{stdout}"
    );
    assert!(dir.join("wright").is_file());
    assert!(dir.join("_wright").is_file());
    assert!(dir.join("wright.fish").is_file());
    assert!(dir.join("_wright.ps1").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn completion_install_undetected_shell_reports_user_error() {
    let output = run_with_env(
        &["completion", "install"],
        &[
            ("WRIGHT_SHELL", ""),
            ("SHELL", ""),
            ("ZSH_VERSION", ""),
            ("BASH_VERSION", ""),
            ("FISH_VERSION", ""),
            ("PSModulePath", ""),
            ("POWERSHELL_DISTRIBUTION_CHANNEL", ""),
            ("PSExecutionPolicyPreference", ""),
            ("ZDOTDIR", ""),
        ],
    );
    // On Unix this is an undetected shell user error (exit 1). On Windows cfg!(windows) defaults to powershell.
    if !cfg!(windows) {
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("could not automatically detect your shell"),
            "{stderr}"
        );
        assert!(
            stderr.contains("wright completion install <bash|zsh|fish|powershell>"),
            "{stderr}"
        );
    }
}

#[test]
fn explicit_renderer_and_color_overrides_are_respected() {
    let path = temp_file("broken.txt", "rule (\"x\") { event { Ongoing - Global; }");

    let github = run_with_env(
        &[
            "check",
            path.to_str().unwrap(),
            "--renderer",
            "github-actions",
        ],
        &[("GITHUB_ACTIONS", "true")],
    );
    assert_eq!(github.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&github.stderr).contains("::error"));
    assert!(String::from_utf8_lossy(&github.stderr).contains("::group::"));

    let plain = run_with_env(
        &["check", path.to_str().unwrap(), "--renderer", "plain"],
        &[("GITHUB_ACTIONS", "true")],
    );
    assert_eq!(plain.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&plain.stderr).contains("::error"));
    assert!(!String::from_utf8_lossy(&plain.stderr).contains("\x1b["));

    let color = run_with_env(
        &[
            "check",
            path.to_str().unwrap(),
            "--renderer",
            "terminal",
            "--color",
            "always",
        ],
        &[("GITHUB_ACTIONS", "true"), ("NO_COLOR", "1")],
    );
    assert_eq!(color.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&color.stderr).contains("\x1b["));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn json_and_source_stdout_stay_pure_in_github_actions() {
    let path = temp_file("broken.txt", "rule (\"x\") { event { Ongoing - Global; }");
    let json = run_with_env(
        &[
            "check",
            path.to_str().unwrap(),
            "-f",
            "json",
            "--renderer",
            "github-actions",
            "--color",
            "always",
        ],
        &[("GITHUB_ACTIONS", "true")],
    );
    assert_eq!(json.status.code(), Some(1));
    assert!(json.stderr.is_empty());
    let envelope = parse_json(&json.stdout);
    assert_eq!(envelope["wright"]["contract"], "wright-result/v1");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());

    let source = corpus_workshop("synthetic/basic-rule");
    let path = temp_file("basic.txt", &source);
    let compile = run_with_env(
        &[
            "compile",
            path.to_str().unwrap(),
            "-o",
            "-",
            "--renderer",
            "github-actions",
        ],
        &[("GITHUB_ACTIONS", "true")],
    );
    assert_eq!(compile.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&compile.stdout);
    assert!(stdout.contains("Disable Inspector Recording"));
    assert!(!stdout.contains("::"));
    assert!(String::from_utf8_lossy(&compile.stderr).contains("::group::"));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn source_artifacts_are_byte_exact_in_plain_and_github_renderers() {
    let source = corpus_workshop("synthetic/basic-rule");
    let path = temp_file("basic.txt", &source);

    let written = path.with_extension("ow");
    assert!(
        run(&[
            "compile",
            path.to_str().unwrap(),
            "-o",
            written.to_str().unwrap()
        ])
        .status
        .success()
    );
    let expected = std::fs::read(&written).unwrap();
    for renderer in ["plain", "github-actions"] {
        let output = run_with_env(
            &[
                "compile",
                path.to_str().unwrap(),
                "-o",
                "-",
                "--renderer",
                renderer,
            ],
            &[("GITHUB_ACTIONS", "true")],
        );
        assert!(output.status.success(), "{renderer}");
        assert_eq!(output.stdout, expected, "{renderer} must preserve bytes");
    }

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn github_summary_uses_step_summary_file_when_available() {
    let path = temp_file("broken.txt", "rule (\"x\") { event { Ongoing - Global; }");
    let summary = temp_file("summary.md", "");
    let output = run_with_env(
        &[
            "check",
            path.to_str().unwrap(),
            "--renderer",
            "github-actions",
        ],
        &[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_STEP_SUMMARY", summary.to_str().unwrap()),
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(!std::fs::read_to_string(&summary).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn opy_file_does_not_fall_back_to_native_frontend() {
    let source =
        std::fs::read_to_string(workspace_root().join("tests/fixtures/opy/basic-rule.opy"))
            .unwrap();
    let path = temp_file("basic-rule.opy", &source);
    let missing_provider = path.parent().unwrap().join("missing-opy-provider");
    let output = run(&[
        "compile",
        path.to_str().unwrap(),
        "--opy-provider",
        missing_provider.to_str().unwrap(),
        "-f",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(4));
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["diagnostics"][0]["code"], "provider-missing");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn provider_backed_opy_analyze_surfaces_provider_resolution_failures() {
    let path = temp_file("main.opy", "rule \"r\":\n    @Event global\n");
    let missing_provider = path.parent().unwrap().join("missing-opy-provider");
    let output = run(&[
        "analyze",
        path.to_str().unwrap(),
        "--opy-provider",
        missing_provider.to_str().unwrap(),
        "-f",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(4));
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["diagnostics"][0]["code"], "provider-missing");
    assert!(envelope["result"]["program"].is_null());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// #655: `wright inspect` and every query subcommand load a provider-backed
/// `.opy` input through the provider's compile operation — the same
/// operation-aware load `lint`/`analyze` run — instead of refusing on the
/// check-only path. The provider is a staged LPP script so the test stays
/// hermetic.
#[cfg(unix)]
#[test]
fn provider_backed_opy_accepts_inspect_and_its_query_subcommands() {
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

    let dir = temp_dir();
    let script = dir.join("fake-provider");
    std::fs::write(&script, PROVIDER).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        dir.join("artifact.ws"),
        corpus_workshop("synthetic/basic-rule"),
    )
    .unwrap();
    let opy = std::fs::read_to_string(workspace_root().join("tests/fixtures/opy/basic-rule.opy"))
        .unwrap();
    let input = dir.join("main.opy");
    std::fs::write(&input, &opy).unwrap();
    let provider = script.to_str().unwrap().to_string();
    let input = input.to_str().unwrap().to_string();
    let run_inspect = |args: &[&str]| {
        let output = run(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            command_result(&output)
        );
        parse_json(&output.stdout)
    };

    let envelope = run_inspect(&["inspect", &input, "--opy-provider", &provider, "-f", "json"]);
    assert_eq!(envelope["command"], "inspect");
    assert_eq!(envelope["ok"], true);
    let rules = envelope["result"]["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 1, "{envelope}");
    assert_eq!(rules[0]["name"], "setup");

    let envelope = run_inspect(&[
        "inspect",
        "rules",
        "--content",
        &input,
        "--opy-provider",
        &provider,
        "-f",
        "json",
    ]);
    assert_eq!(envelope["command"], "rules");
    assert_eq!(envelope["ok"], true);
    let rules = envelope["result"]["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 1, "{envelope}");
    assert_eq!(
        rules[0]["content"]["actions"].as_array().unwrap().len(),
        1,
        "{envelope}"
    );

    let envelope = run_inspect(&[
        "inspect",
        "symbols",
        &input,
        "--opy-provider",
        &provider,
        "-f",
        "json",
    ]);
    assert_eq!(envelope["command"], "symbols");
    assert_eq!(envelope["ok"], true);
    let symbols = envelope["result"].as_array().unwrap();
    assert!(
        symbols.iter().any(|symbol| symbol["name"] == "setup"),
        "{envelope}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// #691: `inspect cost` on a provider-backed `.opy` reports the canonical
/// element count of the compiled artifact — the same `workshop-rs` count a
/// `wright compile` re-parse measures. The provider is a staged LPP script
/// serving one canned artifact, so the test stays hermetic.
#[cfg(unix)]
#[test]
fn provider_backed_opy_cost_reports_the_artifact_element_count() {
    use std::os::unix::fs::PermissionsExt;

    const PROVIDER: &str = r#"#!/usr/bin/env python3
import json, os, sys
artifact = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "artifact.ws")).read()
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
                       "capabilities": {"check": True, "compile": True, "reconstruct": False, "symbols": False, "definition": False, "references": False, "rename": False, "editValidation": False, "projectLoading": True}})
    elif method == "lpp/compile":
        reply(id, {"diagnostics": [], "artifact": {"format": "workshop-rs/text-v1", "content": artifact}})
    else:
        reply(id, {})
"#;

    let dir = temp_dir();
    let script = dir.join("fake-provider");
    std::fs::write(&script, PROVIDER).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        dir.join("artifact.ws"),
        corpus_workshop("synthetic/control-flow"),
    )
    .unwrap();
    let opy = std::fs::read_to_string(workspace_root().join("tests/fixtures/opy/basic-rule.opy"))
        .unwrap();
    let input = dir.join("main.opy");
    std::fs::write(&input, &opy).unwrap();
    let provider = script.to_str().unwrap();
    let input = input.to_str().unwrap();

    // `wright compile` emits the artifact the cost query loads; the
    // canonical count of that output — measured by the owner's own
    // counter — is the expected total.
    let output = run(&["compile", input, "--opy-provider", provider, "-o", "-"]);
    assert!(output.status.success(), "{}", command_result(&output));
    let compiled = String::from_utf8_lossy(&output.stdout);
    let catalog = workshop_rs::catalog::Catalog::builtin_ref().expect("catalog");
    let locale = workshop_rs::catalog::Locale::new("en-US");
    let program = workshop_rs::parser::parse_with_context(&compiled, catalog, &locale, catalog)
        .expect("compiled artifact parses");
    let report = program.element_count(catalog).expect("artifact counts");

    let output = run(&[
        "inspect",
        "cost",
        input,
        "--opy-provider",
        provider,
        "-f",
        "json",
    ]);
    assert!(output.status.success(), "{}", command_result(&output));
    let envelope = parse_json(&output.stdout);
    let elements = &envelope["result"]["elementCount"];
    assert_eq!(elements["total"], report.total);
    assert_eq!(elements["budget"], 32768);
    assert_eq!(
        elements["rules"].as_array().unwrap().len(),
        program.rules.len()
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn convert_workshop_input_to_opy_requires_provider_reconstruct_capability() {
    let path = temp_file("convert.txt", &corpus_workshop("synthetic/basic-rule"));
    let path = path.to_str().unwrap();
    let output = run(&["convert", "--target", "opy", path, "-f", "json"]);
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["command"], "convert");
    assert!(
        matches!(
            envelope["diagnostics"][0]["code"].as_str(),
            Some("capability-unavailable")
        ),
        "unexpected provider refusal: {}",
        envelope["diagnostics"][0]
    );
    assert!(envelope["result"]["text"].as_str().unwrap().is_empty());
}

#[test]
fn convert_workshop_input_to_ostw_reports_provider_unavailable() {
    // `wright convert --target ostw` is a recognized provider boundary.
    let path = temp_file("convert.txt", &corpus_workshop("synthetic/basic-rule"));
    let path = path.to_str().unwrap();
    let output = run(&["convert", "--target", "ostw", path, "-f", "json"]);
    assert_eq!(
        output.status.code(),
        Some(4),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["exit"], 4);
    assert_eq!(
        envelope["diagnostics"][0]["code"],
        "source-provider-unavailable"
    );
    assert_eq!(envelope["diagnostics"][0]["stage"], "internal");
    assert!(envelope["result"]["text"].as_str().unwrap().is_empty());
}

#[test]
fn convert_json_envelope_reports_command_result_and_target() {
    let path = temp_file("convert.txt", &corpus_workshop("synthetic/basic-rule"));
    let path = path.to_str().unwrap();
    let output = run(&["convert", "--target", "opy", path, "-f", "json"]);
    assert!(output.stderr.is_empty(), "JSON mode keeps stderr clean");
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["ok"], false);
    assert_ne!(envelope["exit"], 0);
    assert_eq!(envelope["command"], "convert");
    assert_eq!(envelope["wright"]["contract"], "wright-result/v1");
    assert_eq!(envelope["result"]["target"], "opy");
}

#[test]
fn convert_requires_an_explicit_target_flag() {
    let path = temp_file("convert.txt", &corpus_workshop("synthetic/basic-rule"));
    let path = path.to_str().unwrap();
    let output = run(&["convert", path]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "usage errors write stderr only");
    assert!(String::from_utf8_lossy(&output.stderr).contains("--target"));

    let output = run(&["convert", "--target", "nope", path]);
    assert_eq!(output.status.code(), Some(2));

    let output = run(&["check", "--target", "opy", path]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn convert_rejects_non_workshop_input() {
    // OPY conversion requires an entry provider workflow; it never falls back
    // to a direct OPY ↔ OSTW conversion.
    let source =
        std::fs::read_to_string(workspace_root().join("tests/fixtures/opy/basic-rule.opy"))
            .unwrap();
    let path = temp_file("basic-rule.opy", &source);
    let output = run(&[
        "convert",
        "--target",
        "ostw",
        path.to_str().unwrap(),
        "-f",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(3));
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["ok"], false);
    assert_eq!(
        envelope["diagnostics"][0]["code"],
        "source-provider-unsupported"
    );
    assert!(
        envelope["result"]["text"].as_str().unwrap().is_empty(),
        "no source on a rejected input kind"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

// `inspect symbols`, `inspect refs`, `inspect cfg`, `inspect callgraph`,
// and `inspect cost` run the same operations the agent contract serves;
// `refs`/`cfg` address their target by name.

#[test]
fn symbols_lists_program_symbols_and_filters_by_kind() {
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let path = path.to_str().unwrap();

    let output = run(&["inspect", "symbols", path]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PASS symbols"), "{stdout}");
    assert!(stdout.contains("5 symbol(s)"), "{stdout}");
    // Symbols group by kind; each entry names its primary location (#446).
    assert!(stdout.contains("Global variables (3)"), "{stdout}");
    assert!(stdout.contains("cakePos --> cake.txt:3:14"), "{stdout}");
    assert!(stdout.contains("Rules (2)"), "{stdout}");
    assert!(stdout.contains("cake --> cake.txt:8:1"), "{stdout}");

    let output = run(&["inspect", "symbols", path, "-f", "json"]);
    assert!(output.status.success(), "{}", command_result(&output));
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["command"], "symbols");
    assert_eq!(envelope["wright"]["contract"], "wright-result/v1");
    let symbols = envelope["result"].as_array().unwrap();
    assert_eq!(symbols.len(), 5);
    let rule = symbols
        .iter()
        .find(|symbol| symbol["name"] == "cake")
        .expect("the cake rule symbol");
    assert_eq!(rule["kind"], "rule");
    assert_eq!(rule["span"]["path"], "cake.txt");

    // --only narrows to one symbol kind; --kind stays the input frontend.
    // `kind` predates #531: alone it keeps the bare array, while --max (or
    // --file) wraps the list in `{symbols, selection}` (#531).
    let output = run(&["inspect", "symbols", path, "--only", "rule", "-f", "json"]);
    let envelope = parse_json(&output.stdout);
    let symbols = envelope["result"].as_array().unwrap();
    assert_eq!(symbols.len(), 2);
    assert!(symbols.iter().all(|symbol| symbol["kind"] == "rule"));

    let output = run(&[
        "inspect", "symbols", path, "--only", "rule", "--max", "1", "-f", "json",
    ]);
    let envelope = parse_json(&output.stdout);
    let symbols = envelope["result"]["symbols"].as_array().unwrap();
    assert_eq!(symbols.len(), 1);
    assert!(symbols.iter().all(|symbol| symbol["kind"] == "rule"));
    assert_eq!(envelope["result"]["selection"]["total"], 5);

    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

#[test]
fn refs_reports_references_and_usage_for_a_named_symbol() {
    // The issue's example: cakePos has 16 reads and 1 write across 1 rule.
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let path = path.to_str().unwrap();

    let output = run(&["inspect", "refs", "cakePos", path]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PASS refs"), "{stdout}");
    // Target identity and the usage summary lead the location list (#446).
    assert!(
        stdout.contains("16 read(s), 1 write(s)"),
        "the usage header: {stdout}"
    );
    assert!(
        stdout.contains("References to cakePos (globalVariable)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("declaration --> cake.txt:3:14"),
        "locations carry source positions: {stdout}"
    );
    // 18 references bound to a first page; JSON is the complete path (#446).
    assert!(!stdout.contains("more reference(s)"), "{stdout}");

    let output = run(&["inspect", "refs", "cakePos", path, "-f", "json"]);
    assert!(output.status.success(), "{}", command_result(&output));
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["command"], "refs");
    let result = &envelope["result"];
    assert_eq!(result["symbol"], "cakePos");
    assert_eq!(result["kind"], "globalVariable");
    assert_eq!(result["reads"], 16);
    assert_eq!(result["writes"], 1);
    assert_eq!(result["calls"], 0);
    assert_eq!(result["rules"], 1);
    let references = result["references"].as_array().unwrap();
    assert_eq!(references.len(), 18, "16 reads + 1 write + 1 declaration");
    assert_eq!(
        references
            .iter()
            .filter(|r| r["kind"] == "declaration")
            .count(),
        1
    );
    for reference in references {
        assert_eq!(reference["span"]["path"], "cake.txt");
    }
    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

#[test]
fn cfg_reports_the_named_rules_control_flow_graph() {
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let path = path.to_str().unwrap();

    let output = run(&["inspect", "cfg", "cake", path]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PASS cfg"), "{stdout}");
    // Rule identity and the graph shape lead the block detail (#446).
    assert!(
        stdout.contains("Control-flow graph of rule \"cake\""),
        "{stdout}"
    );
    assert!(stdout.contains("block(s), "), "{stdout}");
    let shape = stdout.find("edge(s):").expect("shape summary");
    let detail = stdout.find("block 0 (entry)").expect("block detail");
    assert!(shape < detail, "shape summary before blocks: {stdout}");

    let output = run(&["inspect", "cfg", "cake", path, "-f", "json"]);
    assert!(output.status.success(), "{}", command_result(&output));
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["command"], "cfg");
    let result = &envelope["result"];
    assert!(result["entry"].is_number() && result["exit"].is_number());
    assert!(!result["blocks"].as_array().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

#[test]
fn callgraph_reports_subroutine_call_edges() {
    let path = temp_file("decl.txt", &corpus_workshop("synthetic/declarations-rules"));
    let path = path.to_str().unwrap();

    let output = run(&["inspect", "callgraph", path, "-f", "json"]);
    assert!(output.status.success(), "{}", command_result(&output));
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["command"], "callgraph");
    assert_eq!(
        envelope["result"],
        serde_json::json!([{ "caller": "player starts", "callee": "showStatus" }])
    );

    let output = run(&["inspect", "callgraph", path]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PASS callgraph"), "{stdout}");
    assert!(stdout.contains("player starts -> showStatus"), "{stdout}");
    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

#[test]
fn callgraph_ranks_fan_in_and_fan_out_before_the_edge_list() {
    let path = temp_file("subs.txt", &corpus_workshop("synthetic/subroutines"));
    let path = path.to_str().unwrap();

    let output = run(&["inspect", "callgraph", path]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The shape summary leads, then notable fan-in/fan-out, then edges (#446).
    assert!(
        stdout.contains("3 edge(s): 2 calling rule(s), 2 subroutine(s) called"),
        "{stdout}"
    );
    assert!(stdout.contains("shared: 2 caller(s)"), "{stdout}");
    assert!(stdout.contains("first: calls 2 subroutine(s)"), "{stdout}");
    let most_called = stdout.find("Most-called subroutines").unwrap();
    let edges = stdout.find("Edges").unwrap();
    assert!(most_called < edges, "ranking before edge list: {stdout}");
    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

#[test]
fn callgraph_highlight_sections_fold_beyond_the_bound() {
    // Twelve subroutines each with two callers: the highlight list exceeds
    // one page and folds with the complete-output pointer like every other
    // bounded section (#446).
    let mut source = String::from("subroutines {\n");
    for i in 0..12 {
        source.push_str(&format!("    {i}: sub{i}\n"));
    }
    source.push_str("}\n");
    for i in 0..12 {
        source.push_str(&format!(
            "rule (\"Subroutine sub{i}\") {{\n    event {{\n        Subroutine;\n        sub{i};\n    }}\n    actions {{\n        Wait(1);\n    }}\n}}\n"
        ));
    }
    let mut calls = String::new();
    for i in 0..12 {
        calls.push_str(&format!("        Call Subroutine(sub{i});\n"));
    }
    for rule in ["a", "b"] {
        source.push_str(&format!(
            "rule (\"{rule}\") {{\n    event {{\n        Ongoing - Global;\n    }}\n    actions {{\n{calls}    }}\n}}\n"
        ));
    }
    let path = temp_file("fan.ws", &source);
    let path = path.to_str().unwrap();

    let output = run(&["inspect", "callgraph", path]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("sub0: 2 caller(s)"), "{stdout}");
    assert!(stdout.contains("showing 20 of 24"), "{stdout}");
    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

/// #723 / ADR-0027: the `.wright/` run cache compares each `inspect cost`
/// run against the previous successful run of the same key, never changes
/// the verdict, and `--no-cache`/`WRIGHT_NO_CACHE`/`CI` leave it untouched.
#[test]
fn cost_run_cache_compares_against_the_previous_run() {
    let dir = temp_dir();
    let path = dir.join("t.ws");
    std::fs::write(&path, "rule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Wait(0.016);\n    }\n}\n").unwrap();
    let path = path.to_str().unwrap();

    // The first run records a reference and reports no comparison.
    let first = run(&["inspect", "cost", path]);
    assert!(first.status.success(), "{}", command_result(&first));
    assert!(
        !String::from_utf8_lossy(&first.stdout).contains("vs previous run"),
        "no reference on a first run"
    );
    assert!(dir.join(".wright").is_dir(), "the run wrote a record");

    // The second run of the same key reports a zero delta.
    let second = run(&["inspect", "cost", path]);
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(stdout.contains("vs previous run"), "{stdout}");
    assert!(stdout.contains("Element cost: 0 "), "{stdout}");

    // The comparison rides the envelope, never the default JSON result.
    let json = run(&["inspect", "cost", path, "-f", "json"]);
    let result = parse_json(&json.stdout)["result"].clone();
    assert!(
        result.get("comparison").is_none(),
        "default JSON carries no comparison (ADR-0027 decision 3): {result}"
    );

    // `--max-total` below the current total exits 1 and names the overage;
    // at or above it exits 0 — identical with and without a cache.
    let over = run(&["inspect", "cost", path, "--max-total", "1"]);
    assert_eq!(over.status.code(), Some(1), "{}", command_result(&over));
    let under = run(&["inspect", "cost", path, "--max-total", "32768"]);
    assert!(under.status.success(), "{}", command_result(&under));

    // `--no-cache`, `WRIGHT_NO_CACHE`, and a truthy `CI` each leave
    // `.wright/` untouched and produce no comparison.
    for (args, env) in [
        (vec!["inspect", "cost", path, "--no-cache"], vec![]),
        (
            vec!["inspect", "cost", path],
            vec![("WRIGHT_NO_CACHE", "1")],
        ),
        (vec!["inspect", "cost", path], vec![("CI", "true")]),
    ] {
        let before = std::fs::read_dir(dir.join(".wright")).map(|r| r.count());
        let output = run_with_env(&args, &env);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            command_result(&output)
        );
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("vs previous run"),
            "{args:?} suppressed the comparison"
        );
        let after = std::fs::read_dir(dir.join(".wright")).map(|r| r.count());
        assert_eq!(before.ok(), after.ok(), "{args:?} left the cache untouched");
    }

    // A named baseline compares against a fixed reference; an unknown name
    // given explicitly is a usage error.
    let save = run(&["inspect", "cost", path, "--save-baseline", "v1"]);
    assert!(save.status.success(), "{}", command_result(&save));
    let named = run(&["inspect", "cost", path, "--baseline", "v1"]);
    let stdout = String::from_utf8_lossy(&named.stdout);
    assert!(stdout.contains("baseline:v1"), "{stdout}");
    let unknown = run(&["inspect", "cost", path, "--baseline", "nope"]);
    assert_eq!(
        unknown.status.code(),
        Some(1),
        "{}",
        command_result(&unknown)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cost_run_cache_is_shared_by_compile_and_inspect() {
    let dir = temp_dir();
    let path = dir.join("t.ws");
    std::fs::write(&path, corpus_workshop("synthetic/control-flow")).unwrap();
    let path = path.to_str().unwrap();

    let compiled = run(&["compile", path, "-o", "-"]);
    assert!(compiled.status.success(), "{}", command_result(&compiled));
    let cache_records = || {
        std::fs::read_dir(dir.join(".wright"))
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                (
                    path.file_name().unwrap().to_owned(),
                    std::fs::read(&path).unwrap(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let compiled_records = cache_records();
    assert_eq!(compiled_records.len(), 1);

    let inspected = run(&["inspect", "cost", path]);
    assert!(inspected.status.success(), "{}", command_result(&inspected));
    let stdout = String::from_utf8_lossy(&inspected.stdout);
    assert!(stdout.contains("vs previous run"), "{stdout}");
    assert!(stdout.contains("Element cost: 0 "), "{stdout}");
    assert_eq!(cache_records(), compiled_records);

    let _ = std::fs::remove_dir_all(&dir);
}

/// #723: a fixture pair where the second run adds one rule and grows
/// another reports exactly those rules with the correct deltas.
#[test]
fn cost_run_cache_names_added_and_grown_rules() {
    let dir = temp_dir();
    let path = dir.join("t.ws");
    let base = "rule (\"a\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Wait(0.016);\n    }\n}\n";
    std::fs::write(&path, base).unwrap();
    let path_str = path.to_str().unwrap();
    assert!(run(&["inspect", "cost", path_str]).status.success());

    // Grow rule a by one wait and add rule b.
    let grown = "rule (\"a\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Wait(0.016);\n        Wait(0.016);\n    }\n}\n\nrule (\"b\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Wait(0.016);\n    }\n}\n";
    std::fs::write(&path, grown).unwrap();
    let second = run(&["inspect", "cost", path_str]);
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(stdout.contains("vs previous run"), "{stdout}");
    assert!(stdout.contains("a: "), "grown rule a is named: {stdout}");
    assert!(
        stdout.contains("b: added"),
        "added rule b is named: {stdout}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cost_reports_exact_counts_and_selected_findings() {
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let path = path.to_str().unwrap();

    let output = run(&["inspect", "cost", path]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The cost verdict counts static finding severities: warnings warn (#635).
    assert!(stdout.contains("WARN cost"), "{stdout}");
    assert!(stdout.contains("3960 emitted byte(s)"), "{stdout}");
    assert!(stdout.contains("29 action(s)"), "{stdout}");
    // Each repeated-value finding names its expression (#735), so the nine
    // cake findings render as nine distinguishable entries.
    assert_eq!(
        stdout.matches("warning[repeated-value]").count(),
        9,
        "{stdout}"
    );
    assert!(
        stdout.contains("`add(valueInArray(cakePos, 7), vector(0, i2, 0))` is evaluated"),
        "{stdout}"
    );
    // Repeated identical findings still collapse into one counted entry
    // (#446): two hot loops raise the same min-wait-loop finding.
    let identical = temp_file(
        "identical.txt",
        "rule (\"r\") {
    event {
        Ongoing - Global;
    }
    actions {
        While(True);
            Wait(0.016, Ignore Condition);
        End;
        While(True);
            Wait(0.016, Ignore Condition);
        End;
    }
}
",
    );
    let output = run(&["inspect", "cost", identical.to_str().unwrap()]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("min-wait-loop] (2 occurrences)"),
        "{stdout}"
    );

    let output = run(&["inspect", "cost", path, "-f", "json"]);
    assert!(output.status.success(), "{}", command_result(&output));
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["command"], "cost");
    let exact = &envelope["result"]["exact"];
    assert_eq!(exact["emittedBytes"], 3960);
    assert_eq!(exact["programActions"], 29);
    assert_eq!(exact["programRules"], 2);
    assert_eq!(exact["waitActions"], 1);
    assert_eq!(
        envelope["result"]["findings"].as_array().unwrap().len(),
        10,
        "cost reports the same findings set as `findings`/`lint`"
    );

    // The shared finding selection applies exactly as on `costEstimate` (#430).
    let output = run(&["inspect", "cost", path, "--max", "2", "-f", "json"]);
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["result"]["findings"].as_array().unwrap().len(), 2);
    assert_eq!(envelope["result"]["selection"]["total"], 10);
    assert_eq!(envelope["result"]["selection"]["withheld"], 8);
    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
    let _ = std::fs::remove_dir_all(identical.parent().unwrap());
}

/// #691: text `inspect cost` reports the canonical element count against
/// the client budget — scoped as structure, not importability — and each
/// rule's share ranked by count.
#[test]
fn cost_text_reports_the_element_budget_and_rule_shares() {
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let path = path.to_str().unwrap();

    let json = run(&["inspect", "cost", path, "-f", "json"]);
    assert!(json.status.success(), "{}", command_result(&json));
    let elements = parse_json(&json.stdout)["result"]["elementCount"].clone();
    let total = elements["total"]
        .as_u64()
        .expect("the payload reports a total");

    let output = run(&["inspect", "cost", path]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!(
            "{total} element(s) of the 32768-element budget [exact]"
        )),
        "{stdout}"
    );
    assert!(
        stdout.contains("not a statement of client importability"),
        "{stdout}"
    );
    // The payload is already ranked by count, largest first; the text view
    // renders that order as-is.
    let rules = elements["rules"].as_array().unwrap();
    assert!(
        rules
            .windows(2)
            .all(|w| w[0]["count"].as_u64().unwrap() >= w[1]["count"].as_u64().unwrap()),
        "payload is ranked by count: {rules:?}"
    );
    let mut offset = 0;
    for rule in rules {
        // Each share renders `N element(s)  <name>  <path:line:col>` (#722).
        let needle = format!(
            "{} element(s)  {}",
            rule["count"].as_u64().unwrap(),
            rule["name"].as_str().unwrap()
        );
        offset = stdout[offset..]
            .find(&needle)
            .map(|found| offset + found + needle.len())
            .expect("shares render in payload order");
    }
    // Each rule's printed location equals the span `inspect rules` reports.
    for rule in rules {
        let span = &rule["span"];
        let location = format!(
            "{}:{}:{}",
            span["path"].as_str().unwrap(),
            span["start"]["line"].as_u64().unwrap(),
            span["start"]["col"].as_u64().unwrap()
        );
        assert!(
            stdout.contains(&location),
            "rule share carries its location: {stdout}"
        );
    }
    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

/// The default `inspect cost` view keeps the 5 largest rules — payload
/// order is the ranking, so the cap keeps the head of it — and
/// reports the cut through `elementCount.truncation.rules`.
#[test]
fn cost_rule_shares_rank_and_cap_at_five_by_default() {
    // 25 rules with strictly increasing action counts give every rule a
    // distinct element share.
    let mut source = String::from("variables {\n    global:\n        0: g\n}\n\n");
    for index in 0..25 {
        let actions = "        Set Global Variable(g, 0);\n".repeat(index + 1);
        source.push_str(&format!(
            "rule (\"rule-{index}\") {{\n    event {{\n        Ongoing - Global;\n    }}\n    actions {{\n{actions}    }}\n}}\n\n"
        ));
    }
    let path = temp_file("many-rules.ws", &source);
    let path = path.to_str().unwrap();

    // The expected ranking is workshop-rs' own report, not the CLI output.
    let catalog = workshop_rs::catalog::Catalog::builtin_ref().expect("catalog");
    let locale = workshop_rs::catalog::Locale::new("en-US");
    let program = workshop_rs::parser::parse_with_context(&source, catalog, &locale, catalog)
        .expect("generated program parses");
    let report = program.element_count(catalog).expect("program counts");
    let mut expected: Vec<usize> = (0..report.rules.len()).collect();
    expected.sort_by_key(|index| (std::cmp::Reverse(report.rules[*index].count), *index));

    let json = run(&["inspect", "cost", path, "-f", "json"]);
    assert!(json.status.success(), "{}", command_result(&json));
    let elements = parse_json(&json.stdout)["result"]["elementCount"].clone();
    let rules = elements["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 5, "default view keeps the cap: {rules:?}");
    for (entry, index) in rules.iter().zip(&expected[..5]) {
        assert_eq!(entry["index"].as_u64().unwrap() as usize, *index);
        assert_eq!(entry["name"], program.rules[*index].name);
        assert_eq!(
            entry["count"].as_u64().unwrap() as usize,
            report.rules[*index].count
        );
    }
    let truncation = &elements["truncation"]["rules"];
    assert_eq!(truncation["total"].as_u64(), Some(25));
    assert_eq!(truncation["shown"].as_u64(), Some(5));

    let all = run(&["inspect", "cost", path, "--all", "-f", "json"]);
    assert!(all.status.success(), "{}", command_result(&all));
    let elements = parse_json(&all.stdout)["result"]["elementCount"].clone();
    let rules = elements["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 25);
    assert_eq!(
        rules
            .iter()
            .map(|rule| rule["count"].as_u64().unwrap() as usize)
            .sum::<usize>(),
        report.total,
        "all shares sum to the total"
    );

    let text = run(&["inspect", "cost", path]);
    assert!(text.status.success(), "{}", command_result(&text));
    let stdout = String::from_utf8_lossy(&text.stdout);
    assert!(stdout.contains("showing 5 of 25"), "{stdout}");

    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

/// #724: `inspect cost --rule` reports one rule's element-cost breakdown.
/// The `byNode` own-cost shares sum to the rule's `count`, and each
/// `Costliest statements` entry is a direct child ranked by recursive
/// `count` with the same span `inspect rules --content` reports.
#[test]
fn cost_rule_drilldown_reconciles_to_the_rule_total() {
    let path = temp_file("drill.ws", &corpus_workshop("real-world/overpy-cake"));
    let path = path.to_str().unwrap();

    // The whole-program view names the rule and its total; the rule span
    // equals the one `inspect rules` reports for it.
    let whole = run(&["inspect", "cost", path, "-f", "json"]);
    assert!(whole.status.success(), "{}", command_result(&whole));
    let whole = parse_json(&whole.stdout);
    let rule_entry = whole["result"]["elementCount"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"].as_str() == Some("cake"))
        .expect("fixture names the rule")
        .clone();
    let rule_total = rule_entry["count"].as_u64().unwrap();

    let rules_json = run(&["inspect", "rules", path, "-f", "json"]);
    let rules_json = parse_json(&rules_json.stdout);
    let inspect_span = rules_json["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"].as_str() == Some("cake"))
        .and_then(|r| r.get("span"))
        .cloned();
    assert_eq!(
        rule_entry["span"],
        inspect_span.unwrap(),
        "the rule share's span equals `inspect rules`' span (#722)"
    );

    // The drill-down's own-cost shares sum to the rule's total and its
    // statements are the direct children ranked by recursive count.
    let drill = run(&[
        "inspect", "cost", path, "--rule", "cake", "--all", "-f", "json",
    ]);
    assert!(drill.status.success(), "{}", command_result(&drill));
    let drill = parse_json(&drill.stdout);
    let drill = &drill["result"];
    assert_eq!(drill["rule"]["name"], "cake");
    assert_eq!(drill["rule"]["count"], rule_total);
    let by_node_sum: u64 = drill["byNode"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["count"].as_u64().unwrap())
        .sum();
    assert_eq!(
        by_node_sum, rule_total,
        "byNode own-cost shares reconcile to the rule total"
    );
    let statements = drill["statements"].as_array().unwrap();
    assert!(
        statements
            .windows(2)
            .all(|w| w[0]["count"].as_u64().unwrap() >= w[1]["count"].as_u64().unwrap()),
        "statements rank by recursive count: {statements:?}"
    );
    // Each statement's span resolves to a real source location inside the
    // rule's span — the drill-down surfaces the retained spans (#722/#724).
    let rule_span = &drill["rule"]["span"];
    let (r_start, r_end) = (
        rule_span["start"]["line"].as_u64().unwrap(),
        rule_span["end"]["line"].as_u64().unwrap(),
    );
    for statement in statements {
        let span = &statement["span"];
        assert!(
            span["path"].as_str().is_some(),
            "a statement with a retained span resolves a path: {span:?}"
        );
        let line = span["start"]["line"].as_u64().unwrap();
        assert!(
            line >= r_start && line <= r_end,
            "statement {line} lies inside the rule span {r_start}..={r_end}"
        );
    }

    // An unknown rule name is a structured `invalid-selection` usage error.
    let bad = run(&[
        "inspect",
        "cost",
        path,
        "--rule",
        "not-a-rule",
        "-f",
        "json",
    ]);
    assert_eq!(bad.status.code(), Some(1));
    let bad = parse_json(&bad.stdout);
    assert_eq!(bad["diagnostics"][0]["code"], "invalid-selection");

    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

#[test]
fn query_commands_reject_unknown_and_ambiguous_names() {
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let path = path.to_str().unwrap();
    for (command_args, code) in [
        (&["inspect", "refs", "nope"][..], "unknown-symbol"),
        (&["inspect", "cfg", "nope"][..], "unknown-rule"),
    ] {
        let output = run(&[command_args, &[path, "-f", "json"]].concat());
        assert_eq!(
            output.status.code(),
            Some(1),
            "{command_args:?}: {}",
            command_result(&output)
        );
        assert!(output.stderr.is_empty(), "JSON mode: no stderr");
        let envelope = parse_json(&output.stdout);
        assert_eq!(envelope["ok"], false);
        assert_eq!(envelope["diagnostics"][0]["code"], code);
        assert_eq!(envelope["diagnostics"][0]["stage"], "analysis");
        assert!(
            envelope["result"].is_null(),
            "a failed query carries no result"
        );
    }

    // A name shared across symbol kinds (or rules) is ambiguous, never
    // silently resolved to one candidate.
    let dup = temp_file(
        "dup.ws",
        r#"variables {
    global:
        0: dup
}
rule ("dup") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(dup, 1);
    }
}
rule ("dup") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(dup, 2);
    }
}
"#,
    );
    for (command_args, code) in [
        (&["inspect", "refs", "dup"][..], "ambiguous-symbol"),
        (&["inspect", "cfg", "dup"][..], "ambiguous-rule"),
    ] {
        let output = run(&[command_args, &[dup.to_str().unwrap(), "-f", "json"]].concat());
        assert_eq!(output.status.code(), Some(1), "{command_args:?}");
        let envelope = parse_json(&output.stdout);
        assert_eq!(envelope["diagnostics"][0]["code"], code);
        assert!(
            envelope["diagnostics"][0]["message"]
                .as_str()
                .unwrap()
                .contains("dup"),
            "the diagnostic lists candidates: {}",
            envelope["diagnostics"][0]["message"]
        );
    }

    // Text mode reports the same structured diagnostic on stderr, and a
    // failed query shows no fabricated default result (#446).
    let output = run(&["inspect", "refs", "nope", path]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown-symbol"), "{stderr}");
    assert!(stderr.contains("unknown symbol 'nope'"), "{stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ERROR refs"), "{stdout}");
    assert!(
        !stdout.contains("<unknown>") && !stdout.contains("read(s)"),
        "a failed query prints no default result metadata: {stdout}"
    );

    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
    let _ = std::fs::remove_dir_all(dup.parent().unwrap());
}

#[test]
fn inspect_names_the_detail_query_commands() {
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let output = run(&["inspect", path.to_str().unwrap()]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    for pointer in [
        "wright inspect symbols",
        "wright inspect refs <NAME>",
        "wright inspect cfg <RULE>",
        "wright inspect callgraph",
        "wright inspect cost",
    ] {
        assert!(stdout.contains(pointer), "{pointer}: {stdout}");
    }
    // The program inventory leads; the detail commands close (#446).
    assert!(
        stdout.find("Program overview").unwrap() < stdout.find("Detail commands").unwrap(),
        "{stdout}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn inspect_names_unnamed_rules_by_index() {
    // An unnamed rule has no name to address it by; the preview must carry
    // its index instead of repeating indistinguishable `<unnamed>` lines
    // (#446, real-world rule ("") blocks such as overpy-pixelart).
    let path = temp_file(
        "unnamed.ws",
        "rule (\"\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Wait(1);\n    }\n}\n",
    );
    let output = run(&["inspect", path.to_str().unwrap()]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\n  rule 0\n"), "{stdout}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn rename_is_an_ordinary_unknown_subcommand() {
    // #780: semantic rename is removed from the CLI — `rename` parses as an
    // unknown subcommand, the same usage error any other misspelling gives.
    let output = run(&["rename", "score", "total"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "usage errors write stderr only");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unrecognized subcommand"), "{stderr}");
}

#[test]
fn inspect_overview_and_cfg_bound_large_detail() {
    // A program larger than one screen: 13 rules, one of them a wide graph.
    // Text output stays bounded and names the complete-output path (#446).
    let mut source = String::new();
    for i in 0..13 {
        let mut branches = String::new();
        for b in 0..4 {
            branches.push_str(&format!(
                "        If(1 == {b});\n            Wait(1);\n        End;\n"
            ));
        }
        source.push_str(&format!(
            "rule (\"r{i}\") {{\n    event {{\n        Ongoing - Global;\n    }}\n    actions {{\n{branches}    }}\n}}\n"
        ));
    }
    let path = temp_file("big.ws", &source);
    let path = path.to_str().unwrap();

    let output = run(&["inspect", path]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("13 rule(s)"), "{stdout}");
    assert!(!stdout.contains("more rule(s)"), "{stdout}");

    let output = run(&["inspect", "cfg", "r0", path]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("14 block(s), 17 edge(s)"),
        "shape summary: {stdout}"
    );
    assert!(!stdout.contains("more block(s)"), "{stdout}");
    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

#[test]
fn agent_install_mcp_configures_a_project_whose_server_answers_mcp() {
    use std::io::{BufRead, BufReader};
    let project = temp_dir();
    let elsewhere = temp_dir();
    let install = |args: &[&str]| {
        Command::new(wright())
            .args(["agent", "install"])
            .args(args)
            .current_dir(&project)
            .stdin(Stdio::null())
            .output()
            .expect("wright runs")
    };

    let output = install(&["--mcp", "claude", "--no-guide"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!project.join(".agents").exists());
    let first = std::fs::read(project.join(".mcp.json")).unwrap();
    assert!(install(&["--mcp", "claude", "--no-guide"]).status.success());
    assert_eq!(std::fs::read(project.join(".mcp.json")).unwrap(), first);
    assert!(install(&["--mcp", "claude"]).status.success());
    assert!(project.join(".agents/skills/wright/SKILL.md").is_file());

    // Start the configured server as the harness would: the entry's command
    // and args with the harness's project-root expansion applied, from a
    // working directory that is not the project.
    let config: serde_json::Value = serde_json::from_slice(&first).unwrap();
    let entry = &config["mcpServers"]["wright"];
    assert_eq!(entry["command"], "wright");
    std::fs::write(
        project.join("session.ws"),
        corpus_workshop("synthetic/control-flow"),
    )
    .unwrap();
    let project_dir = project.to_str().unwrap();
    let args: Vec<String> = entry["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| {
            arg.as_str()
                .unwrap()
                .replace("${CLAUDE_PROJECT_DIR:-.}", project_dir)
        })
        .collect();
    let mut child = Command::new(wright())
        .args(&args)
        .current_dir(&elsewhere)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("configured server spawns");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut exchange = |request: serde_json::Value| -> serde_json::Value {
        writeln!(stdin, "{request}").unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).expect("JSON response")
    };
    let init = exchange(serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}},
    }));
    assert_eq!(init["result"]["serverInfo"]["name"], "wright-tool-service");
    let list = exchange(serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    assert!(
        list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "wright_project")
    );
    let call = exchange(serde_json::json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "wright_project", "arguments": {}},
    }));
    assert!(call["result"].get("isError").is_none(), "{call}");
    drop(stdin);
    let _ = child.wait();

    // Hand removal (delete the `wright` entry) is the documented removal; a
    // differing entry is then refused without --force.
    let mut config = config;
    config["mcpServers"]["wright"]["command"] = "other".into();
    std::fs::write(project.join(".mcp.json"), config.to_string()).unwrap();
    let refused = install(&["--mcp", "claude", "--no-guide"]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        install(&["--mcp", "claude", "--no-guide", "--force"])
            .status
            .success()
    );
    config["mcpServers"] = serde_json::json!({});
    std::fs::write(project.join(".mcp.json"), config.to_string()).unwrap();
    assert!(install(&["--mcp", "claude", "--no-guide"]).status.success());

    let output = install(&["--mcp", "emacs"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("claude"));
    assert_eq!(install(&["--no-guide"]).status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&project);
    let _ = std::fs::remove_dir_all(&elsewhere);
}

// The command is the agent contract's `lookup` operation on argv: vocabulary
// answers need no input and never load a project.

#[test]
fn lookup_help_carries_the_agent_guidance() {
    let output = run(&["lookup", "--help"]);
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(
        help.contains("unfamiliar language") && help.contains("when a name is rejected"),
        "help tells agents when to reach for lookup: {help}"
    );
    for option in ["--language", "--kind", "--within", "--locale", "--limit"] {
        assert!(help.contains(option), "lookup help documents {option}");
    }
}

#[test]
fn lookup_resolves_a_display_name_to_a_rendered_signature() {
    let output = run(&["lookup", "create hud text"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[action]"), "{stdout}");
    assert!(stdout.contains("createHudText"), "{stdout}");
    assert!(
        stdout.contains("Create HUD Text("),
        "a callable renders its signature: {stdout}"
    );
}

#[test]
fn lookup_json_matches_the_agent_result() {
    let output = run(&["lookup", "wait", "-f", "json"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "JSON mode: no stderr");
    let result = parse_json(&output.stdout);
    assert_eq!(result["language"], "workshop");
    assert_eq!(result["owner"], "workshop-rs");
    let entries = result["entries"].as_array().unwrap();
    assert!(
        entries
            .iter()
            .any(|entry| entry["signature"] == "Wait(time: Any, waitBehavior=Ignore Condition)"),
        "the Wait signature renders from catalog facts: {result}"
    );
}

#[test]
fn lookup_within_lists_an_enums_members() {
    let output = run(&["lookup", "--within", "Team", "--kind", "enum-member"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("All Teams [enumMember]"), "{stdout}");
    assert!(stdout.contains("Team 1 [enumMember]"), "{stdout}");
}

#[test]
fn lookup_within_rejects_an_unknown_scope() {
    let output = run(&["lookup", "--within", "nope"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("lookup.unknownWithin"), "{stderr}");
}

#[test]
fn lookup_never_reads_stdin() {
    // `lookup` takes no input: a stdin that never reaches EOF (a held-open
    // pipe, like a terminal's) must not block the command — the session's
    // warm load resolving the default `InputSpec::Stdin` would read stdin
    // to EOF before the lookup ever runs.
    let mut child = Command::new(wright())
        .args(["lookup", "create hud text"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("wright runs");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if child.try_wait().expect("child status").is_some() {
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            panic!("wright lookup blocked on an open stdin");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let output = child.wait_with_output().expect("child output");
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("createHudText"), "{stdout}");
}

#[test]
fn lookup_names_opy_rs_when_the_provider_cannot_answer() {
    // A provider that cannot run never produces a Wright-side guess: the
    // result is the explicit `unavailable` payload naming the owner.
    let output = run(&[
        "lookup",
        "--language",
        "opy",
        "--opy-provider",
        "/nonexistent",
        "wait",
        "-f",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let result = parse_json(&output.stdout);
    assert_eq!(result["owner"], "opy-rs");
    assert_eq!(result["entries"], serde_json::json!([]));
    assert_eq!(result["unavailable"]["capability"], "lookup");
}

#[test]
fn check_text_points_unknown_names_at_lookup() {
    // ADR-0021 decision 6: a rejected name in text output ends with a
    // one-line pointer to the lookup surface.
    let path = temp_file("lookup-hint.txt", MIXED_CHECK_SOURCE);
    let output = run(&[
        "check",
        path.to_str().unwrap(),
        "--renderer",
        "plain",
        "--color",
        "never",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("hint: run 'wright lookup'"),
        "the unknown-value diagnostic carries the pointer: {stderr}"
    );
    assert!(
        !stderr.contains("--language"),
        "a Workshop source keeps the default-vocabulary hint: {stderr}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// wright#753: a rejected value in a non-default Workshop locale names its
/// file and line, and the hint carries the locale `lookup` needs to resolve it.
/// `无需装弹: 禁用` stays rejected across workshop-rs versions; the
/// secondary-fire key accepts `禁用` starting with workshop-rs 1.16.9.
#[test]
fn check_text_locates_unknown_setting_value_and_names_the_locale() {
    let path = temp_file(
        "lookup-hint-zh.ow",
        "设置\n{\n\t英雄\n\t{\n\t\t队伍1\n\t\t{\n\t\t\t索杰恩\n\t\t\t{\n\t\t\t\t无需装弹: 禁用\n\t\t\t}\n\t\t}\n\t}\n}\n",
    );
    let output = run(&[
        "check",
        path.to_str().unwrap(),
        "--kind",
        "workshop",
        "--locale",
        "zh-CN",
        "--renderer",
        "plain",
        "--color",
        "never",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("lookup-hint-zh.ow:9:"),
        "the rejected value carries its file and line: {stderr}"
    );
    assert!(
        stderr.contains("hint: run 'wright lookup --locale zh-CN'"),
        "the hint names the locale of the rejected name: {stderr}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// #692: `lookup` defaults to the Workshop vocabulary, so an OPY rejected
/// name must point at `--language opy`. The provider is a staged LPP script
/// whose `lpp/check` returns the unknown-action diagnostic an OPY check
/// reports — the driver stamps its `source` origin `opy` (#638), and the
/// hint selects on that origin.
#[cfg(unix)]
#[test]
fn check_text_points_opy_unknown_names_at_the_opy_vocabulary() {
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
        reply(id, {"protocolVersion": request["params"]["protocolVersion"],
                   "serverInfo": {"name": "fake", "version": "0"},
                   "languages": [{"id": "opy", "extensions": ["opy"]}],
                   "capabilities": {"check": True, "compile": False, "reconstruct": False, "symbols": False, "definition": False, "references": False, "rename": False, "editValidation": False, "projectLoading": True}})
    elif method == "lpp/check":
        reply(id, {"documents": [{"uri": request["params"]["entry"]["uri"], "version": 1, "diagnostics": [
            {"range": {"start": {"line": 2, "character": 4}, "end": {"line": 2, "character": 134}},
             "severity": "error", "code": "unknown-action",
             "message": "unknown action 'createHudText' (did you mean 'hudText'?)"}]}]})
    else:
        reply(id, {})
"#;

    let dir = temp_dir();
    let script = dir.join("fake-provider");
    std::fs::write(&script, PROVIDER).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let input = dir.join("main.opy");
    std::fs::write(
        &input,
        "rule \"t\":\n    @Event global\n    createHudText(getAllPlayers())\n",
    )
    .unwrap();
    let output = run(&[
        "check",
        input.to_str().unwrap(),
        "--opy-provider",
        script.to_str().unwrap(),
        "--renderer",
        "plain",
        "--color",
        "never",
    ]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("hint: run 'wright lookup --language opy'"),
        "an OPY rejected name points at the OPY vocabulary: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn check_text_skips_the_lookup_hint_when_no_name_was_rejected() {
    // `unknown-language` fires when the input holds no Workshop language
    // markers at all — detection rejected no name, so lookup has nothing
    // to resolve and the pointer must not appear.
    let path = temp_file("lookup-hint-none.txt", "0.11.0\n");
    let output = run(&[
        "check",
        path.to_str().unwrap(),
        "--renderer",
        "plain",
        "--color",
        "never",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown-language"),
        "the detection diagnostic still reports: {stderr}"
    );
    assert!(
        !stderr.contains("wright lookup"),
        "no rejected name means no lookup pointer: {stderr}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn check_and_compile_json_keep_their_envelope_after_unknown_names() {
    // The hint lives only in text presentation: JSON mode's envelope and
    // stderr are untouched for the same rejected-name input.
    let path = temp_file("lookup-hint-json.txt", MIXED_CHECK_SOURCE);
    for command in ["check", "compile"] {
        let output = run(&[command, path.to_str().unwrap(), "-f", "json"]);
        assert_eq!(output.status.code(), Some(1), "{command}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("wright lookup"),
            "{command} JSON mode carries no hint: {stderr}"
        );
        let envelope = parse_json(&output.stdout);
        assert!(
            !envelope.to_string().contains("wright lookup"),
            "{command} payload is unchanged: {envelope}"
        );
    }
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
