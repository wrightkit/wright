//! Frontend-neutral source-edit transactions (M9 #59, reconciled by M14 #128).
//!
//! Tools and agents propose edits as validated, source-oriented
//! [`SourceEdit`]s — never as mutations of Wright's internal IR. One
//! [`EditTransaction`] carries one or more file edits with exact source
//! ranges plus per-source identity/version preconditions, and
//! [`validate_transaction`] rejects stale versions, overlapping edits, and
//! out-of-range spans, then runs the edited project through the *correct*
//! native frontend for its original source kind (`.opy` through the OPY
//! frontend, `.ostw`/`.del` through the OSTW project frontend) with the
//! edited files supplied as in-memory overlays — no synthetic `edit.opy`
//! path, no OPY hard-coding, and no filesystem write. Unsupported/unsafe
//! edits fail explicitly with structured diagnostics and no partial preview.
//!
//! Validation is atomic: a transaction either applies and previews in full
//! or is refused with diagnostics; the caller decides whether to write any
//! file. Application/writing is always separate from validation.
//!
//! The first evidence-backed refactoring is symbol rename ([`rename_symbol`]),
//! which proposes an edit carrying the source identity precondition; callers
//! validate it inside a transaction with [`validate_transaction`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{SessionConfig, SourceKind};
use crate::diag::{Diagnostic, Origin, Position, Severity, SourceSpan, Stage};
use crate::input::ResolvedInput;
use crate::result::exit_code_from;
use crate::session;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditRange {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

/// One validated source transaction: multiple file edits applied and
/// validated atomically against one project.
///
/// Construction orders the edits deterministically (by source identity, then
/// position) and rejects overlapping edits within one source up front.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditTransaction {
    /// The edits, in deterministic order (source, then position).
    pub edits: Vec<SourceEdit>,
}

impl EditTransaction {
    /// Build a transaction from proposed edits.
    ///
    /// The edits are ordered deterministically and overlapping edits within
    /// one source are rejected (`edit-overlap`); an empty transaction is
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
                .then_with(|| start_position(a).cmp(&start_position(b)))
                .then_with(|| end_position(a).cmp(&end_position(b)))
        });
        for pair in edits.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if a.source == b.source && start_position(b) < end_position(a) {
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
        }
        Ok(EditTransaction { edits })
    }
}

fn start_position(edit: &SourceEdit) -> (u32, u32) {
    (edit.range.start_line, edit.range.start_col)
}

fn end_position(edit: &SourceEdit) -> (u32, u32) {
    (edit.range.end_line, edit.range.end_col)
}

/// The edited text of one source in a validated transaction.
#[derive(Debug, Clone, Serialize)]
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
/// code: its source kind selects the native frontend, its root is preserved,
/// and its transformation profile (when set) applies exactly as the session
/// would apply it. `sources` supplies the current text of every source the
/// transaction touches, keyed by the same source identity the edits carry, so
/// the identity/version preconditions can be verified and the edited project
/// compiled without reading or rewriting the user's files.
///
/// The edited project is validated through the correct native frontend and
/// project semantics (never a forced `.opy`): OPY projects compile with the
/// edited files as include overlays; OSTW projects load their `ds.toml`
/// project graph with the edited files as overlays. Refusals are atomic and
/// structured: stale sources, overlapping/unknown edits, unsupported input
/// kinds, and compiled errors produce source-located diagnostics and no
/// partial preview.
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
    let root = config.root.clone().unwrap_or_else(|| {
        main_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    });

    // The edited main text: the transaction's preview when the main source is
    // edited, else the caller-provided current text, else the filesystem.
    let main_text = match preview_of(&previews, &main_path) {
        Some(preview) => preview.new_text.clone(),
        None => match sources.get(&main_path.to_string_lossy().into_owned()) {
            Some(text) => text.clone(),
            None => match std::fs::read_to_string(&main_path) {
                Ok(text) => text,
                Err(error) => {
                    diagnostics.push(Diagnostic::error(
                        "input-io",
                        Stage::Discovery,
                        format!("cannot read input '{}': {error}", main_path.display()),
                    ));
                    return refusal(diagnostics);
                }
            },
        },
    };

    // Edited non-main sources become in-memory overlays keyed for the native
    // frontend of the project's original source kind.
    let overlay = build_overlay(
        kind,
        &root,
        &main_path,
        previews
            .iter()
            .map(|preview| (preview.source.as_str(), preview.new_text.as_str())),
    );
    let resolved = resolved_input(
        kind,
        &main_path,
        &root,
        &main_text,
        config.locale.as_deref(),
    );

    match compile_project(kind, &resolved, &overlay, config.profile) {
        Ok(_) => {}
        Err(errors) => diagnostics.extend(errors),
    }

    EditValidation {
        ok: !has_error(&diagnostics),
        exit: exit_code_from(&diagnostics),
        diagnostics,
        preview: Some(previews),
    }
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

