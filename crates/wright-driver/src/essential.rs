//! Shared CLI and agent presentation; typed session results retain complete facts.
use serde_json::{Value, json};

pub const DEFAULT_ITEMS: usize = 20;
const DEFAULT_FINDING_BYTES: usize = 5500;

fn list<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value[key].as_array().map_or(&[], Vec::as_slice)
}
fn number(value: &Value, key: &str) -> u64 {
    value[key].as_u64().unwrap_or(0)
}
fn severity(value: &Value) -> u8 {
    match value["severity"].as_str() {
        Some("error") => 0,
        Some("info" | "notice") => 2,
        _ => 1,
    }
}
fn counts(items: &[Value]) -> Value {
    let mut n = [0usize; 3];
    for item in items {
        n[severity(item) as usize] += 1;
    }
    let mut group = json!({"total":items.len(),"error":n[0],"warning":n[1],"info":n[2]});
    // #791: per-code totals over the counted set — the same population
    // `total` reports, broken down so a renderer need not recount a list
    // it may only see capped.
    if let Some(by_code) = by_code_counts(items) {
        group["byCode"] = by_code;
    }
    group
}

/// Per-code totals over a coded list (`code` members: findings,
/// diagnostics), counted on the set as passed — before any truncation the
/// caller applies (#791). `None` for lists whose items carry no `code`
/// (rules, symbols, cfg blocks); their truncation notes stay unchanged.
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
fn findings(items: &[Value]) -> Vec<Value> {
    let mut items = items
        .iter()
        .map(|finding| {
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
            item
        })
        .collect::<Vec<_>>();
    items.sort_by_key(severity);
    items
}

fn cap(result: &mut Value, key: &str, limit: usize, all: bool) {
    let Some(items) = result[key].as_array_mut() else {
        return;
    };
    let total = items.len();
    if !all && total > limit {
        // #791: the note's per-code counts describe the full capped list,
        // so `truncated` can be attributed per code.
        let by_code = by_code_counts(items);
        items.truncate(limit);
        let mut note = json!({"total":total,"shown":limit,"truncated":total-limit,"expand":"--all / all: true"});
        if let Some(by_code) = by_code {
            note["byCode"] = by_code;
        }
        result["truncation"][key] = note;
    }
}

/// Pop `findings` until the serialized output fits the ~1.5k-token budget,
/// recording the truncation so `--all` / `all: true` expands the full set.
/// An item cap that already ran left its note — the fuller set's `total`
/// and per-code counts (#791) survive; only `shown`/`truncated` move.
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
        if note["byCode"].is_null() {
            if let Some(by_code) = &by_code {
                note["byCode"] = by_code.clone();
            }
        }
        note["shown"] = json!(shown);
        note["truncated"] = json!(note["total"].as_u64().unwrap_or(total as u64) - shown as u64);
        note["expand"] = json!("--all / all: true");
    }
}

