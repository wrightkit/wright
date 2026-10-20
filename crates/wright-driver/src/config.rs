use std::path::PathBuf;

use crate::lint_package::LintPackageStore;
use crate::source_provider::SourceBackend;
pub use wright_analyzer::registry::{LintConfig, LintPackagePin};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SourceKind {
    #[default]
    Auto,
    Opy,
    Ostw,
    Workshop,
    Protocol,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Opy => "opy",
            Self::Ostw => "ostw",
            Self::Workshop => "workshop",
            Self::Protocol => "protocol",
        }
    }

    pub fn parse(name: &str) -> Option<SourceKind> {
        match name {
            "auto" => Some(Self::Auto),
            "opy" => Some(Self::Opy),
            "ostw" => Some(Self::Ostw),
            "workshop" | "ws" => Some(Self::Workshop),
            "protocol" | "hir" | "json" => Some(Self::Protocol),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

impl OutputFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
        }
    }

    pub fn parse(name: &str) -> Option<OutputFormat> {
        match name {
            "text" | "human" => Some(Self::Text),
            "json" | "machine" => Some(Self::Json),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum InputSpec {
    Path(PathBuf),
    /// Caller-held text resolved without a disk read (#555): `path` gives
    /// the input its file identity — kind detection, root resolution, and
    /// span paths behave exactly like `Path` — while `text` replaces the
    /// file's bytes. `None` is an anonymous document with no filesystem
    /// identity.
    Text {
        text: String,
        path: Option<PathBuf>,
    },
    #[default]
    Stdin,
}

impl InputSpec {
    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            Self::Path(path) => Some(path),
            Self::Text { path, .. } => path.as_ref(),
            Self::Stdin => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionConfig {
    pub input: InputSpec,
    pub kind: SourceKind,
    pub source_backend: SourceBackend,
    pub locale: Option<String>,
    pub root: Option<PathBuf>,
    pub format: OutputFormat,
    pub profile: wright_transform::Profile,
    pub lint: LintConfig,
    /// Lint rule package store/fetch overrides (issue #843). The default is
    /// the platform store (`WRIGHT_LINT_PACKAGE_DIR` or the OS data dir)
    /// talking to github.com; the endpoint fields exist for tests and
    /// GitHub Enterprise deployments.
    pub lint_package: LintPackageConfig,
    pub lint_rule_paths: Vec<PathBuf>,
    /// Finding/diagnostic selection applied to reported output (#430).
    /// Selection narrows presentation only; verdicts and exit codes are
    /// computed on the full set.
    pub selection: crate::select::FindingSelection,
    /// The project run cache and cost-comparison options (ADR-0027, #723).
    /// Comparison is reported on `compile`/`inspect cost` when a reference
    /// exists; `--max-total` is an explicit gate that reads no cache.
    pub run: RunOptions,
    pub providers: wright_lpp::ProviderRegistry,
    pub opy_provider: crate::opy_provider::OpyProviderConfig,
}

/// Run-cache and cost-gate options (ADR-0027). Disabled entirely under
/// `--no-cache` or a non-empty `WRIGHT_NO_CACHE`. Under a truthy `CI` only
/// the explicit baseline flags read and write `.wright/` — the implicit
/// previous-run comparison stays off and an unusable `--baseline` degrades
/// silently (ADR-0030 decision 14).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunOptions {
    /// `--no-cache`: neither read nor write `.wright/`.
    pub no_cache: bool,
    /// `--baseline <name>`: compare against the named reference instead of
    /// the previous run. An unknown name given explicitly is a usage error
    /// outside CI; under CI it turns the comparison off silently.
    pub baseline: Option<String>,
    /// `--save-baseline <name>`: record the current run under `name`.
    pub save_baseline: Option<String>,
    /// `--max-total <N>`: `inspect cost` fails with exit 1 when the element
    /// total exceeds N and names the overage. It reads no cache, so it
    /// behaves identically with and without history.
    pub max_total: Option<usize>,
}

/// Lint rule package store and fetch endpoints (issue #843). Every field
/// defaults to the production behavior; overrides exist for tests and
/// GitHub Enterprise deployments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LintPackageConfig {
    /// Store root override — defaults to [`LintPackageStore::default_root`],
    /// which honors `WRIGHT_LINT_PACKAGE_DIR`.
    pub store_dir: Option<PathBuf>,
    /// GitHub API base URL override.
    pub api_base: Option<String>,
    /// codeload tarball base URL override.
    pub codeload_base: Option<String>,
}

impl LintPackageConfig {
    /// The store this configuration describes: default endpoints unless a
    /// field overrides one.
    pub(crate) fn store(&self) -> LintPackageStore {
        let mut store = LintPackageStore::new(
            self.store_dir
                .clone()
                .unwrap_or_else(LintPackageStore::default_root),
        );
        if let Some(base) = &self.api_base {
            store = store.with_api_base(base.clone());
        }
        if let Some(base) = &self.codeload_base {
            store = store.with_codeload_base(base.clone());
        }
        store
    }
}

impl SessionConfig {
    pub fn from_path(path: impl Into<PathBuf>) -> SessionConfig {
        SessionConfig {
            input: InputSpec::Path(path.into()),
            ..SessionConfig::default()
        }
    }

    /// The fields that differ from `other`, named for diagnostics (#511);
    /// an empty list means the two configurations are identical.
    pub(crate) fn changed_fields(&self, other: &SessionConfig) -> Vec<&'static str> {
        let mut changed = Vec::new();
        if self.input != other.input {
            changed.push("input");
        }
        if self.kind != other.kind {
            changed.push("kind");
        }
        if self.source_backend != other.source_backend {
            changed.push("source_backend");
        }
        if self.locale != other.locale {
            changed.push("locale");
        }
        if self.root != other.root {
            changed.push("root");
        }
        if self.format != other.format {
            changed.push("format");
        }
        if self.profile != other.profile {
            changed.push("profile");
        }
        if self.lint != other.lint {
            changed.push("lint");
        }
        if self.lint_package != other.lint_package {
            changed.push("lint_package");
        }
        if self.lint_rule_paths != other.lint_rule_paths {
            changed.push("lint_rule_paths");
        }
        if self.selection != other.selection {
            changed.push("selection");
        }
        if self.run != other.run {
            changed.push("run");
        }
        if self.providers != other.providers {
            changed.push("providers");
        }
        if self.opy_provider != other.opy_provider {
            changed.push("opy_provider");
        }
        changed
    }
}
