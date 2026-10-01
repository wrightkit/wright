//! Transport adapter tests (#60): the stdio and JSON-RPC adapters expose the
//! same operations and structured results as the in-process tool service,
//! with capability/version negotiation intact.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
    let mut child = Command::new(executable)
        .args(args)
        .arg(input)
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
fn stdio_transport_serves_structured_queries() {
    let responses = run_lines(
        "stdio",
        &corpus_workshop("synthetic/control-flow"),
        &[
            r#"{"op":"capabilities"}"#,
            r#"{"op":"project"}"#,
            r#"{"op":"callGraph"}"#,
            r#"{"op":"costEstimate"}"#,
        ],
    );
    assert_eq!(responses.len(), 4);
    assert_eq!(responses[0]["result"]["contract"], "wright-result/v1");
    assert_eq!(responses[0]["result"]["agent_contract"], "wright-agent/v1");
    assert_eq!(responses[1]["result"]["origin"]["kind"], "workshop");
    assert!(responses[2]["result"].as_array().unwrap().is_empty());
    assert!(
        responses[3]["result"]["exact"]["emittedBytes"]
            .as_u64()
            .unwrap()
            > 0
    );
}

#[test]
fn jsonrpc_transport_serves_requests_and_workflows() {
    let responses = run_lines(
        "jsonrpc",
        &corpus_workshop("synthetic/control-flow"),
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"request","params":{"op":"rules"}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"request","params":{"op":"costEstimate"}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"compile"}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"check"}"#,
            r#"{"jsonrpc":"2.0","id":5,"method":"analyze"}"#,
            r#"{"jsonrpc":"2.0","id":6,"method":"inspect"}"#,
        ],
    );
    assert_eq!(responses.len(), 6);
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"].as_array().unwrap().len(), 2);
    assert_eq!(responses[1]["id"], 2);
    assert_eq!(responses[2]["id"], 3);
    assert_eq!(responses[2]["result"]["command"], "compile");
    assert_eq!(responses[3]["id"], 4);
    assert_eq!(responses[3]["result"]["command"], "check");
    assert_eq!(responses[4]["id"], 5);
    assert_eq!(responses[4]["result"]["command"], "analyze");
    assert_eq!(responses[5]["id"], 6);
    assert_eq!(responses[5]["result"]["command"], "inspect");
    for response in responses {
        assert_eq!(response["jsonrpc"], "2.0");
        assert!(response.get("result").is_some());
        assert!(response.get("error").is_none());
    }
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
fn stdio_transport_applies_finding_selection() {
    // #430: selection fields ride on the wire request and the response
    // reports the withheld count; an unknown rule id is a structured error.
    let input = corpus_workshop("real-world/overpy-cake");
    let responses = run_lines(
        "stdio",
        &input,
        &[
            r#"{"op":"findings","max":3}"#,
            r#"{"op":"lint","severity":"error"}"#,
            r#"{"op":"lint","rule":"not-a-rule"}"#,
        ],
    );
    assert_eq!(
        responses[0]["result"]["findings"].as_array().unwrap().len(),
        3
    );
    assert_eq!(responses[0]["result"]["selection"]["total"], 10);
    assert_eq!(responses[0]["result"]["selection"]["withheld"], 7);
    assert!(
        responses[1]["result"]["findings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(responses[1]["result"]["selection"]["total"], 10);
    assert_eq!(responses[2]["error"]["code"], "invalid-selection");
}

#[test]
fn transports_match_in_process_semantics() {
    // The same query through both transports yields equivalent results.
    let stdio = run_lines(
        "stdio",
        &corpus_workshop("synthetic/control-flow"),
        &[r#"{"op":"findings"}"#],
    );
    let jsonrpc = run_lines(
        "jsonrpc",
        &corpus_workshop("synthetic/control-flow"),
        &[r#"{"jsonrpc":"2.0","id":1,"method":"request","params":{"op":"findings"}}"#],
    );
    assert_eq!(stdio[0]["result"], jsonrpc[0]["result"]);
}

#[test]
fn jsonrpc_transport_preserves_application_errors_and_protocol_errors() {
    let responses = run_lines(
        "jsonrpc",
        &corpus_workshop("synthetic/control-flow"),
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"request","params":{"op":"references","symbol":999999}}"#,
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

#[test]
fn capability_negotiation_is_preserved() {
    let responses = run_lines(
        "stdio",
        &corpus_workshop("synthetic/basic-rule"),
        &[r#"{"op":"capabilities"}"#],
    );
    let capabilities = &responses[0]["result"];
    assert!(
        capabilities["operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|op| op == "compile")
    );
    assert!(
        capabilities["contract"]
            .as_str()
            .unwrap()
            .starts_with("wright-result/")
    );
    assert_eq!(capabilities["agent_contract"], "wright-agent/v1");
}

#[test]
fn stdio_transport_serves_mutation_operations() {
    // #130/#434: the stdio adapter exposes the shared mutation operations as
    // thin mappings — validated edit preview and semantic rename — with the
    // same structured all-or-nothing results as in-process consumers. On raw
    // Workshop input both validate through `workshop-rs` reparse.
    let input = corpus_workshop("synthetic/control-flow");
    let source = std::fs::read_to_string(&input).unwrap();
    let identity = wright_driver::input_identity(&source);
    // A rule-name rewrite stays valid Workshop: `"bounded while"` at 6:8-21.
    let request = serde_json::json!({
        "op": "validateEditTransaction",
        "sources": { input.to_string_lossy().into_owned(): source.clone() },
        "transaction": {
            "edits": [{
                "kind": "edit",
                "source": input.to_string_lossy().into_owned(),
                "source_identity": identity,
                "range": {
                    "start_line": 6, "start_col": 8,
                    "end_line": 6, "end_col": 21,
                },
                "new_text": "bounded loop"
            }]
        }
    });
    let responses = run_lines(
        "stdio",
        &input,
        &[&serde_json::to_string(&request).unwrap()],
    );
    assert_eq!(responses[0]["result"]["ok"], true, "{responses:?}");
    assert!(
        responses[0]["result"]["preview"][0]["new_text"]
            .as_str()
            .unwrap()
            .contains("\"bounded loop\""),
        "the preview carries the edited source: {responses:?}"
    );

    // Semantic rename through the same transport (#434): the declared name
    // addresses the symbol; every occurrence rewrites through provenance.
    let rename = serde_json::json!({
        "op": "semanticRename",
        "sources": { input.to_string_lossy().into_owned(): source.clone() },
        "target": { "symbol": "index", "to": "counter" }
    });
    let responses = run_lines("stdio", &input, &[&serde_json::to_string(&rename).unwrap()]);
    assert_eq!(responses[0]["result"]["ok"], true, "{responses:?}");
    let preview = responses[0]["result"]["preview"][0]["new_text"]
        .as_str()
        .unwrap();
    assert!(preview.contains("0: counter"), "{preview}");
    assert!(preview.contains("Global.counter"), "{preview}");
}

#[test]
fn a_long_lived_session_observes_disk_changes_and_enforces_fresh_ids() {
    // #471: `wright serve` is long-lived, so an agent mutating the project on
    // disk between requests must observe the new program — while numeric ids
    // issued before the reload refuse `stale-id` until the new space is
    // listed. Stdio `writeln!` flushes per line, so requests and edits can
    // interleave on one live process.
    use std::io::{BufRead, BufReader};
    let dir = std::env::temp_dir().join(format!("wright-serve-471-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("session.ws");
    let v1 = r#"
variables {
    global:
        0: score
}
rule ("setup") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(score, 1);
    }
}
"#;
    let v2 = r#"
variables {
    global:
        0: points
}
rule ("setup") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(points, sqrt(4));
    }
}
rule ("extra") {
    event {
        Ongoing - Global;
    }
    actions {
        While(Compare(Global.points, <, 3));
            Modify Global Variable(points, Add, 1);
            Wait(0.016, Ignore Condition);
        End;
    }
}
"#;
    std::fs::write(&input, v1).unwrap();

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

    let names = |response: &serde_json::Value| {
        response["result"]
            .as_array()
            .unwrap()
            .iter()
            .map(|symbol| symbol["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&exchange(r#"{"op":"symbols"}"#)), ["score", "setup"]);
    // A numeric id issued by the current program works.
    assert!(
        exchange(r#"{"op":"references","symbol":0}"#)
            .get("result")
            .is_some()
    );

    std::fs::write(&input, v2).unwrap();

    // The id issued by the earlier program refuses `stale-id`; the name still
    // resolves against the reloaded program.
    let stale = exchange(r#"{"op":"references","symbol":0}"#);
    assert_eq!(stale["error"]["code"], "stale-id", "{stale}");
    let by_name = exchange(r#"{"op":"references","symbol":"points"}"#);
    assert!(by_name.get("result").is_some(), "{by_name}");
    assert_eq!(
        exchange(r#"{"op":"cfg","rule":1}"#)["error"]["code"],
        "stale-id"
    );

    // Listings re-establish their own spaces.
    assert_eq!(
        names(&exchange(r#"{"op":"symbols"}"#)),
        ["points", "setup", "extra"]
    );
    assert!(
        exchange(r#"{"op":"references","symbol":0}"#)
            .get("result")
            .is_some()
    );
    assert!(exchange(r#"{"op":"rules"}"#).get("result").is_some());
    assert!(exchange(r#"{"op":"cfg","rule":1}"#).get("result").is_some());

    // The workflow ops observe the same reloaded program.
    let check = exchange(r#"{"op":"check"}"#);
    assert_eq!(check["result"]["ok"], false, "{check}");
    assert!(
        check["result"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"].as_str().unwrap().contains("sqrt")),
        "{check}"
    );
    let lint = exchange(r#"{"op":"lint"}"#);
    assert!(
        lint["result"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["code"] == "min-wait-loop"),
        "{lint}"
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
    // #130/#434: stdio and JSON-RPC map the same mutation request to the same
    // in-process behavior.
    let input = corpus_workshop("synthetic/control-flow");
    let source = std::fs::read_to_string(&input).unwrap();
    let rename = serde_json::json!({
        "op": "semanticRename",
        "sources": { input.to_string_lossy().into_owned(): source },
        "target": { "symbol": "index", "to": "counter" }
    });
    let stdio = run_lines("stdio", &input, &[&serde_json::to_string(&rename).unwrap()]);
    let jsonrpc_request = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "request", "params": rename
    });
    let jsonrpc = run_lines(
        "jsonrpc",
        &input,
        &[&serde_json::to_string(&jsonrpc_request).unwrap()],
    );
    assert_eq!(stdio[0]["result"], jsonrpc[0]["result"]);
    assert_eq!(stdio[0]["result"]["ok"], true, "{:?}", stdio[0]);
}
