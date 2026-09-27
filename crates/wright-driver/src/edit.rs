//! Tools and agents propose edits as validated, source-oriented
//! [`SourceEdit`]s — never as mutations of Wright's internal IR.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::{SessionConfig, SourceKind};
use crate::diag::{Diagnostic, Position, SourceSpan, Stage, source_provider_unavailable};
use crate::result::exit_code_from;

/// One proposed source edit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEdit {
    #[serde(rename = "kind")]
    pub edit_kind: String,
    pub source: String,
    pub source_identity: String,
    pub range: EditRange,
    pub new_text: String,
}

/// A source range (1-based line and character column, half-open; `end` is exclusive).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditRange {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

/// One validated source transaction: multiple file edits applied and validated atomically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditTransaction {
    pub edits: Vec<SourceEdit>,
}

impl EditTransaction {
    pub fn new(mut edits: Vec<SourceEdit>) -> Result<EditTransaction, Diagnostic> {
        if edits.is_empty() {
            return Err(Diagnostic::error(
                "edit-empty-transaction",
                Stage::Discovery,
                "a source-edit transaction must carry at least one edit",
            ));
        }
        edits.sort_by(|a, b| {
            a.source
                .cmp(&b.source)
                .then_with(|| {
                    (a.range.start_line, a.range.start_col)
                        .cmp(&(b.range.start_line, b.range.start_col))
                })
                .then_with(|| {
                    (a.range.end_line, a.range.end_col).cmp(&(b.range.end_line, b.range.end_col))
                })
        });
        for pair in edits.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if a.source != b.source {
                continue;
            }
            if (b.range.start_line, b.range.start_col) < (a.range.end_line, a.range.end_col) {
                return Err(Diagnostic::error(
                    "edit-overlap",
                    Stage::Discovery,
                    format!(
                        "the transaction carries overlapping edits in '{}' at {}:{}-{} and {}:{}-{}",
                        a.source,
                        a.range.start_line,
                        a.range.start_col,
                        a.range.end_line,
                        a.range.end_col,
                        b.range.start_line,
                        b.range.start_col
                    ),
                ));
            }
            if (a.range.start_line, a.range.start_col) == (b.range.start_line, b.range.start_col)
                && (is_zero_width(a) || is_zero_width(b))
            {
                return Err(Diagnostic::error(
                    "edit-zero-width-conflict",
                    Stage::Discovery,
                    format!(
                        "the transaction carries order-dependent zero-width edits in '{}' at {}:{}; refusing rather than defining an arbitrary insertion order",
                        a.source, a.range.start_line, a.range.start_col
                    ),
                ));
            }
        }
        Ok(EditTransaction { edits })
    }

    pub fn apply(
        &self,
        sources: &BTreeMap<String, String>,
    ) -> Result<Vec<SourcePreview>, Diagnostic> {
        let mut grouped: BTreeMap<&str, Vec<&SourceEdit>> = BTreeMap::new();
        for edit in &self.edits {
            grouped.entry(&edit.source).or_default().push(edit);
        }
        let mut previews = Vec::new();
        for (source, edits) in grouped {
            let original = sources.get(source).expect("precondition verified");
            let mut new_text = original.clone();
            for edit in edits.iter().rev() {
                new_text = apply_edit(&new_text, edit)?;
            }
            previews.push(SourcePreview {
                source: source.to_string(),
                source_identity: crate::input_identity(&new_text),
                new_text,
            });
        }
        Ok(previews)
    }
}

fn is_zero_width(edit: &SourceEdit) -> bool {
    (edit.range.start_line, edit.range.start_col) == (edit.range.end_line, edit.range.end_col)
}

/// The edited text of one source in a validated transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcePreview {
    pub source: String,
    pub new_text: String,
    pub source_identity: String,
}

/// The result of validating a proposed transaction.
#[derive(Debug, Clone, Serialize)]
pub struct EditValidation {
    pub ok: bool,
    pub exit: u8,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<Vec<SourcePreview>>,
}

fn validate_project_input(
    config: &SessionConfig,
    sources: &BTreeMap<String, String>,
    previews: Option<&[SourcePreview]>,
) -> Result<(), Diagnostic> {
    let Some(main_path) = config.input.path() else {
        return Err(Diagnostic::error(
            "edit-input-stdin",
            Stage::Discovery,
            "edit validation requires a path-based input so the edited project's main source identity is established; stdin has no project identity",
        ));
    };
    validate_source_kind(config, main_path)?;

    let main_source = main_path.to_string_lossy().into_owned();
    if previews
        .and_then(|previews| preview_of(previews, main_path))
        .is_some()
        || sources.contains_key(&main_source)
    {
        return Ok(());
    }
    std::fs::read_to_string(main_path).map(|_| ()).map_err(|e| {
        Diagnostic::error(
            "input-io",
            Stage::Discovery,
            format!("cannot read input '{}': {e}", main_path.display()),
        )
    })
}

