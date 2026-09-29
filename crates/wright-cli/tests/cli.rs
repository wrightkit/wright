//! Black-box CLI end-to-end tests (#41): the actual `wright` executable is
//! exercised across commands, inputs, output modes, exit codes, diagnostics,
//! and stdout/stderr separation — the automation contract of the CLI.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The path of the `wright` binary under test.
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
    let output = run(&["compile", path.to_str().unwrap()]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty(), "stderr clean on success");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Disable Inspector Recording"), "{stdout}");
    assert!(stdout.contains("Ongoing - Global"));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn compile_writes_output_file_and_reports_envelope() {
    let path = temp_file("basic.txt", &corpus_workshop("synthetic/basic-rule"));
    let out_path = temp_file("emitted.txt", "");
    let text_output = run(&[
        "compile",
        path.to_str().unwrap(),
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert!(text_output.status.success());
    assert!(
        text_output.stdout.is_empty(),
        "-o keeps artifacts off stdout"
    );
    assert!(text_output.stderr.is_empty());

    let output = run(&[
        "compile",
        path.to_str().unwrap(),
        "-o",
        out_path.to_str().unwrap(),
        "-f",
        "json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "JSON mode keeps stderr clean");
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["exit"], 0);
    assert_eq!(envelope["command"], "compile");
    assert_eq!(envelope["wright"]["contract"], "wright-result/v1");
    assert_eq!(
        envelope["result"]["output"]["written_to"].as_str().unwrap(),
        out_path.to_str().unwrap()
    );
    let stored = std::fs::read_to_string(&out_path).unwrap();
    assert!(stored.contains("Disable Inspector Recording"));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
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
        ("analyze", "PASS analyze", "Program overview"),
        ("inspect", "PASS inspect", "Program structure"),
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
    assert!(stdout.contains("Control-flow summary"));
    assert!(stdout.contains("Top rules (heuristic ranking"));
    assert!(stdout.contains("State and coupling"));
    assert!(stdout.contains("[static]"));
    assert!(
        stdout.lines().count() <= 40,
        "report is not bounded:\n{stdout}"
    );
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

#[test]
fn analyze_over_workshop_input_reports_semantic_facts() {
    let path = temp_file("flow.txt", &corpus_workshop("synthetic/control-flow"));
    let output = run(&["analyze", path.to_str().unwrap(), "-f", "json"]);
    assert!(output.status.success());
    let envelope = parse_json(&output.stdout);
    assert!(envelope["result"]["program"]["findings"].is_null());
    assert!(
        !envelope["result"]["facts"]["symbols"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !envelope["result"]["facts"]["rules"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        envelope["result"]["facts"]["persistentObjects"].is_array(),
        "analyze reports persistent-object facts (#429)"
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
    assert!(!envelope["result"]["symbols"].as_array().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

// ── Lint (#98) ───────────────────────────────────────────────────────────────

#[test]
fn lint_over_workshop_input_reports_findings_in_text_and_json() {
    let path = temp_file("flow.txt", &corpus_workshop("synthetic/control-flow"));
    // Text mode: summary line, findings with evidence and source spans.
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
    // JSON mode: the lint envelope with findings, rules, and config.
    let output = run(&["lint", path.to_str().unwrap(), "-f", "json"]);
    assert!(output.status.success());
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["command"], "lint");
    assert_eq!(envelope["ok"], true);
    assert!(
        envelope["result"]["input_identity"].as_str().unwrap().len() == 64,
        "lint carries the SHA-256 input identity"
    );
    let findings = envelope["result"]["findings"].as_array().unwrap();
    assert!(!findings.is_empty(), "control-flow produces findings");
    for finding in findings {
        assert!(finding["evidence"].is_string(), "findings carry evidence");
        assert!(
            finding["span"]["path"].is_string(),
            "finding spans carry the resolved path"
        );
    }
    let rules = envelope["result"]["rules"].as_array().unwrap();
    assert!(!rules.is_empty(), "first-party rules are reported");
    assert_eq!(
        envelope["result"]["config"]["rules"]
            .as_object()
            .unwrap()
            .len(),
        rules.len()
    );
    for rule in rules {
        assert!(rule["id"].is_string() && rule["effectiveSeverity"].is_string());
        assert_eq!(
            rule.as_object().unwrap().len(),
            2,
            "lint inlines only id and effectiveSeverity; full metadata is lintRules (#431): {rule}"
        );
    }
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
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
    let absolute_path = parse_json(&absolute.stdout)["result"]["findings"][0]["span"]["path"]
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
    let bare_path = parse_json(&bare.stdout)["result"]["findings"][0]["span"]["path"]
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
    let relative_path = parse_json(&relative.stdout)["result"]["findings"][0]["span"]["path"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(absolute_path, "loop.txt");
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
    // --disable-rule removes the rule's findings and reports enabled:false.
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
    assert_eq!(
        envelope["result"]["config"]["rules"]["min-wait-loop"]["enabled"],
        false
    );

    // --rule-severity overrides the effective severity of a rule. The
    // control-flow fixture produces no expensive-loop-check findings, so
    // the assertion is on the rules metadata.
    let output = run(&[
        "lint",
        path.to_str().unwrap(),
        "--rule-severity",
        "expensive-loop-check:warn",
        "-f",
        "json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = parse_json(&output.stdout);
    let rules = envelope["result"]["rules"].as_array().unwrap();
    let exp_loop = rules
        .iter()
        .find(|rule| rule["id"] == "expensive-loop-check")
        .unwrap();
    assert_eq!(exp_loop["effectiveSeverity"], "warning");
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
    let rules = envelope["result"]["rules"].as_array().unwrap();
    let local = rules
        .iter()
        .find(|rule| rule["id"] == "community/cli-smoke")
        .expect("every registered rule, including a --rule YAML rule, appears");
    assert_eq!(
        local,
        &serde_json::json!({ "id": "community/cli-smoke", "effectiveSeverity": "warning" }),
        "local rules carry the same compact shape (#431)"
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

// ── Finding selection (#430) ─────────────────────────────────────────────────
// `real-world/overpy-cake.ws` produces 10 findings: 9 identical
// `repeated-value` warnings and 1 `min-wait-loop` warning.

#[test]
fn lint_selection_filters_findings_and_reports_withheld() {
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let path = path.to_str().unwrap();

    // --rule-id selects one rule; --max truncates and reports the withheld
    // count in the envelope.
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
    let reported_path = parse_json(&run(&["lint", path, "-f", "json"]).stdout)
        ["result"]["findings"][0]["span"]["path"]
        .as_str()
        .unwrap()
        .to_string();
    let output = run(&["lint", path, "--file", &reported_path, "-f", "json"]);
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

    // The lint verdict survives an empty selected set on the warning-only
    // fixture: severity=error selects nothing, the verdict still says WARN.
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
fn lint_text_collapses_identical_findings_into_one_entry() {
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let output = run(&["lint", path.to_str().unwrap()]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The nine identical findings render as one entry listing nine locations;
    // the verdict still reports the true total.
    assert!(stdout.contains("10 finding(s)"), "{stdout}");
    assert_eq!(
        stdout.matches("[repeated-value]").count(),
        1,
        "repeated findings collapse into one entry: {stdout}"
    );
    assert_eq!(
        stdout.matches(" --> ").count(),
        10,
        "nine grouped locations plus the single min-wait-loop: {stdout}"
    );
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
fn stdin_workshop_works_and_legacy_protocol_is_refused() {
    // Workshop text on stdin.
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
    // Text mode: result on stdout, no stderr on success.
    let output = run(&["check", path.to_str().unwrap()]);
    assert!(output.stderr.is_empty());
    assert!(String::from_utf8_lossy(&output.stdout).contains("PASS check"));
    // JSON mode: envelope on stdout only.
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
    let output = run(&["version"]);
    assert!(output.status.success());
    let banner = String::from_utf8_lossy(&output.stdout);
    assert!(banner.starts_with("wright "), "{banner}");
    assert!(
        banner.contains(env!("CARGO_PKG_VERSION")),
        "banner does not report the implementation version: {banner}"
    );
    assert!(banner.contains("wright-driver"), "{banner}");

    let output = run(&["--version"]);
    assert!(output.status.success());
    let flag_banner = String::from_utf8_lossy(&output.stdout);
    assert_eq!(flag_banner, banner, "--version matches `version`");

    let output = run(&["--help"]);
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    for command in ["compile", "convert", "check", "analyze", "lint", "inspect"] {
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

    // Idempotent rerun: reports already up to date
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

    // Force rerun: reports updated
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

    let expected = parse_json(&run(&["compile", path.to_str().unwrap(), "-f", "json"])
        .stdout)["result"]["output"]["text"]
        .as_str()
        .unwrap()
        .as_bytes()
        .to_vec();
    for renderer in ["plain", "github-actions"] {
        let output = run_with_env(
            &["compile", path.to_str().unwrap(), "--renderer", renderer],
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

// ── Convert (#126) ───────────────────────────────────────────────────────────

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
    // Missing or unknown --target is a usage error (exit 2); --target on
    // another command is a usage error too.
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

// ── Semantic query commands (#429) ───────────────────────────────────────────
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
    assert!(stdout.contains("globalVariable cakePos"), "{stdout}");
    assert!(stdout.contains("rule cake"), "{stdout}");

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
    let output = run(&["inspect", "symbols", path, "--only", "rule", "-f", "json"]);
    let envelope = parse_json(&output.stdout);
    let symbols = envelope["result"].as_array().unwrap();
    assert_eq!(symbols.len(), 2);
    assert!(symbols.iter().all(|symbol| symbol["kind"] == "rule"));

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
    assert!(
        stdout.contains("16 read(s), 1 write(s)"),
        "the usage header: {stdout}"
    );

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
    assert_eq!(
        references.len(),
        17,
        "16 reads + 1 write; #433 keeps locations approximate"
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
    assert!(stdout.contains("Control-flow graph"), "{stdout}");

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
fn cost_reports_exact_counts_and_selected_findings() {
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let path = path.to_str().unwrap();

    let output = run(&["inspect", "cost", path]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PASS cost"), "{stdout}");
    assert!(stdout.contains("3960 emitted byte(s)"), "{stdout}");
    assert!(stdout.contains("29 action(s)"), "{stdout}");

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

    // Text mode reports the same structured diagnostic on stderr.
    let output = run(&["inspect", "refs", "nope", path]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown-symbol"), "{stderr}");
    assert!(stderr.contains("unknown symbol 'nope'"), "{stderr}");

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
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
