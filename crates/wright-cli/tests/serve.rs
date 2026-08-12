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
