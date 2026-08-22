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
use wright_driver::result::Envelope;

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
    fn disabled() -> Self {
        Self {
            done: Arc::new(AtomicBool::new(true)),
            visible: Arc::new(AtomicBool::new(false)),
            status: Arc::new(Mutex::new(None)),
            output: Arc::new(Mutex::new(())),
            #[cfg(test)]
            frame: Arc::new(AtomicUsize::new(0)),
            handle: None,
        }
    }

    fn start() -> Self {
        let done = Arc::new(AtomicBool::new(false));
        let visible = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(None));
        let output = Arc::new(Mutex::new(()));
        #[cfg(test)]
        let frame = Arc::new(AtomicUsize::new(0));
        write_activity_line(&output, None, None);
        visible.store(true, Ordering::Release);
        let thread_done = Arc::clone(&done);
        let thread_status = Arc::clone(&status);
        let thread_output = Arc::clone(&output);
        #[cfg(test)]
        let thread_frame = Arc::clone(&frame);
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(60));
            let mut frame = 0;
            while !thread_done.load(Ordering::Acquire) {
                let event = *thread_status.lock().expect("activity status lock");
                write_activity_line(&thread_output, event, Some(SPINNER[frame]));
                frame = (frame + 1) % SPINNER.len();
                #[cfg(test)]
                thread_frame.store(frame, Ordering::Release);
                thread::sleep(Duration::from_millis(80));
            }
        });
        Self {
            done,
            visible,
            status,
            output,
            #[cfg(test)]
            frame,
            handle: Some(handle),
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
    match spinner {
        Some(spinner) => eprint!("\r\x1b[2K\r  {spinner} {label}…"),
        None => eprint!("\r\x1b[2K\r  {label}…"),
    }
    let _ = std::io::stderr().flush();
}

