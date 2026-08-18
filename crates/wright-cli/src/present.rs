//! CLI-only presentation policy and renderers.
//!
//! The driver owns structured envelopes and diagnostics. This module owns how
//! those existing values are presented to a terminal, a pipe, or GitHub
//! Actions. JSON and source artifacts bypass every human/CI renderer.

use std::io::{IsTerminal, Write};

use wright_driver::Severity;
use wright_driver::config::OutputFormat;
use wright_driver::result::Envelope;

use crate::cli::{ColorArg, CommonArgs, OutputFormatArg, RendererArg};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Presentation {
    format: OutputFormat,
    renderer: Renderer,
    color: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Renderer {
    Terminal,
    Plain,
    GithubActions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuntimeEnvironment {
    github_actions: bool,
    ci: bool,
    stdout_terminal: bool,
    no_color: bool,
    force_color: bool,
    term_dumb: bool,
}

impl RuntimeEnvironment {
    fn process() -> Self {
        Self {
            github_actions: env_truthy("GITHUB_ACTIONS"),
            ci: env_truthy("CI"),
            stdout_terminal: std::io::stdout().is_terminal(),
            no_color: std::env::var_os("NO_COLOR").is_some(),
            force_color: env_truthy("FORCE_COLOR"),
            term_dumb: std::env::var("TERM").is_ok_and(|term| term == "dumb"),
        }
    }
}

impl Presentation {
    pub(crate) fn from_common(common: &CommonArgs) -> Self {
        let format = match common.format {
            OutputFormatArg::Text => OutputFormat::Text,
            OutputFormatArg::Json => OutputFormat::Json,
        };
        Self::resolve(
            format,
            common.renderer,
            common.color,
            RuntimeEnvironment::process(),
        )
    }

    fn resolve(
        format: OutputFormat,
        renderer: RendererArg,
        color: ColorArg,
        environment: RuntimeEnvironment,
    ) -> Self {
        let renderer = match renderer {
            RendererArg::Terminal => Renderer::Terminal,
            RendererArg::Plain => Renderer::Plain,
            RendererArg::GithubActions => Renderer::GithubActions,
            RendererArg::Auto => {
                if environment.github_actions {
                    Renderer::GithubActions
                } else if environment.ci || !environment.stdout_terminal {
                    Renderer::Plain
                } else {
                    Renderer::Terminal
                }
            }
        };
        let color = match color {
            ColorArg::Always => renderer != Renderer::GithubActions,
            ColorArg::Never => false,
            ColorArg::Auto => {
                renderer == Renderer::Terminal && !environment.no_color && !environment.term_dumb
                    || environment.force_color
                        && renderer == Renderer::Terminal
                        && !environment.no_color
            }
        };
        Self {
            format,
            renderer,
            color,
        }
    }
}

fn env_truthy(name: &str) -> bool {
    match std::env::var(name) {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "no"
        ),
        Err(_) => false,
    }
}

/// Render one result envelope. JSON is deliberately handled before renderer
/// selection so it can never receive ANSI, progress, or workflow commands.
pub(crate) fn render<T: serde::Serialize>(envelope: &Envelope<T>, presentation: Presentation) {
    if presentation.format == OutputFormat::Json {
        let value = serde_json::to_value(envelope).expect("envelope serializes");
        let text = serde_json::to_string_pretty(&value).expect("envelope serializes");
        println!("{text}");
        return;
    }

    match presentation.renderer {
        Renderer::GithubActions => render_github(envelope),
        Renderer::Terminal | Renderer::Plain => render_text(envelope, presentation.color),
    }
}

fn render_text<T: serde::Serialize>(envelope: &Envelope<T>, color: bool) {
    for diagnostic in &envelope.diagnostics {
        render_diagnostic(diagnostic, color);
    }
    if envelope.command == "check" {
        render_ostw_summary(envelope);
    }
    if !envelope.ok {
        if envelope.diagnostics.is_empty() {
            eprintln!("{}: failed", envelope.command);
        }
        return;
    }

    match envelope.command.as_str() {
        "compile" => render_compile(envelope),
        "convert" => render_convert(envelope),
        "check" => println!("check: ok"),
        "analyze" => render_analyze(envelope),
        "lint" => render_lint(envelope),
        "inspect" => render_inspect(envelope),
        other => println!("{other}: ok"),
    }
}

fn render_github<T: serde::Serialize>(envelope: &Envelope<T>) {
    for diagnostic in &envelope.diagnostics {
        emit_diagnostic_annotation(diagnostic);
    }
    let value = serde_json::to_value(envelope).expect("envelope serializes");
    if let Some(findings) = value
        .pointer("/result/findings")
        .and_then(serde_json::Value::as_array)
    {
        for finding in findings {
            emit_finding_annotation(finding);
        }
    }

    eprintln!(
        "::group::{}",
        escape_workflow_data(&format!("wright {}", envelope.command))
    );
    if envelope.command == "compile" {
        render_compile(envelope);
    } else if envelope.command == "convert" {
        render_convert(envelope);
    } else {
        let status = summary_status(envelope, &value);
        eprintln!("{status} {}", envelope.command);
    }
    eprintln!("::endgroup::");
    emit_summary(envelope, &value);
}

fn emit_diagnostic_annotation(diagnostic: &wright_driver::Diagnostic) {
    let kind = match diagnostic.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "notice",
    };
    let mut properties = vec![format!(
        "title={}",
        escape_workflow_property(&diagnostic.code)
    )];
    if let Some(span) = diagnostic
        .span
        .as_ref()
        .filter(|span| is_real_source_path(&span.path))
    {
        properties.insert(0, format!("file={}", escape_workflow_property(&span.path)));
        properties.push(format!("line={}", span.start.line));
        properties.push(format!("col={}", span.start.col));
        properties.push(format!("endLine={}", span.end.line));
        properties.push(format!("endColumn={}", span.end.col));
    }
    eprintln!(
        "::{kind} {}::{}",
        properties.join(","),
        escape_workflow_data(&diagnostic.message)
    );
}

