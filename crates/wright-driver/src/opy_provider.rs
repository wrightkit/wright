//! Resolution and installation of the first-party OPY LPP provider (#244).
//!
//! This module owns only distribution state. The provider process and wire
//! protocol remain owned by `wright-lpp`, and OPY project loading remains an
//! `opy-rs` concern.

use std::fmt;
use std::io::{Read, Write};
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

const DEFAULT_API_URL: &str = "https://api.github.com/repos/wrightkit/opy-rs/releases/latest";
const DEFAULT_BASE_URL: &str = "https://github.com/wrightkit/opy-rs/releases/download";
const PROVIDER_BINARY: &str = "opy-provider";
const MAX_DOWNLOAD_BYTES: u64 = 128 * 1024 * 1024;

/// The LPP language id served by the first-party OPY provider.
pub const OPY_LANGUAGE_ID: &str = "opy";

/// Settings for first-party OPY provider resolution.
#[derive(Debug, Clone, Default)]
pub struct OpyProviderConfig {
    /// An explicitly selected local executable. It has highest precedence.
    pub executable: Option<PathBuf>,
    /// Override the per-user provider store, primarily for embedding/tests.
    pub store_dir: Option<PathBuf>,
}

impl OpyProviderConfig {
    /// Select a local provider executable without enabling first-party
    /// download behavior.
    pub fn with_executable(path: impl Into<PathBuf>) -> Self {
        Self {
            executable: Some(path.into()),
            ..Self::default()
        }
    }

    /// Resolve using this configuration.
    pub fn resolve(&self) -> Result<ResolvedOpyProvider, OpyProviderError> {
        let resolver =
            OpyProviderResolver::new(self.store_dir.clone().unwrap_or_else(default_store_dir));
        resolver.resolve(self.executable.as_deref())
    }

    /// Explicitly install/update a provider version. Unlike [`Self::resolve`]
    /// for an already installed provider, this operation may contact the
    /// release source by design.
    pub fn update(&self, version: Option<&str>) -> Result<ResolvedOpyProvider, OpyProviderError> {
        let resolver =
            OpyProviderResolver::new(self.store_dir.clone().unwrap_or_else(default_store_dir));
        resolver.update(version)
    }
}

/// A resolved executable ready to pass to `wright-lpp::ProviderRegistry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedOpyProvider {
    /// The local executable path.
    pub executable: PathBuf,
    /// The installed release version, if this came from the first-party
    /// store. Explicit local providers have no release version.
    pub version: Option<String>,
}

/// First-party OPY provider resolver and installer.
#[derive(Debug, Clone)]
pub struct OpyProviderResolver {
    store_dir: PathBuf,
    target: Option<String>,
    api_url: String,
    base_url: String,
}

