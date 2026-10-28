use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::json;

fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

fn wright_serve() -> &'static str {
    env!("CARGO_BIN_EXE_wright-serve")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn corpus_workshop(id: &str) -> PathBuf {
    workspace_root()
        .join("tests/fixtures/workshop")
        .join(id)
        .with_extension("ws")
}

fn run_lines(transport: &str, input: &Path, lines: &[&str]) -> Vec<serde_json::Value> {
    run_lines_with(wright(), &["serve", "--transport", transport], input, lines)
}

fn run_lines_with(
    executable: &str,
    args: &[&str],
    input: &Path,
    lines: &[&str],
) -> Vec<serde_json::Value> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let cwd = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "serve-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&cwd).unwrap();
    let mut child = Command::new(executable)
        .args(args)
        .arg(input)
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wright-serve spawns");
    let mut stdin = child.stdin.take().unwrap();
    for line in lines {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&cwd);
    assert!(
        output.status.success(),
        "wright-serve failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON response"))
        .collect()
}

#[test]
fn workflows_have_equivalent_results_through_agent_requests_and_legacy_methods() {
    let input = corpus_workshop("synthetic/control-flow");
    let stdio = run_lines(
        "stdio",
        &input,
        &[
            r#"{"op":"compile"}"#,
            r#"{"op":"check"}"#,
            r#"{"op":"analyze"}"#,
            r#"{"op":"inspect"}"#,
        ],
    );
    let jsonrpc_requests = run_lines(
        "jsonrpc",
        &input,
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"request","params":{"op":"compile"}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"request","params":{"op":"check"}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"request","params":{"op":"analyze"}}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"request","params":{"op":"inspect"}}"#,
        ],
    );
    let jsonrpc_methods = run_lines(
        "jsonrpc",
        &input,
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"compile"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"check"}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"analyze"}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"inspect"}"#,
        ],
    );
    for (index, command) in ["compile", "check", "analyze", "inspect"]
        .iter()
        .enumerate()
    {
        let result = &stdio[index]["result"];
        assert_eq!(result["command"], *command);
        assert_eq!(result, &jsonrpc_requests[index]["result"]);
        assert_eq!(result, &jsonrpc_methods[index]["result"]);
    }
}

#[test]
fn standalone_wright_serve_binary_matches_the_installed_subcommand() {
    let input = corpus_workshop("synthetic/basic-rule");
    let cli = run_lines("stdio", &input, &[r#"{"op":"capabilities"}"#]);
    let standalone = run_lines_with(
        wright_serve(),
        &["--transport", "stdio"],
        &input,
        &[r#"{"op":"capabilities"}"#],
    );
    assert_eq!(cli[0], standalone[0]);
}

#[test]
fn serve_reserves_stdin_for_session_requests() {
    let output = Command::new(wright())
        .args(["serve", "-"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("reserves stdin for requests"));
}

#[test]
fn jsonrpc_transport_preserves_application_errors_and_protocol_errors() {
    let responses = run_lines(
        "jsonrpc",
        &corpus_workshop("synthetic/control-flow"),
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"request","params":{"op":"references","symbol":999999,"snapshot":1}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"missing"}"#,
            "not json",
            r#"{"jsonrpc":"2.0","id":4,"method":"request"}"#,
            r#"{"jsonrpc":"2.0","id":5,"method":"request","params":{"op":"unknown"}}"#,
        ],
    );
    assert_eq!(responses.len(), 5);

    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"]["error"]["code"], "invalid-id");
    assert!(responses[0].get("error").is_none());

    assert_eq!(responses[1]["id"], 2);
    assert_eq!(responses[1]["error"]["code"], -32601);
    assert!(responses[1].get("result").is_none());

    assert_eq!(responses[2]["id"], serde_json::Value::Null);
    assert_eq!(responses[2]["error"]["code"], -32700);

    assert_eq!(responses[3]["id"], 4);
    assert_eq!(responses[3]["error"]["code"], -32602);

    assert_eq!(responses[4]["id"], 5);
    assert_eq!(responses[4]["error"]["code"], -32602);
    for response in responses {
        assert_eq!(response["jsonrpc"], "2.0");
        assert!(response.get("result").is_some() ^ response.get("error").is_some());
    }
}

#[test]
fn jsonrpc_transport_rejects_invalid_requests_with_null_id() {
    let responses = run_lines(
        "jsonrpc",
        &corpus_workshop("synthetic/basic-rule"),
        &[
            r#"{"jsonrpc":"1.0","id":1,"method":"check"}"#,
            r#"{"jsonrpc":"2.0","id":2}"#,
            "[]",
        ],
    );
    assert_eq!(responses.len(), 3);
    for response in responses {
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], serde_json::Value::Null);
        assert_eq!(response["error"]["code"], -32600);
        assert!(response.get("result").is_none());
    }
}

