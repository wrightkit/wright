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

    /// Read `count` notifications of the given method, in order.
    fn read_notifications(&mut self, method: &str, count: usize) -> Vec<serde_json::Value> {
        let mut notifications = Vec::new();
        for _ in 0..count {
            if let Some(notification) = self.read_notification(method) {
                notifications.push(notification);
            }
        }
        notifications
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Apply a full-document WorkspaceEdit range to the original source.
fn apply_lsp_edit(source: &str, result: &serde_json::Value, uri: &str) -> String {
    let edit = &result["changes"][uri][0];
    let range = &edit["range"];
    let new_text = edit["newText"].as_str().unwrap();
    let start_line = range["start"]["line"].as_u64().unwrap() as usize;
    let start_char = range["start"]["character"].as_u64().unwrap() as usize;
    let end_line = range["end"]["line"].as_u64().unwrap() as usize;
    let end_char = range["end"]["character"].as_u64().unwrap() as usize;
    let lines: Vec<&str> = source.split('\n').collect();
    assert_eq!(start_line, 0, "full-document edit starts at the top");
    assert_eq!(start_char, 0, "full-document edit starts at column 0");
    assert_eq!(
        end_line,
        lines.len() - 1,
        "full-document edit ends on the last line"
    );
    let last_line = lines.last().unwrap_or(&"");
    assert_eq!(
        end_char,
        last_line.chars().count(),
        "full-document edit ends at last line length"
    );
    new_text.to_string()
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
        published["params"]["version"], 1,
        "didOpen version is preserved"
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
    let range = &rename["result"]["changes"][uri_for("main.opy")][0]["range"];
    assert!(
        !(range["start"]["line"] == 0
            && range["start"]["character"] == 0
            && range["end"]["line"] == 0
            && range["end"]["character"] == 0),
        "rename range must not be the degenerate (0,0)..(0,0): {range}"
    );
    assert!(
        range["end"]["line"].as_u64().unwrap() > 0,
        "range covers the document: {range}"
    );
    // Applying the returned edit to the original source reproduces the
    // validated preview exactly.
    let original = corpus_source("synthetic/declarations-rules");
    let applied = apply_lsp_edit(&original, &rename["result"], &uri_for("main.opy"));
    assert_eq!(
        applied, new_text,
        "applying the WorkspaceEdit yields the validated result"
    );

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
        published["params"]["version"], 2,
        "didChange version is preserved"
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
fn stale_didchange_does_not_overwrite_newer_state() {
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

    let clean = "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    score += 1\n";
    let newer = "globalvar total = 0\n\nrule \"r\":\n    @Event global\n    total += 1\n";
    let stale = "rule \"broken\"\n";

    // Open at version 5.
    client.notify("textDocument/didOpen", serde_json::json!({
        "textDocument": { "uri": uri_for("v.opy"), "languageId": "opy", "version": 5, "text": clean },
    }));
    let published = client
        .read_notification("textDocument/publishDiagnostics")
        .expect("open publish");
    assert_eq!(published["params"]["version"], 5);

    // A stale didChange (version 4) must not overwrite the newer text; the
    // server republishes the still-current version 5.
    client.notify(
        "textDocument/didChange",
        serde_json::json!({
            "textDocument": { "uri": uri_for("v.opy"), "version": 4 },
            "contentChanges": [{ "text": stale }],
        }),
    );
    let published = client
        .read_notification("textDocument/publishDiagnostics")
        .expect("stale publish");
    assert_eq!(
        published["params"]["version"], 5,
        "stale change does not overwrite version 5"
    );

    // A newer didChange (version 6) applies and publishes version 6.
    client.notify(
        "textDocument/didChange",
        serde_json::json!({
            "textDocument": { "uri": uri_for("v.opy"), "version": 6 },
            "contentChanges": [{ "text": newer }],
        }),
    );
    let published = client
        .read_notification("textDocument/publishDiagnostics")
        .expect("newer publish");
    assert_eq!(published["params"]["version"], 6, "newer change applies");

    // Hover reflects the newer document (total, not score).
    let hover = client.request(
        2,
        "textDocument/hover",
        serde_json::json!({
            "textDocument": { "uri": uri_for("v.opy") },
            "position": { "line": 0, "character": 3 },
        }),
    );
    assert!(
        serde_json::to_string(&hover["result"])
            .unwrap()
            .contains("total"),
        "hover reflects the newer document: {}",
        hover
    );

    client.request(3, "shutdown", serde_json::json!(null));
    client.notify("exit", serde_json::json!(null));
}

#[test]
fn lsp_cross_file_definition_and_references() {
    let fixtures = workspace_root().join("crates/wright-language/tests/fixtures/multifile");
    let main = std::fs::read_to_string(fixtures.join("main.opy")).unwrap();
    let mut client = LspClient::spawn(&fixtures);
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
    client.notify("textDocument/didOpen", serde_json::json!({
        "textDocument": { "uri": uri_for("main.opy"), "languageId": "opy", "version": 1, "text": main },
    }));
    let _ = client
        .read_notification("textDocument/publishDiagnostics")
        .expect("diagnostics");

    // The showStatus() call is on line 5 (0-based line 4).
    let definition = client.request(
        2,
        "textDocument/definition",
        serde_json::json!({
            "textDocument": { "uri": uri_for("main.opy") },
            "position": { "line": 4, "character": 5 },
        }),
    );
    let target = definition["result"]["uri"].as_str().unwrap();
    assert!(
        target.contains("shared.opy"),
        "definition target URI is shared.opy, not the requesting document: {target}"
    );

    let references = client.request(
        3,
        "textDocument/references",
        serde_json::json!({
            "textDocument": { "uri": uri_for("main.opy") },
            "position": { "line": 4, "character": 5 },
            "context": { "includeDeclaration": true },
        }),
    );
    let uris: Vec<&str> = references["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|location| location["uri"].as_str())
        .collect();
    assert!(
        uris.iter().any(|uri| uri.contains("shared.opy")),
        "references include shared.opy: {uris:?}"
    );
    assert!(
        uris.iter().any(|uri| uri.contains("main.opy")),
        "references include main.opy: {uris:?}"
    );

    client.request(4, "shutdown", serde_json::json!(null));
    client.notify("exit", serde_json::json!(null));
}

#[test]
fn lsp_workspace_root_comes_from_initialize_not_cwd() {
    // Spawn with the repo root as cwd; pass the multi-file fixture dir as
    // rootUri. Include resolution must use rootUri (not the cwd), so
    // `#!include "shared.opy"` resolves even though shared.opy is not at the
    // repo root.
    let fixtures = workspace_root().join("crates/wright-language/tests/fixtures/multifile");
    let main = std::fs::read_to_string(fixtures.join("main.opy")).unwrap();
    let mut client = LspClient::spawn(&workspace_root());
    client.request(
        1,
        "initialize",
        serde_json::json!({
            "processId": null,
            "rootUri": format!("file://{}", fixtures.display()),
            "capabilities": {},
        }),
    );
    client.notify("initialized", serde_json::json!({}));
    client.notify("textDocument/didOpen", serde_json::json!({
        "textDocument": { "uri": uri_for("main.opy"), "languageId": "opy", "version": 1, "text": main },
    }));
    let published = client
        .read_notification("textDocument/publishDiagnostics")
        .expect("diagnostics");
    assert!(
        published["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .all(|diagnostic| diagnostic["severity"] != 1),
        "rootUri resolves the include; no error diagnostics: {}",
        published["params"]["diagnostics"]
    );

    let definition = client.request(
        2,
        "textDocument/definition",
        serde_json::json!({
            "textDocument": { "uri": uri_for("main.opy") },
            "position": { "line": 4, "character": 5 },
        }),
    );
    assert!(
        definition["result"]["uri"]
            .as_str()
            .unwrap()
            .contains("shared.opy"),
        "cross-file definition resolves under rootUri: {}",
        definition
    );

    client.request(3, "shutdown", serde_json::json!(null));
    client.notify("exit", serde_json::json!(null));
}

#[test]
fn lsp_open_unsaved_overlay_participates_in_includes() {
    let fixtures = workspace_root().join("crates/wright-language/tests/fixtures/overlay");
    let main = std::fs::read_to_string(fixtures.join("main.opy")).unwrap();
    let shared_good = "subroutine showStatus\n\ndef showStatus():\n    print(\"overlay\")\n";
    let mut client = LspClient::spawn(&fixtures);
    client.request(
        1,
        "initialize",
        serde_json::json!({
            "processId": null,
            "rootUri": format!("file://{}", fixtures.display()),
            "capabilities": {},
        }),
    );
    client.notify("initialized", serde_json::json!({}));

    // shared.opy does not exist on disk; the open unsaved overlay provides it.
    client.notify("textDocument/didOpen", serde_json::json!({
        "textDocument": { "uri": uri_for("main.opy"), "languageId": "opy", "version": 1, "text": main },
    }));
    let _ = client.read_notification("textDocument/publishDiagnostics");
    client.notify("textDocument/didOpen", serde_json::json!({
        "textDocument": { "uri": format!("file://{}", fixtures.join("shared.opy").display()), "languageId": "opy", "version": 1, "text": shared_good },
    }));
    let _ = client.read_notification("textDocument/publishDiagnostics");

    let definition = client.request(
        2,
        "textDocument/definition",
        serde_json::json!({
            "textDocument": { "uri": uri_for("main.opy") },
            "position": { "line": 4, "character": 5 },
        }),
    );
    assert!(
        definition["result"]["uri"]
            .as_str()
            .unwrap()
            .contains("shared.opy"),
        "overlay include resolves: {}",
        definition
    );

    client.request(3, "shutdown", serde_json::json!(null));
    client.notify("exit", serde_json::json!(null));
}

#[test]
fn lsp_didsave_is_explicit_and_harmless() {
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
    let source = "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    score += 1\n";
    client.notify(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": { "uri": uri_for("saved.opy"), "languageId": "opy", "version": 1, "text": source },
        }),
    );
    let _ = client
        .read_notification("textDocument/publishDiagnostics")
        .expect("open diagnostics");

    // didSave is an explicit lifecycle point (a no-op for full-sync
    // documents); the server must stay responsive afterwards.
    client.notify(
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": { "uri": uri_for("saved.opy") },
        }),
    );
    let hover = client.request(
        2,
        "textDocument/hover",
        serde_json::json!({
            "textDocument": { "uri": uri_for("saved.opy") },
            "position": { "line": 0, "character": 3 },
        }),
    );
    assert!(
        serde_json::to_string(&hover["result"])
            .unwrap()
            .contains("score"),
        "hover works after didSave: {hover}"
    );

    client.request(3, "shutdown", serde_json::json!(null));
    client.notify("exit", serde_json::json!(null));
}

