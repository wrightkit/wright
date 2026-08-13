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
use std::path::{Path, PathBuf};

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
    ///
    /// The editor `character` is a UTF-16 code-unit offset (the LSP
    /// convention); the compiler column is a Unicode scalar-value column.
    pub fn to_line_col(&self, position: Position) -> (u32, u32) {
        let line = (position.line as usize).min(self.text.lines().count().max(1) - 1);
        let line_text = self.text.lines().nth(line).unwrap_or_default();
        let character = utf16_offset_to_char(line_text, position.character as usize);
        (line as u32 + 1, character as u32 + 1)
    }

    /// Convert a 1-based compiler span into a 0-based editor range, where
    /// the editor character is a UTF-16 code-unit offset.
    pub fn from_span(&self, span: &wright_ir::source::Span) -> Range {
        span_to_range(span, &self.text)
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

    /// The text of a filesystem path, preferring an open unsaved document
    /// overlay and falling back to the filesystem.
    pub fn text_for_path(&self, path: &PathBuf) -> Option<String> {
        for document in self.documents.values() {
            if let Some(document_path) = uri_to_path(&document.uri) {
                if document_path == *path {
                    return Some(document.text.clone());
                }
            }
        }
        std::fs::read_to_string(path).ok()
    }

    /// The open document URI for a filesystem path, when one is open.
    pub fn uri_for_path(&self, path: &PathBuf) -> Option<String> {
        for document in self.documents.values() {
            if let Some(document_path) = uri_to_path(&document.uri) {
                if document_path == *path {
                    return Some(document.uri.clone());
                }
            }
        }
        None
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

/// Convert a standard `file://` URI to a filesystem path, when applicable.
///
/// Handles percent-encoding, spaces, Unicode filenames, and platform-specific
/// path behavior through the standard URL parser, so an open document's URI
/// identity maps to the same filesystem path the include resolver produces.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let url = url::Url::parse(uri).ok()?;
    if url.scheme() != "file" {
        return None;
    }
    url.to_file_path().ok()
}

/// Convert a filesystem path to a standard `file://` URI string, when
/// applicable. The reverse of [`uri_to_path`].
pub fn path_to_uri(path: &Path) -> Option<String> {
    url::Url::from_file_path(path)
        .ok()
        .map(|url| url.to_string())
}

/// The UTF-16 code-unit length of a string (non-BMP chars count 2).
pub fn utf16_len(s: &str) -> usize {
    s.chars().map(|c| c.len_utf16()).sum()
}

/// Convert a 0-based UTF-16 code-unit offset to a 0-based character offset
/// (clamped to the line's character count).
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

/// Convert a 0-based character offset to a 0-based UTF-16 code-unit offset
/// (clamped to the line's UTF-16 length).
pub fn char_offset_to_utf16(line: &str, char_offset: usize) -> usize {
    line.chars().take(char_offset).map(|c| c.len_utf16()).sum()
}

/// Convert a 1-based compiler span to a 0-based editor range with UTF-16
/// character offsets, using the source text to resolve each line's length.
pub fn span_to_range(span: &wright_ir::source::Span, source_text: &str) -> Range {
    let start_line = source_text
        .lines()
        .nth(span.start.line.saturating_sub(1) as usize)
        .unwrap_or_default();
    let end_line = source_text
        .lines()
        .nth(span.end.line.saturating_sub(1) as usize)
        .unwrap_or_default();
    Range {
        start: Position {
            line: span.start.line.saturating_sub(1),
            character: char_offset_to_utf16(start_line, span.start.col.saturating_sub(1) as usize)
                as u32,
        },
        end: Position {
            line: span.end.line.saturating_sub(1),
            character: char_offset_to_utf16(end_line, span.end.col.saturating_sub(1) as usize)
                as u32,
        },
    }
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

    #[test]
    fn utf16_offsets_account_for_non_bmp_characters() {
        // 🎯 is U+1F3AF: one Rust char, two UTF-16 code units.
        let line = "🎯 score";
        assert_eq!(line.chars().count(), 7);
        assert_eq!(utf16_len(line), 8);
        assert_eq!(utf16_offset_to_char(line, 0), 0);
        assert_eq!(
            utf16_offset_to_char(line, 1),
            1,
            "an offset inside a surrogate pair rounds up to the next char boundary"
        );
        assert_eq!(utf16_offset_to_char(line, 2), 1);
        assert_eq!(char_offset_to_utf16(line, 0), 0);
        assert_eq!(
            char_offset_to_utf16(line, 1),
            2,
            "one char becomes two UTF-16 units"
        );
    }

    #[test]
    fn uri_to_path_decodes_percent_encoding_spaces_and_unicode() {
        // Spaces and percent-encoded segments resolve to real paths.
        assert_eq!(
            uri_to_path("file:///tmp/my%20dir/main.opy"),
            Some(PathBuf::from("/tmp/my dir/main.opy"))
        );
        // Unicode filenames decode from percent-encoded URIs.
        assert_eq!(
            uri_to_path("file:///tmp/%E6%96%87%E4%BB%B6.opy"),
            Some(PathBuf::from("/tmp/文件.opy"))
        );
        // Non-file schemes are not filesystem paths.
        assert_eq!(uri_to_path("untitled:scratch"), None);
        assert_eq!(uri_to_path("https://example.com/a.opy"), None);
    }

    #[test]
    fn span_to_range_uses_utf16_units_on_non_bmp_lines() {
        // 🎯 is one Rust char but two UTF-16 units, so `score` (1-based
        // column 16) starts at UTF-16 offset 16 and ends at 21.
        let source = "    debug(\"🎯\", score)\n";
        let span = wright_ir::source::Span::new(
            wright_ir::ids::Id::from_index(0),
            wright_ir::source::Position::new(1, 16),
            wright_ir::source::Position::new(1, 21),
        );
        let range = span_to_range(&span, source);
        assert_eq!(range.start.line, 0);
        assert_eq!(
            range.start.character, 16,
            "score starts at UTF-16 offset 16"
        );
        assert_eq!(range.end.character, 21, "score ends at UTF-16 offset 21");
    }

    #[test]
    fn path_to_uri_round_trips_spaces_and_unicode() {
        let encoded = path_to_uri(Path::new("/tmp/my dir/文件.opy")).unwrap();
        assert!(encoded.starts_with("file:///tmp/my%20dir/"), "{encoded}");
        assert!(encoded.contains("%E6%96%87%E4%BB%B6"), "{encoded}");
        assert_eq!(
            uri_to_path(&encoded),
            Some(PathBuf::from("/tmp/my dir/文件.opy")),
            "path -> URI -> path round-trips"
        );
    }

    #[test]
    fn windows_drive_paths_use_standard_file_uris() {
        // A drive-style file URI always decodes through the standard parser;
        // on non-Windows the authority-style path is preserved literally.
        let decoded = uri_to_path("file:///C:/work/main.opy").expect("file URI decodes");
        assert!(
            decoded.to_string_lossy().ends_with("C:/work/main.opy")
                || decoded.to_string_lossy().ends_with("C:\\work\\main.opy"),
            "{decoded:?}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_round_trip_through_file_uris() {
        let encoded = path_to_uri(Path::new(r"C:\work\main.opy")).unwrap();
        assert_eq!(encoded, "file:///C:/work/main.opy", "{encoded}");
        assert_eq!(
            uri_to_path(&encoded),
            Some(PathBuf::from(r"C:\work\main.opy")),
            "path -> URI -> path round-trips"
        );
    }
}