fn emit_finding_annotation(finding: &serde_json::Value) {
    let severity = match finding.get("severity").and_then(serde_json::Value::as_str) {
        Some("error") => Severity::Error,
        Some("warning") => Severity::Warning,
        _ => Severity::Info,
    };
    let span = finding.get("span").filter(|span| span.is_object());
    let Some(span) = span else { return };
    let path = span
        .get("path")
        .and_then(serde_json::Value::as_str)
        .filter(|path| is_real_source_path(path));
    let Some(path) = path else { return };
    let line = span
        .pointer("/start/line")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);
    let col = span
        .pointer("/start/col")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);
    let end_line = span
        .pointer("/end/line")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(line);
    let end_col = span
        .pointer("/end/col")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(col);
    let code = finding
        .get("code")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("finding");
    let message = finding
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let kind = match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "notice",
    };
    eprintln!(
        "::{kind} file={},line={line},col={col},endLine={end_line},endColumn={end_col},title={}::{}",
        escape_workflow_property(path),
        escape_workflow_property(code),
        escape_workflow_data(message)
    );
}

fn summary_status<T: serde::Serialize>(
    envelope: &Envelope<T>,
    value: &serde_json::Value,
) -> &'static str {
    if !envelope.ok
        || envelope
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        "ERROR"
    } else if envelope
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Warning)
        || value
            .pointer("/result/findings")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|findings| !findings.is_empty())
    {
        "WARN"
    } else {
        "PASS"
    }
}

fn emit_summary<T: serde::Serialize>(envelope: &Envelope<T>, value: &serde_json::Value) {
    let status = summary_status(envelope, value);
    let line = format!(
        "Wright `{}`: **{status}** (exit {})",
        envelope.command, envelope.exit
    );
    if let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        let result = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| writeln!(file, "{line}"));
        if let Err(error) = result {
            eprintln!(
                "::warning title=Wright summary::{}",
                escape_workflow_data(&error.to_string())
            );
        }
    } else {
        eprintln!(
            "::notice title=Wright summary::{}",
            escape_workflow_data(&line)
        );
    }
}

