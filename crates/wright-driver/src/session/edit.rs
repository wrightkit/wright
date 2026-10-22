//! Validated source-edit operations on the session (#434):
//! `validateEditTransaction` runs on raw Workshop input through
//! `workshop-rs` reparse; other source languages stay at the provider
//! boundary. `lint --fix` resolves finding fixes through the same
//! validated source-edit path — a validated diff by default, an atomic
//! filesystem update with `--write`.

use std::collections::BTreeMap;
use std::sync::Arc;
use wright_analyzer::service::marker;

use super::{CompilerSession, Loaded, Provenance};
use crate::config::InputSpec;
use crate::diag::{Diagnostic, Stage};
use crate::edit::{EditTransaction, EditValidation};
use crate::input;
use crate::result::{
    Envelope, LintFixOutcome, LintFixPreview, LintFixStatus, LintResult, exit_code_from,
};

impl CompilerSession {
    /// `validateEditTransaction` (#434): apply `transaction` to the supplied
    /// current sources and, on raw Workshop input, reparse and validate the
    /// edited result through `workshop-rs`. When `sources` is absent (#472)
    /// the current text of the files the transaction names is read from
    /// disk; when present it stays the caller's precondition text. OPY and
    /// other source languages refuse with `edit-requires-provider` naming
    /// `providerValidateEdit`.
    pub fn validate_edit_transaction(
        &self,
        sources: Option<&BTreeMap<String, String>>,
        transaction: &EditTransaction,
    ) -> EditValidation {
        if let Err(diagnostic) = self.verify_fixed_config() {
            let diagnostics = vec![diagnostic];
            return EditValidation {
                ok: false,
                exit: crate::result::exit_code_from(&diagnostics),
                diagnostics,
                preview: None,
            };
        }
        crate::edit::validate_transaction(&self.config, self.catalog, sources, transaction)
    }

    /// Load the canonical program for caller-supplied Workshop text
    /// (ADR-0029): `editStatements` addresses the program its `sources`
    /// describe, so the text runs the same load path a disk resolution
    /// would — locale detection, parse, validation, canonical-id check,
    /// and the configured transform profile — and the derived `Loaded`
    /// keeps the `ResolvedInput`'s own display spelling and identity.
    /// Nothing is cached or adopted; the served snapshot is untouched.
    pub(crate) fn load_workshop_text(
        &mut self,
        resolved: &crate::input::ResolvedInput,
    ) -> Result<Loaded, Diagnostic> {
        let (mut program, locale) = self.load_workshop(resolved)?;
        super::apply_profile(&mut program, self.config.profile)?;
        let mut resolved = resolved.clone();
        resolved.origin.locale = Some(locale);
        let source_files = Arc::new(vec![resolved.display.clone()]);
        Ok(Loaded {
            program: Arc::new(program),
            origin: resolved.origin.clone(),
            input: resolved,
            provenance: Provenance::Source,
            source_files,
            completeness_issues: None,
            load_diagnostics: Arc::new(Vec::new()),
        })
    }