impl OpyProviderResolver {
    /// Create a resolver using the current supported host target.
    pub fn new(store_dir: impl Into<PathBuf>) -> Self {
        Self {
            store_dir: store_dir.into(),
            target: None,
            api_url: DEFAULT_API_URL.to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    /// Override the target triple. This is useful for deterministic tests and
    /// cross-target embedding; normal callers should leave it unset.
    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    /// Override the release endpoints without changing the artifact layout.
    pub fn with_release_urls(
        mut self,
        api_url: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        self.api_url = api_url.into();
        self.base_url = base_url.into();
        self
    }

    /// Resolve an OPY provider in precedence order: explicit local executable,
    /// active installed provider, then lazy first-party bootstrap.
    pub fn resolve(
        &self,
        explicit: Option<&Path>,
    ) -> Result<ResolvedOpyProvider, OpyProviderError> {
        if let Some(path) = explicit {
            return validate_explicit(path);
        }

        let target = self.target()?;
        if let Some((version, executable)) = self.active_provider(&target)? {
            return Ok(ResolvedOpyProvider {
                executable,
                version: Some(version),
            });
        }

        self.install_release(None, &target)
    }

    /// Explicitly update/bootstrap the provider from the first-party release
    /// source. No caller should invoke this as part of an ordinary source
    /// command.
    pub fn update(
        &self,
        requested_version: Option<&str>,
    ) -> Result<ResolvedOpyProvider, OpyProviderError> {
        let target = self.target()?;
        self.install_release(requested_version, &target)
    }

    fn target(&self) -> Result<String, OpyProviderError> {
        let target = match &self.target {
            Some(target) => Ok(target.clone()),
            None => current_target(),
        }?;
        if matches!(
            target.as_str(),
            "x86_64-unknown-linux-gnu" | "x86_64-apple-darwin" | "aarch64-apple-darwin"
        ) {
            Ok(target)
        } else {
            Err(OpyProviderError::unsupported(format!(
                "unsupported OPY provider target '{target}'"
            )))
        }
    }

    fn active_provider(&self, target: &str) -> Result<Option<(String, PathBuf)>, OpyProviderError> {
        let active = self.store_dir.join("active");
        let version = match std::fs::read_to_string(&active) {
            Ok(value) => normalize_version(value.trim())?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(OpyProviderError::install(format!(
                    "cannot read active OPY provider pointer '{}': {error}",
                    active.display()
                )));
            }
        };
        let executable = self.provider_path(&version, target);
        if is_executable(&executable) {
            Ok(Some((version, executable)))
        } else {
            Ok(None)
        }
    }

    fn provider_path(&self, version: &str, target: &str) -> PathBuf {
        self.store_dir
            .join(version)
            .join(target)
            .join(PROVIDER_BINARY)
    }

    fn install_release(
        &self,
        requested_version: Option<&str>,
        target: &str,
    ) -> Result<ResolvedOpyProvider, OpyProviderError> {
        let version = match requested_version {
            Some(version) => normalize_version(version)?,
            None => self.fetch_latest_version()?,
        };
        let archive_name = format!("opy-provider-{version}-{target}.tar.gz");
        let archive_url = format!(
            "{}/v{version}/{archive_name}",
            self.base_url.trim_end_matches('/')
        );
        let checksum_url = format!("{archive_url}.sha256");
        let archive = fetch(&archive_url)?;
        let checksum = fetch_text(&checksum_url)?;
        verify_checksum(&archive, &checksum, &archive_name)?;
        self.install_archive(&version, target, &archive)?;
        Ok(ResolvedOpyProvider {
            executable: self.provider_path(&version, target),
            version: Some(version),
        })
    }

    fn fetch_latest_version(&self) -> Result<String, OpyProviderError> {
        let body = fetch_text(&self.api_url)?;
        let value: serde_json::Value = serde_json::from_str(&body).map_err(|error| {
            OpyProviderError::download(format!(
                "cannot parse the OPY release response from {}: {error}",
                self.api_url
            ))
        })?;
        let tag = value
            .get("tag_name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                OpyProviderError::download(format!(
                    "the OPY release response from {} has no tag_name",
                    self.api_url
                ))
            })?;
        normalize_version(tag)
    }

    fn install_archive(
        &self,
        version: &str,
        target: &str,
        archive: &[u8],
    ) -> Result<(), OpyProviderError> {
        std::fs::create_dir_all(&self.store_dir).map_err(|error| {
            OpyProviderError::install(format!(
                "cannot create the OPY provider store '{}': {error}",
                self.store_dir.display()
            ))
        })?;

        let final_dir = self.store_dir.join(version).join(target);
        let final_executable = final_dir.join(PROVIDER_BINARY);
        if final_executable.is_file() {
            if !is_executable(&final_executable) {
                return Err(OpyProviderError::install(format!(
                    "installed OPY provider '{}' is not executable",
                    final_executable.display()
                )));
            }
            return self.activate(version);
        }
        if final_dir.exists() {
            return Err(OpyProviderError::install(format!(
                "cannot install OPY provider: target directory '{}' already exists without a valid executable",
                final_dir.display()
            )));
        }

        let staging = self.store_dir.join(format!(
            ".opy-provider-{version}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        let result = (|| -> Result<(), OpyProviderError> {
            std::fs::create_dir_all(&staging).map_err(|error| {
                OpyProviderError::install(format!(
                    "cannot create OPY provider staging directory '{}': {error}",
                    staging.display()
                ))
            })?;
            extract_provider(archive, &staging, version, target)?;
            std::fs::create_dir_all(final_dir.parent().expect("provider target has a parent"))
                .map_err(|error| {
                    OpyProviderError::install(format!(
                        "cannot create OPY provider version directory '{}': {error}",
                        final_dir.display()
                    ))
                })?;
            std::fs::rename(&staging, &final_dir).map_err(|error| {
                OpyProviderError::install(format!(
                    "cannot activate staged OPY provider '{}': {error}",
                    final_dir.display()
                ))
            })?;
            self.activate(version)
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&staging);
        }
        result
    }

    fn activate(&self, version: &str) -> Result<(), OpyProviderError> {
        let temporary = self.store_dir.join(format!(
            "active.{}.{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        let result = (|| -> Result<(), OpyProviderError> {
            let mut file = std::fs::File::create(&temporary).map_err(|error| {
                OpyProviderError::install(format!(
                    "cannot write temporary OPY provider pointer '{}': {error}",
                    temporary.display()
                ))
            })?;
            file.write_all(version.as_bytes()).map_err(|error| {
                OpyProviderError::install(format!(
                    "cannot write temporary OPY provider pointer '{}': {error}",
                    temporary.display()
                ))
            })?;
            file.sync_all().map_err(|error| {
                OpyProviderError::install(format!(
                    "cannot persist temporary OPY provider pointer '{}': {error}",
                    temporary.display()
                ))
            })?;
            std::fs::rename(&temporary, self.store_dir.join("active")).map_err(|error| {
                OpyProviderError::install(format!(
                    "cannot atomically activate OPY provider {version}: {error}"
                ))
            })
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}

/// A machine-readable provider distribution failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpyProviderError {
    Missing(String),
    UnsupportedPlatform(String),
    Offline(String),
    Download(String),
    Integrity(String),
    Install(String),
}

impl OpyProviderError {
    fn missing(message: impl Into<String>) -> Self {
        Self::Missing(message.into())
    }
    fn unsupported(message: impl Into<String>) -> Self {
        Self::UnsupportedPlatform(message.into())
    }
    fn offline(message: impl Into<String>) -> Self {
        Self::Offline(message.into())
    }
    fn download(message: impl Into<String>) -> Self {
        Self::Download(message.into())
    }
    fn integrity(message: impl Into<String>) -> Self {
        Self::Integrity(message.into())
    }
    fn install(message: impl Into<String>) -> Self {
        Self::Install(message.into())
    }

    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Missing(_) => "provider-missing",
            Self::UnsupportedPlatform(_) => "provider-unsupported-platform",
            Self::Offline(_) => "provider-offline",
            Self::Download(_) => "provider-download",
            Self::Integrity(_) => "provider-integrity",
            Self::Install(_) => "provider-install",
        }
    }

    /// The CLI exit code for this failure.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::UnsupportedPlatform(_) => crate::result::exit::UNSUPPORTED,
            _ => crate::result::exit::INTERNAL,
        }
    }
}

