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
    pub diagnostics: Vec<Diagnostic>,
    pub result: T,
}

pub fn exit_code_from(diagnostics: &[Diagnostic]) -> u8 {
    let mut has_source_error = false;
    for d in diagnostics {
        if d.severity != Severity::Error {
            continue;
        }
        if d.stage == crate::diag::Stage::Internal {
            return exit::INTERNAL;
        }
        if d.stage == crate::diag::Stage::Reconstruction
            || d.code == "adapter-stdin-unsupported"
            || d.code == "source-provider-unsupported"
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

#[derive(Debug, Clone, Default, Serialize)]
pub struct CheckResult {}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AnalyzeResult {
    pub program: serde_json::Value,
    pub facts: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct InspectResult {
    pub program: serde_json::Value,
    pub rules: serde_json::Value,
    pub symbols: serde_json::Value,
    pub references: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LintResult {
    pub input_identity: String,
    pub program: serde_json::Value,
    pub rules: serde_json::Value,
    pub config: serde_json::Value,
    pub findings: serde_json::Value,
    pub skipped: serde_json::Value,
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
