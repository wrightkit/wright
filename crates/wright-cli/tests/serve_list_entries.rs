use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

fn corpus_workshop(id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/workshop")
        .join(id)
        .with_extension("ws")
}

fn run_lines(transport: &str, input: &Path, lines: &[&str]) -> Vec<Value> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let cwd = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "serve-list-entries-{}-{}",
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

#[test]
fn list_entries_answers_identically_over_stdio_and_jsonrpc_and_is_not_an_mcp_tool() {
    let input = corpus_workshop("synthetic/basic-rule");
    let request = r#"{"op":"listEntries","language":"workshop","kind":"enumMember","within":"Hero","pageSize":20}"#;
    let stdio = run_lines("stdio", &input, &[request]);
    let jsonrpc = run_lines(
        "jsonrpc",
        &input,
        &[&format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"request","params":{request}}}"#
        )],
    );
    let result = &stdio[0]["result"];
    assert_eq!(result["owner"], "workshop-rs");
    assert_eq!(result["entries"].as_array().unwrap().len(), 20);
    assert!(result["total"].as_u64().unwrap() > 20);
    assert!(result["nextCursor"].is_string());
    assert_eq!(result, &jsonrpc[0]["result"]);

    let next = format!(
        r#"{{"op":"listEntries","language":"workshop","kind":"enumMember","within":"Hero","pageSize":20,"cursor":{}}}"#,
        result["nextCursor"]
    );
    let second = run_lines("stdio", &input, &[&next]);
    assert_ne!(second[0]["result"]["entries"], result["entries"]);

    let dir =
        std::env::temp_dir().join(format!("wright-serve-list-entries-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let broken = run_lines("stdio", &dir.join("missing.ws"), &[request]);
    assert_eq!(broken[0]["result"], *result);
    let _ = std::fs::remove_dir_all(&dir);

    let mcp = run_lines(
        "mcp",
        &input,
        &[r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#],
    );
    let tools = mcp[0]["result"]["tools"].as_array().expect("tools");
    assert!(
        tools
            .iter()
            .all(|tool| tool["name"] != "wright_list_entries"),
        "listEntries is not an MCP tool"
    );
}
