use serde_json::Value;

pub(crate) fn rule_address(rule: &Value) -> Option<&str> {
    rule["address"]
        .as_str()
        .or(rule["name"].as_str())
        .filter(|name| !name.is_empty())
}

pub(crate) fn rule_name(rule: &Value) -> &str {
    rule_address(rule).unwrap_or("<unnamed>")
}

pub(crate) fn edge_caller(edge: &Value) -> &str {
    edge["callerAddress"]
        .as_str()
        .or(edge["caller"].as_str())
        .unwrap_or("<unnamed>")
}

pub(crate) fn tree_rule(tree: &Value) -> &str {
    tree["ruleAddress"]
        .as_str()
        .or(tree["rule"].as_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("<unnamed>")
}
