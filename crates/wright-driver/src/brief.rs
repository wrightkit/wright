//! The `brief` result form (#532): counts, the highest-priority items, and
//! how to expand — the full result stays one option away. `analyze`,
//! `inspect`, and `lint` accept a `brief` request field (the CLI's
//! `--brief`); the same transforms produce the form on every surface, so
//! the CLI, `serve`, and the MCP adapter return the same brief result.
//!
//! The form is a small object `{brief: true, program?, counts, items,
//! expand}`: `program` is the operation's existing program summary when it
//! carries one, `counts` totals the collections the full result reports,
//! `items` holds at most [`BRIEF_ITEMS`] highest-priority entries, and
//! `expand` names the way back to the full result. A `lint` result
//! produced under a selection keeps its `selection` member so the
//! pre-selection total stays visible.

use serde_json::{Value, json};

/// Entries a brief's `items` array carries at most.
const BRIEF_ITEMS: usize = 5;

/// The presentation severity order — errors, then warnings, then
/// informational; unknown severities sort as warnings, matching the CLI's
/// conservative finding order.
fn severity_rank(finding: &Value) -> u8 {
    match finding["severity"].as_str() {
        Some("error") => 0,
        Some("info") | Some("notice") => 2,
        _ => 1,
    }
}

/// `{"total", "error", "warning", "info"}` over a finding list.
fn severity_counts(findings: &[Value]) -> Value {
    let mut counts = [0usize; 3];
    for finding in findings {
        counts[severity_rank(finding) as usize] += 1;
    }
    json!({
        "total": findings.len(),
        "error": counts[0],
        "warning": counts[1],
        "info": counts[2],
    })
}

/// The highest-severity findings, stable within one severity, at most
/// [`BRIEF_ITEMS`]. Each entry keeps the actionable members — severity,
/// code, message, evidence, and span; index/repetition internals stay in
/// the full result.
fn top_findings(findings: &[Value]) -> Vec<Value> {
    let mut sorted = findings.to_vec();
    sorted.sort_by_key(severity_rank);
    sorted.truncate(BRIEF_ITEMS);
    for finding in &mut sorted {
        if let Some(object) = finding.as_object_mut() {
            object.retain(|key, _| {
                matches!(
                    key.as_str(),
                    "severity" | "code" | "message" | "evidence" | "span"
                )
            });
        }
    }
    sorted
}

/// `result[key]` as an array slice, empty when absent or not a list.
fn list<'a>(result: &'a Value, key: &str) -> Vec<&'a Value> {
    result[key]
        .as_array()
        .map_or(Vec::new(), |items| items.iter().collect())
}

/// The `analyze` brief: the program summary, collection counts including
/// the element total and risk severities, and the costliest rules — the
/// ranked semantic report's headline. Per-condition element detail is an
/// expansion concern and is trimmed from the reported items.
pub fn analyze(result: &Value) -> Value {
    let facts = &result["facts"];
    let rules = list(facts, "rules");
    let mut items: Vec<Value> = rules.iter().map(|rule| (*rule).clone()).collect();
    items.sort_by_key(|rule| std::cmp::Reverse(rule["elements"].as_u64().unwrap_or(0)));
    items.truncate(BRIEF_ITEMS);
    for item in &mut items {
        if let Some(object) = item.as_object_mut() {
            object.retain(|key, _| matches!(key.as_str(), "id" | "name" | "elements" | "span"));
        }
    }
    let mut counts = json!({
        "rules": rules.len(),
        "symbols": list(facts, "symbols").len(),
        "persistentObjects": list(facts, "persistentObjects").len(),
        "risks": severity_counts(&list(facts, "risks").into_iter().cloned().collect::<Vec<_>>()),
    });
    if let Some(elements) = facts["cost"]["elementCount"].as_u64() {
        counts["elements"] = json!(elements);
    }
    let mut brief = json!({
        "brief": true,
        "counts": counts,
        "items": items,
        "expand": "drop the brief option for the full ranked report and every risk finding",
    });
    if let Some(program) = result.get("program") {
        brief["program"] = program.clone();
    }
    brief
}

/// The `inspect` brief: the program summary, totals for the result's
/// collections, and the leading rule entries — the program's primary
/// units. Per-symbol reference detail is the expansion.
pub fn inspect(result: &Value) -> Value {
    let rules = list(result, "rules");
    let references = list(result, "references");
    let reference_count: usize = references
        .iter()
        .map(|entry| entry.as_array().map_or(0, Vec::len))
        .sum();
    let mut brief = json!({
        "brief": true,
        "counts": {
            "rules": rules.len(),
            "symbols": list(result, "symbols").len(),
            "references": reference_count,
        },
        "items": rules.into_iter().take(BRIEF_ITEMS).cloned().collect::<Vec<_>>(),
        "expand": "drop the brief option for the full result; the symbols, references, usage, cfg, and callGraph operations answer narrower questions",
    });
    if let Some(program) = result.get("program") {
        brief["program"] = program.clone();
    }
    brief
}

