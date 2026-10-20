//! Offline lint package store tests: every remote answer is served by a
//! recorded-response HTTP server on localhost; no test touches real network.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use wright_driver::{LintPackageSource, LintPackageStore, RemoteCheck};

const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

const RULE: &str = "\
id: community/minimum-wait
metadata:
  summary: loop contains a minimum wait
  rationale: minimum waits can create high-frequency loops
  documentation: Finds a minimum wait inside a while scope.
  known-limits: This is structural and does not measure runtime cost.
  tags: [performance]
matcher:
  scope: while
  actions:
    - kind: call
";

const SECOND_RULE: &str = "\
id: community/has-wait
metadata:
  summary: rule contains a wait
  rationale: waits yield the scheduler
  documentation: Finds a wait call at rule scope.
  known-limits: Structural only.
  tags: [performance]
matcher:
  scope: rule
  actions:
    - kind: call
";

fn tarball(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, body) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, path, &body[..]).unwrap();
    }
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&builder.into_inner().unwrap()).unwrap();
    encoder.finish().unwrap()
}

fn package_tarball(rule: &str) -> Vec<u8> {
    tarball(&[
        ("o-r-aaaaaaaa/rules/rule.yaml", rule.as_bytes()),
        ("o-r-aaaaaaaa/README.md", b"# package"),
        ("o-r-aaaaaaaa/src/tool.sh", b"#!/bin/sh\n"),
    ])
}

#[derive(Clone)]
struct Request {
    method: String,
    path: String,
    authorization: Option<String>,
    if_none_match: Option<String>,
}

struct Response {
    status: &'static str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Response {
    fn ok(body: Vec<u8>) -> Self {
        Self {
            status: "200 OK",
            headers: Vec::new(),
            body,
        }
    }
    fn json(body: &str) -> Self {
        Self::ok(body.as_bytes().to_vec())
    }
    fn not_found() -> Self {
        Self {
            status: "404 Not Found",
            headers: Vec::new(),
            body: b"not found".to_vec(),
        }
    }
}

/// A localhost HTTP server replaying the handler's recorded responses.
struct TestServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<Request>>>,
    thread: Option<JoinHandle<()>>,
}

impl TestServer {
    fn serve(handler: impl Fn(&Request) -> Response + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let requests: Arc<Mutex<Vec<Request>>> = Arc::new(Mutex::new(Vec::new()));
        let (stop_flag, seen) = (Arc::clone(&stop), Arc::clone(&requests));
        let thread = thread::spawn(move || {
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // Accepted sockets inherit the listener's nonblocking
                        // flag on some platforms; request reads must block.
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                        let request = read_request(&mut stream);
                        let response = handler(&request);
                        write_response(&mut stream, &response);
                        seen.lock().unwrap().push(request);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if stop_flag.load(Ordering::Relaxed) {
                            break;
                        }
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            addr,
            stop,
            requests,
            thread: Some(thread),
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn requests(&self) -> Vec<(String, String)> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| (r.method.clone(), r.path.clone()))
            .collect()
    }

    fn find(&self, path_prefix: &str) -> Vec<Request> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path.starts_with(path_prefix))
            .cloned()
            .collect()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Request {
    let mut raw = Vec::new();
    let mut buffer = [0; 4096];
    while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => raw.extend_from_slice(&buffer[..n]),
        }
    }
    let text = String::from_utf8_lossy(&raw);
    let mut lines = text.lines();
    let mut request_line = lines.next().unwrap_or_default().split_whitespace();
    let (method, path) = (
        request_line.next().unwrap_or_default().to_string(),
        request_line.next().unwrap_or_default().to_string(),
    );
    let mut authorization = None;
    let mut if_none_match = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "authorization" => authorization = Some(value.trim().to_string()),
            "if-none-match" => if_none_match = Some(value.trim().to_string()),
            _ => {}
        }
    }
    Request {
        method,
        path,
        authorization,
        if_none_match,
    }
}

fn write_response(stream: &mut TcpStream, response: &Response) {
    let mut head = format!("HTTP/1.1 {}\r\n", response.status);
    for (name, value) in &response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        response.body.len()
    ));
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&response.body);
}

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!("wright-lint-package-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn source() -> LintPackageSource {
    LintPackageSource::new("community", "rules").unwrap()
}

