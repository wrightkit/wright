//! CLI-only presentation policy and renderers.
//!
//! The driver owns structured envelopes and diagnostics. This module owns how
//! those existing values are presented to a terminal, a pipe, or GitHub
//! Actions. JSON and source artifacts bypass every human/CI renderer.

use std::io::{IsTerminal, Write};
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

use wright_driver::Severity;
use wright_driver::config::OutputFormat;
use wright_driver::progress::{ProgressEvent, ProgressObserver, ProgressPhase, ProgressUnit};
use wright_driver::result::{
    AnalyzeResult, CheckResult, CompileResult, ConvertResult, Envelope, InspectResult, LintResult,
};

use crate::cli::{ColorArg, CommonArgs, OutputFormatArg, RendererArg};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Presentation {
    format: OutputFormat,
    renderer: Renderer,
    color: bool,
    interactive: bool,
}

/// A deliberately small, boundary-only activity indicator for interactive
/// terminal runs. It never participates in the result contract and is never
/// created for JSON, plain, CI, or GitHub Actions rendering.
pub(crate) struct Activity {
    done: Arc<AtomicBool>,
    visible: Arc<AtomicBool>,
    status: Arc<Mutex<Option<ProgressEvent>>>,
    output: Arc<Mutex<()>>,
    #[cfg(test)]
    #[allow(dead_code)]
    frame: Arc<AtomicUsize>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Activity {
    fn new(active: bool) -> Self {
        let (done, visible, status, output) = (
            Arc::new(AtomicBool::new(!active)),
            Arc::new(AtomicBool::new(active)),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(())),
        );
        #[cfg(test)]
        let frame = Arc::new(AtomicUsize::new(0));
        let handle = if active {
            write_activity_line(&output, None, None);
            let (done, status, output) =
                (Arc::clone(&done), Arc::clone(&status), Arc::clone(&output));
            #[cfg(test)]
            let thread_frame = Arc::clone(&frame);
            Some(thread::spawn(move || {
                thread::sleep(Duration::from_millis(60));
                let mut frame = 0;
                while !done.load(Ordering::Acquire) {
                    let event = *status.lock().expect("activity status lock");
                    write_activity_line(&output, event, Some(SPINNER[frame]));
                    frame = (frame + 1) % SPINNER.len();
                    #[cfg(test)]
                    thread_frame.store(frame, Ordering::Release);
                    thread::sleep(Duration::from_millis(80));
                }
            }))
        } else {
            None
        };
        Self {
            done,
            visible,
            status,
            output,
            #[cfg(test)]
            frame,
            handle,
        }
    }
}

const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

impl ProgressObserver for Activity {
    fn on_progress(&self, event: ProgressEvent) {
        if self.done.load(Ordering::Acquire) {
            return;
        }
        *self.status.lock().expect("activity status lock") = Some(event);
        write_activity_line(&self.output, Some(event), None);
    }
}

fn write_activity_line(output: &Mutex<()>, event: Option<ProgressEvent>, spinner: Option<char>) {
    let _guard = output.lock().expect("activity output lock");
    let label = event
        .map(progress_label)
        .unwrap_or("Starting workflow".to_string());
    let prefix = spinner.map_or(String::new(), |s| format!("{s} "));
    eprint!("\r\x1b[2K\r  {prefix}{label}…");
    let _ = std::io::stderr().flush();
}

fn progress_label(event: ProgressEvent) -> String {
    let label = match event.phase {
        ProgressPhase::InputResolution => "Resolving input",
        ProgressPhase::ProjectLoading => "Loading project",
        ProgressPhase::Parsing => "Parsing",
        ProgressPhase::Validation => "Validating",
        ProgressPhase::Lowering => "Lowering",
        ProgressPhase::SemanticAnalysis => "Resolving semantics",
        ProgressPhase::Linting => "Running lint rules",
        ProgressPhase::Emission => "Emitting Workshop",
        ProgressPhase::Conversion => "Reconstructing source",
    };
    match (event.count, event.unit) {
        (Some(c), Some(ProgressUnit::Files)) => format!("{label} {c} files"),
        (Some(c), Some(ProgressUnit::Rules)) => format!("{label} {c} rules"),
        _ => label.to_string(),
    }
}