fn progress_label(event: ProgressEvent) -> String {
    let label = match event.phase {
        ProgressPhase::InputResolution => "Resolving input".to_string(),
        ProgressPhase::ProjectLoading => "Loading project".to_string(),
        ProgressPhase::Parsing => "Parsing".to_string(),
        ProgressPhase::Validation => "Validating".to_string(),
        ProgressPhase::Lowering => "Lowering".to_string(),
        ProgressPhase::SemanticAnalysis => "Resolving semantics".to_string(),
        ProgressPhase::Linting => "Running lint rules".to_string(),
        ProgressPhase::Emission => "Emitting Workshop".to_string(),
        ProgressPhase::Conversion => "Reconstructing source".to_string(),
    };
    match (event.count, event.unit) {
        (Some(count), Some(ProgressUnit::Files)) => format!("{label} {count} files"),
        (Some(count), Some(ProgressUnit::Rules)) => format!("{label} {count} rules"),
        _ => label,
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
    let _ = write!(writer, "\r\x1b[2K\r");
    let _ = writeln!(writer);
    let _ = writer.flush();
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
            interactive: renderer == Renderer::Terminal
                && environment.stdout_terminal
                && !environment.term_dumb,
        }
    }

    pub(crate) fn activity(&self) -> Activity {
        if self.format == OutputFormat::Text && self.interactive {
            Activity::start()
        } else {
            Activity::disabled()
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

fn render_text<T: serde::Serialize>(envelope: &Envelope<T>, color: bool) {
    let value = serde_json::to_value(envelope).expect("envelope serializes");
    if !matches!(envelope.command.as_str(), "compile" | "convert") {
        render_verdict(envelope, &value, color);
    }
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
        "check" => {}
        "analyze" => render_analyze(envelope),
        "lint" => render_lint(envelope),
        "inspect" => render_inspect(envelope),
        other => println!("{other}: ok"),
    }
}

fn render_verdict<T: serde::Serialize>(
    envelope: &Envelope<T>,
    value: &serde_json::Value,
    color: bool,
) {
    let status = summary_status(envelope, value);
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
        "lint" => format!(
            "{} finding(s) across {} rule(s)",
            array_len(value, "/result/findings"),
            array_len(value, "/result/rules"),
        ),
        "analyze" => format!(
            "{} rule(s), {} symbol(s); ranked semantic report",
            value
                .pointer("/result/program")
                .map_or(0, |program| count(program, "rules")),
            array_len(value, "/result/facts/symbols"),
        ),
        "inspect" => format!(
            "{} rule(s), {} symbol(s)",
            array_len(value, "/result/rules"),
            array_len(value, "/result/symbols"),
        ),
        _ => return,
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

fn array_len(value: &serde_json::Value, pointer: &str) -> usize {
    value
        .pointer(pointer)
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len)
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SummaryStatus {
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

fn summary_status<T: serde::Serialize>(
    envelope: &Envelope<T>,
    value: &serde_json::Value,
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
    if let Some(findings) = value
        .pointer("/result/findings")
        .and_then(serde_json::Value::as_array)
    {
        for finding in findings {
            if let Some(severity) = finding.get("severity").and_then(serde_json::Value::as_str) {
                status = status.max(SummaryStatus::from_finding_severity(severity));
            }
        }
    }
    status.as_str()
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
    let program = value
        .pointer("/result/program")
        .cloned()
        .unwrap_or_default();
    let facts = value.pointer("/result/facts").cloned().unwrap_or_default();
    let symbols = facts
        .get("symbols")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let rules = facts
        .get("rules")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    println!("\nProgram overview");
    println!(
        "  {} file(s), {} rule(s), {} global variable(s), {} player variable(s), {} subroutine(s)",
        count(&program, "files"),
        count(&program, "rules"),
        count(&program, "globalVariables"),
        count(&program, "playerVariables"),
        count(&program, "subroutines"),
    );
    println!("  evidence: [static] parsed program inventory");

    let mut total_blocks = 0;
    let mut total_edges = 0;
    let mut total_loops = 0;
    let mut total_waits = 0;
    let mut rule_hotspots = Vec::new();
    for rule in &rules {
        let flow = rule.get("controlFlow").cloned().unwrap_or_default();
        let blocks = count(&flow, "blocks");
        let edges = count(&flow, "edges");
        let loops = count(&flow, "loopBlocks");
        let waits = count(&flow, "waitBlocks");
        total_blocks += blocks;
        total_edges += edges;
        total_loops += loops;
        total_waits += waits;
        rule_hotspots.push((
            blocks + edges,
            rule.get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("<unnamed>"),
            blocks,
            edges,
            loops,
            waits,
        ));
    }
    rule_hotspots.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(right.1)));
    println!("\nControl-flow summary");
    println!(
        "  {total_blocks} blocks, {total_edges} edges, {total_loops} loop block(s), {total_waits} wait block(s)"
    );
    println!("  Top rules (heuristic ranking: blocks + edges; facts are [static])");
    if rule_hotspots.is_empty() {
        println!("    none");
    } else {
        for (_, name, blocks, edges, loops, waits) in rule_hotspots.iter().take(5) {
            println!(
                "    {name}: {blocks} blocks, {edges} edges, {loops} loop block(s), {waits} wait block(s)"
            );
        }
    }

    let mut coupled_symbols = symbols
        .iter()
        .filter(|symbol| {
            matches!(
                symbol.get("kind").and_then(serde_json::Value::as_str),
                Some("globalVariable" | "playerVariable")
            )
        })
        .map(|symbol| {
            let usage = symbol.get("usage").cloned().unwrap_or_default();
            let rules = count(&usage, "rules");
            let reads = count(&usage, "reads");
            let writes = count(&usage, "writes");
            (
                rules,
                reads + writes,
                symbol
                    .get("kind")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("variable"),
                symbol
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("<unnamed>"),
                reads,
                writes,
            )
        })
        .collect::<Vec<_>>();
    coupled_symbols.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| right.1.cmp(&left.1))
            .then_with(|| left.3.cmp(right.3))
    });
    println!("\nState and coupling");
    println!("  Top variables (heuristic ranking: rules touched, then reads + writes)");
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

fn count(value: &serde_json::Value, key: &str) -> usize {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize
}

fn render_lint<T: serde::Serialize>(envelope: &Envelope<T>) {
    let value = serde_json::to_value(envelope).expect("envelope serializes");
    let findings = value
        .pointer("/result/findings")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    println!("\nLint findings");
    if findings.is_empty() {
        println!("  none");
    }
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
    println!("\nProgram structure");
    println!("  {count} rule(s), {} symbol(s)", symbols.len());
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
    let line = span
        .pointer("/start/line")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let col = span
        .pointer("/start/col")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    println!("{indent}--> {path}:{line}:{col}");
    render_source_context(path, line as u32, col as u32, indent);
}