pub fn lint(result: &Value, all: bool) -> Value {
    let items = list(result, "findings");
    let mut output = json!({"counts":{"findings":counts(items)},"findings":findings(items),"expand":"lint --all / all: true; lintRules for effective configuration and rule metadata"});
    for key in ["ok", "exit", "selection", "fixes"] {
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
    cap(&mut output, "findings", DEFAULT_ITEMS, all);
    if !all {
        fit_finding_bytes(&mut output, items.len());
    }
    output
}

pub fn inspect(result: &Value, all: bool) -> Value {
    let mut output = json!({"program":result["program"],"counts":{"rules":list(result,"rules").len(),"symbols":list(result,"symbols").len(),"references":list(result,"references").iter().map(|v|v.as_array().map_or(0,Vec::len)).sum::<usize>()},"rules":list(result,"rules").iter().map(|v|fields(v,&["id","name"])).collect::<Vec<_>>(),"expand":"inspect symbols, refs, cfg, callgraph, cost; --all / all: true for all rule names"});
    cap(&mut output, "rules", DEFAULT_ITEMS, all);
    output
}

pub fn analyze(result: &Value, all: bool) -> Value {
    let facts = &result["facts"];
    let rules = list(facts, "rules");
    let risks = list(facts, "risks");
    let mut hotspots = rules
        .iter()
        .map(|rule| {
            let mut item = fields(rule, &["id", "name", "elements", "controlFlow", "span"]);
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
    let mut output = json!({"program":result["program"],"cost":facts["cost"],"complexity":complexity,"hotspots":hotspots,"conditionTrees":trees,"risks":findings(risks),"variables":variables,"persistentObjects":object_summary,"counts":{"risks":counts(risks),"persistentObjects":list(facts,"persistentObjects").len()},"expand":"--all / all: true; inspect symbols, refs, cfg, callgraph, cost for detail"});
    if let Some(selection) = result.get("selection") {
        output["selection"] = selection.clone();
    }
    for (key, limit) in [
        ("hotspots", 5),
        ("conditionTrees", 3),
        ("variables", 5),
        ("risks", 5),
    ] {
        cap(&mut output, key, limit, all);
    }
    output
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

/// `elementCount.rules` is already ranked largest-first by the service, so the
/// default view keeps the five largest like analyze hotspots; the generic cap
/// would otherwise keep twenty.
pub fn cost(result: &Value, all: bool) -> Value {
    let mut output = result.clone();
    // The `--rule` drill-down (#724) carries no `elementCount`/`findings`;
    // bound its two lists and leave the rest of the payload as-is so the
    // CLI/agent comparison sees exactly the same items.
    if output.get("rule").is_some_and(|rule| rule.is_object()) {
        cap(&mut output, "statements", 5, all);
        cap(&mut output, "byNode", 5, all);
        return output;
    }
    cap(&mut output["elementCount"], "rules", 5, all);
    let mut output = query(&output, all);
    // Findings carry resolved span objects (#722), which cost roughly four
    // times the lint `location` string — on a large project the default
    // payload would exceed the same ~1.5k-token budget `lint` fits, so the
    // findings list is bounded by serialized bytes exactly like `lint`'s.
    if !all && output["findings"].is_array() {
        let total = output["findings"].as_array().map_or(0, Vec::len);
        fit_finding_bytes(&mut output, total);
    }
    output
}

/// Preserve the complete-set verdict while bounding its displayed diagnostics.
pub fn envelope(result: &mut Value, all: bool) {
    if let Some(diagnostics) = result["diagnostics"].as_array_mut() {
        diagnostics.sort_by_key(severity);
    }
    cap(result, "diagnostics", DEFAULT_ITEMS, all);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capped_findings_report_complete_counts_and_expansion() {
        let findings=(0..DEFAULT_ITEMS+1).map(|i|json!({"severity":if i==DEFAULT_ITEMS {"error"} else {"warning"},"code":format!("c{i}"),"message":"fix"})).collect::<Vec<_>>();
        let input = json!({"findings":findings,"config":{"huge":true}});
        let result = lint(&input, false);
        assert_eq!(result["counts"]["findings"]["error"], 1);
        assert_eq!(result["findings"][0]["severity"], "error");
        // #791: per-code counts describe the complete set in both the
        // counts group and the truncation note — the cap cannot hide a
        // code's real count.
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
            lint(&input, true)["findings"].as_array().unwrap().len(),
            DEFAULT_ITEMS + 1
        );
        assert!(result.get("config").is_none());
    }
    #[test]
    fn diagnostics_cap_preserves_verdict_and_all_recovers_every_item() {
        let input = json!({"ok":false,"exit":1,"diagnostics":vec![json!({"severity":"error"});DEFAULT_ITEMS+1]});
        let mut result = input.clone();
        envelope(&mut result, false);
        assert_eq!(result["ok"], false);
        assert_eq!(result["exit"], 1);
        assert_eq!(
            result["truncation"]["diagnostics"]["total"],
            DEFAULT_ITEMS + 1
        );
        envelope(&mut result, true);
        let mut complete = input;
        envelope(&mut complete, true);
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
        // #791: uncoded lists keep the plain note — `byCode` only exists
        // where items carry `code`.
        assert!(query(&bare, false)["truncation"]["items"]["byCode"].is_null());
        assert_eq!(query(&bare, true), bare);
    }
    #[test]
    fn byte_fit_keeps_the_full_set_per_code_counts() {
        // #791: a byte-fit truncation merges into the note an item cap
        // left — `total`/`byCode` still describe the complete set.
        let findings = (0..6)
            .map(|id| {
                json!({"code":format!("risk-{}",id%2),"severity":"error","message":"long actionable evidence ".repeat(120)})
            })
            .collect::<Vec<_>>();
        let result = lint(&json!({"findings": findings}), false);
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
    fn long_findings_fit_the_default_budget_without_changing_the_verdict() {
        let input = json!({"ok":false,"exit":1,"findings":(0..4).map(|id|json!({"code":format!("risk-{id}"),"severity":"error","message":"long actionable evidence ".repeat(120)})).collect::<Vec<_>>()});
        let result = lint(&input, false);
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
        let complete = lint(&input, true);
        assert_eq!(complete["findings"], input["findings"]);
        assert!(complete.get("truncation").is_none());
    }
}
