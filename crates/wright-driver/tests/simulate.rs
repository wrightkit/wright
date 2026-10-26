use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceKind, sha256_hex};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn fixture(relative: &str) -> PathBuf {
    workspace_root().join("tests/fixtures").join(relative)
}

fn identity(path: &Path) -> String {
    sha256_hex(&std::fs::read(path).unwrap())
}

fn service_of(path: &Path, kind: SourceKind) -> (CompilerSession, String) {
    let session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.to_path_buf()),
        kind,
        ..SessionConfig::default()
    })
    .unwrap();
    let identity = identity(path);
    (session, identity)
}

fn service(path: &Path) -> (CompilerSession, String) {
    service_of(path, SourceKind::Workshop)
}

fn request(input_identity: &str, rest: Value) -> ToolRequest {
    let mut request = json!({"op": "simulate", "inputIdentity": input_identity});
    request
        .as_object_mut()
        .unwrap()
        .extend(rest.as_object().unwrap().clone());
    serde_json::from_value(request).expect("request deserializes")
}

fn trace_of(result: &Value) -> &Value {
    &result["result"]["trace"]
}

#[test]
fn simulate_seconds_returns_the_athena_trace() {
    let path = fixture("workshop/synthetic/control-flow.ws");
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let ToolResponse::Ok { result } = service.handle(&request(&identity, json!({"seconds": 10.0})))
    else {
        panic!("simulate refused a scenario-free run")
    };
    assert_eq!(result["command"], json!("simulate"));
    assert_eq!(result["ok"], json!(true));
    let trace = trace_of(&result);
    assert_eq!(trace["outcome"]["status"], json!("complete"));
    for field in [
        "logs",
        "globals",
        "players",
        "hud",
        "effects",
        "timeline",
        "missingInputs",
        "expectations",
    ] {
        assert!(trace.get(field).is_some(), "trace omits {field}: {trace}");
    }
}

#[test]
fn simulate_returns_the_timeline_entries_unchanged() {
    // `athena#48` / #993: two writes separated by a `Wait` land as timeline
    // changes and cross the envelope unchanged (ADR-0033).
    let path = fixture("athena/timeline-writes.ow");
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let ToolResponse::Ok { result } = service.handle(&request(&identity, json!({"seconds": 1.0})))
    else {
        panic!("simulate refused a scenario-free run")
    };
    let trace = trace_of(&result);
    assert_eq!(trace["outcome"]["status"], json!("complete"));
    let timeline = &trace["timeline"];
    assert_eq!(timeline["truncated"], json!(false));
    let entries = timeline["entries"].as_array().unwrap();
    assert!(!entries.is_empty(), "the run had activity");
    let writes: Vec<&Value> = entries
        .iter()
        .flat_map(|entry| entry["changes"].as_array().unwrap().iter())
        .filter(|change| change["kind"] == "globalVariable" && change["name"] == "score")
        .collect();
    assert_eq!(
        writes.len(),
        2,
        "both writes land in the timeline: {entries:?}"
    );
    assert_eq!(writes[0]["new"]["value"], json!(1.0));
    assert_eq!(writes[1]["new"]["value"], json!(2.0));
    let direct = athena_sim::run_text(
        &std::fs::read_to_string(&path).unwrap(),
        athena_sim::Options { seconds: 1.0 },
    )
    .expect("athena accepts the fixture text");
    assert_eq!(
        *trace,
        serde_json::to_value(&direct).unwrap(),
        "the trace crosses the boundary unchanged"
    );
}

#[test]
fn simulate_scenario_runs_the_real_practice_range() {
    let path = fixture("athena/practice-range.ow");
    let scenario_text =
        std::fs::read_to_string(fixture("athena/scenarios/practice-range.json")).unwrap();
    let scenario: Value = serde_json::from_str(&scenario_text).unwrap();
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let ToolResponse::Ok { result } =
        service.handle(&request(&identity, json!({"scenario": scenario})))
    else {
        panic!("simulate refused the practice-range scenario")
    };
    let trace = trace_of(&result);
    assert_eq!(trace["outcome"]["status"], json!("complete"));
    assert_eq!(trace["globals"]["score"]["value"], json!(2.0));
    for expectation in trace["expectations"].as_array().unwrap() {
        assert_eq!(expectation["status"], json!("pass"), "{expectation}");
    }
    assert_eq!(trace["hud"].as_array().unwrap().len(), 1);
    let direct = athena_sim::run_scenario(&std::fs::read_to_string(&path).unwrap(), &scenario_text)
        .expect("athena accepts the fixture text");
    assert_eq!(
        *trace,
        serde_json::to_value(&direct).unwrap(),
        "the trace crosses the boundary unchanged"
    );
}

