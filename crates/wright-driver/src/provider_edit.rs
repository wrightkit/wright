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

fn refusal(
    diagnostics: Vec<Diagnostic>,
    code: Option<String>,
    msg: Option<String>,
) -> ProviderMutation {
    ProviderMutation {
        ok: false,
        transaction: None,
        diagnostics,
        preview: None,
        provider_code: code,
        provider_message: msg,
    }
}

fn refusal_err(diag: Diagnostic) -> ProviderMutation {
    refusal(vec![diag], None, None)
}

fn disc_err(code: &'static str, msg: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, Stage::Discovery, msg)
}

/// A provider-driven semantic rename (#139).
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

    let mut edits: Vec<SourceEdit> = Vec::new();
    for document_edits in &result.edits {
        let Some(document) = request.documents.get(&document_edits.document_uri) else {
            return refusal_err(disc_err(
                "provider-edit-outside-set",
                format!(
                    "the provider returned edits for '{}', which is not in the request document set",
                    document_edits.document_uri
                ),
            ));
        };
        if document_edits.version != document.version {
            return refusal_err(disc_err(
                "edit-stale-source",
                format!(
                    "the provider computed edits for '{}' against version {}, but the current version is {}; re-fetch the source and retry",
                    document_edits.document_uri, document_edits.version, document.version
                ),
            ));
        }
        let identity = crate::input_identity(&document.text);
        for text_edit in &document_edits.text_edits {
            let range = match to_edit_range(&document.text, text_edit.range) {
                Ok(range) => range,
                Err(diagnostic) => return refusal_err(diagnostic),
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

/// A structured refusal for a provider failure.
pub fn provider_failure(error: &wright_lpp::ProviderError) -> ProviderMutation {
    let code = error
        .refusal_code()
        .unwrap_or_else(|| error.code())
        .to_string();
    refusal(
        vec![provider_diagnostic(error)],
        Some(code),
        Some(error.to_string()),
    )
}

/// The structured diagnostic for a provider failure.
fn provider_diagnostic(error: &wright_lpp::ProviderError) -> Diagnostic {
    match error.refusal_code() {
        Some(code) => disc_err(
            "provider-refusal",
            format!("the provider refused the request ({code}): {error}"),
        ),
        None => disc_err(
            "provider-error",
            format!(
                "the provider failed the request ({}): {error}",
                error.code()
            ),
        ),
    }
}

fn finish(
    provider: &mut dyn wright_lpp::LanguageProvider,
    documents: &wright_lpp::DocumentSet,
    edits: Vec<SourceEdit>,
    sources: &BTreeMap<String, String>,
    project_root: Option<&str>,
) -> ProviderMutation {
    let transaction = match EditTransaction::new(edits) {
        Ok(transaction) => transaction,
        Err(diagnostic) => return refusal_err(diagnostic),
    };

    if let Some(diagnostic) = precondition_problems(&transaction, sources) {
        return refusal_err(diagnostic);
    }

    let preview = match transaction.apply(sources) {
        Ok(preview) => preview,
        Err(diagnostic) => return refusal_err(diagnostic),
    };

    match provider_validation(provider, documents, &transaction, &preview, project_root) {
        Ok(()) => ProviderMutation {
            ok: true,
            transaction: Some(transaction),
            diagnostics: Vec::new(),
            preview: Some(preview),
            provider_code: None,
            provider_message: None,
        },
        Err(mutation) => mutation,
    }
}

fn precondition_problems(
    transaction: &EditTransaction,
    sources: &BTreeMap<String, String>,
) -> Option<Diagnostic> {
    for edit in &transaction.edits {
        let Some(current) = sources.get(&edit.source) else {
            return Some(disc_err(
                "edit-unknown-source",
                format!(
                    "the edit targets '{}' but no current text was provided for it; \
                     supply the current source so the version precondition can be verified",
                    edit.source
                ),
            ));
        };
        if crate::input_identity(current) != edit.source_identity {
            return Some(disc_err(
                "edit-stale-source",
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

fn provider_validation(
    provider: &mut dyn wright_lpp::LanguageProvider,
    documents: &wright_lpp::DocumentSet,
    transaction: &EditTransaction,
    previews: &[SourcePreview],
    project_root: Option<&str>,
) -> Result<(), ProviderMutation> {
    let mut grouped: BTreeMap<&str, Vec<&SourceEdit>> = BTreeMap::new();
    for edit in &transaction.edits {
        grouped.entry(edit.source.as_str()).or_default().push(edit);
    }
    for (source, edits) in grouped {
        let Some(document) = documents.get(source) else {
            return Err(refusal_err(disc_err(
                "edit-unknown-source",
                format!(
                    "the transaction targets '{source}' but the request document set has no current text for it"
                ),
            )));
        };
        let mut text_edits = Vec::new();
        for edit in edits {
            text_edits.push(to_text_edit(&document.text, edit).map_err(refusal_err)?);
        }
        let result = provider
            .validate_edits(document, &text_edits)
            .map_err(|e| provider_failure(&e))?;
        if result.version != document.version {
            return Err(refusal_err(disc_err(
                "edit-stale-source",
                format!(
                    "the provider validated edits for '{source}' against version {}, but the current version is {}; re-fetch the source and retry",
                    result.version, document.version
                ),
            )));
        }
        if !result.valid {
            let reason = result.reason.as_deref().unwrap_or("invalid");
            let mut message = format!(
                "the provider refused the edits for '{source}': the edited source is {reason}"
            );
            if let Some(index) = result.failing_edit_index {
                message.push_str(&format!(" (failing edit {index})"));
            }
            return Err(refusal_err(disc_err("provider-validation-failed", message)));
        }
    }

    let edited: wright_lpp::DocumentSet = documents
        .iter()
        .map(|(uri, doc)| {
            let p = previews.iter().find(|p| p.source == *uri);
            let version = if p.is_some() {
                doc.version + 1
            } else {
                doc.version
            };
            let text = p.map_or_else(|| doc.text.clone(), |p| p.new_text.clone());
            (
                uri.clone(),
                wright_lpp::Document {
                    uri: uri.clone(),
                    language_id: doc.language_id.clone(),
                    version,
                    text,
                },
            )
        })
        .collect();
    let checked = provider
        .check(&edited, project_root)
        .map_err(|e| provider_failure(&e))?;
    for document in &checked.documents {
        for diagnostic in &document.diagnostics {
            if diagnostic.severity == wright_lpp::DiagnosticSeverity::Error {
                return Err(refusal_err(disc_err(
                    "provider-semantic-error",
                    format!(
                        "the edited project is semantically invalid in '{}': {}",
                        document.uri, diagnostic.message
                    ),
                )));
            }
        }
    }
    Ok(())
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

fn position_to_column(
    lines: &[&str],
    pos: wright_lpp::Position,
    what: &str,
) -> Result<(u32, u32), Diagnostic> {
    let line_text = lines.get(pos.line as usize).ok_or_else(|| {
        disc_err(
            "edit-invalid-range",
            format!(
                "the {what} names line {}, but the document has {} lines",
                pos.line,
                lines.len()
            ),
        )
    })?;
    let col = utf16_to_char_column(line_text, pos.character).ok_or_else(|| {
        disc_err("edit-invalid-range", format!("the {what} names UTF-16 offset {} in line {}, which is not a character boundary of that line", pos.character, pos.line))
    })?;
    Ok((pos.line + 1, col))
}

fn column_to_position(
    lines: &[&str],
    line: u32,
    column: u32,
) -> Result<wright_lpp::Position, Diagnostic> {
    let line_idx = line.saturating_sub(1) as usize;
    let line_text = lines.get(line_idx).ok_or_else(|| {
        disc_err(
            "edit-invalid-range",
            format!(
                "the edit names line {line}, but the document has {} lines",
                lines.len()
            ),
        )
    })?;
    let character = char_column_to_utf16(line_text, column).ok_or_else(|| {
        disc_err("edit-invalid-range", format!("the edit names column {column} in line {line}, which is outside that line (line {line} has {} characters)", line_text.chars().count()))
    })?;
    Ok(wright_lpp::Position {
        line: line_idx as u32,
        character,
    })
}

fn utf16_to_char_column(line: &str, units: u32) -> Option<u32> {
    let mut acc = 0;
    for (i, ch) in line.chars().enumerate() {
        if acc == units {
            return Some(i as u32 + 1);
        }
        let len = ch.len_utf16() as u32;
        if acc + len > units {
            return None;
        }
        acc += len;
    }
    (acc == units).then_some(line.chars().count() as u32 + 1)
}

fn char_column_to_utf16(line: &str, column: u32) -> Option<u32> {
    if column == 0 {
        return None;
    }
    let mut acc = 0;
    for (i, ch) in line.chars().enumerate() {
        if (i as u32) + 1 == column {
            return Some(acc);
        }
        acc += ch.len_utf16() as u32;
    }
    (column as usize == line.chars().count() + 1).then_some(acc)
}

fn edit_invalid_range(range: wright_lpp::Range) -> Diagnostic {
    disc_err(
        "edit-invalid-range",
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
        CheckResult, ClientInfo, Document, DocumentDiagnostics, DocumentEdits, DocumentSet,
        InitializeResult, LocationsResult, NegotiatedCapabilities, Position, ProviderError, Range,
        ReconstructResult, RenameResult, SymbolsResult, TextEdit, ValidateEditsResult,
        WorkshopArtifact,
    };

    const URI: &str = "file:///project/puzzle.xdl";
    const CLEAN: &str = "puzzle clean {\n  target = 40\n  start = 10\n  ops {\n    double: x => x * 2\n    plus1: x => x + 1\n  }\n  solution = [ double, double ]\n}";

    fn document_set() -> DocumentSet {
        let doc = Document {
            uri: URI.into(),
            language_id: "x-demo-lang".into(),
            version: 3,
            text: CLEAN.into(),
        };
        [(URI.into(), doc)].into_iter().collect()
    }

    fn sources(text: &str) -> BTreeMap<String, String> {
        [(URI.into(), text.into())].into_iter().collect()
    }

    fn rename_request() -> ProviderRenameRequest {
        ProviderRenameRequest {
            documents: document_set(),
            position_document_uri: URI.into(),
            position: Position {
                line: 4,
                character: 6,
            },
            new_name: "twice".into(),
            project_root: Some("file:///project".into()),
            sources: sources(CLEAN),
        }
    }

    fn clean_rename_result() -> RenameResult {
        let edit = |line, s, e| TextEdit {
            range: Range {
                start: Position { line, character: s },
                end: Position { line, character: e },
            },
            new_text: "twice".into(),
        };
        RenameResult {
            edits: vec![DocumentEdits {
                document_uri: URI.into(),
                version: 3,
                text_edits: vec![edit(4, 4, 10), edit(7, 15, 21), edit(7, 23, 29)],
            }],
        }
    }

    #[derive(Default)]
    struct ScriptedProvider {
        rename: Option<Result<RenameResult, ProviderError>>,
        validate_edits: Option<Result<ValidateEditsResult, ProviderError>>,
        check: Option<Result<CheckResult, ProviderError>>,
    }

    type LppRes<T> = Result<T, ProviderError>;

    impl wright_lpp::LanguageProvider for ScriptedProvider {
        fn initialize(&mut self, _: Option<&ClientInfo>) -> LppRes<InitializeResult> {
            unreachable!()
        }
        fn capabilities(&self) -> LppRes<&NegotiatedCapabilities> {
            unreachable!()
        }
        fn check(&mut self, _: &DocumentSet, _: Option<&str>) -> LppRes<CheckResult> {
            self.check
                .clone()
                .unwrap_or(Ok(CheckResult { documents: vec![] }))
        }
        fn compile(
            &mut self,
            _: &DocumentSet,
            _: Option<&str>,
        ) -> LppRes<wright_lpp::CompileResult> {
            unreachable!()
        }
        fn reconstruct(&mut self, _: &WorkshopArtifact) -> LppRes<ReconstructResult> {
            unreachable!()
        }
        fn symbols(&mut self, _: &DocumentSet, _: Option<&str>) -> LppRes<SymbolsResult> {
            unreachable!()
        }
        fn definition(&mut self, _: &Document, _: Position) -> LppRes<LocationsResult> {
            unreachable!()
        }
        fn references(&mut self, _: &Document, _: Position, _: bool) -> LppRes<LocationsResult> {
            unreachable!()
        }
        fn rename(
            &mut self,
            _: &DocumentSet,
            _: &str,
            _: Position,
            _: &str,
            _: Option<&str>,
        ) -> LppRes<RenameResult> {
            self.rename
                .clone()
                .unwrap_or_else(|| Ok(clean_rename_result()))
        }
        fn validate_edits(&mut self, _: &Document, _: &[TextEdit]) -> LppRes<ValidateEditsResult> {
            self.validate_edits
                .clone()
                .unwrap_or(Ok(ValidateEditsResult {
                    valid: true,
                    version: 3,
                    reason: None,
                    failing_edit_index: None,
                }))
        }
        fn shutdown(&mut self) -> LppRes<()> {
            unreachable!()
        }
        fn exit_status(&self) -> Option<i32> {
            None
        }
    }

    fn assert_refusal(mutation: ProviderMutation, diag: &str, code: Option<&str>) {
        assert!(!mutation.ok);
        assert_eq!(mutation.diagnostics[0].code, diag);
        if let Some(c) = code {
            assert_eq!(mutation.provider_code.as_deref(), Some(c));
        }
        assert!(mutation.transaction.is_none() && mutation.preview.is_none());
    }

    fn test_edit(line: u32, s: u32, e: u32, text: &str, ident: &str) -> SourceEdit {
        SourceEdit {
            edit_kind: "rename".into(),
            source: URI.into(),
            source_identity: ident.into(),
            range: EditRange {
                start_line: line,
                start_col: s,
                end_line: line,
                end_col: e,
            },
            new_text: text.into(),
        }
    }

    fn val_req(transaction: EditTransaction) -> ProviderValidateRequest {
        ProviderValidateRequest {
            documents: document_set(),
            transaction,
            sources: sources(CLEAN),
            project_root: None,
        }
    }

    #[test]
    fn utf16_and_edit_range_conversions() {
        let text = "puzzle αβ {\n  target = 40\n}";
        let lpp = Range {
            start: Position {
                line: 0,
                character: 7,
            },
            end: Position {
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
                end_col: 10
            }
        );
        let back = to_text_edit(
            text,
            &test_edit(1, 8, 10, "x", &crate::input_identity(text)),
        )
        .expect("converts back");
        assert_eq!(back.range, lpp);

        let supp = Range {
            start: Position {
                line: 0,
                character: 7,
            },
            end: Position {
                line: 0,
                character: 8,
            },
        };
        assert_eq!(
            to_edit_range("puzzle 𝕏 {", supp).expect_err("inside").code,
            "edit-invalid-range"
        );

        let eol = Range {
            start: Position {
                line: 0,
                character: 2,
            },
            end: Position {
                line: 0,
                character: 2,
            },
        };
        let w = to_edit_range("ab\ncd", eol).expect("end of line is valid");
        assert_eq!((w.start_line, w.start_col), (1, 3));
    }

    #[test]
    fn rename_wraps_provider_edits_in_a_wright_transaction() {
        let mutation = semantic_rename(&mut ScriptedProvider::default(), &rename_request());
        assert!(mutation.ok, "rename succeeds: {:?}", mutation.diagnostics);
        let transaction = mutation.transaction.expect("transaction");
        assert_eq!(transaction.edits.len(), 3);
        assert!(
            transaction
                .edits
                .iter()
                .all(|e| e.edit_kind == "rename"
                    && e.source_identity == crate::input_identity(CLEAN))
        );
        assert_eq!(
            (
                transaction.edits[0].range.start_line,
                transaction.edits[1].range.start_line,
                transaction.edits[2].range.start_line
            ),
            (5, 8, 8)
        );
        let preview = mutation.preview.expect("preview");
        assert!(
            preview[0].new_text.contains("twice: x => x * 2")
                && preview[0].new_text.contains("solution = [ twice, twice ]")
        );
    }

    #[test]
    fn rename_refusal_modes() {
        let mut p = ScriptedProvider::default();
        let mut r1 = clean_rename_result();
        r1.edits[0].document_uri = "file:///project/other.xdl".into();
        p.rename = Some(Ok(r1));
        assert_refusal(
            semantic_rename(&mut p, &rename_request()),
            "provider-edit-outside-set",
            None,
        );

        let mut r2 = clean_rename_result();
        r2.edits[0].version = 2;
        p.rename = Some(Ok(r2));
        assert_refusal(
            semantic_rename(&mut p, &rename_request()),
            "edit-stale-source",
            None,
        );

        let mut req = rename_request();
        req.sources = sources(&format!("{CLEAN}\n"));
        assert_refusal(
            semantic_rename(&mut ScriptedProvider::default(), &req),
            "edit-stale-source",
            None,
        );

        p = ScriptedProvider {
            validate_edits: Some(Err(ProviderError::Exited {
                status: Some(3),
                message: "the LPP provider process exited".into(),
            })),
            ..Default::default()
        };
        assert_refusal(
            semantic_rename(&mut p, &rename_request()),
            "provider-error",
            Some("provider-exited"),
        );

        p = ScriptedProvider {
            rename: Some(Err(ProviderError::lpp(
                wright_lpp::LppErrorKind::Refusal,
                serde_json::json!({ "refusalCode": "rename.nameCollision" }),
                "new name collides with an existing symbol",
            ))),
            ..Default::default()
        };
        assert_refusal(
            semantic_rename(&mut p, &rename_request()),
            "provider-refusal",
            Some("rename.nameCollision"),
        );

        p = ScriptedProvider {
            rename: Some(Err(ProviderError::lpp(
                wright_lpp::LppErrorKind::CapabilityUnavailable,
                serde_json::json!({ "capability": "rename", "method": "lpp/rename" }),
                "capability 'rename' is not available in this session",
            ))),
            ..Default::default()
        };
        assert_refusal(
            semantic_rename(&mut p, &rename_request()),
            "provider-error",
            Some("capability-unavailable"),
        );

        p = ScriptedProvider {
            validate_edits: Some(Ok(ValidateEditsResult {
                valid: false,
                version: 3,
                reason: Some("syntaxError".into()),
                failing_edit_index: None,
            })),
            ..Default::default()
        };
        assert_refusal(
            semantic_rename(&mut p, &rename_request()),
            "provider-validation-failed",
            None,
        );

        let mut err_provider = ScriptedProvider {
            check: Some(Ok(CheckResult {
                documents: vec![DocumentDiagnostics {
                    uri: URI.into(),
                    version: 4,
                    diagnostics: vec![wright_lpp::Diagnostic {
                        range: Range {
                            start: Position {
                                line: 7,
                                character: 15,
                            },
                            end: Position {
                                line: 7,
                                character: 21,
                            },
                        },
                        severity: wright_lpp::DiagnosticSeverity::Error,
                        code: Some("x-demo/unresolved-op".into()),
                        message: "unresolved op reference 'twice'".into(),
                        source: Some("x-demo-lang".into()),
                    }],
                }],
            })),
            ..Default::default()
        };
        assert_refusal(
            semantic_rename(&mut err_provider, &rename_request()),
            "provider-semantic-error",
            None,
        );
    }

    #[test]
    fn validate_transaction_runs_or_refuses() {
        let tx = EditTransaction::new(vec![test_edit(
            5,
            5,
            11,
            "twice",
            &crate::input_identity(CLEAN),
        )])
        .unwrap();
        let mutation = validate_transaction(&mut ScriptedProvider::default(), &val_req(tx));
        assert!(mutation.ok && mutation.preview.is_some());

        let stale = EditTransaction::new(vec![test_edit(
            1,
            8,
            12,
            "twice",
            &crate::input_identity("puzzle stale {\n}"),
        )])
        .unwrap();
        assert_refusal(
            validate_transaction(&mut ScriptedProvider::default(), &val_req(stale)),
            "edit-stale-source",
            None,
        );
    }
}
