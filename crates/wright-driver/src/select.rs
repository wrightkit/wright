//! Finding selection shared by the CLI workflow flags and the
//! `wright-agent/v1` request fields (#430). One selection model drives every
//! finding-shaped output (envelope diagnostics and `result.findings`), so the
//! CLI and the agent contract return the same selected set.
//!
//! Selection only narrows what is *reported*: verdicts and exit codes are
//! computed on the full set before selection applies, and the returned
//! [`SelectionOutcome`] keeps the true total, the truncation count, and the
//! full set's highest severity so no surface can understate the result.

use serde::{Deserialize, Serialize};
use wright_analyzer::registry::{LintConfig, LintRegistry};

use crate::diag::{Diagnostic, Severity};

/// A finding/diagnostic selection: `severity`/`rule`/`file` filter the set,
/// `max` truncates what remains.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingSelection {
    /// Minimum severity reported (`info` selects everything, `error` selects
    /// only errors).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
    /// Select only findings produced by this rule id (the finding `code`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    /// Select only findings located in this source file, matched exactly
    /// against the resolved `span.path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Report at most this many findings after filtering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<usize>,
}

/// The recorded outcome of applying a [`FindingSelection`]: the true total
/// and the count dropped by `max`. `max_severity` preserves the full set's
/// highest severity for verdict/exit computation and is not part of the wire
/// payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SelectionOutcome {
    /// Findings present before selection.
    pub total: usize,
    /// Filtered findings dropped by `max`.
    pub withheld: usize,
    /// Highest severity over the full, unselected set.
    #[serde(skip)]
    pub max_severity: Option<Severity>,
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Info => 0,
        Severity::Warning => 1,
        Severity::Error => 2,
    }
}

/// Serialized findings always carry a known severity; a missing or unknown
/// value falls back to `Warning`, the same conservative default the CLI
/// verdict uses (`SummaryStatus::from_finding_severity`), so malformed data
/// can never understate the true maximum severity.
fn parse_severity(value: Option<&str>) -> Severity {
    match value {
        Some("error") => Severity::Error,
        Some("info") | Some("notice") => Severity::Info,
        _ => Severity::Warning,
    }
}

impl FindingSelection {
    /// Whether any selection dimension is set.
    pub fn is_active(&self) -> bool {
        self.severity.is_some() || self.rule.is_some() || self.file.is_some() || self.max.is_some()
    }

    /// The selection is a usage error when `rule` names no registered lint
    /// rule: silently selecting nothing would report a clean result for a
    /// typo'd id.
    pub fn validate(&self, registry: &LintRegistry) -> Result<(), String> {
        if let Some(rule) = &self.rule {
            let known = registry
                .descriptors(&LintConfig::default())
                .iter()
                .any(|descriptor| descriptor.id == *rule);
            if !known {
                return Err(format!("unknown rule id '{rule}'"));
            }
        }
        Ok(())
    }

    /// Apply the selection to envelope diagnostics.
    pub fn apply_diagnostics(
        &self,
        diagnostics: Vec<Diagnostic>,
    ) -> (Vec<Diagnostic>, Option<SelectionOutcome>) {
        self.apply(diagnostics, |diagnostic| {
            (
                diagnostic.severity,
                diagnostic.code.as_str(),
                diagnostic.span.as_ref().map(|span| span.path.as_str()),
            )
        })
    }

    /// Apply the selection to serialized findings (`code`, `severity`,
    /// `span.path` fields).
    pub fn apply_findings(
        &self,
        findings: Vec<serde_json::Value>,
    ) -> (Vec<serde_json::Value>, Option<SelectionOutcome>) {
        self.apply(findings, |finding| {
            (
                parse_severity(finding.get("severity").and_then(|v| v.as_str())),
                finding.get("code").and_then(|v| v.as_str()).unwrap_or(""),
                finding
                    .get("span")
                    .and_then(|span| span.get("path"))
                    .and_then(|path| path.as_str()),
            )
        })
    }

