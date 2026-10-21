use serde_json::{Map, Value, json};

use crate::tooldefs::{TOOL_SPECS, ToolSpec, input_schema, schema_defs, tool_name};

const CODE_EXECUTION_CALLER: &str = "code_execution_20260120";

fn emitted_description(spec: &ToolSpec) -> String {
    format!("{} Result: {}.", spec.description, spec.result)
}

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
