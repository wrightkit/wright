//! The editor-neutral language service (#63, #65, #66).
//!
//! Composes the native `.opy` frontend, the semantic index/analyzer, the M9
//! safe-edit contract, and the workshop catalog. Every result is tagged with
//! the document version it was computed for, so stale results are
//! deterministic and replaceable (#64). No LSP types appear here.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::Serialize;
use wright_analyzer::analysis::{self, Finding};
use wright_analyzer::symbols::{SemanticIndex, Symbol};
use wright_ir::wir;
use wright_opy::preprocess::FileRecord;

use crate::document::{Document, DocumentStore, Position, Range};

/// A source-aware language diagnostic (editor-neutral).
///
/// Unlike a range-only diagnostic, this carries the source/document identity
/// the diagnostic belongs to, the version of that source when it is an open
/// document, and the version of the requesting (root) document. A diagnostic
/// from an included file is therefore never mapped through the requesting
/// document's text.
#[derive(Debug, Clone, Serialize)]
pub struct SourceDiagnostic {
    /// The source identity: the requesting document URI for the main file, or
    /// the resolved path for an included file.
    pub source: String,
    pub range: Range,
    pub severity: String,
    pub code: String,
    pub message: String,
    /// The version of the document owning `source`, when it is open (else 0).
    pub source_version: i32,
    /// The version of the document that requested the analysis.
    pub document_version: i32,
}

/// Hover content.
#[derive(Debug, Clone, Serialize)]
pub struct Hover {
    pub contents: String,
    pub range: Option<Range>,
    pub document_version: i32,
}

/// A completion item.
#[derive(Debug, Clone, Serialize)]
pub struct CompletionItem {
    pub label: String,
    pub kind: String,
    pub detail: Option<String>,
}

/// A semantic token.
#[derive(Debug, Clone, Serialize)]
pub struct SemanticToken {
    pub line: u32,
    pub character: u32,
    pub length: u32,
    pub token_type: String,
}

/// A source-aware location: a source/document identity plus a range.
#[derive(Debug, Clone, Serialize)]
pub struct SourceLocation {
    /// The source identity: the document URI for the main file, or the
    /// resolved path for an included file.
    pub source: String,
    pub range: Range,
}

/// One source-aware rename edit targeting a full document.
///
/// Carries the source identity, a 0-based full-document replacement range,
/// the replacement text, and version/identity preconditions so a client can
/// apply the edit safely and detect stale state.
#[derive(Debug, Clone, Serialize)]
pub struct RenameEdit {
    /// The source identity: the requesting document URI or a resolved include
    /// path.
    pub source: String,
    /// The full-document replacement range in the source.
    pub range: Range,
    /// The renamed full-document text.
    pub new_text: String,
    /// The SHA-256 identity of the pre-edit source text (stale-state
    /// precondition).
    pub source_identity: String,
    /// The version of the source document, when it is open (else 0).
    pub source_version: i32,
}

/// The result of a project-wide rename request.
#[derive(Debug, Clone, Serialize)]
pub struct RenameResult {
    /// The current requesting document version.
    pub document_version: i32,
    /// Whether the rename validates through the compiler pipeline.
    pub ok: bool,
    /// The source-aware edits for every semantically affected source.
    pub edits: Vec<RenameEdit>,
    /// Structured refusal diagnostics when the rename was rejected.
    pub diagnostics: Vec<String>,
}

/// The analyzed state of one document.
pub struct Analysis {
    pub program: wir::Program,
    pub index: Option<SemanticIndex>,
    pub findings: Vec<Finding>,
    pub parse_errors: Vec<wright_opy::FrontendError>,
    /// The frontend file registry, retained even when parsing/lowering fails
    /// so diagnostic spans can be mapped to their actual source.
    pub files: Vec<FileRecord>,
}

/// The editor-neutral language service over a workspace.
pub struct LanguageService {
    pub store: DocumentStore,
    /// The include root used for project compilation.
    pub root: std::path::PathBuf,
}

impl LanguageService {
    /// A service with the given project root.
    pub fn new(root: std::path::PathBuf) -> LanguageService {
        LanguageService {
            store: DocumentStore::new(root.clone()),
            root,
        }
    }

    /// Analyze one document: preprocess → parse → lower → semantic index.
    /// Open-document overlays (unsaved editor buffers) participate in include
    /// resolution before the filesystem. OSTW documents (`.ostw`/`.del`)
    /// route through the same shared lower/analyze/index path over the #118
    /// semantic HIR instead of a separate tool stack.
    pub fn analyze(&self, document: &Document) -> Analysis {
        if is_ostw_document(&document.uri) {
            return self.analyze_ostw(document);
        }
        let overlay = self.store.overlay(&self.root);
        let wright_opy::CompileOutcome { hir, error, files } =
            wright_opy::compile_with_overlay_outcome(
                &document.text,
                &document.uri,
                &self.root,
                &overlay,
            );
        let Some(hir) = hir else {
            return Analysis {
                program: wir::Program::default(),
                index: None,
                findings: Vec::new(),
                parse_errors: error.into_iter().collect(),
                files,
            };
        };
        let mut findings = Vec::new();
        let mut program = wir::Program::default();
        if let Ok(model) = hir.to_ir() {
            if let Ok(lowered) = wright_ir::lower::lower(&model) {
                program = lowered;
                findings = analysis::analyze(&program);
            }
        }
        let index = SemanticIndex::build(&program).ok();
        Analysis {
            program,
            index,
            findings,
            parse_errors: Vec::new(),
            files,
        }
    }

