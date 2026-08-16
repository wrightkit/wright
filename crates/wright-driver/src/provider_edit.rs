//! Provider-driven source mutation (#139): language-specific semantic
//! decisions route through LPP capabilities while Wright keeps the generic
//! source-edit transaction guarantees.
//!
//! # The seam
//!
//! ```text
//! ToolService / agent-facing mutation requests
//!         |
//!         |  ProviderRenameRequest / ProviderValidateRequest (source-oriented,
//!         |  transport-neutral: documents + positions + current source texts)
//!         v
//!  provider_edit::semantic_rename / provider_edit::validate_transaction
//!         |
//!         |  lpp/rename            -- target resolution + edit generation
//!         |  lpp/validateEdits     -- per-document edit application + re-parse
//!         |  lpp/check             -- provider-owned project semantics
//!         v
//!  LanguageProvider (wright-lpp)   -- capability guards, typed LPP mapping
//! ```
//!
//! Wright owns everything below, exactly as the shared `edit` machinery
//! (`crate::edit`) defines it, and the guarantees are unchanged: edits carry
//! source identity/version preconditions, the transaction orders edits
//! deterministically and rejects overlap/order-dependent conflicts, preview
//! application is mechanical and atomic, and the caller decides whether (and
//! when) to apply anything to real files. The provider owns the
//! language-specific decisions: what symbol a position names, what a rename
//! edit set looks like, and whether the edited project is semantically valid.
//!
//! Refusals are structured and atomic: a provider refusal, an unsupported or
//! missing capability, a stale source/version, a failed semantic validation,
//! or a provider process failure all produce a [`ProviderMutation`] with
//! `ok = false`, no transaction, and no preview — never a partial edit set.
//! There is no fallback to textual search/replace or to Wright's own native
//! frontends when a semantic capability is unavailable.
//!
//! # Position conventions
//!
//! LPP positions and ranges use LSP conventions (0-based lines, 0-based
//! UTF-16 code-unit characters); Wright's [`crate::edit::EditRange`] uses
//! 1-based lines and 1-based character columns (end exclusive). This module
//! converts between the two against the document text, so the provider edits
//! are expressed in Wright's source-oriented edit contract with no loss of
//! precision.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::diag::{Diagnostic, Stage};
use crate::edit::{EditRange, EditTransaction, SourceEdit, SourcePreview};

/// The outcome of a provider-driven mutation flow (#139).
///
/// Atomicity contract: `ok = true` carries the validated transaction and its
/// previews; `ok = false` carries structured refusal diagnostics and **no**
/// transaction and **no** preview. `provider_code`/`provider_message` carry
/// the provider's machine code (a refusal code such as `rename.nameCollision`
/// or a stable failure code such as `capability-unavailable`) when the
/// failure came from the provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMutation {
    /// Whether the mutation is safe to apply.
    pub ok: bool,
    /// The validated exact-range transaction, when the flow succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<EditTransaction>,
    /// Structured refusal/validation diagnostics.
    pub diagnostics: Vec<Diagnostic>,
    /// The per-source previews of the validated transaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<Vec<SourcePreview>>,
    /// The provider's machine code when the failure came from the provider
    /// (a refusal code or a stable `wright_lpp::ProviderError` code), else
    /// `None`. Clients display the diagnostics and must not branch on
    /// unknown codes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_code: Option<String>,
    /// The provider's human-readable message, when the failure came from the
    /// provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_message: Option<String>,
}

/// A provider-driven semantic rename request (#139).
///
/// `documents` is the document set the provider computes against (text,
/// language id, version). `sources` is the caller's *current* text for every
/// source the mutation may touch, keyed by the same document URIs; the edits
/// carry the identity of the text they were computed against, so a caller
/// whose current text no longer matches the snapshot is refused as stale
/// instead of silently applying to changed sources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRenameRequest {
    /// The document set the rename is computed against (the provider's view).
    pub documents: wright_lpp::DocumentSet,
    /// The URI (a key of `documents`) in which `position` is interpreted.
    pub position_document_uri: String,
    /// The position of the symbol to rename (0-based LSP conventions).
    pub position: wright_lpp::Position,
    /// The new name; the provider validates it against the language's
    /// identifier rules.
    pub new_name: String,
    /// The project the documents belong to (informational; providers may use
    /// it for project-aware semantics).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    /// The caller's current text for every source the rename may edit,
    /// keyed by document URI (the identity/version precondition view).
    pub sources: BTreeMap<String, String>,
}

/// A provider-driven edit-validation request (#139): validate a
/// caller-proposed source-edit transaction against the provider's project
/// semantics before the caller applies anything.
///
/// The transaction must be a Wright [`EditTransaction`] whose edits carry
/// document URIs as `source` identities and the identity of the text they
/// were computed against; `documents` supplies the provider's view of the
/// unmodified project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderValidateRequest {
    /// The unmodified project as the provider sees it.
    pub documents: wright_lpp::DocumentSet,
    /// The caller-proposed transaction (Wright-owned edit contract).
    pub transaction: EditTransaction,
    /// The caller's current text for every edited source, keyed by document
    /// URI (the identity/version precondition view).
    pub sources: BTreeMap<String, String>,
    /// The project the documents belong to (informational).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
}

/// The provider's machine code and human-readable message, preserved on a
/// refusal that came from the provider.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderInfo {
    code: String,
    message: String,
}

