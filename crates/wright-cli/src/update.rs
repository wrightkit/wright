//! Resolves the latest stable Wright release from the canonical GitHub
//! Release contract (the same archives and checksums `install.sh` and the
//! package-manager manifests consume), verifies the published SHA-256
//! checksum, extracts the platform archive, and atomically replaces the
//! running `wright` and `wright-lsp` binaries.

use std::cmp::Ordering;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::time::Duration;

use sha2::Digest;

const DEFAULT_BASE_URL: &str = "https://github.com/wrightkit/wright/releases/download";
const DEFAULT_API_URL: &str = "https://api.github.com/repos/wrightkit/wright/releases/latest";
const USER_AGENT: &str = concat!("wright-update/", env!("CARGO_PKG_VERSION"));

mod exit {
    pub(super) const SUCCESS: u8 = 0;
    pub(super) const USER_ERROR: u8 = 1;
    pub(super) const USAGE: u8 = 2;
    pub(super) const UNSUPPORTED: u8 = 3;
    pub(super) const INTERNAL: u8 = 4;
}

/// A failure of `wright update`, carrying the actionable message the CLI prints to stderr.
#[derive(Debug)]
pub(crate) struct UpdateError {
    code: u8,
    message: String,
}

impl UpdateError {
    fn new(code: u8, m: impl Into<String>) -> Self {
        Self {
            code,
            message: m.into(),
        }
    }
    fn usage(m: impl Into<String>) -> Self {
        Self::new(exit::USAGE, m)
    }
    fn rejected(m: impl Into<String>) -> Self {
        Self::new(exit::USER_ERROR, m)
    }
    fn unsupported(m: impl Into<String>) -> Self {
        Self::new(exit::UNSUPPORTED, m)
    }
    fn failed(m: impl Into<String>) -> Self {
        Self::new(exit::INTERNAL, m)
    }

