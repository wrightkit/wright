//! ADR-0035 selector vocabulary: each selector spelling means one thing.
//! `--rule-file` loads lint-policy YAML on `lint`/`serve`/`ui`, `--rule`
//! stays the declared-rule selector on semantic queries, `--only` is the
//! category filter, and removed spellings (`--rule` as a policy path,
//! `inspect rules --only`, `lookup --kind`, the `rules` request's `name`)
//! are rejected — never kept as aliases.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::json;

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

fn run_lines(transport: &str, input: &Path, lines: &[&str]) -> Vec<serde_json::Value> {
    // `compile` writes `<entry stem>.ow` in the working directory, so every
    // service process gets its own.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let cwd = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "selector-serve-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&cwd).unwrap();
    let mut child = Command::new(wright())
        .args(["serve", "--transport", transport])
        .arg(input)
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wright serve spawns");
    let mut stdin = child.stdin.take().unwrap();
    for line in lines {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&cwd);
    assert!(
        output.status.success(),
        "wright serve failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON response"))
        .collect()
}

fn mcp_call(id: u64, name: &str, arguments: serde_json::Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    })
    .to_string()
}

/// The tool result's JSON payload: the single text content block parsed back
/// into a value.
fn mcp_payload(response: &serde_json::Value) -> serde_json::Value {
    serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
        .expect("tool content is JSON")
}

/// Spawn `wright serve` with `args` over `input` and return the child plus
/// the responses to `lint` and `lintRules`.
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

/// The lint policy flags configure the served session (#594): `lint` and
/// `lintRules` report the same effective policy `wright lint` reports for
/// the same flags, and startup refuses bad policy before serving anything.
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

    // An unknown rule id is a startup usage error on every policy surface —
    // a typo must not silently leave the intended rule enabled (#640).
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
fn removed_selector_spellings_are_usage_errors() {
    // Removed spellings are rejected, never kept as aliases: the lint
    // policy path is `--rule-file` now and `--only` never filters
    // `inspect rules` by name, nor does `lookup` take `--kind`.
    let input = corpus_workshop("synthetic/basic-rule");
    let input = input.to_str().unwrap();
    for args in [
        &["lint", "--rule", "rules.yaml", input][..],
        &["serve", "--rule", "rules.yaml", input][..],
        &["ui", "--rule", "rules.yaml", input][..],
        &["inspect", "rules", "--only", "player starts", input][..],
        &["lookup", "--kind", "action"][..],
    ] {
        let output = run(args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?} must be a usage error: {}",
            command_result(&output)
        );
        assert!(output.stdout.is_empty(), "usage errors write stderr only");
    }
}

#[test]
fn ui_help_documents_the_rule_file_flag() {
    // `ui` shares the lint-policy options with `lint`/`serve`; help works
    // whether or not this binary carries the ui feature.
    let output = run(&["ui", "--help"]);
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(
        help.contains("--rule-file"),
        "ui documents --rule-file: {help}"
    );
}

#[test]
fn lookup_only_filters_entries_by_kind() {
    // `lookup --only` is the category filter the removed `--kind` spelled.
    let output = run(&["lookup", "wait", "--only", "action", "-f", "json"]);
    assert_eq!(output.status.code(), Some(0), "{}", command_result(&output));
    let result = parse_json(&output.stdout);
    let entries = result["entries"].as_array().unwrap();
    assert!(!entries.is_empty(), "the filter keeps matching entries");
    assert!(entries.iter().all(|entry| entry["kind"] == "action"));
}

#[test]
fn inspect_refs_and_cfg_keep_only_as_the_category_filter() {
    // `--only` still narrows references by kind and cfg blocks by kind on
    // the semantic queries — the spelling `inspect rules` lost because it
    // meant a name there, not a category.
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

#[test]
fn a_rules_request_carrying_name_is_malformed_on_every_transport() {
    // `name` is removed from `rules` — a request still carrying it is
    // refused `malformed-request`, the same code every other schema
    // violation produces, on stdio, on JSON-RPC `request`, and as an MCP
    // tool-result error. It must never be silently ignored.
    let input = corpus_workshop("synthetic/basic-rule");
    let stdio = run_lines(
        "stdio",
        &input,
        &[
            r#"{"op":"rules","name":"player starts"}"#,
            r#"{"op":"rules"}"#,
        ],
    );
    assert_eq!(stdio[0]["error"]["code"], "malformed-request");
    assert!(
        stdio[1].get("error").is_none() && stdio[1].get("result").is_some(),
        "bare rules still answers: {}",
        stdio[1]
    );

    let jsonrpc = run_lines(
        "jsonrpc",
        &input,
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"request","params":{"op":"rules","name":"player starts"}}"#,
        ],
    );
    // A service refusal rides `result.error`, never the protocol `error`.
    assert_eq!(jsonrpc[0]["result"]["error"]["code"], "malformed-request");
    assert!(jsonrpc[0].get("error").is_none());

    let mcp = run_lines(
        "mcp",
        &input,
        &[&mcp_call(
            1,
            "wright_rules",
            json!({"name": "player starts"}),
        )],
    );
    assert_eq!(mcp[0]["result"]["isError"], true, "{}", mcp[0]);
    assert_eq!(mcp_payload(&mcp[0])["code"], "malformed-request");
}
