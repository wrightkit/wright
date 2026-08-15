//! `wright-serve` — thin transport adapters over the session-aware tool
//! service (M9, issue #60).
//!
//! Exposes the same operations as [`wright_driver::service::ToolService`]
//! over two transports:
//!
//! * **stdio JSON-lines** (`--transport stdio`): one request per line, one
//!   response per line (the M4 tool style, generalized to the M9 service).
//! * **JSON-RPC 2.0** (`--transport jsonrpc`): standard JSON-RPC envelopes
//!   with `id`/`method`/`params` and `result`/`error` responses.
//!
//! Both are thin mappings: no semantic logic lives here, so behavior is
//! identical to in-process consumers. MCP is intentionally not implemented —
//! no agent-integration evidence justified it in v1.

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
        let value: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => {
                let _ = writeln!(
                    out,
                    "{{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{{\"code\":-32700,\"message\":\"parse error\"}}}}"
                );
                continue;
            }
        };
        let id = value.get("id").cloned();
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = value.get("params").cloned();
        let result = match method {
            "request" => {
                // The full params object is forwarded verbatim (op plus any
                // operation arguments, e.g. mutation sources/targets, M14
                // #130) so the JSON-RPC adapter maps the same request shape
                // as the stdio adapter; a params object without `op` is a
                // malformed request rather than a silently empty one.
                let request_line = match params {
                    Some(params) if params.get("op").is_some() => {
                        serde_json::to_string(&params).expect("params serialize")
                    }
                    _ => "{}".to_string(),
                };
                dispatch(service, &request_line)
            }
            "compile" | "check" | "analyze" | "inspect" => {
                let envelope = match method {
                    "compile" => serde_json::to_value(service.compile()),
                    "check" => serde_json::to_value(service.check()),
                    "analyze" => serde_json::to_value(service.analyze()),
                    _ => serde_json::to_value(service.inspect()),
                };
                serde_json::to_string(&serde_json::json!({
                    "result": envelope.expect("envelope serializes"),
                }))
                .expect("response serializes")
            }
            other => serde_json::to_string(&serde_json::json!({
                "error": { "code": -32601, "message": format!("method not found: {other}") },
            }))
            .expect("error serializes"),
        };
        let result_value: Value = serde_json::from_str(&result).unwrap_or(Value::Null);
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result_value,
        });
        if writeln!(out, "{response}").is_err() {
            break;
        }
    }
    ExitCode::SUCCESS
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
