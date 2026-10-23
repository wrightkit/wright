#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub(crate) const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(crate) const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

pub(crate) const RULE: &str = "\
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

pub(crate) fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

pub(crate) fn tarball(files: &[(&str, &[u8])]) -> Vec<u8> {
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

pub(crate) fn package_tarball(rule: &str) -> Vec<u8> {
    tarball(&[
        ("o-r-aaaaaaaa/rules/rule.yaml", rule.as_bytes()),
        ("o-r-aaaaaaaa/README.md", b"# package"),
        ("o-r-aaaaaaaa/src/tool.sh", b"#!/bin/sh\n"),
    ])
}

#[derive(Clone)]
pub(crate) struct Request {
    pub(crate) path: String,
}

pub(crate) struct Response {
    pub(crate) status: &'static str,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

impl Response {
    pub(crate) fn ok(body: Vec<u8>) -> Self {
        Self {
            status: "200 OK",
            headers: Vec::new(),
            body,
        }
    }
    pub(crate) fn json(body: &str) -> Self {
        Self::ok(body.as_bytes().to_vec())
    }
    pub(crate) fn not_found() -> Self {
        Self {
            status: "404 Not Found",
            headers: Vec::new(),
            body: b"not found".to_vec(),
        }
    }
}

pub(crate) struct TestServer {
    pub(crate) addr: SocketAddr,
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) requests: Arc<Mutex<Vec<Request>>>,
    pub(crate) thread: Option<JoinHandle<()>>,
}

impl TestServer {
    pub(crate) fn serve(handler: impl Fn(&Request) -> Response + Send + Sync + 'static) -> Self {
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

    pub(crate) fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub(crate) fn paths(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.path.clone())
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

pub(crate) fn read_request(stream: &mut TcpStream) -> Request {
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
    let path = request_line.nth(1).unwrap_or_default().to_string();
    Request { path }
}

pub(crate) fn write_response(stream: &mut TcpStream, response: &Response) {
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

pub(crate) struct TestDirs {
    pub(crate) root: PathBuf,
    pub(crate) cwd: PathBuf,
}

impl TestDirs {
    pub(crate) fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("wright-pkg-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let cwd = root.join("cwd");
        std::fs::create_dir_all(&cwd).unwrap();
        Self {
            root: root.join("store"),
            cwd,
        }
    }
}

impl Drop for TestDirs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.root.parent().unwrap_or(&self.root));
    }
}

pub(crate) fn run(dirs: &TestDirs, server: Option<&TestServer>, args: &[&str]) -> Output {
    let mut command = Command::new(wright());
    command
        .args(args)
        .current_dir(&dirs.cwd)
        .stdin(Stdio::null())
        .env_remove("CI")
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITHUB_STEP_SUMMARY")
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR")
        .env_remove("GITHUB_TOKEN")
        .env("WRIGHT_LINT_PACKAGE_DIR", &dirs.root);
    if let Some(server) = server {
        command
            .env("WRIGHT_LINT_PACKAGE_API_BASE", server.url())
            .env("WRIGHT_LINT_PACKAGE_CODELOAD_BASE", server.url());
    }
    command.output().expect("wright runs")
}

pub(crate) fn transcript(dirs: &TestDirs, output: &Output) -> String {
    let text = format!(
        "exit {}\n== stdout\n{}\n== stderr\n{}",
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    text.replace(&dirs.root.display().to_string(), "<store>")
}

pub(crate) fn write_entry(root: &Path, owner: &str, repo: &str, sha: &str, rule: &str) -> PathBuf {
    let dir = root.join(owner).join(repo).join(sha);
    std::fs::create_dir_all(dir.join("rules")).unwrap();
    std::fs::write(dir.join("rules").join("rule.yaml"), rule).unwrap();
    dir
}

pub(crate) fn entry(root: &Path, owner: &str, repo: &str, sha: &str) -> PathBuf {
    root.join(owner).join(repo).join(sha)
}

pub(crate) fn search_page(items: &[(&str, Option<&str>)], total: u64) -> String {
    let items: Vec<String> = items
        .iter()
        .map(|(full_name, description)| {
            let description = description
                .map(|d| format!("\"{d}\""))
                .unwrap_or_else(|| "null".to_string());
            format!("{{\"full_name\":\"{full_name}\",\"description\":{description}}}")
        })
        .collect();
    format!(
        "{{\"total_count\":{total},\"incomplete_results\":false,\"items\":[{}]}}",
        items.join(",")
    )
}

pub(crate) fn package_server(
    owner: &'static str,
    repo: &'static str,
    sha: &'static str,
    archive: Vec<u8>,
) -> TestServer {
    let owner = owner.to_string();
    let repo = repo.to_string();
    TestServer::serve(move |request| {
        if request.path == format!("/repos/{owner}/{repo}/commits?per_page=1") {
            Response::json(&format!("[{{\"sha\":\"{sha}\"}}]"))
        } else if request.path == format!("/repos/{owner}/{repo}/commits/v1") {
            Response::json(&format!("{{\"sha\":\"{sha}\"}}"))
        } else if request.path == format!("/{owner}/{repo}/tar.gz/{sha}") {
            Response::ok(archive.clone())
        } else {
            Response::not_found()
        }
    })
}
