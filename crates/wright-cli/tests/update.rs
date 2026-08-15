//! End-to-end `wright update` tests (#116) against a mock release server.
//!
//! Serves fake release archives and checksums over a local HTTP server (the
//! same shape `scripts/test-install.sh` uses for `install.sh`) and exercises
//! the real `wright` binary: version resolution, `--check` without
//! modification, checksum-verified installs, atomic replacement of both
//! binaries, and the refusal paths.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use sha2::Digest;

/// The fake release version served by the mock server.
const RELEASE: &str = "9.9.9";
const TRIPLE: &str = "x86_64-unknown-linux-gnu";

/// A minimal HTTP/1.1 server serving a fixed path -> body map.
struct MockServer {
    addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MockServer {
    fn new(files: HashMap<String, Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("binds an ephemeral port");
        let addr = listener.local_addr().expect("bound address");
        let files = Arc::new(files);
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = shutdown.clone();
        let thread = std::thread::spawn(move || {
            while !thread_shutdown.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => serve(stream, &files),
                    Err(_) => break,
                }
            }
        });
        MockServer {
            addr,
            shutdown,
            thread: Some(thread),
        }
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }

    fn api_url(&self) -> String {
        format!("{}/repos/wrightkit/wright/releases/latest", self.base_url())
    }

    /// Environment overrides that point the child at this server.
    fn env(&self) -> Vec<(&'static str, String)> {
        vec![
            ("WRIGHT_INSTALL_BASE_URL", self.base_url()),
            ("WRIGHT_API_URL", self.api_url()),
            ("WRIGHT_INSTALL_OS", "linux".to_string()),
            ("WRIGHT_INSTALL_ARCH", "x86_64".to_string()),
        ]
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        // A blocked accept() is not woken by closing the listener; connect
        // once so the serving thread observes the flag and exits.
        if let Ok(stream) = TcpStream::connect(self.addr) {
            drop(stream);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(mut stream: TcpStream, files: &HashMap<String, Vec<u8>>) {
    let mut request = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                request.extend_from_slice(&buf[..n]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
        }
    }
    let request_line = String::from_utf8_lossy(&request);
    let path = request_line
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let path = path.split('?').next().unwrap_or(path);
    match files.get(path) {
        Some(body) => {
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(body);
        }
        None => {
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    }
    let _ = stream.flush();
}

/// A release mock: archive + checksum + latest-release metadata.
struct Release {
    files: HashMap<String, Vec<u8>>,
}

fn release(version: &str) -> Release {
    let archive = build_archive(version);
    let name = format!("wright-{version}-{TRIPLE}.tar.gz");
    let mut files = HashMap::new();
    files.insert(
        "/repos/wrightkit/wright/releases/latest".to_string(),
        format!("{{\"tag_name\":\"v{version}\",\"draft\":false,\"prerelease\":false}}\n")
            .into_bytes(),
    );
    files.insert(format!("/v{version}/{name}"), archive.clone());
    files.insert(
        format!("/v{version}/{name}.sha256"),
        format!("{}  {name}\n", sha256_hex(&archive)).into_bytes(),
    );
    Release { files }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = sha2::Sha256::digest(bytes);
    digest.iter().fold(String::new(), |mut output, byte| {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
        output
    })
}

/// Build a gzipped tar archive with the canonical `wright-<version>-<target>/`
/// layout containing fake `wright` and `wright-lsp` shell scripts.
fn build_archive(version: &str) -> Vec<u8> {
    let dir = format!("wright-{version}-{TRIPLE}");
    let mut builder = tar::Builder::new(Vec::new());
    for (name, content) in [
        (
            "wright",
            format!("#!/bin/sh\necho \"fake wright {version}\"\n").into_bytes(),
        ),
        (
            "wright-lsp",
            format!("#!/bin/sh\necho \"fake wright-lsp {version}\"\n").into_bytes(),
        ),
    ] {
        let path = format!("{dir}/{name}");
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, path, &content[..])
            .unwrap();
    }
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, &builder.into_inner().unwrap()).unwrap();
    encoder.finish().unwrap()
}

/// A fresh install directory holding copies of the real test binary.
fn install_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wright-update-it-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let wright = env!("CARGO_BIN_EXE_wright");
    std::fs::copy(wright, dir.join("wright")).unwrap();
    std::fs::copy(wright, dir.join("wright-lsp")).unwrap();
    dir
}