impl Drop for Activity {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        if self.visible.load(Ordering::Acquire) {
            clear_activity_line(&mut std::io::stderr());
        }
    }
}

fn clear_activity_line(writer: &mut impl Write) {
    let _ = write!(writer, "\r\x1b[2K\r\n");
    let _ = writer.flush();
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Renderer {
    Terminal,
    Plain,
    GithubActions,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
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
            term_dumb: std::env::var("TERM").is_ok_and(|t| t == "dumb"),
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
        env: RuntimeEnvironment,
    ) -> Self {
        let renderer = match renderer {
            RendererArg::Terminal => Renderer::Terminal,
            RendererArg::Plain => Renderer::Plain,
            RendererArg::GithubActions => Renderer::GithubActions,
            RendererArg::Auto => {
                if env.github_actions {
                    Renderer::GithubActions
                } else if env.ci || !env.stdout_terminal {
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
                renderer == Renderer::Terminal
                    && !env.no_color
                    && (!env.term_dumb || env.force_color)
            }
        };
        Self {
            format,
            renderer,
            color,
            interactive: renderer == Renderer::Terminal && env.stdout_terminal && !env.term_dumb,
        }
    }

    pub(crate) fn activity(&self) -> Activity {
        Activity::new(self.format == OutputFormat::Text && self.interactive)
    }
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| {
        !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "no"
        )
    })
}

/// Render one result envelope. JSON is deliberately handled before renderer
/// selection so it can never receive ANSI, progress, or workflow commands.
pub(crate) fn render<T: serde::Serialize + ResultPresentation>(
    envelope: &Envelope<T>,
    presentation: Presentation,
) {
    if presentation.format == OutputFormat::Json {
        let mut value = serde_json::to_value(envelope).expect("envelope serializes");
        if envelope.command == "check" {
            value["schema_version"] = serde_json::Value::String("1".to_string());
        }
        let text = serde_json::to_string_pretty(&value).expect("envelope serializes");
        println!("{text}");
        return;
    }

    match presentation.renderer {
        Renderer::GithubActions => render_github(envelope),
        Renderer::Terminal | Renderer::Plain => render_text(envelope, presentation.color),
    }
}

fn render_text<T: serde::Serialize + ResultPresentation>(envelope: &Envelope<T>, color: bool) {
    if !matches!(envelope.command.as_str(), "compile" | "convert") {
        render_verdict(envelope, color);
    }
    for diagnostic in &envelope.diagnostics {
        render_diagnostic(diagnostic, color);
    }
    if envelope.command == "check" {
        envelope.result.render_check_summary();
    }
    if !envelope.ok {
        if envelope.diagnostics.is_empty() {
            eprintln!("{}: failed", envelope.command);
        }
        return;
    }

    envelope.result.render_body();
}

fn render_verdict<T: serde::Serialize + ResultPresentation>(envelope: &Envelope<T>, color: bool) {
    let status = summary_status(envelope);
    let label = if color {
        let code = match status {
            "PASS" => "32",
            "WARN" => "33",
            _ => "31",
        };
        format!("\x1b[{code}m{status}\x1b[0m")
    } else {
        status.to_string()
    };
    println!("{label} {}", envelope.command);
    let metadata = match envelope.command.as_str() {
        "check" => format!("{} diagnostic(s)", envelope.diagnostics.len()),
        _ => envelope.result.metadata().unwrap_or_default(),
    };
    println!("  {}", dim(&metadata, color));
}

fn dim(value: &str, color: bool) -> String {
    if color {
        format!("\x1b[2m{value}\x1b[0m")
    } else {
        value.to_string()
    }
}

fn render_github<T: serde::Serialize + ResultPresentation>(envelope: &Envelope<T>) {
    for diagnostic in &envelope.diagnostics {
        emit_diagnostic_annotation(diagnostic);
    }
    envelope.result.render_github_findings();

    eprintln!(
        "::group::{}",
        escape_workflow_data(&format!("wright {}", envelope.command))
    );
    if matches!(envelope.command.as_str(), "compile" | "convert") {
        envelope.result.render_body();
    } else {
        eprintln!("{} {}", summary_status(envelope), envelope.command);
    }
    eprintln!("::endgroup::");
    emit_summary(envelope);
}

