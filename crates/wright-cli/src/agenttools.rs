//! `wright agent tools`: emits the operations `capabilities` advertises as
//! client tool definitions for code-executing agents — Anthropic Messages
//! API tools for programmatic tool calling, or plain JSON Schema for other
//! harnesses (#535). The operation catalog, descriptions, and schema
//! projection are shared with the MCP surface through [`crate::tooldefs`],
//! so the emitted set can never drift from the advertised one.

use serde_json::{Map, Value, json};

use crate::tooldefs::{TOOL_SPECS, ToolSpec, input_schema, schema_defs, tool_name};

/// The `allowed_callers` value that makes a Messages API tool callable from
/// Anthropic's code execution tool (programmatic tool calling).
const CODE_EXECUTION_CALLER: &str = "code_execution_20260120";

/// The emitted tool description: the operation description with its result
/// described in text, as #535 requires.
fn emitted_description(spec: &ToolSpec) -> String {
    format!("{} Result: {}.", spec.description, spec.result)
}

/// `--format messages`: every advertised operation as a Messages API client
/// tool definition, code-execution-callable.
pub(crate) fn messages_tools() -> Value {
    let defs = schema_defs();
    json!({
        "contract": wright_driver::service::AGENT_CONTRACT,
        "tools": TOOL_SPECS.iter().map(|spec| json!({
            "name": tool_name(spec.op),
            "description": emitted_description(spec),
            "input_schema": input_schema(&defs, spec, &[]),
            "allowed_callers": [CODE_EXECUTION_CALLER],
        })).collect::<Vec<_>>(),
    })
}

/// `--format json-schema`: the same operations as plain JSON Schema for
/// other harnesses — operation name to a standalone request schema holding
/// the emitted description.
pub(crate) fn json_schema_tools() -> Value {
    let defs = schema_defs();
    let mut schemas = Map::new();
    for spec in TOOL_SPECS {
        let mut schema = input_schema(&defs, spec, &[]);
        schema["description"] = Value::String(emitted_description(spec));
        schemas.insert(spec.op.to_string(), schema);
    }
    json!({
        "contract": wright_driver::service::AGENT_CONTRACT,
        "schemas": schemas,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tooldefs::test_support::assert_no_recursive_refs;
    use wright_driver::service::OPERATIONS;

    #[test]
    fn messages_tools_match_the_api_shape_deterministically() {
        let doc = messages_tools();
        assert_eq!(doc["contract"], "wright-agent/v1");
        let tools = doc["tools"].as_array().unwrap();
        assert_eq!(tools.len(), OPERATIONS.len());
        assert_eq!(messages_tools(), doc, "emission is not deterministic");
        for tool in tools {
            for field in ["name", "description", "input_schema", "allowed_callers"] {
                assert!(tool.get(field).is_some(), "{} lacks {field}", tool["name"]);
            }
            assert!(tool["name"].as_str().unwrap().starts_with("wright_"));
            assert_eq!(tool["allowed_callers"], json!([CODE_EXECUTION_CALLER]));
            assert_eq!(tool["input_schema"]["type"], "object");
            assert!(
                tool["description"].as_str().unwrap().contains("Result:"),
                "description does not describe the result"
            );
            assert!(tool["input_schema"]["properties"].get("op").is_none());
            assert_no_recursive_refs(&tool["input_schema"], tool["name"].as_str().unwrap());
        }
    }

    #[test]
    fn json_schema_tools_emit_one_schema_per_operation() {
        let doc = json_schema_tools();
        assert_eq!(doc["contract"], "wright-agent/v1");
        let schemas = doc["schemas"].as_object().unwrap();
        assert_eq!(schemas.len(), OPERATIONS.len());
        for op in OPERATIONS {
            let schema = &schemas[*op];
            assert_eq!(schema["type"], "object", "{op} schema missing");
            assert!(schema["description"].is_string());
            assert_no_recursive_refs(schema, op);
        }
    }
}
