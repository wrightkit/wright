//! Command result envelopes and exit code conventions.

use crate::diag::{Diagnostic, Severity};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct VersionInfo {
    pub version: String,
    pub contract: String,
}

pub const RESULT_CONTRACT: &str = "wright-result/v1";
pub const DRIVER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod exit {
    pub const SUCCESS: u8 = 0;
    pub const SOURCE_ERROR: u8 = 1;
    pub const USAGE: u8 = 2;
    pub const UNSUPPORTED: u8 = 3;
    pub const INTERNAL: u8 = 4;
}

#[derive(Debug, Clone, Serialize)]
pub struct Envelope<T: Serialize> {
    pub wright: VersionInfo,
    pub command: String,
    pub ok: bool,
    pub exit: u8,
    /// Diagnostics remaining after selection; `ok`/`exit` always reflect the
    /// full set, never the selected remainder.
    pub diagnostics: Vec<Diagnostic>,
    /// Present when a finding selection was applied to `diagnostics` (#430).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<crate::select::SelectionOutcome>,
    /// Set when the CLI's `--fail-on` gate raised this result's verdict
    /// (ADR-0030 decision 13): the command succeeded but a diagnostic or
    /// finding reached the threshold severity. In-process only — the wire
    /// already carries the raised `ok`/`exit` pair.
    #[serde(skip)]
    pub gated: bool,
    pub result: T,
    /// The run-cache comparison for `compile`/`inspect cost` (ADR-0027,
    /// #723): in-process only — it never serializes into the default JSON
    /// or agent result (decision 3), so `--format json` and `costEstimate`
    /// stay deterministic. The text renderer reads it to show the delta.
    #[serde(skip)]
    pub comparison: Option<serde_json::Value>,
}

impl<T: Serialize> Envelope<T> {
    /// Rebuild this envelope around a transformed result: verdict, exit
    /// code, diagnostics, and any finding selection carry over unchanged
    /// when selecting the essential presentation.
    pub fn map_result<U: Serialize>(self, f: impl FnOnce(T) -> U) -> Envelope<U> {
        Envelope {
            wright: self.wright,
            command: self.command,
            ok: self.ok,
            exit: self.exit,
            diagnostics: self.diagnostics,
            selection: self.selection,
            gated: self.gated,
            result: f(self.result),
            comparison: self.comparison,
        }
    }
}

