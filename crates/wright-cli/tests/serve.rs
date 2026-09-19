//! Transport adapter tests (#60): the stdio and JSON-RPC adapters expose the
//! same operations and structured results as the in-process tool service,
//! with capability/version negotiation intact.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn wright_serve() -> &'static str {
    env!("CARGO_BIN_EXE_wright-serve")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn corpus_workshop(id: &str) -> PathBuf {
    workspace_root()
        .join("compatibility/fixtures")
        .join(id)
        .join("workshop.ws")
}

fn run_lines(transport: &str, input: &Path, lines: &[&str]) -> Vec<serde_json::Value> {
    let mut child = Command::new(wright_serve())
        .args(["--transport", transport])
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
fn stdio_and_jsonrpc_serve_structured_queries() {
    let input = corpus_workshop("synthetic/control-flow");
    let responses = run_lines(
        "stdio",
        &input,
        &[
            r#"{"op":"capabilities"}"#,
            r#"{"op":"project"}"#,
            r#"{"op":"callGraph"}"#,
            r#"{"op":"costEstimate"}"#,
            r#"{"op":"findings"}"#,
        ],
    );
    assert_eq!(responses.len(), 5);
    let caps = &responses[0]["result"];
    assert!(
        caps["contract"]
            .as_str()
            .unwrap()
            .starts_with("wright-result/")
    );
    assert!(
        caps["operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|op| op == "compile")
    );
    assert_eq!(responses[1]["result"]["origin"]["kind"], "workshop");
    assert!(responses[2]["result"].as_array().unwrap().is_empty());
    assert!(
        responses[3]["result"]["exact"]["emittedBytes"]
            .as_u64()
            .unwrap()
            > 0
    );

    let jsonrpc = run_lines(
        "jsonrpc",
        &input,
        &[r#"{"jsonrpc":"2.0","id":1,"method":"request","params":{"op":"findings"}}"#],
    );
    assert_eq!(responses[4]["result"], jsonrpc[0]["result"]);
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
fn transport_error_handling_and_batch_requests() {
    let input = corpus_workshop("synthetic/control-flow");
    let responses = run_lines(
        "jsonrpc",
        &input,
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
    assert_eq!(responses[1]["error"]["code"], -32601);
    assert_eq!(responses[2]["id"], serde_json::Value::Null);
    assert_eq!(responses[2]["error"]["code"], -32700);
    assert_eq!(responses[3]["error"]["code"], -32602);
    assert_eq!(responses[4]["error"]["code"], -32602);

    let basic = corpus_workshop("synthetic/basic-rule");
    let null_id_responses = run_lines(
        "jsonrpc",
        &basic,
        &[
            r#"{"jsonrpc":"1.0","id":1,"method":"check"}"#,
            r#"{"jsonrpc":"2.0","id":2}"#,
            "[]",
        ],
    );
    for resp in null_id_responses {
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], serde_json::Value::Null);
        assert_eq!(resp["error"]["code"], -32600);
    }

    let batch = run_lines(
        "jsonrpc",
        &basic,
        &[
            r#"[{"jsonrpc":"2.0","id":1,"method":"check"},{"jsonrpc":"2.0","method":"check"},{"jsonrpc":"2.0","id":2,"method":"missing"},1]"#,
        ],
    );
    let items = batch[0].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["id"], 1);
    assert_eq!(items[0]["result"]["command"], "check");
    assert_eq!(items[1]["id"], 2);
    assert_eq!(items[1]["error"]["code"], -32601);
    assert_eq!(items[2]["id"], serde_json::Value::Null);
    assert_eq!(items[2]["error"]["code"], -32600);

    let notif = run_lines(
        "jsonrpc",
        &basic,
        &[
            r#"{"jsonrpc":"2.0","method":"request","params":{"op":"capabilities"}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"check"}"#,
        ],
    );
    assert_eq!(notif.len(), 1);
    assert_eq!(notif[0]["id"], 2);

    let malformed_stdio = run_lines("stdio", &basic, &["not json"]);
    assert_eq!(malformed_stdio[0]["error"]["code"], "malformed-request");
}

#[test]
fn transports_serve_mutation_operations() {
    let input = corpus_workshop("synthetic/control-flow");
    let source = std::fs::read_to_string(&input).unwrap();
    let identity = wright_driver::input_identity(&source);
    let line_count = source.lines().count().max(1) as u32;
    let end_col = source
        .lines()
        .last()
        .map(|line| line.chars().count() as u32 + 1)
        .unwrap_or(1);
    let request = serde_json::json!({
        "op": "validateEditTransaction",
        "sources": { input.to_string_lossy().into_owned(): source.clone() },
        "transaction": {
            "edits": [{
                "kind": "rename",
                "source": input.to_string_lossy().into_owned(),
                "source_identity": identity,
                "range": {
                    "start_line": 1, "start_col": 1,
                    "end_line": line_count, "end_col": end_col,
                },
                "new_text": source.replace("j", "total")
            }]
        }
    });
    let responses = run_lines(
        "stdio",
        &input,
        &[&serde_json::to_string(&request).unwrap()],
    );
    assert_eq!(responses[0]["result"]["ok"], false, "{responses:?}");

    let rename = serde_json::json!({
        "op": "semanticRename",
        "sources": {
            input.to_string_lossy().into_owned():
                std::fs::read_to_string(&input).unwrap()
        },
        "target": { "source": input.to_string_lossy().into_owned(), "line": 1, "col": 11, "to": "total" }
    });
    let stdio = run_lines("stdio", &input, &[&serde_json::to_string(&rename).unwrap()]);
    assert_eq!(stdio[0]["result"]["ok"], false, "{stdio:?}");

    let jsonrpc_request = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "request", "params": rename
    });
    let jsonrpc = run_lines(
        "jsonrpc",
        &input,
        &[&serde_json::to_string(&jsonrpc_request).unwrap()],
    );
    assert_eq!(stdio[0]["result"], jsonrpc[0]["result"]);
}
