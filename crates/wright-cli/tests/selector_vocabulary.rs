use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn corpus_workshop(fixture_id: &str) -> PathBuf {
    workspace_root()
        .join("tests/fixtures/workshop")
        .join(fixture_id)
        .with_extension("ws")
}

fn corpus_text(fixture_id: &str) -> String {
    std::fs::read_to_string(corpus_workshop(fixture_id)).unwrap()
}

fn temp_file(name: &str, content: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-selector-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(wright())
        .args(args)
        .stdin(Stdio::null())
        .env_remove("CI")
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITHUB_STEP_SUMMARY")
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR")
        .output()
        .expect("wright runs")
}

fn command_result(output: &std::process::Output) -> String {
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn parse_json(output: &[u8]) -> serde_json::Value {
    serde_json::from_slice(output).expect("stdout is one JSON envelope")
}

fn serve_with(args: &[&str], input: &Path) -> (std::process::Child, Vec<serde_json::Value>) {
    let mut child = Command::new(wright())
        .arg("serve")
        .args(args)
        .arg(input)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wright serve spawns");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let responses = [r#"{"op":"lint"}"#, r#"{"op":"lintRules"}"#]
        .iter()
        .map(|request| {
            writeln!(stdin, "{request}").unwrap();
            stdin.flush().unwrap();
            let mut line = String::new();
            stdout.read_line(&mut line).unwrap();
            serde_json::from_str(&line).expect("JSON response")
        })
        .collect();
    drop(stdin);
    (child, responses)
}

#[test]
fn lint_lists_a_local_yaml_rule_loaded_with_the_rule_file_flag() {
    let path = temp_file("flow.txt", &corpus_text("synthetic/control-flow"));
    let rule_file = temp_file(
        "cli-smoke.yaml",
        "id: community/cli-smoke\nmetadata:\n  summary: summary\n  rationale: rationale\n  documentation: documentation\n  known-limits: limits\n  tags: []\nmatcher: {}\n",
    );
    let output = run(&[
        "lint",
        path.to_str().unwrap(),
        "--rule-file",
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
fn serve_applies_the_cli_lint_policy_to_the_session() {
    let input = corpus_workshop("real-world/overpy-cake");

    let (mut child, responses) = serve_with(&["--disable-rule", "repeated-value"], &input);
    let lint = &responses[0];
    assert!(
        lint["result"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["code"] != "repeated-value"),
        "the disabled rule produces no served findings: {lint}"
    );
    let rules = &responses[1];
    let repeated = rules["result"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["id"] == "repeated-value")
        .expect("repeated-value is reported");
    assert_eq!(
        repeated["enabled"], false,
        "the policy disables it: {rules}"
    );
    assert!(child.wait().unwrap().success());

    let cli = Command::new(wright())
        .args(["lint"])
        .arg(&input)
        .args(["--disable-rule", "repeated-value", "-f", "json"])
        .output()
        .unwrap();
    assert!(cli.status.success());
    let cli: serde_json::Value =
        serde_json::from_str(&String::from_utf8(cli.stdout).unwrap()).unwrap();
    assert_eq!(
        cli["result"]["findings"].as_array().unwrap(),
        lint["result"]["findings"].as_array().unwrap(),
        "CLI and serve findings agree under the same policy"
    );

    let (mut child, responses) = serve_with(&["--rule-severity", "min-wait-loop:error"], &input);
    let rules = &responses[1];
    let min_wait = rules["result"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["id"] == "min-wait-loop")
        .expect("min-wait-loop is reported");
    assert_eq!(min_wait["effectiveSeverity"], "error", "{rules}");
    assert!(child.wait().unwrap().success());

    let dir = std::env::temp_dir().join(format!("wright-serve-594-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let config_file = dir.join("policy.yaml");
    std::fs::write(
        &config_file,
        "rules:\n  min-wait-loop:\n    severity: error\n",
    )
    .unwrap();
    let (mut child, responses) =
        serve_with(&["--lint-config", config_file.to_str().unwrap()], &input);
    let rules = &responses[1];
    let min_wait = rules["result"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["id"] == "min-wait-loop")
        .expect("min-wait-loop is reported");
    assert_eq!(
        min_wait["effectiveSeverity"], "error",
        "the YAML config's severity override reaches lintRules: {rules}"
    );
    assert!(child.wait().unwrap().success());

    let output = Command::new(wright())
        .args(["serve", "--lint-config"])
        .arg(dir.join("missing.yaml"))
        .arg(&input)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "no request was served");

    let output = Command::new(wright())
        .args(["serve", "--rule-file"])
        .arg(dir.join("missing-rules.yaml"))
        .arg(&input)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "an unreadable --rule-file path is a startup usage error"
    );

    let output = Command::new(wright())
        .args(["serve", "--rule-severity", "min-wait-loop:hot"])
        .arg(&input)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "no request was served");

    let unknown_config = dir.join("unknown.yaml");
    std::fs::write(
        &unknown_config,
        "rules:\n  not-a-rule:\n    enabled: false\n",
    )
    .unwrap();
    for args in [
        vec!["--disable-rule".to_string(), "not-a-rule".to_string()],
        vec!["--rule-severity".to_string(), "not-a-rule:warn".to_string()],
        vec![
            "--lint-config".to_string(),
            unknown_config.to_string_lossy().into_owned(),
        ],
    ] {
        let output = Command::new(wright())
            .arg("serve")
            .args(&args)
            .arg(&input)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "serve {args:?} must refuse an unknown rule id"
        );
        assert!(output.stdout.is_empty(), "no request was served");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("not-a-rule"),
            "the message names the unknown id"
        );
    }

    let rule_file = dir.join("smoke.yaml");
    std::fs::write(
        &rule_file,
        "id: community/serve-smoke\nmetadata:\n  summary: summary\n  rationale: rationale\n  documentation: documentation\n  known-limits: limits\n  tags: []\nmatcher: {}\n",
    )
    .unwrap();
    let (mut child, responses) = serve_with(&["--rule-file", rule_file.to_str().unwrap()], &input);
    let rules = &responses[1];
    assert!(
        rules["result"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule["id"] == "community/serve-smoke"),
        "the --rule-file file's rule is discoverable: {rules}"
    );
    assert!(child.wait().unwrap().success());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lookup_only_filters_entries_by_kind() {
    let output = run(&["lookup", "wait", "--only", "action", "-f", "json"]);
    assert_eq!(output.status.code(), Some(0), "{}", command_result(&output));
    let result = parse_json(&output.stdout);
    let entries = result["entries"].as_array().unwrap();
    assert!(!entries.is_empty(), "the filter keeps matching entries");
    assert!(entries.iter().all(|entry| entry["kind"] == "action"));
}

#[test]
fn lookup_rejects_an_out_of_range_limit() {
    let output = run(&["lookup", "wait", "--limit", "0", "-f", "json"]);
    assert_eq!(output.status.code(), Some(1), "{}", command_result(&output));
    let result = parse_json(&output.stdout);
    assert_eq!(result["error"]["code"], "malformed-request", "{result}");
}

#[test]
fn inspect_refs_and_cfg_keep_only_as_the_category_filter() {
    let path = temp_file("cake.txt", &corpus_text("real-world/overpy-cake"));
    let path = path.to_str().unwrap();

    let output = run(&[
        "inspect", "refs", "cakePos", path, "--only", "read", "-f", "json",
    ]);
    assert!(output.status.success(), "{}", command_result(&output));
    let envelope = parse_json(&output.stdout);
    let references = envelope["result"]["references"].as_array().unwrap();
    assert_eq!(references.len(), 16, "{envelope}");
    assert!(references.iter().all(|r| r["kind"] == "read"));

    let output = run(&[
        "inspect", "cfg", "cake", path, "--only", "entry", "-f", "json",
    ]);
    assert!(output.status.success(), "{}", command_result(&output));
    let envelope = parse_json(&output.stdout);
    let blocks = envelope["result"]["blocks"].as_array().unwrap();
    assert!(!blocks.is_empty(), "{envelope}");
    assert!(blocks.iter().all(|block| block["kind"] == "entry"));

    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}
