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
fn stdio_serves_simulate_on_the_real_practice_range() {
    let input = fixture("athena/practice-range.ow");
    let response = simulate("stdio", &input, json!({"seconds": 1.0}));
    let envelope = &response["result"];
    assert_eq!(envelope["command"], json!("simulate"));
    let trace = trace_of(&response);
    assert_eq!(trace["outcome"]["status"], json!("complete"));
    assert_eq!(trace["globals"]["score"]["value"], json!(0.0));
    let [hud] = trace["hud"].as_array().unwrap().as_slice() else {
        panic!("expected the init rule's HUD element: {}", trace["hud"])
    };
    assert_eq!(hud["header"]["value"], json!("Score: 0.00"));
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
    assert_eq!(caps[0]["result"]["athena_version"], json!("0.11.0"));
}

#[test]
fn stdio_simulate_stale_identity_refuses_before_running() {
    let input = fixture("athena/practice-range.ow");
    let responses = run_lines(
        "stdio",
        &input,
        &[r#"{"op":"simulate","inputIdentity":"stale","seconds":1.0}"#],
    );
    assert_eq!(responses[0]["error"]["code"], json!("edit-stale-source"));
}

// `athena`'s fixture tolerances and comparison, ported over the wire trace
// `wright serve` returns (the driver suite runs the identical comparison
// in-process).

const LOG_TOLERANCE: f64 = 0.0051;
const VARIABLE_TOLERANCE: f64 = 0.0006;
const CAPTURES: [&str; 3] = ["capture.txt", "capture-2.txt", "capture-3.txt"];
const RUN_SECONDS: f64 = 10.0;

struct Capture {
    variables: Vec<(String, String)>,
    logs: Vec<String>,
}

fn parse_capture(text: &str) -> Capture {
    let mut variables = Vec::new();
    let mut logs = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("全局.") {
            let (name, value) = rest.trim_end_matches(';').split_once(" = ").unwrap();
            variables.push((name.trim().to_string(), value.trim().to_string()));
        } else if line.starts_with("[00:") {
            let (_, message) = line.split_once("] ").unwrap();
            logs.push(message.to_string());
        }
    }
    Capture { variables, logs }
}

#[derive(Debug, PartialEq)]
enum Token {
    Text(String),
    Number(f64),
}

fn tokens(text: &str) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut text_run = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i + 1 < chars.len() && chars[i] == '.' && chars[i + 1].is_ascii_digit() {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if !text_run.is_empty() {
                out.push(Token::Text(std::mem::take(&mut text_run)));
            }
            let number: String = chars[start..i].iter().collect();
            out.push(Token::Number(number.parse().unwrap()));
        } else {
            text_run.push(chars[i]);
            i += 1;
        }
    }
    if !text_run.is_empty() {
        out.push(Token::Text(text_run));
    }
    out
}

fn same_log(simulated: &str, captured: &str) -> bool {
    let (a, b) = (tokens(simulated), tokens(captured));
    a.len() == b.len()
        && a.iter().zip(&b).all(|pair| match pair {
            (Token::Text(x), Token::Text(y)) => x == y,
            (Token::Number(x), Token::Number(y)) => (x - y).abs() <= LOG_TOLERANCE,
            _ => false,
        })
}

/// `athena`'s zero-like rule: the client omits variables whose value is
/// zero, so a trace global absent from the export must be null, false, or 0.
fn is_zero_like(value: &Value) -> bool {
    match value["type"].as_str() {
        Some("null") => true,
        Some("bool") => value["value"].as_bool() == Some(false),
        Some("number") => value["value"].as_f64() == Some(0.0),
        _ => false,
    }
}

fn compare_variables(trace: &Value, captured: &Capture) -> Result<(), String> {
    let globals = trace["globals"].as_object().unwrap();
    for (name, expected) in &captured.variables {
        let Some(actual) = globals.get(name) else {
            return Err(format!("variable {name} missing from simulation"));
        };
        let ok = match (expected.as_str(), actual["type"].as_str()) {
            ("真", Some("bool")) => actual["value"].as_bool() == Some(true),
            ("假", Some("bool")) => actual["value"].as_bool() == Some(false),
            ("假", _) => is_zero_like(actual),
            (number, Some("number")) => number
                .parse::<f64>()
                .is_ok_and(|n| (n - actual["value"].as_f64().unwrap()).abs() <= VARIABLE_TOLERANCE),
            _ => false,
        };
        if !ok {
            return Err(format!(
                "variable {name}: capture {expected}, simulated {actual}"
            ));
        }
    }
    for (name, value) in globals {
        let exported = captured.variables.iter().any(|(n, _)| n == name);
        if !exported && !is_zero_like(value) {
            return Err(format!(
                "variable {name} = {value} is absent from the capture"
            ));
        }
    }
    Ok(())
}