#[test]
fn jsonrpc_transport_serves_batch_requests() {
    let responses = run_lines(
        "jsonrpc",
        &corpus_workshop("synthetic/basic-rule"),
        &[
            r#"[{"jsonrpc":"2.0","id":1,"method":"check"},{"jsonrpc":"2.0","method":"check"},{"jsonrpc":"2.0","id":2,"method":"missing"},1]"#,
        ],
    );
    assert_eq!(responses.len(), 1);
    let batch = responses[0].as_array().unwrap();
    assert_eq!(batch.len(), 3);

    assert_eq!(batch[0]["id"], 1);
    assert_eq!(batch[0]["result"]["command"], "check");

    assert_eq!(batch[1]["id"], 2);
    assert_eq!(batch[1]["error"]["code"], -32601);

    assert_eq!(batch[2]["id"], serde_json::Value::Null);
    assert_eq!(batch[2]["error"]["code"], -32600);
    for response in batch {
        assert_eq!(response["jsonrpc"], "2.0");
        assert!(response.get("result").is_some() ^ response.get("error").is_some());
    }
}

#[test]
fn jsonrpc_transport_does_not_respond_to_notifications() {
    let responses = run_lines(
        "jsonrpc",
        &corpus_workshop("synthetic/basic-rule"),
        &[
            r#"{"jsonrpc":"2.0","method":"request","params":{"op":"capabilities"}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"check"}"#,
        ],
    );
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["jsonrpc"], "2.0");
    assert_eq!(responses[0]["id"], 2);
    assert!(responses[0].get("result").is_some());
}

#[test]
fn malformed_requests_are_structured_errors() {
    let responses = run_lines(
        "stdio",
        &corpus_workshop("synthetic/basic-rule"),
        &["not json"],
    );
    assert_eq!(responses[0]["error"]["code"], "malformed-request");
}

fn run_bytes(transport: &str, input: &Path, bytes: &[u8]) -> std::process::Output {
    let mut child = Command::new(wright())
        .args(["serve", "--transport", transport])
        .arg(input)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wright spawns");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(bytes)
        .expect("stdin writes");
    child.wait_with_output().unwrap()
}

#[test]
fn invalid_utf8_on_stdin_is_a_parse_error_not_a_clean_eof() {
    let input = corpus_workshop("synthetic/basic-rule");

    let output = run_bytes(
        "stdio",
        &input,
        b"{\"op\":\"capabilities\"}\n\xff\xfe\n{\"op\":\"capabilities\"}\n",
    );
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[0].contains("\"result\""));
    assert!(
        lines[1].contains("malformed-request"),
        "the non-UTF-8 line reports a structured error: {}",
        lines[1]
    );
    assert!(
        lines[2].contains("\"result\""),
        "the session answers the request after the bad line"
    );

    let output = run_bytes(
        "jsonrpc",
        &input,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"check\"}\n\xff\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"check\"}\n",
    );
    assert!(output.status.success());
    let responses: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(responses.len(), 3, "{responses:?}");
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[1]["id"], serde_json::Value::Null);
    assert_eq!(responses[1]["error"]["code"], -32700);
    assert_eq!(responses[2]["id"], 2);
    assert!(responses[2].get("result").is_some());

    let output = run_bytes(
        "mcp",
        &input,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\"}}\n\xff\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n",
    );
    assert!(output.status.success());
    let responses: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(responses.len(), 3, "{responses:?}");
    assert_eq!(responses[1]["error"]["code"], -32700);
    assert_eq!(responses[1]["id"], serde_json::Value::Null);
    assert_eq!(responses[2]["id"], 2);
}

#[test]
fn a_tools_call_without_an_id_is_an_invalid_request() {
    let responses = run_lines(
        "mcp",
        &corpus_workshop("synthetic/basic-rule"),
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
            r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"wright_check","arguments":{}}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
        ],
    );
    assert_eq!(responses.len(), 3, "{responses:?}");
    assert_eq!(responses[1]["id"], serde_json::Value::Null);
    assert_eq!(responses[1]["error"]["code"], -32600);
    assert_eq!(responses[2]["id"], 2);
    assert!(responses[2].get("result").is_some());
}

#[test]
fn legacy_jsonrpc_methods_reject_a_params_key() {
    let responses = run_lines(
        "jsonrpc",
        &corpus_workshop("synthetic/basic-rule"),
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"compile","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"compile"}"#,
        ],
    );
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["error"]["code"], -32602);
    assert_eq!(responses[1]["id"], 2);
    assert!(responses[1].get("result").is_some());
}

