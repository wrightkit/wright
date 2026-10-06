//! MCP stdio transport (#473): a thin adapter that maps MCP tools onto
//! `ToolService::handle` — one `wright_<operation>` tool per operation in the
//! initial set (ADR-0020). The adapter performs initialization, answers
//! `tools/list`, and turns `tools/call` into a `ToolRequest`; all semantics,
//! freshness, and refusals belong to the service. A successful `result`
//! becomes the tool result content as JSON; a service refusal becomes a tool
//! result with `isError: true` carrying the same `{code, message}`.
//!
//! The protocol layer is handwritten rather than SDK-based: MCP over stdio is
//! newline-delimited JSON-RPC 2.0 — the same framing as the `jsonrpc`
//! transport — and the whole mapping needs five methods, so pulling in the
//! tokio-based `rmcp` crate would buy a runtime this binary does not
//! otherwise need.

use std::process::ExitCode;

use serde_json::{Map, Value, json};
use wright_driver::service::{ToolRequest, ToolResponse, ToolService};

use crate::serve::serve_lines;

/// The committed agent schema is the single source of request shapes; tool
/// input schemas are extracted from its `$defs` at startup rather than
/// written a second time.
const AGENT_SCHEMA: &str = include_str!("../../../schemas/wright-agent-v1.schema.json");

/// Protocol versions the adapter negotiates (`initialize` echoes the client's
/// version when supported, else responds with the newest listed).
const PROTOCOL_VERSIONS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];

/// One tool per operation in the initial MCP set (ADR-0020).
struct ToolSpec {
    /// The `ToolRequest` operation name (`op` field, camelCase).
    op: &'static str,
    /// The JSON Schema `$defs` entry describing this operation's request.
    request_def: &'static str,
    /// Request fields the tool schema omits (`sources` defaults to disk).
    drop_fields: &'static [&'static str],
    /// The tool description shown to the model.
    description: &'static str,
}

const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        op: "project",
        request_def: "ProjectRequest",
        drop_fields: &[],
        description: "The loaded canonical program summary: origin, files, and counts of variables, subroutines, rules, symbols, and findings.",
    },
    ToolSpec {
        op: "symbols",
        request_def: "SymbolsRequest",
        drop_fields: &[],
        description: "Every symbol in the loaded program, optionally filtered by `kind`. Issues the numeric ids other tools accept.",
    },
    ToolSpec {
        op: "references",
        request_def: "ReferencesRequest",
        drop_fields: &[],
        description: "References to a symbol, addressed by its numeric id or its declared name.",
    },
    ToolSpec {
        op: "usage",
        request_def: "UsageRequest",
        drop_fields: &[],
        description: "Usage counts for a symbol, addressed by its numeric id or its declared name.",
    },
    ToolSpec {
        op: "callGraph",
        request_def: "CallGraphRequest",
        drop_fields: &[],
        description: "The subroutine call graph: caller rules mapped to callee subroutines.",
    },
    ToolSpec {
        op: "check",
        request_def: "CheckRequest",
        drop_fields: &[],
        description: "Check the loaded project and report diagnostics.",
    },
    ToolSpec {
        op: "lint",
        request_def: "LintRequest",
        drop_fields: &[],
        description: "Lint findings with effective severities, optionally narrowed by severity, rule, file, or max.",
    },
    ToolSpec {
        op: "costEstimate",
        request_def: "CostEstimateRequest",
        drop_fields: &[],
        description: "Generated-resource cost estimates: exact counts plus findings.",
    },
    ToolSpec {
        op: "semanticRename",
        request_def: "SemanticRenameRequest",
        drop_fields: &["sources"],
        description: "Propose a semantic rename: returns the validated transaction or structured refusal diagnostics. The target is a symbol id or declared name, or a source/line/col position. Sources default to the on-disk text.",
    },
    ToolSpec {
        op: "validateEditTransaction",
        request_def: "ValidateEditTransactionRequest",
        drop_fields: &["sources"],
        description: "Validate and preview a source-edit transaction atomically against the session's project; no filesystem writes. Sources default to the on-disk text.",
    },
    ToolSpec {
        op: "providerSemanticRename",
        request_def: "ProviderSemanticRenameRequest",
        drop_fields: &[],
        description: "Provider-owned rename for source-language projects (e.g. OverPy): the configured provider computes the edits, Wright verifies document versions and source preconditions, the provider validates the transaction, and the edited project is rechecked — returning validated edits or a structured refusal. `documents` and `sources` are supplied by the caller.",
    },
    ToolSpec {
        op: "providerValidateEdit",
        request_def: "ProviderValidateEditRequest",
        drop_fields: &[],
        description: "Validate a caller-proposed source-edit transaction for a provider-owned language through the same provider-backed pipeline as providerSemanticRename; no filesystem writes. `documents` and `sources` are supplied by the caller.",
    },
    ToolSpec {
        op: "lookup",
        request_def: "LookupRequest",
        drop_fields: &[],
        description: "Resolve a display name, near spelling, or guess to the language owner's accepted spelling and signature. Use it before writing in an unfamiliar language and when a name is rejected.",
    },
];

