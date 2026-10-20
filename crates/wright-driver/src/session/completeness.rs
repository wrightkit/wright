//! The completeness-residual projection: `SemanticIssue` values the owner
//! engine reports become `workshop.<kind>.<name>` session diagnostics.

use std::path::Path;

use crate::diag::{Diagnostic, Origin, Position, Severity, SourceSpan, Stage, Status};

/// One completeness residual as a session diagnostic: the owner-classified
/// construct maps onto the `status` output contract, and the owner's spelling
/// suggestion survives as a did-you-mean.
pub(crate) fn map_issue(
    issue: &workshop_rs::rules::SemanticIssue,
    path: &Path,
    origin: &Origin,
) -> Diagnostic {
    let (kind_code, severity) = match issue.kind {
        workshop_rs::rules::IncompletenessKind::RawSetting => ("raw-setting", Severity::Warning),
        workshop_rs::rules::IncompletenessKind::UnknownAction => {
            ("unknown-action", Severity::Error)
        }
        workshop_rs::rules::IncompletenessKind::UnknownValue => ("unknown-value", Severity::Error),
        workshop_rs::rules::IncompletenessKind::OpaqueAction => ("opaque-action", Severity::Error),
    };
    let status = status_for_classification(issue.classification);
    let mut message = format!(
        "Workshop construct '{}' is {} ({})",
        issue.name,
        status_name(status),
        issue.classification.as_str()
    );
    if let Some(spelling) = &issue.suggestion {
        message.push_str(&format!(" (did you mean '{spelling}'?)"));
    }
    Diagnostic {
        code: diagnostic_code(kind_code, &issue.name),
        stage: Stage::Analysis,
        severity,
        message,
        status: Some(status),
        span: Some(issue_span(issue.span, path)),
        source: Some(origin.clone()),
    }
}

pub(crate) fn status_for_classification(
    classification: workshop_rs::rules::ResidualClassification,
) -> Status {
    match classification {
        workshop_rs::rules::ResidualClassification::ProjectDefinedConstruct
        | workshop_rs::rules::ResidualClassification::SourceDeclaredVariable => Status::Partial,
        workshop_rs::rules::ResidualClassification::ProducerExtension
        | workshop_rs::rules::ResidualClassification::LegacyOpaque
        | workshop_rs::rules::ResidualClassification::UnresolvedIdentifier
        | workshop_rs::rules::ResidualClassification::CatalogSpellingNearMiss => {
            Status::Unsupported
        }
    }
}

fn diagnostic_code(kind: &str, identity: &str) -> String {
    format!(
        "workshop.{kind}.{}",
        identity
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect::<String>()
    )
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Supported => "supported",
        Status::Partial => "partially supported",
        Status::Unsupported => "unsupported",
    }
}

fn issue_span(span: Option<workshop_rs::source::Span>, path: &Path) -> SourceSpan {
    let (start_line, start_col, end_line, end_col) = span
        .map(|span| (span.start.line, span.start.col, span.end.line, span.end.col))
        .unwrap_or((1, 1, 1, 1));
    SourceSpan {
        file: 0,
        path: path.display().to_string(),
        start: Position {
            line: start_line,
            col: start_col,
        },
        end: Position {
            line: end_line,
            col: end_col,
        },
    }
}