    pub(crate) fn exit_code(&self) -> u8 {
        self.code
    }
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Platform {
    target: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Provenance {
    Standalone,
    Homebrew,
    Scoop,
    WinGet,
}

impl Provenance {
    fn info(self) -> (&'static str, &'static str) {
        match self {
            Provenance::Standalone => unreachable!("standalone installations are self-updated"),
            Provenance::Homebrew => ("Homebrew", "brew upgrade wrightkit/tap/wright"),
            Provenance::Scoop => ("Scoop", "scoop update wright"),
            Provenance::WinGet => ("WinGet", "winget upgrade WrightKit.Wright"),
        }
    }
}

/// Run the `update` workflow and return the process exit code.
pub(crate) fn run(check_only: bool, requested: Option<&str>) -> Result<u8, UpdateError> {
    let platform = detect_platform()?;
    let exe = std::env::current_exe()
        .map_err(|e| UpdateError::failed(format!("could not locate the wright binary: {e}")))?;
    let provenance = detect_provenance(&exe);
    if provenance != Provenance::Standalone {
        let (channel, guidance) = provenance.info();
        return Err(UpdateError::unsupported(format!(
            "wright appears to be managed by {channel} (installed at {}); upgrade with `{guidance}` instead of `wright update`",
            exe.display()
        )));
    }

    let install_dir = exe.parent().ok_or_else(|| {
        UpdateError::failed(format!(
            "could not determine the installation directory of {}",
            exe.display()
        ))
    })?;
    if !install_dir.join("wright-lsp").is_file() {
        return Err(UpdateError::failed(format!(
            "wright-lsp was not found next to wright at {}; this installation was not created by the official installer — reinstall with `curl -fsSL https://wrightkit.dev/install.sh | bash` to restore a matched pair",
            install_dir.display()
        )));
    }

    let current = env!("CARGO_PKG_VERSION").to_string();
    let (target_version, client) = match requested {
        Some(version) => {
            parse_version(version)?;
            (version.trim_start_matches('v').to_string(), None)
        }
        None => {
            let client = update_client()?;
            let version = resolve_latest(&client, &env_api_url())?;
            (version, Some(client))
        }
    };

    match compare_versions(&current, &target_version) {
        Ordering::Equal => {
            println!("wright {current} is already at version {target_version}");
            return Ok(exit::SUCCESS);
        }
        Ordering::Greater => {
            if check_only {
                println!(
                    "installed wright {current} is newer than requested {target_version}; no update is needed"
                );
                return Ok(exit::SUCCESS);
            }
            return Err(UpdateError::rejected(format!(
                "requested version {target_version} is older than the installed version {current}; refusing to downgrade"
            )));
        }
        Ordering::Less => {}
    }

    if check_only {
        println!(
            "update available: {current} -> {target_version} (run `wright update` to install)"
        );
        return Ok(exit::SUCCESS);
    }

    println!("==> installing wright {current} -> {target_version}");
    let client = match client {
        Some(client) => client,
        None => update_client()?,
    };
    install_version(
        &client,
        &target_version,
        platform,
        &env_base_url(),
        install_dir,
    )?;
    Ok(exit::SUCCESS)
}

fn detect_platform() -> Result<Platform, UpdateError> {
    detect_platform_for(&env_os(), &env_arch())
}

fn detect_platform_for(os: &str, arch: &str) -> Result<Platform, UpdateError> {
    if os == "windows" {
        return Err(UpdateError::unsupported(
            "standalone self-update is not supported on Windows; upgrade with `winget upgrade WrightKit.Wright` or `scoop update wright`",
        ));
    }
    let target = match (os, arch) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("darwin", "x86_64") => "x86_64-apple-darwin",
        ("darwin", "aarch64") => "aarch64-apple-darwin",
        _ => {
            return Err(UpdateError::unsupported(format!(
                "unsupported platform {os}/{arch} (supported: linux/x86_64, darwin/x86_64, darwin/aarch64); use the official installer (curl -fsSL https://wrightkit.dev/install.sh | bash) or a package manager"
            )));
        }
    };
    Ok(Platform { target })
}

fn env_os() -> String {
    std::env::var("WRIGHT_INSTALL_OS").unwrap_or_else(|_| match std::env::consts::OS {
        "macos" => "darwin".to_string(),
        other => other.to_string(),
    })
}

fn env_arch() -> String {
    std::env::var("WRIGHT_INSTALL_ARCH").unwrap_or_else(|_| std::env::consts::ARCH.to_string())
}

fn detect_provenance(exe: &Path) -> Provenance {
    let path = std::fs::canonicalize(exe)
        .unwrap_or_else(|_| exe.to_path_buf())
        .to_string_lossy()
        .to_lowercase();
    if path.contains("homebrew") || path.contains("cellar") {
        Provenance::Homebrew
    } else if path.contains("scoop") {
        Provenance::Scoop
    } else if path.contains("winget") {
        Provenance::WinGet
    } else {
        Provenance::Standalone
    }
}

fn resolve_latest(
    client: &reqwest::blocking::Client,
    api_url: &str,
) -> Result<String, UpdateError> {
    let body = fetch_text(client, api_url)?;
    let value: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
        UpdateError::failed(format!(
            "could not parse the latest-release response from {api_url}: {e}"
        ))
    })?;
    let tag = value
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            UpdateError::failed(format!(
                "could not find the latest release tag in the response from {api_url}; pin a version with `wright update --version`"
            ))
        })?;
    let version = tag.trim_start_matches('v');
    parse_version(version)?;
    Ok(version.to_string())
}

