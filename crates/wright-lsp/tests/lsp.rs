//! LSP contract tests for the currently backed capability set.
//!
//! `wright-lsp` advertises document synchronization, raw-Workshop
//! diagnostics/hover/definition/references, and rename when versioned
//! workspace edits are negotiated; unbacked editor capabilities are neither
//! advertised nor answered. Raw Workshop documents publish the same
//! diagnostics `wright check` reports for the buffer text, while
//! OPY/DEL/OSTW documents report an explicit `source-provider-unavailable`
//! diagnostic. Rename routes through the provider-owned mutation path:
//! applied renames return a `WorkspaceEdit`, and unsupported documents,
//! unconfigured providers, and provider refusals answer with a
//! `RequestFailed` error carrying the structured refusal code in
//! `error.data.code`.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct LspClient {
    child: Child,
    stdin: Option<ChildStdin>,
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
            stdin: Some(stdin),
            stdout,
        }
    }

    fn send(&mut self, message: serde_json::Value) {
        let body = serde_json::to_string(&message).unwrap();
        let stdin = self.stdin.as_mut().unwrap();
        write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        stdin.flush().unwrap();
    }

    /// Raw bytes on the wire, for frames a conforming `send` cannot
    /// produce (#637).
    fn send_raw(&mut self, frame: &[u8]) {
        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(frame).unwrap();
        stdin.flush().unwrap();
    }

    /// Dropping the pipe delivers EOF to the server.
    fn close_stdin(&mut self) {
        self.stdin.take();
    }

    fn wait_status(&mut self) -> Option<i32> {
        self.child.wait().unwrap().code()
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
        // A panicking test leaves the server waiting on stdin; kill it
        // rather than hang the harness on `wait`.
        let _ = self.child.kill();
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
    for capability in ["hoverProvider", "definitionProvider", "referencesProvider"] {
        assert_eq!(
            capabilities[capability], true,
            "raw-Workshop semantic queries are backed and advertised: {capability}"
        );
    }
    for capability in ["completionProvider", "semanticTokensProvider"] {
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

/// A valid raw Workshop program with a declared variable, a subroutine,
/// and call/write sites to query. Every position assertion below names the
/// 1-based line/column spans `wright check`/`wright inspect` report for the
/// same text; the wire ranges are their UTF-16 0-based forms.
const WORKSHOP_SOURCE: &str = concat!(
    "variables {\n",
    "    global:\n",
    "        0: score\n",
    "}\n",
    "\n",
    "subroutines {\n",
    "    0: shared\n",
    "}\n",
    "\n",
    "rule (\"Subroutine shared\") {\n",
    "    event {\n",
    "        Subroutine;\n",
    "        shared;\n",
    "    }\n",
    "    actions {\n",
    "        Set Global Variable(score, 1);\n",
    "    }\n",
    "}\n",
    "\n",
    "rule (\"main\") {\n",
    "    event {\n",
    "        Ongoing - Global;\n",
    "    }\n",
    "    actions {\n",
    "        Call Subroutine(shared);\n",
    "        Set Global Variable(score, 2);\n",
    "    }\n",
    "}\n",
);

/// A buffer `wright check` rejects: a truncated call inside the rule's
/// actions. `didOpen`/`didChange` tests share it as the broken document.
const BROKEN_SOURCE: &str = concat!(
    "variables {\n",
    "    global:\n",
    "        0: score\n",
    "}\n",
    "\n",
    "rule (\"x\") {\n",
    "    event {\n",
    "        Ongoing - Global\n",
    "    }\n",
    "    actions {\n",
    "        Set Global Variable(score\n",
    "    }\n",
    "}\n",
);

fn position_request(uri: &str, line: u32, character: u32) -> serde_json::Value {
    serde_json::json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": character },
    })
}

fn lsp_range(start: (u32, u32), end: (u32, u32)) -> serde_json::Value {
    serde_json::json!({
        "start": { "line": start.0, "character": start.1 },
        "end": { "line": end.0, "character": end.1 },
    })
}