    /// `wright lint --fix [--write]` (#556): resolve the fixes findings
    /// carry through the validated source-edit path. A preview run
    /// validates every offered fix's transaction against the current input
    /// and reports the resulting diff; `write` applies fixes one at a time,
    /// reloading and re-linting between writes so every fix is planned and
    /// validated against the source version it edits. A fix whose
    /// preconditions no longer hold refuses with a structured diagnostic
    /// and writes nothing.
    pub fn lint_fix(&mut self, write: bool) -> Envelope<LintResult> {
        let mut envelope = self.lint();
        if !write {
            // stdin was already consumed by the lint pass; a second resolve
            // would read EOF or block. Fixes still refuse directly: absent
            // sources let `validate_transaction` answer `edit-input-stdin`.
            let sources = if self.config.input.path().is_some() {
                current_sources(&self.config)
            } else {
                None
            };
            let outcomes = self
                .fix_outcomes(&envelope.result.findings, sources.as_ref())
                .into_iter()
                .map(|(outcome, _)| outcome)
                .collect();
            envelope.result.fixes = Some(outcomes);
            return envelope;
        }
        if let Some(diagnostic) = write_input_refusal(&self.config.input, "lint --write") {
            envelope.diagnostics.push(diagnostic);
            recompute_verdict(&mut envelope);
            return envelope;
        }
        let mut applied = Vec::new();
        let mut aborted = None;
        let mut budget = envelope.result.findings.as_array().map_or(0, |findings| {
            findings
                .iter()
                .filter(|finding| finding.get("fix").is_some_and(|fix| fix.is_object()))
                .count()
        });
        loop {
            let sources = current_sources(&self.config);
            let candidate = self
                .fix_outcomes(&envelope.result.findings, sources.as_ref())
                .into_iter()
                .find(|(outcome, _)| outcome.status == LintFixStatus::Preview);
            let Some((outcome, resolved)) = candidate else {
                break;
            };
            if budget == 0 {
                applied.push(LintFixOutcome {
                    status: LintFixStatus::Refused,
                    diagnostics: vec![Diagnostic::error(
                        "edit-fix-stuck",
                        Stage::Discovery,
                        "the fix validated but did not settle its finding; refusing rather than reapplying it",
                    )],
                    ..outcome
                });
                aborted = Some(Diagnostic::error(
                    "edit-aborted",
                    Stage::Discovery,
                    "a fix that kept its finding stopped the run; this fix was not attempted",
                ));
                break;
            }
            let resolved = resolved.expect("a previewed outcome resolves");
            match crate::edit::write_previews(&resolved.previews, &resolved.transaction) {
                Ok(_) => {
                    budget = budget.saturating_sub(1);
                    applied.push(LintFixOutcome {
                        status: LintFixStatus::Applied,
                        ..outcome
                    });
                    match self.reload() {
                        Ok(_) => envelope = self.lint(),
                        Err(diagnostic) => {
                            aborted = Some(Diagnostic::error(
                                "edit-aborted",
                                Stage::Discovery,
                                "re-linting the edited source failed; this fix was not attempted",
                            ));
                            envelope.diagnostics.push(diagnostic);
                            recompute_verdict(&mut envelope);
                            break;
                        }
                    }
                }
                Err(diagnostic) => {
                    aborted = Some(Diagnostic::error(
                        "edit-aborted",
                        Stage::Discovery,
                        "an earlier fix's write failed; this fix was not attempted",
                    ));
                    envelope.diagnostics.push(diagnostic);
                    recompute_verdict(&mut envelope);
                    break;
                }
            }
        }
        let sources = current_sources(&self.config);
        let mut seen: std::collections::HashSet<(String, String)> = applied
            .iter()
            .map(|outcome| (outcome.code.clone(), format!("{:?}", outcome.span)))
            .collect();
        for (outcome, _) in self.fix_outcomes(&envelope.result.findings, sources.as_ref()) {
            if !seen.insert((outcome.code.clone(), format!("{:?}", outcome.span))) {
                continue;
            }
            applied.push(match (&aborted, outcome.status) {
                (Some(reason), LintFixStatus::Preview) => LintFixOutcome {
                    status: LintFixStatus::Refused,
                    preview: None,
                    diagnostics: vec![reason.clone()],
                    ..outcome
                },
                _ => outcome,
            });
        }
        envelope.result.fixes = Some(applied);
        envelope
    }

    /// The validated disposition of every finding in `findings` that
    /// carries a `fix` — in findings order. Each fix's transaction is
    /// deserialized and validated: raw Workshop fixes through
    /// `validate_transaction` (input identity, ranges, Workshop reparse),
    /// provider-mapped fixes through the provider pipeline (#583). A fix
    /// that fails deserializes or validates to a structured refusal.
    fn fix_outcomes(
        &self,
        findings: &serde_json::Value,
        sources: Option<&BTreeMap<String, String>>,
    ) -> Vec<(LintFixOutcome, Option<ResolvedFix>)> {
        let Some(findings) = findings.as_array() else {
            return Vec::new();
        };
        let provider = self.provider_fix_context();
        findings
            .iter()
            .filter_map(|finding| self.fix_outcome(finding, sources, provider.as_ref()))
            .collect()
    }

    /// For a provider-mapped load, the document set and precondition
    /// sources `providerValidateEdits` runs against — the members' current
    /// disk text under their `file://` URIs, built once per fix batch.
    /// `Some(Err)` refuses every fix in the batch with the member-read
    /// diagnostic; `None` keeps the raw Workshop validation path.
    fn provider_fix_context(&self) -> Option<Result<ProviderFixContext, Diagnostic>> {
        let loaded = self.loaded.as_ref()?;
        (loaded.provenance == Provenance::Mapped).then(|| {
            crate::source_provider::provider_document_set(
                &loaded.source_files,
                &loaded.input.root,
                crate::opy_provider::OPY_LANGUAGE_ID,
            )
            .map(|documents| ProviderFixContext {
                sources: crate::source_provider::document_sources(&documents),
                project_root: crate::source_provider::provider_project_root(&loaded.input.root),
                documents,
            })
            .map_err(|info| Diagnostic::error(info.code, Stage::Discovery, info.message))
        })
    }

