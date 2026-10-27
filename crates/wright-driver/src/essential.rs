use std::cmp::Reverse;

use serde_json::{Value, json};

use crate::Severity;
use crate::diag::Diagnostic;
use crate::result::FindingMeta;

pub const DEFAULT_ITEMS: usize = 20;
const DEFAULT_FINDING_BYTES: usize = 5500;

fn list<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value[key].as_array().map_or(&[], Vec::as_slice)
}
fn number(value: &Value, key: &str) -> u64 {
    value[key].as_u64().unwrap_or(0)
}
/// Per-severity counts over findings whose typed identities the caller
/// supplies (#1001): `meta` parallels `items` index for index (the
/// `GetFindings`/selection order), so the projection never re-parses the
/// fields it projects. A short `meta` is a caller bug — asserted in debug,
/// unknowns count as warnings.
fn counts(items: &[Value], meta: &[FindingMeta]) -> Value {
    debug_assert_eq!(items.len(), meta.len(), "finding meta parallels the rows");
    let severity = |index: usize| {
        meta.get(index)
            .map(|meta| meta.severity)
            .unwrap_or(Severity::Warning)
    };
    let count = |wanted| {
        (0..items.len())
            .filter(|&index| severity(index) == wanted)
            .count()
    };
    let mut group = json!({
        "total": items.len(),
        "error": count(Severity::Error),
        "warning": count(Severity::Warning),
        "info": count(Severity::Info),
    });
    if let Some(by_code) = by_code_counts(items) {
        group["byCode"] = by_code;
    }
    group
}

