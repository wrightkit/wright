#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use wright_driver::{LintPackageSource, LintPackageStore};

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

pub(crate) const SECOND_RULE: &str = "\
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
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) authorization: Option<String>,
    pub(crate) if_none_match: Option<String>,
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
                        seen.lock().unwrap().push(request);
                        write_response(&mut stream, &response);
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

    pub(crate) fn requests(&self) -> Vec<(String, String)> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| (r.method.clone(), r.path.clone()))
            .collect()
    }

    pub(crate) fn find(&self, path_prefix: &str) -> Vec<Request> {
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

pub(crate) struct TestDir(PathBuf);

impl TestDir {
    pub(crate) fn new(name: &str) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!("wright-lint-package-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn source() -> LintPackageSource {
    LintPackageSource::new("community", "rules").unwrap()
}

pub(crate) fn store(dir: &Path, server: &TestServer) -> LintPackageStore {
    LintPackageStore::new(dir)
        .with_api_base(server.url())
        .with_codeload_base(server.url())
        .with_token(None)
}

pub(crate) fn store_entries(root: &Path) -> Vec<String> {
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