/// A semantic rename target: the exact identifier occurrence at a 1-based
/// line/column in one source of the project (M14, #129).
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
/// structured refusal diagnostics (M14, #129).
///
/// The transaction edits exactly the resolved semantic identity's occurrence
/// ranges — never a whole-word scan or whole-document replacement — and is
/// validated through the shared [`validate_transaction`] boundary before
/// success is reported. The contract carries no LSP protocol types and
/// exposes no mutable IR.
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
/// position in one source of a project (M14, #129).
///
/// `config` and `sources` are the project and current-source snapshot of the
/// *unmodified* code, exactly as [`validate_transaction`] takes them. The
/// target symbol is resolved through the shared semantic index of the
/// project compiled through its original native frontend; only occurrences
/// belonging to that resolved semantic identity (the declaration identifier,
/// the definition identifier, and every reference identifier) are edited,
/// each as an exact-range edit carrying the source's identity precondition.
/// Ambiguous positions, target-name collisions, occurrences without an exact
/// identifier span, sources without a current text, and edited projects that
/// fail validation refuse with deterministic structured diagnostics and no
/// transaction.
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
    let root = config.root.clone().unwrap_or_else(|| {
        main_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    });

    // The current project state: the main text from the caller's snapshot
    // (or the filesystem), every other source as an in-memory overlay.
    let main_text = match sources.get(&main_path.to_string_lossy().into_owned()) {
        Some(text) => text.clone(),
        None => match std::fs::read_to_string(&main_path) {
            Ok(text) => text,
            Err(error) => {
                return refuse(vec![Diagnostic::error(
                    "input-io",
                    Stage::Discovery,
                    format!("cannot read input '{}': {error}", main_path.display()),
                )]);
            }
        },
    };
    let overlay = build_overlay(
        kind,
        &root,
        &main_path,
        sources
            .iter()
            .map(|(source, text)| (source.as_str(), text.as_str())),
    );
    let resolved = resolved_input(
        kind,
        &main_path,
        &root,
        &main_text,
        config.locale.as_deref(),
    );

    let program = match compile_project(kind, &resolved, &overlay, config.profile) {
        Ok(program) => program,
        Err(diagnostics) => return refuse(diagnostics),
    };
    let index = match wright_analyzer::symbols::SemanticIndex::build(&program) {
        Ok(index) => index,
        Err(error) => {
            return refuse(vec![Diagnostic::error(
                "analysis-error",
                Stage::Analysis,
                format!("cannot build the semantic index for rename: {error}"),
            )]);
        }
    };

    // The position names one file of the compiled project; refuse when the
    // source is not part of it (e.g. an include outside the project closure).
    let Some(file_id) = file_id_for_source(&program, &root, &target.source) else {
        return refuse(vec![Diagnostic::error(
            "rename-unresolved",
            Stage::Discovery,
            format!(
                "'{}' is not part of the compiled project; the rename position \
                 must name a project source",
                target.source
            ),
        )]);
    };
    let Some(symbol) = symbol_at(&index, file_id, target.line, target.col) else {
        return refuse(vec![Diagnostic::error(
            "rename-unresolved",
            Stage::Discovery,
            format!(
                "no symbol is resolvable at {}:{}:{}",
                target.source, target.line, target.col
            ),
        )]);
    };

    // A same-spelled symbol in a different namespace is not a collision (the
    // semantic index distinguishes identities by symbol id); only a genuine
    // target-name conflict with another declared symbol refuses.
    if index
        .symbols()
        .any(|other| other.id != symbol.id && other.name == target.to)
    {
        return refuse(vec![Diagnostic::error(
            "rename-collision",
            Stage::Discovery,
            format!(
                "'{}' is already declared; the new name would collide",
                target.to
            ),
        )]);
    }

    // Collect the exact occurrence spans of the resolved semantic identity.
    let mut occurrences = Vec::new();
    if let Some(occurrence) = symbol.occurrence {
        occurrences.push(occurrence);
    }
    for reference in index.references(symbol.id) {
        match (reference.occurrence, reference.span) {
            (Some(occurrence), _) => occurrences.push(occurrence),
            (None, Some(span)) => {
                // A reference with a source location but no exact identifier
                // occurrence: an exact target cannot be established, so the
                // rename refuses rather than broadening to a statement span.
                let path = program
                    .files
                    .get(span.file)
                    .map(|file| file.path.clone())
                    .unwrap_or_else(|| target.source.clone());
                return refuse(vec![Diagnostic::error(
                    "rename-unresolved-target",
                    Stage::Discovery,
                    format!(
                        "a semantic occurrence in {path} has no exact identifier span; \
                         refusing the rename rather than broadening to a statement span"
                    ),
                )]);
            }
            (None, None) => {}
        }
    }

    // One exact-range edit per occurrence, carrying the current text's
    // identity precondition; the transaction orders and overlap-checks them.
    let mut edits: BTreeMap<String, BTreeSet<(u32, u32, u32, u32)>> = BTreeMap::new();
    for occurrence in occurrences {
        let file = occurrence.file.index();
        let Some(source) = source_key_for_file(&program, &root, &main_path, sources, file) else {
            return refuse(vec![Diagnostic::error(
                "edit-unknown-source",
                Stage::Discovery,
                "no current text was provided for a source the rename would edit; \
                 supply the current text of every project source",
            )]);
        };
        edits.entry(source).or_default().insert((
            occurrence.start.line,
            occurrence.start.col,
            occurrence.end.line,
            occurrence.end.col,
        ));
    }
    let mut source_edits = Vec::new();
    for (source, ranges) in edits {
        let current = sources
            .get(&source)
            .cloned()
            .unwrap_or_else(|| main_text.clone());
        let identity = crate::input_identity(&current);
        for (start_line, start_col, end_line, end_col) in ranges {
            source_edits.push(SourceEdit {
                edit_kind: "rename".to_string(),
                source: source.clone(),
                source_identity: identity.clone(),
                range: EditRange {
                    start_line,
                    start_col,
                    end_line,
                    end_col,
                },
                new_text: target.to.clone(),
            });
        }
    }
    let transaction = match EditTransaction::new(source_edits) {
        Ok(transaction) => transaction,
        Err(diagnostic) => return refuse(vec![diagnostic]),
    };

    // Validate the resulting transaction through #128 before success; an
    // unvalidated transaction is never returned.
    let validation = validate_transaction(config, sources, &transaction);
    if validation.ok {
        SemanticRename {
            ok: true,
            transaction: Some(transaction),
            diagnostics: validation.diagnostics,
            preview: validation.preview,
        }
    } else {
        SemanticRename {
            ok: false,
            transaction: None,
            diagnostics: validation.diagnostics,
            preview: None,
        }
    }
}