pub fn exit_code_from(diagnostics: &[Diagnostic]) -> u8 {
    let mut has_source_error = false;
    for d in diagnostics {
        if d.severity != Severity::Error {
            continue;
        }
        if matches!(
            d.code.as_str(),
            "output-exists" | "output-is-input" | "output-unnamed" | "profile-workshop-only"
            // #759: an undeclared `--to` output locale is a flag error, not
            // a source error.
            | "invalid-locale"
        ) {
            return exit::USAGE;
        }
        if d.stage == crate::diag::Stage::Internal {
            return exit::INTERNAL;
        }
        if d.stage == crate::diag::Stage::Reconstruction
            || d.code == "adapter-stdin-unsupported"
            || d.code == "source-provider-unsupported"
            // #569: a provider-declared capability gap is a recognized but
            // unsupported operation, not a source error.
            || d.code == "capability-unavailable"
        {
            return exit::UNSUPPORTED;
        }
        has_source_error = true;
    }
    if has_source_error {
        exit::SOURCE_ERROR
    } else {
        exit::SUCCESS
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CompiledOutput {
    /// The emitted program. In-process callers read it; it is never
    /// serialized (ADR-0025): the program is delivered to a file.
    #[serde(skip)]
    pub text: String,
    pub sha256: String,
    pub locale: String,
    pub written_to: String,
    pub input_identity: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CompileResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<CompiledOutput>,
}

/// The shared follow-up commands a result offers (#792, ADR-0030): renderers
/// print each entry verbatim as a `next:` record or summary footer, so every
/// string is a complete `wright` command line; `<NAME>`/`<RULE>` mark the
/// caller's slots, matching the detail-command hints bare `inspect` prints.
pub mod next {
    /// `inspect`'s detail commands — the same list bare `inspect` prints.
    pub const INSPECT: &[&str] = &[
        "wright inspect symbols",
        "wright inspect refs <NAME>",
        "wright inspect cfg <RULE>",
        "wright inspect callgraph",
        "wright inspect cost",
    ];
    /// After `check`: findings, cost, and the program model.
    pub const CHECK: &[&str] = &["wright lint", "wright analyze", "wright inspect"];
    /// After `lint`: apply offered fixes, then the cost/model context.
    pub const LINT: &[&str] = &["wright lint --fix", "wright analyze", "wright inspect cost"];
    /// After `analyze`: the lint findings and the detail commands.
    pub const ANALYZE: &[&str] = &[
        "wright lint",
        "wright inspect symbols",
        "wright inspect refs <NAME>",
        "wright inspect cfg <RULE>",
        "wright inspect callgraph",
        "wright inspect cost",
    ];

    /// The list as owned strings for a result field.
    pub fn list(commands: &'static [&'static str]) -> Vec<String> {
        commands
            .iter()
            .map(|command| (*command).to_string())
            .collect()
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CheckResult {
    /// The loaded program's summary — the same members `analyze`'s `program`
    /// reports (#792): origin, files, and declaration counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<serde_json::Value>,
    /// The shared follow-up commands (#792).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AnalyzeResult {
    pub program: serde_json::Value,
    /// The fact layers; `facts.risks` carries the risk findings after
    /// selection. The verdict reports the true set via `selection`, not
    /// this array's length.
    pub facts: serde_json::Value,
    /// Present when a finding selection was applied to `facts.risks` (#430).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<crate::select::SelectionOutcome>,
    /// The shared follow-up commands (#792).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct InspectResult {
    pub program: serde_json::Value,
    pub rules: serde_json::Value,
    pub symbols: serde_json::Value,
    pub references: serde_json::Value,
    /// The shared follow-up commands (#792).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next: Vec<String>,
}

/// `rules` result (ADR-0024): the `rules` agent operation's payload verbatim
/// — with `content`, a selection-wrapped list whose entries carry the
/// owner's canonical rule content.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(transparent)]
pub struct RulesResult(pub serde_json::Value);

/// `symbols` result (#429): the `symbols` agent operation's payload verbatim
/// — an array of `{id, kind, name, span}` entries.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(transparent)]
pub struct SymbolsResult(pub serde_json::Value);

/// `refs` result (#429): the `usage` operation's payload — the resolved
/// symbol's `id`, `kind`, `symbol` name, and read/write/call counts — plus a
/// `references` member holding the `references` operation's payload.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(transparent)]
pub struct RefsResult(pub serde_json::Value);

/// `cfg` result (#429): the `cfg` operation's payload verbatim —
/// `{entry, exit, blocks}` for the addressed rule.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(transparent)]
pub struct CfgResult(pub serde_json::Value);

/// `callgraph` result (#429): the `callGraph` operation's payload verbatim —
/// an array of `{caller, callee}` edges.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(transparent)]
pub struct CallGraphResult(pub serde_json::Value);

/// `cost` result (#429): the `costEstimate` operation's payload verbatim —
/// exact generated-resource counts plus static findings, serialized as
/// `result`. `selection` stays in-process: its serializable members already
/// ride in `result.selection`, and the full set's highest severity must
/// survive for the verdict (#430).
#[derive(Debug, Clone, Default)]
pub struct CostResult {
    /// The `costEstimate` payload verbatim, including its `selection`
    /// member when the request selected.
    pub result: serde_json::Value,
    /// The applied finding selection's outcome; never serialized — the wire
    /// member lives in `result.selection`.
    pub selection: Option<crate::select::SelectionOutcome>,
}

impl Serialize for CostResult {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.result.serialize(serializer)
    }
}

/// The essential presentation payload and complete-set selection outcome.
#[derive(Debug, Clone, Default)]
pub struct EssentialResult {
    pub result: serde_json::Value,
    pub selection: Option<crate::select::SelectionOutcome>,
}
impl Serialize for EssentialResult {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.result.serialize(serializer)
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LintResult {
    pub input_identity: String,
    pub program: serde_json::Value,
    pub rules: serde_json::Value,
    pub config: serde_json::Value,
    /// Lint findings after selection. The verdict reports the true total via
    /// `selection`, not this array's length.
    pub findings: serde_json::Value,
    pub skipped: serde_json::Value,
    /// Present when a finding selection was applied to `findings` (#430).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<crate::select::SelectionOutcome>,
    /// Per-fix dispositions under `lint --fix` / `lint --write` (#556):
    /// what each offered fix validated to, and whether it applied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixes: Option<Vec<LintFixOutcome>>,
    /// The shared follow-up commands (#792).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next: Vec<String>,
}

/// What the validated-edit path did with one finding's fix (#556): the fix
/// the finding carried, the diff it validated to, and whether it applied
/// or was refused.
#[derive(Debug, Clone, Serialize)]
pub struct LintFixOutcome {
    /// The finding code the fix remediates.
    pub code: String,
    /// The fix `kind` from the finding's `fix` contract.
    pub kind: String,
    /// The fix's one-line description.
    pub summary: String,
    /// The finding's span, as reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<serde_json::Value>,
    pub status: LintFixStatus,
    /// The before/after text per source the transaction validated to —
    /// self-contained so a consumer renders the diff without joining an
    /// originals map. Present for `preview` and `applied` outcomes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<Vec<LintFixPreview>>,
    /// The structured refusal when `status` is `refused`.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub diagnostics: Vec<crate::diag::Diagnostic>,
}

/// One lint fix's disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LintFixStatus {
    /// `--fix` validated the transaction; nothing was written.
    Preview,
    /// `--write` validated and applied the transaction.
    Applied,
    /// The transaction refused — a structured reason rides in
    /// `diagnostics`; nothing was written.
    Refused,
}

/// One source's before/after text in a fix preview.
#[derive(Debug, Clone, Serialize)]
pub struct LintFixPreview {
    pub source: String,
    pub original: String,
    pub new_text: String,
}

/// `lint` results keep only the per-rule identity needed to interpret a
/// finding — the stable id and its effective severity (#431). Full rule
/// metadata (summary, rationale, documentation, known limits, evidence, tags)
/// is served by the `lintRules` operation instead of being inlined per call.
pub(crate) fn compact_lint_rules(rules: Option<&serde_json::Value>) -> serde_json::Value {
    let Some(rules) = rules.and_then(serde_json::Value::as_array) else {
        return serde_json::json!([]);
    };
    serde_json::Value::Array(
        rules
            .iter()
            .filter_map(|rule| {
                let id = rule.get("id").filter(|v| v.is_string())?;
                let effective_severity = rule.get("effectiveSeverity").filter(|v| v.is_string())?;
                Some(serde_json::json!({
                    "id": id,
                    "effectiveSeverity": effective_severity,
                }))
            })
            .collect(),
    )
}

pub(crate) fn split_lint_rules(
    lint_rules: &serde_json::Value,
) -> (serde_json::Value, serde_json::Value, serde_json::Value) {
    (
        compact_lint_rules(lint_rules.get("rules")),
        lint_rules
            .get("config")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
        lint_rules
            .get("skipped")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([])),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConvertTarget {
    #[default]
    Opy,
    Ostw,
}

impl ConvertTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Opy => "opy",
            Self::Ostw => "ostw",
        }
    }

    pub fn parse(name: &str) -> Option<ConvertTarget> {
        match name {
            "opy" => Some(Self::Opy),
            "ostw" => Some(Self::Ostw),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ConvertResult {
    pub target: ConvertTarget,
    pub text: String,
    pub sha256: String,
}

/// `lint package` result (#844): the outcome of one package lifecycle
/// command — a repository search, an install/list/outdated/update/remove —
/// as one row per repository or store entry, plus the store root the
/// command ran against.
#[derive(Debug, Clone, Default, Serialize)]
pub struct LintPackageResult {
    /// The lint package store root (`WRIGHT_LINT_PACKAGE_DIR` or the
    /// platform application-data dir) the command ran against.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub store: String,
    /// One row per package or repository the command reported on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<LintPackageRow>,
    /// `package search`: the `total_count` GitHub reported across pages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    /// `package search`: GitHub's `incomplete_results` flag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incomplete: Option<bool>,
    /// The shared follow-up commands (#792).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next: Vec<String>,
}

/// One package row in a `lint package` result: `source` + `status` are
/// always present; every other member appears only when the command that
/// produced the row establishes it.
///
/// `status` is one of: `found` (search hits), `installed` (install/list
/// rows and reused store entries), `unchanged`, `updated`, `unavailable`,
/// `error` (outdated/update remote checks), `damaged` (list rows that fail
/// store-entry validation), `removed`, or `pruned` (remove rows whose
/// entry directory survives because unrelated files remain).
#[derive(Debug, Clone, Default, Serialize)]
pub struct LintPackageRow {
    /// The `owner/repo` source identity.
    pub source: String,
    /// The row's outcome status.
    pub status: String,
    /// The store entry's full commit SHA.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    /// The resolved default-branch head on `outdated`/`update` rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    /// The entry's directory in the store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    /// The entry's validated SPEC-309 rule IDs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<String>,
    /// Whether the archive was downloaded by this command (`false` when an
    /// intact store entry was reused).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloaded: Option<bool>,
    /// The repository description on `found` rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The stable error code on `error`/`unavailable`/`damaged` rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// The failure detail on `error`/`unavailable`/`damaged` rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub fn version_info() -> VersionInfo {
    VersionInfo {
        version: DRIVER_VERSION.to_string(),
        contract: RESULT_CONTRACT.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::compact_lint_rules;
    use serde_json::json;

    #[test]
    fn compact_lint_rules_keeps_only_id_and_effective_severity() {
        let rules = json!([
            {
                "id": "min-wait-loop",
                "defaultSeverity": "warning",
                "effectiveSeverity": "error",
                "enabled": false,
                "summary": "loop body waits at the workshop minimum rate",
                "rationale": "…",
                "documentation": "…",
                "knownLimits": "…",
                "evidence": "static-indicator",
                "tags": ["stability"],
                "kind": "builtin"
            }
        ]);
        assert_eq!(
            compact_lint_rules(Some(&rules)),
            json!([{ "id": "min-wait-loop", "effectiveSeverity": "error" }])
        );
    }

    #[test]
    fn compact_lint_rules_drops_malformed_entries() {
        for rules in [
            json!(null),
            json!("not-an-array"),
            json!(["not-an-object"]),
            json!([{ "id": "min-wait-loop" }]),
            json!([{ "id": "min-wait-loop", "effectiveSeverity": null }]),
            json!([{ "id": 3, "effectiveSeverity": "warning" }]),
        ] {
            assert_eq!(compact_lint_rules(Some(&rules)), json!([]), "{rules}");
        }
        assert_eq!(compact_lint_rules(None), json!([]));
    }
}