#[test]
fn workshop_documents_publish_check_diagnostics_and_serve_semantic_queries() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);
    initialize(&mut client);
    client.notify("initialized", serde_json::json!({}));
    let uri = "file:///workspace/main.ws";
    open_document(&mut client, uri, "workshop", WORKSHOP_SOURCE);
    let published = client.read_notification("textDocument/publishDiagnostics");
    assert_eq!(published["params"]["version"], 1);
    assert!(
        published["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "a valid raw Workshop document publishes no diagnostics"
    );

    // Hover over the `score` write in `Set Global Variable(score, 1);`
    // resolves the globalVariable symbol over the occurrence span.
    let hover = client.request(2, "textDocument/hover", position_request(uri, 15, 30));
    assert_eq!(
        hover["result"]["contents"]["value"], "`globalVariable score`",
        "hover names the resolved symbol like inspect does: {hover}"
    );
    assert_eq!(hover["result"]["range"], lsp_range((15, 28), (15, 33)));

    // Go-to-definition on `Call Subroutine(shared)` lands on the
    // `subroutines {}` declaration — the symbol span inspect reports.
    let definition = client.request(3, "textDocument/definition", position_request(uri, 24, 25));
    assert_eq!(
        definition["result"],
        serde_json::json!({
            "uri": uri,
            "range": lsp_range((6, 7), (6, 13)),
        }),
        "definition targets the declared symbol span: {definition}"
    );

    // References on the same call site are the declaration, the `event`
    // definition, and this call — inspect's `references` for the symbol.
    let references = client.request(
        4,
        "textDocument/references",
        serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": 24, "character": 25 },
            "context": { "includeDeclaration": true },
        }),
    );
    assert_eq!(
        references["result"],
        serde_json::json!([
            { "uri": uri, "range": lsp_range((6, 7), (6, 13)) },
            { "uri": uri, "range": lsp_range((12, 8), (12, 14)) },
            { "uri": uri, "range": lsp_range((24, 24), (24, 30)) },
        ]),
        "references carry every occurrence the inspect query reports: {references}"
    );
    let without_declaration = client.request(
        5,
        "textDocument/references",
        serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": 24, "character": 25 },
            "context": { "includeDeclaration": false },
        }),
    );
    assert_eq!(
        without_declaration["result"].as_array().unwrap().len(),
        2,
        "includeDeclaration=false drops the declaration reference"
    );

    // Rule names resolve too: hover at the `main` name and a position
    // outside any identifier occurrence.
    let rule_hover = client.request(6, "textDocument/hover", position_request(uri, 19, 9));
    assert_eq!(
        rule_hover["result"]["contents"]["value"], "`rule main`",
        "a rule-name position resolves the rule symbol: {rule_hover}"
    );
    let whitespace = client.request(7, "textDocument/hover", position_request(uri, 0, 2));
    assert!(
        whitespace["result"].is_null() && whitespace.get("error").is_none(),
        "a position on no identifier occurrence answers null: {whitespace}"
    );

    let shutdown = client.request(8, "shutdown", serde_json::json!(null));
    assert!(shutdown["result"].is_null());
    client.notify("exit", serde_json::json!(null));
}

