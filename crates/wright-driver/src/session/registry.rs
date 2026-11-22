use std::sync::Arc;

use wright_analyzer::registry::{LintRegistry, RuleRegistryError};

use crate::config::SessionConfig;
use crate::diag::{Diagnostic, ExitClass, Stage};

fn compose(config: &SessionConfig) -> Result<LintRegistry, Diagnostic> {
    let mut registry = LintRegistry::default();
    // Lint rule packages resolve and fetch through the filesystem and the
    // network — native-only (ADR-0037 decision 5). The wasm core builds
    // with no pinned packages, so the registry carries its defaults.
    #[cfg(not(target_arch = "wasm32"))]
    crate::lint_package::activate_pinned(&config.lint, &config.lint_package, &mut registry)?;
    for path in &config.lint_rule_paths {
        registry.load_path(path).map_err(|error| {
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

impl super::CompilerSession {
    pub(super) fn load_with_operation(
        &mut self,
        provider_operation: super::ProviderOperation,
    ) -> Result<super::Loaded, Diagnostic> {
        let loaded = self.load_program(provider_operation)?;
        self.compose_lint_registry()?;
        Ok(loaded)
    }

    /// Compose the lint policy now instead of on first use, for a long-lived
    /// host that refuses a bad policy at startup (`ToolService::new`, the
    /// language service). Refuses a changed configuration first, then reports
    /// the diagnostic the first workflow would.
    pub fn validate_lint_policy(&self) -> Result<(), Diagnostic> {
        self.verify_fixed_config()?;
        self.compose_lint_registry().map(|_| ())
    }

    /// The lint registry, composed once on first call. Every workflow that
    /// needs it loads a program first, and that load reports a composition
    /// error.
    pub(super) fn compose_lint_registry(&self) -> Result<&Arc<LintRegistry>, Diagnostic> {
        self.lint_registry
            .get_or_init(|| {
                self.trace
                    .in_span("lint.compose", || compose(&self.fixed_config).map(Arc::new))
            })
            .as_ref()
            .map_err(Clone::clone)
    }
}
