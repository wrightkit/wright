//! LSP server end-to-end tests (#68): a real protocol harness drives the
//! `wright-lsp` binary over stdio — capability negotiation, document
//! lifecycle, diagnostics, navigation, completion, rename, semantic tokens,
//! and stale-version suppression.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// A minimal LSP client over the binary's stdio.
struct LspClient {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl LspClient {
    fn spawn(cwd: &Path) -> LspClient {
        let mut child: Child = Command::new(env!("CARGO_BIN_EXE_wright-lsp"))
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("wright-lsp spawns");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let _ = child;
        LspClient { stdin, stdout }
    }

    fn send(&mut self, message: &serde_json::Value) {
        let body = serde_json::to_string(message).unwrap();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, id: u64, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        self.read_response()
    }

    fn notify(&mut self, method: &str, params: serde_json::Value) {
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }));
    }

    /// Read one LSP message (response or notification).
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
        let mut body = vec![0u8; content_length.unwrap_or(0)];
        self.stdout.read_exact(&mut body).unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    fn read_response(&mut self) -> serde_json::Value {
        loop {
            let message = self.read_message();
            if message.get("id").is_some() {
                return message;
            }
            // Notifications (e.g. diagnostics) are skipped.
        }
    }

    /// Read a publishDiagnostics notification if one is pending.
    fn read_notification(&mut self, method: &str) -> Option<serde_json::Value> {
        let mut attempts = 0;
        while attempts < 50 {
            let message = self.read_message();
            if message.get("method").and_then(|m| m.as_str()) == Some(method) {
                return Some(message);
            }
            if message.get("id").is_some() {
                return None;
            }
            attempts += 1;
        }
        None
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn corpus_source(id: &str) -> String {
    std::fs::read_to_string(
        workspace_root()
            .join("compatibility/fixtures")
            .join(id)
            .join("source.opy"),
    )
    .unwrap()
}

fn uri_for(name: &str) -> String {
    format!("file:///{}", name)
}

#[test]
fn lsp_negotiates_capabilities_and_serves_workflows() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);

    // Capability negotiation.
    let init = client.request(
        1,
        "initialize",
        serde_json::json!({
            "processId": null,
            "rootUri": uri_for(""),
            "capabilities": {},
        }),
    );
    let capabilities = &init["result"]["capabilities"];
    assert_eq!(init["result"]["serverInfo"]["name"], "wright-lsp");
    assert!(capabilities["hoverProvider"].as_bool() == Some(true));
    assert!(capabilities["definitionProvider"].as_bool() == Some(true));
    assert!(capabilities["referencesProvider"].as_bool() == Some(true));
    assert!(capabilities["renameProvider"].as_bool() == Some(true));
    assert!(capabilities["semanticTokensProvider"].is_object());
    client.notify("initialized", serde_json::json!({}));

    // Open a document and expect publishDiagnostics.
    let source = corpus_source("synthetic/declarations-rules");
    client.notify("textDocument/didOpen", serde_json::json!({
        "textDocument": { "uri": uri_for("main.opy"), "languageId": "opy", "version": 1, "text": source },
    }));
    let published = client
        .read_notification("textDocument/publishDiagnostics")
        .expect("diagnostics published");
    assert_eq!(
        published["params"]["version"], 0,
        "first open is internal version 0"
    );

    // Hover on `score` (line 1, col 3).
    let hover = client.request(
        2,
        "textDocument/hover",
        serde_json::json!({
            "textDocument": { "uri": uri_for("main.opy") },
            "position": { "line": 0, "character": 3 },
        }),
    );
    let hover_value = serde_json::to_string(&hover["result"]["contents"]["value"]).unwrap();
    assert!(hover_value.contains("score"), "hover: {hover_value}");

    // Definition of `score`.
    let definition = client.request(
        3,
        "textDocument/definition",
        serde_json::json!({
            "textDocument": { "uri": uri_for("main.opy") },
            "position": { "line": 0, "character": 3 },
        }),
    );
    assert!(definition["result"]["range"]["start"]["line"] == 0);

    // References of `score` (declaration + read).
    let references = client.request(
        4,
        "textDocument/references",
        serde_json::json!({
            "textDocument": { "uri": uri_for("main.opy") },
            "position": { "line": 5, "character": 30 },
            "context": { "includeDeclaration": true },
        }),
    );
    assert!(references["result"].as_array().unwrap().len() >= 2);

    // Completion includes declared symbols and builtins.
    let completion = client.request(
        5,
        "textDocument/completion",
        serde_json::json!({
            "textDocument": { "uri": uri_for("main.opy") },
            "position": { "line": 0, "character": 0 },
        }),
    );
    let labels: Vec<&str> = completion["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    assert!(labels.contains(&"showStatus"), "completion: {labels:?}");
    assert!(labels.contains(&"globalvar"));

    // Rename `score` → `total`.
    let rename = client.request(
        6,
        "textDocument/rename",
        serde_json::json!({
            "textDocument": { "uri": uri_for("main.opy") },
            "position": { "line": 0, "character": 3 },
            "newName": "total",
        }),
    );
    let new_text = rename["result"]["changes"][uri_for("main.opy")][0]["newText"]
        .as_str()
        .unwrap();
    assert!(new_text.contains("globalvar total"), "renamed: {new_text}");
    assert!(!new_text.contains("globalvar score"));

    // Semantic tokens classify the document.
    let tokens = client.request(
        7,
        "textDocument/semanticTokens/full",
        serde_json::json!({
            "textDocument": { "uri": uri_for("main.opy") },
        }),
    );
    let data = tokens["result"]["data"].as_array().unwrap();
    assert!(!data.is_empty(), "semantic tokens produced");

    // Change the document: version bumps and diagnostics refresh.
    client.notify(
        "textDocument/didChange",
        serde_json::json!({
            "textDocument": { "uri": uri_for("main.opy"), "version": 2 },
            "contentChanges": [{ "text": source + "\nrule \"broken\"\n" }],
        }),
    );
    let published = client
        .read_notification("textDocument/publishDiagnostics")
        .expect("updated diagnostics");
    assert_eq!(
        published["params"]["version"], 1,
        "stale version-0 results are replaced"
    );
    let has_error = published["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic["severity"] == 1);
    assert!(has_error, "the broken rule yields an error diagnostic");

    // Shutdown and exit.
    client.request(8, "shutdown", serde_json::json!(null));
    client.notify("exit", serde_json::json!(null));
}

#[test]
fn lsp_handles_malformed_input_without_crashing() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);
    client.request(
        1,
        "initialize",
        serde_json::json!({
            "processId": null,
            "rootUri": uri_for(""),
            "capabilities": {},
        }),
    );
    client.notify("initialized", serde_json::json!({}));
    client.notify(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": { "uri": uri_for("broken.opy"), "languageId": "opy", "version": 1,
                              "text": "rule \"x\"\n    @Event global\n    if broken" },
        }),
    );
    let published = client
        .read_notification("textDocument/publishDiagnostics")
        .expect("diagnostics published");
    assert!(
        !published["params"]["diagnostics"].as_array().unwrap().is_empty(),
        "malformed input reports diagnostics, not a crash"
    );
    client.request(2, "shutdown", serde_json::json!(null));
    client.notify("exit", serde_json::json!(null));
}
