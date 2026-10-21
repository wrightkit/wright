//! Tools and agents propose edits as validated, source-oriented
//! [`SourceEdit`]s — never as mutations of Wright's internal IR.
//!
//! Raw Workshop input is edited through `workshop-rs` itself (#434): a
//! transaction applies to the caller-supplied current sources and the
//! edited result is reparsed and validated through the session's own
//! Workshop parse path. Source languages stay at the provider boundary
//! (`providerValidateEdit`); there is no static fallback for them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use workshop_rs::catalog::{Catalog, Locale};

use crate::config::{SessionConfig, SourceKind};
use crate::diag::{Diagnostic, Stage, source_provider_unavailable};
use crate::input::{self, ResolvedInput};
use crate::result::exit_code_from;
use crate::session::workshop_diag;

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

    /// Apply the transaction to the supplied current sources. Every edit's
    /// `source` key must be present in `sources` and carry the identity of
    /// the supplied text — the precondition is checked per edit before any
    /// application.
    pub fn apply(
        &self,
        sources: &BTreeMap<String, String>,
    ) -> Result<Vec<SourcePreview>, Diagnostic> {
        for edit in &self.edits {
            if let Some(diagnostic) = source_precondition(edit, sources) {
                return Err(diagnostic);
            }
        }
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

/// `validateEditTransaction` (#434): every edit must name a supplied,
/// current source; the transaction applies atomically to the caller's
/// sources; and on raw Workshop input the edited project is reparsed and
/// validated through the session's own `workshop-rs` path, so an invalid
/// transaction refuses with the real Workshop diagnostics and no partial
/// preview. When `sources` is absent (#472) the current text of the files
/// the transaction names is read from disk — the resolved input's own
/// freshly read text, keyed by each edit's `source` spelling. Other source
/// languages route to `providerValidateEdit`.
pub fn validate_transaction(
    config: &SessionConfig,
    catalog: &Catalog,
    sources: Option<&BTreeMap<String, String>>,
    transaction: &EditTransaction,
) -> EditValidation {
    let refuse = |diagnostics: Vec<Diagnostic>| EditValidation {
        ok: false,
        exit: exit_code_from(&diagnostics),
        diagnostics,
        preview: None,
    };
    let resolved = match resolve_edit_input(config) {
        Ok(resolved) => resolved,
        Err(diagnostic) => return refuse(vec![diagnostic]),
    };
    if let Some(diagnostic) = edit_kind_gate(resolved.kind, "providerValidateEdit") {
        return refuse(vec![diagnostic]);
    }
    let transaction = match EditTransaction::new(transaction.edits.clone()) {
        Ok(transaction) => transaction,
        Err(diagnostic) => return refuse(vec![diagnostic]),
    };
    if let Some(edit) = transaction
        .edits
        .iter()
        .find(|edit| !same_source(&edit.source, &resolved))
    {
        return refuse(vec![Diagnostic::error(
            "edit-unknown-source",
            Stage::Discovery,
            format!(
                "the edit targets '{}' which is not the loaded input '{}'",
                edit.source, resolved.display
            ),
        )]);
    }
    // #472: an absent `sources` means the current text of the files the
    // transaction names comes from disk. `input::resolve` already read the
    // resolved input for this request, and after the check above every
    // edit names that one input — so the defaulted source binds once under
    // its display spelling and the edit spellings unify to it. Without the
    // unification, `apply` would group by the literal spelling and split
    // one file into divergent previews while overlap checks fell through
    // across spellings. A supplied `sources` is untouched: it stays the
    // caller-side precondition an embedder with unsaved buffers needs.
    let defaulted_sources;
    let (transaction, sources) = match sources {
        Some(sources) => (transaction, sources),
        None => {
            let transaction = match EditTransaction::new(
                transaction
                    .edits
                    .into_iter()
                    .map(|mut edit| {
                        edit.source = resolved.display.clone();
                        edit
                    })
                    .collect(),
            ) {
                Ok(transaction) => transaction,
                Err(diagnostic) => return refuse(vec![diagnostic]),
            };
            defaulted_sources = BTreeMap::from([(resolved.display.clone(), resolved.text.clone())]);
            (transaction, &defaulted_sources)
        }
    };
    let mut diagnostics: Vec<Diagnostic> = transaction
        .edits
        .iter()
        .filter_map(|edit| source_precondition(edit, sources))
        .collect();
    if diagnostics
        .iter()
        .any(|d| d.severity == crate::diag::Severity::Error)
    {
        return refuse(diagnostics);
    }
    let previews = match transaction.apply(sources) {
        Ok(previews) => previews,
        Err(diagnostic) => {
            diagnostics.push(diagnostic);
            return refuse(diagnostics);
        }
    };
    let edited = previews
        .iter()
        .find(|preview| same_source(&preview.source, &resolved))
        .map(|preview| preview.new_text.as_str())
        .or_else(|| {
            sources
                .iter()
                .find(|(source, _)| same_source(source, &resolved))
                .map(|(_, text)| text.as_str())
        })
        .unwrap_or(&resolved.text);
    match parse_workshop(edited, catalog, config.locale.as_deref(), &resolved) {
        Ok(_) => EditValidation {
            ok: true,
            exit: 0,
            diagnostics,
            preview: Some(previews),
        },
        Err(parse_diagnostics) => refuse(parse_diagnostics),
    }
}

/// Resolve the session input for an edit operation without consuming stdin:
/// an input with no path carries no source identity, so edit operations
/// refuse it outright.
pub(crate) fn resolve_edit_input(config: &SessionConfig) -> Result<ResolvedInput, Diagnostic> {
    if config.input.path().is_none() {
        return Err(edit_stdin_refusal());
    }
    input::resolve(config)
}

fn edit_stdin_refusal() -> Diagnostic {
    Diagnostic::error(
        "edit-input-stdin",
        Stage::Discovery,
        "edit operations require a path-based input; the input has no source identity",
    )
}

/// The source-kind boundary shared by the raw edit operations (#434): raw
/// Workshop input validates through `workshop-rs`; OPY and other source
/// languages belong to the provider operation (`providerValidateEdit`),
/// which keeps its own contract. The refusal names the operation a caller
/// should route to.
pub(crate) fn edit_kind_gate(kind: SourceKind, provider_operation: &str) -> Option<Diagnostic> {
    match kind {
        SourceKind::Workshop => None,
        SourceKind::Opy => Some(Diagnostic::error(
            "edit-requires-provider",
            Stage::Discovery,
            format!(
                "raw edit operations cover Workshop input; for OPY sources route through '{provider_operation}' instead"
            ),
        )),
        SourceKind::Ostw => Some(source_provider_unavailable()),
        other => Some(Diagnostic::error(
            "edit-unsupported-kind",
            Stage::Discovery,
            format!(
                "edit operations are not defined for '{}' input",
                other.as_str()
            ),
        )),
    }
}

/// Whether a `sources` key names the resolved input file: plain path
/// spellings and `file://` URI spellings (even percent-encoded) of the
/// same path compare by canonical identity.
pub(crate) fn same_source(source: &str, resolved: &ResolvedInput) -> bool {
    resolved
        .path
        .as_deref()
        .is_some_and(|path| same_file(source, path))
}

fn same_file(source: &str, path: &Path) -> bool {
    // `provider_uri_path` decodes `file://` URIs — `my%20dir` names the
    // same member as `my dir` (#636) — and passes plain path spellings
    // through.
    let source = crate::source_provider::provider_uri_path(source);
    source == path.to_string_lossy()
        || matches!(
            (Path::new(&source).canonicalize(), path.canonicalize()),
            (Ok(a), Ok(b)) if a == b
        )
}

/// Parse and validate Workshop text through the session's own path: locale
/// detection (with the session's override), then `workshop-rs` parse and
/// validation — a refused transaction carries the real Workshop
/// diagnostics, not a Wright-side paraphrase.
fn parse_workshop(
    text: &str,
    catalog: &Catalog,
    locale_override: Option<&str>,
    resolved: &ResolvedInput,
) -> Result<workshop_rs::Program, Vec<Diagnostic>> {
    let locale = workshop_rs::detect::resolve_locale(
        text,
        catalog,
        locale_override.map(Locale::new).as_ref(),
    )
    .map_err(|error| vec![workshop_diag(error, resolved)])?;
    let program = workshop_rs::parser::parse_with_context(text, catalog, &locale, catalog)
        .map_err(|error| vec![workshop_diag(error, resolved)])?;
    program
        .validate()
        .map_err(|error| vec![workshop_diag(error, resolved)])?;
    Ok(program)
}

/// Apply validated previews to the filesystem (#434): every file's current
/// bytes must still carry the identity the transaction was validated
/// against — a stale source refuses with no writes — and the whole set is
/// atomic (#734). Every replacement is staged as a sibling temporary
/// before any file is touched, then each temporary replaces its target
/// atomically; a failure rolls the files already replaced back to the
/// validated bytes.
pub fn write_previews(
    previews: &[SourcePreview],
    transaction: &EditTransaction,
) -> Result<Vec<String>, Diagnostic> {
    let mut expected: BTreeMap<&str, &str> = BTreeMap::new();
    for edit in &transaction.edits {
        expected.insert(edit.source.as_str(), edit.source_identity.as_str());
    }
    let mut writes: Vec<PlannedWrite> = Vec::with_capacity(previews.len());
    for preview in previews {
        // A provider-mapped edit names its source by `file://` URI (#583);
        // `provider_uri_path` also passes plain path spellings through.
        let path = PathBuf::from(crate::source_provider::provider_uri_path(&preview.source));
        let bytes = std::fs::read(&path).map_err(|error| {
            Diagnostic::error(
                "input-io",
                Stage::Discovery,
                format!("cannot read '{}': {error}", preview.source),
            )
        })?;
        let current = crate::input::decode_source_text(&bytes);
        let expected_identity = expected
            .get(preview.source.as_str())
            .expect("previews only cover edited sources");
        if crate::input_identity(&current) != *expected_identity {
            return Err(Diagnostic::error(
                "edit-stale-source",
                Stage::Discovery,
                format!(
                    "'{}' changed since the transaction was validated; re-validate and retry",
                    preview.source
                ),
            ));
        }
        let temporary = temporary_path(&path);
        writes.push(PlannedWrite {
            preview,
            path,
            temporary,
            original: bytes,
        });
    }
    for (index, write) in writes.iter().enumerate() {
        if let Err(error) = std::fs::write(&write.temporary, &write.preview.new_text) {
            remove_staged(&writes[..=index]);
            return Err(Diagnostic::error(
                "output-io",
                Stage::Emission,
                format!("cannot write '{}': {error}", write.temporary.display()),
            ));
        }
    }
    for (index, write) in writes.iter().enumerate() {
        if let Err(error) = std::fs::rename(&write.temporary, &write.path) {
            return Err(rollback_write(&writes, index, error));
        }
    }
    Ok(writes
        .iter()
        .map(|write| write.preview.source.clone())
        .collect())
}

/// One file of a staged write: the file to replace, the sibling temporary
/// its new content is staged in, and the exact bytes the identity check
/// captured so a later failure can roll the file back to what was
/// validated.
struct PlannedWrite<'a> {
    preview: &'a SourcePreview,
    path: PathBuf,
    temporary: PathBuf,
    original: Vec<u8>,
}

