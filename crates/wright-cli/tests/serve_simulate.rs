use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn fixture(relative: &str) -> PathBuf {
    workspace_root().join("tests/fixtures").join(relative)
}

fn run_lines(transport: &str, input: &Path, lines: &[&str]) -> Vec<serde_json::Value> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let cwd = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "serve-sim-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&cwd).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_wright"))
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

fn simulate(transport: &str, input: &Path, rest: Value) -> Value {
    let identity = wright_driver::input_identity(&std::fs::read_to_string(input).unwrap());
    let mut request = json!({"op": "simulate", "inputIdentity": identity});
    request
        .as_object_mut()
        .unwrap()
        .extend(rest.as_object().unwrap().clone());
    let line = match transport {
        "jsonrpc" => json!({
            "jsonrpc": "2.0", "id": 1, "method": "request", "params": request
        })
        .to_string(),
        _ => request.to_string(),
    };
    let responses = run_lines(transport, input, &[&line]);
    responses[0].clone()
}

fn trace_of(response: &Value) -> &Value {
    assert!(
        response.get("error").is_none(),
        "request refused: {response}"
    );
    &response["result"]["result"]["trace"]
}

#[test]
fn stdio_serves_simulate_scenario_and_missing_input() {
    let input = fixture("athena/practice-range.ow");
    let scenario: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture("athena/scenarios/practice-range.json")).unwrap(),
    )
    .unwrap();
    let response = simulate("stdio", &input, json!({"scenario": scenario}));
    let trace = trace_of(&response).clone();
    assert_eq!(trace["outcome"]["status"], json!("complete"));
    assert_eq!(trace["globals"]["score"]["value"], json!(2.0));
    for expectation in trace["expectations"].as_array().unwrap() {
        assert_eq!(expectation["status"], json!("pass"), "{expectation}");
    }

    let missing: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture(
            "athena/scenarios/practice-range-missing-event-player.json",
        ))
        .unwrap(),
    )
    .unwrap();
    let response = simulate("stdio", &input, json!({"scenario": missing}));
    let trace = trace_of(&response).clone();
    assert_eq!(trace["outcome"]["status"], json!("missing_input"));
    assert_eq!(trace["missingInputs"][0]["input"], json!("eventPlayer"));
}

#[test]
fn jsonrpc_serves_simulate_and_reports_the_athena_version() {
    let input = fixture("athena/practice-range.ow");
    let scenario: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture("athena/scenarios/practice-range.json")).unwrap(),
    )
    .unwrap();
    let response = simulate("jsonrpc", &input, json!({"scenario": scenario}));
    let trace = trace_of(&response).clone();
    assert_eq!(trace["outcome"]["status"], json!("complete"));
    assert_eq!(trace["globals"]["score"]["value"], json!(2.0));

    let caps = run_lines("stdio", &input, &[r#"{"op":"capabilities"}"#]);
    assert!(
        caps[0]["result"]["athena_version"]
            .as_str()
            .is_some_and(|version| !version.is_empty()),
        "{}",
        caps[0]
    );
}
