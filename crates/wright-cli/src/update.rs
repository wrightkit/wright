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

#[derive(Debug)]
pub(crate) enum UpdateError {
    Usage(String),
    Rejected(String),
    Unsupported(String),
    Failed(String),
}

impl UpdateError {
    fn rejected(msg: impl Into<String>) -> Self {
        Self::Rejected(msg.into())
    }
    fn unsupported(msg: impl Into<String>) -> Self {
        Self::Unsupported(msg.into())
    }
    fn failed(msg: impl Into<String>) -> Self {
        Self::Failed(msg.into())
    }

    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => exit::USAGE,
            Self::Rejected(_) => exit::USER_ERROR,
            Self::Unsupported(_) => exit::UNSUPPORTED,
            Self::Failed(_) => exit::INTERNAL,
        }
    }

    pub(crate) fn message(&self) -> &str {
        match self {
            Self::Usage(m) | Self::Rejected(m) | Self::Unsupported(m) | Self::Failed(m) => m,
        }
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
    fn guidance(self) -> &'static str {
        match self {
            Self::Standalone => unreachable!(),
            Self::Homebrew => "brew upgrade wrightkit/tap/wright",
            Self::Scoop => "scoop update wright",
            Self::WinGet => "winget upgrade WrightKit.Wright",
        }
    }

    fn channel(self) -> &'static str {
        match self {
            Self::Standalone => unreachable!(),
            Self::Homebrew => "Homebrew",
            Self::Scoop => "Scoop",
            Self::WinGet => "WinGet",
        }
    }
}