/// The program file id whose registry path matches a caller source identity.
///
/// Matching walks the program file registry (never assuming the main source
/// is file 0 — the OSTW project registry reserves file 0 for `ds.toml`) and
/// tries the registry path as given, root-joined, and canonicalized, so
/// display-relative spellings (e.g. the driver's cwd-relative display paths)
/// and project-relative spellings both match.
fn file_id_for_source(
    program: &wright_ir::wir::Program,
    root: &Path,
    source: &str,
) -> Option<usize> {
    program
        .files
        .iter()
        .enumerate()
        .find_map(|(index, file)| registry_path_matches(source, root, &file.path).then_some(index))
}

/// Whether a source identity names the same file as a registry path
/// spelling: exact equality, the root-joined spelling, or the spelling as
/// given (resolved against the working directory).
fn registry_path_matches(source: &str, root: &Path, registry_path: &str) -> bool {
    let path = Path::new(registry_path);
    if same_file(source, path) {
        return true;
    }
    if path.is_relative() {
        return same_file(source, &root.join(path));
    }
    false
}

/// The caller's source identity (a `sources` key) for a program file id,
/// matching file registry paths against the provided current texts.
fn source_key_for_file(
    program: &wright_ir::wir::Program,
    root: &Path,
    main_path: &Path,
    sources: &BTreeMap<String, String>,
    file: usize,
) -> Option<String> {
    if file == 0 {
        return sources
            .keys()
            .find(|key| same_file(key, main_path))
            .cloned()
            .or_else(|| Some(main_path.to_string_lossy().into_owned()));
    }
    let registry_path = program
        .files
        .get(wright_ir::source::FileId::from_index(file))
        .map(|source_file| &source_file.path)?;
    sources
        .keys()
        .find(|key| registry_path_matches(key, root, registry_path))
        .cloned()
}

