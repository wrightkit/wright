use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use sha2::Digest;

const RELEASE: &str = "9.9.9";
const TRIPLE: &str = "x86_64-unknown-linux-gnu";
const PROVIDER_OLD: &str = "1.0.0";
const PROVIDER_RELEASE: &str = "1.4.0";

struct MockServer {
    addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<String>>>,
    thread: Option<JoinHandle<()>>,
}

impl MockServer {
    fn new(files: HashMap<String, Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("binds an ephemeral port");
        let addr = listener.local_addr().expect("bound address");
        let files = Arc::new(files);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = shutdown.clone();
        let thread_requests = requests.clone();
        let thread = std::thread::spawn(move || {
            while !thread_shutdown.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => serve(stream, &files, &thread_requests),
                    Err(_) => break,
                }
            }
        });
        MockServer {
            addr,
            shutdown,
            requests,
            thread: Some(thread),
        }
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }

    fn env(&self) -> Vec<(&'static str, String)> {
        vec![
            ("WRIGHT_INSTALL_BASE_URL", self.base_url()),
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

fn serve(mut stream: TcpStream, files: &HashMap<String, Vec<u8>>, requests: &Mutex<Vec<String>>) {
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
    requests.lock().unwrap().push(path.to_string());
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

/// A release mock in the R2 distribution shape: the plain-text latest pointer
/// plus the immutable versioned archive and checksum.
struct Release {
    files: HashMap<String, Vec<u8>>,
}

fn release(version: &str) -> Release {
    let archive = build_archive(version);
    let name = format!("wright-{version}-{TRIPLE}.tar.gz");
    let mut files = HashMap::new();
    files.insert(
        "/latest/version".to_string(),
        format!("{version}\n").into_bytes(),
    );
    files.insert(format!("/releases/{version}/{name}"), archive.clone());
    files.insert(
        format!("/releases/{version}/{name}.sha256"),
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

/// The first-party provider release target for the host running the tests;
/// `None` when no provider artifacts exist for this platform.
fn provider_target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        _ => None,
    }
}

fn provider_binary() -> &'static str {
    if provider_target() == Some("x86_64-pc-windows-msvc") {
        "opy-provider.exe"
    } else {
        "opy-provider"
    }
}

fn provider_store(dir: &Path) -> PathBuf {
    dir.join("provider-data").join("providers").join("opy")
}

/// Record an installed provider in the store without exercising install paths.
fn seed_provider(dir: &Path, version: &str) {
    let target = provider_target().expect("provider tests need a supported host");
    let store = provider_store(dir);
    let executable = store.join(version).join(target).join(provider_binary());
    std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
    std::fs::write(&executable, provider_body(version)).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(store.join("active"), format!("{version}\n")).unwrap();
}

fn provider_body(version: &str) -> Vec<u8> {
    format!("#!/bin/sh\necho \"fake opy-provider {version}\"\n").into_bytes()
}

/// Provider release files for the mock server: the version pointer, the
/// archive, and its checksum under the canonical `opy-rs` routes.
fn provider_release(version: &str, files: &mut HashMap<String, Vec<u8>>) {
    let target = provider_target().expect("provider tests need a supported host");
    let binary = provider_binary();
    let name = format!("opy-provider-{version}-{target}.tar.gz");
    let mut builder = tar::Builder::new(Vec::new());
    let body = provider_body(version);
    let mut header = tar::Header::new_gnu();
    header.set_size(body.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    builder.append_data(&mut header, binary, &body[..]).unwrap();
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, &builder.into_inner().unwrap()).unwrap();
    let archive = encoder.finish().unwrap();
    files.insert(
        "/opy-rs/latest/version".to_string(),
        format!("{version}\n").into_bytes(),
    );
    files.insert(
        format!("/opy-rs/releases/{version}/{name}"),
        archive.clone(),
    );
    files.insert(
        format!("/opy-rs/releases/{version}/{name}.sha256"),
        format!("{}  {name}\n", sha256_hex(&archive)).into_bytes(),
    );
}

fn provider_env(server: &MockServer) -> Vec<(&'static str, String)> {
    let mut env = server.env();
    env.push((
        "WRIGHT_OPY_PROVIDER_BASE_URL",
        format!("{}/opy-rs", server.base_url()),
    ));
    env
}

fn install_dir(tag: &str) -> PathBuf {
    install_dir_under(std::env::temp_dir(), tag)
}

fn install_dir_under(parent: PathBuf, tag: &str) -> PathBuf {
    let dir = parent.join(format!("wright-update-it-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let wright = env!("CARGO_BIN_EXE_wright");
    std::fs::copy(wright, dir.join("wright")).unwrap();
    std::fs::copy(wright, dir.join("wright-lsp")).unwrap();
    dir
}

/// An install directory under a whole `homebrew` path component, the shape
/// Homebrew's prefix actually produces (`/opt/homebrew/bin`, a Cellar
/// path) — provenance detection matches components, not substrings (#641).
fn homebrew_install_dir(tag: &str) -> PathBuf {
    install_dir_under(std::env::temp_dir().join("homebrew"), tag)
}

fn run_update(dir: &Path, args: &[&str], env: &[(&'static str, String)]) -> std::process::Output {
    let mut command = Command::new(dir.join("wright"));
    command
        .args(args)
        .env("WRIGHT_INSTALL_OS", "linux")
        .env("WRIGHT_INSTALL_ARCH", "x86_64")
        .env("WRIGHT_PROVIDER_DATA_DIR", dir.join("provider-data"));
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
        &["update", "--check", "self", "--version", RELEASE],
        &[("WRIGHT_INSTALL_BASE_URL", "http://127.0.0.1:1".to_string())],
    );
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("update available"), "{stdout}");
    assert!(
        stdout.contains(&format!("wright update self --version {RELEASE}")),
        "a pinned check names the pinned install command: {stdout}"
    );
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

    // Self-update touches only the R2 distribution contract — the latest
    // pointer plus the immutable versioned routes — never a release API.
    let name = format!("wright-{RELEASE}-{TRIPLE}.tar.gz");
    assert_eq!(
        server.requests(),
        vec![
            "/latest/version".to_string(),
            format!("/releases/{RELEASE}/{name}"),
            format!("/releases/{RELEASE}/{name}.sha256"),
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pinned_self_update_uses_only_the_versioned_release_routes() {
    let server = MockServer::new(release(RELEASE).files);
    let dir = install_dir("update-pinned");
    let output = run_update(
        &dir,
        &["update", "self", "--version", RELEASE],
        &server.env(),
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let name = format!("wright-{RELEASE}-{TRIPLE}.tar.gz");
    assert_eq!(
        server.requests(),
        vec![
            format!("/releases/{RELEASE}/{name}"),
            format!("/releases/{RELEASE}/{name}.sha256"),
        ],
        "a pinned update must not resolve the latest pointer"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn malformed_latest_version_pointer_is_rejected() {
    // A release-API JSON body is not a valid version pointer: the R2 contract
    // serves a bare version and anything else must fail without changes.
    let mut files = HashMap::new();
    files.insert(
        "/latest/version".to_string(),
        b"{\"tag_name\":\"v9.9.9\"}\n".to_vec(),
    );
    let server = MockServer::new(files);
    let dir = install_dir("update-badlatest");
    let before = read(&dir.join("wright"));
    let output = run_update(&dir, &["update", "--check"], &server.env());
    assert_eq!(output.status.code(), Some(4));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not parse the latest release version"),
        "{stderr}"
    );
    assert_eq!(read(&dir.join("wright")), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_release_archive_fails_before_any_change() {
    // The latest pointer resolves but no archive is published: the download
    // failure is an environment error and nothing is replaced.
    let mut files = HashMap::new();
    files.insert(
        "/latest/version".to_string(),
        format!("{RELEASE}\n").into_bytes(),
    );
    let server = MockServer::new(files);
    let dir = install_dir("update-missing-archive");
    let before = read(&dir.join("wright"));
    let output = run_update(&dir, &["update"], &server.env());
    assert_eq!(output.status.code(), Some(4));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not download") && stderr.contains("404"),
        "{stderr}"
    );
    assert_eq!(read(&dir.join("wright")), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_refreshes_existing_completions() {
    let server = MockServer::new(release(RELEASE).files);
    let dir = install_dir("update-completions");
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

    let output = run_update(&dir, &["update"], &env);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("refreshed") || stdout.contains("installed"),
        "{stdout}"
    );

    let new_content = std::fs::read(&comp_file).unwrap();
    assert_ne!(new_content, b"old completion content");

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&comp_dir);
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
        format!("/releases/{RELEASE}/{name}.sha256"),
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
        &["update", "self", "--version", "0.0.1"],
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
fn update_installs_a_named_provider_into_an_empty_store() {
    if provider_target().is_none() {
        return;
    }
    let dir = install_dir("provider-install");
    let mut files = HashMap::new();
    provider_release(PROVIDER_RELEASE, &mut files);
    let server = MockServer::new(files);
    let output = run_update(&dir, &["update", "provider", "opy"], &provider_env(&server));
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!("installed opy provider {PROVIDER_RELEASE}")),
        "{stdout}"
    );
    let store = provider_store(&dir);
    assert_eq!(
        std::fs::read_to_string(store.join("active"))
            .unwrap()
            .trim(),
        PROVIDER_RELEASE
    );
    let executable = store
        .join(PROVIDER_RELEASE)
        .join(provider_target().unwrap())
        .join(provider_binary());
    assert_eq!(read(&executable), provider_body(PROVIDER_RELEASE));
    assert!(!stdout.contains("installing wright"), "{stdout}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_upgrades_the_installed_provider_alongside_self() {
    if provider_target().is_none() {
        return;
    }
    let dir = install_dir("provider-upgrade");
    seed_provider(&dir, PROVIDER_OLD);
    let mut files = release(RELEASE).files;
    provider_release(PROVIDER_RELEASE, &mut files);
    let server = MockServer::new(files);
    let output = run_update(&dir, &["update"], &provider_env(&server));
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!(
            "updated opy provider {PROVIDER_OLD} -> {PROVIDER_RELEASE}"
        )),
        "{stdout}"
    );
    assert_eq!(
        std::fs::read_to_string(provider_store(&dir).join("active"))
            .unwrap()
            .trim(),
        PROVIDER_RELEASE
    );
    assert!(
        String::from_utf8_lossy(&read(&dir.join("wright")))
            .contains(&format!("fake wright {RELEASE}")),
        "the self target still updated"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_does_not_install_an_absent_provider() {
    let dir = install_dir("provider-absent");
    let server = MockServer::new(release(RELEASE).files);
    let output = run_update(&dir, &["update"], &server.env());
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("opy provider is not installed; skipping"),
        "{stdout}"
    );
    assert!(
        !provider_store(&dir).join("active").exists(),
        "a bare update must not create a provider installation"
    );
    assert!(
        String::from_utf8_lossy(&read(&dir.join("wright")))
            .contains(&format!("fake wright {RELEASE}")),
        "the self target still updated"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn check_reports_provider_availability_without_modifying_anything() {
    if provider_target().is_none() {
        return;
    }
    let dir = install_dir("provider-check");
    seed_provider(&dir, PROVIDER_OLD);
    let mut files = release(RELEASE).files;
    provider_release(PROVIDER_RELEASE, &mut files);
    let server = MockServer::new(files);
    let before = read(&dir.join("wright"));
    let output = run_update(&dir, &["update", "--check"], &provider_env(&server));
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!(
            "update available: opy provider {PROVIDER_OLD} -> {PROVIDER_RELEASE}"
        )),
        "{stdout}"
    );
    assert_eq!(
        std::fs::read_to_string(provider_store(&dir).join("active"))
            .unwrap()
            .trim(),
        PROVIDER_OLD,
        "--check must not touch the active provider"
    );
    assert_eq!(read(&dir.join("wright")), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_provider_without_a_name_skips_an_empty_store() {
    let dir = install_dir("provider-none");
    let output = run_update(&dir, &["update", "provider"], &[]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("opy provider is not installed; skipping"),
        "{stdout}"
    );
    assert!(!provider_store(&dir).join("active").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn package_managed_wright_skips_self_update_but_updates_providers() {
    if provider_target().is_none() {
        return;
    }
    // An install path under a `homebrew` component makes provenance
    // detection classify the executable as Homebrew-managed.
    let dir = homebrew_install_dir("managed");
    seed_provider(&dir, PROVIDER_OLD);
    let mut files = HashMap::new();
    provider_release(PROVIDER_RELEASE, &mut files);
    let server = MockServer::new(files);
    let before = read(&dir.join("wright"));
    let output = run_update(&dir, &["update"], &provider_env(&server));
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("skipped wright self-update") && stdout.contains("brew upgrade"),
        "the skip line carries actionable package-manager guidance: {stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "updated opy provider {PROVIDER_OLD} -> {PROVIDER_RELEASE}"
        )),
        "{stdout}"
    );
    assert_eq!(read(&dir.join("wright")), before, "self-update was skipped");
    assert_eq!(
        std::fs::read_to_string(provider_store(&dir).join("active"))
            .unwrap()
            .trim(),
        PROVIDER_RELEASE
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn explicit_self_update_still_refuses_package_managed_installations() {
    let dir = homebrew_install_dir("self");
    let before = read(&dir.join("wright"));
    let output = run_update(&dir, &["update", "self"], &[]);
    assert_eq!(output.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("brew upgrade"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(read(&dir.join("wright")), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_self_check_reports_availability_without_modifying_files() {
    // `update self --check` resolves the same way as `update --check`
    // restricted to the self target (#641).
    let server = MockServer::new(release(RELEASE).files);
    let dir = install_dir("self-check");
    let before = read(&dir.join("wright"));
    let output = run_update(&dir, &["update", "self", "--check"], &server.env());
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("update available") && stdout.contains(RELEASE),
        "{stdout}"
    );
    assert!(
        !stdout.contains("opy provider"),
        "the self target does not report providers: {stdout}"
    );
    assert_eq!(
        read(&dir.join("wright")),
        before,
        "--check modifies nothing"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_provider_check_reports_availability_without_installing() {
    if provider_target().is_none() {
        return;
    }
    let dir = install_dir("provider-check-flag");
    seed_provider(&dir, PROVIDER_OLD);
    let mut files = HashMap::new();
    provider_release(PROVIDER_RELEASE, &mut files);
    let server = MockServer::new(files);
    let output = run_update(
        &dir,
        &["update", "provider", "--check"],
        &provider_env(&server),
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(&format!(
            "update available: opy provider {PROVIDER_OLD} -> {PROVIDER_RELEASE}"
        )),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        std::fs::read_to_string(provider_store(&dir).join("active"))
            .unwrap()
            .trim(),
        PROVIDER_OLD,
        "--check must not touch the active provider"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_provider_update_makes_update_exit_nonzero() {
    if provider_target().is_none() {
        return;
    }
    // The self target skips (package-managed install) while the installed
    // provider's update fails: `update` must not discard the failure (#641).
    let dir = homebrew_install_dir("provider-fails");
    seed_provider(&dir, PROVIDER_OLD);
    // No provider routes at all: the latest-version fetch 404s.
    let server = MockServer::new(HashMap::new());
    let output = run_update(&dir, &["update"], &provider_env(&server));
    assert_ne!(
        output.status.code(),
        Some(0),
        "a failed provider update must not report success"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("opy"),
        "stderr names the failing target: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_prerelease_pinned_version_installs() {
    // `install.sh` accepts prerelease suffixes; `--version` uses the same
    // grammar, and prerelease precedence keeps 9.9.9-beta.1 above the
    // installed stable (#641).
    let server = MockServer::new(release("9.9.9-beta.1").files);
    let dir = install_dir("self-prerelease");
    let output = run_update(
        &dir,
        &["update", "self", "--version", "9.9.9-beta.1"],
        &server.env(),
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&read(&dir.join("wright"))).contains("fake wright 9.9.9-beta.1"),
        "the prerelease payload was installed"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn standalone_install_in_a_scooped_directory_still_self_updates() {
    // A directory named `scooped` is not Scoop: provenance only matches whole
    // path components, so self-update runs instead of being refused (#641).
    let server = MockServer::new(release(RELEASE).files);
    let dir = install_dir_under(std::env::temp_dir().join("scooped"), "standalone");
    let output = run_update(&dir, &["update"], &server.env());
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("skipped wright self-update"),
        "a `scooped` component must not be classified as Scoop: {stdout}"
    );
    assert!(
        String::from_utf8_lossy(&read(&dir.join("wright")))
            .contains(&format!("fake wright {RELEASE}")),
        "the standalone binary updated"
    );
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