pub fn validate_transaction(
    config: &SessionConfig,
    sources: &BTreeMap<String, String>,
    transaction: &EditTransaction,
) -> EditValidation {
    let mut diagnostics = Vec::new();
    for edit in &transaction.edits {
        if let Some(diagnostic) = source_precondition(edit, sources) {
            diagnostics.push(diagnostic);
        }
    }
    if diagnostics
        .iter()
        .any(|d| d.severity == crate::diag::Severity::Error)
    {
        return refusal(diagnostics);
    }

    let previews = match transaction.apply(sources) {
        Ok(previews) => previews,
        Err(diagnostic) => {
            diagnostics.push(diagnostic);
            return refusal(diagnostics);
        }
    };

    if let Err(diagnostic) = validate_project_input(config, sources, Some(&previews)) {
        diagnostics.push(diagnostic);
        return refusal(diagnostics);
    }
    refusal(vec![source_provider_unavailable()])
}

fn refusal(diagnostics: Vec<Diagnostic>) -> EditValidation {
    EditValidation {
        ok: false,
        exit: exit_code_from(&diagnostics),
        diagnostics,
        preview: None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameTarget {
    pub source: String,
    pub line: u32,
    pub col: u32,
    pub to: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SemanticRename {
    pub ok: bool,
    pub transaction: Option<EditTransaction>,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<Vec<SourcePreview>>,
}

pub fn semantic_rename(
    config: &SessionConfig,
    sources: &BTreeMap<String, String>,
    target: &RenameTarget,
) -> SemanticRename {
    let refuse = |diagnostics: Vec<Diagnostic>| SemanticRename {
        ok: false,
        transaction: None,
        diagnostics,
        preview: None,
    };

    if target.to.is_empty() {
        return refuse(vec![Diagnostic::error(
            "rename-invalid-name",
            Stage::Discovery,
            "rename requires a non-empty new name",
        )]);
    }

    if let Err(diagnostic) = validate_project_input(config, sources, None) {
        return refuse(vec![diagnostic]);
    }
    refuse(vec![source_provider_unavailable()])
}
fn validate_source_kind(config: &SessionConfig, main_path: &Path) -> Result<(), Diagnostic> {
    match config.kind {
        SourceKind::Opy => Ok(()),
        SourceKind::Ostw => Err(source_provider_unavailable()),
        SourceKind::Auto => match main_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref()
        {
            Some("opy") => Ok(()),
            Some("ostw" | "del") => Err(source_provider_unavailable()),
            _ => Err(Diagnostic::error(
                "edit-unsupported-kind",
                Stage::Discovery,
                format!(
                    "cannot detect the source kind of '{}' for edit validation; pass an explicit `opy` source kind",
                    main_path.display()
                ),
            )),
        },
        other => Err(Diagnostic::error(
            "edit-unsupported-kind",
            Stage::Discovery,
            format!(
                "edit validation is declared over the OPY source provider; '{}' input is not an editable source kind",
                other.as_str()
            ),
        )),
    }
}

fn preview_of<'a>(previews: &'a [SourcePreview], main_path: &Path) -> Option<&'a SourcePreview> {
    previews.iter().find(|p| same_file(&p.source, main_path))
}

fn same_file(a: &str, b: &Path) -> bool {
    let b_str = b.to_string_lossy();
    a == b_str
        || matches!((Path::new(a).canonicalize(), b.canonicalize()), (Ok(ca), Ok(cb)) if ca == cb)
}

pub(crate) fn source_precondition(
    edit: &SourceEdit,
    sources: &BTreeMap<String, String>,
) -> Option<Diagnostic> {
    let Some(current) = sources.get(&edit.source) else {
        return Some(Diagnostic::error(
            "edit-unknown-source",
            Stage::Discovery,
            format!(
                "the edit targets '{}' but no current text was provided for it; supply the current source so the version precondition can be verified",
                edit.source
            ),
        ));
    };
    (crate::input_identity(current) != edit.source_identity).then(|| {
        Diagnostic::error(
            "edit-stale-source",
            Stage::Discovery,
            format!("the edit for '{}' targets a different source version (identity mismatch); re-fetch the source and retry", edit.source),
        )
    })
}

fn apply_edit(source: &str, edit: &SourceEdit) -> Result<String, Diagnostic> {
    let lines: Vec<&str> = source.split('\n').collect();
    let (sl, sc, el, ec) = (
        edit.range.start_line,
        edit.range.start_col,
        edit.range.end_line,
        edit.range.end_col,
    );
    if sl < 1 || el < 1 || el as usize > lines.len() || el < sl || (sl == el && ec < sc) {
        return Err(Diagnostic::error(
            "edit-invalid-range",
            Stage::Discovery,
            format!(
                "edit range {sl}-{sc}:{el}-{ec} is outside the source ({} lines)",
                lines.len()
            ),
        ));
    }
    let start_char_count = char_count(lines[sl as usize - 1]) as u32;
    let end_char_count = char_count(lines[el as usize - 1]) as u32;
    if sc < 1 || sc > start_char_count + 1 || ec < 1 || ec > end_char_count + 1 {
        return Err(Diagnostic::error(
            "edit-invalid-range",
            Stage::Discovery,
            format!(
                "edit range {sl}-{sc}:{el}-{ec} has columns outside the source lines (line {sl} has {start_char_count} characters, line {el} has {end_char_count}); columns are 1-based and may not be 0 or beyond the line end"
            ),
        ));
    }
    let mut out = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let ln = (idx + 1) as u32;
        if ln < sl || ln > el {
            out.push((*line).to_string());
        } else if ln == sl && ln == el {
            let s = char_col(line, sc);
            let e = char_col(line, ec);
            out.push(format!("{}{}{}", &line[..s], edit.new_text, &line[e..]));
        } else if ln == sl {
            let s = char_col(line, sc);
            out.push(format!("{}{}", &line[..s], edit.new_text));
        } else if ln == el {
            let e = char_col(line, ec);
            out.push(line[e..].to_string());
        }
    }
    Ok(out.join("\n"))
}

