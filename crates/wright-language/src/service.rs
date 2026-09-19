//! The editor-neutral language service.
//!
//! Language-service capabilities are provider-owned; Wright returns structured
//! refusals or empty responses when no editor capability is negotiated.

use std::path::PathBuf;

use serde::Serialize;
use workshop_rs::Program;
use wright_analyzer::analysis::Finding;
use wright_analyzer::canonical::SemanticIndex;

use crate::document::{Document, DocumentStore, Position, Range};

/// A source-aware language diagnostic (editor-neutral).
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
    pub source: String,
    pub range: Range,
}

/// One source-aware rename edit targeting an exact semantic occurrence.
#[derive(Debug, Clone, Serialize)]
pub struct RenameEdit {
    pub source: String,
    pub range: Range,
    pub new_text: String,
    pub source_identity: String,
    pub source_version: i32,
}

/// The result of a project-wide rename request.
#[derive(Debug, Clone, Serialize)]
pub struct RenameResult {
    pub document_version: i32,
    pub ok: bool,
    pub edits: Vec<RenameEdit>,
    pub previews: Vec<wright_driver::edit::SourcePreview>,
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
    pub files: Vec<SourceFile>,
}

/// The editor-neutral language service over a workspace.
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

    pub fn analyze(&self, document: &Document) -> Analysis {
        let _ = document;
        unavailable_source_analysis()
    }

    pub fn diagnostics(&self, uri: &str) -> Vec<SourceDiagnostic> {
        let Some(document) = self.store.document(uri) else {
            return Vec::new();
        };
        let analysis = self.analyze(document);
        analysis
            .parse_errors
            .into_iter()
            .map(|error| SourceDiagnostic {
                source: document.uri.clone(),
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 0,
                    },
                },
                severity: "error".to_string(),
                code: error.code,
                message: error.message,
                source_version: document.version,
                document_version: document.version,
            })
            .collect()
    }

    pub fn dependent_documents(&self, uri: &str) -> Vec<String> {
        if self.store.document(uri).is_some() {
            vec![uri.to_string()]
        } else {
            Vec::new()
        }
    }

    pub fn hover(&self, uri: &str, position: Position) -> Option<Hover> {
        let _ = (uri, position);
        None
    }

    pub fn definition(&self, uri: &str, position: Position) -> Option<SourceLocation> {
        let _ = (uri, position);
        None
    }

    pub fn references(&self, uri: &str, position: Position) -> Vec<SourceLocation> {
        let _ = (uri, position);
        Vec::new()
    }

    pub fn completion(&self, uri: &str, position: Position) -> Vec<CompletionItem> {
        let _ = (uri, position);
        Vec::new()
    }

    pub fn rename(&self, uri: &str, position: Position, new_name: &str) -> RenameResult {
        let _ = position;
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
        RenameResult {
            document_version: requesting.version,
            ok: false,
            edits: Vec::new(),
            previews: Vec::new(),
            diagnostics: vec![
                "rename-unresolved: no symbol is resolvable at the requested position".to_string(),
            ],
        }
    }

    pub fn semantic_tokens(&self, uri: &str) -> Vec<SemanticToken> {
        let _ = uri;
        Vec::new()
    }
}

fn is_source_document(uri: &str) -> bool {
    crate::document::uri_to_path(uri)
        .and_then(|path| {
            path.extension()
                .map(|ext| ext.to_string_lossy().to_lowercase())
        })
        .is_some_and(|ext| matches!(ext.as_str(), "opy" | "ostw" | "del"))
}

fn unavailable_source_analysis() -> Analysis {
    Analysis {
        program: Program::default(),
        index: None,
        findings: Vec::new(),
        parse_errors: vec![SourceError {
            code: "source-provider-unavailable".to_string(),
            message:
                "source-language analysis is provider-owned and no editor capability is currently negotiated"
                    .to_string(),
            span: None,
        }],
        files: Vec::new(),
    }
}