fn emit_workflow_annotation(
    kind: &str,
    title: &str,
    message: &str,
    span: Option<(&str, u64, u64, u64, u64)>,
) {
    let t = escape_workflow_property(title);
    let props = match span {
        Some((p, l, c, el, ec)) => format!(
            "file={},title={t},line={l},col={c},endLine={el},endColumn={ec}",
            escape_workflow_property(p)
        ),
        None => format!("title={t}"),
    };
    eprintln!("::{kind} {props}::{}", escape_workflow_data(message));
}

fn emit_diagnostic_annotation(diagnostic: &wright_driver::Diagnostic) {
    let kind = match diagnostic.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "notice",
    };
    let span = diagnostic
        .span
        .as_ref()
        .filter(|s| is_real_source_path(&s.path))
        .map(|s| {
            (
                s.path.as_str(),
                s.start.line as u64,
                s.start.col as u64,
                s.end.line as u64,
                s.end.col as u64,
            )
        });
    emit_workflow_annotation(kind, &diagnostic.code, &diagnostic.message, span);
}

fn emit_finding_annotation(finding: &serde_json::Value) {
    let kind = match str_field(finding, "severity", "info") {
        "error" => "error",
        "warning" => "warning",
        _ => "notice",
    };
    let span = finding.get("span").filter(|s| s.is_object()).and_then(|s| {
        let p = s.get("path")?.as_str().filter(|p| is_real_source_path(p))?;
        let l = span_position(s, "start", "line").unwrap_or(1);
        let c = span_position(s, "start", "col").unwrap_or(1);
        Some((
            p,
            l,
            c,
            span_position(s, "end", "line").unwrap_or(l),
            span_position(s, "end", "col").unwrap_or(c),
        ))
    });
    emit_workflow_annotation(
        kind,
        str_field(finding, "code", "finding"),
        str_field(finding, "message", ""),
        span,
    );
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum SummaryStatus {
    Pass,
    Warn,
    Error,
}

impl SummaryStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }

    fn from_finding_severity(severity: &str) -> Self {
        match severity {
            "error" => Self::Error,
            "warning" => Self::Warn,
            "info" | "notice" => Self::Pass,
            _ => Self::Warn,
        }
    }
}

pub(crate) trait ResultPresentation {
    fn metadata(&self) -> Option<String> {
        None
    }
    fn render_body(&self);
    fn render_check_summary(&self) {}
    fn render_github_findings(&self) {}
    fn update_summary_status(&self, _status: &mut SummaryStatus) {}
}

impl ResultPresentation for CompileResult {
    fn render_body(&self) {
        match &self.output {
            Some(out) if out.written_to == "stdout" => print!("{}", out.text),
            None => eprintln!("compile: failed (no output produced)"),
            _ => {}
        }
    }
}

impl ResultPresentation for ConvertResult {
    fn render_body(&self) {
        print!("{}", self.text);
    }
}

impl ResultPresentation for CheckResult {
    fn render_body(&self) {}

    fn render_check_summary(&self) {}
}

impl ResultPresentation for AnalyzeResult {
    fn metadata(&self) -> Option<String> {
        Some(format!(
            "{} rule(s), {} symbol(s); ranked semantic report",
            count(&self.program, "rules"),
            self.facts.get("symbols").map_or(0, array_len),
        ))
    }

    fn render_body(&self) {
        render_analyze(self);
    }
}

impl ResultPresentation for LintResult {
    fn metadata(&self) -> Option<String> {
        Some(format!(
            "{} finding(s) across {} rule(s)",
            array_len(&self.findings),
            array_len(&self.rules),
        ))
    }

    fn render_body(&self) {
        render_lint(self);
    }

    fn render_github_findings(&self) {
        if let Some(findings) = self.findings.as_array() {
            for finding in findings {
                emit_finding_annotation(finding);
            }
        }
    }