    /// Validate one mapped-source fix transaction through the provider
    /// pipeline: Wright's preconditions, `lpp/validateEdits` per edited
    /// document, then `lpp/check` over the edited project (#583).
    fn provider_fix_validation(
        &self,
        context: &ProviderFixContext,
        transaction: &EditTransaction,
    ) -> Result<(Vec<crate::edit::SourcePreview>, Vec<Diagnostic>), Vec<Diagnostic>> {
        let request = crate::provider_edit::ProviderValidateRequest {
            documents: context.documents.clone(),
            transaction: transaction.clone(),
            sources: context.sources.clone(),
            project_root: context.project_root.clone(),
        };
        let mutation = self.run_provider_flow(
            crate::opy_provider::OPY_LANGUAGE_ID,
            &wright_lpp::ClientInfo {
                name: wright_lpp::LPP_CLIENT_NAME.to_string(),
                version: crate::result::DRIVER_VERSION.to_string(),
            },
            |provider| crate::provider_edit::validate_transaction(provider, &request),
        );
        if mutation.ok {
            Ok((mutation.preview.unwrap_or_default(), mutation.diagnostics))
        } else {
            Err(mutation.diagnostics)
        }
    }

    fn fix_outcome(
        &self,
        finding: &serde_json::Value,
        sources: Option<&BTreeMap<String, String>>,
        provider: Option<&Result<ProviderFixContext, Diagnostic>>,
    ) -> Option<(LintFixOutcome, Option<ResolvedFix>)> {
        let fix = finding.get("fix")?;
        if !fix.is_object() {
            return None;
        }
        let outcome = |status, preview, diagnostics| LintFixOutcome {
            code: finding["code"].as_str().unwrap_or_default().to_string(),
            kind: fix["kind"].as_str().unwrap_or_default().to_string(),
            summary: fix["summary"].as_str().unwrap_or_default().to_string(),
            span: finding.get("span").cloned(),
            status,
            preview,
            diagnostics,
        };
        let transaction =
            match serde_json::from_value::<EditTransaction>(fix["transaction"].clone()) {
                Ok(transaction) => transaction,
                Err(error) => {
                    return Some((
                        outcome(
                            LintFixStatus::Refused,
                            None,
                            vec![Diagnostic::error(
                                "edit-invalid-transaction",
                                Stage::Discovery,
                                format!(
                                    "the finding's fix transaction is malformed: {}",
                                    marker::escape(&error.to_string())
                                ),
                            )],
                        ),
                        None,
                    ));
                }
            };
        let validated = match provider {
            Some(Err(diagnostic)) => Err(vec![diagnostic.clone()]),
            Some(Ok(context)) => self
                .provider_fix_validation(context, &transaction)
                .map(|(previews, diagnostics)| (previews, diagnostics, Some(&context.sources))),
            None => {
                let validation = crate::edit::validate_transaction(
                    &self.config,
                    self.catalog,
                    sources,
                    &transaction,
                );
                if validation.ok {
                    Ok((
                        validation.preview.unwrap_or_default(),
                        validation.diagnostics,
                        sources,
                    ))
                } else {
                    Err(validation.diagnostics)
                }
            }
        };
        let (previews, diagnostics, originals) = match validated {
            Ok(validated) => validated,
            Err(diagnostics) => {
                return Some((outcome(LintFixStatus::Refused, None, diagnostics), None));
            }
        };
        let rendered = previews
            .iter()
            .map(|preview| LintFixPreview {
                // A mapped edit's `file://` URI displays as the same member
                // path spelling the finding's `span.path` resolves to.
                source: crate::source_provider::provider_uri_path(&preview.source),
                original: originals
                    .and_then(|sources| sources.get(&preview.source))
                    .cloned()
                    .unwrap_or_default(),
                new_text: preview.new_text.clone(),
            })
            .collect();
        Some((
            outcome(LintFixStatus::Preview, Some(rendered), diagnostics),
            Some(ResolvedFix {
                transaction,
                previews,
            }),
        ))
    }
}

struct ResolvedFix {
    transaction: EditTransaction,
    previews: Vec<crate::edit::SourcePreview>,
}

/// The provider-side context a mapped fix batch validates against (#583):
/// the loaded project's document set, its precondition sources keyed by
/// URI, and the project root the session compiled under.
struct ProviderFixContext {
    documents: wright_lpp::DocumentSet,
    sources: BTreeMap<String, String>,
    project_root: Option<String>,
}

fn write_input_refusal(input: &InputSpec, operation: &str) -> Option<Diagnostic> {
    let reason = match input {
        InputSpec::Path(_) => return None,
        InputSpec::Stdin => "stdin has no writable source",
        InputSpec::Text { .. } => "the session's source is caller-held text, not a writable file",
    };
    Some(Diagnostic::error(
        "edit-input-stdin",
        Stage::Discovery,
        format!("{operation} requires a path-based input; {reason}"),
    ))
}

fn current_sources(config: &crate::config::SessionConfig) -> Option<BTreeMap<String, String>> {
    input::resolve(config)
        .ok()
        .map(|resolved| BTreeMap::from([(resolved.display, resolved.text)]))
}

fn recompute_verdict(envelope: &mut Envelope<LintResult>) {
    envelope.exit = exit_code_from(&envelope.diagnostics);
    envelope.ok = envelope.exit == crate::result::exit::SUCCESS;
}
