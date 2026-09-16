//! Exposes the same operations as [`wright_driver::service::ToolService`]
//! over two transports:
//!
//! * **stdio JSON-lines** (`--transport stdio`): one request per line, one
//!   response per line (the legacy tool style, generalized to the current
//!   service).
//! * **JSON-RPC 2.0** (`--transport jsonrpc`): standard JSON-RPC envelopes
//!   with `id`/`method`/`params` and `result`/`error` responses.
//!
//! MCP is intentionally not implemented — no agent-integration evidence
//! justified it in v1.

use std::io::{BufRead, Write};
use std::process::ExitCode;

use serde_json::Value;
use wright_driver::config::{InputSpec, SessionConfig, SourceKind};
use wright_driver::service::{ToolRequest, ToolService};

fn usage() -> &'static str {
    "usage: wright-serve --transport stdio|jsonrpc [--kind opy|ostw|workshop|protocol] [--locale LOC] [--profile off|compat|aggressive] [INPUT]\n\
     \n\
     Serves the Wright tool service over stdin/stdout. With no INPUT, reads a\n\
     protocol payload or Workshop text from stdin (auto-detected)."
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut transport = "stdio".to_string();
    let mut config = SessionConfig::default();
    let mut positional: Option<std::path::PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--transport" => {
                transport = args.next().unwrap_or_else(|| "stdio".to_string());
            }
            "--kind" => {
                let value = args.next().unwrap_or_default();
                config.kind = SourceKind::parse(&value).unwrap_or(SourceKind::Auto);
            }
            "--locale" => {
                config.locale = args.next();
            }
            "--profile" => {
                let value = args.next().unwrap_or_default();
                config.profile = wright_driver::Profile::parse(&value).unwrap_or_default();
            }
            "--help" | "-h" => {
                println!("{}", usage());
                return ExitCode::SUCCESS;
            }
            other if other.starts_with('-') => {
                eprintln!("wright-serve: unknown argument '{other}'");
                eprintln!("{}", usage());
                return ExitCode::from(2);
            }
            other => {
                positional = Some(std::path::PathBuf::from(other));
            }
        }
    }

    // Build the session: an explicit input path, or stdin auto-detection.
    config.input = match positional {
        Some(path) => InputSpec::Path(path),
        None => InputSpec::Stdin,
    };
    let mut session = match wright_driver::CompilerSession::new(config) {
        Ok(session) => session,
        Err(diagnostic) => {
            eprintln!("wright-serve: {}", diagnostic.message);
            return ExitCode::from(1);
        }
    };
    let mut service = match ToolService::new(&mut session) {
        Ok(service) => service,
        Err(diagnostic) => {
            eprintln!("wright-serve: {}", diagnostic.message);
            return ExitCode::from(1);
        }
    };

    match transport.as_str() {
        "stdio" => serve_stdio(&mut service),
        "jsonrpc" => serve_jsonrpc(&mut service),
        other => {
            eprintln!("wright-serve: unknown transport '{other}'");
            eprintln!("{}", usage());
            ExitCode::from(2)
        }
    }
}

/// One JSON request per line → one JSON response per line.
fn serve_stdio(service: &mut ToolService<'_>) -> ExitCode {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let response = dispatch(service, &line);
        if writeln!(out, "{response}").is_err() {
            break;
        }
    }
    ExitCode::SUCCESS
}

/// JSON-RPC 2.0 envelopes.
fn serve_jsonrpc(service: &mut ToolService<'_>) -> ExitCode {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str(&line) {
            Ok(value) => jsonrpc_dispatch(service, value),
            Err(_) => Some(jsonrpc_error(Value::Null, -32700, "Parse error")),
        };
        if let Some(response) = response {
            if writeln!(out, "{response}").is_err() {
                break;
            }
        }
    }
    ExitCode::SUCCESS
}

fn jsonrpc_dispatch(service: &mut ToolService<'_>, value: Value) -> Option<Value> {
    if let Value::Array(batch) = value {
        if batch.is_empty() {
            return Some(jsonrpc_error(Value::Null, -32600, "Invalid Request"));
        }
        let responses: Vec<Value> = batch
            .into_iter()
            .filter_map(|value| jsonrpc_dispatch_request(service, value))
            .collect();
        return (!responses.is_empty()).then_some(Value::Array(responses));
    }
    jsonrpc_dispatch_request(service, value)
}

fn jsonrpc_dispatch_request(service: &mut ToolService<'_>, value: Value) -> Option<Value> {
    let object = match value.as_object() {
        Some(object) => object,
        None => return Some(jsonrpc_error(Value::Null, -32600, "Invalid Request")),
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(jsonrpc_error(Value::Null, -32600, "Invalid Request"));
    }

    let method = match object.get("method").and_then(Value::as_str) {
        Some(method) => method,
        None => return Some(jsonrpc_error(Value::Null, -32600, "Invalid Request")),
    };
    let has_id = object.contains_key("id");
    let id = object.get("id").cloned().unwrap_or(Value::Null);
    if has_id && !matches!(id, Value::Null | Value::String(_) | Value::Number(_)) {
        return Some(jsonrpc_error(Value::Null, -32600, "Invalid Request"));
    }

    let response = match method {
        "request" => {
            let params = match object.get("params") {
                Some(Value::Object(params)) if params.get("op").is_some() => {
                    Value::Object(params.clone())
                }
                _ => return jsonrpc_optional_error(has_id, id, -32602, "Invalid params"),
            };
            let request = match serde_json::from_value::<ToolRequest>(params) {
                Ok(request) => request,
                Err(_) => return jsonrpc_optional_error(has_id, id, -32602, "Invalid params"),
            };
            tool_response_result(service.handle(&request))
        }
        "compile" | "check" | "analyze" | "inspect" => {
            if object.contains_key("params") {
                return jsonrpc_optional_error(has_id, id, -32602, "Invalid params");
            }
            match method {
                "compile" => serde_json::to_value(service.compile()).expect("result serializes"),
                "check" => serde_json::to_value(service.check()).expect("result serializes"),
                "analyze" => serde_json::to_value(service.analyze()).expect("result serializes"),
                _ => serde_json::to_value(service.inspect()).expect("result serializes"),
            }
        }
        other => {
            return jsonrpc_optional_error(
                has_id,
                id,
                -32601,
                format!("Method not found: {other}"),
            );
        }
    };

    has_id.then(|| jsonrpc_result(id, response))
}

fn tool_response_result(response: wright_driver::service::ToolResponse) -> Value {
    match response {
        wright_driver::service::ToolResponse::Ok { result } => result,
        wright_driver::service::ToolResponse::Error { error } => serde_json::json!({
            "error": error,
        }),
    }
}

fn jsonrpc_result(id: Value, result: Value) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn jsonrpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into(),
        },
    })
}

fn jsonrpc_optional_error(
    has_id: bool,
    id: Value,
    code: i64,
    message: impl Into<String>,
) -> Option<Value> {
    has_id.then(|| jsonrpc_error(id, code, message))
}

/// Dispatch one request JSON line through the tool service.
fn dispatch(service: &mut ToolService<'_>, line: &str) -> String {
    let request: ToolRequest = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(error) => {
            return serde_json::to_string(&serde_json::json!({
                "error": { "code": "malformed-request", "message": error.to_string() },
            }))
            .expect("error serializes");
        }
    };
    let response = service.handle(&request);
    serde_json::to_string(&response).expect("response serializes")
}
