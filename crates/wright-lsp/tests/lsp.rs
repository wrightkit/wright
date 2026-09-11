//! LSP contract tests after the OPY provider cutover.
//!
//! Raw Workshop remains an in-process LSP workflow. OPY language-service
//! behavior is provider-owned and must surface an explicit refusal instead of
//! selecting a removed native frontend.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct LspClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl LspClient {
    fn spawn(cwd: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_wright-lsp"))
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("wright-lsp spawns");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
        }
    }

    fn send(&mut self, message: serde_json::Value) {
        let body = serde_json::to_string(&message).unwrap();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        self.stdin.flush().unwrap();
    }

    fn read_message(&mut self) -> serde_json::Value {
        let mut content_length = None;
        loop {
            let mut header = String::new();
            self.stdout.read_line(&mut header).unwrap();
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            if let Some(value) = header.strip_prefix("Content-Length:") {
                content_length = value.trim().parse::<usize>().ok();
            }
        }
        let mut body = vec![0; content_length.expect("content length")];
        self.stdout.read_exact(&mut body).unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    fn request(&mut self, id: u64, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        loop {
            let message = self.read_message();
            if message.get("id").is_some() {
                return message;
            }
        }
    }

    fn notify(&mut self, method: &str, params: serde_json::Value) {
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }));
    }

    fn read_notification(&mut self, method: &str) -> serde_json::Value {
        loop {
            let message = self.read_message();
            if message.get("method").and_then(|value| value.as_str()) == Some(method) {
                return message;
            }
        }
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        let _ = self.child.wait();
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn initialize(client: &mut LspClient) -> serde_json::Value {
    client.request(
        1,
        "initialize",
        serde_json::json!({
            "processId": null,
            "rootUri": "file:///workspace",
            "capabilities": {},
        }),
    )
}

#[test]
fn lsp_binary_reports_the_implementation_version_non_interactively() {
    let output = Command::new(env!("CARGO_BIN_EXE_wright-lsp"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    let banner = String::from_utf8_lossy(&output.stdout);
    assert!(banner.trim().starts_with("wright-lsp "));
    assert!(banner.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn workshop_lsp_workflow_keeps_protocol_and_lifecycle_contracts() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);
    let init = initialize(&mut client);
    assert_eq!(init["result"]["serverInfo"]["name"], "wright-lsp");
    assert!(init["result"]["capabilities"]["hoverProvider"] == true);
    client.notify("initialized", serde_json::json!({}));

    let source = "rule(\"demo\") {\n    event {\n        Ongoing - Global;\n    }\n}\n";
    client.notify(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": "file:///workspace/main.ws",
                "languageId": "workshop",
                "version": 1,
                "text": source,
            }
        }),
    );
    let published = client.read_notification("textDocument/publishDiagnostics");
    assert_eq!(published["params"]["version"], 1);
    assert!(published["params"]["diagnostics"].is_array());

    let shutdown = client.request(2, "shutdown", serde_json::json!(null));
    assert!(shutdown["result"].is_null());
    client.notify("exit", serde_json::json!(null));
}

#[test]
fn opy_lsp_workflow_reports_provider_boundary_without_static_fallback() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);
    initialize(&mut client);
    client.notify("initialized", serde_json::json!({}));
    client.notify(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": "file:///workspace/main.opy",
                "languageId": "opy",
                "version": 1,
                "text": "globalvar score = 0\n",
            }
        }),
    );
    let published = client.read_notification("textDocument/publishDiagnostics");
    let diagnostics = published["params"]["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics[0]["code"], "source-provider-unavailable");

    let completion = client.request(
        2,
        "textDocument/completion",
        serde_json::json!({
            "textDocument": { "uri": "file:///workspace/main.opy" },
            "position": { "line": 0, "character": 0 },
        }),
    );
    assert!(completion["result"].as_array().unwrap().is_empty());

    let shutdown = client.request(3, "shutdown", serde_json::json!(null));
    assert!(shutdown["result"].is_null());
    client.notify("exit", serde_json::json!(null));
}
