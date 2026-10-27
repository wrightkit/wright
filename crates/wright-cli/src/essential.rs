//! The CLI-side essential-projection plumbing: wrap a workflow result's
//! serialized payload in an [`EssentialResult`] that also declares which
//! command produced it and carries the findings' typed identities, so no
//! downstream renderer re-parses the projection (#1001).

use wright_driver::Severity;
use wright_driver::result::{AnalyzeResult, Envelope, InspectResult, LintResult};
use wright_driver::result::{EssentialCommand, EssentialResult};
use wright_driver::select::SelectionOutcome;

/// Project `lint` to its essential form: the payload rows keep their
/// serialized shape while `findings_meta` carries the typed severities
/// renderers consume (#1001); `max_severity` covers the full set so the
/// verdict reflects every finding, not the projected remainder.
pub(crate) fn lint(envelope: Envelope<LintResult>, all: bool) -> Envelope<EssentialResult> {
    envelope.map_result(|result| {
        let serialized = serde_json::to_value(&result).expect("result serializes");
        let max_severity = result.findings_meta.iter().map(|meta| meta.severity).max();
        let (payload, findings_meta) =
            wright_driver::essential::lint(&serialized, &result.findings_meta, all);
        essential(
            result.selection.clone(),
            payload,
            findings_meta,
            max_severity,
            EssentialCommand::Lint,
        )
    })
}

/// Project `inspect` to its essential form — inspect emits no findings,
/// so the meta channel is empty (#1001).
pub(crate) fn inspect(envelope: Envelope<InspectResult>, all: bool) -> Envelope<EssentialResult> {
    envelope.map_result(|result| {
        let serialized = serde_json::to_value(&result).expect("result serializes");
        let payload = wright_driver::essential::inspect(&serialized, all);
        essential(None, payload, Vec::new(), None, EssentialCommand::Inspect)
    })
}

/// Project `analyze` to its essential form: `risks_meta` supplies the
/// typed risk severities and the full-set max (#1001).
pub(crate) fn analyze(envelope: Envelope<AnalyzeResult>, all: bool) -> Envelope<EssentialResult> {
    envelope.map_result(|result| {
        let serialized = serde_json::to_value(&result).expect("result serializes");
        let max_severity = result.risks_meta.iter().map(|meta| meta.severity).max();
        let (payload, findings_meta) =
            wright_driver::essential::analyze(&serialized, &result.risks_meta, all);
        essential(
            result.selection.clone(),
            payload,
            findings_meta,
            max_severity,
            EssentialCommand::Analyze,
        )
    })
}

fn essential(
    selection: Option<SelectionOutcome>,
    payload: serde_json::Value,
    findings_meta: Vec<wright_driver::FindingMeta>,
    max_severity: Option<Severity>,
    command: EssentialCommand,
) -> EssentialResult {
    EssentialResult {
        result: payload,
        selection,
        command,
        findings_meta,
        max_severity,
    }
}
