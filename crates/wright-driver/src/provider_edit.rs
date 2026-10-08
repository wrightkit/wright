use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::diag::{Diagnostic, Stage};
use crate::edit::{
    EditRange, EditTransaction, SourceEdit, SourcePreview, char_offset_to_utf16,
    utf16_offset_to_char,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMutation {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<EditTransaction>,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<Vec<SourcePreview>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRenameRequest {
    pub documents: wright_lpp::DocumentSet,
    pub position_document_uri: String,
    pub position: wright_lpp::Position,
    pub new_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    pub sources: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderValidateRequest {
    pub documents: wright_lpp::DocumentSet,
    pub transaction: EditTransaction,
    pub sources: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
}

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
        Ok(res) => res,
        Err(err) => return provider_failure(&err),
    };

    let mut edits: Vec<SourceEdit> = Vec::new();
    for doc_edits in &result.edits {
        let Some(document) = request.documents.get(&doc_edits.document_uri) else {
            return refusal(vec![Diagnostic::error(
                "provider-edit-outside-set",
                Stage::Discovery,
                format!(
                    "the provider returned edits for '{}', which is not in the request document set",
                    doc_edits.document_uri
                ),
            )]);
        };
        if doc_edits.version != document.version {
            return refusal(vec![Diagnostic::error(
                "edit-stale-source",
                Stage::Discovery,
                format!(
                    "the provider computed edits for '{}' against version {}, but the current version is {}; re-fetch the source and retry",
                    doc_edits.document_uri, doc_edits.version, document.version
                ),
            )]);
        }
        let identity = crate::input_identity(&document.text);
        for text_edit in &doc_edits.text_edits {
            let range = match to_edit_range(&document.text, text_edit.range) {
                Ok(r) => r,
                Err(diag) => return refusal(vec![diag]),
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

    let transaction = match EditTransaction::new(edits) {
        Ok(tx) => tx,
        Err(diag) => return refusal(vec![diag]),
    };

    // The supplied document set is deliberately wider than the target
    // project: only the provider can compute project membership (include
    // resolution, entry selection), and `lpp/rename` needs every open
    // document to spot references the project must account for. The
    // post-edit check verdict is therefore scoped to the mutation the
    // provider actually produced — the edit sites plus the position
    // document — so an unrelated open document cannot block a valid
    // rename. Diagnostics in member files that were never supplied still
    // attribute to the entry document's check view and remain blocking.
    let mut edit_scope: BTreeSet<String> = transaction
        .edits
        .iter()
        .map(|edit| edit.source.clone())
        .collect();
    edit_scope.insert(request.position_document_uri.clone());

    finish_transaction(
        provider,
        &request.documents,
        transaction,
        &request.sources,
        request.project_root.as_deref(),
        &edit_scope,
    )
}

pub fn validate_transaction(
    provider: &mut dyn wright_lpp::LanguageProvider,
    request: &ProviderValidateRequest,
) -> ProviderMutation {
    let transaction = match EditTransaction::new(request.transaction.edits.clone()) {
        Ok(transaction) => transaction,
        Err(diagnostic) => return refusal(vec![diagnostic]),
    };
    // A caller-supplied document set is the declared project: every
    // supplied document's errors stay blocking, including unedited
    // members — the post-edit check is the only project-level gate here.
    let edit_scope: BTreeSet<String> = request.documents.keys().cloned().collect();
    finish_transaction(
        provider,
        &request.documents,
        transaction,
        &request.sources,
        request.project_root.as_deref(),
        &edit_scope,
    )
}

fn finish_transaction(
    provider: &mut dyn wright_lpp::LanguageProvider,
    documents: &wright_lpp::DocumentSet,
    transaction: EditTransaction,
    sources: &BTreeMap<String, String>,
    project_root: Option<&str>,
    edit_scope: &BTreeSet<String>,
) -> ProviderMutation {
    for edit in &transaction.edits {
        if let Some(diagnostic) = crate::edit::source_precondition(edit, sources) {
            return refusal(vec![diagnostic]);
        }
    }
    let previews = match transaction.apply(sources) {
        Ok(previews) => previews,
        Err(diag) => return refusal(vec![diag]),
    };
    if let Err(mutation) = validate_pipeline(
        provider,
        documents,
        &transaction,
        &previews,
        project_root,
        edit_scope,
    ) {
        return mutation;
    }
    ProviderMutation {
        ok: true,
        transaction: Some(transaction),
        diagnostics: Vec::new(),
        preview: Some(previews),
        provider_code: None,
        provider_message: None,
    }
}

/// The provider-side validation gates a mutation passes after Wright's own
/// preconditions: `lpp/validateEdits` per edited document, then `lpp/check`
/// over the edited project. The `Err` arm is the finished non-ok result —
/// a Wright-side edit rejection from [`refusal`] or a provider request
/// failure from [`provider_failure`], each carrying its own classification.
fn validate_pipeline(
    provider: &mut dyn wright_lpp::LanguageProvider,
    documents: &wright_lpp::DocumentSet,
    transaction: &EditTransaction,
    previews: &[SourcePreview],
    project_root: Option<&str>,
    edit_scope: &BTreeSet<String>,
) -> Result<(), ProviderMutation> {
    let mut by_source: BTreeMap<&str, Vec<&SourceEdit>> = BTreeMap::new();
    for edit in &transaction.edits {
        by_source.entry(&edit.source).or_default().push(edit);
    }

    for (source, edits) in by_source {
        let Some(document) = documents.get(source) else {
            return Err(refusal(vec![Diagnostic::error(
                "edit-unknown-source",
                Stage::Discovery,
                format!(
                    "the transaction targets '{}' but the request document set has no current text for it",
                    source
                ),
            )]));
        };
        let mut text_edits = Vec::new();
        for edit in edits {
            let te = to_text_edit(&document.text, edit).map_err(|d| refusal(vec![d]))?;
            text_edits.push(te);
        }
        let res = provider
            .validate_edits(document, &text_edits)
            .map_err(|error| provider_failure(&error))?;
        if res.version != document.version {
            return Err(refusal(vec![Diagnostic::error(
                "edit-stale-source",
                Stage::Discovery,
                format!(
                    "the provider validated edits for '{}' against version {}, but the current version is {}; re-fetch the source and retry",
                    source, res.version, document.version
                ),
            )]));
        }
        if !res.valid {
            let reason = res.reason.as_deref().unwrap_or("invalid");
            let mut msg = format!(
                "the provider refused the edits for '{source}': the edited source is {reason}"
            );
            if let Some(idx) = res.failing_edit_index {
                msg.push_str(&format!(" (failing edit {idx})"));
            }
            return Err(refusal(vec![Diagnostic::error(
                "provider-validation-failed",
                Stage::Discovery,
                msg,
            )]));
        }
    }

    let mut edited = wright_lpp::DocumentSet::new();
    for (uri, document) in documents {
        let edited_text = previews
            .iter()
            .find(|p| p.source == *uri)
            .map(|p| p.new_text.clone());
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

    let checked = provider
        .check(&edited, project_root)
        .map_err(|error| provider_failure(&error))?;
    for doc in &checked.documents {
        // Only documents the mutation is allowed to affect can block it:
        // the request set may intentionally carry unrelated open documents
        // (see `semantic_rename`), whose diagnostics are not this project's
        // problem.
        if !edit_scope.contains(&doc.uri) {
            continue;
        }
        for diag in &doc.diagnostics {
            if diag.severity == wright_lpp::DiagnosticSeverity::Error {
                return Err(refusal(vec![Diagnostic::error(
                    "provider-semantic-error",
                    Stage::Discovery,
                    format!(
                        "the edited project is semantically invalid in '{}': {}",
                        doc.uri, diag.message
                    ),
                )]));
            }
        }
    }
    Ok(())
}

/// A provider's `Err` outcome as the finished non-ok mutation. The
/// diagnostic keeps the shared provider-failure classification
/// (`crate::source_provider::provider_failure_code`/`provider_failure_stage`):
/// a deliberate refusal reports `provider-refusal-<refusalCode>` at the
/// frontend stage, a capability gap `capability-unavailable`, and
/// transport/process failures their typed `provider-*` code at the
/// internal stage (#570). `provider_code` carries the refusal's
/// `refusalCode` verbatim or the failure's typed code; nothing is applied.
pub fn provider_failure(error: &wright_lpp::ProviderError) -> ProviderMutation {
    ProviderMutation {
        ok: false,
        transaction: None,
        diagnostics: vec![provider_diagnostic(error)],
        preview: None,
        provider_code: Some(
            error
                .refusal_code()
                .map(str::to_string)
                .unwrap_or_else(|| error.code().to_string()),
        ),
        provider_message: Some(error.to_string()),
    }
}

/// A Wright-side edit rejection as the finished non-ok mutation: stale
/// sources, invalid ranges, unknown sources, and the provider's semantic
/// refusals report their own diagnostic code at the discovery stage, and
/// no provider fields are set — the failure is about the edit, not the
/// provider's execution.
fn refusal(diagnostics: Vec<Diagnostic>) -> ProviderMutation {
    ProviderMutation {
        ok: false,
        transaction: None,
        diagnostics,
        preview: None,
        provider_code: None,
        provider_message: None,
    }
}

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
    let line_idx = pos.line as usize;
    let Some(line_text) = lines.get(line_idx) else {
        return Err(Diagnostic::error(
            "edit-invalid-range",
            Stage::Discovery,
            format!(
                "the {what} names line {}, but the document has {} lines",
                pos.line,
                lines.len()
            ),
        ));
    };
    let col = utf16_to_char_column(line_text, pos.character).ok_or_else(|| {
        Diagnostic::error(
            "edit-invalid-range",
            Stage::Discovery,
            format!("the {what} names UTF-16 offset {} in line {}, which is not a character boundary of that line", pos.character, pos.line),
        )
    })?;
    Ok(((line_idx as u32) + 1, col))
}

fn column_to_position(
    lines: &[&str],
    line: u32,
    column: u32,
) -> Result<wright_lpp::Position, Diagnostic> {
    let line_idx = line.saturating_sub(1) as usize;
    let Some(line_text) = lines.get(line_idx) else {
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
            format!("the edit names column {column} in line {line}, which is outside that line (line {line} has {} characters)", line_text.chars().count()),
        )
    })?;
    Ok(wright_lpp::Position {
        line: line_idx as u32,
        character,
    })
}

fn utf16_to_char_column(line: &str, units: u32) -> Option<u32> {
    let offset = utf16_offset_to_char(line, units as usize);
    (char_offset_to_utf16(line, offset) == units as usize).then_some(offset as u32 + 1)
}

fn char_column_to_utf16(line: &str, column: u32) -> Option<u32> {
    let offset = column.checked_sub(1)? as usize;
    (offset <= line.chars().count()).then(|| char_offset_to_utf16(line, offset) as u32)
}

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

/// The diagnostic for one provider `Err`: code and stage come from the
/// shared provider-failure classification every `ProviderError` consumer
/// uses (#569), so a refusal, a capability gap, and a transport fault are
/// distinguishable from `code`/`stage` alone.
fn provider_diagnostic(error: &wright_lpp::ProviderError) -> Diagnostic {
    let code = crate::source_provider::provider_failure_code(error);
    let stage = crate::source_provider::provider_failure_stage(error);
    let refused = matches!(
        error,
        wright_lpp::ProviderError::Lpp(error) if error.kind == wright_lpp::LppErrorKind::Refusal
    );
    let message = match (refused, error.refusal_code()) {
        (true, Some(refusal)) => {
            format!("the provider refused the edit request ({refusal}): {error}")
        }
        (true, None) => format!("the provider refused the edit request: {error}"),
        (false, _) => format!("the provider failed the edit request: {error}"),
    };
    Diagnostic::error(code, stage, message)
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
    const UNRELATED_URI: &str = "file:///project/unrelated.xdl";
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
        // completes: a process exit is an internal failure, not an edit
        // refusal, and nothing is applied.
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
        assert_eq!(mutation.diagnostics[0].code, "provider-exited");
        assert_eq!(mutation.diagnostics[0].stage, Stage::Internal);
        assert_eq!(mutation.provider_code.as_deref(), Some("provider-exited"));
        assert!(mutation.transaction.is_none());
        assert!(mutation.preview.is_none());
    }

    /// #570: every provider execution failure class keeps its typed code at
    /// the internal stage — a consumer can tell a broken provider from a
    /// deliberate refusal or a source-level edit rejection without parsing
    /// the message.
    #[test]
    fn provider_transport_failures_keep_their_typed_classification() {
        for (error, code) in [
            (
                ProviderError::Timeout {
                    method: "lpp/validateEdits".to_string(),
                    duration: std::time::Duration::from_millis(500),
                },
                "provider-timeout",
            ),
            (
                ProviderError::Malformed {
                    detail: "missing result".to_string(),
                },
                "provider-malformed",
            ),
            (
                ProviderError::Io {
                    message: "broken pipe".to_string(),
                },
                "provider-io",
            ),
            (
                ProviderError::JsonRpc {
                    code: -32600,
                    message: "invalid request".to_string(),
                },
                "jsonrpc-error",
            ),
        ] {
            let mutation = provider_failure(&error);
            assert!(!mutation.ok);
            assert_eq!(mutation.diagnostics[0].code, code, "{error:?}");
            assert_eq!(
                mutation.diagnostics[0].stage,
                Stage::Internal,
                "{error:?} is an internal failure, not a discovery/source problem"
            );
            assert_eq!(mutation.provider_code.as_deref(), Some(code), "{error:?}");
            assert!(mutation.transaction.is_none());
            assert!(mutation.preview.is_none());
        }
    }

    /// #570: a `refusal` kind whose details carry no `refusalCode` is still
    /// a deliberate decline (frontend), never an internal provider fault.
    #[test]
    fn a_refusal_without_a_code_is_not_a_process_failure() {
        let mutation = provider_failure(&ProviderError::lpp(
            wright_lpp::LppErrorKind::Refusal,
            serde_json::json!({}),
            "the provider declined the request",
        ));
        assert!(!mutation.ok);
        assert_eq!(mutation.diagnostics[0].code, "provider-refusal");
        assert_eq!(mutation.diagnostics[0].stage, Stage::Frontend);
        assert_eq!(mutation.provider_code.as_deref(), Some("refusal"));
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
        assert_eq!(
            mutation.diagnostics[0].code, "provider-refusal-rename.nameCollision",
            "the refusal code is machine-readable in the diagnostic code"
        );
        assert_eq!(
            mutation.diagnostics[0].stage,
            Stage::Frontend,
            "a deliberate refusal is a request outcome, not an internal failure"
        );
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
        assert_eq!(mutation.diagnostics[0].code, "capability-unavailable");
        assert_eq!(mutation.diagnostics[0].stage, Stage::Frontend);
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
    fn rename_post_edit_check_drops_unrelated_open_document_errors() {
        // The rename document set is language-wide because only the
        // provider can compute project membership; a supplied document the
        // mutation never touched must not block it.
        let mut request = rename_request();
        request.documents.insert(
            UNRELATED_URI.to_string(),
            wright_lpp::Document {
                uri: UNRELATED_URI.to_string(),
                language_id: "x-demo-lang".to_string(),
                version: 1,
                text: "puzzle broken {\n".to_string(),
            },
        );
        request
            .sources
            .insert(UNRELATED_URI.to_string(), "puzzle broken {\n".to_string());
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
                    uri: UNRELATED_URI.to_string(),
                    version: 1,
                    diagnostics: vec![wright_lpp::Diagnostic {
                        range: wright_lpp::Range {
                            start: wright_lpp::Position {
                                line: 0,
                                character: 0,
                            },
                            end: wright_lpp::Position {
                                line: 0,
                                character: 15,
                            },
                        },
                        severity: wright_lpp::DiagnosticSeverity::Error,
                        code: Some("x-demo/unterminated".to_string()),
                        message: "unterminated puzzle".to_string(),
                        source: Some("x-demo-lang".to_string()),
                    }],
                }],
            }),
        };
        let mutation = semantic_rename(&mut provider, &request);
        assert!(
            mutation.ok,
            "an unrelated document's error cannot block the rename: {:?}",
            mutation.diagnostics
        );
        assert!(mutation.transaction.is_some());
    }

    #[test]
    fn validate_transaction_check_scope_covers_the_whole_caller_set() {
        // A caller-supplied document set is the declared project — unlike
        // the rename-collected set above, an error in an unedited document
        // still blocks because the post-edit check is the only project gate.
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
        let mut request = ProviderValidateRequest {
            documents: document_set(),
            transaction,
            sources: sources(CLEAN),
            project_root: None,
        };
        request.documents.insert(
            UNRELATED_URI.to_string(),
            wright_lpp::Document {
                uri: UNRELATED_URI.to_string(),
                language_id: "x-demo-lang".to_string(),
                version: 1,
                text: "puzzle broken {\n".to_string(),
            },
        );
        request
            .sources
            .insert(UNRELATED_URI.to_string(), "puzzle broken {\n".to_string());
        let mut provider = ScriptedProvider {
            rename: Ok(RenameResult { edits: vec![] }),
            validate_edits: Ok(ValidateEditsResult {
                valid: true,
                version: 3,
                reason: None,
                failing_edit_index: None,
            }),
            check: Ok(CheckResult {
                documents: vec![wright_lpp::DocumentDiagnostics {
                    uri: UNRELATED_URI.to_string(),
                    version: 1,
                    diagnostics: vec![wright_lpp::Diagnostic {
                        range: wright_lpp::Range {
                            start: wright_lpp::Position {
                                line: 0,
                                character: 0,
                            },
                            end: wright_lpp::Position {
                                line: 0,
                                character: 15,
                            },
                        },
                        severity: wright_lpp::DiagnosticSeverity::Error,
                        code: Some("x-demo/unterminated".to_string()),
                        message: "unterminated puzzle".to_string(),
                        source: Some("x-demo-lang".to_string()),
                    }],
                }],
            }),
        };
        let mutation = validate_transaction(&mut provider, &request);
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
    fn validate_transaction_rejects_a_directly_constructed_empty_transaction() {
        let request = ProviderValidateRequest {
            documents: document_set(),
            transaction: EditTransaction { edits: Vec::new() },
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
        assert_eq!(mutation.diagnostics[0].code, "edit-empty-transaction");
        assert!(mutation.transaction.is_none());
        assert!(mutation.preview.is_none());
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
