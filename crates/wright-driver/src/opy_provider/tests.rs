use super::*;
use crate::input::sha256_hex;
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicUsize, Ordering},
};
use std::thread;

fn test_root(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("wright-opy-provider-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn archive(_version: &str, target: &str, body: &[u8]) -> Vec<u8> {
    let binary = provider_binary(target);
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(body.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    builder.append_data(&mut header, binary, body).unwrap();
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&builder.into_inner().unwrap()).unwrap();
    encoder.finish().unwrap()
}

#[test]
fn explicit_provider_wins_without_target_or_network_access() {
    let root = test_root("explicit");
    let executable = root.join(provider_binary("x86_64-unknown-linux-gnu"));
    std::fs::write(&executable, b"#!/bin/sh\n").unwrap();
    make_executable(&executable);
    let resolver = OpyProviderResolver::new(root.join("store")).with_target("unsupported");
    let resolved = resolver.resolve(Some(&executable)).unwrap();
    assert_eq!(resolved.executable, executable);
    assert_eq!(resolved.version, None);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn clean_install_and_failed_integrity_update_preserve_active_provider() {
    let root = test_root("install");
    let target = "x86_64-unknown-linux-gnu";
    let resolver = OpyProviderResolver::new(&root).with_target(target);
    let first = archive("1.2.3", target, b"first");
    let first_sum = format!("{}  archive\n", sha256_hex(&first));
    verify_checksum(&first, &first_sum, "archive").unwrap();
    resolver.install_archive("1.2.3", target, &first).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("active")).unwrap(),
        "1.2.3"
    );
    let bad = archive("1.2.4", target, b"second");
    let error = verify_checksum(&bad, &first_sum, "archive").unwrap_err();
    assert_eq!(error.code(), "provider-integrity");
    assert_eq!(
        std::fs::read_to_string(root.join("active")).unwrap(),
        "1.2.3"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn installed_provider_is_reused_without_network_access() {
    let root = test_root("offline");
    let target = "x86_64-unknown-linux-gnu";
    let resolver = OpyProviderResolver::new(&root).with_target(target);
    let bytes = archive("2.0.0", target, b"cached");
    resolver.install_archive("2.0.0", target, &bytes).unwrap();
    let offline = resolver.with_base_url("http://127.0.0.1:1/opy-rs");
    let resolved = offline.resolve(None).unwrap();
    assert_eq!(resolved.version.as_deref(), Some("2.0.0"));
    assert_eq!(std::fs::read(resolved.executable).unwrap(), b"cached");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn missing_provider_bootstraps_from_r2_pointer_and_artifacts() {
    let root = test_root("bootstrap");
    let target = "x86_64-unknown-linux-gnu";
    let version = "3.1.4";
    let bytes = archive(version, target, b"bootstrapped");
    let checksum = format!(
        "{}  opy-provider-{version}-{target}.tar.gz\n",
        sha256_hex(&bytes)
    );
    let (base_url, requests, server) =
        test_server(version.as_bytes().to_vec(), bytes, checksum.into_bytes());
    let resolver = OpyProviderResolver::new(&root)
        .with_target(target)
        .with_base_url(format!("{base_url}/opy-rs"));
    let resolved = resolver.resolve(None).unwrap();
    server.join().unwrap();
    assert_eq!(requests.load(Ordering::Relaxed), 3);
    assert_eq!(resolved.version.as_deref(), Some(version));
    assert_eq!(std::fs::read(resolved.executable).unwrap(), b"bootstrapped");
    assert_eq!(
        std::fs::read_to_string(root.join("active")).unwrap(),
        version
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn concurrent_bootstrap_promotion_reuses_the_winning_local_install() {
    let root = test_root("concurrent-bootstrap");
    let target = "x86_64-unknown-linux-gnu";
    let version = "3.1.4";
    let archive = archive(version, target, b"bootstrapped");
    let resolver = OpyProviderResolver::new(&root).with_target(target);
    let first_staging = resolver.staging_dir(version).unwrap();
    let second_staging = resolver.staging_dir(version).unwrap();
    extract_provider(&archive, &first_staging, target).unwrap();
    extract_provider(&archive, &second_staging, target).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let first_resolver = resolver.clone();
    let first_barrier = Arc::clone(&barrier);
    let first = thread::spawn(move || {
        first_barrier.wait();
        first_resolver.promote_staging(&first_staging, version, target)
    });
    let second_resolver = resolver.clone();
    let second_barrier = Arc::clone(&barrier);
    let second = thread::spawn(move || {
        second_barrier.wait();
        second_resolver.promote_staging(&second_staging, version, target)
    });

    first.join().unwrap().unwrap();
    second.join().unwrap().unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("active")).unwrap(),
        version
    );
    assert_eq!(
        std::fs::read(resolver.provider_path(version, target)).unwrap(),
        b"bootstrapped"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn installed_reports_the_active_provider_without_network() {
    let root = test_root("installed");
    let target = "x86_64-unknown-linux-gnu";
    let resolver = OpyProviderResolver::new(&root)
        .with_target(target)
        .with_base_url("http://127.0.0.1:1/opy-rs");
    assert_eq!(resolver.installed().unwrap(), None);
    let bytes = archive("2.0.0", target, b"cached");
    resolver.install_archive("2.0.0", target, &bytes).unwrap();
    let installed = resolver.installed().unwrap().unwrap();
    assert_eq!(installed.version.as_deref(), Some("2.0.0"));
    assert_eq!(std::fs::read(installed.executable).unwrap(), b"cached");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn update_to_the_active_version_never_downloads() {
    let root = test_root("active-update");
    let target = "x86_64-unknown-linux-gnu";
    let resolver = OpyProviderResolver::new(&root)
        .with_target(target)
        .with_base_url("http://127.0.0.1:1/opy-rs");
    let bytes = archive("1.2.3", target, b"first");
    resolver.install_archive("1.2.3", target, &bytes).unwrap();
    let resolved = resolver.update(Some("1.2.3")).unwrap();
    assert_eq!(resolved.version.as_deref(), Some("1.2.3"));
    assert_eq!(std::fs::read(resolved.executable).unwrap(), b"first");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_bare_update_keeps_a_newer_installed_provider() {
    let root = test_root("newer-installed");
    let target = "x86_64-unknown-linux-gnu";
    let bytes = archive("2.0.0", target, b"newer");
    let resolver = OpyProviderResolver::new(&root).with_target(target);
    resolver.install_archive("2.0.0", target, &bytes).unwrap();
    let (base_url, requests, server) = test_server_n(1, b"1.0.0".to_vec(), Vec::new(), Vec::new());
    let resolver = resolver.with_base_url(format!("{base_url}/opy-rs"));
    let resolved = resolver.update(None).unwrap();
    server.join().unwrap();
    assert_eq!(requests.load(Ordering::Relaxed), 1);
    assert_eq!(resolved.version.as_deref(), Some("2.0.0"));
    assert_eq!(std::fs::read(resolved.executable).unwrap(), b"newer");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn install_repairs_a_final_dir_without_a_valid_executable() {
    let target = "x86_64-unknown-linux-gnu";
    for name in ["missing-binary", "non-executable"] {
        let root = test_root(name);
        let resolver = OpyProviderResolver::new(&root).with_target(target);
        let final_dir = root.join("1.0.0").join(target);
        std::fs::create_dir_all(&final_dir).unwrap();
        if name == "non-executable" {
            std::fs::write(final_dir.join(provider_binary(target)), b"old").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(
                    final_dir.join(provider_binary(target)),
                    std::fs::Permissions::from_mode(0o644),
                )
                .unwrap();
            }
        }
        let bytes = archive("1.0.0", target, b"fresh");
        resolver.install_archive("1.0.0", target, &bytes).unwrap();
        let resolved = resolver.installed().unwrap().unwrap();
        assert_eq!(std::fs::read(resolved.executable).unwrap(), b"fresh");
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn a_corrupt_active_file_does_not_block_resolve_installed_or_update() {
    let target = "x86_64-unknown-linux-gnu";
    for garbage in [&b"not-a-version"[..], &[0xff, 0xfe, 0x00][..]] {
        let root = test_root("corrupt-active");
        std::fs::write(root.join("active"), garbage).unwrap();
        let resolver = OpyProviderResolver::new(&root).with_target(target);
        assert_eq!(resolver.installed().unwrap(), None);
        let bytes = archive("1.0.0", target, b"fresh");
        let checksum = format!(
            "{}  opy-provider-1.0.0-{target}.tar.gz\n",
            sha256_hex(&bytes)
        );
        let (base_url, _, server) = test_server(b"1.0.0".to_vec(), bytes, checksum.into_bytes());
        let resolver = resolver.with_base_url(format!("{base_url}/opy-rs"));
        let resolved = resolver.update(Some("1.0.0")).unwrap();
        assert_eq!(resolved.version.as_deref(), Some("1.0.0"));
        let _ = server;
        assert_eq!(
            std::fs::read_to_string(root.join("active")).unwrap(),
            "1.0.0"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn latest_version_fetches_only_the_release_pointer() {
    let root = test_root("latest");
    let (base_url, requests, server) =
        test_server_n(1, b"4.5.6\n".to_vec(), Vec::new(), Vec::new());
    let resolver = OpyProviderResolver::new(&root)
        .with_target("x86_64-unknown-linux-gnu")
        .with_base_url(format!("{base_url}/opy-rs"));
    assert_eq!(resolver.latest_version().unwrap(), "4.5.6");
    server.join().unwrap();
    assert_eq!(requests.load(Ordering::Relaxed), 1);
    assert!(!root.join("active").is_file());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unsupported_target_is_structured() {
    let error = OpyProviderResolver::new(test_root("target"))
        .with_target("mips-unknown-linux-gnu")
        .update(Some("1.0.0"))
        .unwrap_err();
    assert_eq!(error.code(), "provider-unsupported-platform");
    assert_eq!(
        target_for("windows", "x86_64").unwrap(),
        "x86_64-pc-windows-msvc"
    );
}

#[test]
fn windows_target_uses_exe_and_tar_gz_archive() {
    let target = target_for("windows", "x86_64").unwrap();
    assert_eq!(target, "x86_64-pc-windows-msvc");
    assert_eq!(provider_binary(&target), "opy-provider.exe");
    assert_eq!(PROVIDER_ARCHIVE_EXTENSION, "tar.gz");

    let root = test_root("windows");
    let version = "1.0.0";
    let bytes = archive(version, &target, b"windows-provider");
    let checksum = format!(
        "{}  opy-provider-{version}-{target}.tar.gz\n",
        sha256_hex(&bytes)
    );
    let (base_url, requests, server) =
        test_server(version.as_bytes().to_vec(), bytes, checksum.into_bytes());
    let resolver = OpyProviderResolver::new(&root)
        .with_target(&target)
        .with_base_url(format!("{base_url}/opy-rs"));
    let updated = resolver.update(None).unwrap();
    server.join().unwrap();
    assert_eq!(requests.load(Ordering::Relaxed), 3);
    assert_eq!(updated.version.as_deref(), Some(version));
    assert_eq!(
        std::fs::read(&updated.executable).unwrap(),
        b"windows-provider"
    );
    let resolved = resolver
        .with_base_url("http://127.0.0.1:1/opy-rs")
        .resolve(None)
        .unwrap();
    assert_eq!(resolved, updated);
    let _ = std::fs::remove_dir_all(root);
}

fn test_server(
    latest_version: Vec<u8>,
    archive: Vec<u8>,
    checksum: Vec<u8>,
) -> (String, Arc<AtomicUsize>, thread::JoinHandle<()>) {
    test_server_n(3, latest_version, archive, checksum)
}

fn test_server_n(
    expected_requests: usize,
    latest_version: Vec<u8>,
    archive: Vec<u8>,
    checksum: Vec<u8>,
) -> (String, Arc<AtomicUsize>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&requests);
    let server = thread::spawn(move || {
        for _ in 0..expected_requests {
            let (mut stream, _) = listener.accept().unwrap();
            respond(&mut stream, &latest_version, &archive, &checksum);
            seen.fetch_add(1, Ordering::Relaxed);
        }
    });
    (format!("http://{address}"), requests, server)
}

fn respond(stream: &mut TcpStream, latest_version: &[u8], archive: &[u8], checksum: &[u8]) {
    let mut request = Vec::new();
    let mut buffer = [0; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..count]);
    }
    let request = String::from_utf8_lossy(&request);
    let path = request.split_whitespace().nth(1).unwrap_or_default();
    let (status, body): (&str, &[u8]) = if path == "/opy-rs/latest/version" {
        ("200 OK", latest_version)
    } else if path.starts_with("/opy-rs/releases/") && path.ends_with(".sha256") {
        ("200 OK", checksum)
    } else if path.starts_with("/opy-rs/releases/") && path.ends_with(".tar.gz") {
        ("200 OK", archive)
    } else {
        ("404 Not Found", b"not found")
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .unwrap();
    stream.write_all(body).unwrap();
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}