/// A structured refusal with optional provider failure metadata.
struct Refusal {
    diagnostics: Vec<Diagnostic>,
    provider: Option<ProviderInfo>,
}

impl Refusal {
    /// The atomic [`ProviderMutation`] form of this refusal: no transaction,
    /// no preview.
    fn into_mutation(self) -> ProviderMutation {
        let (provider_code, provider_message) = match self.provider {
            Some(info) => (Some(info.code), Some(info.message)),
            None => (None, None),
        };
        ProviderMutation {
            ok: false,
            transaction: None,
            diagnostics: self.diagnostics,
            preview: None,
            provider_code,
            provider_message,
        }
    }
}

/// A provider-driven semantic rename (#139).
///
/// Routes target resolution and edit generation through the LPP `rename`
/// capability, wraps the provider's source-oriented edit set in Wright's own
/// [`EditTransaction`] (deterministic ordering, overlap/conflict checks, and
/// identity/version preconditions), previews it atomically, and validates it
/// against the provider's project semantics (`lpp/validateEdits` per edited
/// document, then `lpp/check` over the edited project) before success.
///
/// `provider` must be an initialized `LanguageProvider` session. Every
/// failure — a provider refusal, an unsupported capability, a stale
/// source/version, a failed semantic validation, or a provider process
/// failure — is a structured [`ProviderMutation`] refusal with no
/// transaction and no preview.
pub fn semantic_rename(
    provider: &mut dyn wright_lpp::LanguageProvider,
    request: &ProviderRenameRequest,
) -> ProviderMutation {
    let result = match provider.rename(
        &request.documents,
        &request.position_document_uri,
        request.position,
        &request.new_name,
        request.project_root.as_deref(),
    ) {
        Ok(result) => result,
        Err(error) => return provider_failure(&error),
    };

    // Adapter: the provider's per-document edit set becomes Wright source
    // edits carrying the identity of the text they were computed against.
    // A provider that edits a document outside the request set, or echoes a
    // version different from the one the caller sent, is a stale/contract
    // violation and refuses.
    let mut edits: Vec<SourceEdit> = Vec::new();
    for document_edits in &result.edits {
        let Some(document) = request.documents.get(&document_edits.document_uri) else {
            return refusal(
                vec![Diagnostic::error(
                    "provider-edit-outside-set",
                    Stage::Discovery,
                    format!(
                        "the provider returned edits for '{}', which is not in the request document set",
                        document_edits.document_uri
                    ),
                )],
                None,
            );
        };
        if document_edits.version != document.version {
            return refusal(
                vec![Diagnostic::error(
                    "edit-stale-source",
                    Stage::Discovery,
                    format!(
                        "the provider computed edits for '{}' against version {}, but the current version is {}; re-fetch the source and retry",
                        document_edits.document_uri, document_edits.version, document.version
                    ),
                )],
                None,
            );
        }
        let identity = crate::input_identity(&document.text);
        for text_edit in &document_edits.text_edits {
            let range = match to_edit_range(&document.text, text_edit.range) {
                Ok(range) => range,
                Err(diagnostic) => return refusal(vec![diagnostic], None),
            };
            edits.push(SourceEdit {
                edit_kind: "rename".to_string(),
                source: document.uri.clone(),
                source_identity: identity.clone(),
                range,
                new_text: text_edit.new_text.clone(),
            });
        }
    }

    finish(
        provider,
        &request.documents,
        edits,
        &request.sources,
        request.project_root.as_deref(),
    )
}

/// Validate a caller-proposed source-edit transaction against the provider's
/// project semantics (#139).
///
/// Re-asserts the Wright-owned transaction guarantees (deterministic
/// ordering, overlap/conflict checks, identity/version preconditions),
/// previews the transaction atomically, and runs the provider's semantic
/// gates (`lpp/validateEdits` per edited document, then `lpp/check` over the
/// edited project) before reporting success. A transaction that is stale,
/// malformed, or fails provider validation is refused with no partial edit
/// set.
pub fn validate_transaction(
    provider: &mut dyn wright_lpp::LanguageProvider,
    request: &ProviderValidateRequest,
) -> ProviderMutation {
    finish(
        provider,
        &request.documents,
        request.transaction.edits.clone(),
        &request.sources,
        request.project_root.as_deref(),
    )
}

/// A structured refusal for a provider failure: the provider's machine code
/// and message are preserved on the mutation for callers that surface
/// machine-readable refusals.
pub fn provider_failure(error: &wright_lpp::ProviderError) -> ProviderMutation {
    let provider = ProviderInfo {
        code: match error.refusal_code() {
            Some(code) => code.to_string(),
            None => error.code().to_string(),
        },
        message: error.to_string(),
    };
    refusal(vec![provider_diagnostic(error)], Some(provider))
}

/// The structured diagnostic for a provider failure: refusals and other
/// failures are distinct codes, with the provider's message preserved.
fn provider_diagnostic(error: &wright_lpp::ProviderError) -> Diagnostic {
    match error.refusal_code() {
        Some(code) => Diagnostic::error(
            "provider-refusal",
            Stage::Discovery,
            format!("the provider refused the request ({code}): {error}"),
        ),
        None => Diagnostic::error(
            "provider-error",
            Stage::Discovery,
            format!(
                "the provider failed the request ({}): {error}",
                error.code()
            ),
        ),
    }
}

