pub mod document;
pub mod service;

pub use document::{Document, DocumentStore, Position, Range};
pub use service::{LanguageService, SourceDiagnostic};
