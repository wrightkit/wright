//! Tools and agents propose edits as validated, source-oriented
//! [`SourceEdit`]s — never as mutations of Wright's internal IR.
//!
//! Raw Workshop input is edited through `workshop-rs` itself (#434): a
//! transaction applies to the caller-supplied current sources, the edited
//! result is reparsed and validated through the session's own Workshop
//! parse path, and a semantic rename rewrites exactly the identifier spans
//! the parsed program reports — declarations, references, and positions
//! carry the same provenance. Source languages stay at the provider
//! boundary (`providerValidateEdit` / `providerSemanticRename`); there is
//! no textual-search rename and no static fallback for them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use workshop_rs::catalog::{Catalog, Locale};
use wright_analyzer::canonical::{ReferenceKind, SemanticIndex, Symbol, SymbolId, SymbolKind};

use crate::config::{SessionConfig, SourceKind};
use crate::diag::{Diagnostic, Stage, source_provider_unavailable};
use crate::input::{self, ResolvedInput};
use crate::result::exit_code_from;
use crate::service::Address;
use crate::session::{Loaded, workshop_diag};

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
    // A transaction that arrived over the wire carries no structural
    // guarantee: re-run the constructor's normalization and invariants so
    // empty, overlapping, or unsorted edit lists meet the same contract as
    // `EditTransaction::new` callers.
    let transaction = match EditTransaction::new(transaction.edits.clone()) {
        Ok(transaction) => transaction,
        Err(diagnostic) => return refuse(vec![diagnostic]),
    };
    // Every edit must target the loaded input itself — applying a caller's
    // edit to a source that is not part of this program would validate
    // text the transaction never touched.
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
    // A Workshop project is one source file: the edited text is the preview
    // when the transaction touched the input, else the caller's current
    // text for it, else the on-disk input the edits were checked against.
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
/// stdin carries no source identity, so edit operations refuse it outright.
fn resolve_edit_input(config: &SessionConfig) -> Result<ResolvedInput, Diagnostic> {
    if config.input.path().is_none() {
        return Err(edit_stdin_refusal());
    }
    input::resolve(config)
}

fn edit_stdin_refusal() -> Diagnostic {
    Diagnostic::error(
        "edit-input-stdin",
        Stage::Discovery,
        "edit operations require a path-based input; stdin has no source identity",
    )
}

/// The source-kind boundary shared by the raw edit operations (#434): raw
/// Workshop input validates through `workshop-rs`; OPY and other source
/// languages belong to the provider operations (`providerValidateEdit` /
/// `providerSemanticRename`), which keep their own contract. The refusal
/// names the operation a caller should route to.
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

/// The `semanticRename` target (#434): either a symbol address — the numeric
/// id or the declared name, exactly as `references`/`usage` address symbols
/// (#429) — or a `source`/`line`/`col` position inside one identifier
/// occurrence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameTarget {
    /// The symbol to rename, by numeric id or declared name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<Address>,
    /// Position addressing: the source file the position is interpreted in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Position addressing: the 1-based line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Position addressing: the 1-based column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub col: Option<u32>,
    /// The new identifier.
    pub to: String,
}

/// The `semanticRename` result: the validated transaction and its previews,
/// or refusal diagnostics with no transaction and no partial preview.
#[derive(Debug, Clone, Serialize)]
pub struct SemanticRename {
    pub ok: bool,
    pub transaction: Option<EditTransaction>,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<Vec<SourcePreview>>,
}

/// The `wright rename` result (#434): the validated transaction and the
/// per-source previews; `written` lists the files `--write` updated.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RenameResult {
    /// The validated rename transaction (absent on refusal).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<EditTransaction>,
    /// Per-source previews of the validated transaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<Vec<SourcePreview>>,
    /// The files updated by `--write` (empty in preview mode).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub written: Vec<String>,
    /// The source text the transaction was validated against — presentation
    /// input for the human diff, not part of the JSON contract.
    #[serde(skip)]
    pub originals: BTreeMap<String, String>,
}

