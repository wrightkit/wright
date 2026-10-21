//! Rule identity strings for text output (ADR-0035): payloads that name a
//! rule carry its canonical `address` — the reusable form a rule selector
//! accepts — and printers prefer it over the declared `name` so a
//! duplicated or unnamed rule still reports the selector that addresses it.

use serde_json::Value;

/// A rule's canonical `address`, else its declared `name` for payloads
/// that predate the address member. Empty names count as absent.
pub(crate) fn rule_address(rule: &Value) -> Option<&str> {
    rule["address"]
        .as_str()
        .or(rule["name"].as_str())
        .filter(|name| !name.is_empty())
}

/// `rule_address` or `<unnamed>` when the payload names no rule.
pub(crate) fn rule_name(rule: &Value) -> &str {
    rule_address(rule).unwrap_or("<unnamed>")
}

/// A call-graph edge's caller: the canonical `callerAddress`, else the
/// declared `caller` name, else `<unnamed>`.
pub(crate) fn edge_caller(edge: &Value) -> &str {
    edge["callerAddress"]
        .as_str()
        .or(edge["caller"].as_str())
        .unwrap_or("<unnamed>")
}

/// An essential condition tree's rule: `ruleAddress`, else `rule`, else
/// `<unnamed>`.
pub(crate) fn tree_rule(tree: &Value) -> &str {
    tree["ruleAddress"]
        .as_str()
        .or(tree["rule"].as_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("<unnamed>")
}
