//! Wright's product-facing source-provider boundary.
//!
//! The product layer selects a source target and receives source diagnostics
//! plus canonical Workshop text. Provider transport, document synchronization,
//! and compiler implementation types stay behind the adapter that implements
//! this trait. A provider owns project discovery from the selected entry; the
//! target deliberately carries no project graph or preloaded document set.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::diag::{Diagnostic, Stage};

/// A source language that can participate in the product boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceLanguage {
    /// OverPy, whose provider owns `#!mainFile`, includes, and preprocessing.
    Opy,
}

impl SourceLanguage {
    /// The stable product spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            SourceLanguage::Opy => "opy",
        }
    }
}

/// Whether a source language is loaded natively or through an injected
/// provider. The choice is explicit so provider failures cannot select a
/// different implementation by accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SourceBackend {
    /// Use the implementation linked into Wright for the selected source kind.
    #[default]
    Native,
    /// Require the explicitly injected source provider.
    Provider,
}

/// The user-selected source target passed to a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTarget {
    /// The language selected by the product layer.
    pub language: SourceLanguage,
    /// The one path selected by the user, resolved relative to [`cwd`].
    pub entry: PathBuf,
    /// The invocation working directory used to resolve relative CLI paths.
    pub cwd: PathBuf,
}

/// The provenance contract for the canonical Workshop result.
///
/// The current provider boundary has no canonical-Workshop-to-authored-source
/// span map, so provider results remain explicitly unmapped. A mapped variant
/// must not be added without carrying and consuming the actual mapping data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceProvenance {
    /// The canonical artifact has no authored-source mapping.
    Unmapped,
}

impl SourceTarget {
    /// Construct an entry target and resolve a relative path from `cwd`.
    pub fn new(
        language: SourceLanguage,
        entry: impl Into<PathBuf>,
        cwd: impl Into<PathBuf>,
    ) -> Self {
        let cwd = cwd.into();
        let entry = entry.into();
        let entry = if entry.is_absolute() {
            entry
        } else {
            cwd.join(entry)
        };
        Self {
            language,
            entry,
            cwd,
        }
    }

    /// The entry path as a [`Path`].
    pub fn entry_path(&self) -> &Path {
        &self.entry
    }
}

/// The result of a provider-owned source compilation.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceCompilation {
    /// Canonical Workshop source returned by the provider, when compilation
    /// succeeded. The driver validates and parses this text through
    /// `workshop-rs`; it never consumes provider-internal IR.
    pub workshop_text: Option<String>,
    /// Workshop client locale for the returned canonical source.
    pub locale: Option<String>,
    /// Whether canonical Workshop spans can be mapped to authored source.
    pub provenance: SourceProvenance,
    /// Diagnostics already attributed to their authored source files by the
    /// provider adapter. These are preserved alongside Wright diagnostics.
    pub diagnostics: Vec<Diagnostic>,
}

impl SourceCompilation {
    /// A successful canonical Workshop result without diagnostics.
    pub fn success(workshop_text: impl Into<String>) -> Self {
        Self {
            workshop_text: Some(workshop_text.into()),
            locale: None,
            provenance: SourceProvenance::Unmapped,
            diagnostics: Vec::new(),
        }
    }
}

/// A failure at the source-provider boundary, distinct from a source
/// diagnostic returned by the provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceProviderError {
    /// The requested provider was not injected for a provider-backed session.
    NotConfigured { language: SourceLanguage },
    /// The provider cannot perform the requested product operation.
    Unsupported { message: String },
    /// The provider or its process failed independently of source contents.
    Failed { code: String, message: String },
}

impl SourceProviderError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &str {
        match self {
            SourceProviderError::NotConfigured { .. } => "source-provider-not-configured",
            SourceProviderError::Unsupported { .. } => "source-provider-unsupported",
            SourceProviderError::Failed { code, .. } => code,
        }
    }

    /// Convert the boundary failure to the driver's structured diagnostic.
    pub fn diagnostic(&self) -> Diagnostic {
        let message = self.to_string();
        let stage = match self {
            SourceProviderError::Unsupported { .. } => Stage::Frontend,
            SourceProviderError::NotConfigured { .. } | SourceProviderError::Failed { .. } => {
                Stage::Internal
            }
        };
        Diagnostic::error(self.code(), stage, message)
    }
}

impl fmt::Display for SourceProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceProviderError::NotConfigured { language } => write!(
                formatter,
                "no source provider is configured for '{}'",
                language.as_str()
            ),
            SourceProviderError::Unsupported { message }
            | SourceProviderError::Failed { message, .. } => formatter.write_str(message),
        }
    }
}

impl std::error::Error for SourceProviderError {}

/// The provider implementation consumed by the product layer.
pub trait SourceProvider {
    /// The source language served by this provider.
    fn language(&self) -> SourceLanguage;

    /// Compile the user-selected entry. Project discovery and source closure
    /// are owned by the implementation behind this boundary.
    fn compile(&mut self, target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_entry_is_resolved_from_the_invocation_directory() {
        let target = SourceTarget::new(SourceLanguage::Opy, "src/main.opy", "/project");
        assert_eq!(target.entry, PathBuf::from("/project/src/main.opy"));
        assert_eq!(target.cwd, PathBuf::from("/project"));
    }

    #[test]
    fn provider_failures_have_a_distinct_structured_code() {
        let diagnostic = SourceProviderError::Failed {
            code: "provider-exited".to_string(),
            message: "provider exited".to_string(),
        }
        .diagnostic();
        assert_eq!(diagnostic.code, "provider-exited");
        assert_eq!(diagnostic.stage, Stage::Internal);
        assert!(diagnostic.span.is_none());
    }
}
