use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

mod columns;
use columns::{char_count, position_byte};
pub use columns::{char_offset_to_utf16, utf16_offset_to_char};

use serde::{Deserialize, Serialize};
use workshop_rs::catalog::{Catalog, Locale};

use crate::config::{SessionConfig, SourceKind};
use crate::diag::workshop_diag;
use crate::diag::{Diagnostic, Stage, source_provider_unavailable};
use crate::input::{self, ResolvedInput};
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

fn rebind_transaction_to_disk_input(
    transaction: EditTransaction,
    resolved: &ResolvedInput,
) -> Result<EditTransaction, Diagnostic> {
    EditTransaction::new(
        transaction
            .edits
            .into_iter()
            .map(|mut edit| {
                edit.source = resolved.display.clone();
                edit
            })
            .collect(),
    )
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
    let defaulted_sources;
    let (transaction, sources) = match sources {
        Some(sources) => (transaction, sources),
        None => {
            let transaction = match rebind_transaction_to_disk_input(transaction, &resolved) {
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

pub(crate) fn edit_kind_gate(kind: SourceKind, provider_operation: &str) -> Option<Diagnostic> {
    match kind {
        SourceKind::Workshop => None,
        SourceKind::Opy => Some(Diagnostic::error(
            "edit-requires-provider",
            Stage::Discovery,
            format!(
                "raw edit operations cover Workshop input; for OPY sources route through '{}' instead",
                provider_operation
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

pub(crate) fn same_source(source: &str, resolved: &ResolvedInput) -> bool {
    resolved
        .path
        .as_deref()
        .is_some_and(|path| same_file(source, path))
}

fn same_file(source: &str, path: &Path) -> bool {
    let source = crate::source_provider::provider_uri_path(source);
    source == path.to_string_lossy()
        || matches!(
            (Path::new(&source).canonicalize(), path.canonicalize()),
            (Ok(a), Ok(b)) if a == b
        )
}

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
        let path = PathBuf::from(crate::source_provider::provider_uri_path(&preview.source));
        let bytes = std::fs::read(&path).map_err(|error| {
            Diagnostic::error(
                "input-io",
                Stage::Discovery,
                format!("cannot read '{}': {}", preview.source, error),
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
                format!("cannot write '{}': {}", write.temporary.display(), error),
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

fn rollback_write(writes: &[PlannedWrite], failed: usize, error: std::io::Error) -> Diagnostic {
    remove_staged(&writes[failed..]);
    let mut diverged = Vec::new();
    for write in &writes[..failed] {
        if std::fs::write(&write.path, &write.original).is_err() {
            diverged.push(format!("'{}'", write.preview.source));
        }
    }
    let mut message = format!(
        "cannot replace '{}': {}",
        writes[failed].preview.source, error
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    const SOURCE: &str = "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    score += 1\n";

    fn catalog() -> &'static Catalog {
        workshop_rs::catalog::Catalog::builtin_ref().expect("builtin catalog")
    }

    #[test]
    fn same_file_binds_percent_encoded_file_uris() {
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
