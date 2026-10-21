use wright_analyzer::registry::LintRegistry;

use crate::config::SessionConfig;
use crate::diag::{Diagnostic, Stage};

pub(super) fn compose(config: &SessionConfig) -> Result<LintRegistry, Diagnostic> {
    let mut registry = LintRegistry::default();
    crate::lint_package::activate_pinned(&config.lint, &config.lint_package, &mut registry)?;
    for path in &config.lint_rule_paths {
        registry.load_path(path).map_err(|error| {
            Diagnostic::error("lint-rule-error", Stage::Analysis, error.to_string())
        })?;
    }
    if let Err(message) = config.selection.validate(&registry) {
        return Err(Diagnostic::error(
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
        return Err(Diagnostic::error(
            "invalid-selection",
            Stage::Discovery,
            format!("unknown rule id '{rule_id}'"),
        ));
    }
    Ok(registry)
}