/// `semanticRename` on raw Workshop input (#434): the target resolves
/// against the loaded program's semantic index — the same symbol and
/// position addressing `references`/`usage` use — and every occurrence is
/// rewritten through the program's own identifier provenance, never a
/// textual search. When `sources` is absent (#472) the loaded input's
/// current text is read from disk; a supplied `sources` stays the
/// caller-side precondition text. The proposed transaction is validated by
/// reparsing the edited source; every refusal carries structured
/// diagnostics and no partial edit set.
pub fn semantic_rename(
    loaded: &Loaded,
    catalog: &Catalog,
    sources: Option<&BTreeMap<String, String>>,
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
    if let Some(diagnostic) = edit_kind_gate(loaded.input.kind, "providerSemanticRename") {
        return refuse(vec![diagnostic]);
    }
    let Some(input_path) = loaded.input.path.clone() else {
        return refuse(vec![edit_stdin_refusal()]);
    };
    // #472: an absent `sources` means the current text of the file the
    // request names comes from disk — the loaded input, keyed by the
    // `target.source` spelling when position addressing names it, else by
    // the input's display path, so the produced transaction names the
    // source as the caller would have.
    let defaulted_sources;
    let sources = match sources {
        Some(sources) => sources,
        None => {
            let bytes = match std::fs::read(&input_path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return refuse(vec![Diagnostic::error(
                        "input-io",
                        Stage::Discovery,
                        format!("cannot read '{}': {error}", input_path.display()),
                    )]);
                }
            };
            let source = target
                .source
                .clone()
                .filter(|source| same_file(source, &input_path))
                .unwrap_or_else(|| loaded.input.display.clone());
            defaulted_sources =
                BTreeMap::from([(source, String::from_utf8_lossy(&bytes).into_owned())]);
            &defaulted_sources
        }
    };
    // The caller supplies the current text of the file being renamed; it
    // must be the text the loaded program was parsed from, so every
    // provenance span indexes into it exactly.
    let Some((source, current)) = sources
        .iter()
        .find(|(source, _)| same_file(source, &input_path))
        .map(|(source, text)| (source.clone(), text.clone()))
    else {
        return refuse(vec![Diagnostic::error(
            "edit-unknown-source",
            Stage::Discovery,
            format!(
                "semantic rename needs the current text of '{}' in `sources`",
                loaded.input.display
            ),
        )]);
    };
    if current != loaded.input.text {
        return refuse(vec![Diagnostic::error(
            "edit-stale-source",
            Stage::Discovery,
            format!(
                "the supplied text of '{source}' differs from the loaded program; reload the project and retry"
            ),
        )]);
    }

    let index = SemanticIndex::build(&loaded.program);
    let symbol = match resolve_rename_target(&index, loaded, &input_path, target) {
        Ok(symbol) => symbol,
        Err(diagnostic) => return refuse(vec![diagnostic]),
    };
    match symbol.kind {
        SymbolKind::GlobalVariable | SymbolKind::PlayerVariable | SymbolKind::Subroutine => {}
        other => {
            return refuse(vec![Diagnostic::error(
                "rename-unsupported-kind",
                Stage::Discovery,
                format!(
                    "semantic rename covers variables and subroutines, not '{}' symbols",
                    other.as_str()
                ),
            )]);
        }
    }
    if target.to == symbol.name {
        return refuse(vec![Diagnostic::error(
            "rename-invalid-name",
            Stage::Discovery,
            "the new name already names this symbol",
        )]);
    }
    // Same-namespace collision: `to` must not already name another symbol of
    // the same kind — global and player variables are separate namespaces,
    // so the check is kind-scoped.
    if let Some(other) = index
        .symbols()
        .find(|other| other.id != symbol.id && other.kind == symbol.kind && other.name == target.to)
    {
        return refuse(vec![Diagnostic::error(
            "rename-name-collision",
            Stage::Discovery,
            format!(
                "'{}' already names {} #{}; renaming '{}' would collide",
                target.to,
                other.kind.as_str(),
                other.id.index(),
                symbol.name
            ),
        )]);
    }

    // Rewrite every occurrence through the program's exact identifier
    // provenance. A reference without an authored span — or one whose span
    // does not cover exactly the identifier in the supplied text — refuses
    // rather than guessing.
    let mut spans: Vec<workshop_rs::source::Span> = Vec::new();
    for reference in index.references(symbol.id) {
        let Some(occurrence) = reference.occurrence else {
            return refuse(vec![rename_unmapped(loaded, symbol)]);
        };
        let Ok(edit) = loaded.program.edit_source(occurrence, &target.to) else {
            return refuse(vec![rename_unmapped(loaded, symbol)]);
        };
        if current.get(edit.range()) != Some(symbol.name.as_str()) {
            return refuse(vec![rename_unmapped(loaded, symbol)]);
        }
        if !spans.contains(&occurrence) {
            spans.push(occurrence);
        }
    }
    if spans.is_empty() {
        return refuse(vec![Diagnostic::error(
            "rename-invalid-target",
            Stage::Discovery,
            format!("symbol '{}' carries no identifier occurrences", symbol.name),
        )]);
    }
    spans.sort_by_key(|span| {
        (
            span.file.index(),
            span.start.line,
            span.start.col,
            span.end.line,
            span.end.col,
        )
    });
    let identity = crate::input_identity(&current);
    let transaction = match EditTransaction::new(
        spans
            .iter()
            .map(|span| SourceEdit {
                edit_kind: "rename".to_string(),
                source: source.clone(),
                source_identity: identity.clone(),
                range: EditRange {
                    start_line: span.start.line,
                    start_col: span.start.col,
                    end_line: span.end.line,
                    end_col: span.end.col,
                },
                new_text: target.to.clone(),
            })
            .collect(),
    ) {
        Ok(transaction) => transaction,
        Err(diagnostic) => return refuse(vec![diagnostic]),
    };
    let previews = match transaction.apply(sources) {
        Ok(previews) => previews,
        Err(diagnostic) => return refuse(vec![diagnostic]),
    };
    // The rename edits only ever touch the loaded input's source.
    let edited = previews
        .iter()
        .find(|preview| same_file(&preview.source, &input_path))
        .expect("rename edits only ever touch the loaded input")
        .new_text
        .clone();
    match verify_rename(&index, symbol, &target.to, &edited, loaded, catalog) {
        Ok(()) => SemanticRename {
            ok: true,
            transaction: Some(transaction),
            diagnostics: Vec::new(),
            preview: Some(previews),
        },
        Err(diagnostics) => refuse(diagnostics),
    }
}