    fn apply<T>(
        &self,
        items: Vec<T>,
        key: impl Fn(&T) -> (Severity, &str, Option<&str>),
    ) -> (Vec<T>, Option<SelectionOutcome>) {
        if !self.is_active() {
            return (items, None);
        }
        let total = items.len();
        let max_severity = items
            .iter()
            .map(|item| key(item).0)
            .max_by_key(|severity| severity_rank(*severity));
        let mut kept: Vec<T> = items
            .into_iter()
            .filter(|item| {
                let (severity, code, path) = key(item);
                self.severity
                    .is_none_or(|min| severity_rank(severity) >= severity_rank(min))
                    && self.rule.as_ref().is_none_or(|rule| rule == code)
                    && self
                        .file
                        .as_ref()
                        .is_none_or(|file| Some(file.as_str()) == path)
            })
            .collect();
        let withheld = self.max.map_or(0, |max| kept.len().saturating_sub(max));
        if let Some(max) = self.max {
            kept.truncate(max);
        }
        (
            kept,
            Some(SelectionOutcome {
                total,
                withheld,
                max_severity,
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn finding(code: &str, severity: &str, path: Option<&str>) -> serde_json::Value {
        json!({
            "code": code,
            "severity": severity,
            "message": "msg",
            "span": { "path": path },
        })
    }

    fn selection(input: serde_json::Value) -> FindingSelection {
        serde_json::from_value(input).unwrap()
    }

    #[test]
    fn inactive_selection_returns_items_untouched() {
        let items = vec![finding("a", "warning", None)];
        let (kept, outcome) = FindingSelection::default().apply_findings(items.clone());
        assert_eq!(kept, items);
        assert!(outcome.is_none());
    }

    #[test]
    fn severity_threshold_keeps_at_or_above() {
        let items = vec![
            finding("a", "info", None),
            finding("b", "warning", None),
            finding("c", "error", None),
        ];
        let (kept, outcome) = selection(json!({"severity": "warning"})).apply_findings(items);
        assert_eq!(
            kept.iter()
                .map(|f| f["code"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["b", "c"]
        );
        let outcome = outcome.unwrap();
        assert_eq!(outcome.total, 3);
        assert_eq!(outcome.withheld, 0);
        assert_eq!(outcome.max_severity, Some(Severity::Error));
    }

    #[test]
    fn rule_and_file_filter() {
        let items = vec![
            finding("a", "warning", Some("x.ws")),
            finding("a", "warning", Some("y.ws")),
            finding("b", "warning", Some("x.ws")),
        ];
        let (kept, _) = selection(json!({"rule": "a", "file": "x.ws"})).apply_findings(items);
        assert_eq!(kept.len(), 1);
        // No span path never matches a file selection.
        let (kept, _) =
            selection(json!({"file": "x.ws"})).apply_findings(vec![finding("a", "warning", None)]);
        assert!(kept.is_empty());
    }

    #[test]
    fn max_truncates_and_reports_withheld() {
        let items = vec![
            finding("a", "warning", None),
            finding("b", "warning", None),
            finding("c", "warning", None),
            finding("d", "warning", None),
        ];
        let (kept, outcome) = selection(json!({"max": 1})).apply_findings(items);
        assert_eq!(kept.len(), 1);
        let outcome = outcome.unwrap();
        assert_eq!(outcome.total, 4);
        assert_eq!(outcome.withheld, 3);
    }

    #[test]
    fn diagnostics_share_the_same_semantics() {
        let mut items = vec![
            Diagnostic::error("parse-error", crate::diag::Stage::Frontend, "a"),
            Diagnostic::warning("warn-code", crate::diag::Stage::Analysis, "b"),
        ];
        items[0].severity = Severity::Error;
        let (kept, outcome) = selection(json!({"severity": "error"})).apply_diagnostics(items);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].code, "parse-error");
        assert_eq!(outcome.unwrap().max_severity, Some(Severity::Error));
    }

    #[test]
    fn unknown_rule_is_an_error() {
        let registry = LintRegistry::default();
        assert!(
            selection(json!({"rule": "min-wait-loop"}))
                .validate(&registry)
                .is_ok()
        );
        let err = selection(json!({"rule": "not-a-rule"}))
            .validate(&registry)
            .unwrap_err();
        assert!(err.contains("not-a-rule"));
    }

    #[test]
    fn outcome_serializes_only_wire_fields() {
        let outcome = SelectionOutcome {
            total: 4,
            withheld: 3,
            max_severity: Some(Severity::Warning),
        };
        assert_eq!(
            serde_json::to_value(&outcome).unwrap(),
            json!({"total": 4, "withheld": 3})
        );
    }
}
