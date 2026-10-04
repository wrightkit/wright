//! LSP contract tests for the currently backed capability set.
//!
//! `wright-lsp` advertises document synchronization and rename; unbacked
//! editor capabilities are neither advertised nor answered. OPY/DEL/OSTW
//! documents report an explicit `source-provider-unavailable` diagnostic,
//! while raw Workshop documents receive no diagnostic at all. Rename routes
//! through the provider-owned mutation path: applied renames return a
//! `WorkspaceEdit`, and unsupported documents, unconfigured providers, and
//! provider refusals answer with a `RequestFailed` error carrying the
//! structured refusal code in `error.data.code`.

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
        Self::launch(Command::new(env!("CARGO_BIN_EXE_wright-lsp")).current_dir(cwd))
    }

    fn launch(command: &mut Command) -> Self {
        let mut child = command
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

/// A client that can receive versioned workspace edits — the realistic
/// rename-capable configuration.
fn initialize(client: &mut LspClient) -> serde_json::Value {
    client.request(
        1,
        "initialize",
        serde_json::json!({
            "processId": null,
            "rootUri": "file:///workspace",
            "capabilities": {
                "workspace": { "workspaceEdit": { "documentChanges": true } }
            },
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
fn initialize_advertises_only_backed_capabilities() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);
    let init = initialize(&mut client);
    assert_eq!(init["result"]["serverInfo"]["name"], "wright-lsp");
    let capabilities = init["result"]["capabilities"].as_object().unwrap();
    assert!(capabilities.contains_key("textDocumentSync"));
    assert_eq!(
        capabilities["renameProvider"], true,
        "rename is provider-backed and advertised"
    );
    for capability in [
        "hoverProvider",
        "definitionProvider",
        "referencesProvider",
        "completionProvider",
        "semanticTokensProvider",
    ] {
        assert!(
            !capabilities.contains_key(capability),
            "unbacked capability must not be advertised: {capability}"
        );
    }
    client.notify("initialized", serde_json::json!({}));
    let shutdown = client.request(2, "shutdown", serde_json::json!(null));
    assert!(shutdown["result"].is_null());
    client.notify("exit", serde_json::json!(null));
}

#[test]
fn malformed_params_answer_invalid_params_and_the_server_keeps_serving() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);
    initialize(&mut client);
    client.notify("initialized", serde_json::json!({}));

    for (id, message) in [
        (
            7,
            serde_json::json!({"jsonrpc": "2.0", "id": 7, "method": "textDocument/rename"}),
        ),
        (
            8,
            serde_json::json!({"jsonrpc": "2.0", "id": 8, "method": "textDocument/rename", "params": "bogus"}),
        ),
    ] {
        client.send(message);
        let error = client.read_message();
        assert_eq!(error["id"], id);
        assert_eq!(
            error["error"]["code"], -32602,
            "missing or malformed params are Invalid params, not a dead server"
        );
    }

    // a malformed notification gets no response but costs no session either
    client.send(serde_json::json!({"jsonrpc": "2.0", "method": "textDocument/didOpen"}));
    let shutdown = client.request(9, "shutdown", serde_json::json!(null));
    assert!(shutdown["result"].is_null());
    client.notify("exit", serde_json::json!(null));
}

#[test]
fn workshop_lsp_workflow_keeps_protocol_and_lifecycle_contracts() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);
    initialize(&mut client);
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
    assert!(
        published["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "a valid raw Workshop document publishes no diagnostics"
    );

    let hover = client.request(
        2,
        "textDocument/hover",
        serde_json::json!({
            "textDocument": { "uri": "file:///workspace/main.ws" },
            "position": { "line": 0, "character": 1 },
        }),
    );
    assert!(
        hover["result"].is_null() && hover.get("error").is_none(),
        "an unadvertised request returns a null result: {hover}"
    );

    let shutdown = client.request(3, "shutdown", serde_json::json!(null));
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
    assert!(
        completion["result"].is_null() && completion.get("error").is_none(),
        "an unadvertised request returns a null result: {completion}"
    );

    let shutdown = client.request(3, "shutdown", serde_json::json!(null));
    assert!(shutdown["result"].is_null());
    client.notify("exit", serde_json::json!(null));
}

fn rename_request(client: &mut LspClient, id: u64, uri: &str) -> serde_json::Value {
    client.request(
        id,
        "textDocument/rename",
        serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": 0, "character": 1 },
            "newName": "renamed",
        }),
    )
}

fn open_document(client: &mut LspClient, uri: &str, language_id: &str, text: &str) {
    client.notify(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": 1,
                "text": text,
            }
        }),
    );
}

#[test]
fn rename_on_an_unopen_document_is_a_structured_request_error() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);
    initialize(&mut client);
    client.notify("initialized", serde_json::json!({}));

    let response = rename_request(&mut client, 2, "file:///workspace/main.opy");
    assert_eq!(response["error"]["code"], -32803);
    assert_eq!(
        response["error"]["data"]["code"], "rename-unknown-document",
        "the structured refusal code rides in error.data.code: {response}"
    );

    client.notify("exit", serde_json::json!(null));
}

#[test]
fn rename_on_a_non_source_document_is_a_structured_request_error() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);
    initialize(&mut client);
    client.notify("initialized", serde_json::json!({}));
    open_document(
        &mut client,
        "file:///workspace/notes.txt",
        "plaintext",
        "hello\n",
    );
    client.read_notification("textDocument/publishDiagnostics");

    let response = rename_request(&mut client, 2, "file:///workspace/notes.txt");
    assert_eq!(response["error"]["code"], -32803);
    assert_eq!(
        response["error"]["data"]["code"],
        "rename-unsupported-document"
    );

    client.notify("exit", serde_json::json!(null));
}

#[test]
fn rename_without_a_configured_provider_is_a_structured_request_error() {
    // No DEL provider exists, so `.del` deterministically reaches the
    // unconfigured-provider refusal rather than any textual fallback.
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);
    initialize(&mut client);
    client.notify("initialized", serde_json::json!({}));
    open_document(&mut client, "file:///workspace/main.del", "del", "x\n");
    client.read_notification("textDocument/publishDiagnostics");

    let response = rename_request(&mut client, 2, "file:///workspace/main.del");
    assert_eq!(response["error"]["code"], -32803);
    assert_eq!(
        response["error"]["data"]["code"], "provider-not-configured",
        "an unconfigured provider refuses explicitly: {response}"
    );

    client.notify("exit", serde_json::json!(null));
}

#[test]
fn rename_is_only_negotiated_for_versioned_workspace_edits() {
    // `WorkspaceEdit.changes` cannot carry the validated document version,
    // so a client without `documentChanges` gets neither the advertisement
    // nor the edit.
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);
    let init = client.request(
        1,
        "initialize",
        serde_json::json!({
            "processId": null,
            "rootUri": "file:///workspace",
            "capabilities": {},
        }),
    );
    assert!(
        init["result"]["capabilities"]
            .get("renameProvider")
            .is_none(),
        "rename is not advertised without versioned workspace edits: {init}"
    );
    client.notify("initialized", serde_json::json!({}));
    open_document(
        &mut client,
        "file:///workspace/main.opy",
        "opy",
        "globalvar score = 0\n",
    );
    client.read_notification("textDocument/publishDiagnostics");

    let response = rename_request(&mut client, 2, "file:///workspace/main.opy");
    assert_eq!(response["error"]["code"], -32803);
    assert_eq!(
        response["error"]["data"]["code"], "rename-unversioned-workspace-edit",
        "an unversioned workspace edit would violate the freshness contract: {response}"
    );

    client.notify("exit", serde_json::json!(null));
}
