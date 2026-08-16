//! Structured IR errors.
//!
//! [`IrError`] is the Workshop IR error contract, owned by `workshop-rs`
//! (`workshop_rs::wir::error::IrError`) and re-exported here so the Opy
//! conversion, validation, and lowering paths keep one error type (cutover
//! shim, wright#143). No independent implementation lives here. Removal
//! path: when the HIR and lowering extract to `opy-rs`, this shim disappears
//! and importers use `workshop_rs::wir::error` directly.

use workshop_rs::source::Span;

pub use workshop_rs::wir::error::IrError;

/// Shorthand for an unsupported-construct error.
pub(crate) fn unsupported(message: impl Into<String>, span: Option<Span>) -> IrError {
    IrError::Unsupported {
        message: message.into(),
        span,
    }
}
