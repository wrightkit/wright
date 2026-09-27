//! The editor-neutral language service (#63, #65, #66).

use std::path::PathBuf;

use serde::Serialize;
use workshop_rs::Program;
use wright_analyzer::analysis::Finding;
use wright_analyzer::canonical::SemanticIndex;

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
        self.analyze(document)
            .parse_errors
            .into_iter()
            .map(|error| SourceDiagnostic {
                source: document.uri.clone(),
                range: empty_range(),
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

    pub fn hover(&self, _uri: &str, _position: Position) -> Option<Hover> {
        None
    }

    pub fn definition(&self, _uri: &str, _position: Position) -> Option<SourceLocation> {
        None
    }

    pub fn references(&self, _uri: &str, _position: Position) -> Vec<SourceLocation> {
        Vec::new()
    }

    pub fn completion(&self, _uri: &str, _position: Position) -> Vec<CompletionItem> {
        Vec::new()
    }

    pub fn rename(&self, uri: &str, _position: Position, new_name: &str) -> RenameResult {
        if new_name.is_empty() {
            return refused_rename(0, "rename-invalid-name: the new name must not be empty");
        }
        let Some(requesting) = self.store.document(uri) else {
            return refused_rename(
                0,
                format!(
                    "rename-unresolved: no open document for '{uri}'; the source identity cannot be established"
                ),
            );
        };
        if is_source_document(uri) {
            return refused_rename(
                requesting.version,
                "source-provider-unavailable: OPY language-service capabilities are not currently shipped with the configured provider",
            );
        }

        refused_rename(
            requesting.version,
            "rename-unresolved: no symbol is resolvable at the requested position",
        )
    }
    pub fn semantic_tokens(&self, _uri: &str) -> Vec<SemanticToken> {
        Vec::new()
    }
}

fn refused_rename(document_version: i32, diagnostic: impl Into<String>) -> RenameResult {
    RenameResult {
        document_version,
        ok: false,
        edits: Vec::new(),
        previews: Vec::new(),
        diagnostics: vec![diagnostic.into()],
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
