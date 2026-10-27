//! Workshop/provider error → `Diagnostic` rendering for the session
//! loader: provider-artifact origins and span mapping, and the LPP
//! provider failure rendering (native-only, like the transport).

use std::path::Path;

use super::root_relative;
use crate::diag::{Diagnostic, Origin, workshop_diag};
use crate::input::ResolvedInput;

fn provider_artifact_origin(resolved: &ResolvedInput) -> Origin {
    Origin {
        kind: "provider-artifact".to_string(),
        locale: resolved.origin.locale.clone(),
    }
}

pub(super) fn workshop_diag_for_provider_artifact(
    error: workshop_rs::WorkshopError,
    resolved: &ResolvedInput,
    mapped_files: &[String],
) -> Diagnostic {
    let mut diagnostic = workshop_diag(error, resolved);
    if mapped_files.is_empty() {
        if let Some(span) = &mut diagnostic.span {
            span.path = "<provider-artifact>".to_string();
        }
        diagnostic.source = Some(provider_artifact_origin(resolved));
    } else if let Some(span) = &mut diagnostic.span {
        match mapped_files.get(span.file) {
            Some(path) => {
                span.path = root_relative(Some(Path::new(path)), &resolved.root)
                    .unwrap_or_else(|| path.clone());
            }
            None => diagnostic.span = None,
        }
    }
    if diagnostic.span.is_none() {
        diagnostic.source = Some(provider_artifact_origin(resolved));
    }
    diagnostic
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn provider_error_diagnostic(
    error: wright_lpp::ProviderError,
    operation: &str,
) -> Diagnostic {
    Diagnostic::carried_classed_error(
        crate::source_provider::provider_failure_code(&error),
        crate::source_provider::provider_failure_stage(&error),
        crate::source_provider::provider_failure_class(&error),
        format!(
            "the provider could not {} the artifact: {}",
            operation, error
        ),
    )
}
