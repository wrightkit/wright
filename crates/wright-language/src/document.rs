use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub use wright_driver::edit::{char_offset_to_utf16, utf16_offset_to_char};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone)]
pub struct Document {
    pub uri: String,
    pub text: String,
    pub version: i32,
    pub root: PathBuf,
    /// The editor-declared language id (`textDocument.languageId`), when the
    /// host reported one: a `workshop` id marks the buffer as raw Workshop
    /// regardless of its extension, and any other explicit id keeps an
    /// ambiguous extension (`.txt`) out of the Workshop path.
    pub language_id: Option<String>,
}

impl Document {
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
            language_id: None,
        }
    }

    pub fn new(uri: impl Into<String>, text: impl Into<String>, root: PathBuf) -> Document {
        Self::with_version(uri, text, root, 0)
    }

    pub fn with_language_id(mut self, language_id: impl Into<String>) -> Document {
        self.language_id = Some(language_id.into());
        self
    }

    pub fn to_line_col(&self, position: Position) -> (u32, u32) {
        let line = (position.line as usize).min(self.text.lines().count().max(1) - 1);
        let line_text = self.text.lines().nth(line).unwrap_or_default();
        let character = utf16_offset_to_char(line_text, position.character as usize);
        (line as u32 + 1, character as u32 + 1)
    }

    pub fn from_span(&self, span: &workshop_rs::source::Span) -> Range {
        span_to_range(span, &self.text)
    }
}

#[derive(Debug, Default)]
pub struct DocumentStore {
    documents: BTreeMap<String, Document>,
    pub root: PathBuf,
}

impl DocumentStore {
    pub fn new(root: PathBuf) -> DocumentStore {
        DocumentStore {
            documents: BTreeMap::new(),
            root,
        }
    }

    pub fn open(&mut self, document: Document) {
        self.documents.insert(document.uri.clone(), document);
    }

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

    pub fn close(&mut self, uri: &str) {
        self.documents.remove(uri);
    }

    pub fn document(&self, uri: &str) -> Option<&Document> {
        self.documents.get(uri)
    }

    pub fn uris(&self) -> impl Iterator<Item = &str> {
        self.documents.keys().map(String::as_str)
    }

    pub fn text_for_path(&self, path: &PathBuf) -> Option<String> {
        self.document_for_path(path)
            .map(|document| document.text.clone())
            .or_else(|| std::fs::read_to_string(path).ok())
    }

    pub fn uri_for_path(&self, path: &PathBuf) -> Option<String> {
        self.document_for_path(path)
            .map(|document| document.uri.clone())
    }

    fn document_for_path(&self, path: &PathBuf) -> Option<&Document> {
        self.documents.values().find(|document| {
            uri_to_path(&document.uri).is_some_and(|document_path| document_path == *path)
        })
    }

    pub fn overlay(&self, root: &PathBuf) -> BTreeMap<String, String> {
        let mut overlay = BTreeMap::new();
        for doc in self.documents.values() {
            let Some(path) = uri_to_path(&doc.uri) else {
                continue;
            };
            overlay.insert(path.to_string_lossy().into_owned(), doc.text.clone());
            if let Ok(rel) = path.strip_prefix(root) {
                overlay.insert(rel.to_string_lossy().into_owned(), doc.text.clone());
            }
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                overlay.insert(name.to_string(), doc.text.clone());
            }
        }
        overlay
    }
}

pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let url = url::Url::parse(uri).ok()?;
    if url.scheme() != "file" {
        return None;
    }
    url.to_file_path().ok()
}

pub fn path_to_uri(path: &Path) -> Option<String> {
    url::Url::from_file_path(path).ok().map(|u| u.to_string())
}

pub fn source_to_uri(source: &str) -> Option<String> {
    if let Ok(url) = url::Url::parse(source) {
        if url.scheme() == "file" {
            return Some(url.to_string());
        }
    }
    path_to_uri(Path::new(source))
}

pub fn utf16_len(s: &str) -> usize {
    s.chars().map(|c| c.len_utf16()).sum()
}

/// A 1-based line and 1-based character column as a UTF-16 `Position`; a line or column past the end clamps.
pub fn line_col_position(source: &str, line: u32, col: u32) -> Position {
    let index = line.saturating_sub(1) as usize;
    let character = source.lines().nth(index).map_or(0, |line| {
        char_offset_to_utf16(line, col.saturating_sub(1) as usize)
    });
    Position {
        line: index as u32,
        character: character as u32,
    }
}

pub fn span_to_range(span: &workshop_rs::source::Span, source: &str) -> Range {
    Range {
        start: line_col_position(source, span.start.line, span.start.col),
        end: line_col_position(source, span.end.line, span.end.col),
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
        let span = workshop_rs::source::Span::new(
            workshop_rs::source::FileId::from_index(0),
            workshop_rs::source::Position::new(1, 16),
            workshop_rs::source::Position::new(1, 21),
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
    fn source_to_uri_normalizes_uris_and_paths_consistently() {
        // A percent-encoded file URI round-trips through the same parser.
        let uri = source_to_uri("file:///tmp/my%20dir/%E6%96%87%E4%BB%B6.opy").unwrap();
        assert_eq!(
            uri_to_path(&uri),
            Some(PathBuf::from("/tmp/my dir/文件.opy")),
            "normalized URI decodes back to the intended path"
        );
        // A resolved filesystem path converts to a standard file URI.
        let uri = source_to_uri("/tmp/my dir/文件.opy").unwrap();
        assert!(uri.starts_with("file:///tmp/my%20dir/"), "{uri}");
        assert_eq!(
            uri_to_path(&uri),
            Some(PathBuf::from("/tmp/my dir/文件.opy")),
            "path -> URI -> path round-trips"
        );
        // Non-file identities are not filesystem sources.
        assert_eq!(source_to_uri("untitled:scratch"), None);
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
