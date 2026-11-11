use std::cmp::Ordering;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

mod download;
use download::{extract_provider, fetch, fetch_text, provider_client, verify_checksum};

const DEFAULT_BASE_URL: &str = "https://releases.wrightkit.dev/opy-rs";
const MAX_DOWNLOAD_BYTES: u64 = 128 * 1024 * 1024;
const PROVIDER_ARCHIVE_EXTENSION: &str = "tar.gz";
const SUPPORTED_TARGETS: [&str; 4] = [
    "x86_64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
];

pub const OPY_LANGUAGE_ID: &str = "opy";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpyProviderConfig {
    pub executable: Option<PathBuf>,
    pub store_dir: Option<PathBuf>,
}

impl OpyProviderConfig {
    pub fn with_executable(path: impl Into<PathBuf>) -> Self {
        Self {
            executable: Some(path.into()),
            ..Self::default()
        }
    }

    fn resolver(&self) -> OpyProviderResolver {
        OpyProviderResolver::new(self.store_dir.clone().unwrap_or_else(default_store_dir))
    }

    pub fn resolve(&self) -> Result<ResolvedOpyProvider, OpyProviderError> {
        self.resolver().resolve(self.executable.as_deref())
    }

    pub fn update(&self, version: Option<&str>) -> Result<ResolvedOpyProvider, OpyProviderError> {
        self.resolver().update(version)
    }

    pub fn installed(&self) -> Result<Option<ResolvedOpyProvider>, OpyProviderError> {
        self.resolver().installed()
    }