    fn update_summary_status(&self, status: &mut SummaryStatus) {
        if let Some(findings) = self.findings.as_array() {
            for finding in findings {
                if let Some(severity) = finding.get("severity").and_then(serde_json::Value::as_str)
                {
                    *status = (*status).max(SummaryStatus::from_finding_severity(severity));
                }
            }
        }
    }
}

impl ResultPresentation for InspectResult {
    fn metadata(&self) -> Option<String> {
        Some(format!(
            "{} rule(s), {} symbol(s)",
            array_len(&self.rules),
            array_len(&self.symbols),
        ))
    }

    fn render_body(&self) {
        let (rules, symbols) = (
            self.rules.as_array().map_or(&[][..], Vec::as_slice),
            self.symbols.as_array().map_or(&[][..], Vec::as_slice),
        );
        println!(
            "\nProgram structure\n  {} rule(s), {} symbol(s)",
            count(&self.program, "rules"),
            symbols.len()
        );
        for rule in rules {
            println!(
                "  rule {}: \"{}\"",
                u64_field(rule, "id"),
                str_field(rule, "name", "<unnamed>")
            );
        }
        for symbol in symbols {
            println!(
                "  {} {}: {}",
                str_field(symbol, "kind", "symbol"),
                u64_field(symbol, "id"),
                str_field(symbol, "name", "<unnamed>")
            );
        }
    }
}

fn summary_status<T: serde::Serialize + ResultPresentation>(
    envelope: &Envelope<T>,
) -> &'static str {
    let mut status = if envelope.ok {
        SummaryStatus::Pass
    } else {
        SummaryStatus::Error
    };
    for diagnostic in &envelope.diagnostics {
        status = status.max(match diagnostic.severity {
            Severity::Error => SummaryStatus::Error,
            Severity::Warning => SummaryStatus::Warn,
            Severity::Info => SummaryStatus::Pass,
        });
    }
    envelope.result.update_summary_status(&mut status);
    status.as_str()
}

