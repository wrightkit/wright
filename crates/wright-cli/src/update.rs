//! `wright update` — self-update for standalone installations (#116).
//!
//! Resolves the latest stable Wright release from the canonical GitHub
//! Release contract (the same archives and checksums `install.sh` and the
//! package-manager manifests consume), verifies the published SHA-256
//! checksum, extracts the platform archive, and atomically replaces the
//! running `wright` and `wright-lsp` binaries.
//!
//! Package-manager-managed installations (Homebrew, Scoop, WinGet) are
//! detected from the executable location and refused with guidance to the
//! appropriate upgrade command; `wright update` never overwrites a binary it
//! does not own.
//!
//! The command is text-only (it is not a compiler workflow, so it produces no
//! `wright-result/v1` envelope) and uses the same environment overrides as
//! [`install.sh`](https://wrightkit.dev/install.sh) for testing and advanced
//! hooks:
//!
//! * `WRIGHT_INSTALL_BASE_URL` — base URL of release artifacts
//! * `WRIGHT_API_URL` — URL used to resolve the latest release
//! * `WRIGHT_INSTALL_OS` — override OS detection (linux | darwin)
//! * `WRIGHT_INSTALL_ARCH` — override CPU detection (x86_64 | aarch64)

use std::cmp::Ordering;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::time::Duration;

use sha2::Digest;

/// The canonical release-artifact base URL (mirrors `install.sh`).
const DEFAULT_BASE_URL: &str = "https://github.com/wrightkit/wright/releases/download";
/// The canonical latest-release API URL (mirrors `install.sh`).
const DEFAULT_API_URL: &str = "https://api.github.com/repos/wrightkit/wright/releases/latest";
/// The user agent sent with release metadata and download requests.
const USER_AGENT: &str = concat!("wright-update/", env!("CARGO_PKG_VERSION"));

/// The exit-code contract for `update` (shared with the compiler commands).
mod exit {
    pub(super) const SUCCESS: u8 = 0;
    /// User error (refused downgrade).
    pub(super) const USER_ERROR: u8 = 1;
    /// Usage error (unknown flag, invalid version argument).
    pub(super) const USAGE: u8 = 2;
    /// Recognized but unsupported (package-manager-managed, unsupported platform).
    pub(super) const UNSUPPORTED: u8 = 3;
    /// Internal/environment failure (network, checksum, I/O, smoke check).
    pub(super) const INTERNAL: u8 = 4;
}

/// A failure of `wright update`, carrying the actionable message the CLI
/// prints to stderr.
#[derive(Debug)]
pub(crate) enum UpdateError {
    /// The command was invoked incorrectly (exit 2).
    Usage(String),
    /// The user's request cannot be satisfied as given (exit 1).
    Rejected(String),
    /// The installation is recognized but must not be self-updated (exit 3).
    Unsupported(String),
    /// The update could not be performed (exit 4).
    Failed(String),
}

impl UpdateError {
    fn rejected(message: impl Into<String>) -> Self {
        UpdateError::Rejected(message.into())
    }
    fn unsupported(message: impl Into<String>) -> Self {
        UpdateError::Unsupported(message.into())
    }
    fn failed(message: impl Into<String>) -> Self {
        UpdateError::Failed(message.into())
    }

    /// The process exit code for this error.
    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            UpdateError::Usage(_) => exit::USAGE,
            UpdateError::Rejected(_) => exit::USER_ERROR,
            UpdateError::Unsupported(_) => exit::UNSUPPORTED,
            UpdateError::Failed(_) => exit::INTERNAL,
        }
    }

    /// The message to print on stderr.
    pub(crate) fn message(&self) -> &str {
        match self {
            UpdateError::Usage(message)
            | UpdateError::Rejected(message)
            | UpdateError::Unsupported(message)
            | UpdateError::Failed(message) => message,
        }
    }
}

/// The release target triple for the host platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Platform {
    target: &'static str,
}

/// The installation provenance of the running binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Provenance {
    /// A standalone installation (`install.sh` or a manual release archive).
    Standalone,
    Homebrew,
    Scoop,
    WinGet,
}

impl Provenance {
    /// The upgrade command a package-manager-managed installation should use
    /// instead of `wright update`.
    fn guidance(self) -> &'static str {
        match self {
            Provenance::Standalone => unreachable!("standalone installations are self-updated"),
            Provenance::Homebrew => "brew upgrade wrightkit/tap/wright",
            Provenance::Scoop => "scoop update wright",
            Provenance::WinGet => "winget upgrade WrightKit.Wright",
        }
    }

    fn channel(self) -> &'static str {
        match self {
            Provenance::Standalone => unreachable!("standalone installations are self-updated"),
            Provenance::Homebrew => "Homebrew",
            Provenance::Scoop => "Scoop",
            Provenance::WinGet => "WinGet",
        }
    }
}

