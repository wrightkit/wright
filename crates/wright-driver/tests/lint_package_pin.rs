use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::{
    CompilerSession, FindingSelection, InputSpec, LintConfig, LintPackageConfig, SessionConfig,
    SourceKind,
};

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

const FIRST_PARTY_RULE: &str = "\
id: min-wait-loop
metadata:
  summary: loop contains a wait
  rationale: waits yield the scheduler
  documentation: Finds a wait call inside a while scope.
  known-limits: Structural only.
  tags: [performance]
matcher:
  scope: while
  actions:
    - kind: call
      name: Wait
";

const OTHER_RULE: &str = "\
id: other/extra
metadata:
  summary: s
  rationale: r
  documentation: d
  known-limits: k
  tags: []
matcher: {}
";

const BARE_RULE: &str = "\
id: bare-rule
metadata:
  summary: s
  rationale: r
  documentation: d
  known-limits: k
  tags: []
matcher: {}
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
        ("pkg-aaaaaaaa/rules/rule.yaml", rule.as_bytes()),
        ("pkg-aaaaaaaa/README.md", b"# package"),
    ])
}

#[derive(Clone, Debug)]
struct Request {
    path: String,
}

struct Response {
    status: &'static str,
    body: Vec<u8>,
}

impl Response {
    fn ok(body: Vec<u8>) -> Self {
        Self {
            status: "200 OK",
            body,
        }
    }
    fn not_found() -> Self {
        Self {
            status: "404 Not Found",
            body: b"not found".to_vec(),
        }
    }
}

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
    let mut request_line = text.lines().next().unwrap_or_default().split_whitespace();
    let _method = request_line.next();
    Request {
        path: request_line.next().unwrap_or_default().to_string(),
    }
}

fn write_response(stream: &mut TcpStream, response: &Response) {
    let head = format!(
        "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        response.body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&response.body);
}

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!("wright-lint-pin-{name}-{}", std::process::id()));
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

fn seed_store(root: &Path, owner: &str, repo: &str, sha: &str, file: &str, rule: &str) {
    let rules = root.join(owner).join(repo).join(sha).join("rules");
    std::fs::create_dir_all(&rules).unwrap();
    std::fs::write(rules.join(file), rule).unwrap();
}

fn workshop_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests/fixtures/workshop/synthetic/control-flow.ws")
}

fn session_config(lint: LintConfig, store_dir: &Path, server: &TestServer) -> SessionConfig {
    SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        lint,
        lint_package: LintPackageConfig {
            store_dir: Some(store_dir.to_path_buf()),
            api_base: Some(server.url()),
            codeload_base: Some(server.url()),
        },
        ..SessionConfig::default()
    }
}

fn pinned_config(yaml: &str) -> LintConfig {
    LintConfig::from_yaml_str(yaml).expect("lint config YAML parses")
}

fn lint_findings(service: &mut ToolService<'_>) -> Vec<serde_json::Value> {
    match service.handle(&ToolRequest::Lint {
        selection: FindingSelection::default(),
        all: true,
    }) {
        ToolResponse::Ok { result } => result["findings"].as_array().unwrap().clone(),
        ToolResponse::Error { error } => panic!("lint failed: {error:?}"),
    }
}

