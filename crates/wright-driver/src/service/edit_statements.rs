//! `editStatements` (ADR-0029): the service-side planning and validation
//! of a structured statement-edit request — the owner-boundary helpers
//! live in [`crate::statement_edit`].

use wright_analyzer::canonical::SemanticIndex;

use super::{Address, StatementEditRequest, ToolErrorInfo, ToolResponse, ToolService};
use crate::CompilerSession;
use crate::diag::{Diagnostic, Stage};
use crate::edit::{EditTransaction, EditValidation};
use crate::result::exit_code_from;

impl<'a> ToolService<'a> {
    /// The `editStatements` input-kind gate (ADR-0029), evaluated before
    /// the snapshot refresh in `handle_full` so a non-Workshop input
    /// refuses with its provider-boundary diagnostic instead of the
    /// loader's. The refusal rides the result payload, the same shape
    /// `validateEditTransaction` reports refusals in.
    pub(crate) fn edit_statements_kind_gate(&mut self) -> Option<ToolResponse> {
        let refuse = |diagnostic: Diagnostic| {
            self.ok(
                serde_json::to_value(crate::statement_edit::refusal(vec![diagnostic]))
                    .expect("serializes"),
            )
        };
        let resolved = match crate::edit::resolve_edit_input(&self.session.config) {
            Ok(resolved) => resolved,
            Err(diagnostic) => return Some(refuse(diagnostic)),
        };
        crate::edit::edit_kind_gate(resolved.kind, "providerValidateEdit").map(refuse)
    }

    /// `editStatements` (ADR-0029): address the request's statement edits
    /// against the effective program — the caller's `sources` text loaded
    /// through the session's own Workshop path when supplied, the served
    /// snapshot otherwise — delegate content conversion and source-edit
    /// generation to `workshop-rs`, and validate the assembled
    /// transaction through `validateEditTransaction`. Nothing is written
    /// and the canonical program is never mutated.
    pub(crate) fn edit_statements(
        &mut self,
        input_identity: &str,
        sources: &Option<std::collections::BTreeMap<String, String>>,
        edits: &[StatementEditRequest],
    ) -> ToolResponse {
        match self.statement_edits(input_identity, sources, edits) {
            Ok(result) => self.ok(serde_json::to_value(result).expect("serializes")),
            Err(response) => response,
        }
    }