    pub fn latest_version(&self) -> Result<String, OpyProviderError> {
        self.resolver().latest_version()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedOpyProvider {
    pub executable: PathBuf,
    pub version: Option<String>,
}

impl ResolvedOpyProvider {
    /// Run an installed provider's bounded initialize/shutdown warm-up.
    pub fn warm_up(&self) -> Result<(), wright_lpp::ProviderError> {
        use wright_lpp::{LanguageProvider, StdioLanguageProvider};

        let mut provider =
            StdioLanguageProvider::spawn(&self.executable, &[], std::time::Duration::from_secs(5))?;
        provider.initialize(None)?;
        let _ = provider.shutdown();
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct OpyProviderResolver {
    store_dir: PathBuf,
    target: Option<String>,
    base_url: String,
}

impl OpyProviderResolver {
    pub fn new(store_dir: impl Into<PathBuf>) -> Self {
        Self {
            store_dir: store_dir.into(),
            target: None,
            base_url: std::env::var("WRIGHT_OPY_PROVIDER_BASE_URL")
                .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string()),
        }
    }

    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    /// Override the release-distribution base; the `latest/version` pointer
    /// and the `releases/<version>/` artifacts live under it.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

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

    /// Downgrading requires an explicit `version`: a bare `update` keeps a
    /// newer active install instead of rolling it back.
    pub fn update(
        &self,
        requested_version: Option<&str>,
    ) -> Result<ResolvedOpyProvider, OpyProviderError> {
        let target = self.target()?;
        self.install_release(requested_version, &target)
    }

    /// The provider activated in the store, when one is usable. Purely local:
    /// it never touches the network and never installs anything.
    pub fn installed(&self) -> Result<Option<ResolvedOpyProvider>, OpyProviderError> {
        let target = self.target()?;
        Ok(self
            .active_provider(&target)?
            .map(|(version, executable)| ResolvedOpyProvider {
                executable,
                version: Some(version),
            }))
    }

    /// The latest published provider version. Fetches the version pointer
    /// only; nothing is downloaded or installed.
    pub fn latest_version(&self) -> Result<String, OpyProviderError> {
        let client = provider_client()?;
        self.fetch_latest_version(&client)
    }

    fn target(&self) -> Result<String, OpyProviderError> {
        let target = match &self.target {
            Some(t) => Ok(t.clone()),
            None => current_target(),
        }?;
        if SUPPORTED_TARGETS.contains(&target.as_str()) {
            Ok(target)
        } else {
            Err(OpyProviderError::unsupported(format!(
                "unsupported OPY provider target '{}'",
                target
            )))
        }
    }

    fn active_provider(&self, target: &str) -> Result<Option<(String, PathBuf)>, OpyProviderError> {
        let active = self.store_dir.join("active");
        let version = match std::fs::read_to_string(&active) {
            // A corrupt pointer counts as no active provider; the next
            // install rewrites it.
            Ok(val) => match normalize_version(val.trim()) {
                Ok(version) => version,
                Err(_) => return Ok(None),
            },
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidData
                ) =>
            {
                return Ok(None);
            }
            Err(err) => {
                return Err(OpyProviderError::install(format!(
                    "cannot read active OPY provider pointer '{}': {}",
                    active.display(),
                    err
                )));
            }
        };
        let executable = self.provider_path(&version, target);
        Ok(if is_executable(&executable) {
            Some((version, executable))
        } else {
            None
        })
    }

    fn provider_path(&self, version: &str, target: &str) -> PathBuf {
        self.store_dir
            .join(version)
            .join(target)
            .join(provider_binary(target))
    }

    fn install_release(
        &self,
        requested_version: Option<&str>,
        target: &str,
    ) -> Result<ResolvedOpyProvider, OpyProviderError> {
        let requested_version = requested_version.map(normalize_version).transpose()?;
        let client = provider_client()?;
        let version = match &requested_version {
            Some(v) => v.clone(),
            None => self.fetch_latest_version(&client)?,
        };
        if let Some((active, executable)) = self.active_provider(target)? {
            let already_active = active == version;
            let implicit_downgrade = requested_version.is_none()
                && matches!(
                    (version_tuple(&active), version_tuple(&version)),
                    (Some(installed), Some(resolved)) if installed.cmp(&resolved) == Ordering::Greater
                );
            if already_active || implicit_downgrade {
                return Ok(ResolvedOpyProvider {
                    executable,
                    version: Some(active),
                });
            }
        }
        let archive_name = format!("opy-provider-{version}-{target}.{PROVIDER_ARCHIVE_EXTENSION}");
        let archive_url = format!(
            "{}/releases/{version}/{archive_name}",
            self.base_url.trim_end_matches('/')
        );
        let checksum_url = format!("{archive_url}.sha256");
        let archive = fetch(&client, &archive_url)?;
        let checksum = fetch_text(&client, &checksum_url)?;
        verify_checksum(&archive, &checksum, &archive_name)?;
        self.install_archive(&version, target, &archive)?;
        Ok(ResolvedOpyProvider {
            executable: self.provider_path(&version, target),
            version: Some(version),
        })
    }

    fn fetch_latest_version(
        &self,
        client: &reqwest::blocking::Client,
    ) -> Result<String, OpyProviderError> {
        let url = format!("{}/latest/version", self.base_url.trim_end_matches('/'));
        let body = fetch_text(client, &url)?;
        normalize_version(body.trim())
    }

    fn install_archive(
        &self,
        version: &str,
        target: &str,
        archive: &[u8],
    ) -> Result<(), OpyProviderError> {
        std::fs::create_dir_all(&self.store_dir).map_err(|e| {
            OpyProviderError::install(format!(
                "cannot create the OPY provider store '{}': {}",
                self.store_dir.display(),
                e
            ))
        })?;

        let final_dir = self.store_dir.join(version).join(target);
        let final_executable = final_dir.join(provider_binary(target));
        if is_executable(&final_executable) {
            return self.activate(version, target);
        }
        if final_dir.exists() {
            std::fs::remove_dir_all(&final_dir).map_err(|e| {
                OpyProviderError::install(format!(
                    "cannot remove incomplete OPY provider '{}': {}",
                    final_dir.display(),
                    e
                ))
            })?;
        }

        let staging = self.staging_dir(version)?;
        let result = (|| -> Result<(), OpyProviderError> {
            extract_provider(archive, &staging, target)?;
            self.promote_staging(&staging, version, target)
        })();
        let _ = std::fs::remove_dir_all(&staging);
        result
    }

    fn promote_staging(
        &self,
        staging: &Path,
        version: &str,
        target: &str,
    ) -> Result<(), OpyProviderError> {
        let final_dir = self.store_dir.join(version).join(target);
        let final_executable = final_dir.join(provider_binary(target));
        crate::store::promote_dir(staging, &final_dir, || is_executable(&final_executable))
            .map_err(|error| {
                OpyProviderError::install(match error {
                    crate::store::PromoteError::CreateParent(error) => format!(
                        "cannot create OPY provider version directory '{}': {error}",
                        final_dir.display()
                    ),
                    crate::store::PromoteError::Rename(error) => format!(
                        "cannot activate staged OPY provider '{}': {error}",
                        final_dir.display()
                    ),
                })
            })?;
        self.activate(version, target)
    }

    fn staging_dir(&self, version: &str) -> Result<PathBuf, OpyProviderError> {
        crate::store::reserve_staging_dir(
            &self.store_dir,
            &format!(".opy-provider-{version}"),
            "an OPY provider",
        )
        .map_err(OpyProviderError::install)
    }

    fn activate(&self, version: &str, target: &str) -> Result<(), OpyProviderError> {
        let temporary = self.store_dir.join(format!(
            "active.{}.{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let res = (|| -> Result<(), OpyProviderError> {
            let mut file = std::fs::File::create(&temporary).map_err(|e| {
                OpyProviderError::install(format!(
                    "cannot write temporary OPY provider pointer '{}': {}",
                    temporary.display(),
                    e
                ))
            })?;
            write!(file, "{version}").map_err(|e| {
                OpyProviderError::install(format!(
                    "cannot write temporary OPY provider pointer '{}': {}",
                    temporary.display(),
                    e
                ))
            })?;
            file.sync_all().map_err(|e| {
                OpyProviderError::install(format!(
                    "cannot persist temporary OPY provider pointer '{}': {}",
                    temporary.display(),
                    e
                ))
            })?;
            let active = self.store_dir.join("active");
            match std::fs::rename(&temporary, &active) {
                Ok(()) => Ok(()),
                Err(_) if matches!(self.active_provider(target)?, Some((active, _)) if active == version) => {
                    Ok(())
                }
                Err(error) => Err(OpyProviderError::install(format!(
                    "cannot atomically activate OPY provider {}: {}",
                    version, error
                ))),
            }
        })();
        let _ = std::fs::remove_file(&temporary);
        res
    }
}

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
    let [linux_gnu, darwin_x86_64, darwin_aarch64, windows_msvc] = SUPPORTED_TARGETS;
    match (os, arch) {
        ("linux", "x86_64") => Ok(linux_gnu.to_string()),
        ("macos", "x86_64") => Ok(darwin_x86_64.to_string()),
        ("macos", "aarch64") => Ok(darwin_aarch64.to_string()),
        ("windows", "x86_64") => Ok(windows_msvc.to_string()),
        _ => Err(OpyProviderError::unsupported(format!(
            "unsupported OPY provider target {}/{}; supported targets are linux/x86_64, darwin x86_64/aarch64, and windows x86_64",
            os, arch
        ))),
    }
}

fn default_store_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("WRIGHT_PROVIDER_DATA_DIR") {
        return PathBuf::from(path).join("providers").join("opy");
    }
    crate::store::wright_data_base()
        .join("providers")
        .join("opy")
}

