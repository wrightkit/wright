//! Frontend diagnostics: structured, source-located failures.
//!
//! Every frontend failure is a [`FrontendError`] with a stable `code`, a
//! human message, and an optional source span. Spans use the shared
//! `workshop_rs::source` registry types, so the driver maps them into the
//! `wright-result/v1` diagnostic contract with the same provenance rules as
//! the other frontends; wording is not part of the machine contract.

use workshop_rs::source::Span;

/// A structured frontend error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontendError {
    /// A stable machine-readable code, e.g. `ostw-parse-error`.
    pub code: String,
    /// Human-readable message (not part of the machine contract).
    pub message: String,
    /// The offending source region, when known.
    pub span: Option<Span>,
}

/// A crate-wide result alias.
pub type FrontendResult<T> = Result<T, FrontendError>;

impl FrontendError {
    /// An error without a source span.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> FrontendError {
        FrontendError {
            code: code.into(),
            message: message.into(),
            span: None,
        }
    }

    /// An error at a source position.
    pub fn at(code: impl Into<String>, message: impl Into<String>, span: Span) -> FrontendError {
        FrontendError {
            code: code.into(),
            message: message.into(),
            span: Some(span),
        }
    }
}

impl std::fmt::Display for FrontendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for FrontendError {}