#[test]
fn workshop_diagnostics_track_the_current_buffer() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);
    initialize(&mut client);
    client.notify("initialized", serde_json::json!({}));
    let uri = "file:///workspace/main.ws";

    // A buffer `wright check` rejects: the published code and span are the
    // check diagnostic's, at the buffer version.
    open_document(&mut client, uri, "workshop", BROKEN_SOURCE);
    let published = client.read_notification("textDocument/publishDiagnostics");
    assert_eq!(published["params"]["version"], 1);
    let diagnostics = published["params"]["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["code"], "parse-error");
    assert_eq!(diagnostics[0]["severity"], 1);
    assert_eq!(diagnostics[0]["range"], lsp_range((8, 4), (8, 5)));

    // A full-document change republishes the new buffer's diagnostics.
    client.notify(
        "textDocument/didChange",
        serde_json::json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{ "text": WORKSHOP_SOURCE }],
        }),
    );
    let republished = client.read_notification("textDocument/publishDiagnostics");
    assert_eq!(republished["params"]["version"], 2);
    assert!(
        republished["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "a clean buffer clears the stale diagnostics"
    );

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
    assert!(
        completion["result"].is_null() && completion.get("error").is_none(),
        "an unadvertised request returns a null result: {completion}"
    );

    // Hover/definition/references are advertised for raw Workshop only; a
    // source-language document still answers null.
    for (id, method) in [
        (3, "textDocument/hover"),
        (4, "textDocument/definition"),
        (5, "textDocument/references"),
    ] {
        let mut params = serde_json::json!({
            "textDocument": { "uri": "file:///workspace/main.opy" },
            "position": { "line": 0, "character": 4 },
        });
        if method == "textDocument/references" {
            params["context"] = serde_json::json!({ "includeDeclaration": true });
        }
        let response = client.request(id, method, params);
        assert!(
            response["result"].is_null() && response.get("error").is_none(),
            "{method} is not backed for a source-language document: {response}"
        );
    }

    let shutdown = client.request(6, "shutdown", serde_json::json!(null));
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

#[test]
fn malformed_frames_answer_parse_errors_and_the_server_keeps_serving() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);

    // Garbage where a JSON body belongs is a Parse error, not a dead
    // server — the declared body was consumed, so the stream stays
    // aligned for the next frame.
    client.send_raw(b"Content-Length: 5\r\n\r\nxxxxx");
    let error = client.read_message();
    assert_eq!(error["error"]["code"], -32700, "{error}");
    assert!(error["id"].is_null());

    // A non-UTF-8 body is malformed for the same reason.
    client.send_raw(b"Content-Length: 2\r\n\r\n\xff\xfe");
    let error = client.read_message();
    assert_eq!(error["error"]["code"], -32700, "{error}");

    // The session still serves normally after both malformed frames.
    let init = initialize(&mut client);
    assert_eq!(init["result"]["serverInfo"]["name"], "wright-lsp");
    client.notify("initialized", serde_json::json!({}));
    let shutdown = client.request(2, "shutdown", serde_json::json!(null));
    assert!(shutdown["result"].is_null());
    client.notify("exit", serde_json::json!({}));
    assert_eq!(client.wait_status(), Some(0));
}

#[test]
fn oversized_content_length_is_refused_without_allocating_it() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);

    // A 64 GiB declaration is refused outright — answering at all proves
    // no buffer of that size was ever allocated.
    client.send_raw(b"Content-Length: 68719476736\r\n\r\n{}");
    let error = client.read_message();
    assert_eq!(error["error"]["code"], -32600, "{error}");

    // The declared body consumes the rest of the stream, so the session
    // ends when the client gives up rather than hanging or falling over.
    client.close_stdin();
    assert_eq!(client.wait_status(), Some(0));
}

#[test]
fn a_frame_without_content_length_is_malformed() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);

    // A header section without Content-Length cannot be framed; the
    // server reports it instead of waiting forever.
    client.send_raw(b"Content-Type: application/vscode-jsonrpc\r\n\r\n");
    let error = client.read_message();
    assert_eq!(error["error"]["code"], -32600, "{error}");

    client.close_stdin();
    assert_eq!(client.wait_status(), Some(0));
}

#[test]
fn an_overlong_header_line_is_reported_and_the_stream_resynchronizes() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);

    // A header line past the bound is framing garbage. The frame's own
    // bounded body is still consumed, so the next well-formed frame
    // parses normally.
    let body = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "shutdown"}).to_string();
    let mut frame = format!(
        "X-Pad: {}\r\nContent-Length: {}\r\n\r\n",
        "a".repeat(9 * 1024),
        body.len()
    )
    .into_bytes();
    frame.extend_from_slice(body.as_bytes());
    client.send_raw(&frame);
    let error = client.read_message();
    assert_eq!(error["error"]["code"], -32600, "{error}");

    let init = initialize(&mut client);
    assert_eq!(init["result"]["serverInfo"]["name"], "wright-lsp");
    client.notify("initialized", serde_json::json!({}));
    let shutdown = client.request(2, "shutdown", serde_json::json!(null));
    assert!(shutdown["result"].is_null());
    client.notify("exit", serde_json::json!({}));
    assert_eq!(client.wait_status(), Some(0));
}

#[test]
fn malformed_initialize_params_are_invalid_params() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);

    for message in [
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": "bogus"}),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "initialize"}),
    ] {
        let id = message["id"].clone();
        client.send(message);
        let error = client.read_message();
        assert_eq!(error["id"], id);
        assert_eq!(
            error["error"]["code"], -32602,
            "missing or malformed initialize params are Invalid params: {error}"
        );
    }

    // The server stayed in the pre-initialize state and still accepts a
    // conforming initialize.
    let init = initialize(&mut client);
    assert_eq!(init["result"]["serverInfo"]["name"], "wright-lsp");
    let shutdown = client.request(3, "shutdown", serde_json::json!(null));
    assert!(shutdown["result"].is_null());
    client.notify("exit", serde_json::json!({}));
    assert_eq!(client.wait_status(), Some(0));
}