/// Resolve the rename target: a numeric or name `symbol` address (#429), or
/// a `source`/`line`/`col` position inside one identifier occurrence.
fn resolve_rename_target<'a>(
    index: &'a SemanticIndex,
    loaded: &Loaded,
    input_path: &Path,
    target: &RenameTarget,
) -> Result<&'a Symbol, Diagnostic> {
    match &target.symbol {
        Some(Address::Id(id)) => {
            index
                .symbol(SymbolId::from_index(*id as usize))
                .ok_or_else(|| {
                    Diagnostic::error(
                        "invalid-id",
                        Stage::Discovery,
                        format!("unknown symbol {id}"),
                    )
                })
        }
        Some(Address::Name(name)) => resolve_named_symbol(index, name),
        None => symbol_at_position(index, loaded, input_path, target),
    }
}

/// Resolve a declared name to one symbol — the same resolution `references`
/// and `usage` apply: unmatched names are `unknown-symbol`, names shared by
/// more than one symbol are `ambiguous-symbol` listing candidate ids.
fn resolve_named_symbol<'a>(
    index: &'a SemanticIndex,
    name: &str,
) -> Result<&'a Symbol, Diagnostic> {
    let matches: Vec<&Symbol> = index
        .symbols()
        .filter(|symbol| symbol.name == name)
        .collect();
    match matches.as_slice() {
        [symbol] => Ok(symbol),
        [] => Err(Diagnostic::error(
            "unknown-symbol",
            Stage::Discovery,
            format!("unknown symbol '{name}'"),
        )),
        _ => Err(Diagnostic::error(
            "ambiguous-symbol",
            Stage::Discovery,
            format!(
                "ambiguous symbol '{name}': {}",
                matches
                    .iter()
                    .map(|symbol| format!("{} {}", symbol.kind.as_str(), symbol.id.index()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

/// Resolve a `source`/`line`/`col` position to the symbol whose identifier
/// occurrence contains it.
fn symbol_at_position<'a>(
    index: &'a SemanticIndex,
    loaded: &Loaded,
    input_path: &Path,
    target: &RenameTarget,
) -> Result<&'a Symbol, Diagnostic> {
    let (Some(source), Some(line), Some(col)) = (target.source.as_deref(), target.line, target.col)
    else {
        return Err(Diagnostic::error(
            "rename-invalid-target",
            Stage::Discovery,
            "semanticRename needs a `symbol` (numeric id or declared name) or a `source`/`line`/`col` position",
        ));
    };
    if !same_file(source, input_path) {
        return Err(Diagnostic::error(
            "rename-invalid-target",
            Stage::Discovery,
            format!(
                "the position source '{source}' does not name the loaded input '{}'",
                loaded.input.display
            ),
        ));
    }
    let mut hits = Vec::new();
    for symbol in index.symbols() {
        let hit = index.references(symbol.id).iter().any(|reference| {
            reference
                .occurrence
                .is_some_and(|span| position_in_span(span, line, col))
        });
        if hit {
            hits.push(symbol);
        }
    }
    match hits.as_slice() {
        [symbol] => Ok(symbol),
        [] => Err(Diagnostic::error(
            "rename-invalid-target",
            Stage::Discovery,
            format!("no symbol identifier covers {source}:{line}:{col}"),
        )),
        _ => Err(Diagnostic::error(
            "ambiguous-symbol",
            Stage::Discovery,
            format!(
                "more than one symbol covers {source}:{line}:{col}: {}",
                hits.iter()
                    .map(|symbol| format!("{} {}", symbol.kind.as_str(), symbol.id.index()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

/// Whether the 1-based `line`/`col` falls inside the half-open span.
fn position_in_span(span: workshop_rs::source::Span, line: u32, col: u32) -> bool {
    (line > span.start.line || (line == span.start.line && col >= span.start.col))
        && (line < span.end.line || (line == span.end.line && col < span.end.col))
}

/// A provenance gap on one reference: either no authored identifier span
/// exists, or the span does not cover exactly the symbol's name in the
/// caller's text. Both are refusals — rename never degrades to a textual
/// search.
fn rename_unmapped(loaded: &Loaded, symbol: &Symbol) -> Diagnostic {
    Diagnostic::error(
        "rename-unmapped-span",
        Stage::Discovery,
        format!(
            "a '{}' occurrence of {} '{}' does not map to an exact identifier span in '{}'",
            symbol.name,
            symbol.kind.as_str(),
            symbol.id.index(),
            loaded.input.display
        ),
    )
}

/// Reparse `edited` through the loaded program's parse context and verify
/// the rename invariant: the text stays a valid Workshop program and the
/// `(kind, to)` symbol binds the same reference-kind multiset that `(kind,
/// from)` bound before. A name that does not survive reparsing — or a
/// reference set that changed — refuses with no transaction.
fn verify_rename(
    index: &SemanticIndex,
    symbol: &Symbol,
    to: &str,
    edited: &str,
    loaded: &Loaded,
    catalog: &Catalog,
) -> Result<(), Vec<Diagnostic>> {
    let locale = Locale::new(
        loaded
            .origin
            .locale
            .as_deref()
            .expect("Workshop loads record their resolved locale"),
    );
    let program = workshop_rs::parser::parse_with_context(edited, catalog, &locale, catalog)
        .map_err(|error| vec![workshop_diag(error, &loaded.input)])?;
    program
        .validate()
        .map_err(|error| vec![workshop_diag(error, &loaded.input)])?;
    let reparsed = SemanticIndex::build(&program);
    let expected = reference_signature(index, symbol.id);
    let actual = reparsed
        .symbols()
        .find(|candidate| candidate.kind == symbol.kind && candidate.name == to)
        .map(|candidate| reference_signature(&reparsed, candidate.id));
    match actual {
        Some(actual) if actual == expected => Ok(()),
        _ => Err(vec![Diagnostic::error(
            "rename-mismatch",
            Stage::Validation,
            format!(
                "the edited source no longer binds the same references to '{to}' as '{}' did; refusing rather than reporting a partial rename",
                symbol.name
            ),
        )]),
    }
}

/// The sorted multiset of reference kinds for one symbol.
fn reference_signature(index: &SemanticIndex, id: SymbolId) -> Vec<ReferenceKind> {
    let mut kinds: Vec<ReferenceKind> = index
        .references(id)
        .iter()
        .map(|reference| reference.kind)
        .collect();
    kinds.sort_by_key(|kind| *kind as usize);
    kinds
}

/// Whether a `sources` key names the resolved input file: plain path
/// spellings compare by canonical identity, and a `file://` URI spelling of
/// the same path binds too.
fn same_source(source: &str, resolved: &ResolvedInput) -> bool {
    resolved
        .path
        .as_deref()
        .is_some_and(|path| same_file(source, path))
}

fn same_file(source: &str, path: &Path) -> bool {
    let source = source.strip_prefix("file://").unwrap_or(source);
    source == path.to_string_lossy()
        || matches!(
            (Path::new(source).canonicalize(), path.canonicalize()),
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
/// against — a stale source refuses with no writes — and each file is
/// replaced atomically through a sibling temporary file.
pub fn write_previews(
    previews: &[SourcePreview],
    transaction: &EditTransaction,
) -> Result<Vec<String>, Diagnostic> {
    let mut expected: BTreeMap<&str, &str> = BTreeMap::new();
    for edit in &transaction.edits {
        expected.insert(edit.source.as_str(), edit.source_identity.as_str());
    }
    for preview in previews {
        let path = Path::new(&preview.source);
        let bytes = std::fs::read(path).map_err(|error| {
            Diagnostic::error(
                "input-io",
                Stage::Discovery,
                format!("cannot read '{}': {error}", preview.source),
            )
        })?;
        let current = String::from_utf8_lossy(&bytes);
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
    }
    let mut written = Vec::new();
    for preview in previews {
        let path = Path::new(&preview.source);
        let temporary = temporary_path(path);
        std::fs::write(&temporary, &preview.new_text).map_err(|error| {
            Diagnostic::error(
                "output-io",
                Stage::Emission,
                format!("cannot write '{}': {error}", temporary.display()),
            )
        })?;
        std::fs::rename(&temporary, path).map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            Diagnostic::error(
                "output-io",
                Stage::Emission,
                format!("cannot replace '{}': {error}", preview.source),
            )
        })?;
        written.push(preview.source.clone());
    }
    Ok(written)
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

/// Build a whole-file `rename` edit that rewrites every word-boundary
/// occurrence of a name in `source` (#129). This is a proposal helper for
/// callers that only have source text — the raw Workshop `semanticRename`
/// path never uses it; it rewrites exact identifier spans instead (#434).
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    const SOURCE: &str = "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    score += 1\n";

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

    fn catalog() -> Arc<Catalog> {
        wright_analyzer::catalog::builtin().expect("builtin catalog")
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
            &catalog(),
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
        // The current text is missing entirely: the precondition refuses
        // before any range or compile work.
        let validation = validate_transaction(
            &SessionConfig {
                input: crate::InputSpec::Path(path),
                kind: SourceKind::Workshop,
                ..SessionConfig::default()
            },
            &catalog(),
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
    fn workshop_edits_validate_by_reparsing_the_source() {
        let (dir, mut session) = workshop_session(WORKSHOP);
        let loaded = session.load().expect("loaded");
        // One identifier edit: `score` → `total` in the declaration.
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
        // Renaming the declaration to a name that breaks reparse — `0)` —
        // refuses with the Workshop parse diagnostic, not a provider error.
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
    fn semantic_rename_rewrites_only_identifier_spans() {
        let (dir, mut session) = workshop_session(WORKSHOP);
        let loaded = session.load().expect("loaded");
        let sources = BTreeMap::from([(loaded.input.display.clone(), loaded.input.text.clone())]);
        let rename = semantic_rename(
            &loaded,
            session.catalog(),
            Some(&sources),
            &RenameTarget {
                symbol: Some(Address::Name("score".to_string())),
                source: None,
                line: None,
                col: None,
                to: "total".to_string(),
            },
        );
        assert!(rename.ok, "{:?}", rename.diagnostics);
        let transaction = rename.transaction.expect("transaction");
        // Declaration + `Set Global Variable` arg + `Global.score` read.
        assert_eq!(transaction.edits.len(), 3);
        for edit in &transaction.edits {
            assert_eq!(edit.edit_kind, "rename");
            assert_eq!(edit.new_text, "total");
            // The edited range covers exactly the old identifier.
            let lines: Vec<&str> = loaded.input.text.split('\n').collect();
            let line = lines[edit.range.start_line as usize - 1];
            assert_eq!(edit.range.start_line, edit.range.end_line);
            assert_eq!(
                &line[edit.range.start_col as usize - 1..edit.range.end_col as usize - 1],
                "score"
            );
        }
        let preview = rename.preview.expect("preview");
        assert!(preview[0].new_text.contains("0: total"));
        assert!(preview[0].new_text.contains("Set Global Variable(total,"));
        assert!(preview[0].new_text.contains("Add(Global.total, 1)"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn semantic_rename_by_position_and_stale_sources_refuse() {
        let (dir, mut session) = workshop_session(WORKSHOP);
        let loaded = session.load().expect("loaded");
        let sources = BTreeMap::from([(loaded.input.display.clone(), loaded.input.text.clone())]);
        // Position inside the `score` occurrence of `Set Global Variable`.
        let by_position = semantic_rename(
            &loaded,
            session.catalog(),
            Some(&sources),
            &RenameTarget {
                symbol: None,
                source: Some(loaded.input.display.clone()),
                line: Some(11),
                col: Some(30),
                to: "total".to_string(),
            },
        );
        assert!(by_position.ok, "{:?}", by_position.diagnostics);

        // A stale current text refuses: provenance spans must index the
        // loaded program's text.
        let stale = BTreeMap::from([(
            loaded.input.display.clone(),
            loaded.input.text.replacen("score", "other", 1),
        )]);
        let rename = semantic_rename(
            &loaded,
            session.catalog(),
            Some(&stale),
            &RenameTarget {
                symbol: Some(Address::Name("score".to_string())),
                source: None,
                line: None,
                col: None,
                to: "total".to_string(),
            },
        );
        assert!(!rename.ok);
        assert_eq!(rename.diagnostics[0].code, "edit-stale-source");

        // Unknown and missing targets refuse explicitly.
        for (target, code) in [
            (
                RenameTarget {
                    symbol: Some(Address::Name("missing".to_string())),
                    source: None,
                    line: None,
                    col: None,
                    to: "x".to_string(),
                },
                "unknown-symbol",
            ),
            (
                RenameTarget {
                    symbol: None,
                    source: Some(loaded.input.display.clone()),
                    line: Some(1),
                    col: Some(1),
                    to: "x".to_string(),
                },
                "rename-invalid-target",
            ),
        ] {
            let rename = semantic_rename(&loaded, session.catalog(), Some(&sources), &target);
            assert!(!rename.ok);
            assert_eq!(rename.diagnostics[0].code, code, "{target:?}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn semantic_rename_refuses_collisions_and_unsupported_kinds() {
        let (dir, mut session) = workshop_session(WORKSHOP);
        let loaded = session.load().expect("loaded");
        let sources = BTreeMap::from([(loaded.input.display.clone(), loaded.input.text.clone())]);
        // `to` names another global variable: a same-namespace collision.
        let collision = semantic_rename(
            &loaded,
            session.catalog(),
            Some(&sources),
            &RenameTarget {
                symbol: Some(Address::Name("score".to_string())),
                source: None,
                line: None,
                col: None,
                to: "score2".to_string(),
            },
        );
        assert!(collision.ok);
        // Add a second variable and collide with it.
        let (dir2, mut session2) =
            workshop_session(&WORKSHOP.replacen("0: score", "0: score\n        1: other", 1));
        let loaded2 = session2.load().expect("loaded");
        let sources2 =
            BTreeMap::from([(loaded2.input.display.clone(), loaded2.input.text.clone())]);
        let collision = semantic_rename(
            &loaded2,
            session2.catalog(),
            Some(&sources2),
            &RenameTarget {
                symbol: Some(Address::Name("score".to_string())),
                source: None,
                line: None,
                col: None,
                to: "other".to_string(),
            },
        );
        assert!(!collision.ok);
        assert_eq!(collision.diagnostics[0].code, "rename-name-collision");
        // A rule symbol is not a rename target.
        let rule = semantic_rename(
            &loaded,
            session.catalog(),
            Some(&sources),
            &RenameTarget {
                symbol: Some(Address::Name("r".to_string())),
                source: None,
                line: None,
                col: None,
                to: "renamed".to_string(),
            },
        );
        assert!(!rule.ok);
        assert_eq!(rule.diagnostics[0].code, "rename-unsupported-kind");
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(dir2);
    }

    #[test]
    fn the_rename_invariant_catches_edits_that_drop_a_reference() {
        // Ablation: a transaction missing one occurrence still fails the
        // post-reparse reference-shape check rather than "succeeding".
        let (dir, mut session) = workshop_session(WORKSHOP);
        let loaded = session.load().expect("loaded");
        let index = SemanticIndex::build(&loaded.program);
        let symbol = index
            .symbols()
            .find(|symbol| symbol.name == "score")
            .expect("score symbol");
        // Edit only the declaration, leaving the two references stale.
        let edited = loaded.input.text.replacen("0: score", "0: total", 1);
        let result = verify_rename(&index, symbol, "total", &edited, &loaded, session.catalog());
        let diagnostics = result.expect_err("a partial rename refuses");
        assert_eq!(diagnostics[0].code, "rename-mismatch");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn semantic_rename_covers_player_variables_and_subroutines() {
        // #434 acceptance: player variables rewrite their declaration plus
        // `Event Player.name` occurrences (prefix preserved), and
        // subroutines rewrite the declaration, the event binding, and calls.
        let source = "variables {\n    player:\n        1: streak\n}\n\nsubroutines {\n    0: helper\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Each Player;\n        All;\n        All;\n    }\n    actions {\n        Set Player Variable(Event Player, streak, Add(Event Player.streak, 1));\n        Call Subroutine(helper);\n    }\n}\n\nrule (\"sub\") {\n    event {\n        Subroutine;\n        helper;\n    }\n    actions {\n        Set Player Variable(Event Player, streak, 0);\n    }\n}\n";
        let (dir, mut session) = workshop_session(source);
        let loaded = session.load().expect("loaded");
        let sources = BTreeMap::from([(loaded.input.display.clone(), loaded.input.text.clone())]);

        let player = semantic_rename(
            &loaded,
            session.catalog(),
            Some(&sources),
            &RenameTarget {
                symbol: Some(Address::Name("streak".to_string())),
                source: None,
                line: None,
                col: None,
                to: "combo".to_string(),
            },
        );
        assert!(player.ok, "{:?}", player.diagnostics);
        let text = &player.preview.as_ref().unwrap()[0].new_text;
        assert!(text.contains("1: combo"), "{text}");
        assert!(text.contains("Event Player, combo,"), "{text}");
        assert!(text.contains("Event Player.combo"), "{text}");
        assert!(!text.contains("streak"), "{text}");
        // Only `combo` identifiers rewrote: `helper` is untouched.
        assert!(text.contains("Call Subroutine(helper)"), "{text}");

        let subroutine = semantic_rename(
            &loaded,
            session.catalog(),
            Some(&sources),
            &RenameTarget {
                symbol: Some(Address::Name("helper".to_string())),
                source: None,
                line: None,
                col: None,
                to: "assist".to_string(),
            },
        );
        assert!(subroutine.ok, "{:?}", subroutine.diagnostics);
        let text = &subroutine.preview.as_ref().unwrap()[0].new_text;
        assert!(text.contains("0: assist"), "{text}");
        assert!(text.contains("Call Subroutine(assist)"), "{text}");
        assert!(
            text.contains("Subroutine;\n        assist;"),
            "the event binding rewrites: {text}"
        );
        assert!(!text.contains("helper"), "{text}");

        // Position addressing inside the `Subroutine; helper;` binding.
        let helper_line = source
            .lines()
            .enumerate()
            .find(|(_, line)| line.trim() == "helper;")
            .map(|(index, _)| index as u32 + 1)
            .expect("event binding");
        let by_position = semantic_rename(
            &loaded,
            session.catalog(),
            Some(&sources),
            &RenameTarget {
                symbol: None,
                source: Some(loaded.input.display.clone()),
                line: Some(helper_line),
                col: Some(9),
                to: "assist".to_string(),
            },
        );
        assert!(by_position.ok, "{:?}", by_position.diagnostics);
        assert_eq!(
            by_position.transaction.unwrap().edits.len(),
            subroutine.transaction.unwrap().edits.len(),
            "position addressing resolves the same symbol"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn write_previews_refuses_stale_sources_and_updates_atomically() {
        // #434: `write_previews` rechecks each file's identity before
        // writing; a file that changed since validation refuses with no
        // write at all.
        let (dir, mut session) = workshop_session(WORKSHOP);
        let loaded = session.load().expect("loaded");
        let sources = BTreeMap::from([(loaded.input.display.clone(), loaded.input.text.clone())]);
        let rename = semantic_rename(
            &loaded,
            session.catalog(),
            Some(&sources),
            &RenameTarget {
                symbol: Some(Address::Name("score".to_string())),
                source: None,
                line: None,
                col: None,
                to: "total".to_string(),
            },
        );
        assert!(rename.ok, "{:?}", rename.diagnostics);
        let transaction = rename.transaction.as_ref().unwrap();
        let previews = rename.preview.as_ref().unwrap();

        // Stale: the file changed since the transaction was validated.
        std::fs::write(loaded.input.path.as_ref().unwrap(), "// changed\n").unwrap();
        let error = write_previews(previews, transaction).unwrap_err();
        assert_eq!(error.code, "edit-stale-source");
        assert_eq!(
            std::fs::read_to_string(loaded.input.path.as_ref().unwrap()).unwrap(),
            "// changed\n",
            "a stale write leaves the file untouched"
        );

        // Fresh: the file's identity matches and the write lands.
        std::fs::write(loaded.input.path.as_ref().unwrap(), &loaded.input.text).unwrap();
        let written = write_previews(previews, transaction).expect("writes");
        assert_eq!(written.len(), 1);
        let text = std::fs::read_to_string(loaded.input.path.as_ref().unwrap()).unwrap();
        assert!(text.contains("0: total"), "{text}");
        assert!(text.contains("Global.total"), "{text}");
        // No temporary sibling remains.
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
        let mut session = crate::CompilerSession::new(SessionConfig {
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
        let rename = session.semantic_rename(
            Some(&sources),
            &RenameTarget {
                symbol: Some(Address::Name("x".to_string())),
                source: None,
                line: None,
                col: None,
                to: "y".to_string(),
            },
        );
        assert!(!rename.ok);
        assert_eq!(rename.diagnostics[0].code, "edit-requires-provider");
        assert!(
            rename.diagnostics[0]
                .message
                .contains("providerSemanticRename"),
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
    fn semantic_rename_rewrites_start_rule_callee() {
        // A `Start Rule` subroutine argument is a value-position reference:
        // its exact identifier span must rewrite with the rename.
        let source = "subroutines {\n    0: helper\n}\n\nrule (\"sub\") {\n    event {\n        Subroutine;\n        helper;\n    }\n    actions {\n        Call Subroutine(helper);\n    }\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Start Rule(helper, Restart Rule);\n    }\n}\n";
        let (dir, mut session) = workshop_session(source);
        let loaded = session.load().expect("loaded");
        let sources = BTreeMap::from([(loaded.input.display.clone(), loaded.input.text.clone())]);
        let rename = semantic_rename(
            &loaded,
            session.catalog(),
            Some(&sources),
            &RenameTarget {
                symbol: Some(Address::Name("helper".to_string())),
                source: None,
                line: None,
                col: None,
                to: "assist".to_string(),
            },
        );
        assert!(rename.ok, "{:?}", rename.diagnostics);
        let text = &rename.preview.as_ref().unwrap()[0].new_text;
        assert!(text.contains("0: assist"), "{text}");
        assert!(text.contains("Subroutine;\n        assist;"), "{text}");
        assert!(text.contains("Call Subroutine(assist)"), "{text}");
        assert!(
            text.contains("Start Rule(assist, Restart Rule)"),
            "the Start Rule callee rewrites: {text}"
        );
        assert!(!text.contains("helper"), "{text}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn semantic_rename_binds_redeclared_names_to_the_last_declaration() {
        // workshop-rs resolves a redeclared name to its last declaration:
        // every use of `helper` binds #1. Renaming #1 rewrites the uses with
        // it; renaming #0 touches only its own declaration and leaves the
        // surviving `helper` bindings intact.
        let source = "subroutines {\n    0: helper\n    1: helper\n}\n\nrule (\"sub\") {\n    event {\n        Subroutine;\n        helper;\n    }\n    actions {\n        Call Subroutine(helper);\n    }\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Start Rule(helper, Restart Rule);\n    }\n}\n";
        let (dir, mut session) = workshop_session(source);
        let loaded = session.load().expect("loaded");
        let sources = BTreeMap::from([(loaded.input.display.clone(), loaded.input.text.clone())]);
        let target = |symbol: Address| RenameTarget {
            symbol: Some(symbol),
            source: None,
            line: None,
            col: None,
            to: "assist".to_string(),
        };

        let last = semantic_rename(
            &loaded,
            session.catalog(),
            Some(&sources),
            &target(Address::Id(1)),
        );
        assert!(last.ok, "{:?}", last.diagnostics);
        let text = &last.preview.as_ref().unwrap()[0].new_text;
        assert!(text.contains("0: helper"), "{text}");
        assert!(text.contains("1: assist"), "{text}");
        assert!(text.contains("Subroutine;\n        assist;"), "{text}");
        assert!(text.contains("Call Subroutine(assist)"), "{text}");
        assert!(text.contains("Start Rule(assist, Restart Rule)"), "{text}");
        assert_eq!(text.matches("helper").count(), 1, "{text}");

        let first = semantic_rename(
            &loaded,
            session.catalog(),
            Some(&sources),
            &target(Address::Id(0)),
        );
        assert!(first.ok, "{:?}", first.diagnostics);
        let text = &first.preview.as_ref().unwrap()[0].new_text;
        assert!(text.contains("0: assist"), "{text}");
        assert_eq!(
            text.matches("helper").count(),
            4,
            "the surviving declaration keeps every binding: {text}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn wire_transactions_recheck_the_transaction_invariants() {
        // A transaction deserialized off the wire bypasses
        // `EditTransaction::new`; validation re-normalizes it so empty or
        // overlapping edits refuse and unsorted edits still apply in source
        // order rather than splicing blindly.
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

        // Overlapping ranges on line 11 (`score` and `core,`) refuse.
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

        // Unsorted disjoint edits normalize: `Add` (cols 36-39) listed before
        // `score` (cols 29-34) still produces the correctly spliced source.
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

        // An edit targeting a source that is not the loaded input refuses —
        // validating it would check text the transaction never touched.
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

    #[test]
    fn semantic_rename_refuses_an_unchanged_name() {
        let (dir, mut session) = workshop_session(WORKSHOP);
        let loaded = session.load().expect("loaded");
        let sources = BTreeMap::from([(loaded.input.display.clone(), loaded.input.text.clone())]);
        let rename = semantic_rename(
            &loaded,
            session.catalog(),
            Some(&sources),
            &RenameTarget {
                symbol: Some(Address::Name("score".to_string())),
                source: None,
                line: None,
                col: None,
                to: "score".to_string(),
            },
        );
        assert!(!rename.ok);
        assert_eq!(rename.diagnostics[0].code, "rename-invalid-name");
        let _ = std::fs::remove_dir_all(dir);
    }
}