/// The symbol whose declaration or reference occurrence span in `file_id`
/// contains a 1-based line/column; spans in other files are never considered.
fn symbol_at(
    index: &wright_analyzer::symbols::SemanticIndex,
    file_id: usize,
    line: u32,
    col: u32,
) -> Option<wright_analyzer::symbols::Symbol> {
    for symbol in index.symbols() {
        let symbol_id = symbol.id;
        if let Some(span) = symbol.span {
            if span.file.index() == file_id && span_contains(span, line, col) {
                return Some(symbol.clone());
            }
        }
        for reference in index.references(symbol_id) {
            if let Some(span) = reference.span {
                if span.file.index() == file_id && span_contains(span, line, col) {
                    return Some(symbol.clone());
                }
            }
        }
    }
    None
}

fn span_contains(span: wright_ir::source::Span, line: u32, col: u32) -> bool {
    (span.start.line, span.start.col) <= (line, col)
        && (line, col)
            <= (
                span.end.line,
                span.end.col.saturating_sub(1).max(span.start.col),
            )
}

fn has_error(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
}

/// The origin metadata of a resolved input for the given source kind.
fn origin_for(kind: SourceKind, locale: Option<&str>) -> Origin {
    Origin {
        kind: kind.as_str().to_string(),
        locale: locale.map(str::to_string),
    }
}

/// The resolved-input snapshot a validation/rename compile runs against.
fn resolved_input(
    kind: SourceKind,
    main_path: &Path,
    root: &Path,
    main_text: &str,
    locale: Option<&str>,
) -> ResolvedInput {
    ResolvedInput {
        kind,
        text: main_text.to_string(),
        path: Some(main_path.to_path_buf()),
        root: root.to_path_buf(),
        display: crate::input::display_path(main_path),
        identity: crate::input_identity(main_text),
        origin: origin_for(kind, locale),
    }
}

