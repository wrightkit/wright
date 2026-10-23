use std::collections::BTreeMap;

use serde::Serialize;
use workshop_rs::rules::statement_edit::{StatementEditError, StatementList as OwnerStatementList};

use crate::diag::{Diagnostic, Stage};
use crate::edit::{EditRange, EditTransaction, SourceEdit, SourcePreview};
use crate::request::{StatementEditChange, StatementEditRequest, StatementList};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EditStatementsResult {
    pub ok: bool,
    pub exit: u8,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<EditTransaction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<Vec<SourcePreview>>,
}

pub(crate) fn refusal(diagnostics: Vec<Diagnostic>) -> EditStatementsResult {
    EditStatementsResult {
        ok: false,
        exit: crate::result::exit_code_from(&diagnostics),
        diagnostics,
        transaction: None,
        preview: None,
    }
}

pub(crate) fn owner_list(list: StatementList) -> OwnerStatementList {
    match list {
        StatementList::Conditions => OwnerStatementList::Conditions,
        StatementList::Actions => OwnerStatementList::Actions,
    }
}

pub(crate) fn owner_change(
    change: &StatementEditChange,
) -> workshop_rs::rules::statement_edit::StatementEdit<'_> {
    match change {
        StatementEditChange::Insert { statements } => {
            workshop_rs::rules::statement_edit::StatementEdit::Insert(statements)
        }
        StatementEditChange::Replace { statement } => {
            workshop_rs::rules::statement_edit::StatementEdit::Replace(statement)
        }
        StatementEditChange::Delete => workshop_rs::rules::statement_edit::StatementEdit::Delete,
    }
}

pub(crate) fn owner_error(
    error: StatementEditError,
    edit: &StatementEditRequest,
    rule: usize,
    resolved: &crate::input::ResolvedInput,
) -> Diagnostic {
    match error {
        StatementEditError::Content(_)
        | StatementEditError::StatementKind { .. }
        | StatementEditError::EmptyInsert { .. } => Diagnostic::error(
            "invalid-statement",
            Stage::Discovery,
            format!(
                "{} at rule {rule} {} index {}",
                error,
                edit.list.as_str(),
                edit.index
            ),
        ),
        StatementEditError::UnknownRule { rule } => Diagnostic::error(
            "invalid-statement-address",
            Stage::Discovery,
            format!("unknown rule {rule}"),
        ),
        StatementEditError::IndexOutOfRange { index, .. } => Diagnostic::error(
            "invalid-statement-address",
            Stage::Discovery,
            format!(
                "no {} statement at index {index} in rule {rule}",
                edit.list.as_str()
            ),
        ),
        StatementEditError::StaleAddress { .. } => Diagnostic::error(
            "stale-address",
            Stage::Discovery,
            format!(
                "the {} of rule {rule} no longer matches its retained source",
                edit.list.as_str()
            ),
        ),
        StatementEditError::SourceUnavailable { .. } => Diagnostic::error(
            "source-unavailable",
            Stage::Discovery,
            format!("rule {rule} has no editable source"),
        ),
        StatementEditError::Emission(error) => crate::diag::workshop_diag(error, resolved),
        StatementEditError::Edit(error) => {
            Diagnostic::error("source-edit-error", Stage::Internal, error.to_string())
        }
        _ => Diagnostic::error("statement-edit-error", Stage::Internal, error.to_string()),
    }
}

pub(crate) fn wright_edit(
    program: &workshop_rs::Program,
    rule: usize,
    edit: &workshop_rs::source::SourceEdit,
    source: &str,
    source_identity: &str,
) -> Option<SourceEdit> {
    let file = program.rule_span(rule)?.file;
    let text = program.source(file)?.text();
    let range = edit.range();
    let (start_line, start_col) = byte_position(text, range.start);
    let (end_line, end_col) = byte_position(text, range.end);
    Some(SourceEdit {
        edit_kind: "editStatements".to_string(),
        source: source.to_string(),
        source_identity: source_identity.to_string(),
        range: EditRange {
            start_line,
            start_col,
            end_line,
            end_col,
        },
        new_text: edit.replacement().to_string(),
    })
}

fn byte_position(text: &str, offset: usize) -> (u32, u32) {
    let mut line = 1u32;
    let mut col = 1u32;
    for (index, character) in text.char_indices() {
        if index == offset {
            break;
        }
        if character == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

pub(crate) fn supplied_source<'a>(
    sources: &'a BTreeMap<String, String>,
    resolved: &crate::input::ResolvedInput,
) -> Option<(&'a str, &'a str)> {
    sources
        .iter()
        .find(|(key, _)| crate::edit::same_source(key, resolved))
        .map(|(key, text)| (key.as_str(), text.as_str()))
}
