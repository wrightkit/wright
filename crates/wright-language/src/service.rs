//! The editor-neutral language service (#63, #65, #66).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::Serialize;
use workshop_rs::Program;
use wright_analyzer::analysis::Finding;
use wright_analyzer::canonical::{SemanticIndex, Symbol};

use crate::document::{Document, DocumentStore, Position, Range};

#[derive(Debug, Clone, Serialize)]
pub struct SourceDiagnostic {
    pub source: String,
    pub range: Range,
    pub severity: String,
    pub code: String,
    pub message: String,
    pub source_version: i32,
    pub document_version: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hover {
    pub contents: String,
    pub range: Option<Range>,
    pub document_version: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompletionItem {
    pub label: String,
    pub kind: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SemanticToken {
    pub line: u32,
    pub character: u32,
    pub length: u32,
    pub token_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceLocation {
    pub source: String,
    pub range: Range,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenameEdit {
    pub source: String,
    pub range: Range,
    pub new_text: String,
    pub source_identity: String,
    pub source_version: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenameResult {
    pub document_version: i32,
    pub ok: bool,
    pub edits: Vec<RenameEdit>,
    pub previews: Vec<wright_driver::edit::SourcePreview>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceError {
    pub code: String,
    pub message: String,
    pub span: Option<workshop_rs::source::Span>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub id: workshop_rs::source::FileId,
    pub path: String,
}

pub struct Analysis {
    pub program: Program,
    pub index: Option<SemanticIndex>,
    pub findings: Vec<Finding>,
    pub parse_errors: Vec<SourceError>,
    pub files: Vec<SourceFile>,
}

pub struct LanguageService {
    pub store: DocumentStore,
    pub root: PathBuf,
}

impl LanguageService {
    pub fn new(root: PathBuf) -> LanguageService {
        LanguageService {
            store: DocumentStore::new(root.clone()),
            root,
        }
    }

    pub fn analyze(&self, _document: &Document) -> Analysis {
        unavailable_source_analysis()
    }

    pub fn diagnostics(&self, uri: &str) -> Vec<SourceDiagnostic> {
        let Some(document) = self.store.document(uri) else {
            return Vec::new();
        };
        let analysis = self.analyze(document);
        let mut diagnostics = Vec::new();
        for error in &analysis.parse_errors {
            let (source, range) = self.diagnostic_location(&analysis.files, document, error.span);
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
                code: finding.code.clone(),
                message: finding.message.clone(),
                source_version,
                document_version: document.version,
            });
        }
        diagnostics
    }

    pub fn dependent_documents(&self, uri: &str) -> Vec<String> {
        let mut affected = Vec::new();
        if self.store.document(uri).is_some() {
            affected.push(uri.to_string());
        }
        let changed_path = crate::document::uri_to_path(uri);
        for open_uri in self.store.uris() {
            if open_uri != uri && self.document_includes_path(open_uri, changed_path.as_ref()) {
                affected.push(open_uri.to_string());
            }
        }
        affected
    }

    fn document_includes_path(&self, document_uri: &str, changed_path: Option<&PathBuf>) -> bool {
        let (Some(changed), Some(document)) = (changed_path, self.store.document(document_uri))
        else {
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
            &resolved == changed
        })
    }

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
        Some(Hover {
            contents,
            range: symbol.span.map(|s| document.from_span(&s)),
            document_version: document.version,
        })
    }

    pub fn definition(&self, uri: &str, position: Position) -> Option<SourceLocation> {
        let document = self.store.document(uri)?;
        let analysis = self.analyze(document);
        let symbol = self.symbol_at(&analysis, document, position)?;
        let span = analysis
            .index
            .as_ref()
            .and_then(|idx| {
                idx.references(symbol.id)
                    .iter()
                    .find(|r| r.kind == wright_analyzer::canonical::ReferenceKind::Definition)
                    .and_then(|r| r.span)
            })
            .or(symbol.span)?;
        Some(self.source_location(&analysis.files, document, span))
    }

    pub fn references(&self, uri: &str, position: Position) -> Vec<SourceLocation> {
        let Some(document) = self.store.document(uri) else {
            return Vec::new();
        };
        let analysis = self.analyze(document);
        let Some(symbol) = self.symbol_at(&analysis, document, position) else {
            return Vec::new();
        };
        let Some(index) = &analysis.index else {
            return Vec::new();
        };
        index
            .references(symbol.id)
            .iter()
            .filter_map(|r| {
                r.span
                    .map(|s| self.source_location(&analysis.files, document, s))
            })
            .collect()
    }

    pub fn completion(&self, _uri: &str, _position: Position) -> Vec<CompletionItem> {
        Vec::new()
    }

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
                    "source-provider-unavailable: OPY language-service capabilities are not currently shipped with the configured provider".to_string(),
                ],
            };
        }

        let (line, col) = requesting.to_line_col(position);
        let requesting_canonical = self.canonical_source(&requesting.uri);
        let mut unioned: BTreeMap<(String, u32, u32, u32, u32), wright_driver::edit::SourceEdit> =
            BTreeMap::new();
        let mut sources: BTreeMap<String, String> = BTreeMap::new();
        let mut found = false;