fn store(dir: &Path, server: &TestServer) -> LintPackageStore {
    LintPackageStore::new(dir)
        .with_api_base(server.url())
        .with_codeload_base(server.url())
        .with_token(None)
}

fn store_entries(root: &Path) -> Vec<String> {
    let mut entries = Vec::new();
    for owner in std::fs::read_dir(root).into_iter().flatten().flatten() {
        for repo in std::fs::read_dir(owner.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            for sha in std::fs::read_dir(repo.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                entries.push(format!(
                    "{}/{}/{}",
                    owner.file_name().to_string_lossy(),
                    repo.file_name().to_string_lossy(),
                    sha.file_name().to_string_lossy()
                ));
            }
        }
    }
    entries.sort();
    entries
}

#[test]
fn full_sha_install_stores_only_rule_files() {
    let dir = TestDir::new("full-sha");
    let archive = package_tarball(RULE);
    let server = TestServer::serve(move |request| {
        if request.path == format!("/community/rules/tar.gz/{SHA_A}") {
            Response::ok(archive.clone())
        } else {
            Response::not_found()
        }
    });
    let store = store(dir.path(), &server);
    let installed = store.install(&source(), Some(SHA_A)).unwrap();
    assert!(installed.downloaded);
    assert_eq!(installed.sha, SHA_A);
    assert_eq!(installed.rules, ["community/minimum-wait"]);
    assert_eq!(
        store.installed(&source(), SHA_A),
        Some(dir.path().join("community/rules").join(SHA_A))
    );
    // Only rules/*.yaml|yml was extracted; repository code was not read.
    let rules = installed.rules_dir();
    assert_eq!(
        std::fs::read_dir(&rules)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["rule.yaml"]
    );
    assert!(
        std::fs::read_to_string(rules.join("rule.yaml"))
            .unwrap()
            .contains("community/minimum-wait")
    );
    // A full SHA needs no commits-API resolution.
    assert!(server.find("/repos/").is_empty());
}

#[test]
fn multi_rule_packages_load_in_lexical_file_order() {
    let dir = TestDir::new("multi-rule");
    let archive = tarball(&[
        // Lexical file order, not id order, sets the load order.
        ("o-r-aaaaaaaa/rules/b-second.yaml", RULE.as_bytes()),
        ("o-r-aaaaaaaa/rules/a-first.yaml", SECOND_RULE.as_bytes()),
    ]);
    let server = TestServer::serve(move |request| {
        if request.path == format!("/community/rules/tar.gz/{SHA_A}") {
            Response::ok(archive.clone())
        } else {
            Response::not_found()
        }
    });
    let store = store(dir.path(), &server);
    let installed = store.install(&source(), Some(SHA_A)).unwrap();
    assert_eq!(
        installed.rules,
        ["community/has-wait", "community/minimum-wait"]
    );
}

#[test]
fn second_fetch_of_an_intact_entry_makes_no_download() {
    let dir = TestDir::new("no-redownload");
    let archive = package_tarball(RULE);
    let server = TestServer::serve(move |request| {
        if request.path == format!("/community/rules/tar.gz/{SHA_A}") {
            Response::ok(archive.clone())
        } else {
            Response::not_found()
        }
    });
    let store = store(dir.path(), &server);
    store.install(&source(), Some(SHA_A)).unwrap();
    let again = store.install(&source(), Some(SHA_A)).unwrap();
    assert!(!again.downloaded);
    assert_eq!(again.rules, ["community/minimum-wait"]);
    assert_eq!(
        server.requests().len(),
        1,
        "no second download: {:?}",
        server.requests()
    );
}

#[test]
fn a_tag_resolves_to_the_full_sha_it_points_at() {
    let dir = TestDir::new("tag");
    let archive = package_tarball(RULE);
    let server = TestServer::serve(move |request| {
        if request.path == "/repos/community/rules/commits/v1.0" {
            Response::json(&format!("{{\"sha\":\"{SHA_A}\"}}"))
        } else if request.path == format!("/community/rules/tar.gz/{SHA_A}") {
            Response::ok(archive.clone())
        } else {
            Response::not_found()
        }
    });
    let store = store(dir.path(), &server);
    let resolved = store.resolve(&source(), Some("v1.0")).unwrap();
    assert_eq!(resolved, SHA_A);
    let installed = store.install(&source(), Some("v1.0")).unwrap();
    assert_eq!(installed.sha, SHA_A);
    assert_eq!(
        store_entries(dir.path()),
        [format!("community/rules/{SHA_A}")]
    );
}

