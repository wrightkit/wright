use std::io::{BufRead, Write};
use std::process::ExitCode;

use clap::{Args, ValueEnum};
use serde_json::{Value, json};
use wright_driver::config::{InputSpec, SessionConfig, SourceKind};
use wright_driver::service::{ToolRequest, ToolService};
use wright_driver::source_provider::SourceBackend;

/// The `serve` request transports. Every value names a real adapter — an
/// unknown spelling is a usage error at startup, never a silent fallback
/// (#593).
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum TransportArg {
    Stdio,
    Jsonrpc,
    Mcp,
}

/// Parse `--kind` against the same vocabulary the workflow commands accept
/// — a typo is a usage error naming the accepted spellings, not a silent
/// `auto` (#593).
fn parse_kind(value: &str) -> Result<SourceKind, String> {
    SourceKind::parse(value)
        .ok_or_else(|| "expected one of: auto, opy, ostw, workshop, protocol".to_string())
}

/// Parse `--profile` against the workflow vocabulary (#593).
fn parse_profile(value: &str) -> Result<wright_driver::Profile, String> {
    wright_driver::Profile::parse(value)
        .ok_or_else(|| "expected one of: off, compat, aggressive".to_string())
}

#[derive(Debug, Args)]
pub(crate) struct ServeArgs {
    /// Request transport.
    #[arg(long, value_enum, default_value_t = TransportArg::Stdio)]
    transport: TransportArg,
    /// Input frontend.
    #[arg(long, value_parser = parse_kind, default_value = "auto")]
    kind: SourceKind,
    /// Workshop client locale override.
    #[arg(long, value_name = "LOCALE")]
    locale: Option<String>,
    /// WIR transformation policy.
    #[arg(long, value_parser = parse_profile, default_value = "off")]
    profile: wright_driver::Profile,
    #[arg(value_name = "INPUT")]
    input: Option<std::path::PathBuf>,
}

pub(crate) fn run(args: ServeArgs) -> ExitCode {
    if args
        .input
        .as_deref()
        .is_some_and(|input| input == std::path::Path::new("-"))
    {
        eprintln!(
            "wright: serve reserves stdin for requests; pass a source file or project directory"
        );
        return ExitCode::from(2);
    }
    let config = SessionConfig {
        kind: args.kind,
        source_backend: SourceBackend::Auto,
        locale: args.locale,
        profile: args.profile,
        input: InputSpec::Path(args.input.unwrap_or_else(|| ".".into())),
        ..SessionConfig::default()
    };
    let mut session = match wright_driver::CompilerSession::new(config) {
        Ok(session) => session,
        Err(diagnostic) => {
            eprintln!("wright: {}", diagnostic.message);
            return ExitCode::from(1);
        }
    };
    let mut service = match ToolService::new(&mut session) {
        Ok(service) => service,
        Err(diagnostic) => {
            eprintln!("wright: {}", diagnostic.message);
            return ExitCode::from(1);
        }
    };

    match args.transport {
        TransportArg::Stdio => serve_stdio(&mut service),
        TransportArg::Jsonrpc => serve_jsonrpc(&mut service),
        TransportArg::Mcp => crate::mcp::serve_mcp(&mut service),
    }
}

fn serve_stdio(service: &mut ToolService<'_>) -> ExitCode {
    serve_lines(|line| Some(dispatch(service, line)))
}

fn serve_jsonrpc(service: &mut ToolService<'_>) -> ExitCode {
    serve_lines(|line| {
        match serde_json::from_str(line) {
            Ok(value) => jsonrpc_dispatch(service, value),
            Err(_) => Some(jsonrpc_error(Value::Null, -32700, "Parse error")),
        }
        .map(|response| response.to_string())
    })
}

pub(crate) fn serve_lines(mut dispatch: impl FnMut(&str) -> Option<String>) -> ExitCode {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        if dispatch(&line).is_some_and(|response| writeln!(out, "{response}").is_err()) {
            break;
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
    let Some(object) = value.as_object() else {
        return Some(jsonrpc_error(Value::Null, -32600, "Invalid Request"));
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

    if matches!(method, "compile" | "check" | "analyze" | "inspect")
        && object.contains_key("params")
    {
        return has_id.then(|| jsonrpc_error(id, -32602, "Invalid params"));
    }

    let response = match method {
        "request" => {
            let params = match object.get("params") {
                Some(Value::Object(params)) if params.get("op").is_some() => {
                    Value::Object(params.clone())
                }
                _ => return has_id.then(|| jsonrpc_error(id, -32602, "Invalid params")),
            };
            let Ok(request) = serde_json::from_value::<ToolRequest>(params) else {
                return has_id.then(|| jsonrpc_error(id, -32602, "Invalid params"));
            };
            operation_result(service, request)
        }
        "compile" => operation_result(service, ToolRequest::Compile),
        "check" => operation_result(service, ToolRequest::Check),
        "analyze" => operation_result(service, ToolRequest::Analyze { brief: false }),
        "inspect" => operation_result(service, ToolRequest::Inspect { brief: false }),
        other => {
            return has_id.then(|| jsonrpc_error(id, -32601, format!("Method not found: {other}")));
        }
    };

    has_id.then(|| json!({ "jsonrpc": "2.0", "id": id, "result": response }))
}

fn operation_result(service: &mut ToolService<'_>, request: ToolRequest) -> Value {
    match service.handle(&request) {
        wright_driver::service::ToolResponse::Ok { result } => result,
        wright_driver::service::ToolResponse::Error { error } => json!({ "error": error }),
    }
}

fn jsonrpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } })
}

fn dispatch(service: &mut ToolService<'_>, line: &str) -> String {
    let request: ToolRequest = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(error) => {
            return json!({ "error": { "code": "malformed-request", "message": error.to_string() } })
                .to_string();
        }
    };
    serde_json::to_string(&service.handle(&request)).expect("serializes")
}