/// The shared Wright-owned pipeline for both flows: transaction construction
/// and checks, identity/version preconditions, atomic preview, then the
/// provider-owned semantic validation gates.
fn finish(
    provider: &mut dyn wright_lpp::LanguageProvider,
    documents: &wright_lpp::DocumentSet,
    edits: Vec<SourceEdit>,
    sources: &BTreeMap<String, String>,
    project_root: Option<&str>,
) -> ProviderMutation {
    // Wright-owned transaction guarantees: deterministic ordering, overlap
    // and order-dependent zero-width conflict detection, non-empty.
    let transaction = match EditTransaction::new(edits) {
        Ok(transaction) => transaction,
        Err(diagnostic) => return refusal(vec![diagnostic], None),
    };

    // Wright-owned preconditions: every edited source must be known and
    // current, so a stale or fabricated version can never apply.
    if let Some(diagnostic) = precondition_problems(&transaction, sources) {
        return refusal(vec![diagnostic], None);
    }

    // Wright-owned mechanical application: the atomic preview every source
    // the transaction touches, against the caller's current texts.
    let preview = match transaction.apply(sources) {
        Ok(preview) => preview,
        Err(diagnostic) => return refusal(vec![diagnostic], None),
    };

    // Provider-owned semantic validation: per-document edit validation and
    // project-aware check of the edited project. A failed gate refuses with
    // no transaction and no preview.
    match provider_validation(provider, documents, &transaction, &preview, project_root) {
        Ok(()) => ProviderMutation {
            ok: true,
            transaction: Some(transaction),
            diagnostics: Vec::new(),
            preview: Some(preview),
            provider_code: None,
            provider_message: None,
        },
        Err(refusal) => refusal.into_mutation(),
    }
}

/// The identity/version precondition check, mirroring the codes of the
/// shared `crate::edit::validate_transaction`: every edited source must have
/// current text supplied and its identity must match the identity the edits
/// were computed against.
fn precondition_problems(
    transaction: &EditTransaction,
    sources: &BTreeMap<String, String>,
) -> Option<Diagnostic> {
    for edit in &transaction.edits {
        let Some(current) = sources.get(&edit.source) else {
            return Some(Diagnostic::error(
                "edit-unknown-source",
                Stage::Discovery,
                format!(
                    "the edit targets '{}' but no current text was provided for it; \
                     supply the current source so the version precondition can be verified",
                    edit.source
                ),
            ));
        };
        if crate::input_identity(current) != edit.source_identity {
            return Some(Diagnostic::error(
                "edit-stale-source",
                Stage::Discovery,
                format!(
                    "the edit for '{}' targets a different source version (identity mismatch); \
                     re-fetch the source and retry",
                    edit.source
                ),
            ));
        }
    }
    None
}

/// The provider-owned semantic validation gates (#139).
///
/// Gate 1: `lpp/validateEdits` per edited document — the provider applies
/// the edit set under the LPP v1 normative rules (bounds, ordering, overlap,
/// application, re-parse) and reports whether the result is well-formed.
///
/// Gate 2: `lpp/check` over the edited project — the provider's project
/// semantics over the full document set with the edited texts applied
/// (edited versions bump by one, per the LPP client bookkeeping rule). Any
/// error-severity diagnostic refuses.
///
/// Both gates are mandatory: a provider that did not negotiate the
/// `editValidation` or `check` capability cannot semantically validate the
/// mutation, and the flow refuses explicitly — there is no silent fallback.
fn provider_validation(
    provider: &mut dyn wright_lpp::LanguageProvider,
    documents: &wright_lpp::DocumentSet,
    transaction: &EditTransaction,
    previews: &[SourcePreview],
    project_root: Option<&str>,
) -> Result<(), Refusal> {
    // Gate 1: per-document edit validation.
    let mut grouped: BTreeMap<&str, Vec<&SourceEdit>> = BTreeMap::new();
    for edit in &transaction.edits {
        grouped.entry(edit.source.as_str()).or_default().push(edit);
    }
    for (source, edits) in grouped {
        let Some(document) = documents.get(source) else {
            return Err(Refusal {
                diagnostics: vec![Diagnostic::error(
                    "edit-unknown-source",
                    Stage::Discovery,
                    format!(
                        "the transaction targets '{}' but the request document set has no current text for it",
                        source
                    ),
                )],
                provider: None,
            });
        };
        let mut text_edits = Vec::new();
        for edit in edits {
            match to_text_edit(&document.text, edit) {
                Ok(text_edit) => text_edits.push(text_edit),
                Err(diagnostic) => {
                    return Err(Refusal {
                        diagnostics: vec![diagnostic],
                        provider: None,
                    });
                }
            }
        }
        let result = match provider.validate_edits(document, &text_edits) {
            Ok(result) => result,
            Err(error) => return Err(provider_failure_refusal(&error)),
        };
        if result.version != document.version {
            return Err(Refusal {
                diagnostics: vec![Diagnostic::error(
                    "edit-stale-source",
                    Stage::Discovery,
                    format!(
                        "the provider validated edits for '{}' against version {}, but the current version is {}; re-fetch the source and retry",
                        source, result.version, document.version
                    ),
                )],
                provider: None,
            });
        }
        if !result.valid {
            let reason = result.reason.as_deref().unwrap_or("invalid");
            let mut message = format!(
                "the provider refused the edits for '{source}': the edited source is {reason}"
            );
            if let Some(index) = result.failing_edit_index {
                message.push_str(&format!(" (failing edit {index})"));
            }
            return Err(Refusal {
                diagnostics: vec![Diagnostic::error(
                    "provider-validation-failed",
                    Stage::Discovery,
                    message,
                )],
                provider: None,
            });
        }
    }

    // Gate 2: provider-owned project semantics over the edited project.
    let mut edited = wright_lpp::DocumentSet::new();
    for (uri, document) in documents {
        let edited_text = previews
            .iter()
            .find(|preview| preview.source == *uri)
            .map(|preview| preview.new_text.clone());
        let text = edited_text.clone().unwrap_or_else(|| document.text.clone());
        edited.insert(
            uri.clone(),
            wright_lpp::Document {
                uri: uri.clone(),
                language_id: document.language_id.clone(),
                version: if edited_text.is_some() {
                    document.version + 1
                } else {
                    document.version
                },
                text,
            },
        );
    }
    let checked = match provider.check(&edited, project_root) {
        Ok(checked) => checked,
        Err(error) => return Err(provider_failure_refusal(&error)),
    };
    for document in &checked.documents {
        for diagnostic in &document.diagnostics {
            if diagnostic.severity == wright_lpp::DiagnosticSeverity::Error {
                return Err(Refusal {
                    diagnostics: vec![Diagnostic::error(
                        "provider-semantic-error",
                        Stage::Discovery,
                        format!(
                            "the edited project is semantically invalid in '{}': {}",
                            document.uri, diagnostic.message
                        ),
                    )],
                    provider: None,
                });
            }
        }
    }
    Ok(())
}