fn remove_staged(writes: &[PlannedWrite]) {
    for write in writes {
        let _ = std::fs::remove_file(&write.temporary);
    }
}

/// Roll back a failed commit: the staged files that were never committed
/// are removed and every file already replaced is written back to the
/// bytes the identity check captured. The restore writes those bytes
/// directly — the file already differs from the validated original, so a
/// second temporary could only strand another sibling. A file whose
/// restore fails keeps the new content, so the diagnostic names every
/// file the caller still has to recover.
fn rollback_write(writes: &[PlannedWrite], failed: usize, error: std::io::Error) -> Diagnostic {
    remove_staged(&writes[failed..]);
    let mut diverged = Vec::new();
    for write in &writes[..failed] {
        if std::fs::write(&write.path, &write.original).is_err() {
            diverged.push(format!("'{}'", write.preview.source));
        }
    }
    let mut message = format!(
        "cannot replace '{}': {error}",
        writes[failed].preview.source
    );
    if failed > 0 && diverged.is_empty() {
        message.push_str("; the files already written were restored to their validated content");
    } else if !diverged.is_empty() {
        message.push_str(&format!(
            "; the rollback could not restore {}",
            diverged.join(", ")
        ));
    }
    Diagnostic::error("output-io", Stage::Emission, message)
}

