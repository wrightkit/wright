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

#[derive(Debug, Clone, Default)]
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
}
