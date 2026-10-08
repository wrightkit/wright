//! Validated source-edit operations on the session (#434):
//! `validateEditTransaction` and `semanticRename` run on raw Workshop
//! input through `workshop-rs` reparse and identifier provenance; other
//! source languages stay at the provider boundary. `rename` is the
//! `wright rename` CLI workflow — a validated diff by default, an atomic
//! filesystem update with `--write`.

use std::collections::BTreeMap;

use super::CompilerSession;
use crate::diag::{Diagnostic, Stage};
use crate::edit::{EditTransaction, EditValidation, RenameResult, RenameTarget, SemanticRename};
use crate::input;
use crate::result::{
    Envelope, LintFixOutcome, LintFixPreview, LintFixStatus, LintResult, exit_code_from,
};
use crate::service::Address;

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
        crate::edit::validate_transaction(&self.config, &self.catalog, sources, transaction)
    }

    /// `semanticRename` (#434): resolve `target` against the loaded
    /// program's semantic index and rewrite every identifier occurrence
    /// through `workshop-rs` provenance — never a textual search. When
    /// `sources` is absent (#472) the loaded input's current text is read
    /// from disk; when present it stays the caller's precondition text.
    /// OPY and other source languages refuse with `edit-requires-provider`
    /// naming `providerSemanticRename`, whether or not a provider is
    /// configured.
    pub fn semantic_rename(
        &mut self,
        sources: Option<&BTreeMap<String, String>>,
        target: &RenameTarget,
    ) -> SemanticRename {
        let refuse = |diagnostics: Vec<Diagnostic>| SemanticRename {
            ok: false,
            transaction: None,
            diagnostics,
            preview: None,
        };
        if let Err(diagnostic) = self.verify_fixed_config() {
            return refuse(vec![diagnostic]);
        }
        match crate::edit::resolve_edit_input(&self.config) {
            Ok(resolved) => {
                if let Some(diagnostic) =
                    crate::edit::edit_kind_gate(resolved.kind, "providerSemanticRename")
                {
                    return refuse(vec![diagnostic]);
                }
            }
            Err(diagnostic) => return refuse(vec![diagnostic]),
        }
        match self.load() {
            Ok(loaded) => crate::edit::semantic_rename(&loaded, &self.catalog, sources, target),
            Err(diagnostic) => refuse(vec![diagnostic]),
        }
    }

    /// `wright rename <NAME> <TO> [INPUT]` (#434): the validated semantic
    /// rename of a Workshop variable or subroutine — the transaction and a
    /// source diff by default, an atomic filesystem update with `write`.
    /// A stale input or an invalid rename refuses with structured
    /// diagnostics and no partial write.
    pub fn rename(&mut self, name: &str, to: &str, write: bool) -> Envelope<RenameResult> {
        let mut result = RenameResult::default();
        if let Err(diagnostic) = self.verify_fixed_config() {
            self.diagnostics.push(diagnostic);
            return self.finish("rename", result);
        }
        match crate::edit::resolve_edit_input(&self.config) {
            Ok(resolved) => {
                if let Some(diagnostic) =
                    crate::edit::edit_kind_gate(resolved.kind, "providerSemanticRename")
                {
                    self.diagnostics.push(diagnostic);
                    return self.finish("rename", result);
                }
            }
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish("rename", result);
            }
        }
        let loaded = match self.load() {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish("rename", result);
            }
        };
        let sources = BTreeMap::from([(loaded.input.display.clone(), loaded.input.text.clone())]);
        let rename = crate::edit::semantic_rename(
            &loaded,
            &self.catalog,
            Some(&sources),
            &RenameTarget {
                symbol: Some(Address::Name(name.to_string())),
                source: None,
                line: None,
                col: None,
                to: to.to_string(),
            },
        );
        self.diagnostics.extend(rename.diagnostics);
        result.transaction = rename.transaction;
        result.preview = rename.preview;
        result.originals = sources;
        if !rename.ok {
            return self.finish("rename", result);
        }
        if write {
            match crate::edit::write_previews(
                result.preview.as_deref().unwrap_or_default(),
                result
                    .transaction
                    .as_ref()
                    .expect("a successful rename carries its transaction"),
            ) {
                Ok(written) => result.written = written,
                Err(diagnostic) => self.diagnostics.push(diagnostic),
            }
        }
        self.finish("rename", result)
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
            let outcomes = fix_outcomes(
                &self.config,
                &self.catalog,
                &envelope.result.findings,
                sources.as_ref(),
            )
            .into_iter()
            .map(|(outcome, _)| outcome)
            .collect();
            envelope.result.fixes = Some(outcomes);
            return envelope;
        }
        if self.config.input.path().is_none() {
            envelope.diagnostics.push(Diagnostic::error(
                "edit-input-stdin",
                Stage::Discovery,
                "lint --write requires a path-based input; stdin has no writable source",
            ));
            retune(&mut envelope);
            return envelope;
        }
        // `write`: each pass applies the first fix that still validates —
        // a write changes the source identity every other fix's
        // precondition names, so fixes are re-planned from a fresh lint
        // after each application. A fix that never removes its finding
        // cannot loop forever: the initial fixable count bounds the passes.
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
            let candidate = fix_outcomes(
                &self.config,
                &self.catalog,
                &envelope.result.findings,
                sources.as_ref(),
            )
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
                            retune(&mut envelope);
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
                    retune(&mut envelope);
                    break;
                }
            }
        }
        // The final pass reports what remains: offered fixes that were not
        // applied get their validated preview or structured refusal. Fixes
        // already reported by the write loop are not re-reported, and fixes
        // that would have applied after an aborted run report a refusal —
        // they were never attempted, not merely previewed.
        let sources = current_sources(&self.config);
        let mut seen: std::collections::HashSet<(String, String)> = applied
            .iter()
            .map(|outcome| (outcome.code.clone(), format!("{:?}", outcome.span)))
            .collect();
        for (outcome, _) in fix_outcomes(
            &self.config,
            &self.catalog,
            &envelope.result.findings,
            sources.as_ref(),
        ) {
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
}

