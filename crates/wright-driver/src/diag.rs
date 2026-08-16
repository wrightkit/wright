//! Structured diagnostics shared by every driver workflow.
//!
//! A [`Diagnostic`] is the machine-readable unit of compiler feedback: a
//! stable `code`, the pipeline `stage` that produced it, a `severity`, a
//! human message, an optional source span, and the input's origin metadata.
//! CLI human rendering and JSON serialization both derive from this one
//! model, so automation never needs to scrape terminal text.

use serde::{Deserialize, Serialize};

/// The severity of a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Blocks the workflow; the result is not usable.
    Error,
    /// Does not block the workflow, but should be reviewed.
    Warning,
    /// Informational.
    Info,
}

/// The pipeline stage that produced a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// Input/project discovery, reading, and kind resolution.
    Discovery,
    /// The source frontend (parse/preprocess).
    Frontend,
    /// HIR → WIR lowering.
    Lowering,
    /// WIR structural validation.
    Validation,
    /// Workshop emission.
    Emission,
    /// Semantic analysis.
    Analysis,
    /// Source reconstruction (Workshop → OPY/OSTW, #126).
    Reconstruction,
    /// Driver/CLI internal or environment failures.
    Internal,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Discovery => "discovery",
            Stage::Frontend => "frontend",
            Stage::Lowering => "lowering",
            Stage::Validation => "validation",
            Stage::Emission => "emission",
            Stage::Analysis => "analysis",
            Stage::Reconstruction => "reconstruction",
            Stage::Internal => "internal",
        }
    }
}

/// A 1-based line/column position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub col: u32,
}

/// A serializable source span with the resolved file path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    /// The file index in the program's file arena.
    pub file: usize,
    /// The resolved file path (stable, relative when under the cwd).
    pub path: String,
    pub start: Position,
    pub end: Position,
}

/// The origin of a loaded program (mirrors the semantic-service origin).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    /// `workshop`, `protocol`, or `opy` (adapter bridge).
    pub kind: String,
    /// The Workshop client locale for workshop-origin programs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

/// One structured diagnostic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// A stable machine-readable code, e.g. `parse-error`.
    pub code: String,
    pub stage: Stage,
    pub severity: Severity,
    /// Human-readable message; wording is not part of the machine contract.
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<SourceSpan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Origin>,
}

/// Build a source span from the IR source model, resolving the file path.
pub fn span_from_ir(
    span: Option<workshop_rs::source::Span>,
    files: &wright_ir::arena::Arena<workshop_rs::source::SourceFile>,
) -> Option<SourceSpan> {
    let span = span?;
    let file = span.file.index();
    let path = files
        .get(span.file)
        .map(|source_file| source_file.path.clone())
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

/// Convenience constructors for internal diagnostics.
impl Diagnostic {
    /// An error diagnostic in the given stage.
    pub fn error(code: impl Into<String>, stage: Stage, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            code: code.into(),
            stage,
            severity: Severity::Error,
            message: message.into(),
            span: None,
            source: None,
        }
    }

    /// A warning diagnostic in the given stage.
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
            span: None,
            source: None,
        }
    }
}