/// The refusal parts for a provider failure during a validation gate.
fn provider_failure_refusal(error: &wright_lpp::ProviderError) -> Refusal {
    Refusal {
        diagnostics: vec![provider_diagnostic(error)],
        provider: Some(ProviderInfo {
            code: match error.refusal_code() {
                Some(code) => code.to_string(),
                None => error.code().to_string(),
            },
            message: error.to_string(),
        }),
    }
}

/// An atomic refusal: structured diagnostics and no transaction/preview.
fn refusal(diagnostics: Vec<Diagnostic>, provider: Option<ProviderInfo>) -> ProviderMutation {
    let (provider_code, provider_message) = match provider {
        Some(info) => (Some(info.code), Some(info.message)),
        None => (None, None),
    };
    ProviderMutation {
        ok: false,
        transaction: None,
        diagnostics,
        preview: None,
        provider_code,
        provider_message,
    }
}

/// Convert an LPP range (0-based line, 0-based UTF-16 code units) into a
/// Wright [`EditRange`] (1-based line, 1-based character columns, end
/// exclusive), against the document text the range addresses.
///
/// A position inside a supplementary-plane character, or outside the
/// document, is not a valid range endpoint and refuses explicitly.
fn to_edit_range(text: &str, range: wright_lpp::Range) -> Result<EditRange, Diagnostic> {
    let lines: Vec<&str> = text.split('\n').collect();
    let start = position_to_column(&lines, range.start, "range start")?;
    let end = position_to_column(&lines, range.end, "range end")?;
    if (start.0, start.1) > (end.0, end.1) {
        return Err(edit_invalid_range(range));
    }
    Ok(EditRange {
        start_line: start.0,
        start_col: start.1,
        end_line: end.0,
        end_col: end.1,
    })
}

/// Convert a Wright [`EditRange`] back into an LPP [`wright_lpp::TextEdit`]
/// (0-based UTF-16 code units), against the document text the edit
/// addresses. Columns are validated strictly: a column of 0 or beyond the
/// line end is refused, never clamped.
fn to_text_edit(text: &str, edit: &SourceEdit) -> Result<wright_lpp::TextEdit, Diagnostic> {
    let lines: Vec<&str> = text.split('\n').collect();
    let start = column_to_position(&lines, edit.range.start_line, edit.range.start_col)?;
    let end = column_to_position(&lines, edit.range.end_line, edit.range.end_col)?;
    Ok(wright_lpp::TextEdit {
        range: wright_lpp::Range { start, end },
        new_text: edit.new_text.clone(),
    })
}

/// An LPP position (0-based line, 0-based UTF-16 units) as a 1-based
/// (line, character column) pair.
fn position_to_column(
    lines: &[&str],
    position: wright_lpp::Position,
    what: &str,
) -> Result<(u32, u32), Diagnostic> {
    let line = position.line as usize;
    let Some(line_text) = lines.get(line) else {
        return Err(Diagnostic::error(
            "edit-invalid-range",
            Stage::Discovery,
            format!(
                "the {what} names line {}, but the document has {} lines",
                position.line,
                lines.len()
            ),
        ));
    };
    let column = utf16_to_char_column(line_text, position.character).ok_or_else(|| {
        Diagnostic::error(
            "edit-invalid-range",
            Stage::Discovery,
            format!(
                "the {what} names UTF-16 offset {} in line {}, which is not a character boundary of that line",
                position.character, position.line
            ),
        )
    })?;
    Ok(((line as u32) + 1, column))
}

