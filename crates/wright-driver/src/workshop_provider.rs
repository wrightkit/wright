use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::provider::{
    Diagnostic as ProviderDiagnostic, LanguageProvider, ProviderError, Result as ProviderResult,
    Severity as ProviderSeverity, SourceSpan as ProviderSourceSpan, Status,
};
/// Wright's in-process provider for localized raw Workshop source.
pub struct WorkshopProvider {
    catalog: Arc<workshop_rs::catalog::Catalog>,
}

impl WorkshopProvider {
    /// Construct a provider from the canonical Workshop catalog.
    pub fn new() -> ProviderResult<Self> {
        let catalog = wright_analyzer::catalog::builtin()
            .map_err(|error| ProviderError::new("workshop.catalog", error.to_string()))?;
        Ok(Self { catalog })
    }
}

impl LanguageProvider for WorkshopProvider {
    fn check(&self, source: &str, path: &Path) -> ProviderResult<Vec<ProviderDiagnostic>> {
        let locale = workshop_rs::detect::resolve_locale(source, &self.catalog, None)
            .map_err(|error| ProviderError::new("workshop.locale", error.to_string()))?;
        let program =
            workshop_rs::parser::parse_with_context(source, &self.catalog, &locale, &*self.catalog)
                .map_err(|error| ProviderError::new("workshop.parse", error.to_string()))?;
        program
            .validate()
            .map_err(|error| ProviderError::new("workshop.validate", error.to_string()))?;

        let mut diagnostics = Vec::new();
        if let Err(error) = validate_canonical(&program, &self.catalog) {
            diagnostics.push(canonical_diagnostic(&error, path));
        }
        diagnostics.extend(
            program
                .semantic_issues(&self.catalog)
                .into_iter()
                .map(|issue| map_issue(&issue, path)),
        );
        Ok(diagnostics)
    }
}

/// Catalog-aware validation over the owner's canonical contract, composed
/// with the residual completeness surface. `validate_canonical_ids` is
/// fail-fast, so a catalog-unknown residual it also flags would mask every
/// later violation; instead the constructs the owner itself reports as
/// residuals (`semantic_issues`) are neutralized in a clone, and canonical
/// validation judges the remaining program. Residuals still report through
/// the completeness channel; every canonical error on the known remainder
/// is a real owner rejection.
pub(crate) fn validate_canonical(
    program: &workshop_rs::Program,
    catalog: &workshop_rs::catalog::Catalog,
) -> Result<(), workshop_rs::WorkshopError> {
    let mut residual_actions = HashSet::new();
    let mut residual_values = HashSet::new();
    for issue in program.semantic_issues(catalog) {
        match issue.kind {
            workshop_rs::rules::IncompletenessKind::UnknownAction
            | workshop_rs::rules::IncompletenessKind::OpaqueAction => {
                residual_actions.insert(issue.name);
            }
            workshop_rs::rules::IncompletenessKind::UnknownValue => {
                residual_values.insert(issue.name);
            }
            workshop_rs::rules::IncompletenessKind::RawSetting => {}
        }
    }
    if residual_actions.is_empty() && residual_values.is_empty() {
        return workshop_rs::validate::validate_canonical_ids(program, catalog);
    }
    let mut stripped = program.clone();
    strip_residuals(&mut stripped, &residual_actions, &residual_values);
    workshop_rs::validate::validate_canonical_ids(&stripped, catalog)
}

fn strip_residuals(
    program: &mut workshop_rs::Program,
    residual_actions: &HashSet<String>,
    residual_values: &HashSet<String>,
) {
    for rule in &mut program.rules {
        rule.actions
            .retain(|action| !is_residual_action(action, residual_actions));
        for action in &mut rule.actions {
            strip_action_values(action, residual_values);
        }
        for condition in &mut rule.conditions {
            strip_residual_value(&mut condition.value, residual_values);
        }
    }
}

/// Whether an action is itself a reported residual — an unknown or opaque
/// call, or a disabled block wrapping one.
fn is_residual_action(action: &workshop_rs::Action, residual_actions: &HashSet<String>) -> bool {
    match action {
        workshop_rs::Action::Call { name, .. } => residual_actions.contains(name),
        workshop_rs::Action::Disabled { action } => is_residual_action(action, residual_actions),
        _ => false,
    }
}

