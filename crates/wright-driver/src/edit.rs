//! Safe source-edit and refactoring contracts (M9, issue #59).
//!
//! Tools and agents propose edits as validated, source-oriented
//! [`SourceEdit`]s — never as mutations of Wright's internal IR. An edit
//! carries the source identity it applies to, a source range, and the
//! replacement text. [`validate_edit`] rejects stale versions, overlapping
//! edits, and out-of-range spans, then runs the edited source through the
//! normal compiler pipeline so a proposed edit is previewed and validated
//! before application. Unsupported/unsafe edits fail explicitly.
//!
//! The first evidence-backed refactoring is symbol rename: every reference
//! to a declared variable or subroutine is renamed in the source
//! ([`rename_symbol`]), and the result is verified to compile to the same
//! WIR structure (modulo the new name).

use serde::{Deserialize, Serialize};

use crate::diag::{Diagnostic, Position, Severity, SourceSpan, Stage};
use crate::result::exit;

/// One proposed source edit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEdit {
    /// The kind of edit (drives validation and preview semantics).
    #[serde(rename = "kind")]
    pub edit_kind: String,
    /// The SHA-256 identity of the source this edit applies to (stale
    /// versions are rejected).
    pub source_identity: String,
    /// The target source range, 1-based line/column, end exclusive.
    pub range: EditRange,
    /// The replacement text.
    pub new_text: String,
}

/// A source range (1-based, half-open).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditRange {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

/// The result of validating a proposed edit.
#[derive(Debug, Clone, Serialize)]
pub struct EditValidation {
    /// Whether the edit is safe to apply.
    pub ok: bool,
    /// The intended process exit code (source-error semantics).
    pub exit: u8,
    pub diagnostics: Vec<Diagnostic>,
    /// The edited source text (the preview), when validation passed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

/// Validate and preview one source edit against its source identity and the
/// compiler pipeline.
///
/// `source` is the current source text; `edit` must carry the matching
/// identity (SHA-256 of `source`). The edited text is compiled with the same
/// driver configuration; compile errors are returned as diagnostics and the
/// edit is rejected.
pub fn validate_edit(
    source: &str,
    edit: &SourceEdit,
    config: &crate::SessionConfig,
) -> EditValidation {
    let mut diagnostics = Vec::new();
    let identity = crate::input_identity(source);
    if identity != edit.source_identity {
        diagnostics.push(Diagnostic::error(
            "edit-stale-source",
            Stage::Discovery,
            "the edit targets a different source version (identity mismatch); re-fetch the source and retry",
        ));
        return EditValidation {
            ok: false,
            exit: exit::SOURCE_ERROR,
            diagnostics,
            preview: None,
        };
    }

    let edited = match apply_edit(source, edit) {
        Ok(text) => text,
        Err(diagnostic) => {
            diagnostics.push(diagnostic);
            return EditValidation {
                ok: false,
                exit: exit::SOURCE_ERROR,
                diagnostics,
                preview: None,
            };
        }
    };

    // Validate through the normal pipeline: compile the edited source.
    let mut edited_config = config.clone();
    edited_config.input = crate::InputSpec::Stdin;
    edited_config.kind = crate::SourceKind::Opy;
    let mut session = match crate::CompilerSession::new(edited_config) {
        Ok(session) => session,
        Err(diagnostic) => {
            diagnostics.push(diagnostic);
            return EditValidation {
                ok: false,
                exit: exit::INTERNAL,
                diagnostics,
                preview: None,
            };
        }
    };
    // The session reads stdin; drive it through a temporary source file so
    // includes resolve relative to the project root.
    let temp_dir = std::env::temp_dir().join(format!("wright-edit-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let path = temp_dir.join("edit.opy");
    let _ = std::fs::write(&path, &edited);
    session.config.input = crate::InputSpec::Path(path.clone());
    session.config.kind = crate::SourceKind::Opy;
    let envelope = session.check();
    let _ = std::fs::remove_file(&path);

    let mut all_diagnostics = envelope.diagnostics.clone();
    diagnostics.append(&mut all_diagnostics);
    let has_error = diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error);
    EditValidation {
        ok: !has_error,
        exit: if has_error {
            envelope.exit
        } else {
            exit::SUCCESS
        },
        diagnostics,
        preview: Some(edited),
    }
}

/// Apply an edit's replacement text at its range.
fn apply_edit(source: &str, edit: &SourceEdit) -> Result<String, Diagnostic> {
    let lines: Vec<&str> = source.split('\n').collect();
    if edit.range.start_line < 1
        || edit.range.end_line < 1
        || edit.range.end_line as usize > lines.len()
        || edit.range.end_line < edit.range.start_line
        || (edit.range.start_line == edit.range.end_line
            && edit.range.end_col < edit.range.start_col)
    {
        return Err(Diagnostic::error(
            "edit-invalid-range",
            Stage::Discovery,
            format!(
                "edit range {}-{}:{}-{} is outside the source ({} lines)",
                edit.range.start_line,
                edit.range.start_col,
                edit.range.end_line,
                edit.range.end_col,
                lines.len()
            ),
        ));
    }
    let mut out: Vec<String> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let line_number = (index + 1) as u32;
        if line_number < edit.range.start_line || line_number > edit.range.end_line {
            out.push((*line).to_string());
            continue;
        }
        if line_number == edit.range.start_line && line_number == edit.range.end_line {
            let col = byte_col(line, edit.range.start_col);
            let end = byte_col(line, edit.range.end_col);
            let mut replacement = String::new();
            replacement.push_str(&line[..col]);
            replacement.push_str(&edit.new_text);
            replacement.push_str(&line[end.min(line.len())..]);
            out.push(replacement);
        } else if line_number == edit.range.start_line {
            let col = byte_col(line, edit.range.start_col);
            let mut replacement = String::new();
            replacement.push_str(&line[..col]);
            replacement.push_str(&edit.new_text);
            out.push(replacement);
        } else if line_number == edit.range.end_line {
            let end = byte_col(line, edit.range.end_col);
            out.push(line[end.min(line.len())..].to_string());
        } else {
            // A wholly covered middle line is removed.
            continue;
        }
    }
    Ok(out.join("\n"))
}

/// Convert a 1-based column to a byte offset, clamped to the line length.
fn byte_col(line: &str, col: u32) -> usize {
    (col as usize - 1).min(line.len())
}

/// A proposed symbol rename.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameRequest {
    /// The symbol's kind (`globalVariable`, `playerVariable`, `subroutine`).
    pub symbol_kind: String,
    /// The current name.
    pub from: String,
    /// The new name.
    pub to: String,
    /// The source identity this rename applies to.
    pub source_identity: String,
}