/// GitHub workflow command escaping: properties additionally escape `:` and
/// `,`; command data only needs `%`, CR, and LF escaping.
pub(crate) fn escape_workflow_property(value: &str) -> String {
    escape_workflow_data(value)
        .replace(':', "%3A")
        .replace(',', "%2C")
}

pub(crate) fn escape_workflow_data(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn is_real_source_path(path: &str) -> bool {
    !path.is_empty() && !path.starts_with('<')
}

fn render_compile<T: serde::Serialize>(envelope: &Envelope<T>) {
    let value = serde_json::to_value(envelope).expect("envelope serializes");
    let Some(output) = value.pointer("/result/output") else {
        eprintln!("compile: failed (no output produced)");
        return;
    };
    let text = output
        .get("text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if output.get("written_to").and_then(serde_json::Value::as_str) != Some("stdout") {
        return;
    }
    print!("{text}");
}

fn render_convert<T: serde::Serialize>(envelope: &Envelope<T>) {
    let value = serde_json::to_value(envelope).expect("envelope serializes");
    let Some(result) = value.pointer("/result") else {
        eprintln!("convert: failed (no source produced)");
        return;
    };
    let text = result
        .get("text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    print!("{text}");
}

fn render_analyze<T: serde::Serialize>(envelope: &Envelope<T>) {
    let value = serde_json::to_value(envelope).expect("envelope serializes");
    let findings = value
        .pointer("/result/findings")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    println!(
        "analyze: {} finding(s), {} diagnostic(s)",
        findings.len(),
        envelope.diagnostics.len()
    );
    for finding in &findings {
        let code = finding
            .get("code")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("finding");
        let severity = finding
            .get("severity")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("info");
        let message = finding
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        println!("  {severity}[{code}]: {message}");
        if let Some(span) = finding.get("span") {
            print_span(span, "      ");
        }
    }
}

fn render_lint<T: serde::Serialize>(envelope: &Envelope<T>) {
    let value = serde_json::to_value(envelope).expect("envelope serializes");
    let findings = value
        .pointer("/result/findings")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let rules = value
        .pointer("/result/rules")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    println!(
        "lint: {} finding(s) across {} rule(s), {} diagnostic(s)",
        findings.len(),
        rules.len(),
        envelope.diagnostics.len()
    );
    for finding in &findings {
        let code = finding
            .get("code")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("finding");
        let severity = finding
            .get("severity")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("info");
        let evidence = finding
            .get("evidence")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("exact");
        let boundedness = finding
            .get("boundedness")
            .and_then(serde_json::Value::as_str);
        let message = finding
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        match boundedness {
            Some(value) => println!(
                "  {severity}[{code}] (evidence: {evidence}) (boundedness: {value}): {message}"
            ),
            None => println!("  {severity}[{code}] (evidence: {evidence}): {message}"),
        }
        if let Some(span) = finding.get("span") {
            print_span(span, "      ");
        }
    }
}

fn render_inspect<T: serde::Serialize>(envelope: &Envelope<T>) {
    let value = serde_json::to_value(envelope).expect("envelope serializes");
    let program = value
        .pointer("/result/program")
        .cloned()
        .unwrap_or_default();
    let rules = value
        .pointer("/result/rules")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let symbols = value
        .pointer("/result/symbols")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let count = program
        .get("rules")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    println!("inspect: {count} rule(s), {} symbol(s)", symbols.len());
    for rule in &rules {
        let id = rule
            .get("id")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let name = rule
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("<unnamed>");
        println!("  rule {id}: \"{name}\"");
    }
    for symbol in &symbols {
        let id = symbol
            .get("id")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let kind = symbol
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("symbol");
        let name = symbol
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("<unnamed>");
        println!("  {kind} {id}: {name}");
    }
}

fn render_ostw_summary<T: serde::Serialize>(envelope: &Envelope<T>) {
    let Ok(value) = serde_json::to_value(envelope) else {
        return;
    };
    let Some(ostw) = value.pointer("/result/ostw") else {
        return;
    };
    let entry = ostw
        .get("entry")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let files = ostw
        .get("files")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let sources: Vec<_> = files
        .iter()
        .filter(|file| {
            file.get("source")
                .and_then(|s| s.as_bool())
                .unwrap_or(false)
        })
        .collect();
    let parsed = sources
        .iter()
        .filter(|file| {
            file.get("parsed")
                .and_then(|p| p.as_bool())
                .unwrap_or(false)
        })
        .count();
    let inventory = ostw
        .get("inventory")
        .and_then(serde_json::Value::as_array)
        .map(|list| list.len())
        .unwrap_or(0);
    println!(
        "ostw project: entry {entry}, {parsed}/{} import-reachable sources parsed (inventory {inventory})",
        sources.len()
    );
    for file in &files {
        let path = file
            .get("path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if !file
            .get("source")
            .and_then(|s| s.as_bool())
            .unwrap_or(false)
        {
            println!("  {path} (project file)");
        } else {
            let status = if file
                .get("parsed")
                .and_then(|p| p.as_bool())
                .unwrap_or(false)
            {
                "parsed"
            } else {
                "parse-error"
            };
            println!("  {path} {status}");
        }
    }
}

fn render_diagnostic(diagnostic: &wright_driver::Diagnostic, color: bool) {
    let severity = match diagnostic.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    };
    let label = if color {
        ansi_severity(diagnostic.severity, severity)
    } else {
        severity.to_string()
    };
    eprintln!(
        "{label}[{}] ({}): {}",
        diagnostic.code,
        diagnostic.stage.as_str(),
        diagnostic.message
    );
    if let Some(span) = &diagnostic.span {
        eprintln!("  --> {}:{}:{}", span.path, span.start.line, span.start.col);
    }
}

fn ansi_severity(severity: Severity, value: &str) -> String {
    let code = match severity {
        Severity::Error => "31",
        Severity::Warning => "33",
        Severity::Info => "36",
    };
    format!("\x1b[{code}m{value}\x1b[0m")
}

fn print_span(span: &serde_json::Value, indent: &str) {
    let path = span
        .get("path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<span>");
    let line = span
        .pointer("/start/line")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let col = span
        .pointer("/start/col")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    println!("{indent}--> {path}:{line}:{col}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment() -> RuntimeEnvironment {
        RuntimeEnvironment {
            github_actions: false,
            ci: false,
            stdout_terminal: true,
            no_color: false,
            force_color: false,
            term_dumb: false,
        }
    }

    #[test]
    fn auto_renderer_prefers_github_actions_then_ci_then_terminal() {
        let mut env = environment();
        assert_eq!(
            Presentation::resolve(OutputFormat::Text, RendererArg::Auto, ColorArg::Auto, env)
                .renderer,
            Renderer::Terminal
        );
        env.ci = true;
        assert_eq!(
            Presentation::resolve(OutputFormat::Text, RendererArg::Auto, ColorArg::Auto, env)
                .renderer,
            Renderer::Plain
        );
        env.github_actions = true;
        assert_eq!(
            Presentation::resolve(OutputFormat::Text, RendererArg::Auto, ColorArg::Auto, env)
                .renderer,
            Renderer::GithubActions
        );
    }

    #[test]
    fn explicit_renderer_and_color_override_detection() {
        let mut env = environment();
        env.github_actions = true;
        env.no_color = true;
        let terminal = Presentation::resolve(
            OutputFormat::Text,
            RendererArg::Terminal,
            ColorArg::Always,
            env,
        );
        assert_eq!(terminal.renderer, Renderer::Terminal);
        assert!(terminal.color);
        let plain = Presentation::resolve(
            OutputFormat::Text,
            RendererArg::Plain,
            ColorArg::Always,
            env,
        );
        assert_eq!(plain.renderer, Renderer::Plain);
        assert!(
            plain.color,
            "explicit color wins over auto renderer detection"
        );
        let never = Presentation::resolve(
            OutputFormat::Text,
            RendererArg::Terminal,
            ColorArg::Never,
            env,
        );
        assert!(!never.color);
    }

    #[test]
    fn workflow_command_escaping_is_split_by_context() {
        assert_eq!(escape_workflow_property("a,b:c%\n"), "a%2Cb%3Ac%25%0A");
        assert_eq!(escape_workflow_data("a,b:c%\n"), "a,b:c%25%0A");
    }
}