#[test]
fn lsp_file_uris_with_spaces_and_unicode_round_trip() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);
    client.request(
        1,
        "initialize",
        serde_json::json!({
            "processId": null,
            "rootUri": "file:///tmp/wright%20dir",
            "capabilities": {},
        }),
    );
    client.notify("initialized", serde_json::json!({}));

    // A percent-encoded, spaced, Unicode document URI. No includes, so the
    // backing file need not exist on disk.
    let uri = "file:///tmp/wright%20dir/%E6%96%87%E4%BB%B6.opy";
    let source = "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    score += 1\n";
    client.notify(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": { "uri": uri, "languageId": "opy", "version": 1, "text": source },
        }),
    );
    let _ = client
        .read_notification("textDocument/publishDiagnostics")
        .expect("open diagnostics");

    let definition = client.request(
        2,
        "textDocument/definition",
        serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": 0, "character": 3 },
        }),
    );
    let returned = definition["result"]["uri"]
        .as_str()
        .expect("definition uri");
    assert_eq!(
        wright_language::document::uri_to_path(returned),
        Some(std::path::PathBuf::from("/tmp/wright dir/文件.opy")),
        "the returned URI decodes to the intended path: {returned}"
    );

    // The definition URI itself is a valid, non-hand-built file URI.
    let decoded = wright_language::document::uri_to_path(uri).expect("open URI decodes");
    assert_eq!(
        decoded,
        std::path::PathBuf::from("/tmp/wright dir/文件.opy"),
        "the open URI decodes to the intended path"
    );

    client.request(3, "shutdown", serde_json::json!(null));
    client.notify("exit", serde_json::json!(null));
}