    /// Plan an `editStatements` request. Refusals that belong in the
    /// result payload (the `validateEditTransaction` shape) return `Ok`;
    /// addressing contract violations — `invalid-selection`,
    /// `unknown-rule`, `ambiguous-rule` — return the envelope `Err`.
    fn statement_edits(
        &mut self,
        input_identity: &str,
        sources: &Option<std::collections::BTreeMap<String, String>>,
        edits: &[StatementEditRequest],
    ) -> Result<crate::statement_edit::EditStatementsResult, ToolResponse> {
        use crate::statement_edit as se;
        let refuse = |diagnostics: Vec<Diagnostic>| Ok(se::refusal(diagnostics));
        let loaded = self.snapshot().loaded.clone();
        // The program the addresses name is the caller's `sources` text
        // when the request supplies it, else the served snapshot; the
        // edit `source` keeps the caller's own key spelling in the first
        // case and the input's display spelling in the second, the same
        // binding `validateEditTransaction` performs.
        let derived;
        let (effective, source) = match sources {
            Some(sources) => {
                let Some((key, text)) = se::supplied_source(sources, &loaded.input) else {
                    return refuse(vec![Diagnostic::error(
                        "edit-unknown-source",
                        Stage::Discovery,
                        format!(
                            "the request's 'sources' carries no text for the loaded input '{}'",
                            loaded.input.display
                        ),
                    )]);
                };
                let mut resolved = loaded.input.clone();
                resolved.text = text.to_string();
                resolved.identity = crate::input_identity(text);
                derived = match self.session.load_workshop_text(&resolved) {
                    Ok(derived) => derived,
                    Err(diagnostic) => return refuse(vec![diagnostic]),
                };
                (&derived, key.to_string())
            }
            None => (&loaded, loaded.input.display.clone()),
        };
        // `inputIdentity` is the freshness precondition: the identity of
        // the program the addresses were read from — `project`'s reported
        // value for the disk-loaded snapshot, the supplied text's identity
        // for caller-held sources.
        if input_identity != effective.input.identity {
            return refuse(vec![Diagnostic::error(
                "edit-stale-source",
                Stage::Discovery,
                "the request's inputIdentity does not match the program these addresses were read from — re-read `project` (or pass the current `sources` text) and retry",
            )]);
        }
        // Names resolve against the program the edits address: the served
        // semantic service for the snapshot, an index built for the
        // derived one. A numeric index bounds-checks against the same
        // program — `stale-id` already scoped it to its generation.
        let derived_index = sources
            .is_some()
            .then(|| SemanticIndex::build(effective.program.as_ref()));
        let mut seen = std::collections::HashSet::new();
        let mut resolved_edits = Vec::with_capacity(edits.len());
        for edit in edits {
            let rule = match &edit.rule {
                Address::Id(id) if (*id as usize) < effective.program.rules.len() => *id as usize,
                Address::Id(id) => {
                    return Err(ToolResponse::Error {
                        error: ToolErrorInfo::new(
                            "invalid-selection",
                            format!("unknown rule {id}"),
                        ),
                    });
                }
                Address::Name(name) => {
                    let resolved = match &derived_index {
                        Some(index) => index.resolve_rule(name),
                        None => self.semantic().resolve_rule(name),
                    };
                    match resolved {
                        Ok(rule) => rule,
                        // Candidates resolve against the effective program
                        // — the caller's `sources` file table, not the
                        // served snapshot's (#888).
                        Err(mut error) => {
                            crate::service::resolve_candidates(
                                &mut error.message,
                                &mut error.fields,
                                Some(effective),
                            );
                            return Err(ToolResponse::Error { error });
                        }
                    }
                }
            };
            if !seen.insert((rule, edit.list, edit.index)) {
                return refuse(vec![Diagnostic::error(
                    "edit-duplicate-address",
                    Stage::Discovery,
                    format!(
                        "two edits address the same statement: rule {rule} {} index {}",
                        edit.list.as_str(),
                        edit.index
                    ),
                )]);
            }
            resolved_edits.push((rule, edit));
        }
        // `workshop-rs` owns content conversion, canonical emission, and
        // the checked source edit. Each address names a position in the
        // program before any edit in the request — the edits it produces
        // are disjoint, so they assemble into one transaction.
        let locale = CompilerSession::locale_for(effective);
        let source_identity = crate::input_identity(&effective.input.text);
        let mut source_edits = Vec::with_capacity(resolved_edits.len());
        for (rule, edit) in &resolved_edits {
            let produced = effective
                .program
                .edit_statements(
                    self.session.catalog(),
                    &locale,
                    workshop_rs::rules::statement_edit::StatementAddress::new(
                        *rule,
                        se::owner_list(edit.list),
                        edit.index,
                    ),
                    se::owner_change(&edit.change),
                )
                .map_err(|error| se::owner_error(error, edit, *rule, &effective.input));
            let produced = match produced {
                Ok(produced) => produced,
                Err(diagnostic) => return refuse(vec![diagnostic]),
            };
            let Some(source_edit) = se::wright_edit(
                effective.program.as_ref(),
                *rule,
                &produced,
                &source,
                &source_identity,
            ) else {
                return refuse(vec![Diagnostic::error(
                    "source-unavailable",
                    Stage::Discovery,
                    format!("rule {rule} has no retained source"),
                )]);
            };
            source_edits.push(source_edit);
        }
        let transaction = match crate::edit::EditTransaction::new(source_edits) {
            Ok(transaction) => transaction,
            Err(diagnostic) => return refuse(vec![diagnostic]),
        };
        let validation = self
            .session
            .validate_edit_transaction(sources.as_ref(), &transaction);
        Ok(crate::statement_edit::EditStatementsResult {
            ok: validation.ok,
            exit: validation.exit,
            diagnostics: validation.diagnostics,
            transaction: Some(transaction),
            preview: validation.preview,
        })
    }

    /// The `wright ui` host write path (ADR-0032 decision 5 — host
    /// behaviour, not a `wright-agent/v1` operation): the transaction an
    /// `editStatements` result carried is re-validated against the files
    /// on disk — the freshness recheck — before the atomic write stages.
    /// A stale or invalid transaction refuses with diagnostics and writes
    /// nothing; a successful write refreshes the served snapshot so the
    /// next read sees the edited program. The caller never supplies
    /// `sources` here: the host always edits the disk-loaded input.
    pub fn apply_edit_transaction(&mut self, transaction: &EditTransaction) -> EditValidation {
        let refuse = |diagnostics: Vec<Diagnostic>| EditValidation {
            ok: false,
            exit: exit_code_from(&diagnostics),
            diagnostics,
            preview: None,
        };
        // With `sources` absent the validation re-reads the named files
        // from disk: every edit's `source_identity` — the `inputIdentity`
        // `editStatements` stamped — is checked against the current bytes
        // before anything writes, and the edited text is reparsed through
        // the session's own Workshop path.
        let validation = self.session.validate_edit_transaction(None, transaction);
        if !validation.ok {
            return validation;
        }
        let previews = validation.preview.clone().unwrap_or_default();
        // `write_previews` keys its expected identities by `edit.source`;
        // a transaction whose spellings do not cover a previewed source
        // refuses rather than panicking the host.
        if previews.iter().any(|preview| {
            !transaction
                .edits
                .iter()
                .any(|edit| edit.source == preview.source)
        }) {
            return refuse(vec![Diagnostic::error(
                "edit-unknown-source",
                Stage::Discovery,
                "the transaction's edits do not cover every source its preview writes",
            )]);
        }
        if let Err(diagnostic) = crate::edit::write_previews(&previews, transaction) {
            return refuse(vec![diagnostic]);
        }
        // The write changed the served input's fingerprint: refresh now
        // so the next program-reading request serves the edited program,
        // and surface a load failure here rather than on the next read.
        if let Err(error) = self.refresh() {
            let mut diagnostic =
                Diagnostic::carried_error(error.code, Stage::Discovery, error.message);
            diagnostic.fields = error.fields;
            return refuse(vec![diagnostic]);
        }
        validation
    }
}
