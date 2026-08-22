//! In-process raw Workshop provider.

use std::path::{Path, PathBuf};

use wright_core::provider::{
    Diagnostic as ProviderDiagnostic, LanguageProvider, ProviderError, Result as ProviderResult,
    Severity as ProviderSeverity, SourceSpan as ProviderSourceSpan, Status,
};
use wright_core::signatures::ExpectedDomain as WrightExpectedDomain;

/// Wright's in-process provider for localized raw Workshop source.
pub struct WorkshopProvider {
    catalog: workshop_rs_provider::catalog::Catalog,
}

impl WorkshopProvider {
    /// Construct a provider from the canonical Workshop catalog.
    pub fn new() -> ProviderResult<Self> {
        let catalog = workshop_rs_provider::catalog::Catalog::builtin()
            .map_err(|error| ProviderError::new("workshop.catalog", error.to_string()))?;
        Ok(Self { catalog })
    }
}

impl LanguageProvider for WorkshopProvider {
    fn check(&self, source: &str, path: &Path) -> ProviderResult<Vec<ProviderDiagnostic>> {
        let locale = workshop_rs_provider::detect::resolve_locale(source, &self.catalog, None)
            .map_err(|error| ProviderError::new("workshop.locale", error.to_string()))?;
        let manifest = wright_opy::manifest::Manifest::builtin()
            .map_err(|error| ProviderError::new("workshop.manifest", error.to_string()))?;
        let context = ProviderExpectedDomain {
            manifest: &manifest,
            catalog: &self.catalog,
        };
        let program = workshop_rs_provider::parser::parse_with_context(
            source,
            &self.catalog,
            &locale,
            &context,
        )
        .map_err(|error| ProviderError::new("workshop.parse", error.to_string()))?;
        program
            .validate()
            .map_err(|error| ProviderError::new("workshop.validate", error.to_string()))?;

        Ok(program
            .semantic_issues(&self.catalog)
            .into_iter()
            .map(|issue| map_issue(issue, path))
            .collect())
    }
}

struct ProviderExpectedDomain<'a> {
    manifest: &'a wright_opy::manifest::Manifest,
    catalog: &'a workshop_rs_provider::catalog::Catalog,
}

impl workshop_rs_provider::signatures::ExpectedDomain for ProviderExpectedDomain<'_> {
    fn expected_domain(&self, catalog_id: &str, arg_index: usize) -> Option<&str> {
        WrightExpectedDomain::expected_domain(self.manifest, catalog_id, arg_index).or_else(|| {
            workshop_rs_provider::signatures::ExpectedDomain::expected_domain(
                self.catalog,
                catalog_id,
                arg_index,
            )
        })
    }
}

fn map_issue(
    issue: workshop_rs_provider::semantic::SemanticIssue,
    path: &Path,
) -> ProviderDiagnostic {
    let (kind_code, severity) = match issue.kind {
        workshop_rs_provider::semantic::IncompletenessKind::RawSetting => {
            ("raw-setting", ProviderSeverity::Warning)
        }
        workshop_rs_provider::semantic::IncompletenessKind::UnknownAction => {
            ("unknown-action", ProviderSeverity::Error)
        }
        workshop_rs_provider::semantic::IncompletenessKind::UnknownValue => {
            ("unknown-value", ProviderSeverity::Error)
        }
        workshop_rs_provider::semantic::IncompletenessKind::OpaqueAction => {
            ("opaque-action", ProviderSeverity::Error)
        }
    };
    let code = diagnostic_code(kind_code, &issue.name);
    let status = status_for_classification(issue.classification);
    let message = format!(
        "Workshop construct '{}' is {} ({})",
        issue.name,
        status_name(status),
        issue.classification.as_str()
    );
    ProviderDiagnostic {
        code,
        severity,
        status,
        span: provider_span(issue.span, path),
        message,
    }
}

pub fn status_for_classification(
    classification: workshop_rs_provider::semantic::ResidualClassification,
) -> Status {
    match classification {
        workshop_rs_provider::semantic::ResidualClassification::ProjectDefinedConstruct
        | workshop_rs_provider::semantic::ResidualClassification::SourceDeclaredVariable => {
            Status::Partial
        }
        workshop_rs_provider::semantic::ResidualClassification::ProducerExtension
        | workshop_rs_provider::semantic::ResidualClassification::LegacyOpaque
        | workshop_rs_provider::semantic::ResidualClassification::UnresolvedIdentifier => {
            Status::Unsupported
        }
    }
}

pub fn diagnostic_code(kind: &str, identity: &str) -> String {
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

fn provider_span(
    span: Option<workshop_rs_provider::source::Span>,
    path: &Path,
) -> ProviderSourceSpan {
    let (start_line, start_col, end_line, end_col) = span
        .map(|span| (span.start.line, span.start.col, span.end.line, span.end.col))
        .unwrap_or((1, 1, 1, 1));
    ProviderSourceSpan {
        file: PathBuf::from(path),
        start_line,
        start_col,
        end_line,
        end_col,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_residual_classifications_fail_closed() {
        assert_eq!(
            status_for_classification(
                workshop_rs_provider::semantic::ResidualClassification::ProjectDefinedConstruct
            ),
            Status::Partial
        );
        assert_eq!(
            status_for_classification(
                workshop_rs_provider::semantic::ResidualClassification::SourceDeclaredVariable
            ),
            Status::Partial
        );
        assert_eq!(
            status_for_classification(
                workshop_rs_provider::semantic::ResidualClassification::ProducerExtension
            ),
            Status::Unsupported
        );
        assert_eq!(
            status_for_classification(
                workshop_rs_provider::semantic::ResidualClassification::LegacyOpaque
            ),
            Status::Unsupported
        );
        assert_eq!(
            status_for_classification(
                workshop_rs_provider::semantic::ResidualClassification::UnresolvedIdentifier
            ),
            Status::Unsupported
        );
    }
}