fn strip_action_values(action: &mut workshop_rs::Action, residual_values: &HashSet<String>) {
    use workshop_rs::Action;
    match action {
        Action::Call { args, .. } => {
            for arg in args {
                strip_residual_value(arg, residual_values);
            }
        }
        Action::SetGlobalVariable { value, .. }
        | Action::ModifyGlobalVariable { value, .. }
        | Action::If { condition: value }
        | Action::ElseIf { condition: value }
        | Action::While { condition: value } => {
            strip_residual_value(value, residual_values);
        }
        Action::SetPlayerVariable { player, value, .. }
        | Action::ModifyPlayerVariable { player, value, .. }
        | Action::AssignMember {
            target: player,
            value,
            ..
        } => {
            strip_residual_value(player, residual_values);
            strip_residual_value(value, residual_values);
        }
        Action::ForGlobalVariable {
            start, stop, step, ..
        } => {
            for value in [start, stop, step] {
                strip_residual_value(value, residual_values);
            }
        }
        Action::ForPlayerVariable {
            player,
            start,
            stop,
            step,
            ..
        } => {
            for value in [player, start, stop, step] {
                strip_residual_value(value, residual_values);
            }
        }
        Action::Disabled { action } => strip_action_values(action, residual_values),
        Action::CallSubroutine { .. } | Action::Else | Action::End => {}
    }
}

/// Replace a residual value call with `Null`: the owner treats Null as a
/// valid placeholder for every value contract, so the substitution cannot
/// fabricate a violation, and it keeps the enclosing call's arity and the
/// rest of its arguments under canonical scrutiny.
fn strip_residual_value(value: &mut workshop_rs::Value, residual_values: &HashSet<String>) {
    use workshop_rs::Value;
    if matches!(value, Value::Call { name, .. } if residual_values.contains(name)) {
        *value = Value::Null;
        return;
    }
    match value {
        Value::Call { args, .. } => {
            for arg in args {
                strip_residual_value(arg, residual_values);
            }
        }
        Value::Array(elements) => {
            for element in elements {
                strip_residual_value(element, residual_values);
            }
        }
        Value::Vector { x, y, z } => {
            for component in [x.as_mut(), y.as_mut(), z.as_mut()] {
                strip_residual_value(component, residual_values);
            }
        }
        Value::PlayerVariable { player, .. } => strip_residual_value(player, residual_values),
        _ => {}
    }
}

pub(crate) fn map_issue(
    issue: &workshop_rs::rules::SemanticIssue,
    path: &Path,
) -> ProviderDiagnostic {
    let (kind_code, severity) = match issue.kind {
        workshop_rs::rules::IncompletenessKind::RawSetting => {
            ("raw-setting", ProviderSeverity::Warning)
        }
        workshop_rs::rules::IncompletenessKind::UnknownAction => {
            ("unknown-action", ProviderSeverity::Error)
        }
        workshop_rs::rules::IncompletenessKind::UnknownValue => {
            ("unknown-value", ProviderSeverity::Error)
        }
        workshop_rs::rules::IncompletenessKind::OpaqueAction => {
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
    classification: workshop_rs::rules::ResidualClassification,
) -> Status {
    match classification {
        workshop_rs::rules::ResidualClassification::ProjectDefinedConstruct
        | workshop_rs::rules::ResidualClassification::SourceDeclaredVariable => Status::Partial,
        workshop_rs::rules::ResidualClassification::ProducerExtension
        | workshop_rs::rules::ResidualClassification::LegacyOpaque
        | workshop_rs::rules::ResidualClassification::UnresolvedIdentifier => Status::Unsupported,
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

/// Project an owner canonical-validation rejection into a provider
/// diagnostic: the owner error keeps its identity and source span while the
/// rejection surfaces as an error-severity finding, like the residual
/// diagnostics this path already returns.
fn canonical_diagnostic(error: &workshop_rs::WorkshopError, path: &Path) -> ProviderDiagnostic {
    ProviderDiagnostic {
        code: "workshop.catalog-validation".to_string(),
        severity: ProviderSeverity::Error,
        status: Status::Unsupported,
        span: provider_span(workshop_error_span(error), path),
        message: error.to_string(),
    }
}

fn workshop_error_span(error: &workshop_rs::WorkshopError) -> Option<workshop_rs::source::Span> {
    match error {
        workshop_rs::WorkshopError::Unknown { span, .. }
        | workshop_rs::WorkshopError::Malformed { span, .. }
        | workshop_rs::WorkshopError::Unsupported { span, .. } => *span,
        _ => None,
    }
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Supported => "supported",
        Status::Partial => "partially supported",
        Status::Unsupported => "unsupported",
    }
}

fn provider_span(span: Option<workshop_rs::source::Span>, path: &Path) -> ProviderSourceSpan {
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
                workshop_rs::rules::ResidualClassification::ProjectDefinedConstruct
            ),
            Status::Partial
        );
        assert_eq!(
            status_for_classification(
                workshop_rs::rules::ResidualClassification::SourceDeclaredVariable
            ),
            Status::Partial
        );
        assert_eq!(
            status_for_classification(
                workshop_rs::rules::ResidualClassification::ProducerExtension
            ),
            Status::Unsupported
        );
        assert_eq!(
            status_for_classification(workshop_rs::rules::ResidualClassification::LegacyOpaque),
            Status::Unsupported
        );
        assert_eq!(
            status_for_classification(
                workshop_rs::rules::ResidualClassification::UnresolvedIdentifier
            ),
            Status::Unsupported
        );
    }
}
