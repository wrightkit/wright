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
        .env_remove("CI")
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITHUB_STEP_SUMMARY")
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR")
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
        stderr.contains("diagnostic: warning target-element-limit"),
        "the limit warning reaches stderr as a record: {stderr}"
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
    wright_driver::sha256_hex(&std::fs::read(path).unwrap())
}

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
    assert!(stdout.contains("output: path=mode.ow"), "{stdout}");
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

#[test]
fn compile_to_emits_the_target_locale_independently_of_the_input() {
    let dir = temp_dir();
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

    let wrong = run_in(
        &dir,
        &[
            "compile", "mode.ws", "--locale", "en-US", "--to", "zh-CN", "-o", "-",
        ],
    );
    assert_eq!(wrong.status.code(), Some(1), "{}", command_result(&wrong));
    let _ = std::fs::remove_dir_all(&dir);
}

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
    let stdout = String::from_utf8_lossy(&missing.stdout);
    assert!(stdout.contains("missing-mapping"), "{stdout}");
    assert!(
        stdout.contains("tr-tr"),
        "the target locale is named: {stdout}"
    );
    assert!(
        !dir.join("teams.ow").exists(),
        "a missing mapping writes no partial file"
    );
    let _ = std::fs::remove_dir_all(&dir);
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
    assert!(stdout.contains("elements [exact]"));
    assert!(stdout.contains("Hotspots"));
    assert!(stdout.contains("Complexity"));
    assert!(stdout.contains("Performance and stability risks"));
    assert!(stdout.contains("State and coupling"));
    assert!(stdout.contains("[static]"));
    assert!(stdout.contains("Next"));
    assert!(
        stdout.lines().count() <= 48,
        "report is not bounded:\n{stdout}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn check_over_malformed_input_exits_one_with_structured_diagnostics() {
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
fn check_unmapped_locations_are_not_fabricated() {
    let output = run_with_stdin(
        &["check", "-", "--renderer", "plain", "--color", "never"],
        "rule (\"x\") { event { Ongoing - Global; } actions { If(True); }",
    );
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("at=<stdin>:1:63"),
        "the unmapped position is one verbatim token: {stdout}"
    );
    assert!(!stdout.contains("-->"), "{stdout}");
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
    assert!(
        stdout.contains("fix: preview duplicate-condition"),
        "{stdout}"
    );
    assert!(stdout.contains("fix: preview repeated-value"), "{stdout}");
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        FIXABLE_WORKSHOP,
        "preview writes nothing"
    );

    let output = run(&["lint", path, "--fix", "--write"]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("fix: applied duplicate-condition"),
        "{stdout}"
    );
    assert!(stdout.contains("fix: applied repeated-value"), "{stdout}");
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
    let output = run(&[
        "lint",
        path.to_str().unwrap(),
        "--rule-severity",
        "min-wait-loop:error",
        "-f",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(1));
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
    assert_eq!(
        envelope["result"]["selection"]["byCode"]["repeated-value"],
        9
    );
    assert!(findings.iter().all(|f| {
        f["repeated"]["expression"].is_string()
            && f["repeated"]["count"]
                .as_u64()
                .is_some_and(|count| count >= 2)
            && matches!(
                f["repeated"]["spanKind"].as_str(),
                Some("expression") | Some("enclosing-loop")
            )
    }));

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
fn selection_never_changes_the_verdict_or_exit_code() {
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
    assert!(stdout.contains("findings=10"), "{stdout}");

    let _ = std::fs::remove_dir_all(broken.parent().unwrap());
    let _ = std::fs::remove_dir_all(cake.parent().unwrap());
}

#[test]
fn fail_on_raises_warn_to_fail_on_every_renderer() {
    let path = temp_file("waitless.txt", WAITLESS_LOOP);
    let path = path.to_str().unwrap();

    let default = run(&["lint", path]);
    assert_eq!(default.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&default.stdout).contains("WARN lint"),
        "{}",
        String::from_utf8_lossy(&default.stdout)
    );

    let gated = run(&["lint", path, "--fail-on", "warning"]);
    assert_eq!(gated.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&gated.stdout);
    assert!(stdout.contains("FAIL lint"), "{stdout}");
    assert!(
        stdout.contains("finding: warning while-without-wait"),
        "{stdout}"
    );

    let terminal = run(&[
        "lint",
        path,
        "--fail-on",
        "warning",
        "--renderer",
        "terminal",
        "--color",
        "never",
    ]);
    assert_eq!(terminal.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&terminal.stdout);
    assert!(stdout.contains(" FAIL  lint"), "{stdout}");

    let gha = run(&[
        "lint",
        path,
        "--fail-on",
        "warning",
        "--renderer",
        "github-actions",
    ]);
    assert_eq!(gha.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&gha.stderr);
    assert!(stderr.contains("FAIL lint"), "{stderr}");
    assert!(stderr.contains("**FAIL** (exit 1)"), "{stderr}");

    let json = run(&["lint", path, "--fail-on", "warning", "-f", "json"]);
    assert_eq!(json.status.code(), Some(1));
    let envelope = parse_json(&json.stdout);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["exit"], 1);
    assert!(envelope.get("gated").is_none());

    let cost = run(&[
        "inspect",
        "cost",
        path,
        "--fail-on",
        "warning",
        "--renderer",
        "terminal",
        "--color",
        "never",
    ]);
    assert_eq!(cost.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&cost.stdout);
    assert!(stdout.contains(" FAIL  cost"), "{stdout}");

    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

