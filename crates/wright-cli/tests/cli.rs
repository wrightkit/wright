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

/// A program over the Overwatch client element limit (#488) still emits its
/// artifact on stdout in text mode, with the import-limit warning on stderr;
/// in JSON mode the warning rides inside the envelope's diagnostics.
#[test]
fn compile_over_element_limit_warns_and_still_emits() {
    let array = (0..17_000).map(|_| "1").collect::<Vec<_>>().join(", ");
    let source = format!(
        "variables {{\n    global:\n        0: values\n}}\n\nrule (\"fill\") {{\n    event {{\n        Ongoing - Global;\n    }}\n    actions {{\n        Set Global Variable(values, Array({array}));\n    }}\n}}\n"
    );
    let path = temp_file("over-limit.txt", &source);

    let output = run(&["compile", path.to_str().unwrap()]);
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

    let json = run(&["compile", path.to_str().unwrap(), "-f", "json"]);
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
        envelope["result"]["output"]["text"]
            .as_str()
            .is_some_and(|text| text.contains("Set Global Variable")),
        "the JSON envelope still carries the artifact"
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
    // A small clean program still prints the bounded frame and an explicit
    // empty risk section rather than fabricating concerns.
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

// ── Check output hierarchy (#443) ────────────────────────────────────────────
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
    // The verdict leads stdout: failure status plus the blocking error count
    // and the non-blocking remainder, then only execution metadata closes it.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    assert_eq!(lines.next(), Some("ERROR check"), "{stdout}");
    let counts = lines.next().unwrap_or_default().to_string();
    assert!(counts.contains("1 error(s)"), "{stdout}");
    assert!(counts.contains("2 warning(s)"), "{stdout}");
    assert_eq!(lines.next(), Some("  1 file(s) affected"), "{stdout}");
    assert!(lines.next().is_none(), "the footer stays compact: {stdout}");

    // Diagnostics render errors before non-blocking diagnostics, each with
    // its actionable location, source context, and secondary stage metadata.
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
fn check_text_reorder_never_reaches_the_json_envelope() {
    let path = temp_file("mixed.txt", MIXED_CHECK_SOURCE);
    let output = run(&["check", path.to_str().unwrap(), "-f", "json"]);
    assert_eq!(output.status.code(), Some(1));
    let envelope = parse_json(&output.stdout);
    // JSON preserves the driver's production order and the same diagnostic
    // set; only the text layer reorders by severity.
    let severities: Vec<&str> = envelope["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| diagnostic["severity"].as_str().unwrap())
        .collect();
    assert_eq!(severities, ["warning", "warning", "error"]);
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
    let cost = &envelope["result"]["facts"]["cost"];
    assert!(
        cost["elementCount"].as_u64().is_some_and(|total| total > 0),
        "analyze reports the canonical element count (#445): {cost}"
    );
    assert!(
        cost["counts"]["actions"].as_u64().is_some(),
        "analyze reports structural counts (#445): {cost}"
    );
    let rule = &envelope["result"]["facts"]["rules"][0];
    assert!(
        rule["elements"].as_u64().is_some(),
        "analyze attributes element cost to rules (#445): {rule}"
    );
    assert!(
        envelope["result"]["facts"]["risks"].is_array(),
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
    // the verdict still reports the true total by severity.
    assert!(stdout.contains("10 warning(s)"), "{stdout}");
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
    assert!(stdout.contains("(9 findings)"), "{stdout}");
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
    // The message leads; the location and source frame follow; the evidence
    // class trails dimmed as secondary metadata.
    assert!(stdout.contains("WARN lint"), "{stdout}");
    assert!(stdout.contains("1 warning(s) across 6 rule(s)"), "{stdout}");
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
        "compile", "convert", "check", "analyze", "lint", "inspect", "agent",
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

    // --dry-run reports the destination and writes nothing.
    let _ = std::fs::remove_dir_all(&dir);
    let output = install(&["--dry-run"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("would install"));
    assert!(!target.exists());

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
    assert!(
        stdout.contains("... 8 more reference(s) (--format json prints the complete result)"),
        "{stdout}"
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
    let calls: String = (0..12)
        .map(|i| format!("        Call Subroutine(sub{i});\n"))
        .collect();
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
    assert!(
        stdout.contains("... 2 more subroutine(s) (--format json prints the complete result)"),
        "{stdout}"
    );
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
    // Repeated identical findings collapse into one counted entry (#446).
    assert!(
        stdout.contains("repeated-value] (9 occurrences)"),
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
fn rename_previews_a_validated_diff_without_writing() {
    // #434: `wright rename` defaults to a source diff; the file stays
    // untouched and no temporary file leaks next to it.
    let original = corpus_workshop("synthetic/declarations-numbers");
    let path = temp_file("rename.ws", &original);
    let output = run(&[
        "rename",
        "score",
        "total",
        path.to_str().unwrap(),
        "--kind",
        "workshop",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PASS rename"), "{stdout}");
    assert!(stdout.contains("-         0: score"), "{stdout}");
    assert!(stdout.contains("+         0: total"), "{stdout}");
    assert!(
        stdout.contains("--write"),
        "the preview names the apply flag"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    assert_eq!(
        path.parent().unwrap().read_dir().unwrap().count(),
        1,
        "no temporary sibling remains"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn inspect_overview_and_cfg_bound_large_detail() {
    // A program larger than one screen: 13 rules, one of them a wide graph.
    // Text output stays bounded and names the complete-output path (#446).
    let mut source = String::new();
    for i in 0..13 {
        let branches = (0..4)
            .map(|b| format!("        If(1 == {b});\n            Wait(1);\n        End;\n"))
            .collect::<String>();
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
    assert!(
        stdout.contains("... 3 more rule(s) (--format json prints the complete result)"),
        "{stdout}"
    );

    let output = run(&["inspect", "cfg", "r0", path]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("14 block(s), 17 edge(s)"),
        "shape summary: {stdout}"
    );
    assert!(
        stdout.contains("... 4 more block(s) (--format json prints the complete result)"),
        "{stdout}"
    );
    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

#[test]
fn rename_write_applies_the_validated_change_atomically() {
    // #434: --write replaces the input; the result reparses cleanly.
    let path = temp_file(
        "rename.ws",
        &corpus_workshop("synthetic/declarations-numbers"),
    );
    let path_str = path.to_str().unwrap();
    let output = run(&[
        "rename", "score", "total", path_str, "--kind", "workshop", "--write",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&format!("wrote {path_str}")), "{stdout}");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("0: total"), "{text}");
    assert!(text.contains("Set Global Variable(total, 5)"), "{text}");
    assert!(!text.contains("score"), "{text}");
    let check = run(&["check", path_str, "--kind", "workshop"]);
    assert!(check.status.success(), "{}", command_result(&check));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn rename_reports_the_envelope_in_json_mode() {
    let path = temp_file(
        "rename.ws",
        &corpus_workshop("synthetic/declarations-numbers"),
    );
    let output = run(&[
        "rename",
        "score",
        "total",
        path.to_str().unwrap(),
        "--kind",
        "workshop",
        "-f",
        "json",
    ]);
    assert!(output.status.success(), "{}", command_result(&output));
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["command"], "rename");
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["wright"]["contract"], "wright-result/v1");
    assert!(
        envelope["result"]["transaction"]["edits"]
            .as_array()
            .unwrap()
            .len()
            >= 2,
        "{envelope}"
    );
    assert!(
        envelope["result"]["preview"][0]["new_text"]
            .as_str()
            .unwrap()
            .contains("0: total"),
        "{envelope}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn rename_refusals_carry_structured_diagnostics_and_write_nothing() {
    // #434: unknown names, collisions, and non-Workshop input refuse with the
    // structured codes; --write still writes nothing.
    let original = corpus_workshop("synthetic/declarations-numbers");
    let path = temp_file("rename.ws", &original);
    for (name, code) in [
        ("missing", "unknown-symbol"),
        ("numbers", "rename-unsupported-kind"),
    ] {
        let output = run(&[
            "rename",
            name,
            "renamed",
            path.to_str().unwrap(),
            "--kind",
            "workshop",
            "-f",
            "json",
            "--write",
        ]);
        assert_eq!(output.status.code(), Some(1), "{name}");
        let envelope = parse_json(&output.stdout);
        assert_eq!(envelope["diagnostics"][0]["code"], code, "{envelope}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    // OPY input routes to the provider operation rather than renaming
    // through the Workshop path.
    let opy = temp_file("program.opy", "rule \"r\":\n    pass\n");
    let output = run(&[
        "rename",
        "r",
        "renamed",
        opy.to_str().unwrap(),
        "--kind",
        "opy",
        "-f",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["diagnostics"][0]["code"], "edit-requires-provider");
    assert!(
        envelope["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("providerSemanticRename"),
        "{}",
        envelope["diagnostics"][0]["message"]
    );
    assert_eq!(
        std::fs::read_to_string(&opy).unwrap(),
        "rule \"r\":\n    pass\n"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
    let _ = std::fs::remove_dir_all(opy.parent().unwrap());
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

    // An unsupported target and `--no-guide` alone are usage errors.
    let output = install(&["--mcp", "emacs"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("claude"));
    assert_eq!(install(&["--no-guide"]).status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&project);
    let _ = std::fs::remove_dir_all(&elsewhere);
}
