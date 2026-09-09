use workshop_rs::source::Span;

pub use workshop_rs::wir::error::IrError;

/// Shorthand for an unsupported-construct error.
pub(crate) fn unsupported(message: impl Into<String>, span: Option<Span>) -> IrError {
    IrError::Unsupported {
        message: message.into(),
        span,
    }
}