fn install_version(
    client: &reqwest::blocking::Client,
    version: &str,
    platform: Platform,
    base_url: &str,
    install_dir: &Path,
) -> Result<(), UpdateError> {
    let archive_name = format!("wright-{version}-{}.tar.gz", platform.target);
    let archive_url = format!("{base_url}/v{version}/{archive_name}");
    let checksum_url = format!("{archive_url}.sha256");

    println!("==> downloading {archive_url}");
    let archive = fetch(client, &archive_url)?;
    let checksum = fetch_text(client, &checksum_url)?;
    println!("==> verifying SHA-256 checksum");
    verify_checksum(&archive, &checksum, &archive_name)?;

    ensure_writable(install_dir)?;
    let staging = staging_dir(install_dir)?;
    let result = (|| -> Result<(), UpdateError> {
        unpack(&archive, &staging, version, platform.target)?;
        let payload = validated_payload(&staging, version, platform.target)?;
        replace_binary(&payload.join("wright-lsp"), &install_dir.join("wright-lsp"))?;
        replace_binary(&payload.join("wright"), &install_dir.join("wright"))?;
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&staging);

    result?;
    smoke_check(install_dir, version)?;
    println!(
        "==> done: wright and wright-lsp {version} installed in {}",
        install_dir.display()
    );
    let _ = crate::completion::refresh_installed_completions();
    Ok(())
}

fn staging_dir(install_dir: &Path) -> Result<PathBuf, UpdateError> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let staging = install_dir.join(format!(".wright-update-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&staging).map_err(|e| {
        UpdateError::failed(format!(
            "could not create a staging directory in {}: {e}",
            install_dir.display()
        ))
    })?;
    Ok(staging)
}

fn unpack(archive: &[u8], dest: &Path, version: &str, target: &str) -> Result<(), UpdateError> {
    tar::Archive::new(flate2::read::GzDecoder::new(archive))
        .unpack(dest)
        .map_err(|e| {
            UpdateError::failed(format!(
                "failed to extract wright-{version}-{target}.tar.gz: {e}"
            ))
        })
}

fn validated_payload(staging: &Path, version: &str, target: &str) -> Result<PathBuf, UpdateError> {
    let payload = staging.join(format!("wright-{version}-{target}"));
    if !payload.join("wright").is_file() || !payload.join("wright-lsp").is_file() {
        return Err(UpdateError::failed(format!(
            "unexpected archive layout: expected 'wright' and 'wright-lsp' inside {}",
            payload.display()
        )));
    }
    Ok(payload)
}

fn verify_checksum(
    archive: &[u8],
    checksum_file: &str,
    archive_name: &str,
) -> Result<(), UpdateError> {
    let published = checksum_file
        .split_whitespace()
        .next()
        .ok_or_else(|| UpdateError::failed(format!("empty checksum file for {archive_name}")))?;
    if published.len() != 64 || !published.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(UpdateError::failed(format!(
            "invalid checksum file for {archive_name}: expected one SHA-256 hex digest (got '{}')",
            truncate(published)
        )));
    }
    let actual = sha256_hex(archive);
    if !actual.eq_ignore_ascii_case(published) {
        return Err(UpdateError::failed(format!(
            "checksum verification failed for {archive_name} (published {published}, got {actual}); the download may be corrupted or tampered with — nothing was changed"
        )));
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}

fn ensure_writable(install_dir: &Path) -> Result<(), UpdateError> {
    let probe = install_dir.join(format!(".wright-update-probe-{}", std::process::id()));
    std::fs::write(&probe, b"").map_err(|e| {
        UpdateError::failed(format!(
            "installation directory {} is not writable ({e}); fix permissions or reinstall with the official installer (curl -fsSL https://wrightkit.dev/install.sh | bash)",
            install_dir.display()
        ))
    })?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

fn replace_binary(source: &Path, destination: &Path) -> Result<(), UpdateError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(source)
            .map_err(|e| UpdateError::failed(format!("could not read {}: {e}", source.display())))?
            .permissions()
            .mode();
        std::fs::set_permissions(source, std::fs::Permissions::from_mode(mode | 0o111)).map_err(
            |e| {
                UpdateError::failed(format!(
                    "could not make {} executable: {e}",
                    source.display()
                ))
            },
        )?;
    }
    std::fs::rename(source, destination).map_err(|e| {
        UpdateError::failed(format!(
            "could not replace {}: {e}; reinstall with the official installer (curl -fsSL https://wrightkit.dev/install.sh | bash)",
            destination.display()
        ))
    })
}