#[test]
fn requests_outside_the_running_state_are_refused() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);
    let uri = "file:///workspace/main.ws";

    // A request before initialize is refused ServerNotInitialized.
    client.send(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "textDocument/hover",
        "params": position_request(uri, 0, 0),
    }));
    let refused = client.read_message();
    assert_eq!(refused["id"], 1);
    assert_eq!(refused["error"]["code"], -32002, "{refused}");

    // A notification before initialize is dropped without a response —
    // the document it carried is never registered.
    open_document(&mut client, uri, "workshop", WORKSHOP_SOURCE);

    // Shutdown before initialize is refused too.
    client.send(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "shutdown",
    }));
    let refused = client.read_message();
    assert_eq!(refused["id"], 2);
    assert_eq!(refused["error"]["code"], -32002, "{refused}");

    let init = initialize(&mut client);
    assert_eq!(init["result"]["serverInfo"]["name"], "wright-lsp");
    client.notify("initialized", serde_json::json!({}));

    // The pre-initialize didOpen was dropped: hover on that URI sees no
    // buffer and answers null.
    let hover = client.request(3, "textDocument/hover", position_request(uri, 19, 9));
    assert!(
        hover["result"].is_null() && hover.get("error").is_none(),
        "a document opened before initialize must not exist: {hover}"
    );

    // A second initialize is InvalidRequest and does not reset state.
    client.send(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "initialize",
        "params": { "capabilities": {} },
    }));
    let refused = client.read_message();
    assert_eq!(refused["error"]["code"], -32600, "{refused}");

    let shutdown = client.request(5, "shutdown", serde_json::json!(null));
    assert!(shutdown["result"].is_null());

    // Requests after shutdown are refused InvalidRequest; the session
    // is over.
    client.send(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "textDocument/hover",
        "params": position_request(uri, 0, 0),
    }));
    let refused = client.read_message();
    assert_eq!(refused["error"]["code"], -32600, "{refused}");

    client.notify("exit", serde_json::json!({}));
    assert_eq!(client.wait_status(), Some(0));
}

#[test]
fn exit_before_shutdown_exits_with_status_1() {
    let root = workspace_root();

    // Never initialized.
    let mut client = LspClient::spawn(&root);
    client.notify("exit", serde_json::json!({}));
    assert_eq!(client.wait_status(), Some(1));

    // Initialized but not shut down.
    let mut client = LspClient::spawn(&root);
    initialize(&mut client);
    client.notify("exit", serde_json::json!({}));
    assert_eq!(client.wait_status(), Some(1));
}

#[test]
fn client_responses_and_methodless_messages_are_never_answered() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);
    initialize(&mut client);
    client.notify("initialized", serde_json::json!({}));

    // Client → server response messages carry an id and a result but no
    // method; answering them would be a protocol violation. Bare or
    // id-only objects get no response either.
    client.send(serde_json::json!({"jsonrpc": "2.0", "id": 90, "result": {"x": 1}}));
    client.send(serde_json::json!({"jsonrpc": "2.0", "id": 91}));
    client.send(serde_json::json!({"jsonrpc": "2.0"}));

    // The next wire message must be the answer to this request — any
    // spurious response to the noise above would arrive first with a
    // wrong id.
    let hover = client.request(
        2,
        "textDocument/hover",
        position_request("file:///workspace/main.ws", 0, 0),
    );
    assert_eq!(hover["id"], 2, "{hover}");

    let shutdown = client.request(3, "shutdown", serde_json::json!(null));
    assert!(shutdown["result"].is_null());
    client.notify("exit", serde_json::json!({}));
    assert_eq!(client.wait_status(), Some(0));
}

