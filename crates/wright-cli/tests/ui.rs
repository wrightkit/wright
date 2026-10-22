//! `wright ui` (ADR-0032): the loopback endpoint carries `wright-agent/v1`
//! bodies to `ToolService` unchanged, gated by the session token and the
//! loopback `Host`/`Origin` checks. These tests spawn the built `wright`
//! binary like `tests/serve.rs`; the endpoint tests compile only with the
//! `ui` feature, which enables the embedded-assets server.

#[cfg(feature = "ui")]
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(feature = "ui")]
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(feature = "ui")]
use std::process::{Child, Stdio};

fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn corpus_workshop(id: &str) -> PathBuf {
    workspace_root()
        .join("tests/fixtures/workshop")
        .join(id)
        .with_extension("ws")
}

#[cfg(feature = "ui")]
struct UiSession {
    child: Child,
    authority: String,
    token: String,
}

#[cfg(feature = "ui")]
impl UiSession {
    /// `wright ui --no-open` prints the session URL — the loopback origin
    /// plus the per-session token in the fragment — as its first stdout
    /// line, then serves until killed.
    fn start(input: &Path) -> Self {
        let mut child = Command::new(wright())
            .args(["ui", "--no-open"])
            .arg(input)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("wright ui spawns");
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut url = String::new();
        stdout.read_line(&mut url).expect("session URL on stdout");
        let url = url.trim();
        let (base, token) = url
            .rsplit_once('#')
            .unwrap_or_else(|| panic!("session URL carries the token fragment: {url}"));
        let authority = base
            .strip_prefix("http://")
            .and_then(|rest| rest.strip_suffix('/'))
            .unwrap_or_else(|| panic!("session URL is a loopback http URL: {url}"));
        Self {
            child,
            authority: authority.to_string(),
            token: token.to_string(),
        }
    }

    /// One HTTP/1.0 request against the bound loopback address, returning
    /// `(status, body)`. Hand-rolled so the test controls `Host`/`Origin`
    /// exactly and no proxy configuration can intercept loopback traffic.
    fn request(
        &self,
        method: &str,
        path: &str,
        host: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> (u16, String) {
        let mut stream = TcpStream::connect(&self.authority).expect("connects to the UI server");
        let mut request = format!(
            "{method} {path} HTTP/1.0\r\nHost: {host}\r\nContent-Length: {}\r\n",
            body.len()
        );
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        request.push_str(body);
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        let response = String::from_utf8_lossy(&response);
        let (head, body) = response
            .split_once("\r\n\r\n")
            .expect("HTTP response split");
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .expect("HTTP status code");
        (status, body.to_string())
    }

    fn api(&self, body: &str) -> (u16, String) {
        self.request(
            "POST",
            "/api",
            &self.authority,
            &[("Authorization", &format!("Bearer {}", self.token))],
            body,
        )
    }
}

#[cfg(feature = "ui")]
impl Drop for UiSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(feature = "ui")]
#[test]
fn idle_connections_do_not_block_pipelined_keep_alive_responses() {
    let session = UiSession::start(&corpus_workshop("synthetic/basic-rule"));
    let _idle = TcpStream::connect(&session.authority).unwrap();
    let mut stream = TcpStream::connect(&session.authority).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let request = format!("GET / HTTP/1.1\r\nHost: {}\r\n\r\n", session.authority);
    stream
        .write_all(format!("{request}{request}").as_bytes())
        .unwrap();
    let mut reader = BufReader::new(stream);
    for _ in 0..2 {
        let mut status = String::new();
        reader.read_line(&mut status).unwrap();
        assert!(status.starts_with("HTTP/1.1 200"), "{status}");
        let mut length = None;
        loop {
            let mut header = String::new();
            reader.read_line(&mut header).unwrap();
            if header == "\r\n" {
                break;
            }
            let (name, value) = header.split_once(':').expect("HTTP header");
            if name.eq_ignore_ascii_case("Content-Length") {
                length = Some(value.trim().parse::<usize>().unwrap());
            }
            if name.eq_ignore_ascii_case("Connection") {
                assert_ne!(value.trim().to_ascii_lowercase(), "close");
            }
        }
        let mut body = vec![0; length.expect("response length")];
        reader.read_exact(&mut body).unwrap();
        assert!(!body.is_empty());
    }
}

