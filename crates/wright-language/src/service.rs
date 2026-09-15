//! The editor-neutral language service (#63, #65, #66).
//!
//! Composes provider-backed source capabilities with Wright's editor-neutral
//! response and safe-edit contracts. Every result is tagged with
//! the document version it was computed for, so stale results are
//! deterministic and replaceable (#64). No LSP types appear here.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::Serialize;
use workshop_rs::Program;
use wright_analyzer::analysis::{self, Finding};
use wright_analyzer::canonical::{SemanticIndex, Symbol};

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

/// One source-aware rename edit targeting an exact semantic occurrence
/// (#131).
///
/// Carries the source identity, a 0-based editor-convention range covering
/// exactly the occurrence's identifier, the replacement text (the new name),
/// and identity/version preconditions so a client can apply the edit safely
/// and detect stale state. Multiple edits per source are allowed — this is
/// the shared #129 transaction mapped to editor conventions, never a
/// whole-document replacement.
#[derive(Debug, Clone, Serialize)]
pub struct RenameEdit {
    /// The source identity: the requesting document URI or a resolved include
    /// path.
    pub source: String,
    /// The 0-based editor range of the exact occurrence (UTF-16 positions).
    pub range: Range,
    /// The replacement text (the new name).
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
    /// The exact-occurrence edits for every semantically affected source.
    pub edits: Vec<RenameEdit>,
    /// The validated full edited text of every affected source (the #128
    /// transaction preview).
    pub previews: Vec<wright_driver::edit::SourcePreview>,
    /// Structured refusal diagnostics when the rename was rejected.
    pub diagnostics: Vec<String>,
}

/// A source-language-independent diagnostic retained by the service analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceError {
    pub code: String,
    pub message: String,
    pub span: Option<workshop_rs::source::Span>,
}

/// A source-language-independent file identity retained by the service analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub id: workshop_rs::source::FileId,
    pub path: String,
}

/// The analyzed state of one document.
pub struct Analysis {
    pub program: Program,
    pub index: Option<SemanticIndex>,
    pub findings: Vec<Finding>,
    pub parse_errors: Vec<SourceError>,
    /// The frontend file registry, retained even when parsing/lowering fails
    /// so diagnostic spans can be mapped to their actual source.
    pub files: Vec<SourceFile>,
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

