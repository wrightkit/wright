use wright_analyzer::registry::{LintRegistry, RuleRegistryError};

use crate::config::SessionConfig;
use crate::diag::{Diagnostic, ExitClass, Stage};

pub(super) fn compose(config: &SessionConfig) -> Result<LintRegistry, Diagnostic> {
    let mut registry = LintRegistry::default();
    // Lint rule packages resolve and fetch through the filesystem and the
    // network — native-only (ADR-0037 decision 5). The wasm core builds
    // with no pinned packages, so the registry carries its defaults.
    #[cfg(not(target_arch = "wasm32"))]
    crate::lint_package::activate_pinned(&config.lint, &config.lint_package, &mut registry)?;
    for path in &config.lint_rule_paths {
        registry.load_path(path).map_err(|error| {
            // #1028: a missing or unreadable `--rule-file` path is a usage
            // error; a file that loads but is invalid content stays a
            // source error.
            let class = match error {
                RuleRegistryError::Io(..) => ExitClass::Usage,
                _ => ExitClass::SourceError,
            };
            Diagnostic::classed_error("lint-rule-error", Stage::Analysis, class, error.to_string())
        })?;
    }
    if let Err(message) = config.selection.validate(&registry) {
        return Err(Diagnostic::usage_error(
            "invalid-selection",
            Stage::Discovery,
            message,
        ));
    }
    if let Some(rule_id) = config
        .lint
        .rules
        .keys()
        .filter(|id| !registry.contains(id))
        .min()
    {
        return Err(Diagnostic::usage_error(
            "invalid-selection",
            Stage::Discovery,
            format!("unknown rule id '{rule_id}'"),
        ));
    }
    Ok(registry)
}
