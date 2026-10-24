mod remote;
mod rules;
mod source;

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use remote::ResolutionCache;
pub use source::LintPackageSource;

const DEFAULT_API_BASE: &str = "https://api.github.com";
const DEFAULT_CODELOAD_BASE: &str = "https://codeload.github.com";
const STORE_DIR_ENV: &str = "WRIGHT_LINT_PACKAGE_DIR";

/// The first-party lint rule package repository (`owner/repo`), matched
/// case-insensitively — the only package source allowed bare rule IDs; a
/// project entry for it replaces the embedded default rules it covers
/// instead of colliding (issue #843). The repository name is provisional:
/// choosing and creating the first-party package repository is an owner
/// action tracked by wrightkit/wright#845.
pub const FIRST_PARTY_PACKAGE: &str = "wrightkit/lint-rules";

/// One store entry directory discovered under `<root>/<owner>/<repo>/<sha>/`.
/// The directory names carry the source identity and the immutable revision;
/// an intact entry validates as a package, a `damaged` entry carries the
/// reason it does not (`lint-package-install`).
#[derive(Debug)]
pub struct StoreEntry {
    /// The package source, from the `<owner>/<repo>` directory names.
    pub source: LintPackageSource,
    /// The full commit SHA the entry is keyed by.
    pub sha: String,
    /// The entry directory `<root>/<owner>/<repo>/<sha>/`.
    pub dir: PathBuf,
    /// The validated SPEC-309 rule IDs of an intact entry; empty for a
    /// damaged one.
    pub rules: Vec<String>,
    /// Why the entry does not validate as a package, when it is damaged —
    /// missing `rules/`, no rule files, or locally edited managed content
    /// that no longer loads.
    pub damage: Option<LintPackageError>,
}

/// The outcome of `LintPackageStore::remove`: the entry's managed `rules/`
/// tree is gone, and `dir_removed` reports whether the entry directory was
/// deleted too — it survives when unrelated, non-managed files remain.
#[derive(Debug)]
pub struct RemovedPackage {
    /// The package source.
    pub source: LintPackageSource,
    /// The removed entry's full commit SHA.
    pub sha: String,
    /// The entry directory.
    pub dir: PathBuf,
    /// Whether the entry directory was removed. `false` means unrelated
    /// files beside `rules/` keep it — managed content is still gone.
    pub dir_removed: bool,
}

/// One repository found by `LintPackageStore::search` on the
/// `wrightkit-lint` topic.
#[derive(Debug)]
pub struct PackageSearchHit {
    /// The `owner/repo` source identity.
    pub source: LintPackageSource,
    /// The repository description, when GitHub returns one.
    pub description: Option<String>,
}

/// The paged result of a `topic:wrightkit-lint` repository search.
#[derive(Debug)]
pub struct PackageSearch {
    /// The `total_count` GitHub reported across all pages.
    pub total: u64,
    /// Whether GitHub reported incomplete results.
    pub incomplete: bool,
    /// The repositories found, in API order.
    pub hits: Vec<PackageSearchHit>,
}

/// One installed package directory in the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackage {
    /// The package source.
    pub source: LintPackageSource,
    /// The full commit SHA this store entry is keyed by; only the SHA is
    /// stored, never the ref it was resolved from.
    pub sha: String,
    /// The store directory `<root>/<owner>/<repo>/<full-sha>/`.
    pub dir: PathBuf,
    /// The validated SPEC-309 rule IDs the package provides, in load order.
    pub rules: Vec<String>,
    /// Whether this call downloaded the archive. A second fetch of an intact
    /// store entry makes no download.
    pub downloaded: bool,
}

impl InstalledPackage {
    /// The `rules/` directory a lint registry loads with `load_path`.
    pub fn rules_dir(&self) -> PathBuf {
        self.dir.join("rules")
    }
}

/// The state of one read-only remote check against an installed SHA.
#[derive(Debug)]
pub enum RemoteCheck {
    /// The remote ref resolves to the installed SHA (including an ETag 304).
    Unchanged { sha: String },
    /// The remote ref resolves to a different SHA than installed.
    Updated { sha: String },
    /// The remote answered that the repository or ref is not there (or is
    /// private without a usable token) — the package source is gone.
    Unavailable { reason: String },
    /// The check itself could not be completed: network, rate limit, or an
    /// unexpected answer.
    Error(LintPackageError),
}

