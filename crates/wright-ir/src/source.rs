//! Source model: files, positions, and spans.
//!
//! The conventions match the `wright/opy-hir` bridge protocol
//! ([`docs/hir/opy-hir-v1.md`](../../../docs/hir/opy-hir-v1.md)): positions
//! are 1-based, and a span is a half-open interval (`end` is exclusive).
//! Spans carry a typed [`FileId`] instead of a raw file index.

use crate::ids::Id;

/// A typed ID referencing a [`SourceFile`] in the program's file arena.
pub type FileId = Id<SourceFile>;

/// One source file in the program's file registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// The file name as the frontend reported it (for diagnostics).
    pub path: String,
}

impl SourceFile {
    /// Create a file entry with the given path.
    pub fn new(path: impl Into<String>) -> Self {
        SourceFile { path: path.into() }
    }
}

/// A 1-based line/column position in a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub line: u32,
    pub col: u32,
}

impl Position {
    /// A position at line `line`, column `col` (both 1-based).
    pub const fn new(line: u32, col: u32) -> Self {
        Position { line, col }
    }

    /// Whether this position is valid (1-based).
    pub const fn is_valid(self) -> bool {
        self.line >= 1 && self.col >= 1
    }
}

/// A half-open, 1-based source interval in one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub file: FileId,
    pub start: Position,
    pub end: Position,
}

impl Span {
    /// Create a span in `file` from `start` (inclusive) to `end` (exclusive).
    pub const fn new(file: FileId, start: Position, end: Position) -> Self {
        Span { file, start, end }
    }

    /// Whether the span is structurally valid: both positions are 1-based and
    /// `end` is not before `start`.
    pub const fn is_valid(self) -> bool {
        self.start.is_valid()
            && self.end.is_valid()
            && (self.end.line > self.start.line
                || (self.end.line == self.start.line && self.end.col >= self.start.col))
    }
}

#[cfg(test)]
mod tests {
    use super::{Position, SourceFile, Span};
    use crate::ids::Id;

    #[test]
    fn positions_are_one_based_and_validated() {
        assert!(Position::new(1, 1).is_valid());
        assert!(Position::new(10, 24).is_valid());
        assert!(!Position::new(0, 1).is_valid());
        assert!(!Position::new(1, 0).is_valid());
    }

    #[test]
    fn spans_require_end_not_before_start() {
        let file = Id::from_index(0);
        assert!(Span::new(file, Position::new(1, 1), Position::new(1, 5)).is_valid());
        assert!(Span::new(file, Position::new(1, 1), Position::new(2, 1)).is_valid());
        assert!(Span::new(file, Position::new(1, 1), Position::new(1, 1)).is_valid());
        assert!(!Span::new(file, Position::new(1, 5), Position::new(1, 1)).is_valid());
        assert!(!Span::new(file, Position::new(2, 1), Position::new(1, 1)).is_valid());
    }

    #[test]
    fn source_files_carry_paths() {
        let file = SourceFile::new("source.opy");
        assert_eq!(file.path, "source.opy");
    }
}
