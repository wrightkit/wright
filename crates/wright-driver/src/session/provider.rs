//! The provider-backed load: connect or reuse the injected
//! `SourceProvider`, run the session's check/compile operation against
//! it, and fold the answer into a `Loaded` snapshot. Split from
//! `session.rs` so the session loader stays under the context-size
//! limit.

use std::path::Path;
use std::sync::Arc;

use workshop_rs::Program;

use super::diagnostics::workshop_diag_for_provider_artifact;
#[cfg(not(target_arch = "wasm32"))]
use super::lpp_client_info;
use super::{CompilerSession, Loaded, Provenance, ProviderOperation};
use crate::diag::{Diagnostic, Severity, Stage};
use crate::input::{InputTarget, ResolvedInput};
#[cfg(not(target_arch = "wasm32"))]
use crate::opy_provider;
use crate::progress::{ProgressEvent, ProgressPhase};
use crate::source_provider::{
    SourceLanguage, SourceProvenance, SourceProviderError, SourceTarget, provider_uri_path,
};

impl CompilerSession {
    pub(super) fn load_from_source_provider(
        &mut self,
        resolved: &mut ResolvedInput,
        operation: ProviderOperation,
    ) -> Result<Loaded, Diagnostic> {
        let language = SourceLanguage::Opy;
        let entry = resolved
            .path
            .clone()
            .unwrap_or_else(|| Path::new("<stdin>").to_path_buf());
        let target = SourceTarget {
            kind: resolved.target,
            ..SourceTarget::new(language, entry, resolved.cwd.clone())
        }
        .with_project_root(resolved.root.clone());
        let diagnostics_start = self.diagnostics.len();
        // The LPP provider spawn path is native-only (ADR-0037 decision 5):
        // an injected `SourceProvider` still serves the compile below on
        // WebAssembly, but no process transport exists to create one.
        #[cfg(not(target_arch = "wasm32"))]
        if self.source_provider.is_none() {
            // The shared connect path (#1004): entry requests prefer the
            // 1.4 artifact-negotiation session, and the target's kind sets
            // the retry floor — entry requests need 1.1 project loading,
            // and a directory target cannot drop below the 1.3
            // source-identity contract it loads under (#638).
            let connected = self
                .connect_provider(
                    opy_provider::OPY_LANGUAGE_ID,
                    &lpp_client_info(),
                    wright_lpp::LPP_ARTIFACT_NEGOTIATION_PROTOCOL_VERSION,
                    Some(resolved.target.into()),
                )
                .map_err(|error| {
                    crate::source_provider::provider_error(
                        error,
                        "the 'opy' source provider could not be initialized",
                    )
                    .diagnostic()
                })?;
            // A directory target's canonical identity is the provider's
            // `sourceIdentity` (LPP 1.3): the spec requires the client to
            // refuse a session that negotiated without the capability
            // (#638).
            if resolved.target == InputTarget::Directory
                && !connected.initialize.capabilities.source_identity
            {
                return Err(crate::source_provider::provider_error(
                    wright_lpp::ProviderError::lpp(
                        wright_lpp::LppErrorKind::CapabilityUnavailable,
                        serde_json::json!({
                            "capability": "sourceIdentity",
                            "method": "lpp/compile",
                        }),
                        "directory targets require the LPP 1.3 'sourceIdentity' capability",
                    ),
                    "the 'opy' source provider could not be initialized",
                )
                .diagnostic());
            }
            self.source_provider = Some(Box::new(crate::source_provider::LppSourceProvider::new(
                connected.provider,
                self.config.locale.clone(),
            )));
        }
        let Some(provider) = self.source_provider.as_mut() else {
            return Err(SourceProviderError::NotConfigured { language }.diagnostic());
        };
        if provider.language() != language {
            return Err(Diagnostic::error(
                "source-provider-language",
                Stage::Discovery,
                format!(
                    "the injected provider serves '{}' but the selected source is '{}'",
                    provider.language().as_str(),
                    language.as_str()
                ),
            ));
        }
        let compilation = match operation {
            // The OPY provider contract makes `compile` diagnostics a
            // superset of `check` diagnostics — compile runs the same
            // source pipeline check runs, then the integration stage — so
            // `check` issues one compile request instead of paying for
            // check + compile plus an O(n²) merge. A provider that
            // declines compile (an unnegotiated capability) falls back to
            // its own check answer.
            ProviderOperation::Check => match provider.compile(&target) {
                Ok(compilation) => compilation,
                Err(error)
                    if matches!(error, SourceProviderError::Unsupported { .. })
                        || error.code() == "capability-unavailable" =>
                {
                    provider
                        .check(&target)
                        .map_err(|error| error.diagnostic())?
                }
                Err(error) => return Err(error.diagnostic()),
            },
            ProviderOperation::Compile => provider.compile(&target).map_err(|e| e.diagnostic())?,
        };
        if (operation == ProviderOperation::Compile || compilation.workshop_text.is_some())
            && resolved.target == InputTarget::Directory
        {
            let Some(id) = compilation
                .source_identity
                .as_ref()
                .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            else {
                return Err(Diagnostic::error(
                    "source-provider-identity",
                    Stage::Frontend,
                    if compilation.source_identity.is_none() {
                        "the source provider returned no source identity for the directory target"
                    } else {
                        "the source provider returned an invalid SHA-256 source identity"
                    },
                ));
            };
            resolved.identity = id.clone();
        }
        let mut provider_diagnostics = compilation.diagnostics;
        if let Some(index) = provider_diagnostics
            .iter()
            .position(|d| d.severity == Severity::Error)
        {
            let first = provider_diagnostics.remove(index);
            self.diagnostics.extend(provider_diagnostics);
            return Err(first);
        }
        self.diagnostics.extend(provider_diagnostics);
        let source_map = match compilation.provenance {
            SourceProvenance::Unmapped => None,
            SourceProvenance::Mapped(map) => Some(map),
        };
        let locale_name = compilation
            .locale
            .or_else(|| resolved.origin.locale.clone())
            .unwrap_or_else(|| "en-US".to_string());
        let locale = workshop_rs::catalog::Locale::new(&locale_name);
        if operation == ProviderOperation::Check && compilation.workshop_text.is_none() {
            return Ok(Loaded {
                program: Arc::new(Program::default()),
                origin: resolved.origin.clone(),
                input: resolved.clone(),
                provenance: Provenance::Unmapped,
                source_files: Arc::new(vec![resolved.display.clone()]),
                completeness_issues: None,
                load_diagnostics: Arc::new(Vec::new()),
            });
        }
        let Some(workshop_text) = compilation.workshop_text else {
            return Err(Diagnostic::error(
                "source-provider-no-artifact",
                Stage::Frontend,
                "the source provider returned no canonical Workshop output",
            ));
        };
        self.progress(ProgressEvent::new(ProgressPhase::Parsing));
        let mut program = workshop_rs::parser::parse_with_context(
            &workshop_text,
            self.catalog,
            &locale,
            self.catalog,
        )
        .map_err(|error| workshop_diag_for_provider_artifact(error, resolved, &[]))?;
        let mut provenance = Provenance::Unmapped;
        let mut source_files = vec![resolved.display.clone()];
        if let Some(map) = &source_map {
            match map.apply(&mut program) {
                Ok(()) => {
                    provenance = Provenance::Mapped;
                    source_files = map.files().iter().map(|f| provider_uri_path(f)).collect();
                    let mut unreadable = Vec::new();
                    for (index, member) in source_files.iter().enumerate() {
                        let member_path = resolved.root.join(member);
                        match std::fs::read_to_string(&member_path) {
                            Ok(text) => {
                                program.set_file_source(
                                    workshop_rs::source::FileId::from_index(index),
                                    text,
                                );
                            }
                            Err(error) => unreadable.push(format!(
                                "{} ({})",
                                member,
                                error
                            )),
                        }
                    }
                    if !unreadable.is_empty() {
                        self.diagnostics.push(Diagnostic::warning(
                            "source-map-member-unreadable",
                            Stage::Frontend,
                            format!(
                                "{} provider source-map member(s) could not be read, so their source extents are unavailable: {}",
                                unreadable.len(),
                                unreadable.join(", "),
                            ),
                        ));
                    }
                }
                Err(error) => self.diagnostics.push(Diagnostic::warning(
                    "source-map-mismatch",
                    Stage::Frontend,
                    format!(
                        "the provider source map does not match its Workshop artifact ({}); findings are reported as unmapped",
                        error
                    ),
                )),
            }
        }
        self.progress(ProgressEvent::new(ProgressPhase::Validation));
        program.validate().map_err(|error| {
            workshop_diag_for_provider_artifact(
                error,
                resolved,
                if provenance == Provenance::Mapped {
                    &source_files
                } else {
                    &[]
                },
            )
        })?;
        resolved.origin.locale = Some(locale.to_string());
        let loaded = Loaded {
            program: Arc::new(program),
            origin: resolved.origin.clone(),
            input: resolved.clone(),
            provenance,
            source_files: Arc::new(source_files),
            completeness_issues: None,
            load_diagnostics: Arc::new(self.diagnostics[diagnostics_start..].to_vec()),
        };
        self.loaded = Some(loaded.clone());
        // The loaded snapshot was produced by a compile answer regardless
        // of which workflow asked for it (#1005): a `check` whose compile
        // returned an artifact holds a canonical compile result, and
        // `load()` may serve it.
        self.loaded_operation = Some(ProviderOperation::Compile);
        Ok(loaded)
    }
}