#[test]
fn simulate_missing_input_is_result_content_not_a_refusal() {
    let path = fixture("athena/practice-range.ow");
    let scenario: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture(
            "athena/scenarios/practice-range-missing-event-player.json",
        ))
        .unwrap(),
    )
    .unwrap();
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let ToolResponse::Ok { result } =
        service.handle(&request(&identity, json!({"scenario": scenario})))
    else {
        panic!("missing inputs are trace content, not a refusal")
    };
    assert_eq!(result["ok"], json!(true));
    let trace = trace_of(&result);
    assert_eq!(trace["outcome"]["status"], json!("missing_input"));
    let missing = trace["missingInputs"].as_array().unwrap();
    assert_eq!(missing.len(), 1, "{missing:?}");
    assert_eq!(missing[0]["input"], json!("eventPlayer"));
}

#[test]
fn simulate_not_modeled_is_result_content_not_a_refusal() {
    let path = fixture("athena/probes/p5-value-refresh-rate/probe.workshop");
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let ToolResponse::Ok { result } = service.handle(&request(&identity, json!({"seconds": 10.0})))
    else {
        panic!("not_modeled is trace content, not a refusal")
    };
    assert_eq!(result["ok"], json!(true));
    let trace = trace_of(&result);
    assert_eq!(trace["outcome"]["status"], json!("not_modeled"));
    assert!(
        trace["outcome"]["construct"].is_string(),
        "not_modeled names the construct: {}",
        trace["outcome"]
    );
}

#[test]
fn simulate_identity_is_the_freshness_precondition() {
    let path = fixture("workshop/synthetic/control-flow.ws");
    let (mut session, _identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let ToolResponse::Error { error } =
        service.handle(&request("stale-identity", json!({"seconds": 10.0})))
    else {
        panic!("a stale inputIdentity refuses")
    };
    assert_eq!(error.code, "edit-stale-source");
}

#[test]
fn simulate_supplied_sources_run_the_supplied_text() {
    let path = fixture("athena/practice-range.ow");
    let original = std::fs::read_to_string(&path).unwrap();
    let supplied = original.replacen(
        "Set Global Variable(score, 0);",
        "Set Global Variable(score, 41);",
        1,
    );
    assert_ne!(supplied, original);
    let supplied_identity = wright_driver::input_identity(&supplied);
    let (mut session, _disk_identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let display = path.to_string_lossy().to_string();
    let ToolResponse::Ok { result } = service.handle(&request(
        &supplied_identity,
        json!({"sources": {display: supplied}, "seconds": 1.0}),
    )) else {
        panic!("supplied-sources simulate refused")
    };
    let trace = trace_of(&result);
    assert_eq!(trace["outcome"]["status"], json!("complete"));
    assert_eq!(trace["globals"]["score"]["value"], json!(41.0));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    assert_eq!(service.snapshot_generation(), Some(1));
}

#[test]
fn simulate_sources_must_name_the_loaded_input() {
    let path = fixture("workshop/synthetic/control-flow.ws");
    let (mut session, _identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let other = std::fs::read_to_string(&path).unwrap();
    let ToolResponse::Error { error } = service.handle(&request(
        &wright_driver::input_identity(&other),
        json!({"sources": {"elsewhere.ws": other}, "seconds": 1.0}),
    )) else {
        panic!("foreign sources refuse")
    };
    assert_eq!(error.code, "edit-unknown-source");
}

#[test]
fn simulate_requires_exactly_one_source_of_simulated_time() {
    let path = fixture("workshop/synthetic/control-flow.ws");
    let scenario: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture("athena/scenarios/practice-range.json")).unwrap(),
    )
    .unwrap();
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    for rest in [
        json!({"seconds": 1.0, "scenario": scenario}),
        json!({}),
        json!({"seconds": 0.0}),
        json!({"seconds": -1.0}),
    ] {
        let ToolResponse::Error { error } = service.handle(&request(&identity, rest.clone()))
        else {
            panic!("{rest} must refuse malformed-request")
        };
        assert_eq!(error.code, "malformed-request", "{rest}");
    }
}

#[test]
fn simulate_malformed_scenario_refuses() {
    let path = fixture("workshop/synthetic/control-flow.ws");
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let ToolResponse::Error { error } =
        service.handle(&request(&identity, json!({"scenario": {"seconds": 1.0}})))
    else {
        panic!("a malformed scenario refuses")
    };
    assert_eq!(error.code, "malformed-scenario");
}

#[test]
fn simulate_reports_a_project_that_does_not_load() {
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(fixture("workshop/synthetic/definitely-missing.ws")),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    let ToolResponse::Error { error } =
        service.handle(&request("irrelevant", json!({"seconds": 1.0})))
    else {
        panic!("an unloadable input refuses")
    };
    assert_ne!(error.code, "edit-stale-source", "{error:?}");
}

#[cfg(unix)]
const COMPILE_PROVIDER: &str = r#"#!/usr/bin/env python3
import json, os, sys
artifact = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "artifact.ws")).read()
def reply(id, result=None, error=None):
    message = {"jsonrpc": "2.0", "id": id}
    message.update({"error": error} if error else {"result": result})
    print(json.dumps(message), flush=True)
