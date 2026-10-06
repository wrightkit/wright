//! The operation catalog client surfaces share (#535): one entry per
//! operation in `capabilities.operations`, in the contract's order. MCP lists
//! the `mcp` subset (ADR-0020's initial set); `wright agent tools` emits the
//! whole catalog as Messages API tool definitions or plain JSON Schema.

use serde_json::{Map, Value, json};

/// The committed agent schema is the single source of request shapes; tool
/// input schemas are extracted from its `$defs` at startup rather than
/// written a second time.
const AGENT_SCHEMA: &str = include_str!("../../../schemas/wright-agent-v1.schema.json");

/// One operation advertised by `capabilities`.
pub(crate) struct ToolSpec {
    /// The `ToolRequest` operation name (`op` field, camelCase).
    pub(crate) op: &'static str,
    /// The JSON Schema `$defs` entry describing this operation's request.
    pub(crate) request_def: &'static str,
    /// Request fields the MCP tool schema omits (`sources` defaults to disk
    /// there; programmatic callers may pass it, so `agent tools` keeps it).
    pub(crate) drop_fields: &'static [&'static str],
    /// Whether the operation belongs to the initial MCP set (ADR-0020).
    pub(crate) mcp: bool,
    /// The tool description shown to the model.
    pub(crate) description: &'static str,
    /// What a successful call returns, appended to `description` when client
    /// tool definitions are emitted (#535 wants the result described in text).
    /// The `wright-serve` binary shares this catalog without the emit path.
    #[allow(dead_code)]
    pub(crate) result: &'static str,
}

