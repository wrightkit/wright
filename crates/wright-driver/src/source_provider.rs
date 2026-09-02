//! Wright's product-facing source-provider boundary.
//!
//! The product layer selects a source target and receives source diagnostics
//! plus canonical Workshop text. Provider transport, document synchronization,
//! and compiler implementation types stay behind the adapter that implements
//! this trait. A provider owns project discovery from the selected entry; the
//! target deliberately carries no project graph or preloaded document set.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::diag::{Diagnostic, Origin, Position, Severity, SourceSpan, Stage};

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
    /// The CLI-supplied project root, when one was provided.
    pub project_root: Option<PathBuf>,
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
            project_root: None,
        }
    }

    /// Attach the CLI-supplied project root without changing owner-side
    /// project discovery.
    pub fn with_project_root(mut self, project_root: PathBuf) -> Self {
        self.project_root = Some(project_root);
        self
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

    /// Check the user-selected entry and return owner diagnostics.
    fn check(&mut self, target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError> {
        self.compile(target)
    }

    /// Compile the user-selected entry. Project discovery and source closure
    /// are owned by the implementation behind this boundary.
    fn compile(&mut self, target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError>;
}

/// The Wright adapter for a first-party LPP source provider.
pub struct LppSourceProvider {
    provider: Box<dyn wright_lpp::LanguageProvider>,
    locale: Option<String>,
}

impl LppSourceProvider {
    /// Construct an already initialized LPP provider adapter.
    pub fn new(provider: Box<dyn wright_lpp::LanguageProvider>, locale: Option<String>) -> Self {
        Self { provider, locale }
    }

    fn entry(
        &self,
        target: &SourceTarget,
    ) -> Result<wright_lpp::ProjectEntry, SourceProviderError> {
        let uri = url::Url::from_file_path(target.entry_path())
            .map_err(|()| SourceProviderError::Failed {
                code: "provider-invalid-entry".to_string(),
                message: format!(
                    "cannot convert OPY entry '{}' to an absolute file URI",
                    target.entry_path().display()
                ),
            })?
            .to_string();
        Ok(wright_lpp::ProjectEntry {
            uri,
            language_id: SourceLanguage::Opy.as_str().to_string(),
            version: 1,
        })
    }

    fn project_root_uri(target: &SourceTarget) -> Option<String> {
        target
            .project_root
            .as_deref()
            .and_then(|root| url::Url::from_directory_path(root).ok())
            .map(|uri| uri.to_string())
    }

    fn check_result(
        &mut self,
        target: &SourceTarget,
    ) -> Result<SourceCompilation, SourceProviderError> {
        let entry = self.entry(target)?;
        let result = self
            .provider
            .check_entry(
                &entry,
                Self::project_root_uri(target).as_deref(),
                self.locale.as_deref(),
            )
            .map_err(provider_error)?;
        Ok(SourceCompilation {
            workshop_text: None,
            locale: self.locale.clone(),
            provenance: SourceProvenance::Unmapped,
            diagnostics: provider_diagnostics(result.documents, self.locale.as_deref()),
        })
    }
}

impl Drop for LppSourceProvider {
    fn drop(&mut self) {
        let _ = self.provider.shutdown();
    }
}

impl SourceProvider for LppSourceProvider {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Opy
    }

    fn check(&mut self, target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError> {
        self.check_result(target)
    }

    fn compile(&mut self, target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError> {
        let entry = self.entry(target)?;
        let result = self
            .provider
            .compile_entry(
                &entry,
                Self::project_root_uri(target).as_deref(),
                self.locale.as_deref(),
            )
            .map_err(provider_error)?;
        let workshop_text = match result.artifact {
            Some(artifact) if artifact.format == "workshop-rs/text-v1" => Some(artifact.content),
            Some(artifact) => {
                return Err(SourceProviderError::Failed {
                    code: "provider-artifact-format".to_string(),
                    message: format!(
                        "the source provider returned unsupported artifact format '{}', expected 'workshop-rs/text-v1'",
                        artifact.format
                    ),
                });
            }
            None => None,
        };
        Ok(SourceCompilation {
            workshop_text,
            locale: self.locale.clone(),
            provenance: SourceProvenance::Unmapped,
            diagnostics: provider_diagnostics(result.diagnostics, self.locale.as_deref()),
        })
    }
}

fn provider_error(error: wright_lpp::ProviderError) -> SourceProviderError {
    SourceProviderError::Failed {
        code: error.code().to_string(),
        message: error.to_string(),
    }
}

fn provider_diagnostics(
    documents: Vec<wright_lpp::DocumentDiagnostics>,
    locale: Option<&str>,
) -> Vec<Diagnostic> {
    documents
        .into_iter()
        .enumerate()
        .flat_map(|(file, document)| {
            let path = provider_uri_path(&document.uri);
            document.diagnostics.into_iter().map(move |diagnostic| {
                let severity = match diagnostic.severity {
                    wright_lpp::DiagnosticSeverity::Error => Severity::Error,
                    wright_lpp::DiagnosticSeverity::Warning => Severity::Warning,
                    wright_lpp::DiagnosticSeverity::Info | wright_lpp::DiagnosticSeverity::Hint => {
                        Severity::Info
                    }
                };
                Diagnostic {
                    code: diagnostic
                        .code
                        .unwrap_or_else(|| "provider-diagnostic".to_string()),
                    stage: Stage::Frontend,
                    severity,
                    message: diagnostic.message,
                    status: None,
                    span: Some(SourceSpan {
                        file,
                        path: path.clone(),
                        start: provider_position(diagnostic.range.start),
                        end: provider_position(diagnostic.range.end),
                    }),
                    source: Some(Origin {
                        kind: SourceLanguage::Opy.as_str().to_string(),
                        locale: locale.map(str::to_owned),
                    }),
                }
            })
        })
        .collect()
}

fn provider_uri_path(uri: &str) -> String {
    url::Url::parse(uri)
        .ok()
        .and_then(|url| url.to_file_path().ok())
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| uri.to_string())
}

fn provider_position(position: wright_lpp::Position) -> Position {
    Position {
        line: position.line.saturating_add(1),
        col: position.character.saturating_add(1),
    }
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