fn compare_logs(trace: &Value, captured: &Capture) -> Result<(), String> {
    let simulated: Vec<&str> = trace["logs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["text"].as_str().unwrap())
        .collect();
    if simulated.len() != captured.logs.len() {
        return Err(format!(
            "{} simulated log lines, {} captured",
            simulated.len(),
            captured.logs.len()
        ));
    }
    for (index, (sim, cap)) in simulated.iter().zip(&captured.logs).enumerate() {
        if !same_log(sim, cap) {
            return Err(format!(
                "log line {index}: simulated {sim:?}, captured {cap:?}"
            ));
        }
    }
    Ok(())
}

fn check_probe(id: &str) {
    let dir = fixture(&format!("athena/probes/{id}"));
    let program = dir.join("probe.workshop");
    let response = simulate("stdio", &program, json!({"seconds": RUN_SECONDS}));
    let trace = trace_of(&response);
    assert_eq!(trace["outcome"]["status"], json!("complete"), "{id}");
    for file in CAPTURES {
        let captured = parse_capture(&std::fs::read_to_string(dir.join(file)).unwrap());
        if let Err(message) =
            compare_logs(trace, &captured).and(compare_variables(trace, &captured))
        {
            panic!("{id} vs {file}: {message}");
        }
    }
}

#[test]
fn serve_probe_p0_matches_recorded_captures() {
    check_probe("p0-timer-resolution");
}

#[test]
fn serve_probe_p1_matches_recorded_captures() {
    check_probe("p1-wait-rounding");
}

#[test]
fn serve_probe_p2_matches_recorded_captures() {
    check_probe("p2-edge-trigger-and-wait-block");
}

#[test]
fn serve_probe_p3_matches_recorded_captures() {
    check_probe("p3-same-tick-rule-order");
}

#[test]
fn serve_probe_p3_driver_last_matches_recorded_captures() {
    check_probe("p3-same-tick-rule-order-driver-last");
}

#[cfg(unix)]
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

#[cfg(unix)]
#[test]
fn stdio_simulates_a_provider_loaded_opy_artifact() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::fs::PermissionsExt;

    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        (os, arch) => panic!("unsupported OPY provider target {os}/{arch}"),
    };
    let dir = std::env::temp_dir().join(format!("wright-serve-sim-opy-{}", std::process::id()));
    let provider_dir = dir.join("providers/opy/9.9.9").join(target);
    std::fs::create_dir_all(&provider_dir).unwrap();
    let script = provider_dir.join("opy-provider");
    std::fs::write(&script, PROVIDER).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        provider_dir.join("artifact.ws"),
        std::fs::read_to_string(fixture(
            "athena/probes/p5-value-refresh-rate/probe.workshop",
        ))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(dir.join("providers/opy/active"), "9.9.9").unwrap();
    let input = dir.join("main.opy");
    std::fs::write(&input, "# fake OPY entry; the staged provider ignores it").unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_wright"))
        .args(["serve", "--transport", "stdio"])
        .arg(&input)
        .env("WRIGHT_PROVIDER_DATA_DIR", &dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wright serve spawns");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut exchange = |request: &str| -> Value {
        writeln!(stdin, "{request}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        assert_ne!(
            stdout.read_line(&mut line).unwrap(),
            0,
            "the session answered {request}"
        );
        serde_json::from_str(&line).expect("JSON response")
    };

    let project = exchange(r#"{"op":"project"}"#);
    let identity = project["result"]["inputIdentity"].as_str().unwrap();
    let simulate = exchange(
        &json!({"op": "simulate", "inputIdentity": identity, "seconds": RUN_SECONDS}).to_string(),
    );
    let trace = &simulate["result"]["result"]["trace"];
    assert_eq!(trace["outcome"]["status"], "not_modeled", "{simulate}");
    assert!(
        trace["outcome"]["construct"].is_string(),
        "not_modeled names the construct: {simulate}"
    );

    drop(stdin);
    let output = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        output.status.success(),
        "serve exited: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