/// Run the `update` workflow and return the process exit code.
///
/// `check_only` reports availability without modifying the installation;
/// `requested` pins an exact version instead of resolving the latest stable
/// release.
pub(crate) fn run(check_only: bool, requested: Option<&str>) -> Result<u8, UpdateError> {
    let platform = detect_platform()?;
    let exe = std::env::current_exe().map_err(|error| {
        UpdateError::failed(format!("could not locate the wright binary: {error}"))
    })?;
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
    // Both binaries must be replaced together so they stay on one version.
    if !install_dir.join("wright-lsp").is_file() {
        return Err(UpdateError::failed(format!(
            "wright-lsp was not found next to wright at {}; this installation was not created by the official installer — reinstall with `curl -fsSL https://wrightkit.dev/install.sh | bash` to restore a matched pair",
            install_dir.display()
        )));
    }

    let current = env!("CARGO_PKG_VERSION").to_string();
    let target_version = match requested {
        Some(version) => {
            parse_version(version)?;
            version.trim_start_matches('v').to_string()
        }
        None => resolve_latest(&env_api_url())?,
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
    install_version(&target_version, platform, &env_base_url(), install_dir)?;
    Ok(exit::SUCCESS)
}

/// Detect the host platform and map it to the release target triple.
fn detect_platform() -> Result<Platform, UpdateError> {
    detect_platform_for(&env_os(), &env_arch())
}

/// Map an OS/arch pair to the release target triple.
fn detect_platform_for(os: &str, arch: &str) -> Result<Platform, UpdateError> {
    // Standalone self-update is a Unix-channel feature; Windows installs are
    // package-manager-managed (WinGet/Scoop), which update themselves.
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

/// The OS name for platform detection (`WRIGHT_INSTALL_OS` override first).
fn env_os() -> String {
    match std::env::var("WRIGHT_INSTALL_OS") {
        Ok(override_os) => override_os,
        Err(_) => match std::env::consts::OS {
            "linux" => "linux".to_string(),
            "macos" => "darwin".to_string(),
            "windows" => "windows".to_string(),
            other => other.to_string(),
        },
    }
}

/// The CPU architecture for platform detection (`WRIGHT_INSTALL_ARCH` first).
fn env_arch() -> String {
    match std::env::var("WRIGHT_INSTALL_ARCH") {
        Ok(override_arch) => override_arch,
        Err(_) => std::env::consts::ARCH.to_string(),
    }
}

/// Detect the installation provenance of the running executable from its
/// resolved location: package-manager directories are never self-updated.
fn detect_provenance(exe: &Path) -> Provenance {
    let path = std::fs::canonicalize(exe)
        .unwrap_or_else(|_| exe.to_path_buf())
        .to_string_lossy()
        .to_lowercase();
    if path.contains("homebrew") || path.contains("cellar") {
        return Provenance::Homebrew;
    }
    if path.contains("scoop") {
        return Provenance::Scoop;
    }
    if path.contains("winget") {
        return Provenance::WinGet;
    }
    Provenance::Standalone
}

/// Resolve the latest stable release version from the GitHub Releases API.
fn resolve_latest(api_url: &str) -> Result<String, UpdateError> {
    let body = fetch_text(api_url)?;
    let value: serde_json::Value = serde_json::from_str(&body).map_err(|error| {
        UpdateError::failed(format!(
            "could not parse the latest-release response from {api_url}: {error}"
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

/// Download, verify, extract, and atomically install the release archive.
fn install_version(
    version: &str,
    platform: Platform,
    base_url: &str,
    install_dir: &Path,
) -> Result<(), UpdateError> {
    let archive_name = format!("wright-{version}-{}.tar.gz", platform.target);
    let archive_url = format!("{base_url}/v{version}/{archive_name}");
    let checksum_url = format!("{archive_url}.sha256");

    println!("==> downloading {archive_url}");
    let archive = fetch(&archive_url)?;
    let checksum = fetch_text(&checksum_url)?;
    println!("==> verifying SHA-256 checksum");
    verify_checksum(&archive, &checksum, &archive_name)?;

    ensure_writable(install_dir)?;
    let staging = staging_dir(install_dir)?;
    let result = (|| -> Result<(), UpdateError> {
        // Extract inside the install directory so the final rename is on one
        // filesystem (rename cannot cross mounts).
        unpack(&archive, &staging, version, platform.target)?;
        let payload = validated_payload(&staging, version, platform.target)?;
        // The LSP binary first so the primary `wright` is the last write.
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

/// Create a private staging directory inside `install_dir`.
fn staging_dir(install_dir: &Path) -> Result<PathBuf, UpdateError> {
    let staging = install_dir.join(format!(
        ".wright-update-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::create_dir_all(&staging).map_err(|error| {
        UpdateError::failed(format!(
            "could not create a staging directory in {}: {error}",
            install_dir.display()
        ))
    })?;
    Ok(staging)
}

/// Extract a release archive into `dest`.
fn unpack(archive: &[u8], dest: &Path, version: &str, target: &str) -> Result<(), UpdateError> {
    let decoder = flate2::read::GzDecoder::new(archive);
    let mut tar = tar::Archive::new(decoder);
    tar.unpack(dest).map_err(|error| {
        UpdateError::failed(format!(
            "failed to extract wright-{version}-{target}.tar.gz: {error}"
        ))
    })
}

/// Validate the extracted archive layout and return the payload directory.
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

/// Verify the archive's SHA-256 against the published checksum file.
fn verify_checksum(
    archive: &[u8],
    checksum_file: &str,
    archive_name: &str,
) -> Result<(), UpdateError> {
    let published = checksum_file
        .split_whitespace()
        .next()
        .ok_or_else(|| UpdateError::failed(format!("empty checksum file for {archive_name}")))?;
    if published.len() != 64 || !published.bytes().all(|byte| byte.is_ascii_hexdigit()) {
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

/// Compute the lowercase hex SHA-256 of `bytes`.
fn sha256_hex(bytes: &[u8]) -> String {
    let digest = sha2::Sha256::digest(bytes);
    digest.iter().fold(String::new(), |mut output, byte| {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
        output
    })
}

/// Refuse to touch an installation directory that cannot accept new files.
fn ensure_writable(install_dir: &Path) -> Result<(), UpdateError> {
    let probe = install_dir.join(format!(".wright-update-probe-{}", std::process::id()));
    std::fs::write(&probe, b"").map_err(|error| {
        UpdateError::failed(format!(
            "installation directory {} is not writable ({error}); fix permissions or reinstall with the official installer (curl -fsSL https://wrightkit.dev/install.sh | bash)",
            install_dir.display()
        ))
    })?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

/// Atomically replace `destination` with `source` (same directory, so the
/// rename cannot cross filesystems and is atomic on POSIX).
fn replace_binary(source: &Path, destination: &Path) -> Result<(), UpdateError> {
    // Ensure the extracted binary is executable before it goes live.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(source)
            .map_err(|error| {
                UpdateError::failed(format!("could not read {}: {error}", source.display()))
            })?
            .permissions()
            .mode();
        std::fs::set_permissions(source, std::fs::Permissions::from_mode(mode | 0o111)).map_err(
            |error| {
                UpdateError::failed(format!(
                    "could not make {} executable: {error}",
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

/// Run the installed binaries and confirm they report `version`.
fn smoke_check(install_dir: &Path, version: &str) -> Result<(), UpdateError> {
    check_version(&install_dir.join("wright"), version)?;
    check_version(&install_dir.join("wright-lsp"), version)
}

/// Run one installed binary's `--version` and confirm it reports `version`.
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

/// Fetch `url` and return the response body as UTF-8 text.
fn fetch_text(url: &str) -> Result<String, UpdateError> {
    let bytes = fetch(url)?;
    String::from_utf8(bytes).map_err(|error| {
        UpdateError::failed(format!("could not decode the response from {url}: {error}"))
    })
}

/// Fetch `url` and return the raw response body (bounded to 128 MiB).
fn fetch(url: &str) -> Result<Vec<u8>, UpdateError> {
    let response = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .timeout(Duration::from_secs(60))
        .call()
        .map_err(|error| {
            UpdateError::failed(format!(
                "could not download {url} ({error}); check the network connection or pin a version with `wright update --version`"
            ))
        })?;
    if !(200..300).contains(&response.status()) {
        return Err(UpdateError::failed(format!(
            "could not download {url} (HTTP {})",
            response.status()
        )));
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(128 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|error| UpdateError::failed(format!("could not download {url}: {error}")))?;
    Ok(bytes)
}

/// Parse a strict numeric semver (`X.Y.Z`, optionally `v`-prefixed).
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

/// Compare two strict numeric semver versions.
fn compare_versions(a: &str, b: &str) -> Ordering {
    let a = parse_version(a).expect("current version is a valid semver");
    let b = parse_version(b).expect("resolved/requested version is a valid semver");
    a.cmp(&b)
}

/// Shorten a string for error messages.
fn truncate(value: &str) -> String {
    let mut chars = value.chars();
    let head: String = chars.by_ref().take(16).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

/// The effective release-artifact base URL.
fn env_base_url() -> String {
    std::env::var("WRIGHT_INSTALL_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string())
}

/// The effective latest-release API URL.
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
