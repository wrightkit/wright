use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, ProviderError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError {
    pub code: String,
    pub message: String,
}

impl ProviderError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ProviderError {}

pub trait LanguageProvider {
    fn check(&self, source: &str, path: &Path) -> Result<Vec<Diagnostic>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Supported,
    Partial,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub file: PathBuf,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub status: Status,
    pub span: SourceSpan,
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
