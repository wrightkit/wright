//! `lint` — the one pipeline every surface shares (#1002): the session's
//! lint workflow, the verdict contract (any `error`-severity finding
//! yields `ok: false`, `exit: 1`), and the `--fail-on` gate the driver
//! applies on top, so CLI, `serve`, MCP, and embedding cannot diverge.

use wright_analyzer::canonical::SemanticService;
use wright_analyzer::service::Request;

use super::{
    CompilerSession, Loaded, ProviderOperation, findings_with_meta, full_finding_max,
    program_summary, select_findings, service_response,
};
use crate::progress::{ProgressEvent, ProgressPhase, ProgressUnit};
use crate::result::{Envelope, LintResult};

impl CompilerSession {
    /// `lint` over an already-served snapshot (#1002): the same
    /// pipeline [`Self::lint`] runs — the service's snapshot is the
    /// loaded input, so the `lint` operation and `wright lint` share one
    /// implementation and one verdict.
    pub(crate) fn lint_loaded(
        &mut self,
        loaded: Loaded,
        service: &SemanticService<'_>,
        selection: &crate::select::FindingSelection,
    ) -> Envelope<LintResult> {
        let mut envelope = self.with_loaded(
            "lint",
            |_| Ok(loaded),
            |session, loaded| {
                session.attach_workshop_completeness(&loaded);
                let configured;
                let service = if session.config.lint.rules.is_empty() {
                    service
                } else {
                    configured = service.with_lint_config(session.config.lint.clone());
                    &configured
                };
                session.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
                let program = program_summary(service, &loaded);
                let lint_rules = service_response(service, &Request::LintRules);
                let lint_rule_count = lint_rules
                    .pointer("/rules")
                    .and_then(serde_json::Value::as_array)
                    .map_or(0, Vec::len);
                session.progress(ProgressEvent::with_count(
                    ProgressPhase::Linting,
                    lint_rule_count,
                    ProgressUnit::Rules,
                ));
                let (findings, findings_meta, outcome) = select_findings(
                    service,
                    &loaded,
                    session.catalog(),
                    selection,
                    findings_with_meta(service),
                );
                let (rules, config, skipped) = crate::result::split_lint_rules(&lint_rules);
                LintResult {
                    input_identity: loaded.input.identity.clone(),
                    program,
                    rules,
                    config,
                    findings: serde_json::Value::Array(findings),
                    findings_meta,
                    skipped,
                    selection: outcome,
                    fixes: None,
                    next: crate::result::next::list(crate::result::next::LINT),
                }
            },
        );
        lint_verdict(&mut envelope, self.config.fail_on);
        envelope
    }

    /// `lint`: load and produce the source identity, program summary, per-rule
    /// id and effective severity, effective configuration, and findings (#98).
    /// Full rule metadata is served by `lintRules` rather than inlined into
    /// every `lint` result (#431).
    ///
    /// Lint rule findings are reported in `result.findings`; frontend and
    /// Workshop semantic-completeness diagnostics remain in the envelope.
    /// Rule enable/disable and severity come from `self.config.lint`, the same
    /// configuration the CLI flags and programmatic consumers set.
    ///
    /// The verdict is the one lint contract (#1002): any `error`-severity
    /// finding yields `ok: false`, `exit: 1`, then the configured
    /// `--fail-on` gate may raise it further — every surface (CLI, serve,
    /// MCP, embedding) reports the same verdict for the same input.
    #[hotpath::measure]
    pub fn lint(&mut self) -> Envelope<LintResult> {
        let mut envelope = self.with_loaded(
            "lint",
            |session| session.load_with_operation(ProviderOperation::Compile),
            |session, loaded| {
                session.attach_workshop_completeness(&loaded);
                let shared = session.shared_semantic(&loaded);
                let configured;
                let service = if session.config.lint.rules.is_empty() {
                    &*shared
                } else {
                    configured = shared.with_lint_config(session.config.lint.clone());
                    &configured
                };
                session.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
                let program = program_summary(service, &loaded);
                let lint_rules = service_response(service, &Request::LintRules);
                let lint_rule_count = lint_rules
                    .pointer("/rules")
                    .and_then(serde_json::Value::as_array)
                    .map_or(0, Vec::len);
                session.progress(ProgressEvent::with_count(
                    ProgressPhase::Linting,
                    lint_rule_count,
                    ProgressUnit::Rules,
                ));
                let (findings, findings_meta, selection) = select_findings(
                    service,
                    &loaded,
                    session.catalog(),
                    &session.config.selection,
                    findings_with_meta(service),
                );
                let (rules, config, skipped) = crate::result::split_lint_rules(&lint_rules);
                LintResult {
                    input_identity: loaded.input.identity.clone(),
                    program,
                    rules,
                    config,
                    findings: serde_json::Value::Array(findings),
                    findings_meta,
                    skipped,
                    selection,
                    fixes: None,
                    next: crate::result::next::list(crate::result::next::LINT),
                }
            },
        );
        lint_verdict(&mut envelope, self.config.fail_on);
        envelope
    }
}

/// The one lint verdict (#1002): the complete finding set's highest
/// severity — an `error` yields `ok: false`, `exit: 1` — then the
/// configured `--fail-on` gate applies on top. Both lint entry points
/// (`lint`, `lint_loaded`) resolve their verdict through this so CLI,
/// `serve`, MCP, and embedding cannot diverge.
fn lint_verdict(envelope: &mut Envelope<LintResult>, fail_on: Option<crate::Severity>) {
    let full_max = full_finding_max(&envelope.result.selection, &envelope.result.findings_meta);
    if full_max == Some(crate::Severity::Error) {
        envelope.ok = false;
        envelope.exit = crate::result::exit::SOURCE_ERROR;
    }
    if let Some(threshold) = fail_on {
        envelope.apply_fail_on(threshold, full_max);
    }
}