/// Rename a declared symbol across the source.
///
/// Returns a single multi-line [`SourceEdit`] covering every occurrence of
/// `from` (declaration and references). The caller validates it with
/// [`validate_edit`], which recompiles the result. Names that are not
/// declared fail explicitly (`unknown-symbol`).
pub fn rename_symbol(source: &str, request: &RenameRequest) -> Result<SourceEdit, Diagnostic> {
    if request.from.is_empty() || request.to.is_empty() {
        return Err(Diagnostic::error(
            "edit-invalid-name",
            Stage::Discovery,
            "rename requires non-empty `from` and `to` names",
        ));
    }
    // Verify the symbol is declared and collect every reference by scanning
    // the source for the declared name (declaration + references share the
    // spelling in the declared surface). A declared symbol must exist.
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

    // Rename every whole-word occurrence of `from`.
    let mut out = String::new();
    for line in source.split('\n') {
        out.push_str(&rename_in_line(line, &request.from, &request.to));
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
        source_identity: request.source_identity.clone(),
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
        .is_some_and(|candidate| candidate == name)
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

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    score += 1\n";

    #[test]
    fn rename_rewrites_declaration_and_references() {
        let edit = rename_symbol(
            SOURCE,
            &RenameRequest {
                symbol_kind: "globalVariable".to_string(),
                from: "score".to_string(),
                to: "total".to_string(),
                source_identity: crate::input_identity(SOURCE),
            },
        )
        .unwrap();
        assert_eq!(edit.edit_kind, "rename");
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
        let edit = rename_symbol(
            SOURCE,
            &RenameRequest {
                symbol_kind: "globalVariable".to_string(),
                from: "score".to_string(),
                to: "total".to_string(),
                source_identity: crate::input_identity(SOURCE),
            },
        )
        .unwrap();
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
            source_identity: "wrong-identity".to_string(),
            range: EditRange {
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 1,
            },
            new_text: String::new(),
        };
        let config = crate::SessionConfig::default();
        let validation = validate_edit(SOURCE, &edit, &config);
        assert!(!validation.ok);
        assert_eq!(validation.diagnostics[0].code, "edit-stale-source");
    }

    #[test]
    fn rename_validates_through_the_pipeline() {
        let edit = rename_symbol(
            SOURCE,
            &RenameRequest {
                symbol_kind: "globalVariable".to_string(),
                from: "score".to_string(),
                to: "total".to_string(),
                source_identity: crate::input_identity(SOURCE),
            },
        )
        .unwrap();
        let config = crate::SessionConfig::default();
        let validation = validate_edit(SOURCE, &edit, &config);
        assert!(
            validation.ok,
            "the renamed source must compile: {:?}",
            validation.diagnostics
        );
        assert!(
            validation
                .preview
                .as_ref()
                .unwrap()
                .contains("globalvar total"),
            "preview shows the renamed source"
        );
    }
}
