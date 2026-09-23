use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Discovery,
    Frontend,
    Lowering,
    Validation,
    Emission,
    Analysis,
    Reconstruction,
    Internal,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::Frontend => "frontend",
            Self::Lowering => "lowering",
            Self::Validation => "validation",
            Self::Emission => "emission",
            Self::Analysis => "analysis",
            Self::Reconstruction => "reconstruction",
            Self::Internal => "internal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub file: usize,
    pub path: String,
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub stage: Stage,
    pub severity: Severity,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<crate::provider::Status>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<SourceSpan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Origin>,
}

pub fn span_from_ir(
    span: Option<workshop_rs::source::Span>,
    files: &workshop_rs::arena::Arena<workshop_rs::source::SourceFile>,
) -> Option<SourceSpan> {
    let span = span?;
    let file = span.file.index();
    let path = files
        .get(span.file)
        .map(|f| f.path.clone())
        .unwrap_or_else(|| format!("<file {file}>"));
    Some(SourceSpan {
        file,
        path,
        start: Position {
            line: span.start.line,
            col: span.start.col,
        },
        end: Position {
            line: span.end.line,
            col: span.end.col,
        },
    })
}

impl Diagnostic {
    pub fn error(code: impl Into<String>, stage: Stage, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            code: code.into(),
            stage,
            severity: Severity::Error,
            message: message.into(),
            status: None,
            span: None,
            source: None,
        }
    }

    pub fn warning(
        code: impl Into<String>,
        stage: Stage,
        message: impl Into<String>,
    ) -> Diagnostic {
        Diagnostic {
            code: code.into(),
            stage,
            severity: Severity::Warning,
            message: message.into(),
            status: None,
            span: None,
            source: None,
        }
    }
}