fn by_code_counts(items: &[Value]) -> Option<Value> {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for item in items {
        if let Some(code) = item["code"].as_str() {
            *counts.entry(code.to_string()).or_default() += 1;
        }
    }
    (!counts.is_empty()).then(|| json!(counts))
}
fn fields(value: &Value, keys: &[&str]) -> Value {
    let mut result = serde_json::Map::new();
    for key in keys {
        if let Some(value) = value.get(*key).filter(|v| !v.is_null()) {
            result.insert((*key).into(), value.clone());
        }
    }
    Value::Object(result)
}
/// The essential finding rows paired with their typed identities
/// (#1001): each emitted row keeps its `FindingMeta` so renderers read
/// severities from the meta, not the projection. Sorted by the typed
/// severity, most severe first — the same order the untyped projection
/// produced.
fn findings(items: &[Value], meta: &[FindingMeta]) -> Vec<(Value, FindingMeta)> {
    debug_assert_eq!(items.len(), meta.len(), "finding meta parallels the rows");
    let mut items = items
        .iter()
        .zip(meta.iter().map(Some).chain(std::iter::repeat(None)))
        .map(|(finding, meta)| {
            let meta = meta.cloned().unwrap_or(FindingMeta {
                severity: Severity::Warning,
                code: finding["code"].as_str().unwrap_or_default().to_string(),
                span_file: None,
            });
            let mut item = fields(
                finding,
                &[
                    "code",
                    "severity",
                    "message",
                    "evidence",
                    "boundedness",
                    "repeated",
                ],
            );
            if let Some(path) = finding["span"]["path"].as_str() {
                let start = &finding["span"]["start"];
                item["location"] = json!(format!(
                    "{}:{}:{}",
                    path,
                    number(start, "line"),
                    number(start, "col")
                ));
            }
            (item, meta)
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|(_, meta)| Reverse(meta.severity));
    items
}

/// `cap` over a finding pair list: truncate rows and metas together and
/// record the truncation note under `key`, exactly as `cap` does on a
/// bare array.
fn cap_pairs(
    output: &mut Value,
    pairs: &mut Vec<(Value, FindingMeta)>,
    key: &str,
    limit: usize,
    all: bool,
) {
    let total = pairs.len();
    if all || total <= limit {
        return;
    }
    let by_code = by_code_counts(
        &pairs
            .iter()
            .map(|(item, _)| item.clone())
            .collect::<Vec<_>>(),
    );
    pairs.truncate(limit);
    let mut note =
        json!({"total":total,"shown":limit,"truncated":total-limit,"expand":"--all / all: true"});
    if let Some(by_code) = by_code {
        note["byCode"] = by_code;
    }
    output["truncation"][key] = note;
}

/// `fit_finding_bytes` over a finding pair list: pop rows and metas
/// together until the serialized output fits the default budget.
fn fit_finding_pairs(
    output: &mut Value,
    pairs: &mut Vec<(Value, FindingMeta)>,
    key: &str,
    total: usize,
) {
    let by_code = by_code_counts(
        &pairs
            .iter()
            .map(|(item, _)| item.clone())
            .collect::<Vec<_>>(),
    );
    while serde_json::to_vec(output)
        .expect("presentation serializes")
        .len()
        > DEFAULT_FINDING_BYTES
    {
        if pairs.pop().is_none() {
            break;
        }
        output[key] = Value::Array(pairs.iter().map(|(item, _)| item.clone()).collect());
        let shown = pairs.len();
        let note = &mut output["truncation"][key];
        if note["total"].is_null() {
            note["total"] = json!(total);
        }
        if note["byCode"].is_null()
            && let Some(by_code) = &by_code
        {
            note["byCode"] = by_code.clone();
        }
        note["shown"] = json!(shown);
        note["truncated"] = json!(note["total"].as_u64().unwrap_or(total as u64) - shown as u64);
        note["expand"] = json!("--all / all: true");
    }
}

fn cap(result: &mut Value, key: &str, limit: usize, all: bool) {
    let Some(items) = result[key].as_array_mut() else {
        return;
    };
    let total = items.len();
    if !all && total > limit {
        let by_code = by_code_counts(items);
        items.truncate(limit);
        let mut note = json!({"total":total,"shown":limit,"truncated":total-limit,"expand":"--all / all: true"});
        if let Some(by_code) = by_code {
            note["byCode"] = by_code;
        }
        result["truncation"][key] = note;
    }
}

fn fit_finding_bytes(output: &mut Value, total: usize) {
    let by_code = output["findings"]
        .as_array()
        .and_then(|findings| by_code_counts(findings));
    while serde_json::to_vec(output)
        .expect("presentation serializes")
        .len()
        > DEFAULT_FINDING_BYTES
    {
        let shown = {
            let findings = output["findings"]
                .as_array_mut()
                .expect("findings is a list");
            if findings.pop().is_none() {
                break;
            }
            findings.len()
        };
        let note = &mut output["truncation"]["findings"];
        if note["total"].is_null() {
            note["total"] = json!(total);
        }
        if note["byCode"].is_null()
            && let Some(by_code) = &by_code
        {
            note["byCode"] = by_code.clone();
        }
        note["shown"] = json!(shown);
        note["truncated"] = json!(note["total"].as_u64().unwrap_or(total as u64) - shown as u64);
        note["expand"] = json!("--all / all: true");
    }
}

/// The `lint` essential projection and the typed identities of the
/// emitted `findings` rows (#1001): `meta` parallels `result.findings`
/// (the `GetFindings`/selection order); the returned metas parallel the
/// emitted rows after sorting, capping, and byte-fitting, so renderers
/// stay typed end to end.
pub fn lint(result: &Value, meta: &[FindingMeta], all: bool) -> (Value, Vec<FindingMeta>) {
    let items = list(result, "findings");
    let mut pairs = findings(items, meta);
    let mut output = json!({"counts":{"findings":counts(items, meta)},"expand":"lint --all / all: true; lintRules for effective configuration and rule metadata","next":crate::result::next::LINT});
    for key in ["ok", "exit", "selection", "fixes", "next", "snapshot"] {
        if let Some(value) = result.get(key) {
            output[key] = value.clone();
        }
    }
    if let Some(identity) = result
        .get("inputIdentity")
        .or_else(|| result.get("input_identity"))
    {
        output["inputIdentity"] = identity.clone();
    }
    cap_pairs(&mut output, &mut pairs, "findings", DEFAULT_ITEMS, all);
    output["findings"] = Value::Array(pairs.iter().map(|(item, _)| item.clone()).collect());
    if !all {
        fit_finding_pairs(&mut output, &mut pairs, "findings", items.len());
    }
    let metas = pairs.into_iter().map(|(_, meta)| meta).collect();
    (output, metas)
}

pub fn inspect(result: &Value, all: bool) -> Value {
    let mut output = json!({"program":result["program"],"counts":{"rules":list(result,"rules").len(),"symbols":list(result,"symbols").len(),"references":list(result,"references").iter().map(|v|v.as_array().map_or(0,Vec::len)).sum::<usize>()},"rules":list(result,"rules").iter().map(|v|fields(v,&["id","name","address"])).collect::<Vec<_>>(),"expand":"inspect symbols, refs, cfg, callgraph, cost; --all / all: true for all rule names","next":crate::result::next::INSPECT});
    cap(&mut output, "rules", DEFAULT_ITEMS, all);
    output
}

/// The `analyze` essential projection and the typed identities of the
/// emitted `risks` rows — the same pairing contract as [`lint`].
pub fn analyze(result: &Value, risks_meta: &[FindingMeta], all: bool) -> (Value, Vec<FindingMeta>) {
    let facts = &result["facts"];
    let rules = list(facts, "rules");
    let risks = list(facts, "risks");
    let mut hotspots = rules
        .iter()
        .map(|rule| {
            let mut item = fields(
                rule,
                &["id", "name", "address", "elements", "controlFlow", "span"],
            );
            item["risks"] = json!(
                risks
                    .iter()
                    .filter(|risk| risk["rule"] == rule["id"])
                    .count()
            );
            item
        })
        .collect::<Vec<_>>();
    let hotspot_key = |rule: &Value| {
        rule["elements"].as_u64().unwrap_or_else(|| {
            number(&rule["controlFlow"], "blocks") + number(&rule["controlFlow"], "edges")
        })
    };
    hotspots.sort_by(|l, r| {
        hotspot_key(r)
            .cmp(&hotspot_key(l))
            .then_with(|| l["name"].as_str().cmp(&r["name"].as_str()))
    });
    let mut complexity = json!({"blocks":0,"edges":0,"loopBlocks":0,"waitBlocks":0});
    for rule in rules {
        for key in ["blocks", "edges", "loopBlocks", "waitBlocks"] {
            complexity[key] = json!(number(&complexity, key) + number(&rule["controlFlow"], key));
        }
    }
    let mut trees = rules
        .iter()
        .flat_map(|rule| {
            list(rule, "conditions")
                .iter()
                .filter(|c| number(c, "elements") > 0)
                .map(move |condition| {
                    let mut item = fields(condition, &["index", "elements", "span"]);
                    item["rule"] = rule["name"].clone();
                    item["ruleAddress"] = rule["address"].clone();
                    item
                })
        })
        .collect::<Vec<_>>();
    trees.sort_by(|l, r| {
        number(r, "elements")
            .cmp(&number(l, "elements"))
            .then_with(|| l["rule"].as_str().cmp(&r["rule"].as_str()))
            .then_with(|| number(l, "index").cmp(&number(r, "index")))
    });
    let mut variables = list(facts, "symbols")
        .iter()
        .filter(|s| {
            matches!(
                s["kind"].as_str(),
                Some("globalVariable" | "playerVariable")
            )
        })
        .map(|v| fields(v, &["name", "kind", "usage"]))
        .collect::<Vec<_>>();
    variables.sort_by(|l, r| {
        number(&r["usage"], "rules")
            .cmp(&number(&l["usage"], "rules"))
            .then_with(|| {
                (number(&r["usage"], "reads") + number(&r["usage"], "writes"))
                    .cmp(&(number(&l["usage"], "reads") + number(&l["usage"], "writes")))
            })
            .then_with(|| l["name"].as_str().cmp(&r["name"].as_str()))
    });
    let objects = list(facts, "persistentObjects");
    let mut by_kind = std::collections::BTreeMap::<&str, usize>::new();
    for object in objects {
        *by_kind
            .entry(object["kind"].as_str().unwrap_or("object"))
            .or_default() += 1;
    }
    let object_summary = json!({"total":objects.len(),"byKind":by_kind,"withoutRetainedIdentity":objects.iter().filter(|object|object["identityRetained"]==false).count(),"visibleToAllPlayers":objects.iter().filter(|object|object["visibility"]=="all-players").count()});
    let mut risk_pairs = findings(risks, risks_meta);
    let mut output = json!({"program":result["program"],"cost":facts["cost"],"complexity":complexity,"hotspots":hotspots,"conditionTrees":trees,"variables":variables,"persistentObjects":object_summary,"counts":{"risks":counts(risks, risks_meta),"persistentObjects":list(facts,"persistentObjects").len()},"expand":"--all / all: true; inspect symbols, refs, cfg, callgraph, cost for detail","next":crate::result::next::ANALYZE});
    if let Some(selection) = result.get("selection") {
        output["selection"] = selection.clone();
    }
    for (key, limit) in [("hotspots", 5), ("conditionTrees", 3), ("variables", 5)] {
        cap(&mut output, key, limit, all);
    }
    cap_pairs(&mut output, &mut risk_pairs, "risks", 5, all);
    output["risks"] = Value::Array(risk_pairs.iter().map(|(item, _)| item.clone()).collect());
    let metas = risk_pairs.into_iter().map(|(_, meta)| meta).collect();
    (output, metas)
}

/// Cap project-sized query lists after explicit selections, preserving their totals.
pub fn query(result: &Value, all: bool) -> Value {
    let mut output = result.clone();
    if output.is_array() {
        if !all && output.as_array().is_some_and(|v| v.len() > DEFAULT_ITEMS) {
            output = json!({"items":output});
            cap(&mut output, "items", DEFAULT_ITEMS, all);
        }
        return output;
    }
    if output["selection"]["withheld"]
        .as_u64()
        .is_some_and(|count| count > 0)
    {
        output["expand"] =
            json!("--all / all: true for the default cap; increase max for an explicit selection");
    }
    if let Some(object) = output.as_object() {
        let keys = object
            .iter()
            .filter(|(_, v)| v.is_array())
            .map(|(k, _)| k.clone())
            .collect::<Vec<_>>();
        for key in keys {
            cap(&mut output, &key, DEFAULT_ITEMS, all);
        }
        let nested = output
            .as_object()
            .unwrap()
            .iter()
            .filter(|(key, value)| *key != "truncation" && value.is_object())
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in nested {
            output[&key] = query(&output[&key], all);
        }
    }
    output
}

/// The payload shape selects which arrays `cap` truncates, probed in the
/// same order the untagged variants matched: a rule-shaped cost first
/// (`rule` object + `statements`/`byNode` arrays), then a program's
/// `elementCount` (an object always carrying a `budget`).
pub fn cost(result: &Value, all: bool) -> Value {
    let mut output = result.clone();
    if result["rule"].is_object() && result["statements"].is_array() && result["byNode"].is_array()
    {
        cap(&mut output, "statements", 5, all);
        cap(&mut output, "byNode", 5, all);
        return output;
    }
    let element_count = &result["elementCount"];
    if !element_count.is_object() || element_count["budget"].as_u64().is_none() {
        return output;
    }
    if element_count["total"].as_u64().is_some() && element_count["rules"].is_array() {
        cap(&mut output["elementCount"], "rules", 5, all);
    }
    let mut output = query(&output, all);
    if !all && let Some(findings) = output.get("findings").and_then(Value::as_array) {
        let total = findings.len();
        fit_finding_bytes(&mut output, total);
    }
    output
}

/// Preserve the complete-set verdict while bounding its displayed
/// diagnostics. `diagnostics` carries the envelope's typed diagnostics
/// (#1001) — the projection sorts them by `Diagnostic::severity`, never by
/// re-parsing the serialized label.
pub fn envelope(result: &mut Value, diagnostics: &[Diagnostic], all: bool) {
    if let Some(values) = result["diagnostics"].as_array_mut() {
        let mut order: Vec<usize> = (0..values.len()).collect();
        order.sort_by_key(|&index| {
            diagnostics
                .get(index)
                .map(|diagnostic| Reverse(diagnostic.severity))
                .unwrap_or(Reverse(Severity::Warning))
        });
        let mut sorted = Vec::with_capacity(values.len());
        for index in order {
            sorted.push(values[index].clone());
        }
        *values = sorted;
    }
    cap(result, "diagnostics", DEFAULT_ITEMS, all);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn meta(severity: Severity) -> FindingMeta {
        FindingMeta {
            severity,
            code: String::new(),
            span_file: None,
        }
    }

    #[test]
    fn capped_findings_report_complete_counts_and_expansion() {
        let findings=(0..DEFAULT_ITEMS+1).map(|i|json!({"severity":if i==DEFAULT_ITEMS {"error"} else {"warning"},"code":format!("c{i}"),"message":"fix"})).collect::<Vec<_>>();
        let meta = findings
            .iter()
            .map(|f| meta(Severity::from_label(f["severity"].as_str())))
            .collect::<Vec<_>>();
        let input = json!({"findings":findings,"config":{"huge":true}});
        let (result, metas) = lint(&input, &meta, false);
        assert_eq!(metas.len(), DEFAULT_ITEMS);
        assert_eq!(result["counts"]["findings"]["error"], 1);
        assert_eq!(result["findings"][0]["severity"], "error");
        assert_eq!(
            result["counts"]["findings"]["byCode"]["c0"], 1,
            "counts carry per-code totals: {}",
            result["counts"]["findings"]
        );
        assert_eq!(
            result["truncation"]["findings"]["byCode"],
            result["counts"]["findings"]["byCode"]
        );
        assert_eq!(result["truncation"]["findings"]["total"], DEFAULT_ITEMS + 1);
        assert!(
            result["truncation"]["findings"]["expand"]
                .as_str()
                .unwrap()
                .contains("all")
        );
        assert_eq!(
            lint(&input, &meta, true).0["findings"]
                .as_array()
                .unwrap()
                .len(),
            DEFAULT_ITEMS + 1
        );
        assert!(result.get("config").is_none());
    }
    #[test]
    fn diagnostics_cap_preserves_verdict_and_all_recovers_every_item() {
        let input = json!({"ok":false,"exit":1,"diagnostics":vec![json!({"severity":"error"});DEFAULT_ITEMS+1]});
        let diagnostics =
            vec![Diagnostic::error("x", crate::Stage::Analysis, "x"); DEFAULT_ITEMS + 1];
        let mut result = input.clone();
        envelope(&mut result, &diagnostics, false);
        assert_eq!(result["ok"], false);
        assert_eq!(result["exit"], 1);
        assert_eq!(
            result["truncation"]["diagnostics"]["total"],
            DEFAULT_ITEMS + 1
        );
        envelope(&mut result, &diagnostics, true);
        let mut complete = input;
        envelope(&mut complete, &diagnostics, true);
        assert_eq!(
            complete["diagnostics"].as_array().unwrap().len(),
            DEFAULT_ITEMS + 1
        );
    }
    #[test]
    fn query_caps_each_requested_list_with_recoverable_totals() {
        let items = (0..DEFAULT_ITEMS + 1)
            .map(|id| json!({"id": id}))
            .collect::<Vec<_>>();
        let input = json!({"blocks": items, "detail": {"edges": items}});
        let bounded = query(&input, false);
        for (list, note) in [
            (&bounded["blocks"], &bounded["truncation"]["blocks"]),
            (
                &bounded["detail"]["edges"],
                &bounded["detail"]["truncation"]["edges"],
            ),
        ] {
            assert_eq!(list.as_array().unwrap().len(), DEFAULT_ITEMS);
            assert_eq!(note["total"], DEFAULT_ITEMS + 1);
            assert!(note["expand"].as_str().unwrap().contains("all"));
        }
        assert_eq!(query(&input, true), input);
        let bare = json!(items);
        assert_eq!(
            query(&bare, false)["truncation"]["items"]["total"],
            DEFAULT_ITEMS + 1
        );
        assert!(query(&bare, false)["truncation"]["items"]["byCode"].is_null());
        assert_eq!(query(&bare, true), bare);
    }
    #[test]
    fn byte_fit_keeps_the_full_set_per_code_counts() {
        let findings = (0..6)
            .map(|id| {
                json!({"code":format!("risk-{}",id%2),"severity":"error","message":"long actionable evidence ".repeat(120)})
            })
            .collect::<Vec<_>>();
        let meta = vec![meta(Severity::Error); 6];
        let (result, _) = lint(&json!({"findings": findings}), &meta, false);
        let note = &result["truncation"]["findings"];
        assert_eq!(note["total"], 6);
        assert_eq!(note["byCode"], json!({"risk-0": 3, "risk-1": 3}));
        assert_eq!(
            note["shown"].as_u64().unwrap() + note["truncated"].as_u64().unwrap(),
            6
        );
    }
    #[test]
    fn cost_keeps_the_five_largest_rule_shares_by_default() {
        let rules = (0..DEFAULT_ITEMS + 1)
            .map(|id| json!({"index": id, "name": format!("r{id}"), "count": id}))
            .collect::<Vec<_>>();
        let input = json!({"elementCount":{"total":1,"budget":32768,"rules":rules},"findings":[]});
        let bounded = cost(&input, false);
        let kept = bounded["elementCount"]["rules"].as_array().unwrap();
        assert_eq!(kept.len(), 5);
        let note = &bounded["elementCount"]["truncation"]["rules"];
        assert_eq!(note["total"], DEFAULT_ITEMS + 1);
        assert_eq!(note["shown"], 5);
        assert_eq!(cost(&input, true), input);
    }
    #[test]
    fn unavailable_cost_preserves_the_bare_budget_without_fabricating_rules() {
        let input = json!({
            "elementCount": {"budget": 32768},
            "elementCountUnavailable": "unsupported element",
            "findings": [],
            "snapshot": 3,
        });
        assert_eq!(cost(&input, false), input);
        assert_eq!(cost(&input, true), input);
        assert!(cost(&input, false)["elementCount"].get("rules").is_none());
    }

    #[test]
    fn rule_cost_caps_each_breakdown_and_preserves_extension_fields() {
        let items = (0..7)
            .map(|index| json!({"count": index}))
            .collect::<Vec<_>>();
        let input = json!({
            "rule": {"name": "r", "index": 0},
            "statements": items,
            "byNode": items,
            "snapshot": 4,
            "extension": {"data": [1, 2, 3]},
        });
        let bounded = cost(&input, false);
        for key in ["statements", "byNode"] {
            assert_eq!(bounded[key].as_array().unwrap().len(), 5);
            assert_eq!(bounded["truncation"][key]["total"], 7);
        }
        assert_eq!(bounded["extension"], input["extension"]);
        assert_eq!(bounded["snapshot"], 4);
        assert_eq!(cost(&input, true), input);
    }

    #[test]
    fn long_findings_fit_the_default_budget_without_changing_the_verdict() {
        let input = json!({"ok":false,"exit":1,"findings":(0..4).map(|id|json!({"code":format!("risk-{id}"),"severity":"error","message":"long actionable evidence ".repeat(120)})).collect::<Vec<_>>()});
        let meta = vec![meta(Severity::Error); 4];
        let (result, _) = lint(&input, &meta, false);
        assert!(serde_json::to_vec(&result).unwrap().len() <= DEFAULT_FINDING_BYTES);
        assert_eq!(result["ok"], false);
        assert_eq!(result["exit"], 1);
        assert_eq!(result["counts"]["findings"]["error"], 4);
        assert_eq!(result["truncation"]["findings"]["total"], 4);
        let shown = result["findings"].as_array().unwrap().len();
        assert!(shown < 4);
        assert_eq!(result["truncation"]["findings"]["shown"], shown);
        assert!(
            result["truncation"]["findings"]["expand"]
                .as_str()
                .unwrap()
                .contains("all")
        );
        let (complete, _) = lint(&input, &meta, true);
        assert_eq!(complete["findings"], input["findings"]);
        assert!(complete.get("truncation").is_none());
    }
}