#[test]
fn stale_or_ranged_didchange_neither_republishes_nor_corrupts() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);
    initialize(&mut client);
    client.notify("initialized", serde_json::json!({}));
    let uri = "file:///workspace/main.ws";

    open_document(&mut client, uri, "workshop", BROKEN_SOURCE);
    let published = client.read_notification("textDocument/publishDiagnostics");
    assert_eq!(published["params"]["version"], 1);
    assert_eq!(
        published["params"]["diagnostics"].as_array().unwrap().len(),
        1,
        "the broken v1 buffer publishes its parse error"
    );

    // A stale version is rejected by the store — and must not republish
    // the old diagnostics.
    client.notify(
        "textDocument/didChange",
        serde_json::json!({
            "textDocument": { "uri": uri, "version": 1 },
            "contentChanges": [{ "text": WORKSHOP_SOURCE }],
        }),
    );
    // A ranged change is not a full-document replacement under the
    // negotiated Full sync; applying it as one would corrupt the buffer,
    // so it is dropped like the stale one.
    client.notify(
        "textDocument/didChange",
        serde_json::json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{
                "range": lsp_range((0, 0), (0, 9)).clone(),
                "text": "vars",
            }],
        }),
    );
    // The next conforming full change applies and republishes.
    client.notify(
        "textDocument/didChange",
        serde_json::json!({
            "textDocument": { "uri": uri, "version": 3 },
            "contentChanges": [{ "text": WORKSHOP_SOURCE }],
        }),
    );
    let republished = client.read_notification("textDocument/publishDiagnostics");
    assert_eq!(
        republished["params"]["version"], 3,
        "the first republish must be the conforming v3 change — a stale or \
         ranged change that republished would arrive first: {republished}"
    );
    assert!(
        republished["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "the clean v3 buffer clears diagnostics: {republished}"
    );

    // The stored buffer is the v3 replacement — hover still resolves the
    // rule name at its span.
    let hover = client.request(4, "textDocument/hover", position_request(uri, 19, 9));
    assert_eq!(
        hover["result"]["contents"]["value"], "`rule main`",
        "the v3 buffer is intact: {hover}"
    );

    let shutdown = client.request(5, "shutdown", serde_json::json!(null));
    assert!(shutdown["result"].is_null());
    client.notify("exit", serde_json::json!({}));
    assert_eq!(client.wait_status(), Some(0));
}

#[test]
fn diagnostics_publish_under_the_clients_raw_document_uri() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);
    initialize(&mut client);
    client.notify("initialized", serde_json::json!({}));

    // The store is keyed by the URI string the client sent; publishing a
    // normalized respelling would leave diagnostics attached to a
    // document the client never opened (#637).
    let uri = "file://localhost/workspace/main.ws";
    open_document(&mut client, uri, "workshop", BROKEN_SOURCE);
    let published = client.read_notification("textDocument/publishDiagnostics");
    assert_eq!(
        published["params"]["uri"], uri,
        "diagnostics publish under the client's raw URI: {published}"
    );
    assert_eq!(published["params"]["version"], 1);
    assert_eq!(
        published["params"]["diagnostics"].as_array().unwrap().len(),
        1
    );

    let shutdown = client.request(2, "shutdown", serde_json::json!(null));
    assert!(shutdown["result"].is_null());
    client.notify("exit", serde_json::json!({}));
    assert_eq!(client.wait_status(), Some(0));
}

#[test]
fn a_notification_method_carrying_an_id_is_answered() {
    let root = workspace_root();
    let mut client = LspClient::spawn(&root);
    initialize(&mut client);

    // `initialized` is notification-only in LSP, but a message carrying
    // an `id` is a JSON-RPC request and must get a response (#637).
    client.send(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "initialized",
        "params": {},
    }));
    let response = client.read_message();
    assert_eq!(response["id"], 2);
    assert!(response["result"].is_null() && response.get("error").is_none());

    // The same holds for a document notification sent as a request: the
    // open applies, its diagnostics publish, and the request answers.
    client.send(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///workspace/main.ws",
                "languageId": "workshop",
                "version": 1,
                "text": BROKEN_SOURCE,
            }
        },
    }));
    client.read_notification("textDocument/publishDiagnostics");
    let response = client.read_message();
    assert_eq!(response["id"], 3);
    assert!(response["result"].is_null() && response.get("error").is_none());

    let response = client.request(4, "shutdown", serde_json::json!(null));
    assert_eq!(response["id"], 4);
    client.notify("exit", serde_json::json!({}));
    assert_eq!(client.wait_status(), Some(0));
}