#[test]
fn lsp_utf16_positions_account_for_non_bmp_characters() {
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
    // `score` starts at char column 16 but UTF-16 column 17.
    let source =
        "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    debug(\"🎯\", score)\n";
    client.notify("textDocument/didOpen", serde_json::json!({
        "textDocument": { "uri": uri_for("u.opy"), "languageId": "opy", "version": 1, "text": source },
    }));
    let _ = client.read_notification("textDocument/publishDiagnostics");

    let hover = client.request(
        2,
        "textDocument/hover",
        serde_json::json!({
            "textDocument": { "uri": uri_for("u.opy") },
            "position": { "line": 4, "character": 16 },
        }),
    );
    assert!(
        serde_json::to_string(&hover["result"])
            .unwrap()
            .contains("score"),
        "UTF-16 offset resolves the symbol: {}",
        hover
    );
    let miss = client.request(
        3,
        "textDocument/hover",
        serde_json::json!({
            "textDocument": { "uri": uri_for("u.opy") },
            "position": { "line": 4, "character": 15 },
        }),
    );
    assert!(
        miss["result"].is_null(),
        "the character offset resolves no symbol: {miss}"
    );

    client.request(4, "shutdown", serde_json::json!(null));
    client.notify("exit", serde_json::json!(null));
}