/// A fix that validated: the deserialized transaction and the source
/// previews `write_previews` applies — the internal handle a previewed
/// [`LintFixOutcome`] carries through the write path.
struct ResolvedFix {
    transaction: EditTransaction,
    previews: Vec<crate::edit::SourcePreview>,
}

/// The validated disposition of every finding in `findings` that carries a
/// `fix` — in findings order. Each fix's transaction is deserialized and
/// validated through `validate_transaction` (input identity, ranges,
/// Workshop reparse); a fix that fails deserializes or validates to a
/// structured refusal.
fn fix_outcomes(
    config: &crate::config::SessionConfig,
    catalog: &workshop_rs::catalog::Catalog,
    findings: &serde_json::Value,
    sources: Option<&BTreeMap<String, String>>,
) -> Vec<(LintFixOutcome, Option<ResolvedFix>)> {
    let Some(findings) = findings.as_array() else {
        return Vec::new();
    };
    findings
        .iter()
        .filter_map(|finding| fix_outcome(config, catalog, finding, sources))
        .collect()
}

fn fix_outcome(
    config: &crate::config::SessionConfig,
    catalog: &workshop_rs::catalog::Catalog,
    finding: &serde_json::Value,
    sources: Option<&BTreeMap<String, String>>,
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
    let transaction = match serde_json::from_value::<EditTransaction>(fix["transaction"].clone()) {
        Ok(transaction) => transaction,
        Err(error) => {
            return Some((
                outcome(
                    LintFixStatus::Refused,
                    None,
                    vec![Diagnostic::error(
                        "edit-invalid-transaction",
                        Stage::Discovery,
                        format!("the finding's fix transaction is malformed: {error}"),
                    )],
                ),
                None,
            ));
        }
    };
    let validation = crate::edit::validate_transaction(config, catalog, sources, &transaction);
    if !validation.ok {
        return Some((
            outcome(LintFixStatus::Refused, None, validation.diagnostics),
            None,
        ));
    }
    let previews = validation.preview.unwrap_or_default();
    let rendered = previews
        .iter()
        .map(|preview| LintFixPreview {
            source: preview.source.clone(),
            original: sources
                .and_then(|sources| sources.get(&preview.source))
                .cloned()
                .unwrap_or_default(),
            new_text: preview.new_text.clone(),
        })
        .collect();
    Some((
        outcome(
            LintFixStatus::Preview,
            Some(rendered),
            validation.diagnostics,
        ),
        Some(ResolvedFix {
            transaction,
            previews,
        }),
    ))
}

/// The input's current text for fix validation — the same fresh resolve
/// `validate_transaction` performs, kept for the preview's before-text.
fn current_sources(config: &crate::config::SessionConfig) -> Option<BTreeMap<String, String>> {
    input::resolve(config)
        .ok()
        .map(|resolved| BTreeMap::from([(resolved.display, resolved.text)]))
}

/// Recompute an envelope's verdict after pushing diagnostics outside
/// `finish` — the lint pass had already packaged its diagnostics.
fn retune(envelope: &mut Envelope<LintResult>) {
    envelope.exit = exit_code_from(&envelope.diagnostics);
    envelope.ok = envelope.exit == crate::result::exit::SUCCESS;
}