fn smoke_check(install_dir: &Path, version: &str) -> Result<(), UpdateError> {
    for name in ["wright", "wright-lsp"] {
        let exe = install_dir.join(name);
        let output = Process::new(&exe).arg("--version").output().map_err(|e| {
            UpdateError::failed(format!(
                "smoke check failed: could not run {}: {e}",
                exe.display()
            ))
        })?;
        let reported = String::from_utf8_lossy(&output.stdout);
        if !reported.contains(version) {
            return Err(UpdateError::failed(format!(
                "smoke check failed: `{} --version` did not report version {version} (got {:?}); reinstall with the official installer (curl -fsSL https://wrightkit.dev/install.sh | bash)",
                exe.display(),
                reported.trim()
            )));
        }
    }
    Ok(())
}

fn update_client() -> Result<reqwest::blocking::Client, UpdateError> {
    reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| {
            UpdateError::failed(format!(
                "could not initialize HTTPS client for release downloads: {e}"
            ))
        })
}

fn fetch_text(client: &reqwest::blocking::Client, url: &str) -> Result<String, UpdateError> {
    String::from_utf8(fetch(client, url)?)
        .map_err(|e| UpdateError::failed(format!("could not decode the response from {url}: {e}")))
}

fn fetch(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>, UpdateError> {
    let response = client.get(url).send().map_err(|e| {
        UpdateError::failed(format!("could not download {url} ({e}); check the network connection or pin a version with `wright update --version`"))
    })?;
    if !response.status().is_success() {
        return Err(UpdateError::failed(format!(
            "could not download {url} (HTTP {})",
            response.status()
        )));
    }
    let mut bytes = Vec::new();
    response
        .take(128 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| UpdateError::failed(format!("could not download {url}: {e}")))?;
    Ok(bytes)
}

fn parse_version(version: &str) -> Result<(u64, u64, u64), UpdateError> {
    let err = || {
        UpdateError::usage(format!(
            "invalid version '{version}' (expected semver like 0.1.0)"
        ))
    };
    let mut parts = version.strip_prefix('v').unwrap_or(version).split('.');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(a), Some(b), Some(c), None) => {
            let parse = |p: &str| p.parse::<u64>().map_err(|_| err());
            Ok((parse(a)?, parse(b)?, parse(c)?))
        }
        _ => Err(err()),
    }
}

fn compare_versions(a: &str, b: &str) -> Ordering {
    parse_version(a)
        .expect("valid semver")
        .cmp(&parse_version(b).expect("valid semver"))
}

fn truncate(value: &str) -> String {
    let head: String = value.chars().take(16).collect();
    if value.chars().nth(16).is_some() {
        format!("{head}…")
    } else {
        head
    }
}

