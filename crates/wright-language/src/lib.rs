//! Wright's editor-neutral language-service core (milestone M10, issue #63).
//!
//! This crate owns language intelligence **without any LSP types**: a
//! versioned [`Document`]/[`DocumentStore`] workspace model, editor-neutral
//! requests and results (positions, ranges, diagnostics, hover, definition,
//! references, completion, rename, semantic tokens), and a
//! [`service::LanguageService`] that composes the native `.opy` frontend, the
//! semantic index/analyzer, the safe-edit contract, and the workshop catalog.
//! The LSP adapter (`wright-lsp`) is a thin mapping over these types.

pub mod document;
pub mod service;

pub use document::{Document, DocumentStore, Position, Range};
pub use service::{
    CompletionItem, Diagnostic as LanguageDiagnostic, Hover, LanguageService, SemanticToken,
};
