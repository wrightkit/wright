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
    let oracle = std::fs::read_to_string(
        workspace_root()
            .join("compatibility/fixtures")
            .join(fixture_id)
            .join("oracle.json"),
    )
    .unwrap();
    serde_json::from_str::<serde_json::Value>(&oracle).unwrap()["compile"]["workshop"]
        .as_str()
        .unwrap()
        .to_string()
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
    assert!(String::from_utf8_lossy(&output.stdout).contains("check: ok"));
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
fn check_reports_analysis_findings_as_diagnostics() {
    let path = temp_file("flow.txt", &corpus_workshop("synthetic/control-flow"));
    let output = run(&["check", path.to_str().unwrap(), "-f", "json"]);
    assert_eq!(output.status.code(), Some(0), "warnings do not fail check");
    let envelope = parse_json(&output.stdout);
    assert!(
        envelope["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic["code"] == "min-wait-loop")
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn analyze_over_workshop_input_reports_findings_with_spans() {
    let path = temp_file("flow.txt", &corpus_workshop("synthetic/control-flow"));
    let output = run(&["analyze", path.to_str().unwrap(), "-f", "json"]);
    assert!(output.status.success());
    let envelope = parse_json(&output.stdout);
    let findings = envelope["result"]["findings"].as_array().unwrap();
    assert!(
        findings
            .iter()
            .any(|finding| finding["code"] == "min-wait-loop"),
        "findings: {findings:?}"
    );
    for finding in findings {
        assert!(finding["span"].is_object(), "findings carry spans");
        let path = finding["span"]["path"]
            .as_str()
            .expect("findings carry a resolved span path");
        assert!(!path.is_empty(), "the resolved span path is non-empty");
    }
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
    assert!(stdout.contains("lint:"), "summary line: {stdout}");
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
    assert_eq!(
        envelope["result"]["rules"].as_array().unwrap().len(),
        5,
        "all five first-party rules are reported"
    );
    assert_eq!(
        envelope["result"]["config"]["rules"]
            .as_object()
            .unwrap()
            .len(),
        5
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn span_path_is_consistent_across_input_spellings() {
    // The issue's repro (#102): lint resolves the same root-relative
    // `span.path` for the absolute, bare-name (cwd), and dir-relative
    // spellings of the same file, and `analyze` reports the identical value.
    // Each subprocess gets its own cwd, so the bare-name spelling is
    // exercised end-to-end exactly as in the issue.
    let dir = temp_dir();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(
        dir.join("sub").join("loop.opy"),
        "rule \"loop\":\n    @Event eachPlayer\n    while (true):\n        wait(0.016)\n",
    )
    .unwrap();

    let absolute = run(&[
        "lint",
        dir.join("sub").join("loop.opy").to_str().unwrap(),
        "-f",
        "json",
    ]);
    assert!(
        absolute.status.success(),
        "{}",
        String::from_utf8_lossy(&absolute.stderr)
    );
    let absolute_path = parse_json(&absolute.stdout)["result"]["findings"][0]["span"]["path"]
        .as_str()
        .unwrap()
        .to_string();

    let bare = Command::new(wright())
        .args(["lint", "loop.opy", "-f", "json"])
        .current_dir(dir.join("sub"))
        .stdin(Stdio::null())
        .output()
        .expect("wright runs");
    assert!(
        bare.status.success(),
        "{}",
        String::from_utf8_lossy(&bare.stderr)
    );
    let bare_path = parse_json(&bare.stdout)["result"]["findings"][0]["span"]["path"]
        .as_str()
        .unwrap()
        .to_string();

    let relative = Command::new(wright())
        .args(["lint", "sub/loop.opy", "-f", "json"])
        .current_dir(&dir)
        .stdin(Stdio::null())
        .output()
        .expect("wright runs");
    assert!(
        relative.status.success(),
        "{}",
        String::from_utf8_lossy(&relative.stderr)
    );
    let relative_path = parse_json(&relative.stdout)["result"]["findings"][0]["span"]["path"]
        .as_str()
        .unwrap()
        .to_string();

    let analyze = Command::new(wright())
        .args(["analyze", "loop.opy", "-f", "json"])
        .current_dir(dir.join("sub"))
        .stdin(Stdio::null())
        .output()
        .expect("wright runs");
    assert!(
        analyze.status.success(),
        "{}",
        String::from_utf8_lossy(&analyze.stderr)
    );
    let analyze_path = parse_json(&analyze.stdout)["result"]["findings"][0]["span"]["path"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(
        absolute_path, "loop.opy",
        "the absolute spelling resolves to the root-relative basename"
    );
    assert_eq!(
        bare_path, absolute_path,
        "the bare-name (cwd) spelling must agree with the absolute spelling"
    );
    assert_eq!(
        relative_path, absolute_path,
        "the dir-relative spelling must agree with the absolute spelling"
    );
    assert_eq!(
        analyze_path, absolute_path,
        "analyze must report the same span.path as lint"
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
    let rules = envelope["result"]["rules"].as_array().unwrap();
    let min_wait = rules
        .iter()
        .find(|rule| rule["id"] == "min-wait-loop")
        .unwrap();
    assert_eq!(min_wait["enabled"], false);

    // --rule-severity overrides the effective severity of a rule. The
    // control-flow fixture produces no expensive-loop-check findings, so
    // the assertion is on the rules metadata.
    let output = run(&[
        "lint",
        path.to_str().unwrap(),
        "--rule-severity",
        "expensive-loop-check:warning",
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

#[test]
fn stdin_workshop_and_protocol_piping_work() {
    // Workshop text on stdin.
    let output = run_with_stdin(&["check", "-"], &corpus_workshop("synthetic/basic-rule"));
    assert_eq!(output.status.code(), Some(0));

    // Protocol JSON on stdin (auto-detected by leading `{`).
    let protocol = std::fs::read_to_string(
        workspace_root().join("adapter/fixtures/synthetic/basic-rule.json"),
    )
    .unwrap();
    let output = run_with_stdin(&["check", "-"], &protocol);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdin protocol: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn stdin_opy_compiles_natively() {
    let source = std::fs::read_to_string(
        workspace_root().join("compatibility/fixtures/synthetic/basic-rule/source.opy"),
    )
    .unwrap();
    let output = run_with_stdin(&["compile", "-", "--kind", "opy", "-f", "json"], &source);
    assert_eq!(
        output.status.code(),
        Some(0),
        "native .opy stdin: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["ok"], true);
    assert!(
        envelope["result"]["output"]["text"]
            .as_str()
            .unwrap()
            .contains("Disable Inspector Recording")
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
    assert!(String::from_utf8_lossy(&output.stdout).contains("check: ok"));
    // JSON mode: envelope on stdout only.
    let output = run(&["check", path.to_str().unwrap(), "-f", "json"]);
    assert!(output.stderr.is_empty());
    parse_json(&output.stdout);
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

    for target in ["opy", "ostw"] {
        let fixture = if target == "opy" {
            workspace_fixture(
                "crates/wright-opy/tests/fixtures/reconstruct/variables-declarations.ws",
            )
        } else {
            workspace_fixture("compatibility/ostw/reconstruction/surface-basic/workshop.txt")
        };
        let expected = parse_json(
            &run(&["convert", "--target", target, &fixture, "-f", "json"]).stdout,
        )["result"]["text"]
            .as_str()
            .unwrap()
            .as_bytes()
            .to_vec();
        for renderer in ["plain", "github-actions"] {
            let output = run_with_env(
                &[
                    "convert",
                    "--target",
                    target,
                    &fixture,
                    "--renderer",
                    renderer,
                ],
                &[("GITHUB_ACTIONS", "true")],
            );
            assert_eq!(output.status.code(), Some(0), "{target}/{renderer}");
            assert_eq!(
                output.stdout, expected,
                "{target}/{renderer} must preserve bytes"
            );
        }
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
fn opy_file_compiles_through_the_native_frontend() {
    let source = std::fs::read_to_string(
        workspace_root().join("compatibility/fixtures/synthetic/basic-rule/source.opy"),
    )
    .unwrap();
    let path = temp_file("basic-rule.opy", &source);
    let output = run(&["compile", path.to_str().unwrap(), "-f", "json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["ok"], true);
    let oracle = std::fs::read_to_string(
        workspace_root().join("compatibility/fixtures/synthetic/basic-rule/oracle.json"),
    )
    .unwrap();
    let oracle_value: serde_json::Value = serde_json::from_str(&oracle).unwrap();
    let expected = oracle_value["compile"]["workshop"].as_str().unwrap();
    assert_eq!(
        envelope["result"]["output"]["text"]
            .as_str()
            .unwrap()
            .trim(),
        expected.trim(),
        "native .opy output matches the oracle Workshop text"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn opy_corpus_frontend_errors_are_structured() {
    // Malformed `.opy` fails with a structured frontend diagnostic, not a
    // panic, and does not fall back to the adapter silently.
    let path = temp_file("broken.opy", "rule \"missing colon\"\n    @Event global\n");
    let output = run(&["check", path.to_str().unwrap(), "-f", "json"]);
    assert_eq!(output.status.code(), Some(1));
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["diagnostics"][0]["code"], "parse-error");
    assert_eq!(envelope["diagnostics"][0]["stage"], "frontend");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn opy_chase_reevaluation_enums_compile_with_reference_semantics() {
    // #105: ChaseTimeReeval.NONE (and the other reference-validated members
    // of the ChaseTimeReeval/ChaseRateReeval domains) lower through the enum
    // catalog data path and emit with the same Workshop value as the pinned
    // oracle — `None`/`Destination and Duration`/`Destination and Rate`,
    // distinct from the `Null` literal.
    let fixture = workspace_root().join("compatibility/fixtures/synthetic/chase-enums");
    let source = std::fs::read_to_string(fixture.join("source.opy")).unwrap();
    let path = temp_file("chase-enums.opy", &source);
    let output = run(&["compile", path.to_str().unwrap(), "-f", "json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["ok"], true);
    let oracle = std::fs::read_to_string(fixture.join("oracle.json")).unwrap();
    let oracle_value: serde_json::Value = serde_json::from_str(&oracle).unwrap();
    let expected = oracle_value["compile"]["workshop"].as_str().unwrap();
    let emitted = envelope["result"]["output"]["text"].as_str().unwrap();
    assert_eq!(
        emitted.trim(),
        expected.trim(),
        "native .opy output matches the oracle Workshop text"
    );
    assert!(
        emitted.contains("None"),
        "the NONE member emits its spelling: {emitted}"
    );
    assert!(emitted.contains("Destination and Duration"), "{emitted}");
    assert!(emitted.contains("Destination and Rate"), "{emitted}");
    assert!(
        emitted.contains("Set Global Variable(time_reeval, None)"),
        "the enum member emits as a bare spelling, not the Null literal: {emitted}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn opy_unknown_enum_member_is_a_deterministic_frontend_diagnostic() {
    // #105: enum members outside the evidenced catalog surface keep failing
    // with a deterministic, source-located structured diagnostic.
    let path = temp_file(
        "unknown-member.opy",
        "globalvar g\nrule \"r\":\n    @Event global\n    g = ChaseTimeReeval.NOPE\n",
    );
    let output = run(&["check", path.to_str().unwrap(), "-f", "json"]);
    assert_eq!(output.status.code(), Some(1));
    let envelope = parse_json(&output.stdout);
    let diagnostic = &envelope["diagnostics"][0];
    assert_eq!(diagnostic["code"], "unknown-enum-member");
    assert_eq!(diagnostic["stage"], "frontend");
    assert!(
        diagnostic["span"].is_object(),
        "the diagnostic is source-located"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

// ── Convert (#126) ───────────────────────────────────────────────────────────

fn workspace_fixture(relative: &str) -> String {
    workspace_root().join(relative).display().to_string()
}

#[test]
fn convert_workshop_input_to_opy_reconstructs_source() {
    // `wright convert --target opy` over a committed Workshop fixture writes
    // the reconstructed OPY source (not a success banner) to stdout.
    let fixture =
        workspace_fixture("crates/wright-opy/tests/fixtures/reconstruct/variables-declarations.ws");
    let output = run(&["convert", "--target", "opy", &fixture]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    for anchor in ["globalvar", "playervar", "rule \"", "@Event"] {
        assert!(
            stdout.contains(anchor),
            "missing OPY anchor {anchor:?}:\n{stdout}"
        );
    }
}

#[test]
fn convert_workshop_input_to_ostw_reconstructs_source() {
    // `wright convert --target ostw` writes the reconstructed OSTW source.
    let fixture = workspace_fixture("compatibility/ostw/reconstruction/surface-basic/workshop.txt");
    let output = run(&["convert", "--target", "ostw", &fixture]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    for anchor in ["globalvar Any", "playervar Any", "rule: \"", "void "] {
        assert!(
            stdout.contains(anchor),
            "missing OSTW anchor {anchor:?}:\n{stdout}"
        );
    }
}

#[test]
fn convert_json_envelope_reports_command_result_and_target() {
    let fixture =
        workspace_fixture("crates/wright-opy/tests/fixtures/reconstruct/variables-declarations.ws");
    let output = run(&["convert", "--target", "opy", &fixture, "-f", "json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "JSON mode keeps stderr clean");
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["exit"], 0);
    assert_eq!(envelope["command"], "convert");
    assert_eq!(envelope["wright"]["contract"], "wright-result/v1");
    assert_eq!(envelope["result"]["target"], "opy");
    let text = envelope["result"]["text"].as_str().unwrap();
    assert!(
        text.contains("rule \"") && text.contains("@Event"),
        "{text}"
    );
    assert_eq!(
        envelope["result"]["sha256"].as_str().unwrap().len(),
        64,
        "the reconstructed source carries its SHA-256"
    );
    // The envelope text is exactly the reconstructed source.
    assert!(text.starts_with("globalvar "), "{text}");
}

#[test]
fn convert_is_byte_deterministic_across_runs() {
    for (target, fixture) in [
        (
            "opy",
            "crates/wright-opy/tests/fixtures/reconstruct/variables-declarations.ws",
        ),
        (
            "ostw",
            "compatibility/ostw/reconstruction/surface-basic/workshop.txt",
        ),
    ] {
        let fixture = workspace_fixture(fixture);
        let first = run(&["convert", "--target", target, &fixture, "-f", "json"]);
        let second = run(&["convert", "--target", target, &fixture, "-f", "json"]);
        assert_eq!(
            first.stdout, second.stdout,
            "convert --target {target} must be byte-deterministic"
        );
        assert!(!first.stdout.is_empty());
    }
}

#[test]
fn convert_rejects_unsupported_constructs_with_exit_three() {
    // Non-representable Workshop constructs fail deterministically with the
    // reconstructor's stable code, the documented unsupported exit code (3),
    // and no partial source — identically on every run.
    for (target, fixture, expected_code) in [
        (
            "ostw",
            "compatibility/ostw/reconstruction/reject/for-player-variable/workshop.txt",
            "reconstruct-unsupported-action",
        ),
        (
            "opy",
            "crates/wright-driver/tests/fixtures/convert/reject-opy-per-player-loop.ws",
            "unsupported-per-player-loop",
        ),
    ] {
        let fixture = workspace_fixture(fixture);
        let first = run(&["convert", "--target", target, &fixture, "-f", "json"]);
        let second = run(&["convert", "--target", target, &fixture, "-f", "json"]);
        assert_eq!(
            first.stdout, second.stdout,
            "convert --target {target} rejection must be deterministic"
        );
        for output in [&first, &second] {
            assert_eq!(
                output.status.code(),
                Some(3),
                "--target {target}: recognized-but-unsupported must exit 3"
            );
            assert!(output.stderr.is_empty(), "JSON mode keeps stderr clean");
            let envelope = parse_json(&output.stdout);
            assert_eq!(envelope["ok"], false);
            assert_eq!(envelope["exit"], 3);
            assert_eq!(envelope["command"], "convert");
            let codes: Vec<&str> = envelope["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .map(|diagnostic| diagnostic["code"].as_str().unwrap())
                .collect();
            assert!(
                codes.contains(&expected_code),
                "--target {target}: expected code {expected_code} in {codes:?}"
            );
            assert!(
                envelope["diagnostics"][0]["stage"] == "reconstruction",
                "rejections carry the reconstruction stage"
            );
            assert!(
                envelope["result"]["text"].as_str().unwrap().is_empty(),
                "a rejection must never carry partial source"
            );
        }
    }
}

#[test]
fn convert_requires_an_explicit_target_flag() {
    // Missing or unknown --target is a usage error (exit 2); --target on
    // another command is a usage error too.
    let fixture =
        workspace_fixture("crates/wright-opy/tests/fixtures/reconstruct/variables-declarations.ws");
    let output = run(&["convert", &fixture]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "usage errors write stderr only");
    assert!(String::from_utf8_lossy(&output.stderr).contains("--target"));

    let output = run(&["convert", "--target", "nope", &fixture]);
    assert_eq!(output.status.code(), Some(2));

    let output = run(&["check", "--target", "opy", &fixture]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn convert_rejects_non_workshop_input() {
    // The conversion surface is declared over Workshop input only; an `.opy`
    // input fails with the structured convert-input-kind diagnostic (exit 1),
    // not a direct OPY ↔ OSTW conversion.
    let source = std::fs::read_to_string(
        workspace_root().join("compatibility/fixtures/synthetic/basic-rule/source.opy"),
    )
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
    assert_eq!(output.status.code(), Some(1));
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["diagnostics"][0]["code"], "convert-input-kind");
    assert!(
        envelope["result"]["text"].as_str().unwrap().is_empty(),
        "no source on a rejected input kind"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