/// A sibling temporary file next to `path` — same directory, so the final
/// rename is atomic on the same filesystem.
fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(format!(".wright-{}.tmp", std::process::id()));
    path.with_file_name(name)
}

fn source_precondition(
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

/// Apply one edit as a standard half-open splice: positions resolve to byte
/// offsets and `source[..start] + new_text + source[end..]` is the result.
/// A range covering whole lines — start at a line's first column, end at a
/// later line's first column — removes the covered newlines with the lines,
/// the same interpretation any consumer of the `EditRange` contract uses.
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
    let start = position_byte(&lines, sl, sc);
    let end = position_byte(&lines, el, ec);
    Ok(format!(
        "{}{}{}",
        &source[..start],
        edit.new_text,
        &source[end..]
    ))
}

fn position_byte(lines: &[&str], line: u32, col: u32) -> usize {
    lines[..line as usize - 1]
        .iter()
        .map(|line| line.len() + 1)
        .sum::<usize>()
        + char_col(lines[line as usize - 1], col)
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

/// Deprecated `wright-embedding/v1` compatibility surface (#514): the
/// request consumed by [`rename_symbol`]. Kept source-compatible for v1
/// callers; removal requires a breaking embedding-contract transition.
#[deprecated = "v1 compatibility helper for `rename_symbol`; no supported rename path supersedes it"]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameRequest {
    pub symbol_kind: String,
    pub from: String,
    pub to: String,
    pub source: String,
    pub source_identity: String,
}