/// A tool definition as listed by `tools/list`.
struct Tool {
    name: String,
    spec: &'static ToolSpec,
    input_schema: Value,
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

/// The tool `inputSchema` derived from the request `$defs` entry: the
/// operation's parameters without `op` (the tool name carries it) and without
/// the fields the transport omits (`sources` on edit operations).
fn input_schema(defs: &Map<String, Value>, spec: &ToolSpec) -> Value {
    let request = &defs[spec.request_def];
    let mut properties = request["properties"].clone();
    let object = properties
        .as_object_mut()
        .expect("request properties object");
    object.remove("op");
    for field in spec.drop_fields {
        object.remove(*field);
    }
    let required: Vec<Value> = request["required"]
        .as_array()
        .map(|required| {
            required
                .iter()
                .filter(|name| {
                    name.as_str() != Some("op")
                        && !spec
                            .drop_fields
                            .contains(&name.as_str().unwrap_or_default())
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
    // `$ref` targets used by the kept fields ship inside the schema so the
    // tool definition is self-contained.
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

fn tool_name(spec: &ToolSpec) -> String {
    format!("wright_{}", snake_case(spec.op))
}

/// The tools this service advertises: the initial set intersected with
/// `capabilities.operations`, so listing and the contract cannot disagree.
fn build_tools(service: &ToolService<'_>) -> (Vec<Tool>, String, String) {
    let capabilities = service.capabilities();
    let defs = serde_json::from_str::<Value>(AGENT_SCHEMA).expect("committed agent schema parses")
        ["$defs"]
        .as_object()
        .expect("schema $defs")
        .clone();
    let tools = TOOLS
        .iter()
        .filter(|spec| capabilities.operations.iter().any(|op| op == spec.op))
        .map(|spec| Tool {
            name: tool_name(spec),
            spec,
            input_schema: input_schema(&defs, spec),
        })
        .collect();
    (tools, capabilities.name, capabilities.version)
}

pub(crate) fn serve_mcp(service: &mut ToolService<'_>) -> ExitCode {
    let (tools, name, version) = build_tools(service);
    serve_lines(|line| {
        dispatch_message(service, &tools, &name, &version, line)
            .map(|response| response.to_string())
    })
}

fn dispatch_message(
    service: &mut ToolService<'_>,
    tools: &[Tool],
    name: &str,
    version: &str,
    line: &str,
) -> Option<Value> {
    let value: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(_) => return Some(error_response(Value::Null, -32700, "Parse error")),
    };
    let Some(object) = value.as_object() else {
        return Some(error_response(Value::Null, -32600, "Invalid Request"));
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(error_response(Value::Null, -32600, "Invalid Request"));
    }
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return Some(error_response(Value::Null, -32600, "Invalid Request"));
    };
    let Some(id) = object.get("id").cloned() else {
        // Notifications (initialized, cancelled, progress) carry no id and
        // get no response.
        return None;
    };
    if !matches!(id, Value::Null | Value::String(_) | Value::Number(_)) {
        return Some(error_response(Value::Null, -32600, "Invalid Request"));
    }
    let params = object.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": negotiate(params.get("protocolVersion").and_then(Value::as_str)),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": name, "version": version },
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({
            "tools": tools.iter().map(|tool| json!({
                "name": tool.name,
                "description": tool.spec.description,
                "inputSchema": tool.input_schema,
            })).collect::<Vec<_>>(),
        })),
        "tools/call" => call_tool(service, tools, &params),
        other => Err((-32601, format!("Method not found: {other}"))),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => error_response(id, code, message),
    })
}

fn negotiate(requested: Option<&str>) -> &'static str {
    PROTOCOL_VERSIONS
        .iter()
        .copied()
        .find(|version| Some(*version) == requested)
        .unwrap_or_else(|| PROTOCOL_VERSIONS.last().copied().unwrap())
}

fn call_tool(
    service: &mut ToolService<'_>,
    tools: &[Tool],
    params: &Value,
) -> Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(tool) = tools.iter().find(|tool| tool.name == name) else {
        return Err((-32602, format!("unknown tool '{name}'")));
    };
    let arguments = match params.get("arguments") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(arguments)) => arguments.clone(),
        _ => return Err((-32602, "tools/call arguments must be an object".into())),
    };
    // The tool name carries the operation; caller arguments are the request
    // fields. An `op` passed by the client is the adapter's own, not theirs.
    let mut request = arguments;
    request.insert("op".to_string(), Value::String(tool.spec.op.to_string()));
    let request = serde_json::from_value::<ToolRequest>(Value::Object(request))
        .map_err(|error| (-32602, format!("invalid arguments: {error}")))?;
    Ok(tool_result(service.handle(&request)))
}

