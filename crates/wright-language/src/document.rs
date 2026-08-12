//! The document/workspace model (#63).
//!
//! A [`Document`] is one open source file with a stable URI, its current
//! text, and a monotonically increasing version assigned by the host on every
//! change. A [`DocumentStore`] owns the workspace's open documents and the
//! project root. Positions and ranges are editor-neutral 0-based line/column
//! pairs (the LSP convention), converted at the service boundary to the
//! compiler's 1-based spans. Results carry the document version they were
//! computed for, so stale results are detectable and replaceable (#64).

use std::collections::BTreeMap;
use std::path::PathBuf;

/// A 0-based line/character position (editor convention).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

/// A 0-based half-open range (editor convention).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

/// One open document.
#[derive(Debug, Clone)]
pub struct Document {
    /// A stable URI/path identity (e.g. `file:///project/main.opy`).
    pub uri: String,
    /// The current source text.
    pub text: String,
    /// The host-assigned version (monotonic across edits).
    pub version: i64,
    /// The project root used for include resolution.
    pub root: PathBuf,
}

impl Document {
    /// Create a document at version 0.
    pub fn new(uri: impl Into<String>, text: impl Into<String>, root: PathBuf) -> Document {
        Document {
            uri: uri.into(),
            text: text.into(),
            version: 0,
            root,
        }
    }

    /// The 1-based line/column of a 0-based editor position (clamped).
    pub fn to_line_col(&self, position: Position) -> (u32, u32) {
        let line = (position.line as usize).min(self.text.lines().count().max(1) - 1);
        let line_text = self.text.lines().nth(line).unwrap_or_default();
        let character = (position.character as usize).min(line_text.chars().count());
        (line as u32 + 1, character as u32 + 1)
    }

    /// Convert a 1-based compiler span into a 0-based editor range.
    pub fn from_span(&self, span: &wright_ir::source::Span) -> Range {
        let start = Position {
            line: span.start.line.saturating_sub(1),
            character: span.start.col.saturating_sub(1),
        };
        let end = Position {
            line: span.end.line.saturating_sub(1),
            character: span.end.col.saturating_sub(1),
        };
        Range { start, end }
    }
}

/// The workspace's open documents, keyed by URI.
#[derive(Debug, Default)]
pub struct DocumentStore {
    documents: BTreeMap<String, Document>,
    pub root: PathBuf,
}

impl DocumentStore {
    /// A store with the given project root.
    pub fn new(root: PathBuf) -> DocumentStore {
        DocumentStore {
            documents: BTreeMap::new(),
            root,
        }
    }

    /// Open (or replace) a document at a fresh version.
    pub fn open(&mut self, document: Document) {
        self.documents.insert(document.uri.clone(), document);
    }

    /// Apply a full-document change, bumping the version.
    pub fn change(&mut self, uri: &str, new_text: &str) -> Option<i64> {
        let document = self.documents.get_mut(uri)?;
        document.text = new_text.to_string();
        document.version += 1;
        Some(document.version)
    }

    /// Close a document.
    pub fn close(&mut self, uri: &str) {
        self.documents.remove(uri);
    }

    /// The current document for a URI.
    pub fn document(&self, uri: &str) -> Option<&Document> {
        self.documents.get(uri)
    }

    /// Every open document URI.
    pub fn uris(&self) -> impl Iterator<Item = &str> {
        self.documents.keys().map(String::as_str)
    }
}