/// Deprecated `wright-embedding/v1` compatibility helper (#514): builds a
/// whole-file `rename` edit that rewrites every word-boundary occurrence of
/// a name in `source` (#129). The replacement is textual only — it cannot
/// distinguish semantic identity from strings, comments, or unrelated
/// same-named symbols — so it carries no semantic-identity guarantee and no
/// Wright product, CLI, LSP, agent, or embedding workflow may use it.
/// Removal requires a breaking embedding-contract transition.
#[deprecated = "textual whole-word proposal helper with no semantic-identity guarantee"]
#[allow(deprecated)]
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

// The textual rewrite behind the deprecated `rename_symbol` v1 helper
// (#514): private implementation, not a supported public surface.
fn rename_occurrences(
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    const SOURCE: &str = "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    score += 1\n";

    #[allow(deprecated)]
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

    fn catalog() -> &'static Catalog {
        workshop_rs::catalog::Catalog::builtin_ref().expect("builtin catalog")
    }

    #[test]
    fn same_file_binds_percent_encoded_file_uris() {
        // #636: `file:///…/my%20dir/main.ws` names the same member as the
        // decoded path — a space-carrying source spells either way.
        let path = Path::new("/tmp/my dir/main.ws");
        assert!(same_file("file:///tmp/my%20dir/main.ws", path));
        assert!(same_file("/tmp/my dir/main.ws", path));
        assert!(!same_file("file:///tmp/my%20dir/other.ws", path));
    }

    fn temp_workshop(text: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wright-edit-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("input.ws");
        std::fs::write(&path, text).unwrap();
        (dir, path)
    }

    const WORKSHOP: &str = "variables {\n    global:\n        0: score\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(score, Add(Global.score, 1));\n    }\n}\n";

    fn workshop_session(text: &str) -> (PathBuf, crate::CompilerSession) {
        let (dir, path) = temp_workshop(text);
        let session = crate::CompilerSession::new(SessionConfig {
            input: crate::InputSpec::Path(path),
            kind: SourceKind::Workshop,
            ..SessionConfig::default()
        })
        .expect("session");
        (dir, session)
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
        assert!(!edit.new_text.contains("totalboard"));
    }

    #[test]
    #[allow(deprecated)]
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
        let (dir, path) = temp_workshop(WORKSHOP);
        let edit = SourceEdit {
            edit_kind: "rename".to_string(),
            source: path.to_string_lossy().into_owned(),
            source_identity: "wrong-identity".to_string(),
            range: EditRange {
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 1,
            },
            new_text: String::new(),
        };
        let sources = BTreeMap::from([(path.to_string_lossy().into_owned(), WORKSHOP.to_string())]);
        let validation = validate_transaction(
            &SessionConfig {
                input: crate::InputSpec::Path(path),
                kind: SourceKind::Workshop,
                ..SessionConfig::default()
            },
            catalog(),
            Some(&sources),
            &EditTransaction::new(vec![edit]).unwrap(),
        );
        assert!(!validation.ok);
        assert_eq!(validation.diagnostics[0].code, "edit-stale-source");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_source_identity_is_rejected_before_any_validation() {
        let (dir, path) = temp_workshop(WORKSHOP);
        let edit = SourceEdit {
            edit_kind: "rename".to_string(),
            source: path.to_string_lossy().into_owned(),
            source_identity: crate::input_identity(SOURCE),
            range: EditRange {
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 1,
            },
            new_text: String::new(),
        };
        let validation = validate_transaction(
            &SessionConfig {
                input: crate::InputSpec::Path(path),
                kind: SourceKind::Workshop,
                ..SessionConfig::default()
            },
            catalog(),
            Some(&BTreeMap::new()),
            &EditTransaction::new(vec![edit]).unwrap(),
        );
        assert!(!validation.ok);
        assert_eq!(validation.diagnostics[0].code, "edit-unknown-source");
        assert!(validation.preview.is_none(), "no partial preview");
        let _ = std::fs::remove_dir_all(dir);
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
    fn workshop_edits_validate_by_reparsing_the_source() {
        let (dir, mut session) = workshop_session(WORKSHOP);
        let loaded = session.load().expect("loaded");
        let edit = SourceEdit {
            edit_kind: "edit".to_string(),
            source: loaded.input.display.clone(),
            source_identity: crate::input_identity(&loaded.input.text),
            range: EditRange {
                start_line: 3,
                start_col: 12,
                end_line: 3,
                end_col: 17,
            },
            new_text: "total".to_string(),
        };
        let sources = BTreeMap::from([(loaded.input.display.clone(), loaded.input.text.clone())]);
        let validation = validate_transaction(
            &session.config,
            session.catalog(),
            Some(&sources),
            &EditTransaction::new(vec![edit]).unwrap(),
        );
        assert!(validation.ok, "{:?}", validation.diagnostics);
        let preview = validation.preview.expect("preview");
        assert!(preview[0].new_text.contains("0: total"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn workshop_edits_refuse_with_the_reparse_diagnostics() {
        let (dir, mut session) = workshop_session(WORKSHOP);
        let loaded = session.load().expect("loaded");
        let edit = SourceEdit {
            edit_kind: "edit".to_string(),
            source: loaded.input.display.clone(),
            source_identity: crate::input_identity(&loaded.input.text),
            range: EditRange {
                start_line: 3,
                start_col: 12,
                end_line: 3,
                end_col: 17,
            },
            new_text: "0)".to_string(),
        };
        let sources = BTreeMap::from([(loaded.input.display.clone(), loaded.input.text.clone())]);
        let validation = validate_transaction(
            &session.config,
            session.catalog(),
            Some(&sources),
            &EditTransaction::new(vec![edit]).unwrap(),
        );
        assert!(!validation.ok);
        assert_eq!(validation.diagnostics[0].code, "parse-error");
        assert!(validation.preview.is_none(), "no partial preview");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn write_previews_refuses_stale_sources_and_updates_atomically() {
        // #434: `write_previews` rechecks each file's identity before
        // writing; a file that changed since validation refuses with no
        // write at all.
        let (dir, path) = temp_workshop(WORKSHOP);
        let new_text = WORKSHOP.replacen("0: score", "0: total", 1).replacen(
            "Global.score",
            "Global.total",
            1,
        );
        let (previews, transaction) = write_fixtures(&[(path.as_path(), WORKSHOP, &new_text)]);

        std::fs::write(&path, "// changed\n").unwrap();
        let error = write_previews(&previews, &transaction).unwrap_err();
        assert_eq!(error.code, "edit-stale-source");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "// changed\n",
            "a stale write leaves the file untouched"
        );

        std::fs::write(&path, WORKSHOP).unwrap();
        let written = write_previews(&previews, &transaction).expect("writes");
        assert_eq!(written.len(), 1);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("0: total"), "{text}");
        assert!(text.contains("Global.total"), "{text}");
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().contains(".wright-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "no temporary file leaks: {leftovers:?}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Build a write fixture: one preview per `(path, current, new_text)`
    /// plus a transaction carrying each source's identity precondition.
    fn write_fixtures(files: &[(&Path, &str, &str)]) -> (Vec<SourcePreview>, EditTransaction) {
        let previews = files
            .iter()
            .map(|(path, _, new_text)| SourcePreview {
                source: path.to_string_lossy().into_owned(),
                new_text: new_text.to_string(),
                source_identity: crate::input_identity(new_text),
            })
            .collect();
        let edits = files
            .iter()
            .map(|(path, current, new_text)| SourceEdit {
                edit_kind: "edit".to_string(),
                source: path.to_string_lossy().into_owned(),
                source_identity: crate::input_identity(current),
                range: EditRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 1,
                    end_col: 1,
                },
                new_text: new_text.to_string(),
            })
            .collect();
        let transaction = EditTransaction::new(edits).expect("transaction");
        (previews, transaction)
    }

    fn staged_temporaries(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().contains(".wright-"))
            })
            .collect()
    }

    #[cfg(unix)]
    #[test]
    fn a_write_failure_on_a_later_file_leaves_every_source_untouched() {
        use std::os::unix::fs::PermissionsExt;
        // #734: a multi-source transaction whose later file cannot be
        // written must leave every source at the validated bytes — the
        // write used to keep the earlier files' new content.
        let dir = std::env::temp_dir().join(format!(
            "wright-edit-atomic-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let dir_a = dir.join("a");
        let dir_b = dir.join("b");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();
        let path_a = dir_a.join("one.ws");
        let path_b = dir_b.join("two.ws");
        std::fs::write(&path_a, "original a\n").unwrap();
        std::fs::write(&path_b, "original b\n").unwrap();
        std::fs::set_permissions(&dir_b, std::fs::Permissions::from_mode(0o555)).unwrap();

        let (previews, transaction) = write_fixtures(&[
            (path_a.as_path(), "original a\n", "new a\n"),
            (path_b.as_path(), "original b\n", "new b\n"),
        ]);
        let error = write_previews(&previews, &transaction).unwrap_err();
        assert_eq!(error.code, "output-io");
        assert_eq!(
            std::fs::read_to_string(&path_a).unwrap(),
            "original a\n",
            "the earlier file keeps no new content"
        );
        assert_eq!(std::fs::read_to_string(&path_b).unwrap(), "original b\n");
        assert!(staged_temporaries(&dir_a).is_empty(), "no staged file");
        assert!(staged_temporaries(&dir_b).is_empty(), "no staged file");

        std::fs::set_permissions(&dir_b, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_replace_rolls_back_the_writes_already_committed() {
        // #734: a later file's replace fails after an earlier one
        // committed. The failure is deterministic here because a
        // `file://` URI and the plain path spell the same file: both
        // previews stage to one sibling temporary, so the second rename
        // has nothing to move. The committed file is rolled back to the
        // validated bytes and no temporary is left behind.
        let dir = std::env::temp_dir().join(format!(
            "wright-edit-rollback-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("input.ws");
        std::fs::write(&path, "original\n").unwrap();
        let source = path.to_string_lossy().into_owned();
        let uri = format!("file://{}", path.display());
        let preview = |source: &str, new_text: &str| SourcePreview {
            source: source.to_string(),
            new_text: new_text.to_string(),
            source_identity: crate::input_identity(new_text),
        };
        let edit = |source: &str, new_text: &str| SourceEdit {
            edit_kind: "edit".to_string(),
            source: source.to_string(),
            source_identity: crate::input_identity("original\n"),
            range: EditRange {
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 1,
            },
            new_text: new_text.to_string(),
        };
        let previews = vec![preview(&source, "new plain\n"), preview(&uri, "new uri\n")];
        let transaction =
            EditTransaction::new(vec![edit(&source, "x"), edit(&uri, "y")]).expect("transaction");

        let error = write_previews(&previews, &transaction).unwrap_err();
        assert_eq!(error.code, "output-io");
        assert!(
            error
                .message
                .contains("restored to their validated content"),
            "the rollback is reported: {:?}",
            error.message
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "original\n",
            "the committed write is rolled back"
        );
        assert!(staged_temporaries(&dir).is_empty(), "no staged file");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_rollback_diagnostic_names_every_source_it_could_not_restore() {
        // #734: a file whose restore fails still carries the new content,
        // so the diagnostic names each one the caller has to recover.
        let dir = std::env::temp_dir().join(format!(
            "wright-edit-diverged-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let missing = dir.join("missing");
        let first = SourcePreview {
            source: "one.ws".to_string(),
            new_text: "new\n".to_string(),
            source_identity: String::new(),
        };
        let second = SourcePreview {
            source: "two.ws".to_string(),
            new_text: "new\n".to_string(),
            source_identity: String::new(),
        };
        let writes = vec![
            PlannedWrite {
                preview: &first,
                path: missing.join("one.ws"),
                temporary: missing.join("one.ws.wright-tmp"),
                original: b"original one\n".to_vec(),
            },
            PlannedWrite {
                preview: &second,
                path: missing.join("two.ws"),
                temporary: missing.join("two.ws.wright-tmp"),
                original: b"original two\n".to_vec(),
            },
        ];
        let diagnostic = rollback_write(
            &writes,
            1,
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        );
        assert_eq!(diagnostic.code, "output-io");
        assert!(
            diagnostic.message.contains("cannot replace 'two.ws'"),
            "{:?}",
            diagnostic.message
        );
        assert!(
            diagnostic.message.contains("'one.ws'"),
            "the unrestored file is named: {:?}",
            diagnostic.message
        );
    }

    #[test]
    fn opy_input_routes_to_the_provider_operation() {
        let dir = std::env::temp_dir().join(format!(
            "wright-edit-opy-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("input.opy");
        std::fs::write(&path, "rule \"r\":\n    pass\n").unwrap();
        let session = crate::CompilerSession::new(SessionConfig {
            input: crate::InputSpec::Path(path.clone()),
            kind: SourceKind::Opy,
            ..SessionConfig::default()
        })
        .expect("session");
        let sources = BTreeMap::from([(
            path.to_string_lossy().into_owned(),
            "rule \"r\":\n    pass\n".to_string(),
        )]);
        let edit = SourceEdit {
            edit_kind: "edit".to_string(),
            source: path.to_string_lossy().into_owned(),
            source_identity: crate::input_identity("rule \"r\":\n    pass\n"),
            range: EditRange {
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 2,
            },
            new_text: "x".to_string(),
        };
        let validation = validate_transaction(
            &session.config,
            session.catalog(),
            Some(&sources),
            &EditTransaction::new(vec![edit]).unwrap(),
        );
        assert!(!validation.ok);
        assert_eq!(validation.diagnostics[0].code, "edit-requires-provider");
        assert!(
            validation.diagnostics[0]
                .message
                .contains("providerValidateEdit"),
            "the refusal names the provider operation"
        );
        let _ = std::fs::remove_dir_all(dir);
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

    #[test]
    fn wire_transactions_recheck_the_transaction_invariants() {
        let (dir, session) = workshop_session(WORKSHOP);
        let path = dir.join("input.ws");
        let key = path.to_string_lossy().into_owned();
        let sources = BTreeMap::from([(key.clone(), WORKSHOP.to_string())]);
        let identity = crate::input_identity(WORKSHOP);
        let wire = |edits: serde_json::Value| {
            serde_json::from_value::<EditTransaction>(serde_json::json!({ "edits": edits }))
                .expect("deserializes")
        };
        let edit = |start_col: u32, end_col: u32, new_text: &str| {
            serde_json::json!({
                "kind": "edit",
                "source": key.clone(),
                "source_identity": identity.clone(),
                "range": {
                    "start_line": 11,
                    "start_col": start_col,
                    "end_line": 11,
                    "end_col": end_col,
                },
                "new_text": new_text,
            })
        };

        let validation = validate_transaction(
            &session.config,
            session.catalog(),
            Some(&sources),
            &wire(serde_json::json!([])),
        );
        assert!(!validation.ok);
        assert_eq!(validation.diagnostics[0].code, "edit-empty-transaction");

        let validation = validate_transaction(
            &session.config,
            session.catalog(),
            Some(&sources),
            &wire(serde_json::json!([
                edit(29, 34, "total"),
                edit(30, 35, "x")
            ])),
        );
        assert!(!validation.ok);
        assert_eq!(validation.diagnostics[0].code, "edit-overlap");

        let validation = validate_transaction(
            &session.config,
            session.catalog(),
            Some(&sources),
            &wire(serde_json::json!([
                edit(36, 39, "Subtract"),
                edit(29, 34, "total")
            ])),
        );
        assert!(validation.ok, "{:?}", validation.diagnostics);
        let text = &validation.preview.as_ref().unwrap()[0].new_text;
        assert!(
            text.contains("Set Global Variable(total, Subtract(Global.score, 1))"),
            "{text}"
        );

        let mut wider = sources.clone();
        wider.insert("other.ws".to_string(), WORKSHOP.to_string());
        let foreign = serde_json::json!({
            "kind": "edit",
            "source": "other.ws",
            "source_identity": identity,
            "range": {"start_line": 3, "start_col": 12, "end_line": 3, "end_col": 17},
            "new_text": "total",
        });
        let validation = validate_transaction(
            &session.config,
            session.catalog(),
            Some(&wider),
            &wire(serde_json::json!([foreign])),
        );
        assert!(!validation.ok);
        assert_eq!(validation.diagnostics[0].code, "edit-unknown-source");
        let _ = std::fs::remove_dir_all(dir);
    }
}
