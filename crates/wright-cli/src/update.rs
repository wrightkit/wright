use std::cmp::Ordering;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use clap::{Args, Subcommand, ValueEnum};
use wright_driver::result::exit;
use wright_driver::{OpyProviderConfig, OpyProviderError, ResolvedOpyProvider, sha256_hex};

const DEFAULT_BASE_URL: &str = "https://releases.wrightkit.dev/wright";
const USER_AGENT: &str = concat!("wright-update/", env!("CARGO_PKG_VERSION"));

mod install;

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
            Self::Rejected(_) => exit::SOURCE_ERROR,
            Self::Unsupported(_) => exit::UNSUPPORTED,
            Self::Failed(_) => exit::INTERNAL,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::Usage(_) => "usage",
            Self::Rejected(_) => "rejected",
            Self::Unsupported(_) => "unsupported",
            Self::Failed(_) => "failed",
        }
    }

    pub(crate) fn record(&self) -> String {
        format!(
            "error: {} {}",
            self.code(),
            crate::plain::field("message", self.message())
        )
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
    windows: bool,
}

impl Platform {
    fn archive_name(self, version: &str) -> String {
        let ext = if self.windows { "zip" } else { "tar.gz" };
        format!("wright-{version}-{}.{ext}", self.target)
    }

    fn exe(self, name: &str) -> String {
        if self.windows {
            format!("{name}.exe")
        } else {
            name.to_string()
        }
    }