/// Add one source line only when the provenance path resolves in the current
/// process. Structured output never calls this renderer, and an unresolved
/// path remains a normal location-only presentation.
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
    fn activity_is_only_enabled_for_interactive_text() {
        let terminal = Presentation::resolve(
            OutputFormat::Text,
            RendererArg::Terminal,
            ColorArg::Never,
            environment(),
        );
        let plain = Presentation::resolve(
            OutputFormat::Text,
            RendererArg::Plain,
            ColorArg::Never,
            environment(),
        );
        let json = Presentation::resolve(
            OutputFormat::Json,
            RendererArg::Terminal,
            ColorArg::Always,
            environment(),
        );
        let mut dumb_environment = environment();
        dumb_environment.term_dumb = true;
        let dumb = Presentation::resolve(
            OutputFormat::Text,
            RendererArg::Terminal,
            ColorArg::Never,
            dumb_environment,
        );
        assert!(terminal.activity().handle.is_some());
        assert!(plain.activity().handle.is_none());
        assert!(json.activity().handle.is_none());
        assert!(dumb.activity().handle.is_none());
    }

    #[test]
    fn activity_is_visible_immediately_and_accepts_phase_updates() {
        let terminal = Presentation::resolve(
            OutputFormat::Text,
            RendererArg::Terminal,
            ColorArg::Never,
            environment(),
        );
        let activity = terminal.activity();
        assert!(activity.visible.load(Ordering::Acquire));
        assert_eq!(*activity.status.lock().unwrap(), None);
        activity.on_progress(ProgressEvent::with_count(
            ProgressPhase::Linting,
            12,
            ProgressUnit::Rules,
        ));
        assert_eq!(
            *activity.status.lock().unwrap(),
            Some(ProgressEvent::with_count(
                ProgressPhase::Linting,
                12,
                ProgressUnit::Rules,
            ))
        );
        thread::sleep(Duration::from_millis(150));
        assert!(activity.frame.load(Ordering::Acquire) > 0);
    }

    #[test]
    fn activity_cleanup_clears_the_line_and_terminates_it() {
        let mut output = Vec::new();
        clear_activity_line(&mut output);
        assert_eq!(output, b"\r\x1b[2K\r\n");
    }

    #[test]
    fn metadata_is_dimmed_only_for_terminal_color_output() {
        assert_eq!(dim("2 symbols", false), "2 symbols");
        assert_eq!(dim("2 symbols", true), "\x1b[2m2 symbols\x1b[0m");
    }

    #[test]
    fn workflow_command_escaping_is_split_by_context() {
        assert_eq!(escape_workflow_property("a,b:c%\n"), "a%2Cb%3Ac%25%0A");
        assert_eq!(escape_workflow_data("a,b:c%\n"), "a,b:c%25%0A");
    }

    fn summary_envelope(
        ok: bool,
        diagnostics: Vec<wright_driver::Diagnostic>,
        findings: serde_json::Value,
    ) -> (Envelope<serde_json::Value>, serde_json::Value) {
        let envelope = Envelope {
            wright: wright_driver::result::VersionInfo {
                version: "test".to_string(),
                contract: "wright-result/v1".to_string(),
            },
            command: "lint".to_string(),
            ok,
            exit: if ok { 0 } else { 1 },
            diagnostics,
            result: serde_json::json!({"findings": findings}),
        };
        let value = serde_json::to_value(&envelope).unwrap();
        (envelope, value)
    }

    fn diagnostic(severity: Severity) -> wright_driver::Diagnostic {
        wright_driver::Diagnostic {
            code: "test".to_string(),
            stage: wright_driver::Stage::Analysis,
            severity,
            message: "test".to_string(),
            status: None,
            span: None,
            source: None,
        }
    }

    #[test]
    fn summary_info_only_is_pass() {
        let (envelope, value) = summary_envelope(
            true,
            vec![diagnostic(Severity::Info)],
            serde_json::json!([
                {"severity": "info"},
                {"severity": "notice"}
            ]),
        );
        assert_eq!(summary_status(&envelope, &value), "PASS");
    }

    #[test]
    fn summary_warning_over_info_is_warn() {
        let (envelope, value) = summary_envelope(
            true,
            vec![diagnostic(Severity::Warning)],
            serde_json::json!([{"severity": "info"}]),
        );
        assert_eq!(summary_status(&envelope, &value), "WARN");
    }

    #[test]
    fn summary_error_over_warning_is_error() {
        let (envelope, value) = summary_envelope(
            true,
            vec![diagnostic(Severity::Error)],
            serde_json::json!([
                {"severity": "warning"},
                {"severity": "error"},
                {"severity": "info"}
            ]),
        );
        assert_eq!(summary_status(&envelope, &value), "ERROR");
    }
}