impl fmt::Display for OpyProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(message)
            | Self::UnsupportedPlatform(message)
            | Self::Offline(message)
            | Self::Download(message)
            | Self::Integrity(message)
            | Self::Install(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for OpyProviderError {}

fn validate_explicit(path: &Path) -> Result<ResolvedOpyProvider, OpyProviderError> {
    if !is_executable(path) {
        return Err(OpyProviderError::missing(format!(
            "explicit OPY provider '{}' does not exist or is not executable",
            path.display()
        )));
    }
    Ok(ResolvedOpyProvider {
        executable: path.to_path_buf(),
        version: None,
    })
}

fn current_target() -> Result<String, OpyProviderError> {
    target_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn target_for(os: &str, arch: &str) -> Result<String, OpyProviderError> {
    match (os, arch) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu".to_string()),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin".to_string()),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin".to_string()),
        _ => Err(OpyProviderError::unsupported(format!(
            "unsupported OPY provider target {os}/{arch}; supported targets are linux/x86_64 and darwin x86_64/aarch64"
        ))),
    }
}

fn default_store_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("WRIGHT_PROVIDER_DATA_DIR") {
        return PathBuf::from(path).join("providers").join("opy");
    }
    let base = match std::env::consts::OS {
        "macos" => std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|path| path.join("Library").join("Application Support")),
        "windows" => std::env::var_os("APPDATA").map(PathBuf::from),
        _ => std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".local"))),
    };
    base.unwrap_or_else(|| PathBuf::from("."))
        .join("wright")
        .join("providers")
        .join("opy")
}