fn emit_summary<T: serde::Serialize + ResultPresentation>(envelope: &Envelope<T>) {
    let status = summary_status(envelope);
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

fn str_field<'a>(value: &'a serde_json::Value, key: &str, default: &'a str) -> &'a str {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or(default)
}

fn u64_field(value: &serde_json::Value, key: &str) -> u64 {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
}

fn count(value: &serde_json::Value, key: &str) -> usize {
    u64_field(value, key) as usize
}

fn array_len(value: &serde_json::Value) -> usize {
    value.as_array().map_or(0, Vec::len)
}

fn span_position(span: &serde_json::Value, edge: &str, coordinate: &str) -> Option<u64> {
    span.get(edge)?.get(coordinate)?.as_u64()
}

fn render_analyze(result: &AnalyzeResult) {
    let (program, facts) = (&result.program, &result.facts);
    let symbols = facts
        .get("symbols")
        .and_then(serde_json::Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let rules = facts
        .get("rules")
        .and_then(serde_json::Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    println!(
        "\nProgram overview\n  {} file(s), {} rule(s), {} global variable(s), {} player variable(s), {} subroutine(s)\n  evidence: [static] parsed program inventory",
        count(program, "files"),
        count(program, "rules"),
        count(program, "globalVariables"),
        count(program, "playerVariables"),
        count(program, "subroutines"),
    );

    let (mut total_blocks, mut total_edges, mut total_loops, mut total_waits) = (0, 0, 0, 0);
    let mut rule_hotspots: Vec<_> = rules
        .iter()
        .map(|rule| {
            let flow = rule.get("controlFlow").cloned().unwrap_or_default();
            let (blocks, edges, loops, waits) = (
                count(&flow, "blocks"),
                count(&flow, "edges"),
                count(&flow, "loopBlocks"),
                count(&flow, "waitBlocks"),
            );
            total_blocks += blocks;
            total_edges += edges;
            total_loops += loops;
            total_waits += waits;
            (
                blocks + edges,
                str_field(rule, "name", "<unnamed>"),
                blocks,
                edges,
                loops,
                waits,
            )
        })
        .collect();
    rule_hotspots.sort_by(|l, r| r.0.cmp(&l.0).then_with(|| l.1.cmp(r.1)));
    println!(
        "\nControl-flow summary\n  {total_blocks} blocks, {total_edges} edges, {total_loops} loop block(s), {total_waits} wait block(s)\n  Top rules (heuristic ranking: blocks + edges; facts are [static])"
    );
    if rule_hotspots.is_empty() {
        println!("    none");
    } else {
        for (_, name, blocks, edges, loops, waits) in rule_hotspots.iter().take(5) {
            println!(
                "    {name}: {blocks} blocks, {edges} edges, {loops} loop block(s), {waits} wait block(s)"
            );
        }
    }

    let mut coupled_symbols: Vec<_> = symbols
        .iter()
        .filter(|s| {
            matches!(
                s.get("kind").and_then(serde_json::Value::as_str),
                Some("globalVariable" | "playerVariable")
            )
        })
        .map(|s| {
            let usage = s.get("usage").cloned().unwrap_or_default();
            let (rules, reads, writes) = (
                count(&usage, "rules"),
                count(&usage, "reads"),
                count(&usage, "writes"),
            );
            (
                rules,
                reads + writes,
                str_field(s, "kind", "variable"),
                str_field(s, "name", "<unnamed>"),
                reads,
                writes,
            )
        })
        .collect();
    coupled_symbols.sort_by(|l, r| {
        r.0.cmp(&l.0)
            .then_with(|| r.1.cmp(&l.1))
            .then_with(|| l.3.cmp(r.3))
    });
    println!(
        "\nState and coupling\n  Top variables (heuristic ranking: rules touched, then reads + writes)"
    );
    if coupled_symbols.is_empty() {
        println!("    none");
    } else {
        for (rules, _, kind, name, reads, writes) in coupled_symbols.iter().take(5) {
            println!(
                "    {kind} {name}: {rules} rule(s), {reads} read(s), {writes} write(s) [static]"
            );
        }
    }
}

fn render_lint(result: &LintResult) {
    let findings = result.findings.as_array().map_or(&[][..], Vec::as_slice);
    println!("\nLint findings");
    if findings.is_empty() {
        println!("  none");
    }
    for finding in findings {
        let (code, severity, evidence, message) = (
            str_field(finding, "code", "finding"),
            str_field(finding, "severity", "info"),
            str_field(finding, "evidence", "exact"),
            str_field(finding, "message", ""),
        );
        match finding
            .get("boundedness")
            .and_then(serde_json::Value::as_str)
        {
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
        render_source_context(&span.path, span.start.line, span.start.col, "  ");
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
    let line = span_position(span, "start", "line").unwrap_or(0);
    let col = span_position(span, "start", "col").unwrap_or(0);
    println!("{indent}--> {path}:{line}:{col}");
    render_source_context(path, line as u32, col as u32, indent);
}

fn render_source_context(path: &str, line: u32, col: u32, indent: &str) {
    let Ok(source) = std::fs::read_to_string(path) else {
        return;
    };
    let Some(text) = source.lines().nth(line.saturating_sub(1) as usize) else {
        return;
    };
    let number_width = line.to_string().len();
    println!("{indent}| {:>number_width$} | {text}", line);
    let marker_col = col.saturating_sub(1) as usize;
    let prefix = text
        .chars()
        .take(marker_col)
        .map(|ch| if ch == '\t' { '\t' } else { ' ' })
        .collect::<String>();
    println!("{indent}| {:>number_width$} | {prefix}^", "");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment() -> RuntimeEnvironment {
        RuntimeEnvironment {
            stdout_terminal: true,
            ..Default::default()
        }
    }

    #[test]
    fn auto_renderer_prefers_github_actions_then_ci_then_terminal() {
        let res = |gh, ci| {
            let env = RuntimeEnvironment {
                github_actions: gh,
                ci,
                stdout_terminal: true,
                ..Default::default()
            };
            Presentation::resolve(OutputFormat::Text, RendererArg::Auto, ColorArg::Auto, env)
                .renderer
        };
        assert_eq!(res(false, false), Renderer::Terminal);
        assert_eq!(res(false, true), Renderer::Plain);
        assert_eq!(res(true, false), Renderer::GithubActions);
    }

    #[test]
    fn explicit_renderer_and_color_override_detection() {
        let env = RuntimeEnvironment {
            stdout_terminal: true,
            github_actions: true,
            no_color: true,
            ..Default::default()
        };
        let p = |ren, col| Presentation::resolve(OutputFormat::Text, ren, col, env);
        let terminal = p(RendererArg::Terminal, ColorArg::Always);
        assert_eq!(
            (terminal.renderer, terminal.color),
            (Renderer::Terminal, true)
        );
        let plain = p(RendererArg::Plain, ColorArg::Always);
        assert_eq!((plain.renderer, plain.color), (Renderer::Plain, true));
        assert!(!p(RendererArg::Terminal, ColorArg::Never).color);
    }

    #[test]
    fn activity_is_only_enabled_for_interactive_text() {
        let check = |fmt, ren, col, dumb| {
            let env = RuntimeEnvironment {
                stdout_terminal: true,
                term_dumb: dumb,
                ..Default::default()
            };
            Presentation::resolve(fmt, ren, col, env)
                .activity()
                .handle
                .is_some()
        };
        assert!(check(
            OutputFormat::Text,
            RendererArg::Terminal,
            ColorArg::Never,
            false
        ));
        assert!(!check(
            OutputFormat::Text,
            RendererArg::Plain,
            ColorArg::Never,
            false
        ));
        assert!(!check(
            OutputFormat::Json,
            RendererArg::Terminal,
            ColorArg::Always,
            false
        ));
        assert!(!check(
            OutputFormat::Text,
            RendererArg::Terminal,
            ColorArg::Never,
            true
        ));
    }

    #[test]
    fn activity_lifecycle_and_rendering() {
        let activity = Presentation::resolve(
            OutputFormat::Text,
            RendererArg::Terminal,
            ColorArg::Never,
            environment(),
        )
        .activity();
        assert!(activity.visible.load(Ordering::Acquire));
        assert_eq!(*activity.status.lock().unwrap(), None);
        let event = ProgressEvent::with_count(ProgressPhase::Linting, 12, ProgressUnit::Rules);
        activity.on_progress(event);
        assert_eq!(*activity.status.lock().unwrap(), Some(event));
        thread::sleep(Duration::from_millis(150));
        assert!(activity.frame.load(Ordering::Acquire) > 0);

        let mut output = Vec::new();
        clear_activity_line(&mut output);
        assert_eq!(output, b"\r\x1b[2K\r\n");
    }

    #[test]
    fn escaping_and_styling() {
        assert_eq!(dim("2 symbols", false), "2 symbols");
        assert_eq!(dim("2 symbols", true), "\x1b[2m2 symbols\x1b[0m");
        assert_eq!(escape_workflow_property("a,b:c%\n"), "a%2Cb%3Ac%25%0A");
        assert_eq!(escape_workflow_data("a,b:c%\n"), "a,b:c%25%0A");
    }

    fn summary_envelope(sev: Severity, findings: serde_json::Value) -> Envelope<LintResult> {
        Envelope {
            wright: wright_driver::result::VersionInfo {
                version: "test".into(),
                contract: "wright-result/v1".into(),
            },
            command: "lint".into(),
            ok: true,
            exit: 0,
            diagnostics: vec![wright_driver::Diagnostic {
                code: "test".into(),
                stage: wright_driver::Stage::Analysis,
                severity: sev,
                message: "test".into(),
                status: None,
                span: None,
                source: None,
            }],
            result: LintResult {
                findings,
                ..LintResult::default()
            },
        }
    }

    #[test]
    fn summary_status_progression() {
        for (sev, findings, expected) in [
            (
                Severity::Info,
                serde_json::json!([{"severity": "info"}, {"severity": "notice"}]),
                "PASS",
            ),
            (
                Severity::Warning,
                serde_json::json!([{"severity": "info"}]),
                "WARN",
            ),
            (
                Severity::Error,
                serde_json::json!([{"severity": "warning"}, {"severity": "error"}]),
                "ERROR",
            ),
        ] {
            assert_eq!(summary_status(&summary_envelope(sev, findings)), expected);
        }
    }
}
