use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

struct LspClient {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    /// The isolated provider data dir this spawn ran with — kept so
    /// `Drop` can remove it.
    provider_dir: PathBuf,
}

impl LspClient {
    fn spawn(cwd: &Path) -> Self {
        Self::launch(Command::new(env!("CARGO_BIN_EXE_wright-lsp")).current_dir(cwd))
    }

    /// A server launched with `--opy-provider` pointing at a staged fake
    /// provider; `caps` is the comma-separated `FAKE_CAPS` list the fake
    /// advertises on `lpp/initialize` (#738).
    #[cfg(unix)]
    fn spawn_with_provider(cwd: &Path, provider: &Path, caps: &str) -> Self {
        Self::launch(
            Command::new(env!("CARGO_BIN_EXE_wright-lsp"))
                .current_dir(cwd)
                .arg("--opy-provider")
                .arg(provider)
                .env("FAKE_CAPS", caps),
        )
    }

    fn launch(command: &mut Command) -> Self {
        // An empty provider data dir keeps a developer's installed OPY
        // provider out of these tests: provider resolution is either the
        // explicit `--opy-provider` flag or deterministically absent.
        let provider_dir = std::env::temp_dir().join(format!(
            "wright-lsp-test-{}-{}",
            std::process::id(),
            SPAWN_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&provider_dir).unwrap();
        let mut child = command
            .env("WRIGHT_PROVIDER_DATA_DIR", &provider_dir)
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
            provider_dir,
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

    /// The `publishDiagnostics` notification for `uri` — a set-wide
    /// provider check can publish several sources per refresh, so the
    /// test reads until the right document reports (#738).
    fn read_diagnostics_for(&mut self, uri: &str) -> serde_json::Value {
        loop {
            let message = self.read_notification("textDocument/publishDiagnostics");
            if message["params"]["uri"] == uri {
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
        let _ = std::fs::remove_dir_all(&self.provider_dir);
    }
}

static SPAWN_COUNTER: AtomicUsize = AtomicUsize::new(0);

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
    for capability in [
        "completionProvider",
        "semanticTokensProvider",
        // OPY document symbols are provider-backed only (#738): no
        // negotiated provider — the test runs against an isolated empty
        // provider store — means no advertisement.
        "documentSymbolProvider",
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
        serde_json::json!([{
            "uri": uri,
            "range": lsp_range((6, 7), (6, 13)),
        }]),
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

    // With no negotiated provider, OPY document symbols are not backed
    // (#738): the request answers null rather than an error.
    let symbols = client.request(
        6,
        "textDocument/documentSymbol",
        serde_json::json!({
            "textDocument": { "uri": "file:///workspace/main.opy" },
        }),
    );
    assert!(
        symbols["result"].is_null() && symbols.get("error").is_none(),
        "document symbols without provider backing answer null: {symbols}"
    );

    // Hover is raw Workshop only; the provider navigation requests answer
    // null without negotiated provider backing either.
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

    client.send_raw(b"Content-Length: 2\r\n\r\n\xff\xfe");
    let error = client.read_message();
    assert_eq!(error["error"]["code"], -32700, "{error}");

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

    let mut client = LspClient::spawn(&root);
    client.notify("exit", serde_json::json!({}));
    assert_eq!(client.wait_status(), Some(1));

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

// ── Provider-backed OPY diagnostics and navigation (#738) ─────────────
//
// The staged provider is a newline-JSON-RPC script that advertises the
// `FAKE_CAPS` capability list on `lpp/initialize` and serves `lpp/check`,
// `lpp/symbols`, `lpp/definition`, and `lpp/references` from the document
// text it is sent — the editor's overlay — resolving `#!include`/`#include`
// targets from disk for cross-file answers.

#[cfg(unix)]
const FAKE_OPY_PROVIDER: &str = r#"#!/usr/bin/env python3
import json, os, re, sys, urllib.parse

CAPS = {c for c in os.environ.get("FAKE_CAPS", "").split(",") if c}
CAP_OF = {"lpp/check": "check", "lpp/symbols": "symbols",
          "lpp/definition": "definition", "lpp/references": "references"}
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
DECL = re.compile(r"^\s*(globalvar|playervar|subroutine)\s+([A-Za-z_][A-Za-z0-9_]*)")
KIND = {"globalvar": "global", "playervar": "player", "subroutine": "subroutine"}
INCLUDE = re.compile(r"^\s*#!?include\s+[\"']([^\"']+)[\"']")

def reply(id, result=None, error=None):
    message = {"jsonrpc": "2.0", "id": id}
    message.update({"error": error} if error else {"result": result})
    print(json.dumps(message), flush=True)

def lpp_error(id, kind, details, text):
    reply(id, error={"code": -32000, "message": text,
                     "data": {"lpp": {"kind": kind, "details": details}}})

def path_of(uri):
    return urllib.parse.unquote(urllib.parse.urlparse(uri).path)

def uri_of(path):
    return "file://" + urllib.parse.quote(os.path.abspath(path))

def rng(line, col, end):
    return {"start": {"line": line, "character": col},
            "end": {"line": line, "character": end}}

def declarations(text):
    for i, line in enumerate(text.splitlines()):
        m = DECL.match(line)
        if m:
            yield m.group(2), i, line.index(m.group(2)), m.group(1)

def include_paths(doc):
    for line in doc["text"].splitlines():
        m = INCLUDE.match(line)
        if m:
            yield os.path.join(os.path.dirname(path_of(doc["uri"])), m.group(1))

def included_texts(doc):
    for path in include_paths(doc):
        if os.path.exists(path):
            yield uri_of(path), open(path).read()

def word_at(text, position):
    lines = text.splitlines()
    if position["line"] >= len(lines):
        return None
    line = lines[position["line"]]
    for m in IDENT.finditer(line):
        if m.start() <= position["character"] < m.end():
            return m.group(0)
    return None

def decl_site(doc, name):
    if name is not None:
        for decl, i, col, _kw in declarations(doc["text"]):
            if decl == name:
                return doc["uri"], rng(i, col, col + len(name))
        for uri, text in included_texts(doc):
            for decl, i, col, _kw in declarations(text):
                if decl == name:
                    return uri, rng(i, col, col + len(name))
    return None

def occurrences(doc, name):
    for i, line in enumerate(doc["text"].splitlines()):
        for m in re.finditer(r"\b%s\b" % re.escape(name), line):
            yield doc["uri"], rng(i, m.start(), m.end())
    for uri, text in included_texts(doc):
        for i, line in enumerate(text.splitlines()):
            for m in re.finditer(r"\b%s\b" % re.escape(name), line):
                yield uri, rng(i, m.start(), m.end())

for line in sys.stdin:
    request = json.loads(line)
    id, method = request["id"], request["method"]
    if method == "lpp/initialize":
        reply(id, {"protocolVersion": request["params"]["protocolVersion"],
                   "serverInfo": {"name": "fake-opy-provider", "version": "0"},
                   "languages": [{"id": "opy", "extensions": ["opy"]}],
                   "capabilities": {c: (c in CAPS) for c in
                        ["check", "compile", "reconstruct", "symbols",
                         "definition", "references", "rename",
                         "editValidation", "projectLoading"]}})
    elif method in CAP_OF and CAP_OF[method] not in CAPS:
        lpp_error(id, "capabilityUnavailable",
                  {"capability": CAP_OF[method], "method": method},
                  "capability not negotiated")
    elif method == "lpp/check":
        docs = []
        for uri, doc in request["params"]["documents"].items():
            diags = [{"range": rng(i, m.start(), m.end()), "severity": "error",
                      "code": "fake-oops", "message": "invalid token 'oops'"}
                     for i, l in enumerate(doc["text"].splitlines())
                     for m in re.finditer(r"\boops\b", l)]
            docs.append({"uri": uri, "version": doc["version"], "diagnostics": diags})
        reply(id, {"documents": docs})
    elif method == "lpp/symbols":
        docs = []
        for uri, doc in request["params"]["documents"].items():
            syms = [{"name": name, "kind": KIND[kw],
                     "range": rng(i, col, col + len(name)),
                     "selectionRange": rng(i, col, col + len(name))}
                    for name, i, col, kw in declarations(doc["text"])]
            docs.append({"uri": uri, "version": doc["version"], "symbols": syms})
        reply(id, {"documents": docs})
    elif method == "lpp/definition":
        doc, pos = request["params"]["document"], request["params"]["position"]
        site = decl_site(doc, word_at(doc["text"], pos))
        if site is None:
            lpp_error(id, "refusal",
                      {"refusalCode": "definition.noSymbolAtPosition"},
                      "no symbol at position")
        else:
            reply(id, {"locations": [{"uri": site[0], "range": site[1]}]})
    elif method == "lpp/references":
        doc, pos = request["params"]["document"], request["params"]["position"]
        name = word_at(doc["text"], pos)
        site = decl_site(doc, name)
        if site is None:
            lpp_error(id, "refusal",
                      {"refusalCode": "references.noSymbolAtPosition"},
                      "no symbol at position")
        else:
            locs = [{"uri": u, "range": r} for u, r in occurrences(doc, name)]
            if not request["params"].get("includeDeclaration", True):
                locs = [l for l in locs
                        if not (l["uri"] == site[0] and l["range"] == site[1])]
            reply(id, {"locations": locs})
    elif method == "lpp/shutdown":
        reply(id)
        sys.exit(0)
    else:
        reply(id, {})
"#;

/// Stage the fake provider as an executable file and return its path.
#[cfg(unix)]
fn stage_fake_provider(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("fake-opy-provider");
    std::fs::write(&path, FAKE_OPY_PROVIDER).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A real workspace directory under the temp tree — `#!include` targets
/// the fake provider resolves from disk live here (#738).
#[cfg(unix)]
fn opy_workspace(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wright-lsp-opy-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(unix)]
fn file_uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

#[cfg(unix)]
#[test]
fn opy_capabilities_are_advertised_from_the_negotiated_provider() {
    let root = workspace_root();
    let dir = opy_workspace("provider-caps");
    let provider = stage_fake_provider(&dir);
    let mut client =
        LspClient::spawn_with_provider(&root, &provider, "check,symbols,definition,references");
    let init = initialize(&mut client);
    let capabilities = init["result"]["capabilities"].as_object().unwrap();
    assert_eq!(
        capabilities["documentSymbolProvider"], true,
        "lpp/symbols was negotiated, so the capability advertises: {init}"
    );
    // The session stays up and still serves.
    let shutdown = client.request(2, "shutdown", serde_json::json!(null));
    assert!(shutdown["result"].is_null());
    client.notify("exit", serde_json::json!(null));
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn opy_diagnostics_come_from_the_open_buffer_not_the_disk() {
    let root = workspace_root();
    let dir = opy_workspace("provider-diag");
    let provider = stage_fake_provider(&dir);
    let mut client = LspClient::spawn_with_provider(&root, &provider, "check");
    initialize(&mut client);
    client.notify("initialized", serde_json::json!({}));

    // `main.opy` never exists on disk: the only place this text lives is
    // the open buffer — a diagnostic on `oops` can only come from the
    // overlay document the provider was sent (#738).
    let uri = "file:///workspace/main.opy";
    client.notify(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": "opy",
                "version": 1,
                "text": "globalvar score = oops\n",
            }
        }),
    );
    let published = client.read_diagnostics_for(uri);
    let diagnostics = published["params"]["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["code"], "fake-oops");
    assert_eq!(
        diagnostics[0]["range"],
        lsp_range((0, 18), (0, 22)),
        "the provider's authored range carries through unchanged"
    );

    // Fixing the buffer republishes an empty set — the stale diagnostic
    // retires rather than lingering.
    client.notify(
        "textDocument/didChange",
        serde_json::json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{ "text": "globalvar score = 0\n" }],
        }),
    );
    let republished = client.read_diagnostics_for(uri);
    assert!(
        republished["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "a clean buffer clears provider diagnostics: {republished}"
    );

    let shutdown = client.request(2, "shutdown", serde_json::json!(null));
    assert!(shutdown["result"].is_null());
    client.notify("exit", serde_json::json!(null));
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn opy_navigation_routes_through_the_provider_across_includes() {
    let workspace = opy_workspace("nav");
    let provider = stage_fake_provider(&workspace);
    let mut client = LspClient::spawn_with_provider(
        &workspace,
        &provider,
        "check,symbols,definition,references",
    );
    client.request(
        1,
        "initialize",
        serde_json::json!({
            "processId": null,
            "rootUri": file_uri(&workspace),
            "capabilities": {},
        }),
    );
    client.notify("initialized", serde_json::json!({}));

    // `defs.opy` exists only on disk — the provider owns cross-file
    // project semantics; `main.opy` exists only as an open buffer, so its
    // text is the overlay the provider was sent.
    let defs = workspace.join("defs.opy");
    std::fs::write(&defs, "globalvar included_var = 0\n").unwrap();
    let defs_uri = file_uri(&defs);
    let main_uri = file_uri(&workspace.join("main.opy"));
    let main_text = concat!(
        "#!include \"defs.opy\"\n",
        "\n",
        "rule \"use\":\n",
        "    @Event global\n",
        "    included_var = 2\n",
    );
    client.notify(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": main_uri,
                "languageId": "opy",
                "version": 1,
                "text": main_text,
            }
        }),
    );
    client.read_diagnostics_for(&main_uri);

    // `lpp/symbols` backs textDocument/documentSymbol: `defs.opy` is not
    // open, so only the symbols the provider declares in `main.opy`'s
    // buffer arrive — the fake declares none, which is still a real
    // answer ([]), unlike null.
    let symbols = client.request(
        2,
        "textDocument/documentSymbol",
        serde_json::json!({ "textDocument": { "uri": main_uri } }),
    );
    assert_eq!(
        symbols["result"],
        serde_json::json!([]),
        "the provider answers with its own symbol list: {symbols}"
    );

    // Definition on the `included_var` use (line 4, chars 4–16) resolves
    // through the `#!include` into `defs.opy` — a URI this session never
    // opened.
    let definition = client.request(
        3,
        "textDocument/definition",
        position_request(&main_uri, 4, 5),
    );
    assert_eq!(
        definition["result"],
        serde_json::json!([{
            "uri": defs_uri,
            "range": lsp_range((0, 10), (0, 22)),
        }]),
        "the provider's cross-file location arrives verbatim: {definition}"
    );

    // References honor `includeDeclaration` and span both files.
    let references = client.request(
        4,
        "textDocument/references",
        serde_json::json!({
            "textDocument": { "uri": main_uri },
            "position": { "line": 4, "character": 5 },
            "context": { "includeDeclaration": true },
        }),
    );
    assert_eq!(
        references["result"],
        serde_json::json!([
            { "uri": main_uri, "range": lsp_range((4, 4), (4, 16)) },
            { "uri": defs_uri, "range": lsp_range((0, 10), (0, 22)) },
        ]),
        "references span the open buffer and the included file: {references}"
    );
    let uses_only = client.request(
        5,
        "textDocument/references",
        serde_json::json!({
            "textDocument": { "uri": main_uri },
            "position": { "line": 4, "character": 5 },
            "context": { "includeDeclaration": false },
        }),
    );
    assert_eq!(
        uses_only["result"],
        serde_json::json!([{ "uri": main_uri, "range": lsp_range((4, 4), (4, 16)) }]),
        "includeDeclaration=false drops the declaration site: {uses_only}"
    );

    // A position on no identifier is a provider refusal, surfaced as a
    // null result — never an LSP error.
    let miss = client.request(
        6,
        "textDocument/definition",
        position_request(&main_uri, 1, 0),
    );
    assert!(
        miss["result"].is_null() && miss.get("error").is_none(),
        "noSymbolAtPosition answers null: {miss}"
    );

    let shutdown = client.request(7, "shutdown", serde_json::json!(null));
    assert!(shutdown["result"].is_null());
    client.notify("exit", serde_json::json!(null));
    let _ = std::fs::remove_dir_all(&workspace);
}

#[cfg(unix)]
#[test]
fn opy_navigation_is_unadvertised_and_answers_null_without_provider_capabilities() {
    let root = workspace_root();
    let dir = opy_workspace("provider-caps-off");
    let provider = stage_fake_provider(&dir);
    // `check` only: the provider negotiates diagnostics but no
    // navigation, so nothing navigation-shaped may be advertised (#738).
    let mut client = LspClient::spawn_with_provider(&root, &provider, "check");
    let init = initialize(&mut client);
    assert!(
        init["result"]["capabilities"]
            .get("documentSymbolProvider")
            .is_none(),
        "the provider never negotiated lpp/symbols: {init}"
    );
    client.notify("initialized", serde_json::json!({}));

    let uri = "file:///workspace/main.opy";
    open_document(&mut client, uri, "opy", "globalvar score = 0\n");
    let published = client.read_diagnostics_for(uri);
    assert!(
        published["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "check still serves diagnostics: {published}"
    );

    for (id, method, params) in [
        (
            2,
            "textDocument/documentSymbol",
            serde_json::json!({ "textDocument": { "uri": uri } }),
        ),
        (3, "textDocument/definition", position_request(uri, 0, 11)),
        (
            4,
            "textDocument/references",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": 0, "character": 11 },
                "context": { "includeDeclaration": true },
            }),
        ),
    ] {
        let response = client.request(id, method, params);
        assert!(
            response["result"].is_null() && response.get("error").is_none(),
            "{method} without negotiated provider backing answers null: {response}"
        );
    }

    let shutdown = client.request(5, "shutdown", serde_json::json!(null));
    assert!(shutdown["result"].is_null());
    client.notify("exit", serde_json::json!(null));
    let _ = std::fs::remove_dir_all(&dir);
}