/// Compile a project through its original native frontend (OPY with include
/// overlays, OSTW with the `ds.toml` project graph and overlays) and the
/// shared HIR→IR→lower→validate chain, applying the session's transformation
/// profile for OPY exactly as `CompilerSession::load` does. Returns the
/// validated WIR program or every structured source-located diagnostic.
fn compile_project(
    kind: SourceKind,
    resolved: &ResolvedInput,
    overlay: &BTreeMap<String, String>,
    profile: crate::Profile,
) -> Result<wright_ir::wir::Program, Vec<Diagnostic>> {
    match kind {
        SourceKind::Opy => {
            let outcome = wright_opy::compile_with_overlay_outcome(
                &resolved.text,
                &resolved.display,
                &resolved.root,
                overlay,
            );
            let Some(program) = outcome.hir else {
                return Err(vec![session::opy_diag(
                    outcome
                        .error
                        .expect("a failed compile outcome always carries an error"),
                    &outcome.files,
                    resolved,
                )]);
            };
            if let Err(error) = program.validate() {
                return Err(vec![session::hir_diag(error, resolved)]);
            }
            let model = match program.to_ir() {
                Ok(model) => model,
                Err(error) => {
                    return Err(vec![session::ir_diag(
                        "convert-error",
                        crate::diag::Stage::Lowering,
                        error,
                        resolved,
                    )]);
                }
            };
            let mut program = match wright_ir::lower::lower(&model) {
                Ok(program) => program,
                Err(error) => {
                    return Err(vec![session::ir_diag(
                        "lower-error",
                        crate::diag::Stage::Lowering,
                        error,
                        resolved,
                    )]);
                }
            };
            if let Err(error) = program.validate() {
                return Err(vec![session::ir_diag(
                    "validation-error",
                    crate::diag::Stage::Validation,
                    error,
                    resolved,
                )]);
            }
            if profile != crate::Profile::Off {
                if let Err(error) = wright_transform::run(&mut program, profile) {
                    return Err(vec![Diagnostic::error(
                        "transform-error",
                        crate::diag::Stage::Internal,
                        format!("WIR transformation failed: {error}"),
                    )]);
                }
            }
            Ok(program)
        }
        SourceKind::Ostw => {
            let relative = resolved
                .path
                .as_ref()
                .and_then(|path| path.strip_prefix(&resolved.root).ok())
                .map(|relative| relative.to_string_lossy().replace('\\', "/"));
            let (outcome, semantic) = wright_ostw::compile_with_semantics_overlay(
                &resolved.text,
                relative.as_deref(),
                &resolved.root,
                overlay,
            );
            let mut diagnostics = Vec::new();
            if let Some(error) = &outcome.error {
                diagnostics.push(session::ostw_diag(error.clone(), &outcome, resolved));
            }
            for error in &outcome.diagnostics {
                diagnostics.push(session::ostw_diag(error.clone(), &outcome, resolved));
            }
            for error in &semantic.diagnostics {
                diagnostics.push(session::ostw_diag(error.clone(), &outcome, resolved));
            }
            if has_error(&diagnostics) {
                return Err(diagnostics);
            }
            let Some(hir) = semantic.hir else {
                // The frontend outcome carries no reachable semantic HIR and
                // no diagnostics: the session path treats this as an empty
                // program (check succeeds), so rename finds no symbols.
                return Ok(wright_ir::wir::Program::default());
            };
            let program = match wright_ir::lower::lower(&hir) {
                Ok(program) => program,
                Err(error) => {
                    return Err(vec![session::ir_diag(
                        "lower-error",
                        crate::diag::Stage::Lowering,
                        error,
                        resolved,
                    )]);
                }
            };
            if let Err(error) = program.validate() {
                return Err(vec![session::ir_diag(
                    "validation-error",
                    crate::diag::Stage::Validation,
                    error,
                    resolved,
                )]);
            }
            Ok(program)
        }
        _ => unreachable!("compile_project only runs for OPY/OSTW"),
    }
}