fn run_update(dir: &Path, args: &[&str], env: &[(&'static str, String)]) -> std::process::Output {
    let mut command = Command::new(dir.join("wright"));
    command
        .args(args)
        .env("WRIGHT_INSTALL_OS", "linux")
        .env("WRIGHT_INSTALL_ARCH", "x86_64");
    for (key, value) in env {
        command.env(key, value);
    }
    command
        .stdin(Stdio::null())
        .output()
        .expect("wright update runs")
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

#[test]
fn check_reports_an_available_update_without_modifying_files() {
    let server = MockServer::new(release(RELEASE).files);
    let dir = install_dir("check-available");
    let before = read(&dir.join("wright"));
    let output = run_update(&dir, &["update", "--check"], &server.env());
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("update available") && stdout.contains(RELEASE),
        "{stdout}"
    );
    assert_eq!(
        read(&dir.join("wright")),
        before,
        "--check must not modify the binary"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn check_with_pinned_version_reports_availability_without_network() {
    // A pinned --check never touches the API or downloads: only the version
    // comparison runs.
    let dir = install_dir("check-pinned");
    let before = read(&dir.join("wright"));
    let output = run_update(
        &dir,
        &["update", "--check", "--version", RELEASE],
        &[("WRIGHT_INSTALL_BASE_URL", "http://127.0.0.1:1".to_string())],
    );
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("update available"));
    assert_eq!(read(&dir.join("wright")), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_installs_and_replaces_both_binaries() {
    let server = MockServer::new(release(RELEASE).files);
    let dir = install_dir("update-ok");
    let output = run_update(&dir, &["update"], &server.env());
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(RELEASE), "{stdout}");

    // Both binaries are the fake release payloads now.
    assert_eq!(
        String::from_utf8_lossy(&read(&dir.join("wright"))),
        format!("#!/bin/sh\necho \"fake wright {RELEASE}\"\n")
    );
    assert_eq!(
        String::from_utf8_lossy(&read(&dir.join("wright-lsp"))),
        format!("#!/bin/sh\necho \"fake wright-lsp {RELEASE}\"\n")
    );

    // The smoke check ran the installed binaries: they report the version.
    let wright_version = Command::new(dir.join("wright"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&wright_version.stdout).contains(RELEASE));
    let lsp_version = Command::new(dir.join("wright-lsp"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&lsp_version.stdout).contains(RELEASE));

    // No staging directories are left behind.
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".wright-update-")
        })
        .collect();
    assert!(
        leftovers.is_empty(),
        "staging dirs must be cleaned up: {leftovers:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_already_at_latest_reports_up_to_date() {
    let current = env!("CARGO_PKG_VERSION");
    let server = MockServer::new(release(current).files);
    let dir = install_dir("update-current");
    let before = read(&dir.join("wright"));
    let output = run_update(&dir, &["update"], &server.env());
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("already at version"));
    assert_eq!(read(&dir.join("wright")), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn checksum_mismatch_is_rejected_before_any_change() {
    let mut mock = release(RELEASE);
    let name = format!("wright-{RELEASE}-{TRIPLE}.tar.gz");
    mock.files.insert(
        format!("/v{RELEASE}/{name}.sha256"),
        format!("{}  {name}\n", sha256_hex(b"not the archive")).into_bytes(),
    );
    let server = MockServer::new(mock.files);
    let dir = install_dir("update-badsum");
    let before = read(&dir.join("wright"));
    let output = run_update(&dir, &["update"], &server.env());
    assert_eq!(
        output.status.code(),
        Some(4),
        "checksum failure is an environment error"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("checksum verification failed"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        read(&dir.join("wright")),
        before,
        "nothing is replaced on failure"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_wright_lsp_fails_with_guidance() {
    let server = MockServer::new(release(RELEASE).files);
    let dir = install_dir("update-nolsp");
    std::fs::remove_file(dir.join("wright-lsp")).unwrap();
    let output = run_update(&dir, &["update"], &server.env());
    assert_eq!(output.status.code(), Some(4));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("wright-lsp was not found"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn unwritable_install_directory_fails_with_guidance() {
    use std::os::unix::fs::PermissionsExt;
    let server = MockServer::new(release(RELEASE).files);
    let dir = install_dir("update-readonly");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let output = run_update(&dir, &["update"], &server.env());
    assert_eq!(
        output.status.code(),
        Some(4),
        "an unwritable install directory is an environment failure"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not writable"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn downgrade_is_refused() {
    let dir = install_dir("update-downgrade");
    let before = read(&dir.join("wright"));
    let output = run_update(
        &dir,
        &["update", "--version", "0.0.1"],
        &[("WRIGHT_INSTALL_BASE_URL", "http://127.0.0.1:1".to_string())],
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "a refused downgrade is a user error"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("refusing to downgrade"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(read(&dir.join("wright")), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_update_flag_is_a_usage_error() {
    let dir = install_dir("update-usage");
    let output = run_update(&dir, &["update", "--frobnicate"], &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "usage errors write stderr only");
    assert!(!output.stderr.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn help_documents_the_update_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_wright"))
        .arg("--help")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("update"), "help documents update: {help}");
    assert!(help.contains("--check"), "help documents --check");
    assert!(
        help.contains("--version <VERSION>"),
        "help documents --version"
    );
}
