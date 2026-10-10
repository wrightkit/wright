//! Automated fixes for lint findings (#556): the analyzer's `LintFix` plan
//! is materialized here into the validated [`EditTransaction`] contract —
//! edits against the authored source, carrying the input identity, that
//! `validateEditTransaction` previews and `write_previews` applies. Fixes
//! exist only for `exact` findings whose correction is unambiguous
//! (`duplicate-condition`, `repeated-value`), and only for input whose
//! spans map to retained authored source: raw Workshop parses, and
//! provider-mapped programs whose file table retains source text (#583).

use serde::Serialize;
use serde_json::{Value as JsonValue, json};
use workshop_rs::catalog::{Catalog, Kind, Locale};
use workshop_rs::source::Span;
use wright_analyzer::canonical::{Finding, LintFix};

use crate::edit::{EditRange, EditTransaction, SourceEdit};
use crate::session::{Loaded, Provenance};

/// A finding's materialized fix as it appears in the structured result:
/// a stable `kind`, a one-line `summary`, and the validated-edit
/// `transaction` a consumer previews through `validateEditTransaction` and
/// applies through the same source-edit path as a semantic rename.
#[derive(Debug, Clone, Serialize)]
pub struct LintFixProposal {
    pub kind: &'static str,
    pub summary: &'static str,
    pub transaction: EditTransaction,
}

/// Attach each finding's materialized fix to its serialized form. The
/// `findings` list and `json` array are the same findings in the same order
/// (`SemanticService` serializes `findings()` in order); a finding whose
/// fix cannot be materialized — no plan, or no authored source — simply
/// carries no `fix` member.
pub fn attach_fixes(
    json: &mut [JsonValue],
    findings: &[Finding],
    loaded: &Loaded,
    catalog: &Catalog,
    locale: &Locale,
) {
    // An unmapped provider artifact's spans index generated text, not the
    // caller's files; a mapped artifact's spans address authored members.
    if loaded.provenance == Provenance::Unmapped {
        return;
    }
    for (finding, json) in findings.iter().zip(json.iter_mut()) {
        let Some(proposal) = materialize(finding, loaded, catalog, locale) else {
            continue;
        };
        if let Some(object) = json.as_object_mut() {
            object.insert(
                "fix".to_string(),
                json!({
                    "kind": proposal.kind,
                    "summary": proposal.summary,
                    "transaction": proposal.transaction,
                }),
            );
        }
    }
}

/// Turn a finding's fix plan into the source-edit transaction for the
/// loaded input: each edit names the input's display source and the
/// identity of the text the program was parsed from, so a changed source
/// refuses at validation time with `edit-stale-source`.
fn materialize(
    finding: &Finding,
    loaded: &Loaded,
    catalog: &Catalog,
    locale: &Locale,
) -> Option<LintFixProposal> {
    let fix = finding.fix.as_ref()?;
    let edits = match fix {
        LintFix::RemoveDeadBranch { span } => vec![edit(loaded, *span, String::new())?],
        LintFix::EvaluateOnce { occurrences } => {
            let spelling = catalog.spelling(Kind::Value, locale, "evaluateOnce")?;
            occurrences
                .iter()
                .map(|span| {
                    let source = loaded
                        .program
                        .source(span.file)
                        .filter(|document| document.text() == loaded.input.text)?;
                    let range = source.byte_range(*span)?;
                    let original = source.text()[range].to_string();
                    if original.is_empty() {
                        return None;
                    }
                    edit(loaded, *span, format!("{spelling}({original})"))
                })
                .collect::<Option<Vec<_>>>()?
        }
    };
    let kind = match fix {
        LintFix::RemoveDeadBranch { .. } => "remove-dead-branch",
        LintFix::EvaluateOnce { .. } => "evaluate-once",
    };
    let summary = match fix {
        LintFix::RemoveDeadBranch { .. } => {
            "remove the unreachable Else If branch, keeping the rest of the chain"
        }
        LintFix::EvaluateOnce { .. } => {
            "mark each duplicated occurrence with Evaluate Once, the idiom for an intentional repeated evaluation"
        }
    };
    Some(LintFixProposal {
        kind,
        summary,
        transaction: EditTransaction::new(edits).ok()?,
    })
}

/// One plan span as a `SourceEdit` against the loaded input. For a raw
/// Workshop input the span must map to file 0's parsed text; for a
/// provider-mapped program the span's file indexes the source map's file
/// table — the edit names the member's `file://` URI and the identity of
/// the retained text, so `providerValidateEdits` and the write path
/// preconditions see the same bytes (#583).
fn edit(loaded: &Loaded, span: Span, new_text: String) -> Option<SourceEdit> {
    let range = EditRange {
        start_line: span.start.line,
        start_col: span.start.col,
        end_line: span.end.line,
        end_col: span.end.col,
    };
    match loaded.provenance {
        Provenance::Source => (span.file.index() == 0).then(|| SourceEdit {
            edit_kind: "fix".to_string(),
            source: loaded.input.display.clone(),
            source_identity: loaded.input.identity.clone(),
            range,
            new_text,
        }),
        Provenance::Mapped => {
            let member = loaded.source_files.get(span.file.index())?;
            let (uri, _) =
                crate::source_provider::provider_member(member, &loaded.input.root).ok()?;
            let identity = crate::input_identity(loaded.program.source(span.file)?.text());
            Some(SourceEdit {
                edit_kind: "fix".to_string(),
                source: uri,
                source_identity: identity,
                range,
                new_text,
            })
        }
        Provenance::Unmapped => None,
    }
}
