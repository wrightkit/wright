use std::path::PathBuf;

use crate::source_provider::SourceBackend;
pub use wright_analyzer::registry::LintConfig;

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
    #[default]
    Stdin,
}

impl InputSpec {
    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            Self::Path(path) => Some(path),
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
    pub output: Option<PathBuf>,
    pub format: OutputFormat,
    pub profile: wright_transform::Profile,
    pub lint: LintConfig,
    pub lint_rule_paths: Vec<PathBuf>,
    /// Finding/diagnostic selection applied to reported output (#430).
    /// Selection narrows presentation only; verdicts and exit codes are
    /// computed on the full set.
    pub selection: crate::select::FindingSelection,
    pub providers: wright_lpp::ProviderRegistry,
    pub opy_provider: crate::opy_provider::OpyProviderConfig,
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
        if self.output != other.output {
            changed.push("output");
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
        if self.lint_rule_paths != other.lint_rule_paths {
            changed.push("lint_rule_paths");
        }
        if self.selection != other.selection {
            changed.push("selection");
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
