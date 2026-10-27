use serde::{Deserialize, Serialize};
/// The severity of a diagnostic.
pub use wright_analyzer::analysis::Severity;
/// The exit class a diagnostic's producer assigns (#1003).
pub use wright_analyzer::service::ExitClass;
/// The origin of a loaded program.
pub use wright_analyzer::service::Origin;

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
    /// Source reconstruction (Workshop → OPY, with OSTW at a provider boundary).
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

/// Semantic support classification carried on a Workshop
/// completeness-residual diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Supported,
    Partial,
    Unsupported,
}

/// The class a `stage` belongs to by default: `internal` and `reconstruction`
/// are their own classes; every other stage is a source/user error unless the
/// creator overrides it.
pub(crate) fn class_for_stage(stage: Stage) -> ExitClass {
    match stage {
        Stage::Internal => ExitClass::Internal,
        Stage::Reconstruction => ExitClass::Unsupported,
        _ => ExitClass::SourceError,
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

/// One structured diagnostic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// A stable machine-readable code, e.g. `parse-error`.
    pub code: String,
    pub stage: Stage,
    pub severity: Severity,
    /// Human-readable message; wording is not part of the machine contract.
    pub message: String,
    /// The exit class this diagnostic belongs to; set where it is created.
    /// In-process only — it never serializes.
    #[serde(skip)]
    pub class: ExitClass,
    /// The residual's support classification, when this diagnostic reports a
    /// Workshop completeness residual.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<SourceSpan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Origin>,
    /// Refusal metadata surfaces reword or consume programmatically
    /// (#888): a `listOp`/`listAll` name-listing operation, `candidates`
    /// entries (`address`, `file`/`path`, `line`, `column`),
    /// `declared`/`declaredTotal` bounded name lists, and the queried
    /// `name`/`file`/`kind`. Serializes as sibling keys, which the
    /// diagnostic schema already permits.
    #[serde(default, flatten)]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

pub(crate) fn source_span(span: workshop_rs::source::Span, path: String) -> SourceSpan {
    SourceSpan {
        file: span.file.index(),
        path,
        start: Position {
            line: span.start.line,
            col: span.start.col,
        },
        end: Position {
            line: span.end.line,
            col: span.end.col,
        },
    }
}

impl Diagnostic {
    /// An error diagnostic in the given stage.
    ///
    /// `code` must obey the diagnostic-code grammar
    /// (`wright_analyzer::service::valid_code`); a violation is a bug, so it
    /// fails a debug assertion. The diagnostic's exit class defaults to the
    /// stage's class; use [`with_class`](Self::with_class) for a usage or
    /// unsupported error that a stage alone does not mark.
    pub fn error(code: impl Into<String>, stage: Stage, message: impl Into<String>) -> Diagnostic {
        Self::coded(code, stage, message, |code| {
            debug_assert!(
                wright_analyzer::service::valid_code(code),
                "diagnostic code {code:?} violates the code grammar"
            );
        })
    }

    /// An error diagnostic whose exit class differs from the stage's class
    /// (a usage or unsupported refusal reported under a source stage).
    pub fn classed_error(
        code: impl Into<String>,
        stage: Stage,
        class: ExitClass,
        message: impl Into<String>,
    ) -> Diagnostic {
        Self::error(code, stage, message).with_class(class)
    }

    /// An error diagnostic whose exit class is usage — the caller's
    /// argument was wrong (#1003).
    pub fn usage_error(
        code: impl Into<String>,
        stage: Stage,
        message: impl Into<String>,
    ) -> Diagnostic {
        Self::classed_error(code, stage, ExitClass::Usage, message)
    }

    /// Override the exit class set at creation.
    pub fn with_class(mut self, class: ExitClass) -> Diagnostic {
        self.class = class;
        self
    }

    /// A carried error diagnostic with an explicit exit class.
    pub fn carried_classed_error(
        code: impl Into<String>,
        stage: Stage,
        class: ExitClass,
        message: impl Into<String>,
    ) -> Diagnostic {
        Self::carried_error(code, stage, message).with_class(class)
    }

    /// A carried error diagnostic; the exit class defaults to the stage's
    /// class.
    pub(crate) fn carried_error(
        code: impl Into<String>,
        stage: Stage,
        message: impl Into<String>,
    ) -> Diagnostic {
        Self::coded(code, stage, message, |code| {
            debug_assert!(
                wright_analyzer::service::carried_code_ok(code),
                "carried diagnostic code {code:?} is empty or contains whitespace"
            );
        })
    }

    fn coded(
        code: impl Into<String>,
        stage: Stage,
        message: impl Into<String>,
        check: impl Fn(&str),
    ) -> Diagnostic {
        let code = code.into();
        check(&code);
        Diagnostic {
            code,
            stage,
            severity: Severity::Error,
            message: message.into(),
            class: class_for_stage(stage),
            status: None,
            span: None,
            source: None,
            fields: Default::default(),
        }
    }

    /// A warning diagnostic in the given stage.
    pub fn warning(
        code: impl Into<String>,
        stage: Stage,
        message: impl Into<String>,
    ) -> Diagnostic {
        let mut diagnostic = Self::error(code, stage, message);
        diagnostic.severity = Severity::Warning;
        diagnostic
    }
}

pub(crate) fn workshop_diag(
    error: workshop_rs::WorkshopError,
    resolved: &crate::input::ResolvedInput,
) -> Diagnostic {
    let to_span = |s: Option<workshop_rs::source::Span>| {
        s.map(|span| source_span(span, resolved.display.clone()))
    };
    let (code, stage, span) = match &error {
        workshop_rs::WorkshopError::Catalog(catalog) => {
            return Diagnostic::error(
                "catalog-error",
                Stage::Internal,
                format!("{}: {}", catalog.code, catalog.message),
            );
        }
        workshop_rs::WorkshopError::Unknown { kind, span, .. }
        | workshop_rs::WorkshopError::UnknownWithCandidates { kind, span, .. }
        | workshop_rs::WorkshopError::NotDetected { kind, span, .. } => {
            let code = unknown_owner_phrase_code(kind);
            (code, Stage::Frontend, to_span(*span))
        }
        workshop_rs::WorkshopError::Malformed { span, .. } => {
            ("parse-error".to_string(), Stage::Frontend, to_span(*span))
        }
        workshop_rs::WorkshopError::Unsupported { span, .. } => (
            "unsupported-construct".to_string(),
            Stage::Frontend,
            to_span(*span),
        ),
        workshop_rs::WorkshopError::MissingMapping { .. } => {
            ("missing-mapping".to_string(), Stage::Frontend, None)
        }
        _ => ("workshop-error".to_string(), Stage::Internal, None),
    };
    let mut diagnostic = Diagnostic::error(code, stage, error.to_string());
    diagnostic.span = span;
    diagnostic.source = Some(resolved.origin.clone());
    diagnostic
}

fn unknown_owner_phrase_code(kind: &str) -> String {
    format!("unknown-{}", kind.replace(' ', "-"))
}

pub(crate) fn source_provider_unavailable() -> Diagnostic {
    Diagnostic::error(
        "source-provider-unavailable",
        Stage::Internal,
        "the requested source-provider workflow is not currently shipped with Wright",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SourceKind;
    use crate::input::{InputTarget, ResolvedInput};
    use std::path::PathBuf;

    #[test]
    fn error_accepts_a_kebab_case_code() {
        let diagnostic = Diagnostic::error("unknown-enum-member", Stage::Frontend, "message");
        assert_eq!(diagnostic.code, "unknown-enum-member");
    }

    #[test]
    #[should_panic(expected = "violates the code grammar")]
    fn error_rejects_a_spaced_code() {
        let _ = Diagnostic::error("unknown-enum member", Stage::Frontend, "message");
    }

    #[test]
    #[should_panic(expected = "violates the code grammar")]
    fn error_rejects_an_uppercase_code() {
        let _ = Diagnostic::error("Unknown-Value", Stage::Frontend, "message");
    }

    #[test]
    fn carried_error_keeps_foreign_vocabulary() {
        let diagnostic = Diagnostic::carried_error(
            "provider-refusal-compile.tooLarge",
            Stage::Frontend,
            "message",
        );
        assert_eq!(diagnostic.code, "provider-refusal-compile.tooLarge");
    }

    #[test]
    #[should_panic(expected = "empty or contains whitespace")]
    fn carried_error_rejects_whitespace() {
        let _ = Diagnostic::carried_error("bad code", Stage::Frontend, "message");
    }

    #[test]
    fn workshop_diag_codes_match_the_grammar_for_every_owner_kind() {
        const OWNER_KINDS: &[&str] = &[
            "action",
            "enum domain",
            "enum member",
            "event",
            "event player",
            "event team",
            "global variable",
            "language",
            "locale",
            "localized string",
            "lookup scope",
            "modify operator",
            "player variable",
            "rule section",
            "setting",
            "setting boolean",
            "settings enum",
            "subroutine",
            "top-level section",
            "value",
        ];
        let resolved = ResolvedInput {
            kind: SourceKind::Workshop,
            text: String::new(),
            path: None,
            target: InputTarget::File,
            root: PathBuf::new(),
            cwd: PathBuf::new(),
            display: "<test>".to_string(),
            identity: String::new(),
            origin: Origin {
                kind: "workshop".to_string(),
                locale: Some("en-US".to_string()),
            },
        };
        let locale = workshop_rs::catalog::Locale::new("en-US");
        for kind in OWNER_KINDS {
            for error in [
                workshop_rs::WorkshopError::unknown(kind, "spelling", locale.clone(), None),
                workshop_rs::WorkshopError::not_detected(kind, "message", None),
                workshop_rs::WorkshopError::unknown(kind, "spelling", locale.clone(), None)
                    .with_candidates(vec!["candidate".to_string()]),
            ] {
                let diagnostic = workshop_diag(error, &resolved);
                assert!(
                    wright_analyzer::service::valid_code(&diagnostic.code),
                    "kind {kind:?} produced code {:?}",
                    diagnostic.code
                );
            }
        }
        let diagnostic = workshop_diag(
            workshop_rs::WorkshopError::unknown("enum member", "x", locale, None),
            &resolved,
        );
        assert_eq!(diagnostic.code, "unknown-enum-member");
    }
}