pub(crate) fn run(check_only: bool, requested: Option<&str>) -> Result<u8, UpdateError> {
    let platform = detect_platform()?;
    let exe = std::env::current_exe()
        .map_err(|e| UpdateError::failed(format!("could not locate the wright binary: {e}")))?;
    let provenance = detect_provenance(&exe);
    if provenance != Provenance::Standalone {
        return Err(UpdateError::unsupported(format!(
            "wright appears to be managed by {} (installed at {}); upgrade with `{}` instead of `wright update`",
            provenance.channel(),
            exe.display(),
            provenance.guidance()
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
        Some(v) => {
            parse_version(v)?;
            (v.trim_start_matches('v').to_string(), None)
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
        Some(c) => c,
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
    let val: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
        UpdateError::failed(format!(
            "could not parse the latest-release response from {api_url}: {e}"
        ))
    })?;
    let tag = val.get("tag_name").and_then(serde_json::Value::as_str).ok_or_else(|| {
        UpdateError::failed(format!("could not find the latest release tag in the response from {api_url}; pin a version with `wright update --version`"))
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
    let staging = install_dir.join(format!(
        ".wright-update-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::create_dir_all(&staging).map_err(|e| {
        UpdateError::failed(format!(
            "could not create a staging directory in {}: {e}",
            install_dir.display()
        ))
    })?;
    Ok(staging)
}

fn unpack(archive: &[u8], dest: &Path, version: &str, target: &str) -> Result<(), UpdateError> {
    let decoder = flate2::read::GzDecoder::new(archive);
    tar::Archive::new(decoder).unpack(dest).map_err(|e| {
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
    std::fs::rename(source, destination).map_err(|error| {
        UpdateError::failed(format!(
            "could not replace {}: {error}; reinstall with the official installer (curl -fsSL https://wrightkit.dev/install.sh | bash)",
            destination.display()
        ))
    })
}

fn smoke_check(install_dir: &Path, version: &str) -> Result<(), UpdateError> {
    check_version(&install_dir.join("wright"), version)?;
    check_version(&install_dir.join("wright-lsp"), version)
}

fn check_version(exe: &Path, version: &str) -> Result<(), UpdateError> {
    let output = Process::new(exe)
        .arg("--version")
        .output()
        .map_err(|error| {
            UpdateError::failed(format!(
                "smoke check failed: could not run {}: {error}",
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
    let bytes = fetch(client, url)?;
    String::from_utf8(bytes)
        .map_err(|e| UpdateError::failed(format!("could not decode the response from {url}: {e}")))
}

fn fetch(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>, UpdateError> {
    let response = client.get(url).send().map_err(|e| UpdateError::failed(format!("could not download {url} ({e}); check the network connection or pin a version with `wright update --version`")))?;
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
    let core = version.strip_prefix('v').unwrap_or(version);
    let mut parts = core.split('.');
    let (Some(major), Some(minor), Some(patch), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(UpdateError::Usage(format!(
            "invalid version '{version}' (expected semver like 0.1.0)"
        )));
    };
    let parse = |part: &str| {
        part.parse::<u64>().map_err(|_| {
            UpdateError::Usage(format!(
                "invalid version '{version}' (expected semver like 0.1.0)"
            ))
        })
    };
    Ok((parse(major)?, parse(minor)?, parse(patch)?))
}

fn compare_versions(a: &str, b: &str) -> Ordering {
    let a = parse_version(a).expect("current version is a valid semver");
    let b = parse_version(b).expect("resolved/requested version is a valid semver");
    a.cmp(&b)
}

fn truncate(value: &str) -> String {
    let mut chars = value.chars();
    let head: String = chars.by_ref().take(16).collect();
    if chars.next().is_some() {
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
    fn parse_version_accepts_stable_semver_and_leading_v() {
        assert_eq!(parse_version("0.1.0").unwrap(), (0, 1, 0));
        assert_eq!(parse_version("v0.2.10").unwrap(), (0, 2, 10));
        assert_eq!(parse_version("9.9.9").unwrap(), (9, 9, 9));
    }

    #[test]
    fn parse_version_rejects_malformed_versions() {
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
    }

    #[test]
    fn compare_versions_orders_numerically() {
        use std::cmp::Ordering::*;
        assert_eq!(compare_versions("0.1.0", "0.1.0"), Equal);
        assert_eq!(compare_versions("0.2.0", "0.1.0"), Greater);
        assert_eq!(compare_versions("0.1.10", "0.1.9"), Greater);
        assert_eq!(compare_versions("0.2.0", "0.10.0"), Less);
        assert_eq!(compare_versions("1.0.0", "0.9.9"), Greater);
    }

    #[test]
    fn target_mapping_covers_the_release_matrix() {
        let platform = detect_platform_for("linux", "x86_64").unwrap();
        assert_eq!(platform.target, "x86_64-unknown-linux-gnu");
        let platform = detect_platform_for("darwin", "x86_64").unwrap();
        assert_eq!(platform.target, "x86_64-apple-darwin");
        let platform = detect_platform_for("darwin", "aarch64").unwrap();
        assert_eq!(platform.target, "aarch64-apple-darwin");
    }

    #[test]
    fn windows_and_unknown_platforms_are_refused() {
        let error = detect_platform_for("windows", "x86_64").unwrap_err();
        assert_eq!(error.exit_code(), exit::UNSUPPORTED);
        assert!(error.message().contains("winget"));
        let error = detect_platform_for("linux", "arm64").unwrap_err();
        assert_eq!(error.exit_code(), exit::UNSUPPORTED);
    }

    #[test]
    fn provenance_detects_package_managers_and_standalone() {
        assert_eq!(
            provenance_of("/opt/homebrew/bin/wright"),
            Provenance::Homebrew
        );
        assert_eq!(
            provenance_of("/usr/local/Cellar/wright/0.1.0/bin/wright"),
            Provenance::Homebrew
        );
        assert_eq!(
            provenance_of("/c/Users/me/scoop/apps/wright/0.1.0/wright.exe"),
            Provenance::Scoop
        );
        assert_eq!(
            provenance_of(
                "C:\\Users\\me\\AppData\\Local\\Microsoft\\WinGet\\Packages\\WrightKit.Wright\\wright.exe"
            ),
            Provenance::WinGet
        );
        assert_eq!(
            provenance_of("/Users/me/.local/bin/wright"),
            Provenance::Standalone
        );
        assert_eq!(
            provenance_of("/usr/local/bin/wright"),
            Provenance::Standalone
        );
    }

    #[test]
    fn checksum_verification_accepts_good_and_rejects_bad() {
        let bytes = b"the archive bytes";
        let name = "wright-9.9.9-x86_64-unknown-linux-gnu.tar.gz";
        let good = format!("{}  {name}\n", sha256_hex(bytes));
        verify_checksum(bytes, &good, name).unwrap();

        let bad = format!("{}  archive\n", sha256_hex(b"other bytes"));
        let error = verify_checksum(bytes, &bad, "archive").unwrap_err();
        assert!(error.message().contains("checksum verification failed"));

        let malformed = "not-a-hash  archive\n";
        let error = verify_checksum(bytes, malformed, "archive").unwrap_err();
        assert!(error.message().contains("invalid checksum"));

        let short = "deadbeef\n";
        assert!(verify_checksum(bytes, short, "archive").is_err());
    }

    #[test]
    fn archive_layout_is_validated_before_install() {
        let staged =
            std::env::temp_dir().join(format!("wright-update-layout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&staged);
        std::fs::create_dir_all(&staged).unwrap();
        // An archive missing wright-lsp must fail layout validation.
        let bytes = build_archive("9.9.9", &[("wright", b"#!/bin/sh\n")]);
        unpack(&bytes, &staged, "9.9.9", "x86_64-unknown-linux-gnu").unwrap();
        let error = validated_payload(&staged, "9.9.9", "x86_64-unknown-linux-gnu").unwrap_err();
        assert!(error.message().contains("unexpected archive layout"));
        let _ = std::fs::remove_dir_all(&staged);

        // A complete archive passes.
        let staged =
            std::env::temp_dir().join(format!("wright-update-layout-ok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&staged);
        std::fs::create_dir_all(&staged).unwrap();
        let bytes = build_archive(
            "9.9.9",
            &[("wright", b"#!/bin/sh\n"), ("wright-lsp", b"#!/bin/sh\n")],
        );
        unpack(&bytes, &staged, "9.9.9", "x86_64-unknown-linux-gnu").unwrap();
        let payload = validated_payload(&staged, "9.9.9", "x86_64-unknown-linux-gnu").unwrap();
        assert!(payload.join("wright").is_file());
        assert!(payload.join("wright-lsp").is_file());
        let _ = std::fs::remove_dir_all(&staged);
    }

    /// Build a gzipped tar archive with `wright-<version>-<target>/` layout.
    fn build_archive(version: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
        let dir = format!("wright-{version}-x86_64-unknown-linux-gnu");
        let mut builder = tar::Builder::new(Vec::new());
        for (name, content) in files {
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

    fn provenance_of(path: &str) -> Provenance {
        detect_provenance(Path::new(path))
    }
}
