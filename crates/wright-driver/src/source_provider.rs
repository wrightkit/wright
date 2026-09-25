use std::fmt;
use std::path::{Path, PathBuf};

use workshop_rs::program::{MAPPED_TEXT_V1, TEXT_V1};
use workshop_rs::{MappedText, SourceMap};

use crate::diag::{Diagnostic, Origin, Position, Severity, SourceSpan, Stage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceLanguage {
    Opy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceTargetKind {
    File,
    Directory,
}

impl SourceLanguage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Opy => "opy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SourceBackend {
    #[default]
    Native,
    Auto,
    Provider,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTarget {
    pub language: SourceLanguage,
    pub entry: PathBuf,
    pub kind: SourceTargetKind,
    pub cwd: PathBuf,
    pub project_root: Option<PathBuf>,
}

/// How a compiled Workshop artifact relates to the authored source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceProvenance {
    /// The artifact carries no mapping to the authored source.
    Unmapped,
    /// The provider returned `workshop-rs/mapped-text-v1`; the map applies to
    /// the program parsed from the artifact's Workshop text.
    Mapped(SourceMap),
}

impl SourceTarget {
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
            kind: SourceTargetKind::File,
            cwd,
            project_root: None,
        }
    }

    pub fn directory(
        language: SourceLanguage,
        directory: impl Into<PathBuf>,
        cwd: impl Into<PathBuf>,
    ) -> Self {
        let mut target = Self::new(language, directory, cwd);
        target.kind = SourceTargetKind::Directory;
        target
    }

    pub fn with_project_root(mut self, project_root: PathBuf) -> Self {
        self.project_root = Some(project_root);
        self
    }

    pub fn entry_path(&self) -> &Path {
        &self.entry
    }

    pub fn is_directory(&self) -> bool {
        self.kind == SourceTargetKind::Directory
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceCompilation {
    pub workshop_text: Option<String>,
    pub locale: Option<String>,
    pub provenance: SourceProvenance,
    pub diagnostics: Vec<Diagnostic>,
    pub source_identity: Option<String>,
}

impl SourceCompilation {
    pub fn success(workshop_text: impl Into<String>) -> Self {
        Self {
            workshop_text: Some(workshop_text.into()),
            locale: None,
            provenance: SourceProvenance::Unmapped,
            diagnostics: Vec::new(),
            source_identity: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceProviderError {
    NotConfigured { language: SourceLanguage },
    Unsupported { message: String },
    Failed { code: String, message: String },
}

impl SourceProviderError {
    pub fn code(&self) -> &str {
        match self {
            Self::NotConfigured { .. } => "source-provider-not-configured",
            Self::Unsupported { .. } => "source-provider-unsupported",
            Self::Failed { code, .. } => code,
        }
    }

    pub fn diagnostic(&self) -> Diagnostic {
        let stage = match self {
            Self::Unsupported { .. } => Stage::Frontend,
            Self::NotConfigured { .. } | Self::Failed { .. } => Stage::Internal,
        };
        Diagnostic::error(self.code(), stage, self.to_string())
    }
}

impl fmt::Display for SourceProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConfigured { language } => write!(
                f,
                "no source provider is configured for '{}'",
                language.as_str()
            ),
            Self::Unsupported { message } | Self::Failed { message, .. } => f.write_str(message),
        }
    }
}

impl std::error::Error for SourceProviderError {}

pub trait SourceProvider {
    fn language(&self) -> SourceLanguage;
    fn check(&mut self, target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError> {
        self.compile(target)
    }
    fn compile(&mut self, target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError>;
}

pub struct LppSourceProvider {
    provider: Box<dyn wright_lpp::LanguageProvider>,
    locale: Option<String>,
}

impl LppSourceProvider {
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
            kind: if target.is_directory() {
                wright_lpp::ProjectTargetKind::Directory
            } else {
                wright_lpp::ProjectTargetKind::File
            },
        })
    }

    fn project_root_uri(target: &SourceTarget) -> Option<String> {
        target
            .project_root
            .as_deref()
            .and_then(|r| url::Url::from_directory_path(r).ok())
            .map(|u| u.to_string())
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
        let entry = self.entry(target)?;
        let root_uri = Self::project_root_uri(target);
        let res = if target.is_directory() {
            self.provider
                .check_target(&entry, root_uri.as_deref(), self.locale.as_deref())
        } else {
            self.provider
                .check_entry(&entry, root_uri.as_deref(), self.locale.as_deref())
        }
        .map_err(provider_error)?;
        Ok(SourceCompilation {
            workshop_text: None,
            locale: self.locale.clone(),
            provenance: SourceProvenance::Unmapped,
            diagnostics: provider_diagnostics(res.documents, self.locale.as_deref()),
            source_identity: None,
        })
    }

    fn compile(&mut self, target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError> {
        let entry = self.entry(target)?;
        let root_uri = Self::project_root_uri(target);
        let negotiates_artifacts = self.provider.capabilities().is_ok_and(|c| {
            c.protocol_version == wright_lpp::LPP_ARTIFACT_NEGOTIATION_PROTOCOL_VERSION
        });
        let res = if negotiates_artifacts {
            self.provider.compile_target_accepting(
                &entry,
                root_uri.as_deref(),
                self.locale.as_deref(),
                &[MAPPED_TEXT_V1, TEXT_V1],
            )
        } else if target.is_directory() {
            self.provider
                .compile_target(&entry, root_uri.as_deref(), self.locale.as_deref())
        } else {
            self.provider
                .compile_entry(&entry, root_uri.as_deref(), self.locale.as_deref())
        }
        .map_err(provider_error)?;
        let (workshop_text, provenance) = match res.artifact {
            Some(a) if a.format == TEXT_V1 => (Some(a.content), SourceProvenance::Unmapped),
            Some(a) if negotiates_artifacts && a.format == MAPPED_TEXT_V1 => {
                let mapped = MappedText::from_json(&a.content).map_err(|error| {
                    SourceProviderError::Failed {
                        code: "provider-artifact-format".to_string(),
                        message: format!(
                            "the source provider returned an invalid '{MAPPED_TEXT_V1}' artifact: {error}"
                        ),
                    }
                })?;
                (Some(mapped.text), SourceProvenance::Mapped(mapped.map))
            }
            Some(a) => {
                return Err(SourceProviderError::Failed {
                    code: "provider-artifact-format".to_string(),
                    message: format!(
                        "the source provider returned unsupported artifact format '{}', expected '{TEXT_V1}'",
                        a.format
                    ),
                });
            }
            None => (None, SourceProvenance::Unmapped),
        };
        Ok(SourceCompilation {
            workshop_text,
            locale: self.locale.clone(),
            provenance,
            diagnostics: provider_diagnostics(res.diagnostics, self.locale.as_deref()),
            source_identity: res.source_identity,
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
        .flat_map(|(file, doc)| {
            let path = provider_uri_path(&doc.uri);
            doc.diagnostics.into_iter().map(move |d| {
                let severity = match d.severity {
                    wright_lpp::DiagnosticSeverity::Error => Severity::Error,
                    wright_lpp::DiagnosticSeverity::Warning => Severity::Warning,
                    _ => Severity::Info,
                };
                Diagnostic {
                    code: d.code.unwrap_or_else(|| "provider-diagnostic".to_string()),
                    stage: Stage::Frontend,
                    severity,
                    message: d.message,
                    status: None,
                    span: Some(SourceSpan {
                        file,
                        path: path.clone(),
                        start: provider_position(d.range.start),
                        end: provider_position(d.range.end),
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

pub(crate) fn provider_uri_path(uri: &str) -> String {
    url::Url::parse(uri)
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| uri.to_string())
}

fn provider_position(pos: wright_lpp::Position) -> Position {
    Position {
        line: pos.line.saturating_add(1),
        col: pos.character.saturating_add(1),
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
