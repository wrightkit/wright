#![allow(clippy::result_large_err)]

pub mod document;
pub mod service;

pub use document::{Document, DocumentStore, Position, Range};
pub use service::{
    HoverInfo, LanguageService, OpyProviderSupport, RenameOutcome, SourceDiagnostic,
    SourceLocation, SourceSymbol, SourceTextEdit,
};
pub use wright_driver::{OpyProviderConfig, SessionConfig};
/// The diagnostic-severity vocabulary `SourceDiagnostic` carries — the
/// provider contract's enum, re-exported so consumers need no direct
/// `wright-lpp` dependency (#1001).
pub use wright_lpp::DiagnosticSeverity;