#[test]
fn an_unchanged_ref_reuses_the_etag_stored_resolution() {
    let dir = TestDir::new("etag");
    let body = format!("{{\"sha\":\"{SHA_A}\"}}");
    let server = TestServer::serve(move |request| {
        if request.path == "/repos/community/rules/commits/main" {
            if request.if_none_match.as_deref() == Some("\"main-etag\"") {
                return Response {
                    status: "304 Not Modified",
                    headers: Vec::new(),
                    body: Vec::new(),
                };
            }
            return Response {
                status: "200 OK",
                headers: vec![("ETag".to_string(), "\"main-etag\"".to_string())],
                body: body.clone().into_bytes(),
            };
        }
        Response::not_found()
    });
    let store = store(dir.path(), &server);
    assert_eq!(store.resolve(&source(), Some("main")).unwrap(), SHA_A);
    assert_eq!(store.resolve(&source(), Some("main")).unwrap(), SHA_A);
    let requests = server.find("/repos/community/rules/commits/main");
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].if_none_match, None);
    assert_eq!(requests[1].if_none_match.as_deref(), Some("\"main-etag\""));
}

#[test]
fn a_remote_check_distinguishes_all_four_states() {
    let dir = TestDir::new("remote-check");
    let server = TestServer::serve(move |request| match request.path.as_str() {
        "/repos/community/rules/commits/main" => {
            Response::json(&format!("{{\"sha\":\"{SHA_A}\"}}"))
        }
        "/repos/community/rules/commits/next" => {
            Response::json(&format!("{{\"sha\":\"{SHA_B}\"}}"))
        }
        "/repos/community/rules/commits/gone" => Response::not_found(),
        _ => Response {
            status: "500 Internal Server Error",
            headers: Vec::new(),
            body: b"boom".to_vec(),
        },
    });
    let store = store(dir.path(), &server);
    match store.check_remote(&source(), Some("main"), SHA_A) {
        RemoteCheck::Unchanged { sha } => assert_eq!(sha, SHA_A),
        other => panic!("expected unchanged, got {}", other.status()),
    }
    match store.check_remote(&source(), Some("next"), SHA_A) {
        RemoteCheck::Updated { sha } => assert_eq!(sha, SHA_B),
        other => panic!("expected updated, got {}", other.status()),
    }
    match store.check_remote(&source(), Some("gone"), SHA_A) {
        RemoteCheck::Unavailable { reason } => {
            assert!(reason.contains("not found"), "{reason}")
        }
        other => panic!("expected unavailable, got {}", other.status()),
    }
    match store.check_remote(&source(), Some("error"), SHA_A) {
        RemoteCheck::Error(error) => assert_eq!(error.code(), "lint-package-remote"),
        other => panic!("expected error, got {}", other.status()),
    }
}

#[test]
fn the_default_ref_uses_the_default_branch_head() {
    let dir = TestDir::new("default-head");
    let archive = package_tarball(RULE);
    let server = TestServer::serve(move |request| {
        if request.path == "/repos/community/rules/commits?per_page=1" {
            Response::json(&format!("[{{\"sha\":\"{SHA_A}\"}}]"))
        } else if request.path == format!("/community/rules/tar.gz/{SHA_A}") {
            Response::ok(archive.clone())
        } else {
            Response::not_found()
        }
    });
    let store = store(dir.path(), &server);
    let installed = store.install(&source(), None).unwrap();
    assert_eq!(installed.sha, SHA_A);
}