    /// Analyze an OSTW document through the shared services: the native
    /// frontend loads the project and resolves the #118 semantic HIR, which
    /// is lowered through the shared HIR→WIR path; diagnostics and the
    /// semantic index then come from the same shared code OPY/Workshop use.
    fn analyze_ostw(&self, document: &Document) -> Analysis {
        let relative = crate::document::uri_to_path(&document.uri).and_then(|path| {
            path.strip_prefix(&self.root)
                .ok()
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        });
        let (outcome, semantic) =
            wright_ostw::compile_with_semantics(&document.text, relative.as_deref(), &self.root);
        let files: Vec<FileRecord> = outcome
            .project
            .as_ref()
            .map(|project| {
                project
                    .files
                    .iter()
                    .map(|file| FileRecord {
                        id: file.id,
                        path: file.path.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut parse_errors: Vec<wright_opy::FrontendError> =
            outcome.diagnostics.iter().map(ostw_error_to_opy).collect();
        parse_errors.extend(semantic.diagnostics.iter().map(ostw_error_to_opy));
        let Some(hir) = semantic.hir else {
            return Analysis {
                program: wir::Program::default(),
                index: None,
                findings: Vec::new(),
                parse_errors,
                files,
            };
        };
        let mut findings = Vec::new();
        let mut program = wir::Program::default();
        if let Ok(lowered) = wright_ir::lower::lower(&hir) {
            if lowered.validate().is_ok() {
                program = lowered;
                findings = analysis::analyze(&program);
            }
        }
        let index = SemanticIndex::build(&program).ok();
        Analysis {
            program,
            index,
            findings,
            parse_errors,
            files,
        }
    }

    /// Diagnostics for a document: parse errors and semantic findings,
    /// each source-aware (see [`SourceDiagnostic`]).
    pub fn diagnostics(&self, uri: &str) -> Vec<SourceDiagnostic> {
        let Some(document) = self.store.document(uri) else {
            return Vec::new();
        };
        let analysis = self.analyze(document);
        let mut diagnostics = Vec::new();
        for error in &analysis.parse_errors {
            let span = error.span.as_ref().map(ir_span);
            let (source, range) = self.diagnostic_location(&analysis.files, document, span);
            let source_version = self.source_version(&source, document);
            diagnostics.push(SourceDiagnostic {
                source,
                range,
                severity: "error".to_string(),
                code: error.code.clone(),
                message: error.message.clone(),
                source_version,
                document_version: document.version,
            });
        }
        for finding in &analysis.findings {
            let (source, range) = self.diagnostic_location(&analysis.files, document, finding.span);
            let source_version = self.source_version(&source, document);
            diagnostics.push(SourceDiagnostic {
                source,
                range,
                severity: severity_name(finding.severity).to_string(),
                code: finding.code.to_string(),
                message: finding.message.clone(),
                source_version,
                document_version: document.version,
            });
        }
        diagnostics
    }

    /// Open document URIs whose analysis may change when `uri` changes,
    /// including `uri` itself when it is open. A document depends on another
    /// when the latter appears in its frontend include closure, so an open
    /// overlay or filesystem include change refreshes dependent root
    /// documents without a diagnostics-only project model.
    pub fn dependent_documents(&self, uri: &str) -> Vec<String> {
        let mut affected = Vec::new();
        if self.store.document(uri).is_some() {
            affected.push(uri.to_string());
        }
        let changed_path = crate::document::uri_to_path(uri);
        for open_uri in self.store.uris() {
            if open_uri == uri {
                continue;
            }
            if self.document_includes_path(open_uri, changed_path.as_ref()) {
                affected.push(open_uri.to_string());
            }
        }
        affected
    }

    /// Whether `document_uri` includes `changed_path` directly or
    /// transitively, according to the frontend file registry.
    fn document_includes_path(&self, document_uri: &str, changed_path: Option<&PathBuf>) -> bool {
        let Some(changed_path) = changed_path else {
            return false;
        };
        let Some(document) = self.store.document(document_uri) else {
            return false;
        };
        let analysis = self.analyze(document);
        analysis.files.iter().skip(1).any(|file| {
            let include_path = PathBuf::from(&file.path);
            let resolved = if include_path.is_absolute() {
                include_path
            } else {
                self.root.join(include_path)
            };
            &resolved == changed_path
        })
    }

    /// Hover content for a symbol at a position.
    pub fn hover(&self, uri: &str, position: Position) -> Option<Hover> {
        let document = self.store.document(uri)?;
        let analysis = self.analyze(document);
        let symbol = self.symbol_at(&analysis, document, position)?;
        let usage = analysis.index.as_ref().map(|index| index.usage(symbol.id));
        let mut contents = format!("**{}** · {}", symbol.name, symbol_kind_name(symbol.kind));
        if let Some(usage) = usage {
            contents.push_str(&format!(
                "\nreads: {}, writes: {}, calls: {}, rules: {}",
                usage.reads, usage.writes, usage.calls, usage.rules
            ));
        }
        let range = symbol.span.map(|span| document.from_span(&span));
        Some(Hover {
            contents,
            range,
            document_version: document.version,
        })
    }

    /// The definition location of the symbol referenced at a position.
    ///
    /// Prefers the symbol's definition site (a `def` body for subroutines),
    /// falling back to its declaration. Returns a source-aware location:
    /// cross-file declarations carry their own source identity (the included
    /// file), not the requesting document.
    pub fn definition(&self, uri: &str, position: Position) -> Option<SourceLocation> {
        let document = self.store.document(uri)?;
        let analysis = self.analyze(document);
        let symbol = self.symbol_at(&analysis, document, position)?;
        let span = analysis
            .index
            .as_ref()
            .and_then(|index| {
                index
                    .references(symbol.id)
                    .iter()
                    .find(|reference| {
                        reference.kind == wright_analyzer::symbols::ReferenceKind::Definition
                    })
                    .and_then(|reference| reference.span)
            })
            .or(symbol.span)?;
        Some(self.source_location(&analysis.files, document, span))
    }

    /// Every reference location of the symbol at a position.
    ///
    /// Each location is source-aware and preserves the compiler's `span.file`
    /// provenance so references into included files point at the correct
    /// source identity.
    pub fn references(&self, uri: &str, position: Position) -> Vec<SourceLocation> {
        let Some(document) = self.store.document(uri) else {
            return Vec::new();
        };
        let analysis = self.analyze(document);
        let Some(symbol) = self.symbol_at(&analysis, document, position) else {
            return Vec::new();
        };
        analysis
            .index
            .as_ref()
            .map(|index| {
                index
                    .references(symbol.id)
                    .iter()
                    .filter_map(|reference| reference.span)
                    .map(|span| self.source_location(&analysis.files, document, span))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Map a compiler span to its source identity and 0-based range.
    fn source_location(
        &self,
        files: &[FileRecord],
        document: &Document,
        span: wright_ir::source::Span,
    ) -> SourceLocation {
        let file_index = span.file.index();
        let source = self.source_identity(files, document, file_index);
        let source_text = self.source_text(&source, document);
        SourceLocation {
            source,
            range: crate::document::span_to_range(&span, &source_text),
        }
    }

    /// The editor-neutral source identity for a frontend file id: the
    /// requesting document URI for the main file, or a resolved path for an
    /// included file (relative include paths resolve against the workspace
    /// root).
    fn source_identity(
        &self,
        files: &[FileRecord],
        document: &Document,
        file_index: usize,
    ) -> String {
        if file_index == 0 {
            return document.uri.clone();
        }
        match files.iter().find(|file| file.id as usize == file_index) {
            Some(file) => {
                let path = PathBuf::from(&file.path);
                if path.is_absolute() {
                    path.to_string_lossy().into_owned()
                } else {
                    self.root.join(path).to_string_lossy().into_owned()
                }
            }
            None => format!("<file {file_index}>"),
        }
    }

    /// The text of a source identity, preferring an open unsaved document
    /// overlay and falling back to the filesystem.
    fn source_text(&self, source: &str, document: &Document) -> String {
        if source == document.uri {
            return document.text.clone();
        }
        let path = PathBuf::from(source);
        self.store.text_for_path(&path).unwrap_or_default()
    }

    /// The version of a source identity: the document version for the main
    /// source, the open overlay's version when the source is an open
    /// document, or 0 for a filesystem-backed include without an open buffer.
    fn source_version(&self, source: &str, document: &Document) -> i32 {
        if source == document.uri {
            return document.version;
        }
        let path = PathBuf::from(source);
        self.store
            .uri_for_path(&path)
            .and_then(|uri| self.store.document(&uri))
            .map(|document| document.version)
            .unwrap_or(0)
    }

    /// Map an optional compiler span to its source identity and 0-based
    /// range, using the span's actual source text.
    fn diagnostic_location(
        &self,
        files: &[FileRecord],
        document: &Document,
        span: Option<wright_ir::source::Span>,
    ) -> (String, Range) {
        let Some(span) = span else {
            return (document.uri.clone(), empty_range());
        };
        let file_index = span.file.index();
        let source = self.source_identity(files, document, file_index);
        let source_text = self.source_text(&source, document);
        (source, crate::document::span_to_range(&span, &source_text))
    }

    /// Completion items: declared symbols, builtin names, and keywords.
    pub fn completion(&self, uri: &str, position: Position) -> Vec<CompletionItem> {
        let Some(document) = self.store.document(uri) else {
            return Vec::new();
        };
        let analysis = self.analyze(document);

        // Position/context: the identifier being typed and whether the
        // position follows a member-access dot.
        let prefix = word_prefix(&document.text, position);
        let member = member_receiver(&document.text, position);

        if let Some(receiver) = member {
            // Member context: enum members when the receiver is a declared
            // enum domain, otherwise the manifest-declared receiver members
            // (the manifest is the authoritative OPY semantic table, #109).
            if let Some(domain) = wright_opy::manifest::Manifest::builtin()
                .ok()
                .and_then(|manifest| manifest.enum_domain(&receiver))
            {
                return domain
                    .members
                    .iter()
                    .filter(|member| member.starts_with(&prefix))
                    .map(|member| CompletionItem {
                        label: member.clone(),
                        kind: "enumMember".to_string(),
                        detail: Some(domain.domain.clone()),
                    })
                    .collect();
            }
            return receiver_members()
                .into_iter()
                .filter(|member| member.starts_with(&prefix))
                .map(|member| CompletionItem {
                    label: member.to_string(),
                    kind: "method".to_string(),
                    detail: Some("receiver member".to_string()),
                })
                .collect();
        }

        let mut items = Vec::new();
        if let Some(index) = &analysis.index {
            for symbol in index.symbols() {
                if symbol.name.starts_with(&prefix) {
                    items.push(CompletionItem {
                        label: symbol.name.clone(),
                        kind: symbol_kind_name(symbol.kind).to_string(),
                        detail: None,
                    });
                }
            }
        }
        for builtin in builtin_names() {
            if builtin.starts_with(&prefix) {
                items.push(CompletionItem {
                    label: builtin.to_string(),
                    kind: "function".to_string(),
                    detail: Some("builtin".to_string()),
                });
            }
        }
        for keyword in KEYWORDS {
            if keyword.starts_with(&prefix) {
                items.push(CompletionItem {
                    label: keyword.to_string(),
                    kind: "keyword".to_string(),
                    detail: None,
                });
            }
        }
        items
    }

    /// Rename the symbol at a position across the whole project.
    ///
    /// Resolves the symbol through the semantic index of every open root
    /// whose project includes the requesting document, unions the
    /// declaration/definition/reference *spans* of that symbol, produces one
    /// source-aware full-document edit per affected source whose replacement
    /// text is built from those exact semantic spans (never a whole-word scan
    /// of the raw source), and validates the edited project through the
    /// compiler pipeline before returning. Every refusal — an unresolvable
    /// symbol, an unestablished source identity, a target collision, an
    /// affected source that changed relative to the validated state, or
    /// failed edited-project validation — is explicit (`ok = false` with
    /// diagnostics); a silent or partial rename is never returned.
    pub fn rename(&self, uri: &str, position: Position, new_name: &str) -> RenameResult {
        if new_name.is_empty() {
            return RenameResult {
                document_version: 0,
                ok: false,
                edits: Vec::new(),
                diagnostics: vec!["rename-invalid-name: the new name must not be empty".to_string()],
            };
        }
        let Some(requesting) = self.store.document(uri) else {
            return RenameResult {
                document_version: 0,
                ok: false,
                edits: Vec::new(),
                diagnostics: vec![format!(
                    "rename-unresolved: no open document for '{uri}'; the source identity cannot be established"
                )],
            };
        };
        let (line, col) = requesting.to_line_col(position);
        let requesting_canonical = self.canonical_source(&requesting.uri);
        let mut from: Option<String> = None;
        let mut collision: Option<String> = None;
        // Canonical source identity -> the symbol's exact 1-based occurrence
        // spans (the declaration identifier, the definition identifier, and
        // every reference identifier), deduplicated across the root analyses
        // that reach the requesting file.
        let mut targets: BTreeMap<String, BTreeSet<TargetSpan>> = BTreeMap::new();

        // Union the symbol's exact occurrences across every open root whose
        // project includes the requesting document, so a rename from a
        // declaration in an included file also reaches the roots that
        // reference it. The position is matched only against spans in the
        // requesting document's file, never by coincidental line/column in
        // another file.
        for root_uri in self.dependent_documents(uri) {
            let Some(root_document) = self.store.document(&root_uri) else {
                continue;
            };
            let analysis = self.analyze(root_document);
            let Some(file_index) = (0..analysis.files.len()).find(|index| {
                let source = self.source_identity(&analysis.files, root_document, *index);
                self.canonical_source(&source) == requesting_canonical
            }) else {
                continue;
            };
            let Some(symbol) = self.symbol_at_in_file(&analysis, file_index, line, col) else {
                continue;
            };
            from.get_or_insert_with(|| symbol.name.clone());
            if let Some(problem) = self.collision_problem(&analysis, &symbol, new_name) {
                collision = Some(problem);
                break;
            }
            if let Some(index) = &analysis.index {
                let mut occurrences = Vec::new();
                if let Some(occurrence) = symbol.occurrence {
                    occurrences.push(occurrence);
                }
                for reference in index.references(symbol.id) {
                    match (reference.occurrence, reference.span) {
                        (Some(occurrence), _) => occurrences.push(occurrence),
                        (None, Some(span)) => {
                            // The reference has a source location but no exact
                            // identifier occurrence: an exact target cannot be
                            // established, so the rename refuses rather than
                            // broadening to the statement span.
                            let source = self.source_identity(
                                &analysis.files,
                                root_document,
                                span.file.index(),
                            );
                            return RenameResult {
                                document_version: requesting.version,
                                ok: false,
                                edits: Vec::new(),
                                diagnostics: vec![format!(
                                    "rename-unresolved-target: a semantic occurrence in {source} has no exact identifier span; refusing the rename rather than broadening to a statement span"
                                )],
                            };
                        }
                        (None, None) => {}
                    }
                }
                for occurrence in occurrences {
                    let source = self.source_identity(
                        &analysis.files,
                        root_document,
                        occurrence.file.index(),
                    );
                    targets
                        .entry(self.canonical_source(&source))
                        .or_default()
                        .insert(TargetSpan::from_span(occurrence));
                }
            }
        }

        if from.is_none() {
            return RenameResult {
                document_version: requesting.version,
                ok: false,
                edits: Vec::new(),
                diagnostics: vec![
                    "rename-unresolved: no symbol is resolvable at the requested position"
                        .to_string(),
                ],
            };
        }
        if let Some(problem) = collision {
            return RenameResult {
                document_version: requesting.version,
                ok: false,
                edits: Vec::new(),
                diagnostics: vec![problem],
            };
        }

        // Produce one full-document edit per affected source from the symbol's
        // exact semantic occurrence ranges. Each range is the identifier
        // itself (declaration/definition/reference), so no source-wide or
        // statement-level scan ever edits unrelated text. Open overlays take
        // precedence over disk, and the edit carries the identity/version
        // preconditions of the exact text it was computed from.
        let mut edits = Vec::new();
        for (source, spans) in &targets {
            let text = self.source_text(source, requesting);
            let Some(new_text) = renamed_text(&text, new_name, spans) else {
                return RenameResult {
                    document_version: requesting.version,
                    ok: false,
                    edits: Vec::new(),
                    diagnostics: vec![format!(
                        "rename-incomplete-coverage: an exact semantic target in {source} cannot be established; refusing the rename rather than editing unrelated text"
                    )],
                };
            };
            edits.push(RenameEdit {
                source: source.clone(),
                range: full_document_range(&text),
                new_text,
                source_identity: wright_driver::input_identity(&text),
                source_version: self.source_version(source, requesting),
            });
        }

        // Stale-state guard: the rename must not return edits for a source
        // that changed relative to the validated state. Re-fetch the current
        // effective text of every affected source and verify the identity the
        // edits were computed against still holds.
        for edit in &edits {
            if wright_driver::input_identity(&self.source_text(&edit.source, requesting))
                != edit.source_identity
            {
                return RenameResult {
                    document_version: requesting.version,
                    ok: false,
                    edits: Vec::new(),
                    diagnostics: vec![format!(
                        "rename-stale-source: {} changed relative to the validated state; re-fetch the source and retry",
                        edit.source
                    )],
                };
            }
        }

        // Validate the edited project state before returning.
        if let Some(problems) = self.validate_renamed_project(uri, &edits) {
            return RenameResult {
                document_version: requesting.version,
                ok: false,
                edits: Vec::new(),
                diagnostics: problems,
            };
        }

        RenameResult {
            document_version: requesting.version,
            ok: true,
            edits,
            diagnostics: Vec::new(),
        }
    }

    /// A refusal reason when renaming `symbol` to `new_name` would introduce
    /// a target-name collision with another declared symbol, else `None`.
    ///
    /// A same-spelled symbol in a different namespace is *not* a collision:
    /// the semantic index distinguishes the two identities by typed symbol ID,
    /// so span-targeted rename edits only the selected symbol's occurrences
    /// (test C). Only a genuine target-name conflict refuses the rename.
    fn collision_problem(
        &self,
        analysis: &Analysis,
        symbol: &wright_analyzer::symbols::Symbol,
        new_name: &str,
    ) -> Option<String> {
        let Some(index) = &analysis.index else {
            return Some("rename-collision: semantic identity is unavailable".to_string());
        };
        for other in index.symbols() {
            if other.id == symbol.id {
                continue;
            }
            if other.name == new_name {
                return Some(format!(
                    "rename-collision: '{}' is already declared as {}; the new name would collide",
                    new_name,
                    symbol_kind_name(other.kind)
                ));
            }
        }
        None
    }

    /// Validate the edited project: every affected root must still compile
    /// with the edits applied as overlays. Returns refusal reasons when any
    /// affected root fails.
    fn validate_renamed_project(
        &self,
        requesting_uri: &str,
        edits: &[RenameEdit],
    ) -> Option<Vec<String>> {
        let edited: BTreeMap<String, String> = edits
            .iter()
            .map(|edit| (edit.source.clone(), edit.new_text.clone()))
            .collect();
        let overlay = self.overlay_with_edits(&edited);

        // Affected roots: the requesting document plus any open document that
        // includes an edited source.
        let mut roots = vec![requesting_uri.to_string()];
        for open_uri in self.store.uris() {
            if open_uri == requesting_uri {
                continue;
            }
            if edited
                .keys()
                .any(|source| self.document_includes_source(open_uri, source))
            {
                roots.push(open_uri.to_string());
            }
        }

        let mut problems = Vec::new();
        for root in roots {
            let Some(document) = self.store.document(&root) else {
                continue;
            };
            let canonical = self.canonical_source(&document.uri);
            let main_text = edited
                .get(&canonical)
                .cloned()
                .unwrap_or_else(|| document.text.clone());
            let outcome = wright_opy::compile_with_overlay_outcome(
                &main_text,
                &document.uri,
                &self.root,
                &overlay,
            );
            if let Some(error) = outcome.error {
                problems.push(format!("{}: {}", error.code, error.message));
            }
        }
        if problems.is_empty() {
            None
        } else {
            Some(problems)
        }
    }

    /// The canonical filesystem identity of a source: the resolved path for
    /// file-backed URIs/paths, or the original identity for synthetic
    /// sources. Two analyses may identify the same physical file by URI (as a
    /// root) or by resolved path (as an include); the canonical form lets
    /// project-wide rename dedupe them.
    fn canonical_source(&self, source: &str) -> String {
        match crate::document::uri_to_path(source) {
            Some(path) => path.to_string_lossy().into_owned(),
            None => source.to_string(),
        }
    }

    /// The include overlay with edited sources taking precedence over open
    /// overlays and the filesystem.
    fn overlay_with_edits(&self, edited: &BTreeMap<String, String>) -> BTreeMap<String, String> {
        let mut overlay = self.store.overlay(&self.root);
        for (source, text) in edited {
            let path = PathBuf::from(source);
            overlay.insert(path.to_string_lossy().into_owned(), text.clone());
            if let Ok(relative) = path.strip_prefix(&self.root) {
                overlay.insert(relative.to_string_lossy().into_owned(), text.clone());
            }
            if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                overlay.insert(name.to_string(), text.clone());
            }
        }
        overlay
    }

    /// Whether an open document's include closure references `source`.
    fn document_includes_source(&self, document_uri: &str, source: &str) -> bool {
        let Some(document) = self.store.document(document_uri) else {
            return false;
        };
        let analysis = self.analyze(document);
        let target = PathBuf::from(source);
        analysis.files.iter().any(|file| {
            let include_path = PathBuf::from(&file.path);
            let resolved = if include_path.is_absolute() {
                include_path
            } else {
                self.root.join(include_path)
            };
            resolved == target
        })
    }

    /// Semantic tokens for a document, classified by the native lexer.
    pub fn semantic_tokens(&self, uri: &str) -> Vec<SemanticToken> {
        let Some(document) = self.store.document(uri) else {
            return Vec::new();
        };
        if is_ostw_document(uri) {
            return self.semantic_tokens_ostw(document);
        }
        let analysis = self.analyze(document);
        // Token classification needs the raw lexer stream plus the semantic
        // index for symbol identity (never name-string membership alone).
        let tokens = match wright_opy::lexer::lex(wright_opy::lexer::LexInput {
            file_id: 0,
            text: &document.text,
        }) {
            Ok(tokens) => tokens,
            Err(_) => return Vec::new(),
        };
        let mut result = Vec::new();
        for token in &tokens {
            if token.kind == wright_opy::lexer::TokenKind::Eof {
                continue;
            }
            let token_type = classify_token(token, analysis.index.as_ref());
            if token_type.is_empty() {
                continue;
            }
            result.push(SemanticToken {
                line: token.span.start.line.saturating_sub(1),
                character: crate::document::char_offset_to_utf16(
                    document
                        .text
                        .lines()
                        .nth(token.span.start.line.saturating_sub(1) as usize)
                        .unwrap_or_default(),
                    token.span.start.col.saturating_sub(1) as usize,
                ) as u32,
                length: crate::document::utf16_len(&token.text).max(1) as u32,
                token_type,
            });
        }
        result
    }

    /// Semantic tokens for an OSTW document: the OSTW frontend lexer
    /// supplies the token stream; symbol classification goes through the
    /// shared semantic index over the lowered program, exactly like OPY.
    fn semantic_tokens_ostw(&self, document: &Document) -> Vec<SemanticToken> {
        let analysis = self.analyze(document);
        let tokens = match wright_ostw::lexer::lex(wright_ostw::lexer::LexInput {
            file_id: wright_ir::ids::Id::from_index(0),
            text: &document.text,
        }) {
            Ok(tokens) => tokens,
            Err(_) => return Vec::new(),
        };
        let mut result = Vec::new();
        for token in &tokens {
            if token.kind == wright_ostw::lexer::TokenKind::Eof {
                continue;
            }
            let token_type = classify_ostw_token(token, analysis.index.as_ref());
            if token_type.is_empty() {
                continue;
            }
            result.push(SemanticToken {
                line: token.span.start.line.saturating_sub(1),
                character: crate::document::char_offset_to_utf16(
                    document
                        .text
                        .lines()
                        .nth(token.span.start.line.saturating_sub(1) as usize)
                        .unwrap_or_default(),
                    token.span.start.col.saturating_sub(1) as usize,
                ) as u32,
                length: crate::document::utf16_len(&token.text).max(1) as u32,
                token_type,
            });
        }
        result
    }

    /// The symbol whose declaration or reference span contains a position.
    fn symbol_at(
        &self,
        analysis: &Analysis,
        document: &Document,
        position: Position,
    ) -> Option<Symbol> {
        let (line, col) = document.to_line_col(position);
        self.symbol_at_line_col(analysis, line, col)
    }

    /// The symbol whose declaration or reference span contains a 1-based
    /// line/column. Line/column coordinates are resolved against the
    /// requesting document so a position in an included file matches spans in
    /// any root analysis that splices that file.
    fn symbol_at_line_col(&self, analysis: &Analysis, line: u32, col: u32) -> Option<Symbol> {
        let index = analysis.index.as_ref()?;
        for symbol in index.symbols() {
            let symbol_id = symbol.id;
            if let Some(span) = symbol.span {
                if span_contains(span, line, col) {
                    return Some(symbol.clone());
                }
            }
            for reference in index.references(symbol_id) {
                if let Some(span) = reference.span {
                    if span_contains(span, line, col) {
                        return Some(symbol.clone());
                    }
                }
            }
        }
        None
    }

    /// The symbol whose declaration or reference span in `file_index`
    /// contains a 1-based line/column. Unlike [`Self::symbol_at_line_col`],
    /// spans in other files are never considered, so a position in the
    /// requesting document cannot resolve to a coincidental same-coordinate
    /// symbol in an included file.
    fn symbol_at_in_file(
        &self,
        analysis: &Analysis,
        file_index: usize,
        line: u32,
        col: u32,
    ) -> Option<Symbol> {
        let index = analysis.index.as_ref()?;
        for symbol in index.symbols() {
            let symbol_id = symbol.id;
            if let Some(span) = symbol.span {
                if span.file.index() == file_index && span_contains(span, line, col) {
                    return Some(symbol.clone());
                }
            }
            for reference in index.references(symbol_id) {
                if let Some(span) = reference.span {
                    if span.file.index() == file_index && span_contains(span, line, col) {
                        return Some(symbol.clone());
                    }
                }
            }
        }
        None
    }
}

/// Convert a frontend span to the IR span representation.
fn ir_span(span: &wright_opy::diag::Span) -> wright_ir::source::Span {
    wright_ir::source::Span::new(
        wright_ir::ids::Id::from_index(span.file as usize),
        wright_ir::source::Position::new(span.start.line, span.start.col),
        wright_ir::source::Position::new(span.end.line, span.end.col),
    )
}

/// Whether a document URI names an OSTW source (`.ostw`/`.del`), mirroring
/// the driver's extension detection.
fn is_ostw_document(uri: &str) -> bool {
    crate::document::uri_to_path(uri)
        .and_then(|path| {
            path.extension()
                .map(|ext| ext.to_string_lossy().to_lowercase())
        })
        .map(|extension| extension == "ostw" || extension == "del")
        .unwrap_or(false)
}

/// Map an OSTW frontend error into the shared language-service error shape
/// (same code/message/span contract; the registry ids are project ids).
fn ostw_error_to_opy(error: &wright_ostw::FrontendError) -> wright_opy::FrontendError {
    wright_opy::FrontendError {
        code: error.code.clone(),
        message: error.message.clone(),
        span: error.span.map(opy_span),
    }
}

/// Convert a shared `wright_ir` span into the language service's frontend
/// span shape.
fn opy_span(span: wright_ir::source::Span) -> wright_opy::diag::Span {
    wright_opy::diag::Span {
        file: span.file.index() as u32,
        start: wright_opy::diag::Position::new(span.start.line, span.start.col),
        end: wright_opy::diag::Position::new(span.end.line, span.end.col),
    }
}

fn span_contains(span: wright_ir::source::Span, line: u32, col: u32) -> bool {
    (span.start.line, span.start.col) <= (line, col)
        && (line, col)
            <= (
                span.end.line,
                span.end.col.saturating_sub(1).max(span.start.col),
            )
}

fn empty_range() -> Range {
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: 0,
            character: 0,
        },
    }
}

/// A 0-based range covering the entire source text, including any trailing
/// newline (so a full-document replacement can delete the final line break).
/// The end character is a UTF-16 code-unit offset, matching the editor
/// convention used everywhere else at the LSP boundary.
fn full_document_range(text: &str) -> Range {
    let lines: Vec<&str> = text.split('\n').collect();
    let line_count = lines.len().max(1) as u32;
    let last_line_len = crate::document::utf16_len(lines.last().unwrap_or(&"")) as u32;
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: line_count - 1,
            character: last_line_len,
        },
    }
}

/// One 1-based source span of the renamed symbol (a declaration, definition,
/// or reference site), deduplicated across the root analyses that reach the
/// requesting file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TargetSpan {
    start_line: u32,
    start_col: u32,
    end_line: u32,
    end_col: u32,
}

impl TargetSpan {
    fn from_span(span: wright_ir::source::Span) -> TargetSpan {
        TargetSpan {
            start_line: span.start.line,
            start_col: span.start.col,
            end_line: span.end.line,
            end_col: span.end.col,
        }
    }
}

/// Build the renamed source text by editing exactly the symbol's semantic
/// occurrence ranges: every 1-based half-open target span (the declaration
/// identifier, the definition identifier, or a reference identifier) is
/// replaced with `to`.
///
/// The spans are identifier-exact by construction — derived in the
/// frontend/HIR/WIR/semantic-index provenance path, never by scanning source
/// text for the spelling — so a same-spelled string, comment, or sibling
/// identifier anywhere else in the source is never touched. A span that is
/// not a valid single-line identifier range in the source (an exact target
/// that cannot be established) returns `None` so the caller refuses instead
/// of emitting a partial edit.
fn renamed_text(text: &str, to: &str, spans: &BTreeSet<TargetSpan>) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    // Group the exact 0-based character ranges by line, validating each span
    // against the source so a stale or imprecise span refuses rather than
    // silently editing the wrong text.
    let mut per_line: BTreeMap<u32, Vec<(usize, usize)>> = BTreeMap::new();
    for span in spans {
        if span.start_line != span.end_line || span.start_line == 0 {
            return None;
        }
        let line = lines.get(span.start_line as usize - 1)?;
        let char_count = line.chars().count() as u32;
        if span.start_col == 0 || span.end_col < span.start_col || span.end_col > char_count + 1 {
            return None;
        }
        per_line
            .entry(span.start_line - 1)
            .or_default()
            .push((span.start_col as usize - 1, span.end_col as usize - 1));
    }

