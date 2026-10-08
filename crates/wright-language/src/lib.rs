// `wright_driver::Diagnostic` is the error contract the session APIs hand
// through; boxing it here would change nothing observable.
#![allow(clippy::result_large_err)]

pub mod document;
pub mod service;

pub use document::{Document, DocumentStore, Position, Range};
pub use service::{
    HoverInfo, LanguageService, RenameOutcome, SourceDiagnostic, SourceLocation, SourceTextEdit,
};
pub use wright_driver::{OpyProviderConfig, SessionConfig};