#[test]
fn malformed_incompatible_empty_and_duplicate_packages_are_refused() {
    let dir = TestDir::new("invalid");
    let cases: HashMap<&str, Vec<u8>> = [
        (
            "malformed",
            tarball(&[(
                "o-r-bad/rules/bad.yaml",
                b"id: community/bad\nmetadata: []\nmatcher: {}\n",
            )]),
        ),
        (
            "bare-id",
            tarball(&[(
                "o-r-bare/rules/bare.yaml",
                b"id: bare-id\nmetadata:\n  summary: s\n  rationale: r\n  documentation: d\n  known-limits: k\n  tags: []\nmatcher: {}\n",
            )]),
        ),
        (
            "reserved",
            tarball(&[(
                "o-r-wr/rules/wr.yaml",
                b"id: wright/official\nmetadata:\n  summary: s\n  rationale: r\n  documentation: d\n  known-limits: k\n  tags: []\nmatcher: {}\n",
            )]),
        ),
        ("empty", tarball(&[("o-r-e/README.md", b"nothing")])),
        (
            "nested-only",
            tarball(&[(
                "o-r-n/rules/nested/deep.yaml",
                RULE.as_bytes(),
            )]),
        ),
        (
            "duplicate",
            tarball(&[
                ("o-r-d/rules/a.yaml", RULE.as_bytes()),
                ("o-r-d/rules/b.yml", RULE.as_bytes()),
            ]),
        ),
    ]
    .into_iter()
    .collect();
    let server = TestServer::serve(move |request| {
        if let Some(name) = request.path.strip_prefix("/community/rules/tar.gz/") {
            match name {
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" => {
                    return Response::ok(cases["malformed"].clone());
                }
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" => {
                    return Response::ok(cases["bare-id"].clone());
                }
                "cccccccccccccccccccccccccccccccccccccccc" => {
                    return Response::ok(cases["reserved"].clone());
                }
                "dddddddddddddddddddddddddddddddddddddddd" => {
                    return Response::ok(cases["empty"].clone());
                }
                "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee" => {
                    return Response::ok(cases["nested-only"].clone());
                }
                "ffffffffffffffffffffffffffffffffffffffff" => {
                    return Response::ok(cases["duplicate"].clone());
                }
                _ => {}
            }
        }
        Response::not_found()
    });
    let store = store(dir.path(), &server);
    for (sha, expect_file) in [
        (SHA_A, "bad.yaml"),
        (SHA_B, "bare.yaml"),
        ("cccccccccccccccccccccccccccccccccccccccc", "wr.yaml"),
        ("dddddddddddddddddddddddddddddddddddddddd", "rules"),
        ("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee", "rules"),
        ("ffffffffffffffffffffffffffffffffffffffff", "b.yml"),
    ] {
        let error = store.install(&source(), Some(sha)).unwrap_err();
        assert_eq!(error.code(), "lint-package-invalid", "sha {sha}: {error}");
        assert!(
            error.to_string().contains(expect_file),
            "diagnostic names the file ({expect_file}): {error}"
        );
        assert!(
            !store.package_dir(&source(), sha).exists(),
            "no partial store entry for {sha}"
        );
    }
    assert_eq!(store_entries(dir.path()), Vec::<String>::new());
    // A refused install also leaves no staging debris behind.
    let debris: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(".lint-package-")
        })
        .collect();
    assert!(debris.is_empty());
}

#[test]
fn a_missing_repository_ref_or_sha_is_a_not_found_error() {
    let dir = TestDir::new("not-found");
    let server = TestServer::serve(|_| Response::not_found());
    let store = store(dir.path(), &server);
    let error = store.resolve(&source(), Some("v9")).unwrap_err();
    assert_eq!(error.code(), "lint-package-not-found");
    let error = store.install(&source(), Some(SHA_A)).unwrap_err();
    assert_eq!(error.code(), "lint-package-not-found");
    assert_eq!(store_entries(dir.path()), Vec::<String>::new());
}

#[test]
fn a_network_failure_is_an_offline_error() {
    let dir = TestDir::new("offline");
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let dead = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let store = LintPackageStore::new(dir.path())
        .with_api_base(&dead)
        .with_codeload_base(&dead)
        .with_token(None);
    let error = store.resolve(&source(), Some("v1")).unwrap_err();
    assert_eq!(error.code(), "lint-package-offline");
    let error = store.install(&source(), Some(SHA_A)).unwrap_err();
    assert_eq!(error.code(), "lint-package-offline");
    assert_eq!(store_entries(dir.path()), Vec::<String>::new());
}