#[test]
fn lsp_round_trips_are_bounded() {
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
    let source = corpus_source("synthetic/control-flow");
    client.notify("textDocument/didOpen", serde_json::json!({
        "textDocument": { "uri": uri_for("flow.opy"), "languageId": "opy", "version": 1, "text": source },
    }));
    let _ = client.read_notification("textDocument/publishDiagnostics");

    let start = std::time::Instant::now();
    for index in 0..20 {
        client.request(
            2 + index,
            "textDocument/hover",
            serde_json::json!({
                "textDocument": { "uri": uri_for("flow.opy") },
                "position": { "line": 6, "character": 15 },
            }),
        );
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs_f64() < 10.0,
        "20 LSP round-trips must be bounded: {elapsed:?}"
    );

    client.request(99, "shutdown", serde_json::json!(null));
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
        !published["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "malformed input reports diagnostics, not a crash"
    );
    client.request(2, "shutdown", serde_json::json!(null));
    client.notify("exit", serde_json::json!(null));
}

#[test]
fn lsp_didclose_clears_diagnostics() {
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

    let broken = "rule \"x\"\n    @Event global\n    if broken";
    client.notify(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": { "uri": uri_for("closed.opy"), "languageId": "opy", "version": 1, "text": broken },
        }),
    );
    let published = client
        .read_notification("textDocument/publishDiagnostics")
        .expect("open publish");
    assert!(
        !published["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "malformed document publishes diagnostics before close"
    );

    client.notify(
        "textDocument/didClose",
        serde_json::json!({
            "textDocument": { "uri": uri_for("closed.opy") },
        }),
    );
    let cleared = client
        .read_notification("textDocument/publishDiagnostics")
        .expect("close clears diagnostics");
    assert_eq!(cleared["params"]["uri"], uri_for("closed.opy"));
    assert!(
        cleared["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "closed document diagnostics are cleared"
    );
    assert!(
        cleared["params"]["version"].is_null(),
        "closed document publish carries no version"
    );

    client.request(2, "shutdown", serde_json::json!(null));
    client.notify("exit", serde_json::json!(null));
}

#[test]
fn lsp_include_change_refreshes_dependent_diagnostics() {
    let fixtures = workspace_root().join("crates/wright-language/tests/fixtures/overlay");
    let main = std::fs::read_to_string(fixtures.join("main.opy")).unwrap();
    let shared_good = "subroutine showStatus\n\ndef showStatus():\n    print(\"overlay\")\n";
    let shared_broken = "this is not valid opy\n";
    let main_uri = uri_for("main.opy");
    let shared_uri = format!("file://{}", fixtures.join("shared.opy").display());

    let mut client = LspClient::spawn(&fixtures);
    client.request(
        1,
        "initialize",
        serde_json::json!({
            "processId": null,
            "rootUri": format!("file://{}", fixtures.display()),
            "capabilities": {},
        }),
    );
    client.notify("initialized", serde_json::json!({}));

    // Open main first (shared.opy is not on disk yet), then open the unsaved
    // overlay. Opening an overlay refreshes shared and its dependent main.
    client.notify(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": { "uri": main_uri, "languageId": "opy", "version": 1, "text": main },
        }),
    );
    let _ = client
        .read_notification("textDocument/publishDiagnostics")
        .expect("main open diagnostics");
    client.notify(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": { "uri": shared_uri, "languageId": "opy", "version": 1, "text": shared_good },
        }),
    );
    let _ = client.read_notifications("textDocument/publishDiagnostics", 2);

    // Change the open overlay to an invalid state; both shared.opy and the
    // dependent main.opy must be refreshed.
    client.notify(
        "textDocument/didChange",
        serde_json::json!({
            "textDocument": { "uri": shared_uri, "version": 2 },
            "contentChanges": [{ "text": shared_broken }],
        }),
    );
    let notifications = client.read_notifications("textDocument/publishDiagnostics", 3);
    let shared_error = notifications.iter().any(|notification| {
        notification["params"]["uri"]
            .as_str()
            .unwrap()
            .contains("shared.opy")
            && notification["params"]["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| diagnostic["severity"] == 1)
    });
    assert!(
        shared_error,
        "broken overlay publishes an error for shared.opy: {notifications:?}"
    );
    let main_refreshed = notifications.iter().any(|notification| {
        notification["params"]["uri"]
            .as_str()
            .unwrap()
            .contains("main.opy")
    });
    assert!(
        main_refreshed,
        "dependent main.opy is refreshed: {notifications:?}"
    );

    // Change the open overlay back to valid without reopening the root.
    client.notify(
        "textDocument/didChange",
        serde_json::json!({
            "textDocument": { "uri": shared_uri, "version": 3 },
            "contentChanges": [{ "text": shared_good }],
        }),
    );
    let notifications = client.read_notifications("textDocument/publishDiagnostics", 2);
    let shared_clear = notifications.iter().any(|notification| {
        notification["params"]["uri"]
            .as_str()
            .unwrap()
            .contains("shared.opy")
            && notification["params"]["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .all(|diagnostic| diagnostic["severity"] != 1)
    });
    assert!(
        shared_clear,
        "restored overlay clears shared.opy diagnostics: {notifications:?}"
    );
    let main_clear = notifications.iter().any(|notification| {
        notification["params"]["uri"]
            .as_str()
            .unwrap()
            .contains("main.opy")
            && notification["params"]["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .all(|diagnostic| diagnostic["severity"] != 1)
    });
    assert!(
        main_clear,
        "restored overlay clears dependent main.opy diagnostics: {notifications:?}"
    );

    client.request(2, "shutdown", serde_json::json!(null));
    client.notify("exit", serde_json::json!(null));
}

#[test]
fn lsp_rename_returns_multi_document_workspace_edit() {
    // Canonicalize so the server-side (cwd-resolved) include path matches the
    // client-constructed shared URI string.
    let fixtures = std::fs::canonicalize(
        workspace_root().join("crates/wright-language/tests/fixtures/multifile"),
    )
    .unwrap();
    let main = std::fs::read_to_string(fixtures.join("main.opy")).unwrap();
    let shared = std::fs::read_to_string(fixtures.join("shared.opy")).unwrap();
    let main_uri = uri_for("main.opy");
    let shared_uri = format!("file://{}", fixtures.join("shared.opy").display());

    let mut client = LspClient::spawn(&fixtures);
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
            "textDocument": { "uri": main_uri, "languageId": "opy", "version": 1, "text": main },
        }),
    );
    let _ = client
        .read_notification("textDocument/publishDiagnostics")
        .expect("main open diagnostics");
    client.notify(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": { "uri": shared_uri, "languageId": "opy", "version": 1, "text": shared },
        }),
    );

    // Rename showStatus (call site on line 5, col 5) to refresh.
    // read_response skips any pending publishDiagnostics notifications.
    let rename = client.request(
        2,
        "textDocument/rename",
        serde_json::json!({
            "textDocument": { "uri": main_uri },
            "position": { "line": 4, "character": 5 },
            "newName": "refresh",
        }),
    );
    let changes = rename["result"]["changes"]
        .as_object()
        .expect("changes map");
    let main_edit = changes
        .get(&main_uri)
        .and_then(|edits| edits.as_array().and_then(|edits| edits.first()))
        .expect("root edit in the workspace edit");
    let shared_edit = changes
        .get(&shared_uri)
        .and_then(|edits| edits.as_array().and_then(|edits| edits.first()))
        .expect("include edit in the workspace edit");
    assert!(
        main_edit["newText"].as_str().unwrap().contains("refresh()"),
        "root call site renamed: {main_edit}"
    );
    assert!(
        !main_edit["newText"]
            .as_str()
            .unwrap()
            .contains("showStatus"),
        "old name gone from the root"
    );
    assert!(
        shared_edit["newText"]
            .as_str()
            .unwrap()
            .contains("subroutine refresh"),
        "include declaration renamed: {shared_edit}"
    );
    assert!(
        !shared_edit["newText"]
            .as_str()
            .unwrap()
            .contains("showStatus"),
        "old name gone from the include"
    );

    client.request(3, "shutdown", serde_json::json!(null));
    client.notify("exit", serde_json::json!(null));
}
