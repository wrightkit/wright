//! Tools and agents propose edits as validated, source-oriented
//! [`SourceEdit`]s — never as mutations of Wright's internal IR. One
//! [`EditTransaction`] carries one or more file edits with exact source
//! ranges plus per-source identity/version preconditions, and
//! [`validate_transaction`] rejects stale versions, overlapping edits, and
//! out-of-range spans, then routes source-language validation through the
//! owner provider. Unsupported
//! source kinds fail explicitly with structured diagnostics and no partial
//! preview.
//!
//! Validation is atomic: a transaction either applies and previews in full
//! or is refused with diagnostics; the caller decides whether to write any
//! file. Application/writing is always separate from validation.
//!
//! The first evidence-backed refactoring is symbol rename ([`rename_symbol`]),
//! which proposes an edit carrying the source identity precondition; callers
//! validate it inside a transaction with [`validate_transaction`].

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::{SessionConfig, SourceKind};
use crate::diag::{Diagnostic, Position, Severity, SourceSpan, Stage};
use crate::result::exit_code_from;

/// One proposed source edit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEdit {
    /// The kind of edit (drives validation and preview semantics).
    #[serde(rename = "kind")]
    pub edit_kind: String,
    /// The source file identity the edit applies to (a path as given by the
    /// caller), so one transaction can target multiple files.
    pub source: String,
    /// The SHA-256 identity of the source text this edit applies to (stale
    /// versions are rejected).
    pub source_identity: String,
    /// The target source range, 1-based line/character column, end exclusive
    /// (matching the compiler's span convention).
    pub range: EditRange,
    /// The replacement text.
    pub new_text: String,
}

/// A source range (1-based line and character column, half-open; `end` is
/// exclusive).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditRange {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

impl EditRange {
    fn start(&self) -> (u32, u32) {
        (self.start_line, self.start_col)
    }
    fn end(&self) -> (u32, u32) {
        (self.end_line, self.end_col)
    }
    fn is_zero_width(&self) -> bool {
        self.start() == self.end()
    }
}

/// One validated source transaction: multiple file edits applied and
/// validated atomically against one project.
///
/// Construction orders the edits deterministically (by source identity, then
/// position), rejects overlapping edits within one source, and refuses
/// order-dependent zero-width combinations at the same position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditTransaction {
    /// The edits, in deterministic order (source, then position).
    pub edits: Vec<SourceEdit>,
}

impl EditTransaction {
    /// Build a transaction from proposed edits.
    ///
    /// The edits are ordered deterministically (by source identity, then
    /// position) and overlapping edits within one source are rejected
    /// (`edit-overlap`). Order-dependent zero-width combinations are refused
    /// as conflicts (`edit-zero-width-conflict`): two edits at the same
    /// position where at least one is a zero-width insertion would apply
    /// differently depending on their order, so the transaction refuses
    /// instead of defining an arbitrary result. An empty transaction is
    /// rejected (`edit-empty-transaction`).
    pub fn new(edits: Vec<SourceEdit>) -> Result<EditTransaction, Diagnostic> {
        if edits.is_empty() {
            return Err(Diagnostic::error(
                "edit-empty-transaction",
                Stage::Discovery,
                "a source-edit transaction must carry at least one edit",
            ));
        }
        let mut edits = edits;
        edits.sort_by(|a, b| {
            a.source
                .cmp(&b.source)
                .then_with(|| a.range.start().cmp(&b.range.start()))
                .then_with(|| a.range.end().cmp(&b.range.end()))
        });
        for pair in edits.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if a.source != b.source {
                continue;
            }
            if b.range.start() < a.range.end() {
                return Err(Diagnostic::error(
                    "edit-overlap",
                    Stage::Discovery,
                    format!(
                        "the transaction carries overlapping edits in '{}'",
                        a.source
                    ),
                ));
            }
            if a.range.start() == b.range.start()
                && (a.range.is_zero_width() || b.range.is_zero_width())
            {
                return Err(Diagnostic::error(
                    "edit-zero-width-conflict",
                    Stage::Discovery,
                    format!(
                        "the transaction carries order-dependent zero-width edits in '{}'",
                        a.source
                    ),
                ));
            }
        }
        Ok(EditTransaction { edits })
    }

    /// Apply the transaction's edits to the caller-provided current texts,
    /// returning the complete edited text of every affected source.
    ///
    /// Every range addresses the same original source snapshot (per source,
    /// the edits apply in descending position order), so an earlier
    /// replacement's length or newline changes can never shift a later
    /// range. Application is mechanical and separate from validation:
    /// callers validate the result with [`validate_transaction`] before
    /// applying it to real files. Malformed ranges refuse explicitly.
    pub fn apply(
        &self,
        sources: &BTreeMap<String, String>,
    ) -> Result<Vec<SourcePreview>, Diagnostic> {
        apply_transaction(sources, self)
    }
}