pub(crate) const TOOL_SPECS: &[ToolSpec] = &[
    ToolSpec {
        op: "capabilities",
        request_def: "CapabilitiesRequest",
        drop_fields: &[],
        mcp: false,
        description: "The service contract: name and version, the advertised operations, the result-schema map, languages, and profiles.",
        result: "the service contract payload",
    },
    ToolSpec {
        op: "project",
        request_def: "ProjectRequest",
        drop_fields: &[],
        mcp: true,
        description: "The loaded canonical program summary: origin, files, and counts of variables, subroutines, rules, symbols, and findings.",
        result: "the loaded program's origin, files, and counts",
    },
    ToolSpec {
        op: "rules",
        request_def: "RulesRequest",
        drop_fields: &[],
        mcp: false,
        description: "The canonical Workshop rules in the loaded program, optionally narrowed by `name`, `file`, or `max`.",
        result: "the program's rules, selection-wrapped when a selector was sent",
    },
    ToolSpec {
        op: "symbols",
        request_def: "SymbolsRequest",
        drop_fields: &[],
        mcp: true,
        description: "Every symbol in the loaded program, optionally narrowed by `kind`, `file`, or `max`. Issues the numeric ids other tools accept.",
        result: "the program's symbols, selection-wrapped when `file`/`max` was sent (`kind` alone keeps the bare array)",
    },
    ToolSpec {
        op: "references",
        request_def: "ReferencesRequest",
        drop_fields: &[],
        mcp: true,
        description: "References to a symbol, addressed by its numeric id or its declared name, optionally narrowed by `kind`, `rule`, `file`, or `max`.",
        result: "the symbol's references, selection-wrapped when a selector was sent",
    },
    ToolSpec {
        op: "usage",
        request_def: "UsageRequest",
        drop_fields: &[],
        mcp: true,
        description: "Usage counts for a symbol, addressed by its numeric id or its declared name.",
        result: "the symbol's usage counts plus its resolved id and kind",
    },
    ToolSpec {
        op: "cfg",
        request_def: "CfgRequest",
        drop_fields: &[],
        mcp: false,
        description: "The control-flow graph of one rule, addressed by its index or name, optionally narrowed by `kind` or `max`.",
        result: "the rule's control-flow graph plus selection when a selector was sent",
    },
    ToolSpec {
        op: "findings",
        request_def: "FindingsRequest",
        drop_fields: &[],
        mcp: false,
        description: "The project's check and analysis findings, optionally narrowed by `severity`, `rule`, `file`, or `max`.",
        result: "the project's findings, selection-wrapped when a selector was sent",
    },
    ToolSpec {
        op: "persistentObjects",
        request_def: "PersistentObjectsRequest",
        drop_fields: &[],
        mcp: false,
        description: "The program's persistent Workshop object facts.",
        result: "the program's persistent object facts",
    },
    ToolSpec {
        op: "lint",
        request_def: "LintRequest",
        drop_fields: &[],
        mcp: true,
        description: "Lint findings with effective severities, optionally narrowed by severity, rule, file, or max; `brief` returns the counts-and-top-findings summary.",
        result: "lint findings with effective severities and configuration; the brief summary when `brief` is true",
    },
    ToolSpec {
        op: "lintRules",
        request_def: "LintRulesRequest",
        drop_fields: &[],
        mcp: false,
        description: "The registered lint rules with full metadata and the effective configuration for this project.",
        result: "the registered lint rules and effective configuration",
    },
    ToolSpec {
        op: "callGraph",
        request_def: "CallGraphRequest",
        drop_fields: &[],
        mcp: true,
        description: "The subroutine call graph: caller rules mapped to callee subroutines, optionally narrowed by `caller`, `callee`, or `max`.",
        result: "the subroutine call-graph edges, selection-wrapped when a selector was sent",
    },
    ToolSpec {
        op: "costEstimate",
        request_def: "CostEstimateRequest",
        drop_fields: &[],
        mcp: true,
        description: "Generated-resource cost estimates: exact counts plus findings.",
        result: "generated-resource counts and findings, selection-wrapped when a selector was sent",
    },
    ToolSpec {
        op: "targetMetadata",
        request_def: "TargetMetadataRequest",
        drop_fields: &[],
        mcp: false,
        description: "Canonical target and catalog metadata used to address edit targets.",
        result: "the canonical target/catalog metadata",
    },
    ToolSpec {
        op: "compile",
        request_def: "CompileRequest",
        drop_fields: &[],
        mcp: false,
        description: "Compile the loaded project and report the emitted program.",
        result: "the wright-result/v1 compile envelope",
    },
    ToolSpec {
        op: "check",
        request_def: "CheckRequest",
        drop_fields: &[],
        mcp: true,
        description: "Check the loaded project and report diagnostics.",
        result: "the wright-result/v1 check envelope",
    },
    ToolSpec {
        op: "analyze",
        request_def: "AnalyzeRequest",
        drop_fields: &[],
        mcp: true,
        description: "Workshop cost, complexity hotspots, risk indicators, and cross-cutting state; `brief` returns the counts-and-top-items summary.",
        result: "the wright-result/v1 analysis envelope; the brief summary when `brief` is true",
    },
    ToolSpec {
        op: "inspect",
        request_def: "InspectRequest",
        drop_fields: &[],
        mcp: true,
        description: "The structural and semantic program model: program summary, rules, symbols, and references; `brief` returns the counts-and-leading-rules summary.",
        result: "the wright-result/v1 inspection envelope; the brief summary when `brief` is true",
    },
    ToolSpec {
        op: "validateEditTransaction",
        request_def: "ValidateEditTransactionRequest",
        drop_fields: &["sources"],
        mcp: true,
        description: "Validate and preview a source-edit transaction atomically against the session's project; no filesystem writes. Sources default to the on-disk text.",
        result: "the transaction's validation status, diagnostics, and previews when valid",
    },
    ToolSpec {
        op: "semanticRename",
        request_def: "SemanticRenameRequest",
        drop_fields: &["sources"],
        mcp: true,
        description: "Propose a semantic rename: returns the validated transaction or structured refusal diagnostics. The target is a symbol id or declared name, or a source/line/col position. Sources default to the on-disk text.",
        result: "the validated rename transaction or a structured refusal",
    },
    ToolSpec {
        op: "providerSemanticRename",
        request_def: "ProviderSemanticRenameRequest",
        drop_fields: &[],
        mcp: true,
        description: "Provider-owned rename for source-language projects (e.g. OverPy): the configured provider computes the edits, Wright verifies document versions and source preconditions, the provider validates the transaction, and the edited project is rechecked — returning validated edits or a structured refusal. `documents` and `sources` are supplied by the caller.",
        result: "the provider-resolved rename transaction or a structured refusal",
    },
    ToolSpec {
        op: "providerValidateEdit",
        request_def: "ProviderValidateEditRequest",
        drop_fields: &[],
        mcp: true,
        description: "Validate a caller-proposed source-edit transaction for a provider-owned language through the same provider-backed pipeline as providerSemanticRename; no filesystem writes. `documents` and `sources` are supplied by the caller.",
        result: "the provider-validated transaction or a structured refusal",
    },
];

/// The schema `$defs` of the committed agent contract.
pub(crate) fn schema_defs() -> Map<String, Value> {
    serde_json::from_str::<Value>(AGENT_SCHEMA).expect("committed agent schema parses")["$defs"]
        .as_object()
        .expect("schema $defs")
        .clone()
}

fn snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for ch in name.chars() {
        if ch.is_uppercase() {
            out.push('_');
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// The client-facing tool name for an operation.
pub(crate) fn tool_name(op: &str) -> String {
    format!("wright_{}", snake_case(op))
}

/// Collect every `#/$defs/<name>` reachable from `value` into `needed`.
fn collect_refs(value: &Value, defs: &Map<String, Value>, needed: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if key == "$ref" {
                    if let Some(name) = child
                        .as_str()
                        .and_then(|reference| reference.strip_prefix("#/$defs/"))
                    {
                        if !needed.iter().any(|seen| seen == name) {
                            needed.push(name.to_string());
                            if let Some(def) = defs.get(name) {
                                collect_refs(def, defs, needed);
                            }
                        }
                    }
                } else {
                    collect_refs(child, defs, needed);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_refs(item, defs, needed);
            }
        }
        _ => {}
    }
}