fn normalize_version(version: &str) -> Result<String, OpyProviderError> {
    let version = version.trim_start_matches('v');
    let mut parts = version.split('.');
    let valid = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(a), Some(b), Some(c), None) => [a, b, c]
            .into_iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())),
        _ => false,
    };
    if !valid {
        return Err(OpyProviderError::download(format!(
            "invalid OPY provider release version '{version}' (expected X.Y.Z)"
        )));
    }
    Ok(version.to_string())
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn fetch_text(url: &str) -> Result<String, OpyProviderError> {
    let bytes = fetch(url)?;
    String::from_utf8(bytes).map_err(|error| {
        OpyProviderError::download(format!("cannot decode response from {url}: {error}"))
    })
}

fn fetch(url: &str) -> Result<Vec<u8>, OpyProviderError> {
    let response = ureq::get(url)
        .set(
            "User-Agent",
            concat!("wright-opy-provider/", env!("CARGO_PKG_VERSION")),
        )
        .timeout(Duration::from_secs(60))
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(status, _) => {
                OpyProviderError::download(format!("GET {url} failed with HTTP {status}"))
            }
            ureq::Error::Transport(error) => OpyProviderError::offline(format!(
                "cannot reach OPY provider release source {url}: {error}"
            )),
        })?;
    if !(200..300).contains(&response.status()) {
        return Err(OpyProviderError::download(format!(
            "GET {url} failed with HTTP {}",
            response.status()
        )));
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_DOWNLOAD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            OpyProviderError::download(format!("cannot read response from {url}: {error}"))
        })?;
    if bytes.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err(OpyProviderError::download(format!(
            "response from {url} exceeds the 128 MiB provider artifact limit"
        )));
    }
    Ok(bytes)
}

fn verify_checksum(
    archive: &[u8],
    checksum_file: &str,
    archive_name: &str,
) -> Result<(), OpyProviderError> {
    let published = checksum_file.split_whitespace().next().ok_or_else(|| {
        OpyProviderError::integrity(format!("empty checksum file for {archive_name}"))
    })?;
    if published.len() != 64 || !published.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(OpyProviderError::integrity(format!(
            "invalid SHA-256 checksum for {archive_name}"
        )));
    }
    let actual = Sha256::digest(archive);
    let actual = actual
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            use std::fmt::Write as _;
            let _ = write!(output, "{byte:02x}");
            output
        });
    if !actual.eq_ignore_ascii_case(published) {
        return Err(OpyProviderError::integrity(format!(
            "SHA-256 verification failed for {archive_name} (published {published}, got {actual}); the active provider was not changed"
        )));
    }
    Ok(())
}