    // Apply the replacements line by line in ascending order with a running
    // cursor; the ranges are disjoint (deduplicated exact occurrences), so
    // earlier offsets are consumed in order and never shift.
    let mut out = Vec::with_capacity(lines.len());
    for (line_number, line) in lines.iter().enumerate() {
        let mut ranges = per_line.remove(&(line_number as u32)).unwrap_or_default();
        ranges.sort_unstable();
        let chars: Vec<char> = line.chars().collect();
        let mut rebuilt = String::with_capacity(line.len());
        let mut cursor = 0usize;
        for (start, end) in ranges {
            rebuilt.extend(chars[cursor..start].iter());
            rebuilt.push_str(to);
            cursor = end;
        }
        rebuilt.extend(chars[cursor..].iter());
        out.push(rebuilt);
    }
    Some(out.join("\n"))
}

fn severity_name(severity: analysis::Severity) -> &'static str {
    match severity {
        analysis::Severity::Warning => "warning",
        analysis::Severity::Info => "info",
    }
}

fn symbol_kind_name(kind: wright_analyzer::symbols::SymbolKind) -> &'static str {
    match kind {
        wright_analyzer::symbols::SymbolKind::GlobalVariable => "globalVariable",
        wright_analyzer::symbols::SymbolKind::PlayerVariable => "playerVariable",
        wright_analyzer::symbols::SymbolKind::Subroutine => "subroutine",
        wright_analyzer::symbols::SymbolKind::Rule => "rule",
    }
}