fn lint_rule_ids(service: &mut ToolService<'_>) -> Vec<String> {
    match service.handle(&ToolRequest::LintRules) {
        ToolResponse::Ok { result } => result["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|rule| rule["id"].as_str().unwrap().to_string())
            .collect(),
        ToolResponse::Error { error } => panic!("lintRules failed: {error:?}"),
    }
}

#[test]
fn pinned_package_rules_lint_the_project_under_project_policy() {
    let dir = TestDir::new("lint");
    let archive = package_tarball(RULE);
    let server = TestServer::serve(move |request| {
        if request.path == format!("/community/rules/tar.gz/{SHA_A}") {
            Response::ok(archive.clone())
        } else {
            Response::not_found()
        }
    });
    let lint = pinned_config(&format!(
        "packages:\n  - repo: community/rules\n    sha: {SHA_A}\nrules:\n  community/minimum-wait:\n    severity: error\n"
    ));
    let mut session = CompilerSession::new(session_config(lint, dir.path(), &server)).unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    let finding = lint_findings(&mut service)
        .into_iter()
        .find(|finding| finding["code"] == "community/minimum-wait")
        .expect("the pinned package's rule produced a finding");
    assert_eq!(finding["severity"], "error", "the project policy applies");
    assert!(lint_rule_ids(&mut service).contains(&"community/minimum-wait".to_string()));
}

#[test]
fn a_missing_package_is_fetched_by_its_sha_on_activation() {
    let dir = TestDir::new("fetch");
    let archive = package_tarball(RULE);
    let server = TestServer::serve(move |request| {
        if request.path == format!("/community/rules/tar.gz/{SHA_A}") {
            Response::ok(archive.clone())
        } else {
            Response::not_found()
        }
    });
    let lint = pinned_config(&format!(
        "packages:\n  - repo: community/rules\n    sha: {SHA_A}\n"
    ));
    let mut session = CompilerSession::new(session_config(lint, dir.path(), &server)).unwrap();
    assert_eq!(
        server.find("/community/rules/tar.gz/").len(),
        1,
        "the SHA's archive was fetched"
    );
    assert!(server.find("/repos/").is_empty());
    assert!(
        dir.path()
            .join("community/rules")
            .join(SHA_A)
            .join("rules/rule.yaml")
            .is_file()
    );
    let mut service = ToolService::new(&mut session).unwrap();
    assert!(
        lint_findings(&mut service)
            .iter()
            .any(|finding| finding["code"] == "community/minimum-wait")
    );
}

#[test]
fn an_intact_store_entry_activates_without_a_download() {
    let dir = TestDir::new("installed");
    seed_store(dir.path(), "community", "rules", SHA_A, "rule.yaml", RULE);
    let server = TestServer::serve(|_| Response::not_found());
    let lint = pinned_config(&format!(
        "packages:\n  - repo: community/rules\n    sha: {SHA_A}\n"
    ));
    let mut session = CompilerSession::new(session_config(lint, dir.path(), &server)).unwrap();
    assert!(
        server.find("/").is_empty(),
        "an intact store entry makes no request: {:?}",
        server.find("/")
    );
    let mut service = ToolService::new(&mut session).unwrap();
    assert!(
        lint_findings(&mut service)
            .iter()
            .any(|finding| finding["code"] == "community/minimum-wait")
    );
}

#[test]
fn an_unfetchable_sha_fails_with_an_explicit_diagnostic_and_no_findings() {
    let dir = TestDir::new("unfetchable");
    let server = TestServer::serve(|_| Response::not_found());
    let lint = pinned_config(&format!(
        "packages:\n  - repo: community/rules\n    sha: {SHA_A}\n"
    ));
    let error = CompilerSession::new(session_config(lint, dir.path(), &server))
        .err()
        .expect("an unfetchable pinned SHA must not construct a session");
    assert_eq!(error.code, "lint-package-not-found");
    assert!(error.message.contains("community/rules"), "{error:?}");
}

#[test]
fn a_duplicate_rule_id_across_two_packages_is_refused() {
    let dir = TestDir::new("duplicate");
    let dup = RULE.replace("community/minimum-wait", "community/dup");
    let one = package_tarball(&dup);
    let two = package_tarball(&dup);
    let server = TestServer::serve(move |request| {
        if request.path == format!("/community/one/tar.gz/{SHA_A}") {
            Response::ok(one.clone())
        } else if request.path == format!("/community/two/tar.gz/{SHA_B}") {
            Response::ok(two.clone())
        } else {
            Response::not_found()
        }
    });
    let lint = pinned_config(&format!(
        "packages:\n  - repo: community/one\n    sha: {SHA_A}\n  - repo: community/two\n    sha: {SHA_B}\n"
    ));
    let error = CompilerSession::new(session_config(lint, dir.path(), &server))
        .err()
        .expect("a cross-package rule ID collision must not construct a session");
    assert_eq!(error.code, "lint-rule-error");
    assert!(error.message.contains("community/dup"), "{error:?}");
}

#[test]
fn a_bare_id_community_package_is_refused() {
    let dir = TestDir::new("bare-id");
    let archive = package_tarball(BARE_RULE);
    let server = TestServer::serve(move |request| {
        if request.path == format!("/community/rules/tar.gz/{SHA_A}") {
            Response::ok(archive.clone())
        } else {
            Response::not_found()
        }
    });
    let lint = pinned_config(&format!(
        "packages:\n  - repo: community/rules\n    sha: {SHA_A}\n"
    ));
    let error = CompilerSession::new(session_config(lint, dir.path(), &server))
        .err()
        .expect("a non-first-party bare ID must be refused");
    assert_eq!(error.code, "lint-package-invalid");
    assert!(error.message.contains("bare-rule"), "{error:?}");
}

#[test]
fn the_first_party_pin_replaces_the_builtin_without_colliding() {
    let dir = TestDir::new("first-party");
    let archive = package_tarball(FIRST_PARTY_RULE);
    let server = TestServer::serve(move |request| {
        if request.path == format!("/wrightkit/lint-rules/tar.gz/{SHA_A}") {
            Response::ok(archive.clone())
        } else {
            Response::not_found()
        }
    });
    let lint = pinned_config(&format!(
        "packages:\n  - repo: wrightkit/lint-rules\n    sha: {SHA_A}\n"
    ));
    let mut session = CompilerSession::new(session_config(lint, dir.path(), &server)).unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    let full = match service.handle(&ToolRequest::LintRules) {
        ToolResponse::Ok { result } => result,
        ToolResponse::Error { error } => panic!("lintRules failed: {error:?}"),
    };
    let entry = full["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["id"] == "min-wait-loop")
        .expect("min-wait-loop is still registered");
    assert_eq!(
        entry["kind"], "declarative",
        "the package rule replaced the built-in entry: {entry}"
    );
    let findings = lint_findings(&mut service);
    let hits: Vec<_> = findings
        .iter()
        .filter(|finding| finding["code"] == "min-wait-loop")
        .collect();
    assert_eq!(hits.len(), 1, "one owner of the ID: {findings:?}");
    assert_eq!(
        hits[0]["evidence"], "exact",
        "the declarative rule reported it"
    );
}

#[test]
fn the_same_config_yields_the_same_effective_rules_across_different_stores() {
    let config_yaml = format!("packages:\n  - repo: community/rules\n    sha: {SHA_A}\n");
    let dir_a = TestDir::new("store-a");
    seed_store(dir_a.path(), "community", "rules", SHA_A, "rule.yaml", RULE);
    seed_store(dir_a.path(), "other", "more", SHA_B, "x.yaml", OTHER_RULE);
    let dir_b = TestDir::new("store-b");
    let archive = package_tarball(RULE);
    let server = TestServer::serve(move |request| {
        if request.path == format!("/community/rules/tar.gz/{SHA_A}") {
            Response::ok(archive.clone())
        } else {
            Response::not_found()
        }
    });
    let run = |store: &Path| {
        let mut session =
            CompilerSession::new(session_config(pinned_config(&config_yaml), store, &server))
                .unwrap();
        let mut service = ToolService::new(&mut session).unwrap();
        (lint_rule_ids(&mut service), lint_findings(&mut service))
    };
    let (ids_a, findings_a) = run(dir_a.path());
    let (ids_b, findings_b) = run(dir_b.path());
    assert_eq!(ids_a, ids_b, "same effective rules: {ids_a:?} vs {ids_b:?}");
    assert_eq!(findings_a, findings_b, "same effective findings");
    assert!(ids_a.contains(&"community/minimum-wait".to_string()));
    assert!(!ids_a.iter().any(|id| id == "other/extra"));
}

#[test]
fn a_pin_must_name_a_full_sha_without_a_url_ref() {
    let dir = TestDir::new("pin-shape");
    let server = TestServer::serve(|_| Response::not_found());
    for yaml in [
        "packages:\n  - repo: community/rules\n    sha: v1.0\n".to_string(),
        format!(
            "packages:\n  - repo: https://github.com/community/rules/tree/main\n    sha: {SHA_A}\n"
        ),
    ] {
        let error = CompilerSession::new(session_config(pinned_config(&yaml), dir.path(), &server))
            .err()
            .expect("a non-SHA or ref-carrying pin must not construct a session");
        assert_eq!(error.code, "lint-package-source", "{error:?}");
    }
    assert!(
        server.find("/").is_empty(),
        "a rejected pin makes no request: {:?}",
        server.find("/")
    );
}