/// A self-contained JSON Schema for an operation's request fields: the
/// `op` member goes away (the tool name carries it) plus whatever the
/// surface chooses to omit, and reachable `$ref` targets ship inside the
/// schema's own `$defs`.
pub(crate) fn input_schema(defs: &Map<String, Value>, spec: &ToolSpec, drop: &[&str]) -> Value {
    let request = &defs[spec.request_def];
    let mut properties = request["properties"].clone();
    let object = properties
        .as_object_mut()
        .expect("request properties object");
    object.remove("op");
    for field in drop {
        object.remove(*field);
    }
    let required: Vec<Value> = request["required"]
        .as_array()
        .map(|required| {
            required
                .iter()
                .filter(|name| {
                    name.as_str() != Some("op")
                        && !drop.contains(&name.as_str().unwrap_or_default())
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let mut schema = json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false,
    });
    if !required.is_empty() {
        schema["required"] = Value::Array(required);
    }
    let mut needed = Vec::new();
    collect_refs(&schema, defs, &mut needed);
    if !needed.is_empty() {
        let mut sub = Map::new();
        for name in needed {
            sub.insert(name.clone(), defs[&name].clone());
        }
        schema["$defs"] = Value::Object(sub);
    }
    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tooldefs::test_support::assert_no_recursive_refs;
    use wright_driver::service::OPERATIONS;

    #[test]
    fn catalog_covers_every_advertised_operation_in_order() {
        let catalog: Vec<&str> = TOOL_SPECS.iter().map(|spec| spec.op).collect();
        assert_eq!(catalog, OPERATIONS, "tool catalog misses an advertised op");
        for spec in TOOL_SPECS {
            let defs = schema_defs();
            assert!(
                defs.contains_key(spec.request_def),
                "{} has no schema def {}",
                spec.op,
                spec.request_def
            );
            assert!(!spec.description.is_empty() && !spec.result.is_empty());
        }
    }

    #[test]
    fn emitted_schemas_have_no_recursive_refs() {
        let defs = schema_defs();
        for spec in TOOL_SPECS {
            assert_no_recursive_refs(&input_schema(&defs, spec, &[]), spec.op);
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use serde_json::Value;
    use std::collections::{HashMap, HashSet};

    /// Every `$ref` in an emitted input schema resolves inside its own
    /// `$defs` and forms no cycle — the Messages API rejects tools whose
    /// schema has a recursive `$ref` (#535).
    pub(crate) fn assert_no_recursive_refs(schema: &Value, context: &str) {
        let mut edges: HashMap<String, HashSet<String>> = HashMap::new();
        fn walk(value: &Value, from: Option<&str>, edges: &mut HashMap<String, HashSet<String>>) {
            match value {
                Value::Object(map) => {
                    for (key, child) in map {
                        if key == "$ref" {
                            if let Some(target) =
                                child.as_str().and_then(|r| r.strip_prefix("#/$defs/"))
                            {
                                edges
                                    .entry(from.unwrap_or("<root>").to_string())
                                    .or_default()
                                    .insert(target.to_string());
                            }
                        } else {
                            walk(child, from, edges);
                        }
                    }
                }
                Value::Array(items) => {
                    for item in items {
                        walk(item, from, edges);
                    }
                }
                _ => {}
            }
        }
        walk(schema, None, &mut edges);
        if let Some(defs) = schema.get("$defs").and_then(Value::as_object) {
            for (name, def) in defs {
                walk(def, Some(name), &mut edges);
            }
        }
        // DFS for a cycle through the local-$defs graph.
        fn visit<'a>(
            node: &'a str,
            edges: &'a HashMap<String, HashSet<String>>,
            path: &mut Vec<&'a str>,
            done: &mut HashSet<&'a str>,
        ) -> Option<Vec<String>> {
            if path.contains(&node) {
                let mut cycle: Vec<String> = path.iter().map(|s| s.to_string()).collect();
                cycle.push(node.to_string());
                return Some(cycle);
            }
            if done.contains(node) {
                return None;
            }
            path.push(node);
            if let Some(targets) = edges.get(node) {
                for target in targets {
                    if let Some(cycle) = visit(target, edges, path, done) {
                        return Some(cycle);
                    }
                }
            }
            path.pop();
            done.insert(node);
            None
        }
        let mut done = HashSet::new();
        for node in edges.keys() {
            assert!(
                visit(node, &edges, &mut Vec::new(), &mut done).is_none(),
                "{context}: recursive $ref cycle through {node}"
            );
        }
    }
}
