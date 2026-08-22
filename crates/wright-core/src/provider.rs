//! Minimal in-process language-provider contract.
//!
//! This is deliberately smaller than the process-based LPP client. It lets a
//! Wright-owned adapter expose source checking without exposing Workshop IR,
//! transport, or protocol details.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The result type used by [`LanguageProvider::check`].
pub type Result<T> = std::result::Result<T, ProviderError>;

/// A provider-boundary failure that must remain visible to callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError {
    /// Stable machine-readable failure code.
    pub code: String,
    /// Human-readable failure detail.
    pub message: String,
}

impl ProviderError {
    /// Construct a provider-boundary failure.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ProviderError {}

/// A language implementation that can check one source document in process.
pub trait LanguageProvider {
    /// Check `source`, attributing diagnostics to `path`.
    fn check(&self, source: &str, path: &Path) -> Result<Vec<Diagnostic>>;
}

/// Diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// The source cannot be checked successfully.
    Error,
    /// The source is checkable but needs attention.
    Warning,
    /// Informational provider output.
    Info,
}

/// Semantic support status for the diagnosed construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// The construct is understood by the provider.
    Supported,
    /// The construct is understood only partially.
    Partial,
    /// The construct is not understood and must not be treated as supported.
    Unsupported,
}

/// A 1-based, half-open source span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    /// The source file containing the span.
    pub file: PathBuf,
    /// Inclusive start line and column.
    pub start_line: u32,
    pub start_col: u32,
    /// Exclusive end line and column.
    pub end_line: u32,
    pub end_col: u32,
}

/// One provider diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Stable machine-readable diagnostic code.
    pub code: String,
    pub severity: Severity,
    pub status: Status,
    pub span: SourceSpan,
    /// Human-readable diagnostic message.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_serialization_is_stable_and_distinct() {
        let diagnostic = Diagnostic {
            code: "workshop.unresolved-construct".into(),
            severity: Severity::Error,
            status: Status::Unsupported,
            span: SourceSpan {
                file: PathBuf::from("fixture.ow"),
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 5,
            },
            message: "construct is not supported".into(),
        };
        let value = serde_json::to_value(&diagnostic).expect("diagnostic serializes");
        assert_eq!(value["code"], "workshop.unresolved-construct");
        assert_eq!(value["severity"], "error");
        assert_eq!(value["status"], "unsupported");
        assert_eq!(value["span"]["file"], "fixture.ow");
        assert_eq!(value["message"], "construct is not supported");
        assert_ne!(value["code"], value["message"]);
        let round_trip: Diagnostic = serde_json::from_value(value).expect("diagnostic parses");
        assert_eq!(round_trip, diagnostic);
    }
}
