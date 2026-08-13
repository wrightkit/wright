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
    pub version: i32,
    /// The project root used for include resolution.
    pub root: PathBuf,
}

impl Document {
    /// Create a document with an explicit client version.
    pub fn with_version(
        uri: impl Into<String>,
        text: impl Into<String>,
        root: PathBuf,
        version: i32,
    ) -> Document {
        Document {
            uri: uri.into(),
            text: text.into(),
            version,
            root,
        }
    }

    /// Create a document at version 0 (used when the host does not supply a
    /// version, e.g. in-process consumers).
    pub fn new(uri: impl Into<String>, text: impl Into<String>, root: PathBuf) -> Document {
        Self::with_version(uri, text, root, 0)
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

    /// Open (or replace) a document.
    pub fn open(&mut self, document: Document) {
        self.documents.insert(document.uri.clone(), document);
    }

    /// Apply a full-document change with the client's document version.
    ///
    /// Out-of-order or stale versions (a version less than or equal to the
    /// currently stored version) are rejected: they cannot overwrite newer
    /// semantic state. Returns `true` when the change was applied.
    pub fn change(&mut self, uri: &str, new_text: &str, version: i32) -> bool {
        let Some(document) = self.documents.get_mut(uri) else {
            return false;
        };
        if version <= document.version {
            return false;
        }
        document.text = new_text.to_string();
        document.version = version;
        true
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

    /// Build an overlay map for include resolution: open documents keyed by
    /// their include-relative path and their absolute filesystem path, so
    /// unsaved editor buffers participate in include resolution rather than
    /// being silently ignored.
    pub fn overlay(&self, root: &PathBuf) -> BTreeMap<String, String> {
        let mut overlay = BTreeMap::new();
        for document in self.documents.values() {
            // Only overlay file-backed documents (skip synthetic/in-memory
            // URIs without a filesystem path).
            let Some(path) = uri_to_path(&document.uri) else {
                continue;
            };
            overlay.insert(path.to_string_lossy().into_owned(), document.text.clone());
            if let Ok(relative) = path.strip_prefix(root) {
                overlay.insert(
                    relative.to_string_lossy().into_owned(),
                    document.text.clone(),
                );
            }
            if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                overlay.insert(name.to_string(), document.text.clone());
            }
        }
        overlay
    }
}

/// Convert a `file://` URI to a filesystem path, when applicable.
fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let path = uri.strip_prefix("file://")?;
    Some(PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_versions_are_preserved_on_open_and_change() {
        let mut store = DocumentStore::new(PathBuf::from("/project"));
        store.open(Document::with_version(
            "file:///a.opy",
            "rule \"a\":\n",
            PathBuf::from("/project"),
            3,
        ));
        assert_eq!(store.document("file:///a.opy").unwrap().version, 3);
        assert!(
            store.change("file:///a.opy", "rule \"a\":\n    @Event global\n", 4),
            "newer client version applies"
        );
        assert_eq!(store.document("file:///a.opy").unwrap().version, 4);
    }

    #[test]
    fn stale_or_out_of_order_versions_cannot_overwrite_newer_state() {
        let mut store = DocumentStore::new(PathBuf::from("/project"));
        store.open(Document::with_version(
            "file:///a.opy",
            "rule \"a\":\n",
            PathBuf::from("/project"),
            5,
        ));
        // Equal and older versions are rejected.
        assert!(!store.change("file:///a.opy", "stale equal", 5));
        assert!(!store.change("file:///a.opy", "stale older", 4));
        assert_eq!(
            store.document("file:///a.opy").unwrap().text,
            "rule \"a\":\n"
        );
        // A newer version applies.
        assert!(store.change("file:///a.opy", "newer", 6));
        assert_eq!(store.document("file:///a.opy").unwrap().text, "newer");
        assert_eq!(store.document("file:///a.opy").unwrap().version, 6);
    }
}
