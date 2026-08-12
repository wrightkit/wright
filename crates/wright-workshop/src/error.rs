//! Structured errors for the Workshop language model.

use crate::catalog::Locale;

/// A structured Workshop-language error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkshopError {
    /// Catalog data is malformed or fails validation.
    Catalog(CatalogError),
    /// A localized spelling is unknown or ambiguous.
    Unknown {
        kind: &'static str,
        spelling: String,
        locale: Locale,
        span: Option<wright_ir::source::Span>,
    },
    /// The input is syntactically malformed.
    Malformed {
        message: String,
        span: Option<wright_ir::source::Span>,
    },
    /// A construct is recognized but outside the supported surface.
    Unsupported {
        message: String,
        span: Option<wright_ir::source::Span>,
    },
}

/// Catalog-specific error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogError {
    pub code: &'static str,
    pub message: String,
}

impl CatalogError {
    pub(crate) fn malformed(message: String) -> WorkshopError {
        WorkshopError::Catalog(CatalogError {
            code: "malformed-catalog",
            message,
        })
    }

    pub(crate) fn validation(message: String) -> WorkshopError {
        WorkshopError::Catalog(CatalogError {
            code: "invalid-catalog",
            message,
        })
    }
}

/// A crate-wide result alias.
pub(crate) type Result<T> = std::result::Result<T, WorkshopError>;

impl std::fmt::Display for WorkshopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkshopError::Catalog(error) => write!(f, "{}: {}", error.code, error.message),
            WorkshopError::Unknown {
                kind,
                spelling,
                locale,
                ..
            } => {
                write!(
                    f,
                    "unknown {kind} spelling '{spelling}' for locale '{locale}'"
                )
            }
            WorkshopError::Malformed { message, .. } => write!(f, "malformed: {message}"),
            WorkshopError::Unsupported { message, .. } => write!(f, "unsupported: {message}"),
        }
    }
}