/// The `lint` brief: finding counts by severity, the evaluated and skipped
/// rule counts, and the highest-severity findings. A `selection` member,
/// when the request selected, carries over with its pre-selection total.
pub fn lint(result: &Value) -> Value {
    let findings: Vec<Value> = list(result, "findings").into_iter().cloned().collect();
    let mut brief = json!({
        "brief": true,
        "counts": {
            "findings": severity_counts(&findings),
            "rules": list(result, "rules").len(),
            "skipped": list(result, "skipped").len(),
        },
        "items": top_findings(&findings),
        "expand": "drop the brief option for every finding, or select findings by severity, rule, file, and max",
    });
    if let Some(selection) = result.get("selection") {
        brief["selection"] = selection.clone();
    }
    brief
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lint_brief_counts_and_bounds_findings() {
        let result = json!({
            "inputIdentity": "abc",
            "rules": [{"id": "r1"}, {"id": "r2"}],
            "config": {},
            "findings": [
                {"severity": "info", "code": "c1"},
                {"severity": "error", "code": "c2"},
                {"severity": "warning", "code": "c3"},
                {"severity": "warning", "code": "c4"},
                {"severity": "warning", "code": "c5"},
                {"severity": "warning", "code": "c6"},
                {"severity": "info", "code": "c7"},
            ],
            "skipped": [{"id": "s1"}],
        });
        let brief = lint(&result);
        assert_eq!(brief["brief"], json!(true));
        assert_eq!(
            brief["counts"]["findings"],
            json!({"total": 7, "error": 1, "warning": 4, "info": 2})
        );
        assert_eq!(brief["counts"]["rules"], json!(2));
        assert_eq!(brief["counts"]["skipped"], json!(1));
        let items = brief["items"].as_array().unwrap();
        assert_eq!(items.len(), BRIEF_ITEMS);
        assert_eq!(items[0]["code"], "c2", "errors lead the brief items");
        assert_eq!(items[1]["code"], "c3", "then warnings in source order");
        assert!(brief["expand"].as_str().unwrap().contains("drop"));
        assert!(brief.get("selection").is_none());
    }

    #[test]
    fn lint_brief_keeps_the_selection_member() {
        let result = json!({
            "rules": [],
            "findings": [{"severity": "warning", "code": "c"}],
            "skipped": [],
            "selection": {"total": 9, "withheld": 8},
        });
        let brief = lint(&result);
        assert_eq!(brief["selection"], json!({"total": 9, "withheld": 8}));
    }

    #[test]
    fn analyze_brief_ranks_rules_by_elements() {
        let result = json!({
            "program": {"rules": 2},
            "facts": {
                "symbols": [{"id": 0}],
                "rules": [
                    {"id": 0, "name": "small", "elements": 5, "conditions": [{"index": 0}]},
                    {"id": 1, "name": "big", "elements": 50},
                ],
                "cost": {"elementCount": 55},
                "risks": [{"severity": "warning"}, {"severity": "error"}],
                "persistentObjects": [],
            },
        });
        let brief = analyze(&result);
        assert_eq!(brief["program"], json!({"rules": 2}));
        assert_eq!(brief["counts"]["elements"], json!(55));
        assert_eq!(
            brief["counts"]["risks"],
            json!({"total": 2, "error": 1, "warning": 1, "info": 0})
        );
        let items = brief["items"].as_array().unwrap();
        assert_eq!(items[0]["name"], "big", "the costliest rule leads");
        assert!(
            items[0].get("conditions").is_none(),
            "condition detail stays in the full result"
        );
    }

    #[test]
    fn inspect_brief_reports_collection_totals() {
        let result = json!({
            "program": {"rules": 1},
            "rules": [{"id": 0, "name": "r", "span": {}}],
            "symbols": [{"id": 0}, {"id": 1}],
            "references": [[{"kind": "read"}, {"kind": "write"}], [{"kind": "call"}]],
        });
        let brief = inspect(&result);
        assert_eq!(
            brief["counts"],
            json!({"rules": 1, "symbols": 2, "references": 3})
        );
        assert_eq!(brief["items"].as_array().unwrap().len(), 1);
    }
}