    fn installer(self) -> &'static str {
        if self.windows {
            "irm https://install.wrightkit.dev/wright/install.ps1 | iex"
        } else {
            "curl -fsSL https://install.wrightkit.dev/wright/install.sh | bash"
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Provenance {
    Standalone,
    Homebrew,
    Scoop,
    WinGet,
}

impl Provenance {
    fn manager(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Standalone => None,
            Self::Homebrew => Some(("Homebrew", "brew upgrade wrightkit/tap/wright")),
            Self::Scoop => Some(("Scoop", "scoop update wright")),
            Self::WinGet => Some(("WinGet", "winget upgrade WrightKit.Wright")),
        }
    }
}

#[derive(Debug, Args)]
pub(crate) struct UpdateArgs {
    /// Resolve update targets and report available updates without modifying
    /// the installation.
    #[arg(long)]
    pub(crate) check: bool,
    /// Renderer environment; `auto` detects terminal, CI, and GitHub Actions.
    #[arg(long, value_enum, default_value_t = crate::cli::RendererArg::Auto, global = true)]
    pub(crate) renderer: crate::cli::RendererArg,
    /// ANSI color policy.
    #[arg(long, value_enum, default_value_t = crate::cli::ColorArg::Auto, global = true)]
    pub(crate) color: crate::cli::ColorArg,
    #[command(subcommand)]
    pub(crate) command: Option<UpdateCommand>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum UpdateCommand {
    /// Update the standalone Wright installation only.
    #[command(name = "self")]
    SelfUpdate(UpdateSelfArgs),
    /// Update installed first-party providers, or install/update one provider
    /// by name.
    Provider(UpdateProviderArgs),
}

#[derive(Debug, Args)]
pub(crate) struct UpdateSelfArgs {
    /// Report the available update without modifying the installation.
    #[arg(long)]
    pub(crate) check: bool,
    /// Install an exact version instead of the latest stable release.
    #[arg(long, value_name = "VERSION")]
    pub(crate) version: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct UpdateProviderArgs {
    /// Provider to install or update; omitted updates installed providers only.
    pub(crate) provider: Option<ProviderNameArg>,
    /// Report the available update without installing anything.
    #[arg(long)]
    pub(crate) check: bool,
    /// Install an exact release instead of the latest stable release.
    #[arg(long, value_name = "VERSION", requires = "provider")]
    pub(crate) version: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ProviderNameArg {
    Opy,
}

pub(crate) fn run(
    args: &UpdateArgs,
    pres: crate::present::Presentation,
) -> Result<u8, UpdateError> {
    match &args.command {
        Some(UpdateCommand::SelfUpdate(sub)) => {
            self_update(args.check || sub.check, sub.version.as_deref())
        }
        Some(UpdateCommand::Provider(sub)) => Ok(provider_target(
            args.check || sub.check,
            sub.version.as_deref(),
            sub.provider.is_some(),
            pres,
        )),
        None => Ok(update_all(args.check, pres)),
    }
}

fn update_all(check_only: bool, pres: crate::present::Presentation) -> u8 {
    let mut code = exit::SUCCESS;
    match self_update(check_only, None) {
        Ok(_) => {}
        Err(error @ UpdateError::Unsupported(_)) => {
            println!("skipped wright self-update: {}", error.message());
        }
        Err(error) => {
            report_update_error(
                pres,
                crate::present::terminal::exit_word(error.exit_code()),
                error.message(),
                &error.record(),
            );
            code = fold(code, error.exit_code());
        }
    }
    code = fold(code, provider_target(check_only, None, false, pres));
    code
}

fn fold(code: u8, next: u8) -> u8 {
    if code == exit::SUCCESS { next } else { code }
}

fn provider_target(
    check_only: bool,
    requested: Option<&str>,
    may_install: bool,
    pres: crate::present::Presentation,
) -> u8 {
    let installed = match OpyProviderConfig::default().installed() {
        Ok(installed) => installed,
        Err(error @ OpyProviderError::UnsupportedPlatform(_)) if !may_install => {
            println!("skipped opy provider: {error}");
            return exit::SUCCESS;
        }
        Err(error) => return provider_failure(&error, pres),
    };
    if check_only {
        return provider_check(installed.as_ref(), requested, may_install, pres);
    }
    if installed.is_none() && !may_install {
        println!("opy provider is not installed; skipping");
        return exit::SUCCESS;
    }
    let previous = installed
        .as_ref()
        .and_then(|p| p.version.as_deref())
        .map(str::to_string);
    match OpyProviderConfig::default().update(requested) {
        Ok(resolved) => {
            provider_report(previous.as_deref(), &resolved);
            exit::SUCCESS
        }
        Err(error) => provider_failure(&error, pres),
    }
}

fn provider_report(previous: Option<&str>, resolved: &ResolvedOpyProvider) {
    let Some(version) = resolved.version.as_deref() else {
        println!(
            "installed opy provider at {}",
            resolved.executable.display()
        );
        return;
    };
    match previous {
        None => println!(
            "installed opy provider {version} at {}",
            resolved.executable.display()
        ),
        Some(current) => match compare_versions(current, version) {
            Some(Ordering::Equal) => println!("opy provider {version} is already up to date"),
            Some(Ordering::Less) => println!("updated opy provider {current} -> {version}"),
            Some(Ordering::Greater) => println!("downgraded opy provider {current} -> {version}"),
            None => println!("opy provider {version} is active (was {current})"),
        },
    }
}

fn provider_check(
    installed: Option<&ResolvedOpyProvider>,
    requested: Option<&str>,
    may_install: bool,
    pres: crate::present::Presentation,
) -> u8 {
    let current = installed.and_then(|p| p.version.as_deref());
    if current.is_none() && !may_install {
        println!("opy provider is not installed; skipping");
        return exit::SUCCESS;
    }
    let target = match requested {
        Some(version) => match parse_version(version) {
            Ok(_) => version.trim_start_matches('v').to_string(),
            Err(error) => {
                report_update_error(
                    pres,
                    crate::present::terminal::exit_word(error.exit_code()),
                    error.message(),
                    &error.record(),
                );
                return error.exit_code();
            }
        },
        None => match OpyProviderConfig::default().latest_version() {
            Ok(version) => version,
            Err(error) => return provider_failure(&error, pres),
        },
    };
    let Some(current) = current else {
        println!(
            "opy provider is not installed; `wright update provider opy` would install {target}"
        );
        return exit::SUCCESS;
    };
    match compare_versions(current, &target) {
        Some(Ordering::Equal) => println!("opy provider {current} is already up to date"),
        Some(Ordering::Less) => println!(
            "update available: opy provider {current} -> {target} (run `wright update provider opy` to install)"
        ),
        Some(Ordering::Greater) if requested.is_some() => println!(
            "opy provider {target} is older than the installed {current}; `wright update provider opy --version {target}` would downgrade"
        ),
        Some(Ordering::Greater) => println!(
            "installed opy provider {current} is newer than the latest release {target}; keeping {current}"
        ),
        None => println!(
            "installed opy provider {current} is not a recognized release version; latest release is {target}"
        ),
    }
    exit::SUCCESS
}

fn provider_failure(error: &OpyProviderError, pres: crate::present::Presentation) -> u8 {
    report_update_error(
        pres,
        crate::present::terminal::exit_word(error.exit_code()),
        &format!("{}: {}", error.code(), error),
        &format!(
            "error: {} {}",
            error.code(),
            crate::plain::field("message", &error.to_string())
        ),
    );
    error.exit_code()
}

fn report_update_error(
    pres: crate::present::Presentation,
    word: &str,
    message: &str,
    record: &str,
) {
    if pres.is_terminal() {
        crate::present::terminal::report_error(pres, word, "update", message);
    } else {
        eprintln!("{record}");
    }
}

fn self_update(check_only: bool, requested: Option<&str>) -> Result<u8, UpdateError> {
    let platform = detect_platform()?;
    let exe = std::env::current_exe()
        .map_err(|e| UpdateError::failed(format!("could not locate the wright binary: {e}")))?;
    let provenance = detect_provenance(&exe);
    if let Some((channel, guidance)) = provenance.manager() {
        return Err(UpdateError::unsupported(format!(
            "wright appears to be managed by {} (installed at {}); upgrade with `{}` instead of `wright update`",
            channel,
            exe.display(),
            guidance
        )));
    }

    let install_dir = exe.parent().ok_or_else(|| {
        UpdateError::failed(format!(
            "could not determine the installation directory of {}",
            exe.display()
        ))
    })?;
    if !install_dir.join(platform.exe("wright-lsp")).is_file() {
        return Err(UpdateError::failed(format!(
            "wright-lsp was not found next to wright at {}; this installation was not created by the official installer — reinstall with `{}` to restore a matched pair",
            install_dir.display(),
            platform.installer()
        )));
    }

    let current = env!("CARGO_PKG_VERSION").to_string();
    let (target_version, client) = match requested {
        Some(v) => (v.to_string(), None),
        None => {
            let client = update_client()?;
            let version = resolve_latest(&client, &env_base_url())?;
            (version, Some(client))
        }
    };

    let current_version = parse_version(&current)?;
    let parsed_target = parse_version(&target_version)?;
    let target_version = target_version.trim_start_matches('v');
    match current_version.cmp(&parsed_target) {
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
        let hint = match requested {
            Some(_) => format!("wright update self --version {target_version}"),
            None => "wright update".to_string(),
        };
        println!("update available: {current} -> {target_version} (run `{hint}` to install)");
        return Ok(exit::SUCCESS);
    }

    println!("==> installing wright {current} -> {target_version}");
    let client = match client {
        Some(c) => c,
        None => update_client()?,
    };
    install::install_version(
        &client,
        target_version,
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
    let target = match (os, arch) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("darwin", "x86_64") => "x86_64-apple-darwin",
        ("darwin", "aarch64") => "aarch64-apple-darwin",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        _ => {
            return Err(UpdateError::unsupported(format!(
                "unsupported platform {os}/{arch} (supported: linux/x86_64, darwin/x86_64, darwin/aarch64, windows/x86_64); use the official installer (curl -fsSL https://install.wrightkit.dev/wright/install.sh | bash) or a package manager"
            )));
        }
    };
    Ok(Platform {
        target,
        windows: os == "windows",
    })
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
    let path = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
    let is_component = |name: &str| {
        path.to_string_lossy()
            .to_lowercase()
            .split(['/', '\\'])
            .any(|component| component == name)
    };
    if is_component("homebrew") || is_component("cellar") {
        Provenance::Homebrew
    } else if is_component("scoop") {
        Provenance::Scoop
    } else if is_component("winget") {
        Provenance::WinGet
    } else {
        Provenance::Standalone
    }
}

fn resolve_latest(
    client: &reqwest::blocking::Client,
    base_url: &str,
) -> Result<String, UpdateError> {
    let url = format!("{}/latest/version", base_url.trim_end_matches('/'));
    let body = fetch_text(client, &url)?;
    parse_latest_version(&body).ok_or_else(|| {
        UpdateError::failed(format!(
            "could not parse the latest release version from {url} (got '{}'); pin a version with `wright update self --version`",
            truncate(body.trim())
        ))
    })
}

fn parse_latest_version(body: &str) -> Option<String> {
    let version = body.trim().trim_start_matches('v');
    parse_version(version).ok()?;
    Some(version.to_string())
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
    let response = client.get(url).send().map_err(|e| UpdateError::failed(format!("could not download {url} ({e}); check the network connection or pin a version with `wright update self --version`")))?;
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct Version {
    core: (u64, u64, u64),
    prerelease: Vec<PrereleaseIdent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PrereleaseIdent {
    Numeric(u64),
    Text(String),
}

impl Ord for PrereleaseIdent {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Numeric(a), Self::Numeric(b)) => a.cmp(b),
            (Self::Numeric(_), Self::Text(_)) => Ordering::Less,
            (Self::Text(_), Self::Numeric(_)) => Ordering::Greater,
            (Self::Text(a), Self::Text(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for PrereleaseIdent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core.cmp(&other.core).then_with(|| {
            match (self.prerelease.is_empty(), other.prerelease.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self.prerelease.cmp(&other.prerelease),
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn parse_version(version: &str) -> Result<Version, UpdateError> {
    parse_version_str(version).ok_or_else(|| {
        UpdateError::Usage(format!(
            "invalid version '{version}' (expected semver like 0.1.0)"
        ))
    })
}

fn parse_version_str(version: &str) -> Option<Version> {
    let rest = version.strip_prefix('v').unwrap_or(version);
    let (core, suffix) = match rest.find(['-', '+']) {
        Some(index) => rest.split_at(index),
        None => (rest, ""),
    };
    let mut parts = core.split('.');
    let (Some(major), Some(minor), Some(patch), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let core = (
        major.parse::<u64>().ok()?,
        minor.parse::<u64>().ok()?,
        patch.parse::<u64>().ok()?,
    );
    let prerelease = match suffix.as_bytes().first() {
        None => Vec::new(),
        Some(b'+') => Vec::new(),
        Some(b'-') => suffix[1..]
            .split('.')
            .map(|ident| {
                ident
                    .parse::<u64>()
                    .map(PrereleaseIdent::Numeric)
                    .unwrap_or_else(|_| PrereleaseIdent::Text(ident.to_string()))
            })
            .collect(),
        _ => return None,
    };
    if !suffix.is_empty()
        && (suffix[1..].is_empty()
            || !suffix[1..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'))
    {
        return None;
    }
    Some(Version { core, prerelease })
}

fn compare_versions(a: &str, b: &str) -> Option<Ordering> {
    Some(parse_version_str(a)?.cmp(&parse_version_str(b)?))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_accepts_stable_semver_and_leading_v() {
        assert_eq!(parse_version("0.1.0").unwrap().core, (0, 1, 0));
        assert_eq!(parse_version("v0.2.10").unwrap().core, (0, 2, 10));
        assert_eq!(parse_version("9.9.9").unwrap().core, (9, 9, 9));
    }

    #[test]
    fn parse_version_accepts_the_suffixes_install_sh_accepts() {
        for good in [
            "0.1.0-beta.1",
            "v0.1.0-rc",
            "0.1.0+build.5",
            "1.2.3-nightly",
        ] {
            assert!(
                parse_version(good).is_ok(),
                "{good:?} is accepted by install.sh"
            );
        }
        let version = parse_version("0.1.0-beta.1").unwrap();
        assert_eq!(
            version.prerelease,
            vec![
                PrereleaseIdent::Text("beta".to_string()),
                PrereleaseIdent::Numeric(1)
            ]
        );
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
            "",
            "0.1.0-rc+build",
            "0.1.0-",
            "0.1.0+",
            "0.1.0-rc_1",
        ] {
            assert!(parse_version(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn latest_version_pointer_is_plain_text() {
        assert_eq!(parse_latest_version("9.9.9\n"), Some("9.9.9".to_string()));
        assert_eq!(
            parse_latest_version("  v0.2.10 \n"),
            Some("0.2.10".to_string())
        );
        for bad in ["", "latest", "1.2", "{\"tag_name\":\"v9.9.9\"}", "0.1\n0.2"] {
            assert_eq!(parse_latest_version(bad), None, "{bad:?} must be rejected");
        }
    }

    #[test]
    fn compare_versions_orders_numerically() {
        use std::cmp::Ordering::*;
        assert_eq!(compare_versions("0.1.0", "0.1.0"), Some(Equal));
        assert_eq!(compare_versions("0.2.0", "0.1.0"), Some(Greater));
        assert_eq!(compare_versions("0.1.10", "0.1.9"), Some(Greater));
        assert_eq!(compare_versions("0.2.0", "0.10.0"), Some(Less));
        assert_eq!(compare_versions("1.0.0", "0.9.9"), Some(Greater));
    }

    #[test]
    fn compare_versions_orders_prereleases_by_semver_precedence() {
        use std::cmp::Ordering::*;
        assert_eq!(compare_versions("0.2.0-beta.1", "0.2.0"), Some(Less));
        assert_eq!(compare_versions("0.2.0", "0.2.0-rc.1"), Some(Greater));
        assert_eq!(compare_versions("0.2.0-alpha", "0.2.0-alpha.1"), Some(Less));
        assert_eq!(
            compare_versions("0.2.0-alpha.1", "0.2.0-alpha"),
            Some(Greater)
        );
        assert_eq!(
            compare_versions("0.2.0-beta.2", "0.2.0-beta.10"),
            Some(Less)
        );
        assert_eq!(compare_versions("0.2.0+build.1", "0.2.0"), Some(Equal));
        assert_eq!(
            compare_versions("0.2.0+build.1", "0.2.0+build.2"),
            Some(Equal)
        );
        assert_eq!(compare_versions("nightly", "0.2.0"), None);
        assert_eq!(compare_versions("0.2.0", ""), None);
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
    fn windows_maps_to_the_zip_release() {
        let platform = detect_platform_for("windows", "x86_64").unwrap();
        assert_eq!(platform.target, "x86_64-pc-windows-msvc");
        assert_eq!(
            platform.archive_name("9.9.9"),
            "wright-9.9.9-x86_64-pc-windows-msvc.zip"
        );
        assert_eq!(platform.exe("wright-lsp"), "wright-lsp.exe");
        assert!(platform.installer().contains("install.ps1"));
    }

    #[test]
    fn unknown_platforms_are_refused() {
        for (os, arch) in [
            ("linux", "arm64"),
            ("windows", "aarch64"),
            ("freebsd", "x86_64"),
        ] {
            let error = detect_platform_for(os, arch).unwrap_err();
            assert_eq!(error.exit_code(), exit::UNSUPPORTED);
        }
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
        assert_eq!(
            provenance_of("/c/Users/me/scooped/apps/wright/wright.exe"),
            Provenance::Standalone
        );
        assert_eq!(
            provenance_of("/opt/mywinget/bin/wright"),
            Provenance::Standalone
        );
        assert_eq!(
            provenance_of("/Users/me/homebrewed/wright"),
            Provenance::Standalone
        );
    }

    fn provenance_of(path: &str) -> Provenance {
        detect_provenance(Path::new(path))
    }
}