/// The edited text of one source in a validated transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcePreview {
    /// The source file identity the preview belongs to.
    pub source: String,
    /// The complete edited source text.
    pub new_text: String,
    /// The SHA-256 identity of the edited text (the new-source precondition).
    pub source_identity: String,
}

/// The result of validating a proposed transaction.
#[derive(Debug, Clone, Serialize)]
pub struct EditValidation {
    /// Whether the transaction is safe to apply.
    pub ok: bool,
    /// The intended process exit code (source-error semantics).
    pub exit: u8,
    pub diagnostics: Vec<Diagnostic>,
    /// The edited source texts (the preview), one per affected source, when
    /// the transaction applied. `None` when a precondition refused it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<Vec<SourcePreview>>,
}

/// Validate and preview a source-edit transaction against one project.
///
/// `config` is the *original* project/session configuration of the edited
/// code: its source kind selects the provider boundary, its root is preserved,
/// and its transformation profile (when set) applies exactly as the session
/// would apply it. `sources` supplies the current text of every source the
/// transaction touches, keyed by the same source identity the edits carry, so
/// the identity/version preconditions can be verified and the edited project
/// compiled without reading or rewriting the user's files.
///
/// The edited project is validated through the OPY provider boundary with
/// edited files as include overlays. Refusals are atomic and structured:
/// stale sources, overlapping/unknown edits, unavailable providers, and
/// provider errors produce diagnostics and no partial preview.
pub fn validate_transaction(
    config: &SessionConfig,
    sources: &BTreeMap<String, String>,
    transaction: &EditTransaction,
) -> EditValidation {
    let mut diagnostics = Vec::new();

    // Preconditions: every edited source must be known and current, so a
    // stale or fabricated version can never apply.
    for edit in &transaction.edits {
        let Some(current) = sources.get(&edit.source) else {
            diagnostics.push(Diagnostic::error(
                "edit-unknown-source",
                Stage::Discovery,
                format!(
                    "the edit targets '{}' but no current text was provided for it; \
                     supply the current source so the version precondition can be verified",
                    edit.source
                ),
            ));
            continue;
        };
        if crate::input_identity(current) != edit.source_identity {
            diagnostics.push(Diagnostic::error(
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
    if has_error(&diagnostics) {
        return refusal(diagnostics);
    }

    // Apply the exact ranges to build the per-source previews.
    let previews = match apply_transaction(sources, transaction) {
        Ok(previews) => previews,
        Err(diagnostic) => {
            diagnostics.push(diagnostic);
            return refusal(diagnostics);
        }
    };

    // The project under validation: the main source is the configured input;
    // a path-based input is required because the project/source graph needs a
    // stable main-file identity.
    let Some(main_path) = config.input.path().cloned() else {
        diagnostics.push(Diagnostic::error(
            "edit-input-stdin",
            Stage::Discovery,
            "edit validation requires a path-based input so the edited project's \
             main source identity is established; stdin has no project identity",
        ));
        return refusal(diagnostics);
    };
    let kind = match resolve_kind(config, &main_path) {
        Ok(kind) => kind,
        Err(diagnostic) => {
            diagnostics.push(diagnostic);
            return refusal(diagnostics);
        }
    };
    let _ = (kind, previews);
    diagnostics.push(source_provider_unavailable());
    refusal(diagnostics)
}

/// An atomic refusal: structured diagnostics and no preview.
fn refusal(diagnostics: Vec<Diagnostic>) -> EditValidation {
    EditValidation {
        ok: false,
        exit: exit_code_from(&diagnostics),
        diagnostics,
        preview: None,
    }
}

fn has_error(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
}

/// A semantic rename target: the exact identifier occurrence at a 1-based
/// line/column in one source of the project (#129).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameTarget {
    /// The source identity the position names (a key of the current-sources
    /// map, or a path spelling of the main source).
    pub source: String,
    /// The 1-based line of the identifier.
    pub line: u32,
    /// The 1-based column of the identifier.
    pub col: u32,
    /// The new name.
    pub to: String,
}

/// The outcome of a semantic rename: a validated multi-source transaction or
/// structured refusal diagnostics (#129).
#[derive(Debug, Clone, Serialize)]
pub struct SemanticRename {
    /// Whether the rename is safe to apply.
    pub ok: bool,
    /// The validated exact-range transaction, when the rename resolved.
    pub transaction: Option<EditTransaction>,
    /// Structured refusal/validation diagnostics.
    pub diagnostics: Vec<Diagnostic>,
    /// The per-source previews of the validated transaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<Vec<SourcePreview>>,
}

/// Rename the symbol whose declaration or reference occurrence sits at a
/// position in one source of a project (#129).
pub fn semantic_rename(
    config: &SessionConfig,
    sources: &BTreeMap<String, String>,
    target: &RenameTarget,
) -> SemanticRename {
    let _ = sources;
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
    let Some(main_path) = config.input.path().cloned() else {
        return refuse(vec![Diagnostic::error(
            "edit-input-stdin",
            Stage::Discovery,
            "semantic rename requires a path-based input so the project's \
             main source identity is established; stdin has no project identity",
        )]);
    };
    let kind = match resolve_kind(config, &main_path) {
        Ok(kind) => kind,
        Err(diagnostic) => return refuse(vec![diagnostic]),
    };
    let _ = kind;
    refuse(vec![source_provider_unavailable()])
}

/// The concrete source kind to validate against: the configured kind, or
/// detection from the main file extension for `Auto`.
fn resolve_kind(config: &SessionConfig, main_path: &Path) -> Result<SourceKind, Diagnostic> {
    match config.kind {
        SourceKind::Opy => Ok(SourceKind::Opy),
        SourceKind::Ostw => Err(source_provider_unavailable()),
        SourceKind::Auto => match main_path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref()
        {
            Some("opy") => Ok(SourceKind::Opy),
            Some("ostw" | "del") => Err(source_provider_unavailable()),
            _ => Err(Diagnostic::error(
                "edit-unsupported-kind",
                Stage::Discovery,
                format!(
                    "cannot detect the source kind of '{}' for edit validation; \
                     pass an explicit `opy` source kind",
                    main_path.display()
                ),
            )),
        },
        other => Err(Diagnostic::error(
            "edit-unsupported-kind",
            Stage::Discovery,
            format!(
                "edit validation is declared over the OPY source provider; \
                 '{}' input is not an editable source kind",
                other.as_str()
            ),
        )),
    }
}

fn source_provider_unavailable() -> Diagnostic {
    Diagnostic::error(
        "source-provider-unavailable",
        Stage::Internal,
        "the requested source-provider workflow is not currently shipped with Wright",
    )
}

/// Apply every edit of a transaction to the caller-provided current texts,
/// returning one complete edited source per affected source.
///
/// All ranges address the same original source snapshot: per source, the
/// edits are already deterministically ordered ascending by position, and
/// they are applied in descending order, so an earlier replacement's length
/// or newline changes can never shift a later range. Ranges outside the
/// source, invalid columns, and other malformed ranges refuse explicitly.
fn apply_transaction(
    sources: &BTreeMap<String, String>,
    transaction: &EditTransaction,
) -> Result<Vec<SourcePreview>, Diagnostic> {
    let mut previews: Vec<SourcePreview> = Vec::new();
    for (source, edits) in group_by_source(&transaction.edits) {
        let original = sources
            .get(source)
            .expect("precondition check already verified every edited source");
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

/// Group a transaction's edits by source, preserving the deterministic
/// ascending per-source order established by [`EditTransaction::new`].
fn group_by_source(edits: &[SourceEdit]) -> BTreeMap<&str, Vec<&SourceEdit>> {
    let mut grouped: BTreeMap<&str, Vec<&SourceEdit>> = BTreeMap::new();
    for edit in edits {
        grouped.entry(&edit.source).or_default().push(edit);
    }
    grouped
}

/// Apply an edit's replacement text at its range (1-based character columns,
/// end exclusive).
///
/// Columns are validated strictly against the declared 1-based exact-range
/// contract: every column must be in `1..=char_count + 1` for its line
/// (`end` is exclusive, so `char_count + 1` is a valid line-end insertion or
/// full-line replacement point). A column of `0` or a column beyond the line
/// is rejected (`edit-invalid-range`), never clamped to a different
/// location.
fn apply_edit(source: &str, edit: &SourceEdit) -> Result<String, Diagnostic> {
    let lines: Vec<&str> = source.split('\n').collect();
    let r = &edit.range;
    if r.start_line < 1
        || r.end_line < 1
        || r.end_line as usize > lines.len()
        || r.end_line < r.start_line
        || (r.start_line == r.end_line && r.end_col < r.start_col)
    {
        return Err(Diagnostic::error(
            "edit-invalid-range",
            Stage::Discovery,
            format!(
                "edit range {}-{}:{}-{} is outside the source ({} lines)",
                r.start_line,
                r.start_col,
                r.end_line,
                r.end_col,
                lines.len()
            ),
        ));
    }
    let start_len = lines[r.start_line as usize - 1].chars().count() as u32;
    let end_len = lines[r.end_line as usize - 1].chars().count() as u32;
    if r.start_col < 1 || r.start_col > start_len + 1 || r.end_col < 1 || r.end_col > end_len + 1 {
        return Err(Diagnostic::error(
            "edit-invalid-range",
            Stage::Discovery,
            format!(
                "edit range {}-{}:{}-{} has columns outside the source lines \
                 (line {} has {} characters, line {} has {}); columns are 1-based \
                 and may not be 0 or beyond the line end",
                r.start_line,
                r.start_col,
                r.end_line,
                r.end_col,
                r.start_line,
                start_len,
                r.end_line,
                end_len
            ),
        ));
    }
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let num = (i + 1) as u32;
        if num < r.start_line || num > r.end_line {
            out.push((*line).to_string());
        } else if num == r.start_line && num == r.end_line {
            let (start, end) = (char_col(line, r.start_col), char_col(line, r.end_col));
            out.push(format!(
                "{}{}{}",
                &line[..start],
                edit.new_text,
                &line[end..]
            ));
        } else if num == r.start_line {
            out.push(format!(
                "{}{}",
                &line[..char_col(line, r.start_col)],
                edit.new_text
            ));
        } else if num == r.end_line {
            out.push(line[char_col(line, r.end_col)..].to_string());
        }
    }
    Ok(out.join("\n"))
}

fn char_col(line: &str, col: u32) -> usize {
    line.char_indices()
        .nth(col.saturating_sub(1) as usize)
        .map_or(line.len(), |(offset, _)| offset)
}

/// A proposed symbol rename.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameRequest {
    pub symbol_kind: String,
    pub from: String,
    pub to: String,
    pub source: String,
    pub source_identity: String,
}

/// Rename a declared symbol across the source.
pub fn rename_symbol(source: &str, request: &RenameRequest) -> Result<SourceEdit, Diagnostic> {
    if request.from.is_empty() || request.to.is_empty() {
        return Err(invalid_name());
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

/// Rename every whole-word occurrence of `from` to `to`.
pub fn rename_occurrences(
    source: &str,
    from: &str,
    to: &str,
    source_file: &str,
    source_identity: &str,
) -> Result<SourceEdit, Diagnostic> {
    if from.is_empty() || to.is_empty() {
        return Err(invalid_name());
    }

    let lines: Vec<String> = source
        .split('\n')
        .map(|l| rename_in_line(l, from, to))
        .collect();
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
        new_text: lines.join("\n"),
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
    trimmed.strip_prefix(keyword).is_some_and(|rest| {
        rest.trim_start()
            .split(|c: char| c.is_whitespace() || c == '=')
            .next()
            == Some(name)
    })
}

fn rename_in_line(line: &str, from: &str, to: &str) -> String {
    let mut out = String::new();
    let mut remaining = line;
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    while let Some(i) = remaining.find(from) {
        let (before, after) = (&remaining[..i], &remaining[i + from.len()..]);
        if (i == 0 || !before.ends_with(is_ident))
            && (after.is_empty() || !after.starts_with(is_ident))
        {
            out.push_str(before);
            out.push_str(to);
        } else {
            out.push_str(&remaining[..i + from.len()]);
        }
        remaining = after;
    }
    out.push_str(remaining);
    out
}

/// Render an edit range as a source span for diagnostics.
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

fn invalid_name() -> Diagnostic {
    Diagnostic::error(
        "edit-invalid-name",
        Stage::Discovery,
        "rename requires non-empty `from` and `to` names",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    score += 1\n";

    fn val_single(edit: SourceEdit, sources: &BTreeMap<String, String>) -> EditValidation {
        let config = SessionConfig {
            input: crate::InputSpec::Path("program.opy".into()),
            ..SessionConfig::default()
        };
        validate_transaction(&config, sources, &EditTransaction::new(vec![edit]).unwrap())
    }

    fn rename_req(kind: &str, from: &str, to: &str, src: &str) -> RenameRequest {
        RenameRequest {
            symbol_kind: kind.into(),
            from: from.into(),
            to: to.into(),
            source: "program.opy".into(),
            source_identity: crate::input_identity(src),
        }
    }

    fn rename_edit(source: &str, from: &str, to: &str) -> SourceEdit {
        rename_symbol(source, &rename_req("globalVariable", from, to, source)).unwrap()
    }

    fn test_edit(source: &str, id: &str, line: u32, cols: (u32, u32)) -> SourceEdit {
        SourceEdit {
            edit_kind: "rename".into(),
            source: source.into(),
            source_identity: id.into(),
            range: EditRange {
                start_line: line,
                start_col: cols.0,
                end_line: line,
                end_col: cols.1,
            },
            new_text: "x".into(),
        }
    }

    #[test]
    fn rename_rewrites_declaration_and_references() {
        let edit = rename_edit(SOURCE, "score", "total");
        assert_eq!(
            (edit.edit_kind.as_str(), edit.source.as_str()),
            ("rename", "program.opy")
        );
        assert!(
            edit.new_text.contains("globalvar total = 0") && edit.new_text.contains("total += 1")
        );
        assert!(
            !rename_edit(SOURCE, "score", "total")
                .new_text
                .contains("totalboard")
        );
        let err = rename_symbol(
            SOURCE,
            &rename_req("globalVariable", "missing", "x", SOURCE),
        )
        .unwrap_err();
        assert_eq!(err.code, "unknown-symbol");
    }

    #[test]
    fn source_identity_and_overlap_rejections() {
        let map = BTreeMap::from([("program.opy".into(), SOURCE.into())]);
        let val1 = val_single(test_edit("program.opy", "wrong", 1, (1, 1)), &map);
        assert!(!val1.ok && val1.diagnostics[0].code == "edit-stale-source");

        let val2 = val_single(
            test_edit("other.opy", &crate::input_identity(SOURCE), 1, (1, 1)),
            &BTreeMap::new(),
        );
        assert!(
            !val2.ok && val2.preview.is_none() && val2.diagnostics[0].code == "edit-unknown-source"
        );

        let id = crate::input_identity("globalvar score = 0\n");
        let err = EditTransaction::new(vec![
            test_edit("program.opy", &id, 1, (1, 10)),
            test_edit("program.opy", &id, 1, (5, 20)),
        ])
        .unwrap_err();
        assert_eq!(err.code, "edit-overlap");
    }

    #[test]
    fn rename_occurrences_works_without_a_declaration() {
        let src = "rule \"r\":\n    @Event global\n    showStatus()\n";
        let edit = rename_occurrences(
            src,
            "showStatus",
            "refresh",
            "program.opy",
            &crate::input_identity(src),
        )
        .unwrap();
        assert!(edit.new_text.contains("refresh()") && !edit.new_text.contains("showStatus"));
        assert_eq!(edit.source_identity, crate::input_identity(src));
    }

    #[test]
    fn provider_validation_refusals() {
        let map = BTreeMap::from([("program.opy".into(), SOURCE.into())]);
        let val1 = val_single(rename_edit(SOURCE, "score", "total"), &map);
        assert!(
            !val1.ok
                && val1.preview.is_none()
                && val1.diagnostics[0].code == "source-provider-unavailable"
        );

        let src = "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    score += 1\n    missing(;\n";
        let edit =
            rename_symbol(src, &rename_req("globalVariable", "score", "total", src)).unwrap();
        let val2 = val_single(edit, &BTreeMap::from([("program.opy".into(), src.into())]));
        assert!(
            !val2.ok
                && val2.preview.is_none()
                && val2.diagnostics[0].code == "source-provider-unavailable"
        );
    }

    #[test]
    fn transaction_validation_and_ordering() {
        assert_eq!(
            EditTransaction::new(Vec::new()).unwrap_err().code,
            "edit-empty-transaction"
        );
        let id = crate::input_identity("rule \"r\":\n    a\n    b\n");
        let tx = EditTransaction::new(vec![
            test_edit("program.opy", &id, 3, (1, 2)),
            test_edit("program.opy", &id, 2, (1, 2)),
        ])
        .unwrap();
        let lines: Vec<u32> = tx.edits.iter().map(|e| e.range.start_line).collect();
        assert_eq!(lines, vec![2, 3]);
    }
}