        for root_uri in self.dependent_documents(uri) {
            let Some(root_document) = self.store.document(&root_uri) else {
                continue;
            };
            let Some(root_path) = crate::document::uri_to_path(&root_document.uri) else {
                continue;
            };
            let analysis = self.analyze(root_document);
            let has_req = (0..analysis.files.len()).any(|idx| {
                let source = self.source_identity(&analysis.files, root_document, idx);
                self.canonical_source(&source) == requesting_canonical
            });
            if !has_req {
                continue;
            }
            found = true;

            let mut root_sources: BTreeMap<String, String> = BTreeMap::new();
            for file in &analysis.files {
                let identity =
                    self.source_identity(&analysis.files, root_document, file.id.index());
                let canonical = self.canonical_source(&identity);
                root_sources.insert(canonical, self.source_text(&identity, root_document));
            }
            let config = wright_driver::SessionConfig {
                input: wright_driver::InputSpec::Path(root_path),
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
                        .map(|d| format!("{}: {}", d.code, d.message))
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
                Ok(tx) => tx,
                Err(diag) => {
                    return RenameResult {
                        document_version: requesting.version,
                        ok: false,
                        edits: Vec::new(),
                        previews: Vec::new(),
                        diagnostics: vec![format!("{}: {}", diag.code, diag.message)],
                    };
                }
            };

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

    fn validate_renamed_project(
        &self,
        requesting_uri: &str,
        transaction: &wright_driver::edit::EditTransaction,
        sources: &BTreeMap<String, String>,
    ) -> (Option<Vec<String>>, Vec<wright_driver::edit::SourcePreview>) {
        let edited_sources: BTreeSet<String> =
            transaction.edits.iter().map(|e| e.source.clone()).collect();
        let mut roots = vec![requesting_uri.to_string()];
        for open_uri in self.store.uris() {
            if open_uri != requesting_uri
                && edited_sources
                    .iter()
                    .any(|s| self.document_includes_source(open_uri, s))
            {
                roots.push(open_uri.to_string());
            }
        }

        let mut problems = Vec::new();
        let mut previews = Vec::new();
        for root in roots {
            let (Some(_doc), Some(path)) = (
                self.store.document(&root),
                crate::document::uri_to_path(&root),
            ) else {
                continue;
            };
            let config = wright_driver::SessionConfig {
                input: wright_driver::InputSpec::Path(path),
                kind: wright_driver::SourceKind::Auto,
                root: Some(self.root.clone()),
                ..wright_driver::SessionConfig::default()
            };
            let validation =
                wright_driver::edit::validate_transaction(&config, sources, transaction);
            if !validation.ok {
                for diag in &validation.diagnostics {
                    problems.push(format!("{}: {}", diag.code, diag.message));
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

    fn canonical_source(&self, source: &str) -> String {
        crate::document::uri_to_path(source)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| source.to_string())
    }

    fn document_includes_source(&self, document_uri: &str, source: &str) -> bool {
        let Some(doc) = self.store.document(document_uri) else {
            return false;
        };
        let analysis = self.analyze(doc);
        let target = PathBuf::from(source);
        analysis
            .files
            .iter()
            .filter(|f| f.id.index() != 0)
            .any(|f| {
                let p = PathBuf::from(&f.path);
                let resolved = if p.is_absolute() {
                    p
                } else {
                    self.root.join(p)
                };
                resolved == target
            })
    }

    pub fn semantic_tokens(&self, _uri: &str) -> Vec<SemanticToken> {
        Vec::new()
    }

    fn symbol_at(
        &self,
        analysis: &Analysis,
        document: &Document,
        position: Position,
    ) -> Option<Symbol> {
        let (line, col) = document.to_line_col(position);
        let index = analysis.index.as_ref()?;
        for symbol in index.symbols() {
            if let Some(span) = symbol.span {
                if span_contains(span, line, col) {
                    return Some(symbol.clone());
                }
            }
            for reference in index.references(symbol.id) {
                if let Some(span) = reference.span {
                    if span_contains(span, line, col) {
                        return Some(symbol.clone());
                    }
                }
            }
        }
        None
    }

    fn source_location(
        &self,
        files: &[SourceFile],
        document: &Document,
        span: workshop_rs::source::Span,
    ) -> SourceLocation {
        let source = self.source_identity(files, document, span.file.index());
        let text = self.source_text(&source, document);
        SourceLocation {
            source,
            range: crate::document::span_to_range(&span, &text),
        }
    }

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

    fn source_text(&self, source: &str, document: &Document) -> String {
        if source == document.uri {
            return document.text.clone();
        }
        let path = PathBuf::from(source);
        self.store.text_for_path(&path).unwrap_or_default()
    }

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

    fn diagnostic_location(
        &self,
        files: &[SourceFile],
        document: &Document,
        span: Option<workshop_rs::source::Span>,
    ) -> (String, Range) {
        let Some(span) = span else {
            return (document.uri.clone(), empty_range());
        };
        let source = self.source_identity(files, document, span.file.index());
        let text = self.source_text(&source, document);
        (source, crate::document::span_to_range(&span, &text))
    }
}

fn is_source_document(uri: &str) -> bool {
    crate::document::uri_to_path(uri)
        .and_then(|p| p.extension().map(|e| e.to_string_lossy().to_lowercase()))
        .is_some_and(|ext| matches!(ext.as_str(), "opy" | "ostw" | "del"))
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

fn severity_name(severity: wright_analyzer::analysis::Severity) -> &'static str {
    match severity {
        wright_analyzer::analysis::Severity::Error => "error",
        wright_analyzer::analysis::Severity::Warning => "warning",
        wright_analyzer::analysis::Severity::Info => "info",
    }
}