#[test]
fn a_used_up_rate_limit_is_a_rate_limited_error() {
    let dir = TestDir::new("rate-limit");
    let server = TestServer::serve(|_| Response {
        status: "403 Forbidden",
        headers: vec![
            ("x-ratelimit-remaining".to_string(), "0".to_string()),
            ("x-ratelimit-reset".to_string(), "1893456000".to_string()),
        ],
        body: b"API rate limit exceeded".to_vec(),
    });
    let store = store(dir.path(), &server);
    let error = store.resolve(&source(), Some("v1")).unwrap_err();
    assert_eq!(error.code(), "lint-package-rate-limited");
    assert!(error.to_string().contains("GITHUB_TOKEN"));
    match store.check_remote(&source(), Some("v1"), SHA_A) {
        RemoteCheck::Error(error) => assert_eq!(error.code(), "lint-package-rate-limited"),
        other => panic!("expected error, got {}", other.status()),
    }
}

#[test]
fn the_github_token_reaches_only_the_api_host() {
    let dir = TestDir::new("token");
    let archive = package_tarball(RULE);
    let server = TestServer::serve(move |request| {
        if request.path == "/repos/community/rules/commits/main" {
            Response::json(&format!("{{\"sha\":\"{SHA_A}\"}}"))
        } else if request.path == format!("/community/rules/tar.gz/{SHA_A}") {
            Response::ok(archive.clone())
        } else {
            Response::not_found()
        }
    });
    let store = LintPackageStore::new(dir.path())
        .with_api_base(server.url())
        .with_codeload_base(server.url())
        .with_token(Some("test-token".to_string()));
    store.install(&source(), Some("main")).unwrap();
    let api = server.find("/repos/");
    assert_eq!(
        api[0].authorization.as_deref(),
        Some("Bearer test-token"),
        "the API request carries the token"
    );
    let codeload = server.find("/community/rules/tar.gz/");
    assert_eq!(
        codeload[0].authorization, None,
        "the token is never sent to codeload"
    );
}

#[test]
fn a_damaged_store_entry_is_refused_not_overwritten() {
    let dir = TestDir::new("damaged");
    let broken = dir.path().join("community/rules").join(SHA_A).join("rules");
    std::fs::create_dir_all(&broken).unwrap();
    let server = TestServer::serve(|_| Response::not_found());
    let store = store(dir.path(), &server);
    let error = store.install(&source(), Some(SHA_A)).unwrap_err();
    assert_eq!(error.code(), "lint-package-install");
    assert!(server.requests().is_empty(), "no download was attempted");
}

#[test]
fn source_parsing_accepts_github_spellings_and_rejects_others() {
    let (src, reference) = LintPackageSource::parse("community/rules").unwrap();
    assert_eq!(src.slug(), "community/rules");
    assert_eq!(reference, None);
    let (src, reference) = LintPackageSource::parse("https://github.com/community/rules").unwrap();
    assert_eq!(src.slug(), "community/rules");
    assert_eq!(reference, None);
    let (src, reference) =
        LintPackageSource::parse("https://github.com/community/rules.git").unwrap();
    assert_eq!(src.slug(), "community/rules");
    assert_eq!(reference, None);
    let (src, reference) = LintPackageSource::parse("git@github.com:community/rules.git").unwrap();
    assert_eq!(src.slug(), "community/rules");
    assert_eq!(reference, None);
    let (src, reference) =
        LintPackageSource::parse("https://github.com/community/rules/tree/feat/x").unwrap();
    assert_eq!(src.slug(), "community/rules");
    assert_eq!(reference.as_deref(), Some("feat/x"));
    let (src, reference) =
        LintPackageSource::parse("https://github.com/community/rules/releases/tag/v1.0").unwrap();
    assert_eq!(src.slug(), "community/rules");
    assert_eq!(reference.as_deref(), Some("v1.0"));
    for bad in [
        "community",
        "community/rules/extra",
        "https://gitlab.com/community/rules",
        "https://github.com/",
        "../escape/rules",
    ] {
        assert!(
            LintPackageSource::parse(bad).is_err(),
            "'{bad}' is not a lint package source"
        );
    }
}