/// A Wright 1-based (line, column) pair as an LPP position (0-based line,
/// 0-based UTF-16 units).
fn column_to_position(
    lines: &[&str],
    line: u32,
    column: u32,
) -> Result<wright_lpp::Position, Diagnostic> {
    let line_index = line.saturating_sub(1) as usize;
    let Some(line_text) = lines.get(line_index) else {
        return Err(Diagnostic::error(
            "edit-invalid-range",
            Stage::Discovery,
            format!(
                "the edit names line {line}, but the document has {} lines",
                lines.len()
            ),
        ));
    };
    let character = char_column_to_utf16(line_text, column).ok_or_else(|| {
        Diagnostic::error(
            "edit-invalid-range",
            Stage::Discovery,
            format!(
                "the edit names column {column} in line {line}, which is outside that line \
                 (line {line} has {} characters)",
                line_text.chars().count()
            ),
        )
    })?;
    Ok(wright_lpp::Position {
        line: line_index as u32,
        character,
    })
}

/// A 0-based UTF-16 code-unit offset within a line as a 1-based character
/// column (a position at the end of a line is valid). `None` when the offset
/// is inside a supplementary-plane character or beyond the line end.
fn utf16_to_char_column(line: &str, units: u32) -> Option<u32> {
    let mut accumulated = 0u32;
    let mut column = 1u32;
    for ch in line.chars() {
        if accumulated == units {
            return Some(column);
        }
        let len = ch.len_utf16() as u32;
        if accumulated + len > units {
            // Inside a supplementary-plane character: not a valid boundary.
            return None;
        }
        accumulated += len;
        column += 1;
    }
    (accumulated == units).then_some(column)
}

/// A 1-based character column as a 0-based UTF-16 code-unit offset within a
/// line. `None` when the column is 0 or beyond the line end (the end of the
/// line is a valid column: `chars + 1`).
fn char_column_to_utf16(line: &str, column: u32) -> Option<u32> {
    let mut accumulated = 0u32;
    for (index, ch) in line.chars().enumerate() {
        let this_column = (index as u32) + 1;
        if this_column == column {
            return Some(accumulated);
        }
        accumulated += ch.len_utf16() as u32;
    }
    (column as usize == line.chars().count() + 1).then_some(accumulated)
}

