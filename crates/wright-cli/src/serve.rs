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

pub(crate) fn parse_kind(value: &str) -> Result<SourceKind, String> {
    SourceKind::parse(value)
        .ok_or_else(|| "expected one of: auto, opy, ostw, workshop, protocol".to_string())
}

pub(crate) fn parse_profile(value: &str) -> Result<wright_driver::Profile, String> {
    wright_driver::Profile::parse(value).ok_or_else(|| "expected one of: off, compat".to_string())
}

/// Explicit lint-policy configuration (`--lint-config`, `--rule-file`,
/// `--disable-rule`, `--rule-severity`) shared by `lint` and `serve`
/// (#594): both surfaces apply the same file, local rule paths, and CLI
/// overrides to the session's effective policy — the file first, then the
/// flag overrides in order. This type lives here rather than in `cli.rs`
/// so the standalone `wright-serve` binary — which `#[path]`-includes this
/// module — sees it too.
#[derive(Debug, Args, Default)]
pub(crate) struct LintPolicyArgs {
    /// Read project lint configuration YAML.
    #[arg(long = "lint-config", value_name = "PATH")]
    pub(crate) lint_config: Option<std::path::PathBuf>,
    /// Load a local YAML rule file or directory (repeatable).
    #[arg(long = "rule-file", value_name = "PATH")]
    pub(crate) rule_file: Vec<std::path::PathBuf>,
    /// Disable a lint rule (repeatable).
    #[arg(long = "disable-rule", value_name = "ID")]
    pub(crate) disable_rule: Vec<String>,
    /// Override a lint rule policy as ID:off, ID:warn, or ID:error (repeatable).
    #[arg(long = "rule-severity", value_name = "ID:SEVERITY")]
    pub(crate) rule_severity: Vec<String>,
}

impl LintPolicyArgs {
    pub(crate) fn apply_to(&self, config: &mut SessionConfig) -> Result<(), String> {
        if let Some(path) = &self.lint_config {
            config.lint = wright_driver::config::LintConfig::from_yaml_path(path)
                .map_err(|error| format!("cannot read lint config {}: {error}", path.display()))?;
        }
        config.lint_rule_paths = self.rule_file.clone();
        for rule in &self.disable_rule {
            config.lint.disable(rule);
        }
        for value in &self.rule_severity {
            let Some((rule_id, severity)) = value.split_once(':') else {
                return Err(format!(
                    "--rule-severity expects <ID>:<SEVERITY> (got '{value}')"
                ));
            };
            if !config.lint.set_severity_by_name(rule_id, severity) {
                return Err(format!(
                    "unknown severity '{severity}' (expected off|warn|error)"
                ));
            }
        }
        Ok(())
    }
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
    #[command(flatten)]
    policy: LintPolicyArgs,
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
    let mut config = SessionConfig {
        kind: args.kind,
        source_backend: SourceBackend::Auto,
        locale: args.locale,
        profile: args.profile,
        input: InputSpec::Path(args.input.unwrap_or_else(|| ".".into())),
        ..SessionConfig::default()
    };
    if args.profile != wright_driver::Profile::Off
        && matches!(args.kind, SourceKind::Opy | SourceKind::Ostw)
    {
        eprintln!(
            "wright: --profile applies to Workshop input only; remove it or pass --kind workshop"
        );
        return ExitCode::from(2);
    }
    if let Err(message) = args.policy.apply_to(&mut config) {
        eprintln!("wright: {message}");
        return ExitCode::from(2);
    }
    let mut session = match wright_driver::CompilerSession::new(config) {
        Ok(session) => session,
        Err(diagnostic) => {
            eprintln!("wright: {}", diagnostic.message);
            return ExitCode::from(2);
        }
    };
    let mut service = match ToolService::new(&mut session) {
        Ok(service) => service,
        Err(diagnostic) => {
            eprintln!("wright: {}", diagnostic.message);
            return ExitCode::from(2);
        }
    };

    match args.transport {
        TransportArg::Stdio => serve_stdio(&mut service),
        TransportArg::Jsonrpc => serve_jsonrpc(&mut service),
        TransportArg::Mcp => crate::mcp::serve_mcp(&mut service),
    }
}

fn serve_stdio(service: &mut ToolService<'_>) -> ExitCode {
    serve_lines(|line| {
        let line = match line {
            Ok(line) => line,
            Err(_) => {
                return Some(
                    json!({ "error": { "code": "malformed-request", "message": "request is not valid UTF-8" } })
                        .to_string(),
                );
            }
        };
        Some(dispatch(service, line))
    })
}

fn serve_jsonrpc(service: &mut ToolService<'_>) -> ExitCode {
    serve_lines(|line| {
        let value = match line {
            Ok(line) => serde_json::from_str(line).ok(),
            Err(_) => None,
        };
        match value {
            Some(value) => jsonrpc_dispatch(service, value),
            None => Some(jsonrpc_error(Value::Null, -32700, "Parse error")),
        }
        .map(|response| response.to_string())
    })
}

pub(crate) fn serve_lines(
    mut dispatch: impl FnMut(Result<&str, std::str::Utf8Error>) -> Option<String>,
) -> ExitCode {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in std::io::stdin().lock().split(b'\n').map_while(Result::ok) {
        let line = std::str::from_utf8(&line);
        if line.is_ok_and(|line| line.trim().is_empty()) {
            continue;
        }
        if dispatch(line).is_some_and(|response| writeln!(out, "{response}").is_err()) {
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
        "compile" => operation_result(
            service,
            ToolRequest::Compile {
                output: None,
                force: false,
                to: None,
            },
        ),
        "check" => operation_result(service, ToolRequest::Check { all: false }),
        "analyze" => operation_result(
            service,
            ToolRequest::Analyze {
                selection: Default::default(),
                all: false,
            },
        ),
        "inspect" => operation_result(service, ToolRequest::Inspect { all: false }),
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

pub(crate) fn dispatch(service: &mut ToolService<'_>, line: &str) -> String {
    let request: ToolRequest = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(error) => {
            return json!({ "error": { "code": "malformed-request", "message": error.to_string() } })
                .to_string();
        }
    };
    serde_json::to_string(&service.handle(&request)).expect("serializes")
}