fn env_base_url() -> String {
    std::env::var("WRIGHT_INSTALL_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string())
}

fn env_api_url() -> String {
    std::env::var("WRIGHT_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_parsing_and_ordering() {
        for (v, exp) in [
            ("0.1.0", (0, 1, 0)),
            ("v0.2.10", (0, 2, 10)),
            ("9.9.9", (9, 9, 9)),
        ] {
            assert_eq!(parse_version(v).unwrap(), exp);
        }
        for bad in [
            "1.2",
            "1.2.3.4",
            "v",
            "1..3",
            "a.b.c",
            "1.2.x",
            "0.1.0-beta.1",
            "",
        ] {
            assert!(parse_version(bad).is_err(), "{bad:?} must be rejected");
        }
        use std::cmp::Ordering::*;
        for (a, b, exp) in [
            ("0.1.0", "0.1.0", Equal),
            ("0.2.0", "0.1.0", Greater),
            ("0.1.10", "0.1.9", Greater),
            ("0.2.0", "0.10.0", Less),
            ("1.0.0", "0.9.9", Greater),
        ] {
            assert_eq!(compare_versions(a, b), exp);
        }
    }

    #[test]
    fn platform_detection_and_refusals() {
        for (os, arch, target) in [
            ("linux", "x86_64", "x86_64-unknown-linux-gnu"),
            ("darwin", "x86_64", "x86_64-apple-darwin"),
            ("darwin", "aarch64", "aarch64-apple-darwin"),
        ] {
            assert_eq!(detect_platform_for(os, arch).unwrap().target, target);
        }
        let err = detect_platform_for("windows", "x86_64").unwrap_err();
        assert!(err.exit_code() == exit::UNSUPPORTED && err.message().contains("winget"));
        assert_eq!(
            detect_platform_for("linux", "arm64")
                .unwrap_err()
                .exit_code(),
            exit::UNSUPPORTED
        );
    }

    #[test]
    fn provenance_detects_package_managers_and_standalone() {
        for (path, exp) in [
            ("/opt/homebrew/bin/wright", Provenance::Homebrew),
            (
                "/usr/local/Cellar/wright/0.1.0/bin/wright",
                Provenance::Homebrew,
            ),
            (
                "/c/Users/me/scoop/apps/wright/0.1.0/wright.exe",
                Provenance::Scoop,
            ),
            (
                "C:\\Users\\me\\AppData\\Local\\Microsoft\\WinGet\\Packages\\WrightKit.Wright\\wright.exe",
                Provenance::WinGet,
            ),
            ("/Users/me/.local/bin/wright", Provenance::Standalone),
            ("/usr/local/bin/wright", Provenance::Standalone),
        ] {
            assert_eq!(detect_provenance(Path::new(path)), exp);
        }
    }

    #[test]
    fn checksum_verification_and_archive_validation() {
        let bytes = b"the archive bytes";
        let name = "wright-9.9.9-x86_64-unknown-linux-gnu.tar.gz";
        verify_checksum(bytes, &format!("{}  {name}\n", sha256_hex(bytes)), name).unwrap();
        assert!(
            verify_checksum(
                bytes,
                &format!("{}  archive\n", sha256_hex(b"other")),
                "archive"
            )
            .unwrap_err()
            .message()
            .contains("checksum verification failed")
        );
        assert!(
            verify_checksum(bytes, "not-a-hash  archive\n", "archive")
                .unwrap_err()
                .message()
                .contains("invalid checksum")
        );
        assert!(verify_checksum(bytes, "deadbeef\n", "archive").is_err());

        let test_dir = |name, files: &[(&str, &[u8])]| {
            let dir =
                std::env::temp_dir().join(format!("wright-upd-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            unpack(
                &build_archive("9.9.9", files),
                &dir,
                "9.9.9",
                "x86_64-unknown-linux-gnu",
            )
            .unwrap();
            let res = validated_payload(&dir, "9.9.9", "x86_64-unknown-linux-gnu");
            (dir, res)
        };
        let (d1, bad) = test_dir("bad", &[("wright", b"#!/bin/sh\n")]);
        assert!(
            bad.unwrap_err()
                .message()
                .contains("unexpected archive layout")
        );
        let _ = std::fs::remove_dir_all(&d1);

        let (d2, good) = test_dir(
            "good",
            &[("wright", b"#!/bin/sh\n"), ("wright-lsp", b"#!/bin/sh\n")],
        );
        let payload = good.unwrap();
        assert!(payload.join("wright").is_file() && payload.join("wright-lsp").is_file());
        let _ = std::fs::remove_dir_all(&d2);
    }

    fn build_archive(version: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut b = tar::Builder::new(Vec::new());
        for (name, content) in files {
            let mut h = tar::Header::new_gnu();
            h.set_size(content.len() as u64);
            h.set_mode(0o755);
            h.set_cksum();
            b.append_data(
                &mut h,
                format!("wright-{version}-x86_64-unknown-linux-gnu/{name}"),
                *content,
            )
            .unwrap();
        }
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut enc, &b.into_inner().unwrap()).unwrap();
        enc.finish().unwrap()
    }
}
