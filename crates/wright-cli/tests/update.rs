//! End-to-end `wright update` tests (#116) against a mock release server.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sha2::Digest;

const RELEASE: &str = "9.9.9";
const TRIPLE: &str = "x86_64-unknown-linux-gnu";

struct MockServer {
    addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl MockServer {
    fn new(files: HashMap<String, Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("binds port");
        let addr = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let (files, done) = (Arc::new(files), shutdown.clone());
        let thread = std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                if let Ok((mut s, _)) = listener.accept() {
                    let mut buf = [0u8; 1024];
                    let n = s.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/")
                        .split('?')
                        .next()
                        .unwrap_or("/");
                    let (status, body) = files
                        .get(path)
                        .map_or(("404 Not Found", &[][..]), |b| ("200 OK", b.as_slice()));
                    let _ = write!(
                        s,
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = s.write_all(body);
                }
            }
        });
        Self {
            addr,
            shutdown,
            thread: Some(thread),
        }
    }

    fn for_release(version: &str) -> Self {
        Self::new(release(version))
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }

    fn env(&self) -> Vec<(&'static str, String)> {
        vec![
            ("WRIGHT_INSTALL_BASE_URL", self.base_url()),
            (
                "WRIGHT_API_URL",
                format!("{}/repos/wrightkit/wright/releases/latest", self.base_url()),
            ),
            ("WRIGHT_INSTALL_OS", "linux".into()),
            ("WRIGHT_INSTALL_ARCH", "x86_64".into()),
        ]
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn release(version: &str) -> HashMap<String, Vec<u8>> {
    let (archive, name) = (
        build_archive(version),
        format!("wright-{version}-{TRIPLE}.tar.gz"),
    );
    let sha = format!("{}  {name}\n", sha256_hex(&archive)).into_bytes();
    let json = format!("{{\"tag_name\":\"v{version}\",\"draft\":false,\"prerelease\":false}}\n")
        .into_bytes();
    HashMap::from([
        ("/repos/wrightkit/wright/releases/latest".into(), json),
        (format!("/v{version}/{name}"), archive),
        (format!("/v{version}/{name}.sha256"), sha),
    ])
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}

fn build_archive(version: &str) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    for name in ["wright", "wright-lsp"] {
        let content = format!("#!/bin/sh\necho \"fake {name} {version}\"\n");
        let mut h = tar::Header::new_gnu();
        h.set_size(content.len() as u64);
        h.set_mode(0o755);
        h.set_cksum();
        b.append_data(
            &mut h,
            format!("wright-{version}-{TRIPLE}/{name}"),
            content.as_bytes(),
        )
        .unwrap();
    }
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut enc, &b.into_inner().unwrap()).unwrap();
    enc.finish().unwrap()
}

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("wright-update-it-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let wright = env!("CARGO_BIN_EXE_wright");
        std::fs::copy(wright, dir.join("wright")).unwrap();
        std::fs::copy(wright, dir.join("wright-lsp")).unwrap();
        Self { dir }
    }

    fn read(&self, name: &str) -> Vec<u8> {
        std::fs::read(self.dir.join(name)).unwrap()
    }

    fn text(&self, name: &str) -> String {
        String::from_utf8_lossy(&self.read(name)).into_owned()
    }

    fn bin_version(&self, name: &str) -> String {
        let out = Command::new(self.dir.join(name))
            .arg("--version")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn run_update(&self, args: &[&str], env: &[(&'static str, String)]) -> std::process::Output {
        let mut cmd = Command::new(self.dir.join("wright"));
        cmd.args(args)
            .env("WRIGHT_INSTALL_OS", "linux")
            .env("WRIGHT_INSTALL_ARCH", "x86_64");
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.stdin(Stdio::null())
            .output()
            .expect("wright update runs")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

trait OutputExt {
    fn stdout_text(&self) -> String;
    fn stderr_text(&self) -> String;
    fn assert_code(&self, expected: i32);
}

impl OutputExt for std::process::Output {
    fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
    fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
    fn assert_code(&self, expected: i32) {
        assert_eq!(
            self.status.code(),
            Some(expected),
            "stderr: {}",
            self.stderr_text()
        );
    }
}

#[test]
fn check_and_up_to_date_reporting() {
    let server = MockServer::for_release(RELEASE);
    let f = Fixture::new("check-available");
    let before = f.read("wright");
    let output = f.run_update(&["update", "--check"], &server.env());
    output.assert_code(0);
    let stdout = output.stdout_text();
    assert!(stdout.contains("update available") && stdout.contains(RELEASE));
    assert_eq!(f.read("wright"), before);

    let output = f.run_update(
        &["update", "--check", "--version", RELEASE],
        &[("WRIGHT_INSTALL_BASE_URL", "http://127.0.0.1:1".to_string())],
    );
    output.assert_code(0);
    assert!(output.stdout_text().contains("update available"));
    assert_eq!(f.read("wright"), before);

    let current = env!("CARGO_PKG_VERSION");
    let current_server = MockServer::for_release(current);
    let output = f.run_update(&["update"], &current_server.env());
    output.assert_code(0);
    assert!(output.stdout_text().contains("already at version"));
    assert_eq!(f.read("wright"), before);
}

#[test]
fn update_installs_replaces_binaries_and_refreshes_completions() {
    let server = MockServer::for_release(RELEASE);
    let f = Fixture::new("update-ok");
    let comp_dir = std::env::temp_dir().join(format!("wright-comp-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&comp_dir);
    std::fs::create_dir_all(&comp_dir).unwrap();
    let comp_file = comp_dir.join("_wright");
    std::fs::write(&comp_file, b"old completion content").unwrap();

    let mut env = server.env();
    env.push((
        "WRIGHT_COMPLETION_DIR",
        comp_dir.to_str().unwrap().to_string(),
    ));

    let output = f.run_update(&["update"], &env);
    output.assert_code(0);
    let stdout = output.stdout_text();
    assert!(stdout.contains(RELEASE));
    assert!(stdout.contains("refreshed") || stdout.contains("installed"));
    assert_ne!(
        std::fs::read(&comp_file).unwrap(),
        b"old completion content"
    );
    let _ = std::fs::remove_dir_all(&comp_dir);

    assert_eq!(
        f.text("wright"),
        format!("#!/bin/sh\necho \"fake wright {RELEASE}\"\n")
    );
    assert_eq!(
        f.text("wright-lsp"),
        format!("#!/bin/sh\necho \"fake wright-lsp {RELEASE}\"\n")
    );
    assert!(f.bin_version("wright").contains(RELEASE));
    assert!(f.bin_version("wright-lsp").contains(RELEASE));

    let leftovers: Vec<_> = std::fs::read_dir(&f.dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(".wright-update-")
        })
        .collect();
    assert!(
        leftovers.is_empty(),
        "staging dirs must be cleaned up: {leftovers:?}"
    );
}

#[test]
fn update_failure_and_refusal_modes() {
    let mut files = release(RELEASE);
    let name = format!("wright-{RELEASE}-{TRIPLE}.tar.gz");
    files.insert(
        format!("/v{RELEASE}/{name}.sha256"),
        format!("{}  {name}\n", sha256_hex(b"not the archive")).into_bytes(),
    );
    let server = MockServer::new(files);
    let f = Fixture::new("update-failures");
    let before = f.read("wright");

    let output = f.run_update(&["update"], &server.env());
    output.assert_code(4);
    assert!(
        output
            .stderr_text()
            .contains("checksum verification failed")
    );
    assert_eq!(f.read("wright"), before);

    let output = f.run_update(
        &["update", "--version", "0.0.1"],
        &[("WRIGHT_INSTALL_BASE_URL", "http://127.0.0.1:1".to_string())],
    );
    output.assert_code(1);
    assert!(output.stderr_text().contains("refusing to downgrade"));
    assert_eq!(f.read("wright"), before);

    let output = f.run_update(&["update", "--frobnicate"], &[]);
    output.assert_code(2);
    assert!(output.stdout.is_empty() && !output.stderr.is_empty());

    let server_ok = MockServer::for_release(RELEASE);
    std::fs::remove_file(f.dir.join("wright-lsp")).unwrap();
    let output = f.run_update(&["update"], &server_ok.env());
    output.assert_code(4);
    assert!(output.stderr_text().contains("wright-lsp was not found"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let f_ro = Fixture::new("update-readonly");
        std::fs::set_permissions(&f_ro.dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let output = f_ro.run_update(&["update"], &server_ok.env());
        output.assert_code(4);
        assert!(output.stderr_text().contains("not writable"));
        std::fs::set_permissions(&f_ro.dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
