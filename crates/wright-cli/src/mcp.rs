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
use crate::tooldefs::{TOOL_SPECS, ToolSpec, input_schema, tool_name};

/// Protocol versions the adapter negotiates (`initialize` echoes the client's
/// version when supported, else responds with the newest listed).
const PROTOCOL_VERSIONS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];

/// A tool definition as listed by `tools/list`.
struct Tool {
    name: String,
    spec: &'static ToolSpec,
    input_schema: Value,
}

/// The tools this service advertises: the initial MCP set (ADR-0020)
/// intersected with `capabilities.operations`, so listing and the contract
/// cannot disagree.
fn build_tools(service: &ToolService<'_>) -> (Vec<Tool>, String, String) {
    let capabilities = service.capabilities();
    let defs = crate::tooldefs::schema_defs();
    let tools = TOOL_SPECS
        .iter()
        .filter(|spec| spec.mcp && capabilities.operations.iter().any(|op| op == spec.op))
        .map(|spec| Tool {
            name: tool_name(spec.op),
            spec,
            input_schema: input_schema(&defs, spec, spec.drop_fields),
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
        let names: Vec<String> = TOOL_SPECS
            .iter()
            .filter(|spec| spec.mcp)
            .map(|spec| tool_name(spec.op))
            .collect();
        for name in [
            "wright_project",
            "wright_rules",
            "wright_symbols",
            "wright_references",
            "wright_usage",
            "wright_cfg",
            "wright_findings",
            "wright_persistent_objects",
            "wright_call_graph",
            "wright_check",
            "wright_analyze",
            "wright_inspect",
            "wright_lint",
            "wright_lint_rules",
            "wright_cost_estimate",
            "wright_target_metadata",
            "wright_semantic_rename",
            "wright_validate_edit_transaction",
            "wright_provider_semantic_rename",
            "wright_provider_validate_edit",
            "wright_lookup",
        ] {
            assert!(names.iter().any(|n| n == name), "missing {name}");
        }
        assert_eq!(names.len(), 21);
        assert!(names.iter().all(|name| name.starts_with("wright_")));
    }

    #[test]
    fn input_schemas_drop_op_and_edit_sources() {
        let defs = crate::tooldefs::schema_defs();
        let rename = TOOL_SPECS
            .iter()
            .find(|spec| spec.op == "semanticRename")
            .unwrap();
        let input = input_schema(&defs, rename, rename.drop_fields);
        assert_eq!(input["type"], "object");
        assert!(input["properties"].get("op").is_none());
        assert!(input["properties"].get("sources").is_none());
        assert!(input["properties"].get("target").is_some());
        assert_eq!(input["required"], json!(["target"]));
        assert!(input["$defs"].get("RenameTarget").is_some());
        let project = TOOL_SPECS.iter().find(|spec| spec.op == "project").unwrap();
        let input = input_schema(&defs, project, &[]);
        assert!(input["required"].is_null());
        assert_eq!(input["properties"], json!({}));
        let lookup = TOOL_SPECS.iter().find(|spec| spec.op == "lookup").unwrap();
        let input = input_schema(&defs, lookup, &[]);
        assert_eq!(input["required"], json!(["language"]));
        assert!(input["properties"].get("op").is_none());
        assert!(input["properties"].get("within").is_some());

        // #532: the brief tools expose `brief` in their generated schemas.
        for op in ["analyze", "inspect", "lint"] {
            let spec = TOOL_SPECS.iter().find(|spec| spec.op == op).unwrap();
            let input = input_schema(&defs, spec, &[]);
            assert_eq!(
                input["properties"]["brief"]["type"], "boolean",
                "{op} input schema has no brief field"
            );
        }
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

    #[test]
    fn malformed_address_arguments_name_the_expected_shape() {
        // An object-shaped `symbol` used to surface serde's generic "did not
        // match any variant of untagged enum" — useless to an agent. The
        // refusal now names the two accepted forms.
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
        let response = dispatch_message(
            &mut service,
            &tools,
            &name,
            &version,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"wright_references","arguments":{"symbol":{"id":0}}}}"#,
        )
        .unwrap();
        let message = response["error"]["message"].as_str().unwrap();
        assert!(
            message.contains("a numeric id or a declared name string"),
            "the refusal names the accepted shapes: {message}"
        );
    }
}