#[test]
fn fail_on_can_only_raise_a_verdict() {
    let broken = temp_file(
        "broken.txt",
        "rule (\"x\") { event { Ongoing - Global; } actions { If(True); }",
    );
    let output = run(&["check", broken.to_str().unwrap(), "--fail-on", "info"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ERROR check"), "{stdout}");

    let waitless = temp_file("waitless.txt", WAITLESS_LOOP);
    let output = run(&["lint", waitless.to_str().unwrap(), "--fail-on", "error"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("WARN lint"), "{stdout}");

    let clean = temp_file("basic.txt", &corpus_workshop("synthetic/basic-rule"));
    let output = run(&["check", clean.to_str().unwrap(), "--fail-on", "warning"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PASS check"), "{stdout}");

    let cake = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let output = run(&[
        "lint",
        cake.to_str().unwrap(),
        "--rule-severity",
        "min-wait-loop:error",
        "--fail-on",
        "warning",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ERROR lint"), "{stdout}");

    let _ = std::fs::remove_dir_all(broken.parent().unwrap());
    let _ = std::fs::remove_dir_all(waitless.parent().unwrap());
    let _ = std::fs::remove_dir_all(clean.parent().unwrap());
    let _ = std::fs::remove_dir_all(cake.parent().unwrap());
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
    assert!(String::from_utf8_lossy(&output.stdout).contains("truncated: rules=20/21"));
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
        let stdout = String::from_utf8_lossy(&text.stdout);
        assert!(
            stdout.contains(&format!("truncated: {query}=20/21")),
            "{stdout}"
        );
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
fn lint_text_collapses_identical_findings_into_one_entry() {
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
    assert!(stdout.contains("4 warnings"), "{stdout}");
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

    let output = run(&["lint", path.to_str().unwrap()]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("findings=8"), "{stdout}");
    assert_eq!(
        stdout.matches("finding: warning repeated-value").count(),
        2,
        "each loop's finding is its own record: {stdout}"
    );
    assert!(
        stdout.contains("count=2")
            && stdout
                .contains("expression=\"distance(getPosition(eventPlayer), vector(0, 0, 0))\""),
        "{stdout}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn lint_text_orders_all_severity_bands_by_action_priority() {
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
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("1 error, 1 warning, 1 info finding"),
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
fn lint_text_resolves_locations_under_a_subdirectory_input_root() {
    let dir = temp_dir();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let path = dir.join("sub").join("flow.txt");
    std::fs::write(&path, corpus_workshop("synthetic/control-flow")).unwrap();
    let output = run(&["lint", path.to_str().unwrap()]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("at=flow.txt:11:9"),
        "the record's location resolves under the input root: {stdout}"
    );

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
    assert!(
        stdout.contains(&format!("{}:11:9", path.display())),
        "the location resolves under the input root: {stdout}"
    );
    assert!(stdout.contains(" | 11 | "), "source frame: {stdout}");
    let _ = std::fs::remove_dir_all(&dir);
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
        "--rule-file",
        rule_file.to_str().unwrap(),
        "--lint-config",
        config.to_str().unwrap(),
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "a lint-config key for a loaded --rule-file id is known: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn stdin_workshop_works_and_legacy_protocol_is_refused() {
    let output = run_with_stdin(&["check", "-"], &corpus_workshop("synthetic/basic-rule"));
    assert_eq!(output.status.code(), Some(0));

    let protocol = r#"{"protocol":{"name":"wright/opy-hir","version":"1.1.0"}}"#;
    let output = run_with_stdin(&["check", "-"], protocol);
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdin protocol: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("input-kind-unsupported"),
        "{}",
        command_result(&output)
    );
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
fn profile_applies_to_workshop_input_only() {
    let opy = std::fs::read_to_string(workspace_root().join("tests/fixtures/opy/basic-rule.opy"))
        .unwrap();
    let opy_path = temp_file("profile-fixture.opy", &opy);
    let ostw_path = temp_file("profile-fixture.ostw", "anything");
    let workshop_path = temp_file(
        "profile-fixture.ws",
        &corpus_workshop("synthetic/control-flow"),
    );

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

    let workshop = run(&[
        "check",
        workshop_path.to_str().unwrap(),
        "--profile",
        "compat",
    ]);
    assert!(workshop.status.success(), "{}", command_result(&workshop));

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
        .find(" PASS  check")
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
        .find(" PASS  analyze")
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
fn version_reports_the_implementation_version() {
    let output = run(&["--version"]);
    assert!(output.status.success());
    let banner = String::from_utf8_lossy(&output.stdout);
    assert!(banner.starts_with("wright "), "{banner}");
    assert!(banner.contains(env!("CARGO_PKG_VERSION")), "{banner}");
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

#[cfg(unix)]
fn write_convert_provider(dir: &std::path::Path, reconstruct: bool) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    const PROVIDER: &str = r##"#!/usr/bin/env python3
import json, sys
def reply(id, result=None, error=None):
    message = {"jsonrpc": "2.0", "id": id}
    message.update({"error": error} if error else {"result": result})
    print(json.dumps(message), flush=True)
for line in sys.stdin:
    request = json.loads(line)
    id, method = request["id"], request["method"]
    if method == "lpp/initialize":
        reply(id, {"protocolVersion": request["params"]["protocolVersion"], "serverInfo": {"name": "fake", "version": "0"},
                   "languages": [{"id": "opy", "extensions": ["opy"]}],
                   "capabilities": {"check": True, "compile": True, "reconstruct": RECONSTRUCT, "symbols": False, "definition": False, "references": False, "rename": False, "editValidation": False, "projectLoading": True}})
    elif method == "lpp/reconstruct":
        reply(id, {"source": "# reconstructed opy source\n"})
    else:
        reply(id, {})
"##;

    let script = dir.join("fake-convert-provider");
    std::fs::write(
        &script,
        PROVIDER.replace("RECONSTRUCT", if reconstruct { "True" } else { "False" }),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

#[cfg(unix)]
#[test]
fn convert_workshop_input_to_opy_requires_provider_reconstruct_capability() {
    let path = temp_file("convert.txt", &corpus_workshop("synthetic/basic-rule"));
    let provider = write_convert_provider(path.parent().unwrap(), false);
    let output = run(&[
        "convert",
        "--target",
        "opy",
        path.to_str().unwrap(),
        "--opy-provider",
        provider.to_str().unwrap(),
        "-f",
        "json",
    ]);
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
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn convert_workshop_input_to_ostw_reports_provider_unavailable() {
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

#[cfg(unix)]
#[test]
fn convert_json_envelope_reports_command_result_and_target() {
    let path = temp_file("convert.txt", &corpus_workshop("synthetic/basic-rule"));
    let provider = write_convert_provider(path.parent().unwrap(), true);
    let output = run(&[
        "convert",
        "--target",
        "opy",
        path.to_str().unwrap(),
        "--opy-provider",
        provider.to_str().unwrap(),
        "-f",
        "json",
    ]);
    assert!(output.stderr.is_empty(), "JSON mode keeps stderr clean");
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["exit"], 0);
    assert_eq!(envelope["command"], "convert");
    assert_eq!(envelope["wright"]["contract"], "wright-result/v1");
    assert_eq!(envelope["result"]["target"], "opy");
    assert_eq!(
        envelope["result"]["text"].as_str().unwrap(),
        "# reconstructed opy source\n"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
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

#[test]
fn refs_reports_references_and_usage_for_a_named_symbol() {
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let path = path.to_str().unwrap();

    let output = run(&["inspect", "refs", "cakePos", path]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("REFS refs"), "{stdout}");
    assert!(
        stdout.contains(
            "summary: symbol=cakePos kind=globalVariable reads=16 writes=1 calls=0 rules=1"
        ),
        "the usage header: {stdout}"
    );
    assert!(
        stdout.contains("ref: declaration at=cake.txt:3:14"),
        "locations carry source positions: {stdout}"
    );
    assert!(!stdout.contains("truncated:"), "{stdout}");

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
    assert!(stdout.contains("CFG cfg"), "{stdout}");
    assert!(
        stdout.contains("summary: rule=cake blocks=18 edges=22"),
        "{stdout}"
    );
    let shape = stdout.find("summary:").expect("shape summary");
    let detail = stdout.find("block: id=0 kind=entry").expect("block detail");
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
fn callgraph_highlight_sections_fold_beyond_the_bound() {
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
    assert!(
        stdout.contains("fan-in: subroutine=sub0 callers=2"),
        "{stdout}"
    );
    assert!(stdout.contains("truncated: items=20/24"), "{stdout}");
    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

#[test]
fn cost_run_cache_compares_against_the_previous_run() {
    let dir = temp_dir();
    let path = dir.join("t.ws");
    std::fs::write(&path, "rule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Wait(0.016);\n    }\n}\n").unwrap();
    let path = path.to_str().unwrap();

    let first = run(&["inspect", "cost", path]);
    assert!(first.status.success(), "{}", command_result(&first));
    assert!(
        !String::from_utf8_lossy(&first.stdout).contains("comparison:"),
        "no reference on a first run"
    );
    assert!(dir.join(".wright").is_dir(), "the run wrote a record");

    let second = run(&["inspect", "cost", path]);
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(
        stdout.contains("comparison: reference=previous"),
        "{stdout}"
    );
    assert!(stdout.contains("delta=0"), "{stdout}");

    let json = run(&["inspect", "cost", path, "-f", "json"]);
    let result = parse_json(&json.stdout)["result"].clone();
    assert!(
        result.get("comparison").is_none(),
        "default JSON carries no comparison (ADR-0027 decision 3): {result}"
    );

    let over = run(&["inspect", "cost", path, "--max-total", "1"]);
    assert_eq!(over.status.code(), Some(1), "{}", command_result(&over));
    let under = run(&["inspect", "cost", path, "--max-total", "32768"]);
    assert!(under.status.success(), "{}", command_result(&under));

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
            !String::from_utf8_lossy(&output.stdout).contains("comparison:"),
            "{args:?} suppressed the comparison"
        );
        let after = std::fs::read_dir(dir.join(".wright")).map(|r| r.count());
        assert_eq!(before.ok(), after.ok(), "{args:?} left the cache untouched");
    }

    let save = run(&["inspect", "cost", path, "--save-baseline", "v1"]);
    assert!(save.status.success(), "{}", command_result(&save));
    let named = run(&["inspect", "cost", path, "--baseline", "v1"]);
    let stdout = String::from_utf8_lossy(&named.stdout);
    assert!(stdout.contains("reference=baseline:v1"), "{stdout}");
    let unknown = run(&["inspect", "cost", path, "--baseline", "nope"]);
    assert_eq!(
        unknown.status.code(),
        Some(2),
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
    assert!(
        stdout.contains("comparison: reference=previous"),
        "{stdout}"
    );
    assert!(stdout.contains("delta=0"), "{stdout}");
    assert_eq!(cache_records(), compiled_records);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cost_baseline_degrades_silently_under_ci() {
    let dir = temp_dir();
    let source = "rule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Wait(0.016);\n    }\n}\n";
    let a = dir.join("a.ws");
    let b = dir.join("b.ws");
    std::fs::write(&a, source).unwrap();
    std::fs::write(&b, source).unwrap();
    let a = a.to_str().unwrap();
    let b = b.to_str().unwrap();
    let ci: &[(&str, &str)] = &[("CI", "true")];

    let assert_same_run =
        |output: &std::process::Output, bare: &std::process::Output, case: &str| {
            assert_eq!(
                output.status.code(),
                bare.status.code(),
                "{case}: {}",
                command_result(output)
            );
            assert_eq!(output.stdout, bare.stdout, "{case}: stdout changed");
            assert_eq!(output.stderr, bare.stderr, "{case}: stderr changed");
        };

    let bare = run_with_env(&["inspect", "cost", a], ci);
    assert!(bare.status.success(), "{}", command_result(&bare));
    let unknown = run_with_env(&["inspect", "cost", a, "--baseline", "nope"], ci);
    assert_same_run(&unknown, &bare, "unknown name");

    std::fs::create_dir_all(dir.join(".wright")).unwrap();
    std::fs::write(dir.join(".wright/baseline-bad.json"), "not json").unwrap();
    let corrupt = run_with_env(&["inspect", "cost", a, "--baseline", "bad"], ci);
    assert_same_run(&corrupt, &bare, "corrupt record");

    let save = run_with_env(&["inspect", "cost", a, "--save-baseline", "v1"], ci);
    assert!(save.status.success(), "{}", command_result(&save));
    let bare_b = run_with_env(&["inspect", "cost", b], ci);
    assert!(bare_b.status.success(), "{}", command_result(&bare_b));
    let mismatched = run_with_env(&["inspect", "cost", b, "--baseline", "v1"], ci);
    assert_same_run(&mismatched, &bare_b, "mismatched key");

    let named = run_with_env(&["inspect", "cost", a, "--baseline", "v1"], ci);
    assert!(named.status.success(), "{}", command_result(&named));
    let stdout = String::from_utf8_lossy(&named.stdout);
    assert!(stdout.contains("baseline:v1"), "{stdout}");

    let bare_compile = run_with_env(&["compile", a, "-o", "-"], ci);
    let compiled = run_with_env(&["compile", a, "-o", "-", "--baseline", "nope"], ci);
    assert_same_run(&compiled, &bare_compile, "compile with unknown baseline");

    let mut names: Vec<_> = std::fs::read_dir(dir.join(".wright"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["baseline-bad.json", "baseline-v1.json"]);

    let outside = run(&["inspect", "cost", a, "--baseline", "nope"]);
    assert_eq!(
        outside.status.code(),
        Some(2),
        "{}",
        command_result(&outside)
    );
    assert!(
        String::from_utf8_lossy(&outside.stderr).contains("unknown baseline 'nope'"),
        "{}",
        command_result(&outside)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

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
        stdout.contains(&format!("cost: elements={total} budget=32768")),
        "{stdout}"
    );
    let rules = elements["rules"].as_array().unwrap();
    assert!(
        rules
            .windows(2)
            .all(|w| w[0]["count"].as_u64().unwrap() >= w[1]["count"].as_u64().unwrap()),
        "payload is ranked by count: {rules:?}"
    );
    let mut offset = 0;
    for rule in rules {
        let needle = format!(
            "share: rule={} index={} elements={}",
            rule["name"].as_str().unwrap(),
            rule["index"].as_u64().unwrap(),
            rule["count"].as_u64().unwrap()
        );
        offset = stdout[offset..]
            .find(&needle)
            .map(|found| offset + found + needle.len())
            .expect("shares render in payload order");
    }
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

#[test]
fn cost_reports_unavailable_element_count_with_budget_and_reason() {
    let failing = temp_file(
        "opaque.ws",
        "variables {\n    global:\n        0: x\n}\nrule (\"test\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Wait(1);\n        End;\n    }\n}\n",
    );
    let failing = failing.to_str().unwrap();

    let output = run(&["inspect", "cost", failing]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("emittedBytes=unavailable")
            && stdout
                .contains("emittedBytesUnavailable=\"unknown action spelling 'rawWorkshopAction'"),
        "{stdout}"
    );
    assert!(
        stdout.contains(
            "cost: elements=unavailable budget=32768 elementCountUnavailable=\"invalid program: unknown action spelling 'rawWorkshopAction' for locale 'en-us'\""
        ),
        "{stdout}"
    );
    assert!(!stdout.contains("elements=0"), "{stdout}");
    assert!(!stdout.contains("emittedBytes=0"), "{stdout}");

    let json = run(&["inspect", "cost", failing, "-f", "json", "--all"]);
    assert!(json.status.success(), "{}", command_result(&json));
    let result = parse_json(&json.stdout)["result"].clone();
    assert_eq!(result["elementCount"]["budget"], 32768);
    assert!(
        result["elementCount"].get("total").is_none(),
        "the count is absent, not zero: {result}"
    );
    assert!(
        result["elementCount"].get("rules").is_none(),
        "no fabricated shares list: {result}"
    );
    assert!(
        result["elementCountUnavailable"]
            .as_str()
            .unwrap()
            .contains("rawWorkshopAction"),
        "{result}"
    );
    assert!(result["exact"]["emittedBytes"].is_null());
    assert!(
        result["exact"]["emittedBytesUnavailable"]
            .as_str()
            .unwrap()
            .contains("rawWorkshopAction"),
        "{result}"
    );

    let ok = temp_file("ok.ws", &corpus_workshop("synthetic/basic-rule"));
    let ok = ok.to_str().unwrap();
    let json = run(&["inspect", "cost", ok, "-f", "json"]);
    assert!(json.status.success(), "{}", command_result(&json));
    let result = parse_json(&json.stdout)["result"].clone();
    assert!(result["elementCount"]["total"].as_u64().unwrap() > 0);
    assert_eq!(result["elementCount"]["budget"], 32768);
    assert!(result["elementCount"]["rules"].is_array());
    assert!(result["exact"]["emittedBytes"].as_u64().unwrap() > 0);
    assert!(result.get("elementCountUnavailable").is_none());
    assert!(result["exact"].get("emittedBytesUnavailable").is_none());

    let _ = std::fs::remove_dir_all(Path::new(failing).parent().unwrap());
    let _ = std::fs::remove_dir_all(Path::new(ok).parent().unwrap());
}

#[test]
fn cost_rule_shares_rank_and_cap_at_five_by_default() {
    let mut source = String::from("variables {\n    global:\n        0: g\n}\n\n");
    for index in 0..25 {
        let actions = "        Set Global Variable(g, 0);\n".repeat(index + 1);
        source.push_str(&format!(
            "rule (\"rule-{index}\") {{\n    event {{\n        Ongoing - Global;\n    }}\n    actions {{\n{actions}    }}\n}}\n\n"
        ));
    }
    let path = temp_file("many-rules.ws", &source);
    let path = path.to_str().unwrap();

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
    assert!(stdout.contains("truncated: rules=5/25"), "{stdout}");

    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

#[test]
fn cost_rule_drilldown_reconciles_to_the_rule_total() {
    let path = temp_file("drill.ws", &corpus_workshop("real-world/overpy-cake"));
    let path = path.to_str().unwrap();

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
    let inspect_span = rules_json["result"]["rules"]
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

    let bad = run(&[
        "inspect",
        "cost",
        path,
        "--rule",
        "not-a-rule",
        "-f",
        "json",
    ]);
    assert_eq!(bad.status.code(), Some(2));
    let bad = parse_json(&bad.stdout);
    assert_eq!(bad["diagnostics"][0]["code"], "invalid-selection");

    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

#[test]
fn analyze_lint_and_cost_report_the_same_rule_and_finding_totals() {
    let path = temp_file(
        "mixed.txt",
        "rule (\"live one\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Wait(1, Ignore Condition);\n    }\n}\n\ndisabled rule (\"disabled one\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Wait(0.016, Ignore Condition);\n    }\n}\n\nrule (\"waitless\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Global.A = 0;\n        While(Global.A < 10);\n            Global.A += 1;\n        End;\n    }\n}\n",
    );
    let path = path.to_str().unwrap();

    let analyze = run(&["analyze", path, "-f", "json"]);
    assert!(analyze.status.success(), "{}", command_result(&analyze));
    let analyze = parse_json(&analyze.stdout);
    let cost = run(&["inspect", "cost", path, "--no-cache", "-f", "json"]);
    assert!(cost.status.success(), "{}", command_result(&cost));
    let cost = parse_json(&cost.stdout);
    let lint = run(&["lint", path, "-f", "json"]);
    assert!(lint.status.success(), "{}", command_result(&lint));
    let lint = parse_json(&lint.stdout);

    let rules = analyze["result"]["program"]["rules"].as_u64().unwrap();
    assert_eq!(rules, 3, "fixture declares three rules");
    assert_eq!(
        cost["result"]["exact"]["programRules"].as_u64().unwrap(),
        rules,
        "inspect cost reports the same program inventory as analyze"
    );
    assert_eq!(
        cost["result"]["elementCount"]["rules"]
            .as_array()
            .unwrap()
            .len() as u64,
        rules,
        "elementCount.rules enumerates the same declared rules"
    );
    let fix = run(&["lint", path, "--fix"]);
    assert!(fix.status.success(), "{}", command_result(&fix));
    let stdout = String::from_utf8_lossy(&fix.stdout);
    assert!(stdout.contains("program: files=1 rules=3"), "{stdout}");

    let lint_total = lint["result"]["counts"]["findings"]["total"]
        .as_u64()
        .unwrap();
    assert_eq!(lint_total, 1, "fixture raises one finding");
    assert_eq!(
        analyze["result"]["counts"]["risks"]["total"]
            .as_u64()
            .unwrap(),
        lint_total,
        "analyze's risk total matches lint's finding total on this input"
    );
    assert_eq!(
        analyze["result"]["counts"]["risks"]["info"], lint["result"]["counts"]["findings"]["info"],
        "severity split agrees"
    );
    let cost_findings = cost["result"]["findings"].as_array().unwrap();
    assert_eq!(cost_findings.len() as u64, lint_total);
    assert_eq!(cost_findings[0]["code"], "while-without-wait");
    assert_eq!(cost_findings[0]["severity"], "info");
    assert_eq!(
        lint["result"]["findings"][0]["code"], cost_findings[0]["code"],
        "cost serves the same finding set as lint"
    );

    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

#[test]
fn inspect_names_unnamed_rules_by_index() {
    let path = temp_file(
        "unnamed.ws",
        "rule (\"\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Wait(1);\n    }\n}\n",
    );
    let output = run(&["inspect", path.to_str().unwrap()]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("rule: name=#1 id=0"), "{stdout}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn inspect_overview_and_cfg_bound_large_detail() {
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
    assert!(stdout.contains("rules=13"), "{stdout}");
    assert!(!stdout.contains("truncated:"), "{stdout}");

    let output = run(&["inspect", "cfg", "r0", path]);
    assert!(output.status.success(), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("blocks=14") && stdout.contains("edges=17"),
        "shape summary: {stdout}"
    );
    assert!(!stdout.contains("truncated:"), "{stdout}");
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

#[test]
fn lookup_never_reads_stdin() {
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("at=") && stdout.contains("lookup-hint-zh.ow:9:"),
        "the rejected value carries its file and line: {stdout}"
    );
    assert!(
        stdout.contains("next: wright lookup --locale zh-CN"),
        "the follow-up names the locale of the rejected name: {stdout}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

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
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("next: wright lookup --language opy"),
        "an OPY rejected name points at the OPY vocabulary: {stdout}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn check_text_skips_the_lookup_hint_when_no_name_was_rejected() {
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("unknown-language"),
        "the detection diagnostic still reports: {stdout}"
    );
    assert!(
        !stdout.contains("next: wright lookup"),
        "no rejected name means no lookup pointer: {stdout}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn check_and_compile_json_keep_their_envelope_after_unknown_names() {
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