impl RemoteCheck {
    /// The stable status word for records and presentation.
    pub fn status(&self) -> &'static str {
        match self {
            Self::Unchanged { .. } => "unchanged",
            Self::Updated { .. } => "updated",
            Self::Unavailable { .. } => "unavailable",
            Self::Error(_) => "error",
        }
    }
}

/// A Wright-owned lint package store rooted at `<root>/<owner>/<repo>/<sha>/`.
#[derive(Clone)]
pub struct LintPackageStore {
    root: PathBuf,
    api_base: String,
    codeload_base: String,
    token: Option<String>,
    resolved: Arc<ResolutionCache>,
}

impl LintPackageStore {
    /// A store under `root`. `GITHUB_TOKEN`, when set, is sent to the API
    /// host only.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            api_base: DEFAULT_API_BASE.to_string(),
            codeload_base: DEFAULT_CODELOAD_BASE.to_string(),
            token: std::env::var(remote::TOKEN_ENV)
                .ok()
                .filter(|t| !t.is_empty()),
            resolved: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// The platform data directory holding lint packages, honoring
    /// `WRIGHT_LINT_PACKAGE_DIR`. Mirrors the provider data dir convention.
    pub fn default_root() -> PathBuf {
        default_store_dir()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Override the GitHub API base (testing or a GitHub Enterprise host).
    pub fn with_api_base(mut self, base: impl Into<String>) -> Self {
        self.api_base = base.into();
        self
    }

    /// Override the codeload tarball base (testing).
    pub fn with_codeload_base(mut self, base: impl Into<String>) -> Self {
        self.codeload_base = base.into();
        self
    }

    /// Override the token read from `GITHUB_TOKEN`; `None` disables it.
    pub fn with_token(mut self, token: Option<String>) -> Self {
        self.token = token.filter(|t| !t.is_empty());
        self
    }

    /// `<root>/<owner>/<repo>/<full-sha>/` — the immutable package directory.
    pub fn package_dir(&self, source: &LintPackageSource, sha: &str) -> PathBuf {
        self.root
            .join(source.owner())
            .join(source.repo())
            .join(sha.to_lowercase())
    }

    /// The stored package directory for `sha`, when it exists intact.
    pub fn installed(&self, source: &LintPackageSource, sha: &str) -> Option<PathBuf> {
        let dir = self.package_dir(source, sha);
        rules::intact_package(&dir).then_some(dir)
    }

    /// Resolve `reference` — a tag, branch, or short SHA; `None` means the
    /// repository's default-branch head — to a full commit SHA through the
    /// commits API. A full 40-hex SHA needs no resolution. Conditional
    /// requests carry the stored ETag so an unchanged ref answers 304 without
    /// another counted request.
    pub fn resolve(
        &self,
        source: &LintPackageSource,
        reference: Option<&str>,
    ) -> Result<String, LintPackageError> {
        let reference = normalize_ref(reference);
        if let Some(sha) = reference.filter(|r| is_full_sha(r)) {
            return Ok(sha.to_lowercase());
        }
        let client = remote::package_client()?;
        let (url, single) = match reference {
            Some(reference) => (remote::resolve_url(&self.api_base, source, reference), true),
            None => (remote::default_head_url(&self.api_base, source), false),
        };
        let body = remote::api_get(&client, &url, self.token.as_deref(), &self.resolved)?;
        remote::resolved_sha(&body, single, source)
    }

    /// Fetch `source` at `reference` into the store. Resolution happens
    /// first, so only the full SHA is stored; an intact `<sha>/` directory is
    /// reused with no download. A fetched package is validated against the
    /// SPEC-309 loader before promotion, so a refused package leaves no
    /// partial store entry.
    pub fn install(
        &self,
        source: &LintPackageSource,
        reference: Option<&str>,
    ) -> Result<InstalledPackage, LintPackageError> {
        let sha = self.resolve(source, reference)?;
        let dir = self.package_dir(source, &sha);
        if dir.exists() {
            let rules = rules::validate_stored(&dir, source.is_first_party())?;
            return Ok(InstalledPackage {
                source: source.clone(),
                sha,
                dir,
                rules,
                downloaded: false,
            });
        }
        let archive = {
            let client = remote::package_client()?;
            let url = remote::archive_url(&self.codeload_base, source, &sha);
            remote::fetch_archive(&client, &url, source, &sha, self.token.is_some())?
        };
        let staging = self.staging_dir(&sha)?;
        let result = (|| -> Result<Vec<String>, LintPackageError> {
            rules::extract_rules(&archive, &staging)?;
            let validated =
                rules::validate_package(&staging.join("rules"), source.is_first_party())?;
            self.promote(&staging, &dir)?;
            Ok(validated)
        })();
        let _ = std::fs::remove_dir_all(&staging);
        let validated = result?;
        Ok(InstalledPackage {
            source: source.clone(),
            sha,
            dir,
            rules: validated,
            downloaded: true,
        })
    }

    /// Every store entry — `<root>/<owner>/<repo>/<full-sha>/` directories
    /// still carrying a `rules/` tree — in stable `owner/repo/sha` order.
    /// Damaged entries (an unloadable or empty `rules/`) are included and
    /// carry their refusal reason in `damage`; directories that cannot be
    /// package entries (invalid names, non-SHA names, leftover staging, or
    /// an entry dir whose `rules/` is already gone) are skipped. A missing
    /// store root reports an empty store.
    pub fn entries(&self) -> Result<Vec<StoreEntry>, LintPackageError> {
        let read = |dir: &Path| -> Result<Vec<PathBuf>, LintPackageError> {
            match std::fs::read_dir(dir) {
                Ok(d) => Ok(d
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
                Err(e) => Err(LintPackageError::install(format!(
                    "cannot read the lint package store '{}': {e}",
                    dir.display()
                ))),
            }
        };
        let mut entries = Vec::new();
        for owner in read(&self.root)? {
            let Some(owner_name) = owner.file_name().map(|n| n.to_string_lossy()) else {
                continue;
            };
            for repo in read(&owner)? {
                let Some(repo_name) = repo.file_name().map(|n| n.to_string_lossy()) else {
                    continue;
                };
                let Ok(source) = LintPackageSource::new(&owner_name, &repo_name) else {
                    continue;
                };
                for dir in read(&repo)? {
                    let Some(sha) = dir.file_name().map(|n| n.to_string_lossy()) else {
                        continue;
                    };
                    if !is_full_sha(&sha) || !dir.join("rules").is_dir() {
                        continue;
                    }
                    let sha = sha.to_lowercase();
                    let (rules, damage) =
                        match rules::validate_stored(&dir, source.is_first_party()) {
                            Ok(rules) => (rules, None),
                            Err(error) => (Vec::new(), Some(error)),
                        };
                    entries.push(StoreEntry {
                        source: source.clone(),
                        sha,
                        dir,
                        rules,
                        damage,
                    });
                }
            }
        }
        entries.sort_by(|a, b| {
            a.source
                .slug()
                .cmp(&b.source.slug())
                .then_with(|| a.sha.cmp(&b.sha))
        });
        Ok(entries)
    }

    /// Remove one store entry's managed content: its `rules/` tree, then
    /// the entry directory itself only when nothing else remains — a
    /// user-owned file beside `rules/` keeps the directory. Empty
    /// `<owner>/<repo>` parent directories are pruned.
    pub fn remove(
        &self,
        source: &LintPackageSource,
        sha: &str,
    ) -> Result<RemovedPackage, LintPackageError> {
        let dir = self.package_dir(source, sha);
        if !dir.is_dir() {
            return Err(LintPackageError::NotFound(format!(
                "lint package '{source}' has no store entry at commit {sha}"
            )));
        }
        let rules_dir = dir.join("rules");
        if rules_dir.is_dir() {
            std::fs::remove_dir_all(&rules_dir).map_err(|e| {
                LintPackageError::install(format!(
                    "cannot remove managed lint rules '{}': {e}",
                    rules_dir.display()
                ))
            })?;
        }
        let remaining = std::fs::read_dir(&dir)
            .map_err(|e| {
                LintPackageError::install(format!(
                    "cannot inspect lint package entry '{}': {e}",
                    dir.display()
                ))
            })?
            .next();
        let dir_removed = remaining.is_none();
        if dir_removed {
            std::fs::remove_dir(&dir).map_err(|e| {
                LintPackageError::install(format!(
                    "cannot remove lint package entry '{}': {e}",
                    dir.display()
                ))
            })?;
            let mut parent = dir.parent();
            while let Some(path) = parent {
                if path == self.root || std::fs::remove_dir(path).is_err() {
                    break;
                }
                parent = path.parent();
            }
        }
        Ok(RemovedPackage {
            source: source.clone(),
            sha: sha.to_lowercase(),
            dir,
            dir_removed,
        })
    }

    /// Search GitHub for lint rule packages: the
    /// `topic:wrightkit-lint archived:false fork:false` repository query,
    /// paged at `per_page=100`. `terms`, when given, narrow the query
    /// further. Repositories whose `full_name` is not a valid package
    /// source are skipped.
    pub fn search(&self, terms: Option<&str>) -> Result<PackageSearch, LintPackageError> {
        let client = remote::package_client()?;
        let mut hits = Vec::new();
        let mut total = 0u64;
        let mut incomplete = false;
        for page in 1..=10u32 {
            let url = remote::search_url(&self.api_base, terms, page);
            let body = remote::api_get(&client, &url, self.token.as_deref(), &self.resolved)?;
            let page_data = remote::search_page(&body)?;
            total = page_data.total;
            incomplete |= page_data.incomplete;
            let count = page_data.items.len();
            for item in page_data.items {
                let Some(full_name) = item.get("full_name").and_then(|v| v.as_str()) else {
                    continue;
                };
                let Some((owner, repo)) = full_name.split_once('/') else {
                    continue;
                };
                let Ok(source) = LintPackageSource::new(owner, repo) else {
                    continue;
                };
                hits.push(PackageSearchHit {
                    source,
                    description: item
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                });
            }
            if count < 100 || hits.len() as u64 >= total {
                break;
            }
        }
        Ok(PackageSearch {
            total,
            incomplete,
            hits,
        })
    }

    /// Read-only remote check: whether `reference` still resolves to
    /// `installed_sha`. Applies nothing to the store.
    pub fn check_remote(
        &self,
        source: &LintPackageSource,
        reference: Option<&str>,
        installed_sha: &str,
    ) -> RemoteCheck {
        match self.resolve(source, reference) {
            Ok(sha) if sha.eq_ignore_ascii_case(installed_sha) => RemoteCheck::Unchanged { sha },
            Ok(sha) => RemoteCheck::Updated { sha },
            Err(LintPackageError::NotFound(reason)) => RemoteCheck::Unavailable { reason },
            Err(error) => RemoteCheck::Error(error),
        }
    }

    fn staging_dir(&self, sha: &str) -> Result<PathBuf, LintPackageError> {
        std::fs::create_dir_all(&self.root).map_err(|e| {
            LintPackageError::install(format!(
                "cannot create the lint package store '{}': {e}",
                self.root.display()
            ))
        })?;
        crate::store::reserve_staging_dir(
            &self.root,
            &format!(".lint-package-{sha}"),
            "a lint package",
        )
        .map_err(LintPackageError::install)
    }

    fn promote(&self, staging: &Path, final_dir: &Path) -> Result<(), LintPackageError> {
        crate::store::promote_dir(staging, final_dir, || rules::intact_package(final_dir)).map_err(
            |error| {
                LintPackageError::install(match error {
                    crate::store::PromoteError::CreateParent(error) => format!(
                        "cannot create lint package directory '{}': {error}",
                        final_dir.display()
                    ),
                    crate::store::PromoteError::Rename(error) => format!(
                        "cannot promote lint package directory '{}': {error}",
                        final_dir.display()
                    ),
                })
            },
        )
    }
}

pub(crate) fn activate_pinned(
    lint: &wright_analyzer::registry::LintConfig,
    packages: &crate::config::LintPackageConfig,
    registry: &mut wright_analyzer::registry::LintRegistry,
) -> Result<(), crate::diag::Diagnostic> {
    use crate::diag::{Diagnostic, Stage};
    if lint.packages.is_empty() {
        return Ok(());
    }
    let pin_error = |message: String| {
        let error = LintPackageError::source(message);
        Diagnostic::error(error.code(), Stage::Discovery, error.to_string())
    };
    let store = packages.store();
    for pin in &lint.packages {
        let (source, reference) = LintPackageSource::parse(&pin.repo).map_err(|error| {
            Diagnostic::error(
                error.code(),
                Stage::Discovery,
                format!("lint package pin '{}': {error}", pin.repo),
            )
        })?;
        if let Some(reference) = reference {
            return Err(pin_error(format!(
                "lint package pin '{}' carries the ref '{reference}'; pin the commit in `sha`",
                pin.repo
            )));
        }
        if !is_full_sha(&pin.sha) {
            return Err(pin_error(format!(
                "lint package '{source}' must be pinned by a full 40-hex commit SHA, got '{}'",
                pin.sha
            )));
        }
        let installed = store.install(&source, Some(&pin.sha)).map_err(|error| {
            let stage = match error {
                LintPackageError::Source(_) => Stage::Discovery,
                LintPackageError::Invalid { .. } => Stage::Analysis,
                _ => Stage::Internal,
            };
            Diagnostic::error(
                error.code(),
                stage,
                format!("lint package '{source}' at {}: {error}", pin.sha),
            )
        })?;
        let rules_dir = installed.rules_dir();
        let loaded = if source.is_first_party() {
            registry.load_first_party_path(&rules_dir)
        } else {
            registry.load_path(&rules_dir)
        };
        loaded.map_err(|error| {
            Diagnostic::error(
                "lint-rule-error",
                Stage::Analysis,
                format!("lint package '{source}' at {}: {error}", pin.sha),
            )
        })?;
    }
    Ok(())
}

fn normalize_ref(reference: Option<&str>) -> Option<&str> {
    reference.map(str::trim).filter(|r| !r.is_empty())
}

pub(crate) fn is_full_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn default_store_dir() -> PathBuf {
    if let Some(path) = std::env::var_os(STORE_DIR_ENV) {
        return PathBuf::from(path);
    }
    crate::store::wright_data_base().join("lint-packages")
}

/// Every failure mode of a lint package fetch, store, or remote check.
#[derive(Debug)]
pub enum LintPackageError {
    /// The repository/ref spelling itself is invalid — a usage error.
    Source(String),
    /// The remote could not be reached at all.
    Offline(String),
    /// The repository, ref, or SHA does not exist (or is private without a
    /// usable token).
    NotFound(String),
    /// The GitHub API rate limit is used up.
    RateLimited(String),
    /// Any other non-success HTTP status or malformed API answer.
    Remote(String),
    /// The fetched content is not a valid SPEC-309 rule package; the failing
    /// rule file (or the package `rules/` directory for an empty package) is
    /// named.
    Invalid { path: PathBuf, message: String },
    /// The local store could not be written or is damaged.
    Install(String),
}

impl LintPackageError {
    fn source(msg: impl Into<String>) -> Self {
        Self::Source(msg.into())
    }
    fn offline(msg: impl Into<String>) -> Self {
        Self::Offline(msg.into())
    }
    fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }
    fn rate_limited(msg: impl Into<String>) -> Self {
        Self::RateLimited(msg.into())
    }
    fn remote(msg: impl Into<String>) -> Self {
        Self::Remote(msg.into())
    }
    fn install(msg: impl Into<String>) -> Self {
        Self::Install(msg.into())
    }
    fn invalid(path: impl Into<PathBuf>, message: impl fmt::Display) -> Self {
        Self::Invalid {
            path: path.into(),
            message: message.to_string(),
        }
    }

    /// The stable machine-readable code for the failure class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Source(_) => "lint-package-source",
            Self::Offline(_) => "lint-package-offline",
            Self::NotFound(_) => "lint-package-not-found",
            Self::RateLimited(_) => "lint-package-rate-limited",
            Self::Remote(_) => "lint-package-remote",
            Self::Invalid { .. } => "lint-package-invalid",
            Self::Install(_) => "lint-package-install",
        }
    }

    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Source(_) => crate::result::exit::USAGE,
            Self::Invalid { .. } => crate::result::exit::SOURCE_ERROR,
            _ => crate::result::exit::INTERNAL,
        }
    }
}

impl fmt::Display for LintPackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(m)
            | Self::Offline(m)
            | Self::NotFound(m)
            | Self::RateLimited(m)
            | Self::Remote(m)
            | Self::Install(m) => write!(f, "{m}"),
            Self::Invalid { path, message } => write!(f, "{}: {message}", path.display()),
        }
    }
}

impl std::error::Error for LintPackageError {}