for line in sys.stdin:
    request = json.loads(line)
    id, method = request["id"], request["method"]
    if method == "lpp/initialize":
        reply(id, {"protocolVersion": "1.5", "serverInfo": {"name": "fake", "version": "0"},
                   "languages": [{"id": "opy", "extensions": ["opy"]}],
                   "capabilities": {"check": True, "compile": True, "reconstruct": False,
                                    "symbols": False, "definition": False, "references": False,
                                    "rename": False, "editValidation": False,
                                    "projectLoading": True, "lookup": False}})
    elif method == "lpp/compile":
        reply(id, {"diagnostics": [], "artifact": {"format": "workshop-rs/text-v1", "content": artifact}})
    else:
        reply(id, {})
"#;

#[cfg(unix)]
#[test]
fn simulate_on_provider_loaded_opy_runs_the_compiled_artifact() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("wright-simulate-provider-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("fake-provider");
    std::fs::write(&script, COMPILE_PROVIDER).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        dir.join("artifact.ws"),
        std::fs::read_to_string(fixture("athena/practice-range.ow")).unwrap(),
    )
    .unwrap();
    let opy = fixture("opy/basic-rule.opy");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(opy),
        kind: SourceKind::Opy,
        source_backend: wright_driver::SourceBackend::Provider,
        opy_provider: wright_driver::OpyProviderConfig::with_executable(script),
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    let compiled = service.handle(&ToolRequest::Compile {
        output: Some(dir.join("out.ow")),
        force: false,
        to: None,
    });
    let ToolResponse::Ok { result: compiled } = compiled else {
        panic!("the provider-backed compile must run: {compiled:?}")
    };
    assert_eq!(compiled["ok"], json!(true), "compile envelope: {compiled}");
    let project_response = service.handle(&ToolRequest::Project);
    let ToolResponse::Ok { result: project } = project_response else {
        panic!("the provider-backed project must load: {project_response:?}")
    };
    let identity = project["inputIdentity"].as_str().unwrap().to_string();
    let scenario: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture("athena/scenarios/practice-range.json")).unwrap(),
    )
    .unwrap();
    let ToolResponse::Ok { result } =
        service.handle(&request(&identity, json!({"scenario": scenario})))
    else {
        panic!("provider-loaded OPY simulates its compiled artifact")
    };
    let trace = trace_of(&result);
    assert_eq!(trace["outcome"]["status"], json!("complete"));
    assert_eq!(trace["globals"]["score"]["value"], json!(2.0));
}

// The probes' `capture*.txt` are verbatim Inspector exports saved from the
// client. `athena`'s own fixture suite compares its simulation with them
// under these tolerances; `simulate` returns the same trace, so the same
// comparison runs through `wright`'s service layer.

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
            "{} simulated log lines, {} captured\nsimulated: {simulated:#?}\ncaptured: {:#?}",
            simulated.len(),
            captured.logs.len(),
            captured.logs
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
    let (mut session, identity) = service(&program);
    let mut service = ToolService::new(&mut session).unwrap();
    let ToolResponse::Ok { result } =
        service.handle(&request(&identity, json!({"seconds": RUN_SECONDS})))
    else {
        panic!("{id}: simulate refused")
    };
    let trace = trace_of(&result);
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
fn probe_p0_timer_resolution_matches_recorded_captures() {
    check_probe("p0-timer-resolution");
}

#[test]
fn probe_p1_wait_rounding_matches_recorded_captures() {
    check_probe("p1-wait-rounding");
}

#[test]
fn probe_p2_edge_trigger_and_wait_block_matches_recorded_captures() {
    check_probe("p2-edge-trigger-and-wait-block");
}

#[test]
fn probe_p3_same_tick_rule_order_matches_recorded_captures() {
    check_probe("p3-same-tick-rule-order");
}

#[test]
fn probe_p3_same_tick_rule_order_driver_last_matches_recorded_captures() {
    check_probe("p3-same-tick-rule-order-driver-last");
}

#[test]
fn probe_p4_wait_until_matches_recorded_capture() {
    // `athena` ≥ v0.5.0 models `Wait Until` — the probe's `capture.json`
    // pins the client's elapsed times: 0, 16, 32, 112 ms.
    let path = fixture("athena/probes/p4-wait-until-zero-tick/probe.workshop");
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let ToolResponse::Ok { result } =
        service.handle(&request(&identity, json!({"seconds": RUN_SECONDS})))
    else {
        panic!("simulate refused")
    };
    let trace = trace_of(&result);
    assert_eq!(trace["outcome"]["status"], json!("complete"));
    let logs: Vec<&str> = trace["logs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["text"].as_str().unwrap())
        .collect();
    assert_eq!(logs.len(), 4, "{logs:?}");
    for (log, (case, expected_ms)) in logs.iter().zip(
        ["P4a", "P4b", "P4c", "P4d"]
            .into_iter()
            .zip([0.0, 16.0, 32.0, 112.0]),
    ) {
        assert!(log.starts_with(case), "{log}");
        let elapsed: f64 = log
            .split("elapsed_ms=")
            .nth(1)
            .and_then(|rest| {
                rest.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
                    .next()
            })
            .and_then(|num| num.parse().ok())
            .unwrap_or_else(|| panic!("no elapsed_ms in {log}"));
        assert!((elapsed - expected_ms).abs() <= LOG_TOLERANCE, "{log}");
    }
}