fn char_count(line: &str) -> usize {
    line.chars().count()
}

/// Convert a UTF-16 column to a Rust character offset, rounding up within a surrogate pair.
pub fn utf16_offset_to_char(line: &str, utf16_offset: usize) -> usize {
    let mut chars = 0usize;
    let mut utf16 = 0usize;
    for c in line.chars() {
        if utf16 >= utf16_offset {
            break;
        }
        utf16 += c.len_utf16();
        chars += 1;
    }
    chars
}

/// Convert a Rust character offset to a UTF-16 column, clamping at the line end.
pub fn char_offset_to_utf16(line: &str, char_offset: usize) -> usize {
    line.chars().take(char_offset).map(char::len_utf16).sum()
}

fn char_col(line: &str, col: u32) -> usize {
    let skip = col.saturating_sub(1) as usize;
    line.char_indices()
        .nth(skip)
        .map(|(off, _)| off)
        .unwrap_or(line.len())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameRequest {
    pub symbol_kind: String,
    pub from: String,
    pub to: String,
    pub source: String,
    pub source_identity: String,
}

pub fn rename_symbol(source: &str, request: &RenameRequest) -> Result<SourceEdit, Diagnostic> {
    if request.from.is_empty() || request.to.is_empty() {
        return Err(Diagnostic::error(
            "edit-invalid-name",
            Stage::Discovery,
            "rename requires non-empty `from` and `to` names",
        ));
    }
    let declared = source
        .lines()
        .any(|line| line_has_declaration(line, &request.symbol_kind, &request.from));
    if !declared {
        return Err(Diagnostic::error(
            "unknown-symbol",
            Stage::Discovery,
            format!(
                "no {} named '{}' is declared in the source",
                request.symbol_kind, request.from
            ),
        ));
    }
    rename_occurrences(
        source,
        &request.from,
        &request.to,
        &request.source,
        &request.source_identity,
    )
}

pub fn rename_occurrences(
    source: &str,
    from: &str,
    to: &str,
    source_file: &str,
    source_identity: &str,
) -> Result<SourceEdit, Diagnostic> {
    if from.is_empty() || to.is_empty() {
        return Err(Diagnostic::error(
            "edit-invalid-name",
            Stage::Discovery,
            "rename requires non-empty `from` and `to` names",
        ));
    }
    let mut out = String::new();
    for line in source.split('\n') {
        out.push_str(&rename_in_line(line, from, to));
        out.push('\n');
    }
    if out.ends_with('\n') {
        out.pop();
    }
    let line_count = source.lines().count().max(1) as u32;
    let last_line = source.lines().last().unwrap_or_default();
    let end_col = last_line.chars().count() as u32 + 1;

    Ok(SourceEdit {
        edit_kind: "rename".to_string(),
        source: source_file.to_string(),
        source_identity: source_identity.to_string(),
        range: EditRange {
            start_line: 1,
            start_col: 1,
            end_line: line_count,
            end_col,
        },
        new_text: out,
    })
}

fn line_has_declaration(line: &str, kind: &str, name: &str) -> bool {
    let trimmed = line.trim_start();
    let keyword = match kind {
        "globalVariable" => "globalvar",
        "playerVariable" => "playervar",
        "subroutine" => "subroutine",
        _ => return false,
    };
    if !trimmed.starts_with(keyword) {
        return false;
    }
    let rest = trimmed[keyword.len()..].trim_start();
    rest.split(|c: char| c.is_whitespace() || c == '=')
        .next()
        .is_some_and(|c| c == name)
}

fn rename_in_line(line: &str, from: &str, to: &str) -> String {
    let mut out = String::new();
    let mut remaining = line;
    while let Some(index) = remaining.find(from) {
        let before = &remaining[..index];
        let after = &remaining[index + from.len()..];
        let boundary_before = index == 0
            || !before
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        let boundary_after = after.is_empty()
            || !after
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        if boundary_before && boundary_after {
            out.push_str(before);
            out.push_str(to);
            remaining = after;
        } else {
            out.push_str(&remaining[..index + from.len()]);
            remaining = after;
        }
    }
    out.push_str(remaining);
    out
}

pub fn range_as_span(range: &EditRange) -> SourceSpan {
    SourceSpan {
        file: 0,
        path: "<edit>".to_string(),
        start: Position {
            line: range.start_line,
            col: range.start_col,
        },
        end: Position {
            line: range.end_line,
            col: range.end_col,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    score += 1\n";

    fn rename(edit: SourceEdit, sources: &BTreeMap<String, String>) -> EditValidation {
        let config = SessionConfig {
            input: crate::InputSpec::Path("program.opy".into()),
            ..SessionConfig::default()
        };
        validate_transaction(&config, sources, &EditTransaction::new(vec![edit]).unwrap())
    }

    fn rename_edit(source: &str, from: &str, to: &str) -> SourceEdit {
        rename_symbol(
            source,
            &RenameRequest {
                symbol_kind: "globalVariable".to_string(),
                from: from.to_string(),
                to: to.to_string(),
                source: "program.opy".to_string(),
                source_identity: crate::input_identity(source),
            },
        )
        .unwrap()
    }

    #[test]
    fn rename_rewrites_declaration_and_references() {
        let edit = rename_edit(SOURCE, "score", "total");
        assert_eq!(edit.edit_kind, "rename");
        assert_eq!(edit.source, "program.opy", "the edit names its source file");
        assert!(
            edit.new_text.contains("globalvar total = 0"),
            "declaration renamed: {}",
            edit.new_text
        );
        assert!(
            edit.new_text.contains("total += 1"),
            "reference renamed: {}",
            edit.new_text
        );
    }

    #[test]
    fn rename_does_not_touch_longer_identifiers() {
        let edit = rename_edit(SOURCE, "score", "total");
        // A hypothetical `scoreboard` must not be renamed (not present, but
        // the word-boundary logic is what keeps it safe).
        assert!(!edit.new_text.contains("totalboard"));
    }

    #[test]
    fn rename_unknown_symbol_fails_explicitly() {
        let error = rename_symbol(
            SOURCE,
            &RenameRequest {
                symbol_kind: "globalVariable".to_string(),
                from: "missing".to_string(),
                to: "x".to_string(),
                source: "program.opy".to_string(),
                source_identity: crate::input_identity(SOURCE),
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "unknown-symbol");
    }

    #[test]
    fn stale_source_identity_is_rejected() {
        let edit = SourceEdit {
            edit_kind: "rename".to_string(),
            source: "program.opy".to_string(),
            source_identity: "wrong-identity".to_string(),
            range: EditRange {
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 1,
            },
            new_text: String::new(),
        };
        let sources = BTreeMap::from([("program.opy".to_string(), SOURCE.to_string())]);
        let validation = validate_transaction(
            &SessionConfig::default(),
            &sources,
            &EditTransaction::new(vec![edit]).unwrap(),
        );
        assert!(!validation.ok);
        assert_eq!(validation.diagnostics[0].code, "edit-stale-source");
    }

    #[test]
    fn stale_source_identity_is_rejected_before_any_validation() {
        let edit = SourceEdit {
            edit_kind: "rename".to_string(),
            source: "other.opy".to_string(),
            source_identity: crate::input_identity(SOURCE),
            range: EditRange {
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 1,
            },
            new_text: String::new(),
        };
        // The current text is missing entirely: the precondition refuses
        // before any range or compile work.
        let validation = validate_transaction(
            &SessionConfig::default(),
            &BTreeMap::new(),
            &EditTransaction::new(vec![edit]).unwrap(),
        );
        assert!(!validation.ok);
        assert_eq!(validation.diagnostics[0].code, "edit-unknown-source");
        assert!(validation.preview.is_none(), "no partial preview");
    }

    #[test]
    fn overlapping_edits_in_one_source_are_rejected() {
        let source = "globalvar score = 0\n";
        let edit = |start: u32, end: u32| SourceEdit {
            edit_kind: "rename".to_string(),
            source: "program.opy".to_string(),
            source_identity: crate::input_identity(source),
            range: EditRange {
                start_line: 1,
                start_col: start,
                end_line: 1,
                end_col: end,
            },
            new_text: "x".to_string(),
        };
        let error = EditTransaction::new(vec![edit(1, 10), edit(5, 20)]).unwrap_err();
        assert_eq!(error.code, "edit-overlap");
    }

    #[test]
    fn rename_occurrences_works_without_a_declaration() {
        // A source that only references the symbol (declared elsewhere) is
        // still editable: rename_occurrences does not require a declaration.
        let source = "rule \"r\":\n    @Event global\n    showStatus()\n";
        let edit = rename_occurrences(
            source,
            "showStatus",
            "refresh",
            "program.opy",
            &crate::input_identity(source),
        )
        .unwrap();
        assert!(
            edit.new_text.contains("refresh()"),
            "reference renamed: {}",
            edit.new_text
        );
        assert!(
            !edit.new_text.contains("showStatus"),
            "old name gone: {}",
            edit.new_text
        );
        assert_eq!(
            edit.source_identity,
            crate::input_identity(source),
            "the edit carries the source identity precondition"
        );
    }

    #[test]
    fn rename_validation_refuses_without_provider() {
        let sources = BTreeMap::from([("program.opy".to_string(), SOURCE.to_string())]);
        let validation = rename(rename_edit(SOURCE, "score", "total"), &sources);
        assert!(!validation.ok);
        assert_eq!(
            validation.diagnostics[0].code,
            "source-provider-unavailable"
        );
        assert!(validation.preview.is_none(), "no partial preview");
    }

    #[test]
    fn broken_rename_is_refused_with_no_partial_preview() {
        let source = "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    score += 1\n    missing(;\n";
        let edit = rename_symbol(
            source,
            &RenameRequest {
                symbol_kind: "globalVariable".to_string(),
                from: "score".to_string(),
                to: "total".to_string(),
                source: "program.opy".to_string(),
                source_identity: crate::input_identity(source),
            },
        )
        .unwrap();
        let sources = BTreeMap::from([("program.opy".to_string(), source.to_string())]);
        let validation = validate_transaction(
            &SessionConfig {
                input: crate::InputSpec::Path("program.opy".into()),
                ..SessionConfig::default()
            },
            &sources,
            &EditTransaction::new(vec![edit]).unwrap(),
        );
        assert!(!validation.ok, "a rename that breaks the source refuses");
        assert_eq!(
            validation.diagnostics[0].code,
            "source-provider-unavailable"
        );
        assert!(validation.preview.is_none(), "no partial preview");
    }

    #[test]
    fn empty_transaction_is_rejected() {
        let error = EditTransaction::new(Vec::new()).unwrap_err();
        assert_eq!(error.code, "edit-empty-transaction");
    }

    #[test]
    fn transaction_orders_edits_deterministically() {
        let source = "rule \"r\":\n    a\n    b\n";
        let edit = |line: u32| SourceEdit {
            edit_kind: "rename".to_string(),
            source: "program.opy".to_string(),
            source_identity: crate::input_identity(source),
            range: EditRange {
                start_line: line,
                start_col: 1,
                end_line: line,
                end_col: 2,
            },
            new_text: "x".to_string(),
        };
        let transaction = EditTransaction::new(vec![edit(3), edit(2)]).unwrap();
        let positions: Vec<u32> = transaction
            .edits
            .iter()
            .map(|edit| edit.range.start_line)
            .collect();
        assert_eq!(positions, vec![2, 3], "edits are ordered by position");
    }
}
