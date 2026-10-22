//! Effective lint registry composition for a session: the built-in rules
//! plus `packages:` pins (issue #843), `--rule-file` paths, then policy
//! validation. Unknown effective-policy IDs are usage errors (#640).

use wright_analyzer::registry::LintRegistry;
use wright_analyzer::service::marker;

use crate::config::SessionConfig;
use crate::diag::{Diagnostic, Stage};

/// Build the session's lint registry. `packages:` pins activate first —
/// fetching pinned SHAs the store lacks — so package rule IDs exist before
/// `--rule-file` paths and before selection/`rules:` validation.
pub(super) fn compose(config: &SessionConfig) -> Result<LintRegistry, Diagnostic> {
    let mut registry = LintRegistry::default();
    crate::lint_package::activate_pinned(&config.lint, &config.lint_package, &mut registry)?;
    for path in &config.lint_rule_paths {
        registry.load_path(path).map_err(|error| {
            Diagnostic::error(
                "lint-rule-error",
                Stage::Analysis,
                marker::escape(&error.to_string()),
            )
        })?;
    }
    if let Err(message) = config.selection.validate(&registry) {
        return Err(Diagnostic::error(
            "invalid-selection",
            Stage::Discovery,
            message,
        ));
    }
    // A `--disable-rule`, `--rule-severity`, or lint-config `rules:` key
    // names an effective-policy id: an unknown id is a usage error, not
    // a silent no-op that leaves the intended rule enabled (#640).
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
            format!("unknown rule id '{}'", marker::escape(rule_id)),
        ));
    }
    Ok(registry)
}