    /// Analyze one document. OPY language-service capabilities are provider
    /// owned; the first-party provider currently exposes no editor capability,
    /// so Wright returns a structured refusal instead of parsing statically.
    pub fn analyze(&self, document: &Document) -> Analysis {
        let _ = document;
        unavailable_source_analysis()
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
            let span = error.span;
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
                        reference.kind == wright_analyzer::canonical::ReferenceKind::Definition
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
        files: &[SourceFile],
        document: &Document,
        span: workshop_rs::source::Span,
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
        files: &[SourceFile],
        document: &Document,
        file_index: usize,
    ) -> String {
        if file_index == 0 {
            return document.uri.clone();
        }
        match files.iter().find(|file| file.id.index() == file_index) {
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
        files: &[SourceFile],
        document: &Document,
        span: Option<workshop_rs::source::Span>,
    ) -> (String, Range) {
        let Some(span) = span else {
            return (document.uri.clone(), empty_range());
        };
        let file_index = span.file.index();
        let source = self.source_identity(files, document, file_index);
        let source_text = self.source_text(&source, document);
        (source, crate::document::span_to_range(&span, &source_text))
    }

    /// Completion is provider-owned and unavailable until the provider
    /// negotiates a completion capability.
    pub fn completion(&self, uri: &str, position: Position) -> Vec<CompletionItem> {
        let _ = (uri, position);
        Vec::new()
    }

    /// Rename the symbol at a position across the whole project.
    ///
    /// Delegates semantic target resolution, edit generation, and validation
    /// to the shared driver refactoring contract
    /// (`wright_driver::edit::semantic_rename`, #129): every open root whose
    /// project includes the requesting document resolves the symbol through
    /// the shared semantic index, and the union of the resulting exact-range
    /// transactions — deduplicated by (source, range) — is validated through
    /// `wright_driver::edit::validate_transaction` against every affected
    /// root with the original OPY project kind. No whole-word scan
    /// or document-local rename semantics exist here. Every refusal — an
    /// unresolvable symbol, an unestablished source identity, a target
    /// collision, an affected source that changed relative to the validated
    /// state, or failed edited-project validation — is explicit
    /// (`ok = false` with diagnostics); a silent or partial rename is never
    /// returned.
    pub fn rename(&self, uri: &str, position: Position, new_name: &str) -> RenameResult {
        if new_name.is_empty() {
            return RenameResult {
                document_version: 0,
                ok: false,
                edits: Vec::new(),
                previews: Vec::new(),
                diagnostics: vec![
                    "rename-invalid-name: the new name must not be empty".to_string(),
                ],
            };
        }
        let Some(requesting) = self.store.document(uri) else {
            return RenameResult {
                document_version: 0,
                ok: false,
                edits: Vec::new(),
                previews: Vec::new(),
                diagnostics: vec![format!(
                    "rename-unresolved: no open document for '{uri}'; the source identity cannot be established"
                )],
            };
        };
        if is_source_document(uri) {
            return RenameResult {
                document_version: requesting.version,
                ok: false,
                edits: Vec::new(),
                previews: Vec::new(),
                diagnostics: vec![
                    "source-provider-unavailable: OPY language-service capabilities are not currently shipped with the configured provider"
                        .to_string(),
                ],
            };
        }
        let (line, col) = requesting.to_line_col(position);
        let requesting_canonical = self.canonical_source(&requesting.uri);

        // The union of the per-root transactions, deduplicated by exact
        // (source, range), plus the current text of every source any root
        // project sees (the version/identity precondition snapshot).
        let mut unioned: BTreeMap<(String, u32, u32, u32, u32), wright_driver::edit::SourceEdit> =
            BTreeMap::new();
        let mut sources: BTreeMap<String, String> = BTreeMap::new();
        let mut found = false;

        // Union across every open root whose project includes the requesting
        // document, so a rename from a declaration in an included file also
        // reaches the roots that reference it. The position is matched only
        // against spans in the requesting document's file, never by
        // coincidental line/column in another file (the driver resolves it in
        // the root project).
        for root_uri in self.dependent_documents(uri) {
            let Some(root_document) = self.store.document(&root_uri) else {
                continue;
            };
            let Some(root_path) = crate::document::uri_to_path(&root_document.uri) else {
                continue;
            };
            let analysis = self.analyze(root_document);
            let Some(_) = (0..analysis.files.len()).find(|index| {
                let source = self.source_identity(&analysis.files, root_document, *index);
                self.canonical_source(&source) == requesting_canonical
            }) else {
                continue;
            };
            found = true;

            // The root's current project snapshot: every closure file keyed
            // by its canonical source identity (open overlays take precedence
            // over the filesystem, exactly like the service's own analysis).
            let mut root_sources: BTreeMap<String, String> = BTreeMap::new();
            for file in &analysis.files {
                let identity =
                    self.source_identity(&analysis.files, root_document, file.id.index());
                let canonical = self.canonical_source(&identity);
                root_sources.insert(canonical, self.source_text(&identity, root_document));
            }
            let config = wright_driver::SessionConfig {
                input: wright_driver::InputSpec::Path(root_path.clone()),
                // The driver detects the original project kind from the root
                // document extension, so validation runs through the owner
                // frontend.
                kind: wright_driver::SourceKind::Auto,
                root: Some(self.root.clone()),
                ..wright_driver::SessionConfig::default()
            };
            let rename = wright_driver::edit::semantic_rename(
                &config,
                &root_sources,
                &wright_driver::edit::RenameTarget {
                    source: requesting_canonical.clone(),
                    line,
                    col,
                    to: new_name.to_string(),
                },
            );
            if !rename.ok {
                return RenameResult {
                    document_version: requesting.version,
                    ok: false,
                    edits: Vec::new(),
                    previews: Vec::new(),
                    diagnostics: rename
                        .diagnostics
                        .iter()
                        .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
                        .collect(),
                };
            }
            if let Some(transaction) = &rename.transaction {
                for edit in &transaction.edits {
                    unioned.insert(
                        (
                            edit.source.clone(),
                            edit.range.start_line,
                            edit.range.start_col,
                            edit.range.end_line,
                            edit.range.end_col,
                        ),
                        edit.clone(),
                    );
                }
            }
            sources.extend(root_sources);
        }

        if !found {
            return RenameResult {
                document_version: requesting.version,
                ok: false,
                edits: Vec::new(),
                previews: Vec::new(),
                diagnostics: vec![
                    "rename-unresolved: no symbol is resolvable at the requested position"
                        .to_string(),
                ],
            };
        }
        let transaction =
            match wright_driver::edit::EditTransaction::new(unioned.into_values().collect()) {
                Ok(transaction) => transaction,
                Err(diagnostic) => {
                    return RenameResult {
                        document_version: requesting.version,
                        ok: false,
                        edits: Vec::new(),
                        previews: Vec::new(),
                        diagnostics: vec![format!("{}: {}", diagnostic.code, diagnostic.message)],
                    };
                }
            };

        // Stale-state guard: the rename must not return edits for a source
        // that changed relative to the validated state. Re-fetch the current
        // effective text of every affected source and verify the identity the
        // edits were computed against still holds.
        for edit in &transaction.edits {
            if wright_driver::input_identity(&self.source_text(&edit.source, requesting))
                != edit.source_identity
            {
                return RenameResult {
                    document_version: requesting.version,
                    ok: false,
                    edits: Vec::new(),
                    previews: Vec::new(),
                    diagnostics: vec![format!(
                        "rename-stale-source: {} changed relative to the validated state; re-fetch the source and retry",
                        edit.source
                    )],
                };
            }
        }

        // Validate the unioned transaction through the shared #128 contract
        // against every affected root before returning success.
        let (problems, previews) = self.validate_renamed_project(uri, &transaction, &sources);
        if let Some(problems) = problems {
            return RenameResult {
                document_version: requesting.version,
                ok: false,
                edits: Vec::new(),
                previews: Vec::new(),
                diagnostics: problems,
            };
        }

        // Materialize the shared #129 transaction in editor conventions:
        // one exact-occurrence edit per semantic occurrence (0-based UTF-16
        // ranges), plus the validated full edited text of every affected
        // source. The LSP adapter maps these edits directly to
        // `WorkspaceEdit` without re-resolving symbols or reimplementing
        // collision/stale checks (#131).
        let mut edits = Vec::new();
        for edit in &transaction.edits {
            let text = sources.get(&edit.source).cloned().unwrap_or_default();
            let span = workshop_rs::source::Span::new(
                workshop_rs::source::FileId::from_index(0),
                workshop_rs::source::Position::new(edit.range.start_line, edit.range.start_col),
                workshop_rs::source::Position::new(edit.range.end_line, edit.range.end_col),
            );
            edits.push(RenameEdit {
                source: edit.source.clone(),
                range: crate::document::span_to_range(&span, &text),
                new_text: edit.new_text.clone(),
                source_identity: edit.source_identity.clone(),
                source_version: self.source_version(&edit.source, requesting),
            });
        }

        RenameResult {
            document_version: requesting.version,
            ok: true,
            edits,
            previews,
            diagnostics: Vec::new(),
        }
    }

    /// Validate the edited project: every affected root must still compile
    /// with the unioned transaction applied as overlays. Returns refusal
    /// reasons when any affected root fails, plus the validated per-source
    /// previews (identical across roots, since the transaction and the source
    /// snapshot are the same).
    ///
    /// Validation routes through the shared driver transaction contract
    /// (#128) with the root's source kind detected from its extension; no
    /// duplicate edit-validation semantics live here.
    fn validate_renamed_project(
        &self,
        requesting_uri: &str,
        transaction: &wright_driver::edit::EditTransaction,
        sources: &BTreeMap<String, String>,
    ) -> (Option<Vec<String>>, Vec<wright_driver::edit::SourcePreview>) {
        // Affected roots: the requesting document plus any open document that
        // includes an edited source.
        let edited_sources: BTreeSet<String> = transaction
            .edits
            .iter()
            .map(|edit| edit.source.clone())
            .collect();
        let mut roots = vec![requesting_uri.to_string()];
        for open_uri in self.store.uris() {
            if open_uri == requesting_uri {
                continue;
            }
            if edited_sources
                .iter()
                .any(|source| self.document_includes_source(open_uri, source))
            {
                roots.push(open_uri.to_string());
            }
        }

        let mut problems = Vec::new();
        let mut previews = Vec::new();
        for root in roots {
            let Some(document) = self.store.document(&root) else {
                continue;
            };
            let Some(path) = crate::document::uri_to_path(&document.uri) else {
                continue;
            };
            let config = wright_driver::SessionConfig {
                input: wright_driver::InputSpec::Path(path),
                // The original project kind is detected from the root document
                // extension so the edited project validates through the owner
                // frontend.
                kind: wright_driver::SourceKind::Auto,
                root: Some(self.root.clone()),
                ..wright_driver::SessionConfig::default()
            };
            let validation =
                wright_driver::edit::validate_transaction(&config, sources, transaction);
            if !validation.ok {
                for diagnostic in &validation.diagnostics {
                    problems.push(format!("{}: {}", diagnostic.code, diagnostic.message));
                }
            } else if previews.is_empty() {
                previews = validation.preview.unwrap_or_default();
            }
        }
        if problems.is_empty() {
            (None, previews)
        } else {
            (Some(problems), Vec::new())
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

    /// Whether an open document's include closure references `source`.
    fn document_includes_source(&self, document_uri: &str, source: &str) -> bool {
        let Some(document) = self.store.document(document_uri) else {
            return false;
        };
        let analysis = self.analyze(document);
        let target = PathBuf::from(source);
        analysis
            .files
            .iter()
            .filter(|file| file.id.index() != 0)
            .any(|file| {
                let include_path = PathBuf::from(&file.path);
                let resolved = if include_path.is_absolute() {
                    include_path
                } else {
                    self.root.join(include_path)
                };
                resolved == target
            })
    }

    /// Semantic tokens are provider-owned and unavailable without a negotiated
    /// provider capability.
    pub fn semantic_tokens(&self, uri: &str) -> Vec<SemanticToken> {
        let _ = uri;
        Vec::new()
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
}

/// Whether a document is a source-language document whose semantics belong to
/// an external provider rather than Wright.
fn is_source_document(uri: &str) -> bool {
    crate::document::uri_to_path(uri)
        .and_then(|path| {
            path.extension()
                .map(|ext| ext.to_string_lossy().to_lowercase())
        })
        .is_some_and(|extension| matches!(extension.as_str(), "opy" | "ostw" | "del"))
}

fn unavailable_source_analysis() -> Analysis {
    Analysis {
        program: Program::default(),
        index: None,
        findings: Vec::new(),
        parse_errors: vec![SourceError {
            code: "source-provider-unavailable".to_string(),
            message: "source-language analysis is provider-owned and no editor capability is currently negotiated".to_string(),
            span: None,
        }],
        files: Vec::new(),
    }
}

fn span_contains(span: workshop_rs::source::Span, line: u32, col: u32) -> bool {
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

fn symbol_kind_name(kind: wright_analyzer::canonical::SymbolKind) -> &'static str {
    match kind {
        wright_analyzer::canonical::SymbolKind::GlobalVariable => "globalVariable",
        wright_analyzer::canonical::SymbolKind::PlayerVariable => "playerVariable",
        wright_analyzer::canonical::SymbolKind::Subroutine => "subroutine",
        wright_analyzer::canonical::SymbolKind::Rule => "rule",
    }
}

fn severity_name(severity: analysis::Severity) -> &'static str {
    match severity {
        analysis::Severity::Error => "error",
        analysis::Severity::Warning => "warning",
        analysis::Severity::Info => "info",
    }
}