fn normalize_version(version: &str) -> Result<String, OpyProviderError> {
    let core = version.trim_start_matches('v');
    let mut parts = core.split('.');
    let (Some(major), Some(minor), Some(patch), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(OpyProviderError::download(format!(
            "invalid OPY provider release version '{}' (expected X.Y.Z)",
            core
        )));
    };
    if [major, minor, patch]
        .into_iter()
        .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    {
        Ok(core.to_string())
    } else {
        Err(OpyProviderError::download(format!(
            "invalid OPY provider release version '{}' (expected X.Y.Z)",
            core
        )))
    }
}

fn version_tuple(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let tuple = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(tuple)
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

fn provider_binary(target: &str) -> &'static str {
    if target == "x86_64-pc-windows-msvc" {
        "opy-provider.exe"
    } else {
        "opy-provider"
    }
}

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
    fn missing(msg: impl Into<String>) -> Self {
        Self::Missing(msg.into())
    }
    fn unsupported(msg: impl Into<String>) -> Self {
        Self::UnsupportedPlatform(msg.into())
    }
    fn offline(msg: impl Into<String>) -> Self {
        Self::Offline(msg.into())
    }
    fn download(msg: impl Into<String>) -> Self {
        Self::Download(msg.into())
    }
    fn integrity(msg: impl Into<String>) -> Self {
        Self::Integrity(msg.into())
    }
    fn install(msg: impl Into<String>) -> Self {
        Self::Install(msg.into())
    }

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

    pub fn exit_code(&self) -> u8 {
        match self {
            Self::UnsupportedPlatform(_) => crate::result::exit::UNSUPPORTED,
            _ => crate::result::exit::INTERNAL,
        }
    }
}

impl fmt::Display for OpyProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(m)
            | Self::UnsupportedPlatform(m)
            | Self::Offline(m)
            | Self::Download(m)
            | Self::Integrity(m)
            | Self::Install(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for OpyProviderError {}

impl From<OpyProviderError> for wright_lpp::ProviderError {
    fn from(error: OpyProviderError) -> Self {
        let kind = match &error {
            OpyProviderError::Missing(_) => wright_lpp::LocalProviderErrorKind::Missing,
            OpyProviderError::UnsupportedPlatform(_) => {
                wright_lpp::LocalProviderErrorKind::UnsupportedPlatform
            }
            OpyProviderError::Offline(_) => wright_lpp::LocalProviderErrorKind::Offline,
            OpyProviderError::Download(_) => wright_lpp::LocalProviderErrorKind::Download,
            OpyProviderError::Integrity(_) => wright_lpp::LocalProviderErrorKind::Integrity,
            OpyProviderError::Install(_) => wright_lpp::LocalProviderErrorKind::Install,
        };
        wright_lpp::ProviderError::Local {
            kind,
            message: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests;