fn extract_provider(
    archive: &[u8],
    destination: &Path,
    version: &str,
    target: &str,
) -> Result<(), OpyProviderError> {
    let expected_root = format!("opy-provider-{version}-{target}");
    let decoder = GzDecoder::new(archive);
    let mut archive = tar::Archive::new(decoder);
    let mut found = false;
    let entries = archive.entries().map_err(|error| {
        OpyProviderError::install(format!("cannot read OPY provider archive: {error}"))
    })?;
    for entry in entries {
        let mut entry = entry.map_err(|error| {
            OpyProviderError::install(format!("cannot read OPY provider archive entry: {error}"))
        })?;
        let path = entry.path().map_err(|error| {
            OpyProviderError::install(format!("cannot inspect OPY provider archive path: {error}"))
        })?;
        let components: Vec<_> = path.components().collect();
        let expected_path = Path::new(&expected_root).join(PROVIDER_BINARY);
        let is_root = components.len() == 1
            && components[0] == std::path::Component::Normal(expected_root.as_ref());
        if path == expected_path {
            if !entry.header().entry_type().is_file() {
                return Err(OpyProviderError::install(
                    "OPY provider archive executable is not a regular file",
                ));
            }
            let output = destination.join(PROVIDER_BINARY);
            let mut file = std::fs::File::create(&output).map_err(|error| {
                OpyProviderError::install(format!(
                    "cannot create staged OPY provider '{}': {error}",
                    output.display()
                ))
            })?;
            std::io::copy(&mut entry, &mut file).map_err(|error| {
                OpyProviderError::install(format!(
                    "cannot unpack staged OPY provider '{}': {error}",
                    output.display()
                ))
            })?;
            file.sync_all().map_err(|error| {
                OpyProviderError::install(format!(
                    "cannot persist staged OPY provider '{}': {error}",
                    output.display()
                ))
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = entry.header().mode().unwrap_or(0o755) | 0o111;
                std::fs::set_permissions(&output, std::fs::Permissions::from_mode(mode)).map_err(
                    |error| {
                        OpyProviderError::install(format!(
                            "cannot make staged OPY provider executable '{}': {error}",
                            output.display()
                        ))
                    },
                )?;
            }
            found = true;
        } else if !is_root {
            return Err(OpyProviderError::install(
                "OPY provider archive contains an unexpected path",
            ));
        }
    }
    if !found || !is_executable(&destination.join(PROVIDER_BINARY)) {
        return Err(OpyProviderError::install(
            "OPY provider archive does not contain an executable provider",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};
    use std::sync::{
        Arc,
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

    fn archive(version: &str, target: &str, body: &[u8]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(
                &mut header,
                format!("opy-provider-{version}-{target}/{PROVIDER_BINARY}"),
                body,
            )
            .unwrap();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&builder.into_inner().unwrap()).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn explicit_provider_wins_without_target_or_network_access() {
        let root = test_root("explicit");
        let executable = root.join(PROVIDER_BINARY);
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
        let first_sum = format!("{}  archive\n", hex(&first));
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
        let offline = resolver.with_release_urls("http://127.0.0.1:1/latest", "http://127.0.0.1:1");
        let resolved = offline.resolve(None).unwrap();
        assert_eq!(resolved.version.as_deref(), Some("2.0.0"));
        assert_eq!(std::fs::read(resolved.executable).unwrap(), b"cached");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_provider_bootstraps_from_release_api_and_artifacts() {
        let root = test_root("bootstrap");
        let target = "x86_64-unknown-linux-gnu";
        let version = "3.1.4";
        let bytes = archive(version, target, b"bootstrapped");
        let checksum = format!("{}  opy-provider-{version}-{target}.tar.gz\n", hex(&bytes));
        let (base_url, requests, server) = test_server(
            format!(r#"{{"tag_name":"v{version}"}}"#).into_bytes(),
            bytes,
            checksum.into_bytes(),
        );
        let resolver = OpyProviderResolver::new(&root)
            .with_target(target)
            .with_release_urls(format!("{base_url}/latest"), &base_url);
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
    fn unsupported_target_is_structured() {
        let error = OpyProviderResolver::new(test_root("target"))
            .with_target("mips-unknown-linux-gnu")
            .update(Some("1.0.0"))
            .unwrap_err();
        assert_eq!(error.code(), "provider-unsupported-platform");
        let error = target_for("windows", "x86_64").unwrap_err();
        assert_eq!(error.code(), "provider-unsupported-platform");
    }

    fn hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .fold(String::new(), |mut value, byte| {
                use std::fmt::Write as _;
                let _ = write!(value, "{byte:02x}");
                value
            })
    }

    fn test_server(
        api: Vec<u8>,
        archive: Vec<u8>,
        checksum: Vec<u8>,
    ) -> (String, Arc<AtomicUsize>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&requests);
        let server = thread::spawn(move || {
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                respond(&mut stream, &api, &archive, &checksum);
                seen.fetch_add(1, Ordering::Relaxed);
            }
        });
        (format!("http://{address}"), requests, server)
    }

    fn respond(stream: &mut TcpStream, api: &[u8], archive: &[u8], checksum: &[u8]) {
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
        let body = if path.ends_with("/latest") {
            api
        } else if path.ends_with(".sha256") {
            checksum
        } else {
            archive
        };
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
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
}