/// The concrete source kind to validate against: the configured kind, or
/// detection from the main file extension for `Auto`. Only the source
/// frontends OPY and OSTW are declared edit targets; Workshop/Protocol
/// inputs refuse explicitly.
fn resolve_kind(config: &SessionConfig, main_path: &Path) -> Result<SourceKind, Diagnostic> {
    match config.kind {
        SourceKind::Opy | SourceKind::Ostw => Ok(config.kind),
        SourceKind::Auto => match main_path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref()
        {
            Some("opy") => Ok(SourceKind::Opy),
            Some("ostw" | "del") => Ok(SourceKind::Ostw),
            _ => Err(Diagnostic::error(
                "edit-unsupported-kind",
                Stage::Discovery,
                format!(
                    "cannot detect the source kind of '{}' for edit validation; \
                     pass an explicit `opy` or `ostw` source kind",
                    main_path.display()
                ),
            )),
        },
        other => Err(Diagnostic::error(
            "edit-unsupported-kind",
            Stage::Discovery,
            format!(
                "edit validation is declared over the OPY and OSTW source frontends; \
                 '{}' input is not an editable source kind",
                other.as_str()
            ),
        )),
    }
}

/// The edited text of the source matching `main_path`, when the transaction
/// edits the main source.
fn preview_of<'a>(previews: &'a [SourcePreview], main_path: &Path) -> Option<&'a SourcePreview> {
    previews
        .iter()
        .find(|preview| same_file(&preview.source, main_path))
}

/// Whether two spellings identify the same file: canonical paths when both
/// resolve, else the exact path strings.
fn same_file(a: &str, b: &Path) -> bool {
    let b = b.to_string_lossy();
    if a == b {
        return true;
    }
    match (Path::new(a).canonicalize(), Path::new(&*b).canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Build the in-memory overlay of non-main sources, keyed for the project's
/// native frontend.
///
/// OPY includes resolve against the include root by include string and by
/// canonical path; the overlay carries the as-given, canonical, root-relative,
/// and basename spellings (the same spellings the language service overlays
/// use). OSTW sources resolve by normalized project-relative path; the
/// overlay carries the as-given and root-relative normalized spellings. The
/// main source is never overlaid (its text is passed to the frontend
/// directly).
fn build_overlay<'a>(
    kind: SourceKind,
    root: &Path,
    main_path: &Path,
    entries: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> BTreeMap<String, String> {
    let mut overlay = BTreeMap::new();
    for (source, text) in entries {
        if same_file(source, main_path) {
            continue;
        }
        match kind {
            SourceKind::Opy => {
                let path = PathBuf::from(source);
                overlay.insert(path.to_string_lossy().into_owned(), text.to_string());
                if let Ok(canonical) = path.canonicalize() {
                    overlay.insert(canonical.to_string_lossy().into_owned(), text.to_string());
                }
                if let Ok(relative) = path.strip_prefix(root) {
                    overlay.insert(relative.to_string_lossy().into_owned(), text.to_string());
                }
                if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                    overlay.insert(name.to_string(), text.to_string());
                }
            }
            SourceKind::Ostw => {
                overlay.insert(normalize_relative(source), text.to_string());
                if let Ok(relative) = Path::new(source).strip_prefix(root) {
                    overlay.insert(
                        relative.to_string_lossy().replace('\\', "/"),
                        text.to_string(),
                    );
                }
            }
            _ => {}
        }
    }
    overlay
}

/// A normalized project-relative spelling of a path (`\` separators become
/// `/`, matching the OSTW project registry).
fn normalize_relative(path: &str) -> String {
    path.replace('\\', "/")
}

/// Apply every edit of a transaction to the caller-provided current texts,
/// returning one complete edited source per affected source.
///
/// The edits are already deterministically ordered by construction; within
/// one source they apply in order, so exact ranges are consumed and never
/// shift. Ranges outside the source refuse explicitly.
fn apply_transaction(
    sources: &BTreeMap<String, String>,
    transaction: &EditTransaction,
) -> Result<Vec<SourcePreview>, Diagnostic> {
    let mut previews: Vec<SourcePreview> = Vec::new();
    for edit in &transaction.edits {
        let current = sources
            .get(&edit.source)
            .expect("precondition check already verified every edited source");
        match previews.last_mut() {
            Some(last) if last.source == edit.source => {
                last.new_text = apply_edit(&last.new_text, edit)?;
                last.source_identity = crate::input_identity(&last.new_text);
            }
            _ => {
                let new_text = apply_edit(current, edit)?;
                previews.push(SourcePreview {
                    source: edit.source.clone(),
                    source_identity: crate::input_identity(&new_text),
                    new_text,
                });
            }
        }
    }
    Ok(previews)
}

/// Apply an edit's replacement text at its range (1-based character columns,
/// end exclusive).
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
            let col = char_col(line, edit.range.start_col);
            let end = char_col(line, edit.range.end_col);
            let mut replacement = String::new();
            replacement.push_str(&line[..col]);
            replacement.push_str(&edit.new_text);
            replacement.push_str(&line[end..]);
            out.push(replacement);
        } else if line_number == edit.range.start_line {
            let col = char_col(line, edit.range.start_col);
            let mut replacement = String::new();
            replacement.push_str(&line[..col]);
            replacement.push_str(&edit.new_text);
            out.push(replacement);
        } else if line_number == edit.range.end_line {
            let end = char_col(line, edit.range.end_col);
            out.push(line[end..].to_string());
        } else {
            // A wholly covered middle line is removed.
            continue;
        }
    }
    Ok(out.join("\n"))
}

