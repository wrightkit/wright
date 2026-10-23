use std::collections::BTreeMap;

use serde::Serialize;
use workshop_rs::catalog::Catalog;
use workshop_rs::rules::statement::{Statement, statement_from_content};
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

/// The role one `actions` statement plays in the flat block stream
/// `rule-content-v1` exports (#962): `If`/`While`/`For*` open a block an
/// `End` closes, `Else`/`Else If` sit inside an open `If`, and every
/// other statement is plain.
#[derive(Debug, Clone, Copy)]
enum BlockRole {
    Plain,
    /// `if_block` is `true` only for `If`; `name` is the statement's
    /// display form for the refusal message.
    Open {
        if_block: bool,
        name: &'static str,
    },
    /// `Else`/`Else If`, valid only inside an open `If` block.
    Branch(&'static str),
    End,
}

fn action_role(action: &workshop_rs::Action) -> BlockRole {
    use workshop_rs::Action;
    let mut action = action;
    while let Action::Disabled { action: inner } = action {
        action = inner;
    }
    match action {
        Action::If { .. } => BlockRole::Open {
            if_block: true,
            name: "If",
        },
        Action::While { .. } => BlockRole::Open {
            if_block: false,
            name: "While",
        },
        Action::ForGlobalVariable { .. } => BlockRole::Open {
            if_block: false,
            name: "For Global Variable",
        },
        Action::ForPlayerVariable { .. } => BlockRole::Open {
            if_block: false,
            name: "For Player Variable",
        },
        Action::ElseIf { .. } => BlockRole::Branch("Else If"),
        Action::Else => BlockRole::Branch("Else"),
        Action::End => BlockRole::End,
        _ => BlockRole::Plain,
    }
}

fn doc_role(
    catalog: &Catalog,
    doc: &serde_json::Value,
    rule: usize,
    index: usize,
) -> Result<BlockRole, Diagnostic> {
    match statement_from_content(catalog, doc) {
        Ok(Statement::Action(action)) => Ok(action_role(&action)),
        // A condition document in an `actions` edit is refused by the
        // owner's statement-kind check before this simulation runs.
        Ok(_) => Ok(BlockRole::Plain),
        Err(error) => Err(Diagnostic::error(
            "invalid-statement",
            Stage::Discovery,
            format!("{error} at rule {rule} actions index {index}"),
        )),
    }
}

/// The request-level block rule (#962): after every edit in the request
/// is applied to a rule's `actions` list, each `If`/`While`/`For*` has a
/// matching `End` and `Else`/`Else If` sit inside an open `If`. The
/// first statement the resulting stream leaves unpaired produces the
/// `unbalanced-block` refusal — one code for every block form — naming
/// the statement at its position in the result.
pub(crate) fn balance_violation(
    catalog: &Catalog,
    rule: &workshop_rs::Rule,
    rule_index: usize,
    edits: &[&StatementEditRequest],
) -> Option<Diagnostic> {
    let edit_at = |index: usize| edits.iter().find(|edit| edit.index == index);
    let mut open: Vec<(bool, &'static str, usize)> = Vec::new();
    let mut position = 0usize;
    let unbalanced =
        |message: String| Diagnostic::error("unbalanced-block", Stage::Discovery, message);
    let mut feed = |role: Result<BlockRole, Diagnostic>| -> Option<Diagnostic> {
        match role {
            Err(error) => return Some(error),
            Ok(BlockRole::Open { if_block, name }) => open.push((if_block, name, position)),
            Ok(BlockRole::Branch(name)) => {
                if open.last().is_none_or(|(if_block, ..)| !if_block) {
                    return Some(unbalanced(format!(
                        "`{name}` at actions index {position} of rule {rule_index} sits outside an open `If` block"
                    )));
                }
            }
            Ok(BlockRole::End) => {
                if open.pop().is_none() {
                    return Some(unbalanced(format!(
                        "`End` at actions index {position} of rule {rule_index} closes no open block"
                    )));
                }
            }
            Ok(BlockRole::Plain) => {}
        }
        position += 1;
        None
    };
    for index in 0..=rule.actions.len() {
        if let Some(StatementEditChange::Insert { statements }) =
            edit_at(index).map(|edit| &edit.change)
        {
            for doc in statements {
                if let violation @ Some(_) = feed(doc_role(catalog, doc, rule_index, index)) {
                    return violation;
                }
            }
        }
        if index == rule.actions.len() {
            break;
        }
        let role = match edit_at(index).map(|edit| &edit.change) {
            Some(StatementEditChange::Delete) => continue,
            Some(StatementEditChange::Replace { statement }) => {
                doc_role(catalog, statement, rule_index, index)
            }
            _ => Ok(action_role(&rule.actions[index])),
        };
        if let violation @ Some(_) = feed(role) {
            return violation;
        }
    }
    open.last().map(|(_, name, at)| {
        unbalanced(format!(
            "`{name}` at actions index {at} of rule {rule_index} has no matching `End`"
        ))
    })
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
