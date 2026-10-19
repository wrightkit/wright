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
