//! The LPP-backed `SourceProvider` adapter (#142): it speaks the wire
//! contract through `wright_lpp` and folds each answer into a
//! `SourceCompilation`. Split out of `source_provider.rs` under the
//! file-size budget; the domain types stay in the parent module.

use std::path::PathBuf;

use workshop_rs::MappedText;
use workshop_rs::program::{MAPPED_TEXT_V1, TEXT_V1};

use crate::diag::{Diagnostic, Origin, Severity, SourceSpan, Stage};

use super::columns;
use super::{
    SourceCompilation, SourceLanguage, SourceProvenance, SourceProvider, SourceProviderError,
    SourceTarget, provider_error, provider_uri_path,
};

pub struct LppSourceProvider {
    provider: Box<dyn wright_lpp::LanguageProvider>,
    locale: Option<String>,
    /// The declared input set of the most recent provider answer: `Some`
    /// when the session negotiated `compileInputs` and the compile
    /// answered with the list, `None` otherwise.
    compile_inputs: Option<Vec<PathBuf>>,
}

impl LppSourceProvider {
    pub fn new(provider: Box<dyn wright_lpp::LanguageProvider>, locale: Option<String>) -> Self {
        Self {
            provider,
            locale,
            compile_inputs: None,
        }
    }

    fn entry(
        &self,
        target: &SourceTarget,
    ) -> Result<wright_lpp::ProjectEntry, SourceProviderError> {
        let uri = url::Url::from_file_path(target.entry_path())
            .map_err(|()| SourceProviderError::Failed {
                code: "provider-invalid-entry".to_string(),
                stage: Stage::Internal,
                class: crate::diag::ExitClass::Internal,
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
            .and_then(super::provider_project_root)
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
        self.compile_inputs = None;
        let entry = self.entry(target)?;
        let root_uri = Self::project_root_uri(target);
        let res = if target.is_directory() {
            self.provider
                .check_target(&entry, root_uri.as_deref(), self.locale.as_deref())
        } else {
            self.provider
                .check_entry(&entry, root_uri.as_deref(), self.locale.as_deref())
        }
        .map_err(|error| provider_error(error, &request_context("check", target)))?;
        Ok(SourceCompilation {
            workshop_text: None,
            locale: self.locale.clone(),
            provenance: SourceProvenance::Unmapped,
            diagnostics: provider_diagnostics(res.documents, self.locale.as_deref()),
            source_identity: None,
        })
    }

    fn compile(&mut self, target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError> {
        self.compile_inputs = None;
        let entry = self.entry(target)?;
        let root_uri = Self::project_root_uri(target);
        let (negotiates_artifacts, declares_inputs) = {
            let negotiated = self
                .provider
                .capabilities()
                .map_err(|error| provider_error(error, &request_context("compile", target)))?;
            (
                wright_lpp::protocol_version_at_least(
                    &negotiated.protocol_version,
                    wright_lpp::LPP_ARTIFACT_NEGOTIATION_PROTOCOL_VERSION,
                ),
                negotiated.capabilities.compile_inputs,
            )
        };
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
        .map_err(|error| provider_error(error, &request_context("compile", target)))?;
        let (workshop_text, provenance) = match res.artifact {
            Some(a) if a.format == TEXT_V1 => (Some(a.content), SourceProvenance::Unmapped),
            Some(a) if negotiates_artifacts && a.format == MAPPED_TEXT_V1 => {
                let mapped = MappedText::from_json(&a.content).map_err(|error| {
                    SourceProviderError::Failed {
                        code: "provider-artifact-format".to_string(),
                        stage: Stage::Internal,
                        class: crate::diag::ExitClass::Internal,
                        message: format!(
                            "the source provider returned an invalid '{MAPPED_TEXT_V1}' artifact: {}",
                            error
                        ),
                    }
                })?;
                (Some(mapped.text), SourceProvenance::Mapped(mapped.map))
            }
            Some(a) => {
                return Err(SourceProviderError::Failed {
                    code: "provider-artifact-format".to_string(),
                    stage: Stage::Internal,
                    class: crate::diag::ExitClass::Internal,
                    message: format!(
                        "the source provider returned unsupported artifact format '{}', expected '{TEXT_V1}'",
                        a.format
                    ),
                });
            }
            None => (None, SourceProvenance::Unmapped),
        };
        // The declaration is trusted only when the provider advertised
        // `compileInputs` for this session — an unadvertised field is
        // ignored, so the freshness path keeps the wider fingerprint.
        self.compile_inputs = declares_inputs
            .then_some(res.compile_inputs)
            .flatten()
            .map(|uris| {
                uris.iter()
                    .map(|uri| PathBuf::from(provider_uri_path(uri)))
                    .collect()
            });
        Ok(SourceCompilation {
            workshop_text,
            locale: self.locale.clone(),
            provenance,
            diagnostics: provider_diagnostics(res.diagnostics, self.locale.as_deref()),
            source_identity: res.source_identity,
        })
    }

    fn compile_inputs(&self) -> Option<Vec<PathBuf>> {
        self.compile_inputs.clone()
    }
}

fn request_context(operation: &str, target: &SourceTarget) -> String {
    format!(
        "the source provider {operation} request for '{}' failed",
        target.entry_path().display()
    )
}

pub(super) fn provider_diagnostics(
    documents: Vec<wright_lpp::DocumentDiagnostics>,
    locale: Option<&str>,
) -> Vec<Diagnostic> {
    let mut arena: Vec<String> = documents
        .iter()
        .map(|doc| provider_uri_path(&doc.uri))
        .collect();
    arena.sort();
    arena.dedup();
    documents
        .into_iter()
        .flat_map(|doc| {
            let path = provider_uri_path(&doc.uri);
            let file = arena.iter().position(|entry| *entry == path).unwrap_or(0);
            let lines: Vec<String> = std::fs::read_to_string(&path)
                .map(|text| text.split('\n').map(str::to_owned).collect())
                .unwrap_or_default();
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
                    class: crate::diag::class_for_stage(Stage::Frontend),
                    message: d.message.clone(),
                    status: None,
                    span: Some(SourceSpan {
                        file,
                        path: path.clone(),
                        start: columns::provider_position(d.range.start, &lines),
                        end: columns::provider_position(d.range.end, &lines),
                    }),
                    source: Some(Origin {
                        kind: SourceLanguage::Opy.as_str().to_string(),
                        locale: locale.map(str::to_owned),
                    }),
                    fields: Default::default(),
                }
            })
        })
        .collect()
}