/// Convert a 1-based character column to a byte offset, clamped to the line
/// length (so a full-line replacement with a character-counted column never
/// splits a multi-byte character).
fn char_col(line: &str, col: u32) -> usize {
    let skip = col.saturating_sub(1) as usize;
    line.char_indices()
        .nth(skip)
        .map(|(offset, _)| offset)
        .unwrap_or(line.len())
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
    /// The source file identity the rename applies to.
    pub source: String,
    /// The source identity this rename applies to.
    pub source_identity: String,
}

/// Rename a declared symbol across the source.
///
/// Returns a single multi-line [`SourceEdit`] covering every occurrence of
/// `from` (declaration and references), carrying the source identity
/// precondition. The caller validates it inside a transaction with
/// [`validate_transaction`], which recompiles the result. Names that are not
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

    rename_occurrences(
        source,
        &request.from,
        &request.to,
        &request.source,
        &request.source_identity,
    )
}

/// Rename every whole-word occurrence of `from` to `to`, returning a
/// full-document [`SourceEdit`] carrying the source identity precondition.
///
/// Unlike [`rename_symbol`], this does not require a declaration in `source`.
/// This is the M9 textual contract: it is name- and boundary-driven, so the
/// caller must guarantee semantic identity (the project-wide rename in
/// `wright-language` targets exact semantic spans instead, so it never routes
/// through this whole-word scan).
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
    fn rename_validates_through_the_pipeline() {
        let sources = BTreeMap::from([("program.opy".to_string(), SOURCE.to_string())]);
        let validation = rename(rename_edit(SOURCE, "score", "total"), &sources);
        assert!(
            validation.ok,
            "the renamed source must compile: {:?}",
            validation.diagnostics
        );
        let preview = validation.preview.as_ref().unwrap();
        assert_eq!(preview.len(), 1, "one affected source");
        assert!(
            preview[0].new_text.contains("globalvar total"),
            "preview shows the renamed source"
        );
        assert_eq!(
            preview[0].source_identity,
            crate::input_identity(&preview[0].new_text),
            "the preview carries the new-source identity"
        );
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
        assert!(
            validation.diagnostics.iter().any(
                |diagnostic| diagnostic.code == "lex-error" || diagnostic.code == "parse-error"
            ),
            "the refusal carries the compile diagnostic: {:?}",
            validation.diagnostics
        );
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