/// `CallToolResult` for one handled request: the service `result` becomes the
/// tool content as JSON; a refusal becomes `isError` carrying the same
/// `{code, message}` the service produced.
fn tool_result(response: ToolResponse) -> Value {
    let (payload, is_error) = match response {
        ToolResponse::Ok { result } => (result, false),
        ToolResponse::Error { error } => (
            json!({ "code": error.code, "message": error.message }),
            true,
        ),
    };
    let mut result = json!({
        "content": [{ "type": "text", "text": payload.to_string() }],
    });
    if is_error {
        result["isError"] = Value::Bool(true);
    }
    result
}

fn error_response(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_are_wright_prefixed_snake_case() {
        let names: Vec<String> = TOOLS.iter().map(tool_name).collect();
        assert_eq!(names[0], "wright_project");
        assert_eq!(names[4], "wright_call_graph");
        assert_eq!(names[7], "wright_cost_estimate");
        assert_eq!(names[8], "wright_semantic_rename");
        assert_eq!(names[9], "wright_validate_edit_transaction");
        assert_eq!(names[10], "wright_provider_semantic_rename");
        assert_eq!(names[11], "wright_provider_validate_edit");
        assert_eq!(names[12], "wright_lookup");
        assert!(names.iter().all(|name| name.starts_with("wright_")));
    }

    #[test]
    fn input_schemas_drop_op_and_edit_sources() {
        let schema: Value = serde_json::from_str(AGENT_SCHEMA).unwrap();
        let defs = schema["$defs"].as_object().unwrap();
        let rename = TOOLS
            .iter()
            .find(|spec| spec.op == "semanticRename")
            .unwrap();
        let input = input_schema(defs, rename);
        assert_eq!(input["type"], "object");
        assert!(input["properties"].get("op").is_none());
        assert!(input["properties"].get("sources").is_none());
        assert!(input["properties"].get("target").is_some());
        assert_eq!(input["required"], json!(["target"]));
        assert!(input["$defs"].get("RenameTarget").is_some());
        let project = TOOLS.iter().find(|spec| spec.op == "project").unwrap();
        let input = input_schema(defs, project);
        assert!(input["required"].is_null());
        assert_eq!(input["properties"], json!({}));
        let lookup = TOOLS.iter().find(|spec| spec.op == "lookup").unwrap();
        let input = input_schema(defs, lookup);
        assert_eq!(input["required"], json!(["language"]));
        assert!(input["properties"].get("op").is_none());
        assert!(input["properties"].get("within").is_some());
    }

    #[test]
    fn tool_result_passes_refusals_through_with_their_code() {
        // The mapping is code-agnostic: any service refusal — including
        // `edit-requires-provider`, which only an OPY-backed session can
        // produce — arrives as `isError` with the original {code, message}.
        let refusal = tool_result(ToolResponse::Error {
            error: wright_driver::service::ToolErrorInfo {
                code: "edit-requires-provider".to_string(),
                message: "raw edit operations cover Workshop input".to_string(),
            },
        });
        assert_eq!(refusal["isError"], true);
        let payload: Value =
            serde_json::from_str(refusal["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["code"], "edit-requires-provider");

        let ok = tool_result(ToolResponse::Ok {
            result: json!({"answer": 42}),
        });
        assert!(ok.get("isError").is_none());
        let payload: Value =
            serde_json::from_str(ok["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["answer"], 42);
    }

    #[test]
    fn initialize_negotiates_versions_and_ids_are_checked() {
        use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceKind};
        let input = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("tests/fixtures/workshop/synthetic/basic-rule.ws");
        let mut session = CompilerSession::new(SessionConfig {
            input: InputSpec::Path(input),
            kind: SourceKind::Workshop,
            ..SessionConfig::default()
        })
        .unwrap();
        let mut service = ToolService::new(&mut session).unwrap();
        let (tools, name, version) = build_tools(&service);

        // An unknown client version falls back to the newest supported; the
        // string id is echoed.
        let response = dispatch_message(
            &mut service,
            &tools,
            &name,
            &version,
            r#"{"jsonrpc":"2.0","id":"abc","method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
        )
        .unwrap();
        assert_eq!(response["id"], "abc");
        assert_eq!(response["result"]["protocolVersion"], "2025-11-25");

        // Object/array ids are not valid JSON-RPC ids, matching the jsonrpc
        // transport's check.
        let response = dispatch_message(
            &mut service,
            &tools,
            &name,
            &version,
            r#"{"jsonrpc":"2.0","id":{"x":1},"method":"ping"}"#,
        )
        .unwrap();
        assert_eq!(response["id"], Value::Null);
        assert_eq!(response["error"]["code"], -32600);
    }
}
