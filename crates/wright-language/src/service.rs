//! The editor-neutral language service (#63, #65, #66).

use std::path::PathBuf;

use serde::Serialize;

use crate::document::{DocumentStore, Position, Range};

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

    pub fn diagnostics(&self, uri: &str) -> Vec<SourceDiagnostic> {
        let Some(document) = self.store.document(uri) else {
            return Vec::new();
        };
        if !is_source_document(uri) {
            return Vec::new();
        }
        vec![SourceDiagnostic {
            source: document.uri.clone(),
            range: empty_range(),
            severity: "error".to_string(),
            code: "source-provider-unavailable".to_string(),
            message: "source-language analysis is provider-owned and no editor capability is currently negotiated".to_string(),
            source_version: document.version,
            document_version: document.version,
        }]
    }

    pub fn dependent_documents(&self, uri: &str) -> Vec<String> {
        if self.store.document(uri).is_some() {
            vec![uri.to_string()]
        } else {
            Vec::new()
        }
    }
}

fn is_source_document(uri: &str) -> bool {
    crate::document::uri_to_path(uri)
        .and_then(|p| p.extension().map(|e| e.to_string_lossy().to_lowercase()))
        .is_some_and(|ext| matches!(ext.as_str(), "opy" | "ostw" | "del"))
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