#[test]
fn compile_request_carries_the_output_locale() {
    let responses = run_lines(
        "stdio",
        &corpus_workshop("synthetic/basic-rule"),
        &[
            r#"{"op":"compile","output":"en.ow","to":"en-US","force":true}"#,
            r#"{"op":"compile","output":"zh.ow","to":"zh-CN","force":true}"#,
            r#"{"op":"compile","output":"bad.ow","to":"xx-YY"}"#,
        ],
    );
    assert_eq!(
        responses[0]["result"]["result"]["output"]["locale"],
        "en-us"
    );
    assert_eq!(
        responses[1]["result"]["result"]["output"]["locale"],
        "zh-cn"
    );
    let rejected = &responses[2]["result"];
    assert_eq!(rejected["ok"], false);
    assert_eq!(rejected["exit"], 2);
    assert_eq!(rejected["diagnostics"][0]["code"], "invalid-locale");
}

#[test]
fn stdio_and_jsonrpc_transports_serve_over_an_unloadable_project() {
    let dir = std::env::temp_dir().join(format!("wright-serve-512-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("missing.ws");

    let stdio = run_lines(
        "stdio",
        &input,
        &[
            r#"{"op":"capabilities"}"#,
            r#"{"op":"targetMetadata"}"#,
            r#"{"op":"project"}"#,
            r#"{"op":"symbols"}"#,
        ],
    );
    assert_eq!(stdio[0]["result"]["agent_contract"], "wright-agent/v1");
    assert!(
        stdio[1]["result"]["actions"].as_u64().unwrap() > 0,
        "{stdio:?}"
    );
    for response in &stdio[2..] {
        assert_eq!(response["error"]["code"], "input-io", "{response}");
    }

    let jsonrpc = run_lines(
        "jsonrpc",
        &input,
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"request","params":{"op":"project"}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"check"}"#,
        ],
    );
    assert_eq!(jsonrpc[0]["result"]["error"]["code"], "input-io");
    assert!(jsonrpc[0].get("error").is_none());
    assert_eq!(jsonrpc[1]["id"], 2);
    assert!(jsonrpc[1].get("error").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lookup_answers_over_every_transport_and_a_broken_project() {
    let input = corpus_workshop("synthetic/basic-rule");
    let stdio = run_lines(
        "stdio",
        &input,
        &[r#"{"op":"lookup","language":"workshop","query":"wait"}"#],
    );
    let jsonrpc = run_lines(
        "jsonrpc",
        &input,
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"request","params":{"op":"lookup","language":"workshop","query":"wait"}}"#,
        ],
    );
    let mcp = run_lines(
        "mcp",
        &input,
        &[&mcp_call(
            1,
            "wright_lookup",
            json!({"language": "workshop", "query": "wait"}),
        )],
    );
    let result = &stdio[0]["result"];
    assert_eq!(result["owner"], "workshop-rs");
    assert!(
        result["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["signature"]
                .as_str()
                .unwrap_or_default()
                .starts_with("Wait(")),
        "a callable entry renders its signature: {result}"
    );
    assert_eq!(result, &jsonrpc[0]["result"]);
    assert_eq!(result, &mcp_payload(&mcp[0]));

    let dir = std::env::temp_dir().join(format!("wright-serve-529-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let missing = dir.join("missing.ws");
    let broken = run_lines(
        "stdio",
        &missing,
        &[r#"{"op":"lookup","language":"workshop","query":"wait"}"#],
    );
    assert_eq!(broken[0]["result"], *result);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stdio_transport_recovers_in_the_same_process() {
    use std::io::{BufRead, BufReader};
    let dir = std::env::temp_dir().join(format!("wright-serve-512-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("session.ws");

    let mut child = Command::new(wright())
        .args(["serve", "--transport", "stdio"])
        .arg(&input)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wright serve spawns");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut exchange = |request: &str| -> serde_json::Value {
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

    assert_eq!(exchange(r#"{"op":"project"}"#)["error"]["code"], "input-io");
    std::fs::write(
        &input,
        "variables {\n    global:\n        0: score\n}\n\nrule (\"setup\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(score, 1);\n    }\n}\n",
    )
    .unwrap();
    let healed = exchange(r#"{"op":"symbols"}"#);
    assert!(
        healed["result"]["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["name"] == "score"),
        "the repaired input answers in the same session: {healed}"
    );

    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "serve exited: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn transports_are_equivalent_for_mutation_operations() {
    let input = corpus_workshop("synthetic/control-flow");
    let source = std::fs::read_to_string(&input).unwrap();
    let validate = serde_json::json!({
        "op": "validateEditTransaction",
        "sources": { input.to_string_lossy().into_owned(): source.clone() },
        "transaction": {
            "edits": [{
                "kind": "edit",
                "source": input.to_string_lossy().into_owned(),
                "source_identity": wright_driver::input_identity(&source),
                "range": {
                    "start_line": 6, "start_col": 8,
                    "end_line": 6, "end_col": 21,
                },
                "new_text": "bounded loop"
            }]
        }
    });
    let stdio = run_lines(
        "stdio",
        &input,
        &[&serde_json::to_string(&validate).unwrap()],
    );
    let jsonrpc_request = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "request", "params": validate
    });
    let jsonrpc = run_lines(
        "jsonrpc",
        &input,
        &[&serde_json::to_string(&jsonrpc_request).unwrap()],
    );
    assert_eq!(stdio[0]["result"], jsonrpc[0]["result"]);
    assert_eq!(stdio[0]["result"]["ok"], true, "{:?}", stdio[0]);
}

fn mcp_call(id: u64, name: &str, arguments: serde_json::Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    })
    .to_string()
}

fn mcp_payload(response: &serde_json::Value) -> serde_json::Value {
    serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
        .expect("tool content is JSON")
}

#[cfg(unix)]
#[test]
fn stdio_transport_routes_opy_inputs_to_the_provider_backend() {
    use std::io::{BufRead, BufReader};
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

    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        (os, arch) => panic!("unsupported OPY provider target {os}/{arch}"),
    };
    let dir = std::env::temp_dir().join(format!("wright-serve-opy-{}", std::process::id()));
    let provider_dir = dir.join("providers/opy/9.9.9").join(target);
    std::fs::create_dir_all(&provider_dir).unwrap();
    let script = provider_dir.join("opy-provider");
    std::fs::write(&script, PROVIDER).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        provider_dir.join("artifact.ws"),
        std::fs::read_to_string(
            workspace_root().join("tests/fixtures/workshop/synthetic/basic-rule.ws"),
        )
        .unwrap(),
    )
    .unwrap();
    std::fs::write(dir.join("providers/opy/active"), "9.9.9").unwrap();
    let input = dir.join("main.opy");
    std::fs::write(&input, "# fake OPY entry; the staged provider ignores it").unwrap();

    let mut child = Command::new(wright())
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
    let mut exchange = |request: &str| -> serde_json::Value {
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

    let check = exchange(r#"{"op":"check"}"#);
    assert_eq!(
        check["result"]["command"], "check",
        "OPY input must reach the provider backend: {check}"
    );
    assert_eq!(check["result"]["ok"], true, "{check}");
    let project = exchange(r#"{"op":"project"}"#);
    assert_eq!(project["result"]["origin"]["kind"], "opy", "{project}");

    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "serve exited: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_serve_kind_profile_and_transport_are_usage_errors() {
    let input = corpus_workshop("synthetic/basic-rule");
    for (flag, value) in [
        ("--kind", "workshpo"),
        ("--profile", "compatt"),
        ("--transport", "pipes"),
    ] {
        let output = Command::new(wright())
            .args(["serve", flag, value])
            .arg(&input)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "serve {flag} {value} must be a usage error"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("expected one of") || stderr.contains("possible values"),
            "the usage error names the accepted values: {stderr}"
        );
        assert!(output.stdout.is_empty(), "{flag} served a response");
    }
}

#[test]
fn accepted_serve_kind_profile_and_transport_values_still_work() {
    let input = corpus_workshop("synthetic/basic-rule");
    let default = run_lines("stdio", &input, &[r#"{"op":"check"}"#]);
    assert_eq!(default[0]["result"]["ok"], true, "{default:?}");

    let mut child = Command::new(wright())
        .args(["serve", "--kind", "ws", "--profile", "compat"])
        .arg(&input)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("serve --kind ws --profile compat runs");
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, r#"{{"op":"check"}}"#).unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "serve exited: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let response: serde_json::Value = serde_json::from_str(stdout.lines().next().unwrap()).unwrap();
    assert_eq!(response["result"], default[0]["result"]);
}

#[test]
fn serve_rejects_profile_per_request_for_auto_resolved_opy_input() {
    let input = workspace_root().join("tests/fixtures/opy/basic-rule.opy");
    let responses = run_lines_with(
        wright(),
        &["serve", "--profile", "compat"],
        &input,
        &[r#"{"op":"check"}"#],
    );
    assert_eq!(responses.len(), 1);
    let result = &responses[0]["result"];
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(result["exit"], 2, "{result}");
    assert!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| { d["code"] == "profile-workshop-only" && d["stage"] == "discovery" }),
        "{result}"
    );
}
