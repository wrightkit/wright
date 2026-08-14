//! Session configuration owned by the compiler driver.
//!
//! [`SessionConfig`] describes one driver run: where the input comes from,
//! which frontend handles it, optional frontend overrides (Workshop locale,
//! `.opy` include root), where compiled output goes, which presentation
//! format the result is intended for, and the lint rule configuration. The
//! CLI, library consumers, and later tool/LSP adapters all construct the
//! same configuration type.

use std::path::PathBuf;

pub use wright_analyzer::registry::LintConfig;

/// The concrete input frontend to use, or automatic detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// Detect from the input path extension or stdin content.
    Auto,
    /// `.opy` source through the adapter bridge (native frontend lands in M7).
    Opy,
    /// Localized vanilla Workshop text (native frontend).
    Workshop,
    /// An Opy HIR v1 protocol payload (JSON).
    Protocol,
}

impl SourceKind {
    /// The canonical name used in CLI arguments, docs, and diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Auto => "auto",
            SourceKind::Opy => "opy",
            SourceKind::Workshop => "workshop",
            SourceKind::Protocol => "protocol",
        }
    }

    /// Parse a CLI spelling into a source kind (`None` for unknown names).
    pub fn parse(name: &str) -> Option<SourceKind> {
        Some(match name {
            "auto" => SourceKind::Auto,
            "opy" => SourceKind::Opy,
            "workshop" | "ws" => SourceKind::Workshop,
            "protocol" | "hir" | "json" => SourceKind::Protocol,
            _ => return None,
        })
    }
}

/// The machine-readable (`json`) or human-readable (`text`) result mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
}

impl OutputFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            OutputFormat::Text => "text",
            OutputFormat::Json => "json",
        }
    }

    pub fn parse(name: &str) -> Option<OutputFormat> {
        Some(match name {
            "text" | "human" => OutputFormat::Text,
            "json" | "machine" => OutputFormat::Json,
            _ => return None,
        })
    }
}

/// Where the driver reads its input from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputSpec {
    /// A path on disk.
    Path(PathBuf),
    /// Standard input.
    Stdin,
}

impl InputSpec {
    /// The path when this spec names a file, otherwise `None` (stdin).
    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            InputSpec::Path(path) => Some(path),
            InputSpec::Stdin => None,
        }
    }
}

/// One driver run's configuration.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// The input source (file path or stdin).
    pub input: InputSpec,
    /// Frontend selection; `Auto` detects from path/stdin.
    pub kind: SourceKind,
    /// Workshop client-locale override (bypasses auto-detection).
    pub locale: Option<String>,
    /// Include root for `.opy` inputs (defaults to the input's directory).
    pub root: Option<PathBuf>,
    /// Compiled output destination for `compile` (defaults to stdout).
    pub output: Option<PathBuf>,
    /// The requested result presentation format.
    pub format: OutputFormat,
    /// The WIR transformation policy (`off` by default; `compat`/`aggressive`
    /// opt into evidence-backed passes).
    pub profile: wright_transform::Profile,
    /// The lint rule configuration used by `lint` (M12, #97/#98).
    ///
    /// [`LintConfig::default`] enables every registered rule at its default
    /// severity; `--disable-rule`/`--rule-severity` on the CLI and library
    /// consumers override it here, so CLI and programmatic lint runs apply
    /// the same deterministic configuration.
    pub lint: LintConfig,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig {
            input: InputSpec::Stdin,
            kind: SourceKind::Auto,
            locale: None,
            root: None,
            output: None,
            format: OutputFormat::Text,
            profile: wright_transform::Profile::Off,
            lint: LintConfig::default(),
        }
    }
}

impl SessionConfig {
    /// A config that reads `path` with automatic frontend detection.
    pub fn from_path(path: impl Into<PathBuf>) -> SessionConfig {
        SessionConfig {
            input: InputSpec::Path(path.into()),
            ..SessionConfig::default()
        }
    }
}
