//! Structured IR errors.
//!
//! All fallible IR operations (conversion, validation, lowering) report
//! [`IrError`] with a stable code and, when the offending source position is
//! known, a span. Human-readable wording is not part of the stable contract.

use crate::source::Span;

/// A structured IR error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrError {
    /// A node reference is dangling or out of range.
    DanglingReference { what: &'static str, id: u32 },
    /// A construct the IR does not support, with its source location.
    Unsupported { message: String, span: Option<Span> },
    /// A structural or invariant violation.
    Invalid {
        code: &'static str,
        message: String,
        span: Option<Span>,
    },
}

impl IrError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            IrError::DanglingReference { .. } => "dangling-reference",
            IrError::Unsupported { .. } => "unsupported",
            IrError::Invalid { code, .. } => code,
        }
    }

    /// Human-readable message.
    pub fn message(&self) -> String {
        match self {
            IrError::DanglingReference { what, id } => {
                format!("dangling {what} reference: id {id}")
            }
            IrError::Unsupported { message, .. } => message.clone(),
            IrError::Invalid { message, .. } => message.clone(),
        }
    }

    /// The offending source span, when known.
    pub fn span(&self) -> Option<Span> {
        match self {
            IrError::Unsupported { span, .. } => *span,
            IrError::Invalid { span, .. } => *span,
            IrError::DanglingReference { .. } => None,
        }
    }
}

impl std::fmt::Display for IrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for IrError {}
