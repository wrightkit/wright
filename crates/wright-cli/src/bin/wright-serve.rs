//! Exposes the same operations as [`wright_driver::service::ToolService`] over two transports.

use std::io::{BufRead, Write};
use std::process::ExitCode;

use serde_json::{Value, json};
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
            "--transport" => transport = args.next().unwrap_or_else(|| "stdio".to_string()),
            "--kind" => {
                config.kind =
                    SourceKind::parse(&args.next().unwrap_or_default()).unwrap_or(SourceKind::Auto)
            }
            "--locale" => config.locale = args.next(),
            "--profile" => {
                config.profile = wright_driver::Profile::parse(&args.next().unwrap_or_default())
                    .unwrap_or_default()
            }
            "--help" | "-h" => {
                println!("{}", usage());
                return ExitCode::SUCCESS;
            }
            other if other.starts_with('-') => {
                eprintln!("wright-serve: unknown argument '{other}'\n{}", usage());
                return ExitCode::from(2);
            }
            other => positional = Some(std::path::PathBuf::from(other)),
        }
    }

    config.input = positional.map(InputSpec::Path).unwrap_or(InputSpec::Stdin);
    let mut session = match wright_driver::CompilerSession::new(config) {
        Ok(s) => s,
        Err(d) => {
            eprintln!("wright-serve: {}", d.message);
            return ExitCode::from(1);
        }
    };
    let mut service = match ToolService::new(&mut session) {
        Ok(s) => s,
        Err(d) => {
            eprintln!("wright-serve: {}", d.message);
            return ExitCode::from(1);
        }
    };

    match transport.as_str() {
        "stdio" => serve_stdio(&mut service),
        "jsonrpc" => serve_jsonrpc(&mut service),
        other => {
            eprintln!("wright-serve: unknown transport '{other}'\n{}", usage());
            ExitCode::from(2)
        }
    }
}

fn serve_stdio(service: &mut ToolService<'_>) -> ExitCode {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let resp = dispatch(service, &line);
        if writeln!(out, "{resp}").is_err() {
            break;
        }
    }
    ExitCode::SUCCESS
}

fn serve_jsonrpc(service: &mut ToolService<'_>) -> ExitCode {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let resp = match serde_json::from_str(&line) {
            Ok(val) => jsonrpc_dispatch(service, val),
            Err(_) => Some(jsonrpc_error(Value::Null, -32700, "Parse error")),
        };
        if let Some(r) = resp {
            if writeln!(out, "{r}").is_err() {
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
            .filter_map(|v| jsonrpc_dispatch_request(service, v))
            .collect();
        return (!responses.is_empty()).then_some(Value::Array(responses));
    }
    jsonrpc_dispatch_request(service, value)
}

fn jsonrpc_dispatch_request(service: &mut ToolService<'_>, value: Value) -> Option<Value> {
    let Some(object) = value.as_object() else {
        return Some(jsonrpc_error(Value::Null, -32600, "Invalid Request"));
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(jsonrpc_error(Value::Null, -32600, "Invalid Request"));
    }
    let method = match object.get("method").and_then(Value::as_str) {
        Some(m) => m,
        None => return Some(jsonrpc_error(Value::Null, -32600, "Invalid Request")),
    };
    let has_id = object.contains_key("id");
    let id = object.get("id").cloned().unwrap_or(Value::Null);
    if has_id && !matches!(id, Value::Null | Value::String(_) | Value::Number(_)) {
        return Some(jsonrpc_error(Value::Null, -32600, "Invalid Request"));
    }

    if matches!(method, "compile" | "check" | "analyze" | "inspect")
        && object.contains_key("params")
    {
        return has_id.then(|| jsonrpc_error(id, -32602, "Invalid params"));
    }

    let response = match method {
        "request" => {
            let params = match object.get("params") {
                Some(Value::Object(p)) if p.get("op").is_some() => Value::Object(p.clone()),
                _ => return has_id.then(|| jsonrpc_error(id, -32602, "Invalid params")),
            };
            let Ok(req) = serde_json::from_value::<ToolRequest>(params) else {
                return has_id.then(|| jsonrpc_error(id, -32602, "Invalid params"));
            };
            match service.handle(&req) {
                wright_driver::service::ToolResponse::Ok { result } => result,
                wright_driver::service::ToolResponse::Error { error } => json!({ "error": error }),
            }
        }
        "compile" => serde_json::to_value(service.compile()).expect("serializes"),
        "check" => serde_json::to_value(service.check()).expect("serializes"),
        "analyze" => serde_json::to_value(service.analyze()).expect("serializes"),
        "inspect" => serde_json::to_value(service.inspect()).expect("serializes"),
        other => {
            return has_id.then(|| jsonrpc_error(id, -32601, format!("Method not found: {other}")));
        }
    };

    has_id.then(|| json!({ "jsonrpc": "2.0", "id": id, "result": response }))
}

fn jsonrpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } })
}

fn dispatch(service: &mut ToolService<'_>, line: &str) -> String {
    let req: ToolRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            return json!({ "error": { "code": "malformed-request", "message": e.to_string() } })
                .to_string();
        }
    };
    serde_json::to_string(&service.handle(&req)).expect("serializes")
}