/// The generic invalid-range refusal for an LPP range that could not be
/// converted.
fn edit_invalid_range(range: wright_lpp::Range) -> Diagnostic {
    Diagnostic::error(
        "edit-invalid-range",
        Stage::Discovery,
        format!(
            "the provider edit range {}-{}:{}-{} is not a valid range of the document",
            range.start.line, range.start.character, range.end.line, range.end.character
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use wright_lpp::{
        CheckResult, ClientInfo, Document, DocumentSet, InitializeResult, LocationsResult,
        NegotiatedCapabilities, ProviderError, ReconstructResult, RenameResult, SymbolsResult,
        ValidateEditsResult, WorkshopArtifact,
    };

    const URI: &str = "file:///project/puzzle.xdl";
    const CLEAN: &str = "puzzle clean {\n  target = 40\n  start = 10\n  ops {\n    double: x => x * 2\n    plus1: x => x + 1\n  }\n  solution = [ double, double ]\n}";

    fn document(text: &str) -> Document {
        Document {
            uri: URI.to_string(),
            language_id: "x-demo-lang".to_string(),
            version: 3,
            text: text.to_string(),
        }
    }

    fn document_set() -> DocumentSet {
        let mut documents = DocumentSet::new();
        documents.insert(URI.to_string(), document(CLEAN));
        documents
    }

    fn sources(text: &str) -> BTreeMap<String, String> {
        let mut sources = BTreeMap::new();
        sources.insert(URI.to_string(), text.to_string());
        sources
    }

    fn rename_request() -> ProviderRenameRequest {
        ProviderRenameRequest {
            documents: document_set(),
            position_document_uri: URI.to_string(),
            position: wright_lpp::Position {
                line: 4,
                character: 6,
            },
            new_name: "twice".to_string(),
            project_root: Some("file:///project".to_string()),
            sources: sources(CLEAN),
        }
    }

    /// The mock provider's rename edit set for `CLEAN` (declaration plus
    /// both references), as the wire shape describes it.
    fn clean_rename_result() -> RenameResult {
        let text_edit = |line, start, end| wright_lpp::TextEdit {
            range: wright_lpp::Range {
                start: wright_lpp::Position {
                    line,
                    character: start,
                },
                end: wright_lpp::Position {
                    line,
                    character: end,
                },
            },
            new_text: "twice".to_string(),
        };
        RenameResult {
            edits: vec![wright_lpp::DocumentEdits {
                document_uri: URI.to_string(),
                version: 3,
                text_edits: vec![
                    text_edit(4, 4, 10),
                    text_edit(7, 15, 21),
                    text_edit(7, 23, 29),
                ],
            }],
        }
    }

    /// A scripted `LanguageProvider` for flow unit tests: the requested
    /// methods return canned results, everything else is unreachable.
    struct ScriptedProvider {
        rename: Result<RenameResult, ProviderError>,
        validate_edits: Result<ValidateEditsResult, ProviderError>,
        check: Result<CheckResult, ProviderError>,
    }

    impl wright_lpp::LanguageProvider for ScriptedProvider {
        fn initialize(
            &mut self,
            _client_info: Option<&ClientInfo>,
        ) -> Result<InitializeResult, ProviderError> {
            unreachable!("not used by flow unit tests")
        }
        fn capabilities(&self) -> Result<&NegotiatedCapabilities, ProviderError> {
            unreachable!("not used by flow unit tests")
        }
        fn check(
            &mut self,
            _documents: &DocumentSet,
            _project_root: Option<&str>,
        ) -> Result<CheckResult, ProviderError> {
            self.check.clone()
        }
        fn compile(
            &mut self,
            _documents: &DocumentSet,
            _project_root: Option<&str>,
        ) -> Result<wright_lpp::CompileResult, ProviderError> {
            unreachable!("not used by flow unit tests")
        }
        fn reconstruct(
            &mut self,
            _artifact: &WorkshopArtifact,
        ) -> Result<ReconstructResult, ProviderError> {
            unreachable!("not used by flow unit tests")
        }
        fn symbols(
            &mut self,
            _documents: &DocumentSet,
            _project_root: Option<&str>,
        ) -> Result<SymbolsResult, ProviderError> {
            unreachable!("not used by flow unit tests")
        }
        fn definition(
            &mut self,
            _document: &Document,
            _position: wright_lpp::Position,
        ) -> Result<LocationsResult, ProviderError> {
            unreachable!("not used by flow unit tests")
        }
        fn references(
            &mut self,
            _document: &Document,
            _position: wright_lpp::Position,
            _include_declaration: bool,
        ) -> Result<LocationsResult, ProviderError> {
            unreachable!("not used by flow unit tests")
        }
        fn rename(
            &mut self,
            _documents: &DocumentSet,
            _position_document_uri: &str,
            _position: wright_lpp::Position,
            _new_name: &str,
            _project_root: Option<&str>,
        ) -> Result<RenameResult, ProviderError> {
            self.rename.clone()
        }
        fn validate_edits(
            &mut self,
            _document: &Document,
            _edits: &[wright_lpp::TextEdit],
        ) -> Result<ValidateEditsResult, ProviderError> {
            self.validate_edits.clone()
        }
        fn shutdown(&mut self) -> Result<(), ProviderError> {
            unreachable!("not used by flow unit tests")
        }
        fn exit_status(&self) -> Option<i32> {
            None
        }
    }

    fn ok_check() -> CheckResult {
        CheckResult { documents: vec![] }
    }

    // -----------------------------------------------------------------------
    // Position conversion
    // -----------------------------------------------------------------------

    #[test]
    fn utf16_ranges_convert_to_wright_columns_and_back() {
        // "puzzle αβ {": α and β are 2-byte/1-UTF-16-unit characters. The
        // mock provider's own unit tests pin UTF-16 offsets 7 (α) and 8 (β).
        let text = "puzzle αβ {\n  target = 40\n}";
        let lpp = wright_lpp::Range {
            start: wright_lpp::Position {
                line: 0,
                character: 7,
            },
            end: wright_lpp::Position {
                line: 0,
                character: 9,
            },
        };
        let wright = to_edit_range(text, lpp).expect("converts");
        assert_eq!(
            wright,
            EditRange {
                start_line: 1,
                start_col: 8,
                end_line: 1,
                end_col: 10,
            }
        );
        // Round trip: the Wright range converts back to the same UTF-16
        // offsets.
        let back = to_text_edit(
            text,
            &SourceEdit {
                edit_kind: "rename".to_string(),
                source: URI.to_string(),
                source_identity: crate::input_identity(text),
                range: wright,
                new_text: "x".to_string(),
            },
        )
        .expect("converts back");
        assert_eq!(back.range, lpp);
    }

    #[test]
    fn utf16_offset_inside_a_supplementary_character_refuses() {
        // "𝕏" is one character but two UTF-16 code units; offset 1 is
        // inside it.
        let text = "puzzle 𝕏 {";
        let error = to_edit_range(
            text,
            wright_lpp::Range {
                start: wright_lpp::Position {
                    line: 0,
                    character: 7,
                },
                end: wright_lpp::Position {
                    line: 0,
                    character: 8,
                },
            },
        )
        .expect_err("inside a supplementary character");
        assert_eq!(error.code, "edit-invalid-range");
    }

    #[test]
    fn position_at_end_of_line_is_valid() {
        let text = "ab\ncd";
        let wright = to_edit_range(
            text,
            wright_lpp::Range {
                start: wright_lpp::Position {
                    line: 0,
                    character: 2,
                },
                end: wright_lpp::Position {
                    line: 0,
                    character: 2,
                },
            },
        )
        .expect("end of line is a valid insertion point");
        assert_eq!(wright.start_line, 1);
        assert_eq!(wright.start_col, 3);
    }

    // -----------------------------------------------------------------------
    // Flow behavior with a scripted provider
    // -----------------------------------------------------------------------

    #[test]
    fn rename_wraps_provider_edits_in_a_wright_transaction() {
        let mut provider = ScriptedProvider {
            rename: Ok(clean_rename_result()),
            validate_edits: Ok(ValidateEditsResult {
                valid: true,
                version: 3,
                reason: None,
                failing_edit_index: None,
            }),
            check: Ok(ok_check()),
        };
        let mutation = semantic_rename(&mut provider, &rename_request());
        assert!(mutation.ok, "rename succeeds: {:?}", mutation.diagnostics);
        let transaction = mutation.transaction.expect("transaction");
        assert_eq!(transaction.edits.len(), 3);
        assert!(
            transaction
                .edits
                .iter()
                .all(|edit| edit.edit_kind == "rename"),
            "provider edits arrive as Wright rename edits"
        );
        assert!(
            transaction
                .edits
                .iter()
                .all(|edit| edit.source_identity == crate::input_identity(CLEAN)),
            "every edit carries the identity precondition of the text it was computed against"
        );
        // Deterministic ordering: declaration first, then the two
        // references, all in one document.
        assert_eq!(transaction.edits[0].range.start_line, 5);
        assert_eq!(transaction.edits[1].range.start_line, 8);
        assert_eq!(transaction.edits[2].range.start_line, 8);
        let preview = mutation.preview.expect("preview");
        assert_eq!(preview.len(), 1);
        assert!(preview[0].new_text.contains("twice: x => x * 2"));
        assert!(preview[0].new_text.contains("solution = [ twice, twice ]"));
    }

    #[test]
    fn rename_edits_are_never_outside_the_request_document_set() {
        let mut result = clean_rename_result();
        result.edits[0].document_uri = "file:///project/other.xdl".to_string();
        let mut provider = ScriptedProvider {
            rename: Ok(result),
            validate_edits: Ok(ValidateEditsResult {
                valid: true,
                version: 3,
                reason: None,
                failing_edit_index: None,
            }),
            check: Ok(ok_check()),
        };
        let mutation = semantic_rename(&mut provider, &rename_request());
        assert!(!mutation.ok);
        assert_eq!(mutation.diagnostics[0].code, "provider-edit-outside-set");
        assert!(mutation.transaction.is_none());
        assert!(mutation.preview.is_none());
    }

    #[test]
    fn rename_version_echo_mismatch_is_a_stale_refusal() {
        let mut result = clean_rename_result();
        result.edits[0].version = 2;
        let mut provider = ScriptedProvider {
            rename: Ok(result),
            validate_edits: Ok(ValidateEditsResult {
                valid: true,
                version: 3,
                reason: None,
                failing_edit_index: None,
            }),
            check: Ok(ok_check()),
        };
        let mutation = semantic_rename(&mut provider, &rename_request());
        assert!(!mutation.ok);
        assert_eq!(mutation.diagnostics[0].code, "edit-stale-source");
        assert!(mutation.transaction.is_none());
        assert!(mutation.preview.is_none());
    }

    #[test]
    fn stale_current_sources_refuse_without_a_partial_edit_set() {
        // The caller's current text no longer matches the snapshot the
        // provider computed against: the identity precondition refuses.
        let mut request = rename_request();
        request.sources = sources(&format!("{CLEAN}\n"));
        let mut provider = ScriptedProvider {
            rename: Ok(clean_rename_result()),
            validate_edits: Ok(ValidateEditsResult {
                valid: true,
                version: 3,
                reason: None,
                failing_edit_index: None,
            }),
            check: Ok(ok_check()),
        };
        let mutation = semantic_rename(&mut provider, &request);
        assert!(!mutation.ok);
        assert_eq!(mutation.diagnostics[0].code, "edit-stale-source");
        assert!(mutation.transaction.is_none());
        assert!(mutation.preview.is_none());
    }

    #[test]
    fn provider_failure_mid_rename_refuses_without_partial_application() {
        // The provider computes the rename but dies before the flow
        // completes: the refusal is structured and nothing is applied.
        let mut provider = ScriptedProvider {
            rename: Ok(clean_rename_result()),
            validate_edits: Err(ProviderError::Exited {
                status: Some(3),
                message: "the LPP provider process exited".to_string(),
            }),
            check: Ok(ok_check()),
        };
        let mutation = semantic_rename(&mut provider, &rename_request());
        assert!(!mutation.ok);
        assert_eq!(mutation.diagnostics[0].code, "provider-error");
        assert_eq!(mutation.provider_code.as_deref(), Some("provider-exited"));
        assert!(mutation.transaction.is_none());
        assert!(mutation.preview.is_none());
    }

    #[test]
    fn rename_refusal_passes_through_the_provider_code() {
        let mut provider = ScriptedProvider {
            rename: Err(ProviderError::lpp(
                wright_lpp::LppErrorKind::Refusal,
                serde_json::json!({ "refusalCode": "rename.nameCollision" }),
                "new name collides with an existing symbol",
            )),
            validate_edits: Ok(ValidateEditsResult {
                valid: true,
                version: 3,
                reason: None,
                failing_edit_index: None,
            }),
            check: Ok(ok_check()),
        };
        let mutation = semantic_rename(&mut provider, &rename_request());
        assert!(!mutation.ok);
        assert_eq!(mutation.diagnostics[0].code, "provider-refusal");
        assert_eq!(
            mutation.provider_code.as_deref(),
            Some("rename.nameCollision")
        );
        assert!(mutation.transaction.is_none());
        assert!(mutation.preview.is_none());
    }

    #[test]
    fn unsupported_capability_is_an_explicit_refusal_not_a_fallback() {
        let mut provider = ScriptedProvider {
            rename: Err(ProviderError::lpp(
                wright_lpp::LppErrorKind::CapabilityUnavailable,
                serde_json::json!({
                    "capability": "rename",
                    "method": "lpp/rename",
                }),
                "capability 'rename' is not available in this session",
            )),
            validate_edits: Ok(ValidateEditsResult {
                valid: true,
                version: 3,
                reason: None,
                failing_edit_index: None,
            }),
            check: Ok(ok_check()),
        };
        let mutation = semantic_rename(&mut provider, &rename_request());
        assert!(!mutation.ok);
        assert_eq!(
            mutation.provider_code.as_deref(),
            Some("capability-unavailable")
        );
        assert!(mutation.transaction.is_none());
        assert!(mutation.preview.is_none());
    }

    #[test]
    fn semantic_validation_failure_refuses_without_partial_application() {
        // The provider accepts the rename but its own semantic validation of
        // the edited document fails (valid = false): no transaction, no
        // preview.
        let mut provider = ScriptedProvider {
            rename: Ok(clean_rename_result()),
            validate_edits: Ok(ValidateEditsResult {
                valid: false,
                version: 3,
                reason: Some("syntaxError".to_string()),
                failing_edit_index: None,
            }),
            check: Ok(ok_check()),
        };
        let mutation = semantic_rename(&mut provider, &rename_request());
        assert!(!mutation.ok);
        assert_eq!(mutation.diagnostics[0].code, "provider-validation-failed");
        assert!(mutation.transaction.is_none());
        assert!(mutation.preview.is_none());
    }

    #[test]
    fn edited_project_check_refuses_on_error_severity_diagnostics() {
        // Gate 2: the provider's project-aware check reports an error in the
        // edited project, so the mutation refuses atomically.
        let mut provider = ScriptedProvider {
            rename: Ok(clean_rename_result()),
            validate_edits: Ok(ValidateEditsResult {
                valid: true,
                version: 3,
                reason: None,
                failing_edit_index: None,
            }),
            check: Ok(CheckResult {
                documents: vec![wright_lpp::DocumentDiagnostics {
                    uri: URI.to_string(),
                    version: 4,
                    diagnostics: vec![wright_lpp::Diagnostic {
                        range: wright_lpp::Range {
                            start: wright_lpp::Position {
                                line: 7,
                                character: 15,
                            },
                            end: wright_lpp::Position {
                                line: 7,
                                character: 21,
                            },
                        },
                        severity: wright_lpp::DiagnosticSeverity::Error,
                        code: Some("x-demo/unresolved-op".to_string()),
                        message: "unresolved op reference 'twice'".to_string(),
                        source: Some("x-demo-lang".to_string()),
                    }],
                }],
            }),
        };
        let mutation = semantic_rename(&mut provider, &rename_request());
        assert!(!mutation.ok);
        assert_eq!(mutation.diagnostics[0].code, "provider-semantic-error");
        assert!(mutation.transaction.is_none());
        assert!(mutation.preview.is_none());
    }

    #[test]
    fn validate_transaction_runs_the_provider_gates_on_a_caller_transaction() {
        // The caller proposes a Wright transaction; the flow re-asserts the
        // transaction guarantees and runs the provider's semantic gates.
        let transaction = EditTransaction::new(vec![SourceEdit {
            edit_kind: "rename".to_string(),
            source: URI.to_string(),
            source_identity: crate::input_identity(CLEAN),
            range: EditRange {
                start_line: 5,
                start_col: 5,
                end_line: 5,
                end_col: 11,
            },
            new_text: "twice".to_string(),
        }])
        .expect("transaction");
        let request = ProviderValidateRequest {
            documents: document_set(),
            transaction,
            sources: sources(CLEAN),
            project_root: None,
        };
        let mut provider = ScriptedProvider {
            rename: Ok(RenameResult { edits: vec![] }),
            validate_edits: Ok(ValidateEditsResult {
                valid: true,
                version: 3,
                reason: None,
                failing_edit_index: None,
            }),
            check: Ok(ok_check()),
        };
        let mutation = validate_transaction(&mut provider, &request);
        assert!(
            mutation.ok,
            "validation succeeds: {:?}",
            mutation.diagnostics
        );
        assert!(mutation.preview.is_some());
    }

    #[test]
    fn validate_transaction_refuses_a_stale_caller_transaction() {
        // The transaction carries the identity of an older text than the
        // caller's current sources: stale refusal, no preview.
        let transaction = EditTransaction::new(vec![SourceEdit {
            edit_kind: "rename".to_string(),
            source: URI.to_string(),
            source_identity: crate::input_identity("puzzle stale {\n}"),
            range: EditRange {
                start_line: 1,
                start_col: 8,
                end_line: 1,
                end_col: 12,
            },
            new_text: "twice".to_string(),
        }])
        .expect("transaction");
        let request = ProviderValidateRequest {
            documents: document_set(),
            transaction,
            sources: sources(CLEAN),
            project_root: None,
        };
        let mut provider = ScriptedProvider {
            rename: Ok(RenameResult { edits: vec![] }),
            validate_edits: Ok(ValidateEditsResult {
                valid: true,
                version: 3,
                reason: None,
                failing_edit_index: None,
            }),
            check: Ok(ok_check()),
        };
        let mutation = validate_transaction(&mut provider, &request);
        assert!(!mutation.ok);
        assert_eq!(mutation.diagnostics[0].code, "edit-stale-source");
        assert!(mutation.transaction.is_none());
        assert!(mutation.preview.is_none());
    }
}