#[cfg(feature = "ui")]
fn serve_stdio(input: &Path, lines: &[&str]) -> Vec<String> {
    let mut child = Command::new(wright())
        .args(["serve", "--transport", "stdio"])
        .arg(input)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("wright serve spawns");
    let mut stdin = child.stdin.take().unwrap();
    for line in lines {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "wright serve failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

/// The endpoint answers exactly what `wright serve` writes for the same
/// request and project — the transport adds no envelope (ADR-0032
/// decision 3).
#[cfg(feature = "ui")]
#[test]
fn endpoint_bodies_match_the_stdio_transport() {
    let input = corpus_workshop("synthetic/control-flow");
    let requests = [
        r#"{"op":"capabilities"}"#,
        r#"{"op":"rules"}"#,
        r#"{"op":"rules","content":true}"#,
        r#"{"op":"lookup","language":"workshop","query":"wait"}"#,
        r#"{"op":"no-such-operation"}"#,
    ];
    let expected = serve_stdio(&input, &requests);
    let session = UiSession::start(&input);
    for (request, expected) in requests.iter().zip(expected.iter()) {
        let (status, body) = session.api(request);
        assert_eq!(status, 200, "{request}");
        // The stdio transport writes each response as one line; the
        // endpoint returns the same bytes, terminator included.
        assert_eq!(body, format!("{expected}\n"), "{request}");
    }
}

/// `project` lists the loaded sources through `sourceFiles`, entry file
/// first — the page title shows its base name (#879).
#[cfg(feature = "ui")]
#[test]
fn project_lists_source_files_entry_first() {
    let session = UiSession::start(&corpus_workshop("synthetic/control-flow"));
    let (status, body) = session.api(r#"{"op":"project"}"#);
    assert_eq!(status, 200);
    let result: serde_json::Value = serde_json::from_str(&body).expect("project body is JSON");
    let files = result["result"]["sourceFiles"]
        .as_array()
        .expect("project reports sourceFiles");
    assert_eq!(files.len(), 1);
    assert!(
        files[0].as_str().unwrap().ends_with("control-flow.ws"),
        "the entry source names the input file: {files:?}"
    );
}

/// Every refusal below is answered before the request can reach
/// `ToolService`: a missing or wrong token, a foreign `Host`, or a foreign
/// `Origin` (ADR-0032 decision 4).
#[cfg(feature = "ui")]
#[test]
fn endpoint_refuses_foreign_or_unauthenticated_requests() {
    let session = UiSession::start(&corpus_workshop("synthetic/basic-rule"));
    let bearer = format!("Bearer {}", session.token);
    let origin = format!("http://{}", session.authority);
    type Case<'a> = (&'a str, &'a str, Vec<(&'a str, &'a str)>);
    let cases: [Case; 5] = [
        ("missing token", &session.authority, vec![]),
        (
            "wrong token",
            &session.authority,
            vec![("Authorization", "Bearer 00000000000000000000000000000000")],
        ),
        (
            "foreign host",
            "ui.example.com",
            vec![("Authorization", bearer.as_str())],
        ),
        (
            "foreign origin",
            &session.authority,
            vec![
                ("Authorization", bearer.as_str()),
                ("Origin", "https://ui.example.com"),
            ],
        ),
        (
            "foreign host and origin",
            "ui.example.com",
            vec![
                ("Authorization", bearer.as_str()),
                ("Origin", origin.as_str()),
            ],
        ),
    ];
    for (case, host, headers) in &cases {
        let (status, body) =
            session.request("POST", "/api", host, headers, r#"{"op":"capabilities"}"#);
        assert_eq!(status, 403, "{case}");
        assert!(
            !body.contains("\"result\""),
            "{case}: refused before the service"
        );
    }
}

/// A request that passes the checks answers with no CORS headers, and the
/// loopback page is served.
#[cfg(feature = "ui")]
#[test]
fn serves_the_embedded_page_without_cors_headers() {
    let session = UiSession::start(&corpus_workshop("synthetic/basic-rule"));

    let (status, body) = session.request("GET", "/", &session.authority, &[], "");
    assert_eq!(status, 200);
    assert!(
        body.contains("<html") || body.contains("<!doctype html"),
        "{body}"
    );

    let mut stream = TcpStream::connect(&session.authority).unwrap();
    write!(
        stream,
        "POST /api HTTP/1.0\r\nHost: {}\r\nContent-Length: 21\r\nAuthorization: Bearer {}\r\n\r\n",
        session.authority, session.token
    )
    .unwrap();
    stream.write_all(br#"{"op":"capabilities"}"#).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let head = response.split("\r\n\r\n").next().unwrap();
    assert!(
        !head.to_ascii_lowercase().contains("access-control"),
        "the endpoint emits no CORS headers: {head}"
    );
}

/// The URL's host is always a loopback literal — the server binds
/// `127.0.0.1` only (ADR-0032 decision 4).
#[cfg(feature = "ui")]
#[test]
fn the_session_url_is_a_loopback_url() {
    let session = UiSession::start(&corpus_workshop("synthetic/basic-rule"));
    assert!(
        session.authority.starts_with("127.0.0.1:"),
        "{}",
        session.authority
    );
}

#[test]
fn refuses_non_workshop_input_naming_the_owner() {
    let opy = workspace_root().join("tests/fixtures/opy/basic-rule.opy");
    let output = Command::new(wright())
        .args(["ui", "--no-open"])
        .arg(&opy)
        .output()
        .unwrap();
    #[cfg(feature = "ui")]
    {
        assert_eq!(output.status.code(), Some(3));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("OPY"), "{stderr}");
        assert!(stderr.contains("opy-rs"), "{stderr}");
    }
    #[cfg(not(feature = "ui"))]
    {
        // Without the feature the no-UI message wins over input handling.
        assert_eq!(output.status.code(), Some(2));
    }

    let output = Command::new(wright())
        .args(["ui", "--no-open", "--kind", "ostw"])
        .arg(corpus_workshop("synthetic/basic-rule"))
        .output()
        .unwrap();
    #[cfg(feature = "ui")]
    {
        assert_eq!(output.status.code(), Some(3));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("OSTW"), "{stderr}");
        assert!(stderr.contains("deltin-rs"), "{stderr}");
    }
    #[cfg(not(feature = "ui"))]
    {
        assert_eq!(output.status.code(), Some(2));
    }
}

#[test]
fn refuses_stdin_input() {
    let output = Command::new(wright()).args(["ui", "-"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    #[cfg(feature = "ui")]
    assert!(
        stderr.contains("does not read the program from stdin"),
        "{stderr}"
    );
    #[cfg(not(feature = "ui"))]
    assert!(stderr.contains("does not include"), "{stderr}");
}

#[cfg(feature = "ui")]
impl UiSession {
    fn apply(&self, body: &str) -> (u16, String) {
        self.request(
            "POST",
            "/apply",
            &self.authority,
            &[("Authorization", &format!("Bearer {}", self.token))],
            body,
        )
    }
}

/// A writable copy of the control-flow fixture: apply tests edit the
/// file, so they never run against the committed source.
#[cfg(feature = "ui")]
fn editable_input(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wright-ui-apply-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::copy(corpus_workshop("synthetic/control-flow"), &path).unwrap();
    path
}

/// Stage a `disabled` toggle on rule 0's `Wait` action through
/// `editStatements` — the same replace a checkbox in the page produces —
/// and return the transaction the result carries.
#[cfg(feature = "ui")]
fn staged_transaction(session: &UiSession) -> serde_json::Value {
    let (_, project) = session.api(r#"{"op":"project"}"#);
    let project: serde_json::Value = serde_json::from_str(&project).unwrap();
    let identity = project["result"]["inputIdentity"].as_str().unwrap();
    let (_, rules) = session.api(r#"{"op":"rules","all":true,"content":true}"#);
    let rules: serde_json::Value = serde_json::from_str(&rules).unwrap();
    let snapshot = &rules["result"]["snapshot"];
    // `span` is output-only; the caller drops it before reusing the node.
    let mut node = rules["result"]["rules"][0]["content"]["actions"][1].clone();
    let object = node.as_object_mut().unwrap();
    object.remove("span");
    object.insert("disabled".into(), true.into());
    let request = serde_json::json!({
        "op": "editStatements",
        "inputIdentity": identity,
        "snapshot": snapshot,
        "edits": [{
            "rule": 0,
            "list": "actions",
            "index": 1,
            "change": "replace",
            "statement": node,
        }],
    });
    let (status, body) = session.api(&request.to_string());
    assert_eq!(status, 200, "{body}");
    let response: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(response["result"]["ok"], true, "{response}");
    response["result"]["transaction"].clone()
}

/// `POST /apply` (ADR-0032 decision 5): the host re-validates the
/// transaction against the files on disk, writes atomically, and serves
/// the edited program under a new generation.
#[cfg(feature = "ui")]
#[test]
fn apply_writes_the_transaction_and_serves_the_edit() {
    let path = editable_input("apply-ok.ws");
    let session = UiSession::start(&path);
    let transaction = staged_transaction(&session);
    // The pre-write generation the listing issued.
    let (_, before) = session.api(r#"{"op":"rules","all":true}"#);
    let before: serde_json::Value = serde_json::from_str(&before).unwrap();
    let generation = before["result"]["snapshot"].as_u64().unwrap();

    let (status, body) = session.apply(&transaction.to_string());
    assert_eq!(status, 200, "{body}");
    let applied: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(applied["ok"], true, "{applied}");

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("disabled"), "{text}");
    let (_, rules) = session.api(r#"{"op":"rules","all":true,"content":true}"#);
    let rules: serde_json::Value = serde_json::from_str(&rules).unwrap();
    assert_eq!(
        rules["result"]["rules"][0]["content"]["actions"][1]["disabled"],
        serde_json::json!(true),
        "{rules}"
    );
    // The write adopted a fresh generation: a numeric address the old
    // program issued is now stale.
    let request =
        serde_json::json!({"op": "rules", "rule": 0, "snapshot": generation, "content": true});
    let (_, stale) = session.api(&request.to_string());
    assert!(
        stale.contains("stale-id"),
        "old generation refuses: {stale}"
    );
}

/// A transaction whose `source_identity` no longer matches the file is
/// refused `edit-stale-source` and writes nothing.
#[cfg(feature = "ui")]
#[test]
fn apply_refuses_a_stale_transaction_and_writes_nothing() {
    let path = editable_input("apply-stale.ws");
    let session = UiSession::start(&path);
    let transaction = staged_transaction(&session);

    let original = std::fs::read_to_string(&path).unwrap();
    let drifted = format!("{original}\n");
    std::fs::write(&path, &drifted).unwrap();
    let (status, body) = session.apply(&transaction.to_string());
    assert_eq!(status, 200, "{body}");
    let applied: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(applied["ok"], false, "{applied}");
    assert_eq!(
        applied["diagnostics"][0]["code"],
        serde_json::json!("edit-stale-source"),
        "{applied}"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), drifted);
}

/// `/apply` gates like `/api`: the session token is required, and the
/// body must deserialize as an edit transaction.
#[cfg(feature = "ui")]
#[test]
fn apply_requires_the_session_token_and_a_transaction_body() {
    let session = UiSession::start(&corpus_workshop("synthetic/basic-rule"));
    let (status, _) = session.request("POST", "/apply", &session.authority, &[], "{}");
    assert_eq!(status, 403, "the token is required");

    let (status, body) = session.apply("not json");
    assert_eq!(status, 400, "{body}");
    assert!(body.contains("malformed-request"), "{body}");

    // A transaction that cannot validate answers the refusal shape, not a
    // transport error.
    let (status, body) = session.apply(r#"{"edits":[]}"#);
    assert_eq!(status, 200, "{body}");
    let applied: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(applied["ok"], false, "{applied}");
    assert_eq!(
        applied["diagnostics"][0]["code"],
        serde_json::json!("edit-empty-transaction"),
        "{applied}"
    );
}

/// Builds without the `ui` feature report the no-UI build and exit 2 —
/// `cargo test` needs no Node toolchain (ADR-0032 decision 9).
#[cfg(not(feature = "ui"))]
#[test]
fn reports_a_build_without_ui() {
    let output = Command::new(wright())
        .args(["ui", "--no-open"])
        .arg(corpus_workshop("synthetic/basic-rule"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does not include"), "{stderr}");
}