/// Every manifest-declared generic builtin id (the authoritative builtin
/// surface, #109); the source for builtin completion and token
/// classification.
fn builtin_names() -> Vec<&'static str> {
    wright_opy::manifest::Manifest::builtin()
        .map(|manifest| {
            manifest
                .functions
                .iter()
                .filter(|function| !function.kind.is_member())
                .map(|function| function.id.as_str())
                .collect()
        })
        .unwrap_or_default()
}

/// Every manifest-declared receiver member id (#109); the source for
/// member-access completion.
fn receiver_members() -> Vec<&'static str> {
    wright_opy::manifest::Manifest::builtin()
        .map(|manifest| {
            manifest
                .functions
                .iter()
                .filter(|function| function.kind.is_member())
                .map(|function| function.id.as_str())
                .collect()
        })
        .unwrap_or_default()
}

/// Source keywords offered by completion.
const KEYWORDS: &[&str] = &[
    "rule",
    "globalvar",
    "playervar",
    "subroutine",
    "def",
    "enum",
    "macro",
    "if",
    "elif",
    "else",
    "for",
    "while",
    "in",
    "and",
    "or",
    "not",
    "pass",
    "true",
    "false",
    "None",
];

/// The identifier being typed immediately before a position.
///
/// The UTF-16 editor offset is converted to a character offset first, and the
/// line is then sliced by characters (never by byte offsets), so non-ASCII
/// text before the cursor cannot shift the slice.
fn word_prefix(text: &str, position: Position) -> String {
    let line = text.lines().nth(position.line as usize).unwrap_or_default();
    let char_end = crate::document::utf16_offset_to_char(line, position.character as usize);
    let before: String = line.chars().take(char_end).collect();
    before
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

/// The receiver immediately before a member-access dot, when the position
/// follows `<receiver>.` (possibly with a partial member typed after the dot).
fn member_receiver(text: &str, position: Position) -> Option<String> {
    let line = text.lines().nth(position.line as usize)?;
    let char_end = crate::document::utf16_offset_to_char(line, position.character as usize);
    let before: String = line.chars().take(char_end).collect();
    let trimmed = before.trim_end().strip_suffix('.')?;
    let name = trimmed
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect::<Vec<_>>();
    if name.is_empty() {
        None
    } else {
        Some(name.into_iter().rev().collect())
    }
}

/// Classify one token by parser/semantic identity: keywords by parser
/// identity, symbol references by semantic index spans and symbol kinds,
/// builtin functions by the corpus catalog, and everything else as an
/// identifier.
fn classify_token(token: &wright_opy::lexer::Token, index: Option<&SemanticIndex>) -> String {
    use wright_opy::lexer::TokenKind;
    match token.kind {
        TokenKind::Ident => {
            if KEYWORDS.contains(&token.text.as_str()) {
                return "keyword".to_string();
            }
            if let Some(index) = index {
                if let Some(kind) =
                    symbol_kind_at(index, token.span.start.line, token.span.start.col)
                {
                    return match kind {
                        wright_analyzer::symbols::SymbolKind::GlobalVariable
                        | wright_analyzer::symbols::SymbolKind::PlayerVariable => {
                            "variable".to_string()
                        }
                        wright_analyzer::symbols::SymbolKind::Subroutine => "function".to_string(),
                        wright_analyzer::symbols::SymbolKind::Rule => "class".to_string(),
                    };
                }
            }
            if builtin_names().contains(&token.text.as_str()) {
                "function".to_string()
            } else {
                "identifier".to_string()
            }
        }
        TokenKind::Number => "number".to_string(),
        TokenKind::String => "string".to_string(),
        TokenKind::Directive => "macro".to_string(),
        TokenKind::At => "attribute".to_string(),
        TokenKind::Newline | TokenKind::Indent(_) => String::new(),
        _ => "operator".to_string(),
    }
}

/// Classify one OSTW lexer token through the shared semantic index: the
/// identifier/keyword/builtin logic is identical to the OPY path, only the
/// token-kind surface differs (each frontend owns its lexer).
fn classify_ostw_token(token: &wright_ostw::lexer::Token, index: Option<&SemanticIndex>) -> String {
    use wright_ostw::lexer::TokenKind;
    match token.kind {
        TokenKind::Ident => {
            if KEYWORDS.contains(&token.text.as_str()) {
                return "keyword".to_string();
            }
            if let Some(index) = index {
                if let Some(kind) =
                    symbol_kind_at(index, token.span.start.line, token.span.start.col)
                {
                    return match kind {
                        wright_analyzer::symbols::SymbolKind::GlobalVariable
                        | wright_analyzer::symbols::SymbolKind::PlayerVariable => {
                            "variable".to_string()
                        }
                        wright_analyzer::symbols::SymbolKind::Subroutine => "function".to_string(),
                        wright_analyzer::symbols::SymbolKind::Rule => "class".to_string(),
                    };
                }
            }
            if builtin_names().contains(&token.text.as_str()) {
                "function".to_string()
            } else {
                "identifier".to_string()
            }
        }
        TokenKind::Number => "number".to_string(),
        TokenKind::String | TokenKind::VerbatimString => "string".to_string(),
        TokenKind::At => "attribute".to_string(),
        TokenKind::Eof => String::new(),
        _ => "operator".to_string(),
    }
}

/// The semantic kind of the symbol whose declaration or reference span
/// contains a 1-based line/column, when one exists.
fn symbol_kind_at(
    index: &SemanticIndex,
    line: u32,
    col: u32,
) -> Option<wright_analyzer::symbols::SymbolKind> {
    for symbol in index.symbols() {
        if symbol
            .span
            .is_some_and(|span| span_contains(span, line, col))
        {
            return Some(symbol.kind);
        }
        for reference in index.references(symbol.id) {
            if reference
                .span
                .is_some_and(|span| span_contains(span, line, col))
            {
                return Some(symbol.kind);
            }
        }
    }
    None
}
