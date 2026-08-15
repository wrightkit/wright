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

fn corpus_opy(id: &str) -> PathBuf {
    workspace_root()
        .join("compatibility/fixtures")
        .join(id)
        .join("source.opy")
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
fn stdio_transport_serves_structured_queries() {
    let responses = run_lines(
        "stdio",
        &corpus_opy("synthetic/declarations-rules"),
        &[
            r#"{"op":"capabilities"}"#,
            r#"{"op":"project"}"#,
            r#"{"op":"callGraph"}"#,
            r#"{"op":"costEstimate"}"#,
        ],
    );
    assert_eq!(responses.len(), 4);
    assert_eq!(responses[0]["result"]["contract"], "wright-result/v1");
    assert_eq!(responses[1]["result"]["origin"]["kind"], "opy");
    assert_eq!(responses[2]["result"][0]["callee"], "showStatus");
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
        &corpus_opy("synthetic/control-flow"),
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"request","params":{"op":"rules"}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"request","params":{"op":"costEstimate"}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"check"}"#,
        ],
    );
    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(
        responses[0]["result"]["result"].as_array().unwrap().len(),
        2
    );
    assert_eq!(responses[1]["id"], 2);
    assert_eq!(responses[2]["id"], 3);
    assert_eq!(responses[2]["result"]["result"]["command"], "check");
}

#[test]
fn transports_match_in_process_semantics() {
    // The same query through both transports yields equivalent results.
    let stdio = run_lines(
        "stdio",
        &corpus_opy("synthetic/control-flow"),
        &[r#"{"op":"findings"}"#],
    );
    let jsonrpc = run_lines(
        "jsonrpc",
        &corpus_opy("synthetic/control-flow"),
        &[r#"{"jsonrpc":"2.0","id":1,"method":"request","params":{"op":"findings"}}"#],
    );
    assert_eq!(stdio[0]["result"], jsonrpc[0]["result"]["result"]);
}

#[test]
fn malformed_requests_are_structured_errors() {
    let responses = run_lines("stdio", &corpus_opy("synthetic/basic-rule"), &["not json"]);
    assert_eq!(responses[0]["error"]["code"], "malformed-request");
}

#[test]
fn capability_negotiation_is_preserved() {
    let responses = run_lines(
        "stdio",
        &corpus_opy("synthetic/basic-rule"),
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
}

#[test]
fn stdio_transport_serves_mutation_operations() {
    // M14 #130: the stdio adapter exposes the shared mutation operations as
    // thin mappings — validated edit preview and semantic rename — with the
    // same structured all-or-nothing results as in-process consumers.
    let input = corpus_opy("synthetic/declarations-rules");
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
                "new_text": source.replace("score", "total")
            }]
        }
    });
    let responses = run_lines(
        "stdio",
        &input,
        &[&serde_json::to_string(&request).unwrap()],
    );
    assert_eq!(responses[0]["result"]["ok"], true, "{responses:?}");
    let previews = responses[0]["result"]["preview"].as_array().unwrap();
    assert!(
        previews[0]["new_text"]
            .as_str()
            .unwrap()
            .contains("globalvar total = 0"),
        "the preview carries the validated edited text: {responses:?}"
    );

    // Semantic rename through the same transport.
    let rename = serde_json::json!({
        "op": "semanticRename",
        "sources": {
            input.to_string_lossy().into_owned():
                std::fs::read_to_string(&input).unwrap()
        },
        "target": { "source": input.to_string_lossy().into_owned(), "line": 1, "col": 11, "to": "total" }
    });
    let responses = run_lines("stdio", &input, &[&serde_json::to_string(&rename).unwrap()]);
    assert_eq!(responses[0]["result"]["ok"], true, "{responses:?}");
    assert_eq!(
        responses[0]["result"]["transaction"]["edits"][0]["new_text"],
        "total"
    );
    assert!(
        responses[0]["result"]["preview"][0]["new_text"]
            .as_str()
            .unwrap()
            .contains("globalvar total")
    );
}

#[test]
fn transports_are_equivalent_for_mutation_operations() {
    // M14 #130: stdio and JSON-RPC map the same mutation request to the same
    // in-process behavior.
    let input = corpus_opy("synthetic/declarations-rules");
    let rename = serde_json::json!({
        "op": "semanticRename",
        "sources": {
            input.to_string_lossy().into_owned():
                std::fs::read_to_string(&input).unwrap()
        },
        "target": { "source": input.to_string_lossy().into_owned(), "line": 1, "col": 11, "to": "total" }
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
    assert_eq!(stdio[0]["result"], jsonrpc[0]["result"]["result"]);
}
