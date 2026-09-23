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
        let t_done = Arc::clone(&done);
        let t_status = Arc::clone(&status);
        let t_output = Arc::clone(&output);
        #[cfg(test)]
        let t_frame = Arc::clone(&frame);
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(60));
            let mut f = 0;
            while !t_done.load(Ordering::Acquire) {
                let event = *t_status.lock().expect("lock");
                write_activity_line(&t_output, event, Some(SPINNER[f]));
                f = (f + 1) % SPINNER.len();
                #[cfg(test)]
                t_frame.store(f, Ordering::Release);
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
        *self.status.lock().expect("lock") = Some(event);
        write_activity_line(&self.output, Some(event), None);
    }
}

fn write_activity_line(output: &Mutex<()>, event: Option<ProgressEvent>, spinner: Option<char>) {
    let _guard = output.lock().expect("lock");
    let label = event
        .map(progress_label)
        .unwrap_or_else(|| "Starting workflow".to_string());
    match spinner {
        Some(s) => eprint!("\r\x1b[2K\r  {s} {label}…"),
        None => eprint!("\r\x1b[2K\r  {label}…"),
    }
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
        (Some(count), Some(ProgressUnit::Files)) => format!("{label} {count} files"),
        (Some(count), Some(ProgressUnit::Rules)) => format!("{label} {count} rules"),
        _ => label.to_string(),
    }
}

impl Drop for Activity {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
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
                renderer == Renderer::Terminal && !env.no_color && !env.term_dumb
                    || env.force_color && renderer == Renderer::Terminal && !env.no_color
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
        if self.format == OutputFormat::Text && self.interactive {
            Activity::start()
        } else {
            Activity::disabled()
        }
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

pub(crate) fn render<T: serde::Serialize + ResultPresentation>(
    envelope: &Envelope<T>,
    pres: Presentation,
) {
    if pres.format == OutputFormat::Json {
        let mut value = serde_json::to_value(envelope).expect("serializes");
        if envelope.command == "check" {
            value["schema_version"] = serde_json::Value::String("1".to_string());
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&value).expect("serializes")
        );
        return;
    }
    match pres.renderer {
        Renderer::GithubActions => render_github(envelope),
        Renderer::Terminal | Renderer::Plain => render_text(envelope, pres.color),
    }
}

fn render_text<T: serde::Serialize + ResultPresentation>(envelope: &Envelope<T>, color: bool) {
    if !matches!(envelope.command.as_str(), "compile" | "convert") {
        render_verdict(envelope, color);
    }
    for diag in &envelope.diagnostics {
        render_diagnostic(diag, color);
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
    for diag in &envelope.diagnostics {
        emit_diagnostic_annotation(diag);
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

fn emit_diagnostic_annotation(diagnostic: &wright_driver::Diagnostic) {
    let kind = match diagnostic.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "notice",
    };
    let mut props = vec![format!(
        "title={}",
        escape_workflow_property(&diagnostic.code)
    )];
    if let Some(span) = diagnostic
        .span
        .as_ref()
        .filter(|s| is_real_source_path(&s.path))
    {
        props.insert(0, format!("file={}", escape_workflow_property(&span.path)));
        props.push(format!("line={}", span.start.line));
        props.push(format!("col={}", span.start.col));
        props.push(format!("endLine={}", span.end.line));
        props.push(format!("endColumn={}", span.end.col));
    }
    eprintln!(
        "::{kind} {}::{}",
        props.join(","),
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
    let kind = match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "notice",
    };
    let line = span_position(span, "start", "line").unwrap_or(1);
    let col = span_position(span, "start", "col").unwrap_or(1);
    let end_line = span_position(span, "end", "line").unwrap_or(line);
    let end_col = span_position(span, "end", "col").unwrap_or(col);
    let code = finding
        .get("code")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("finding");
    let msg = finding
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    eprintln!(
        "::{kind} file={},line={line},col={col},endLine={end_line},endColumn={end_col},title={}::{}",
        escape_workflow_property(path),
        escape_workflow_property(code),
        escape_workflow_data(msg)
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
        render_compile(self);
    }
}

impl ResultPresentation for ConvertResult {
    fn render_body(&self) {
        render_convert(self);
    }
}

impl ResultPresentation for CheckResult {
    fn render_body(&self) {}
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
            array_len(&self.rules)
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
                if let Some(sev) = finding.get("severity").and_then(serde_json::Value::as_str) {
                    *status = (*status).max(SummaryStatus::from_finding_severity(sev));
                }
            }
        }
    }
}

impl ResultPresentation for InspectResult {
    fn metadata(&self) -> Option<String> {
        Some(format!(
            "{} rule(s), {} symbol(s)",
            count(&self.program, "rules"),
            array_len(&self.symbols)
        ))
    }
    fn render_body(&self) {
        render_inspect(self);
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
    for diag in &envelope.diagnostics {
        status = status.max(match diag.severity {
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

fn span_position(span: &serde_json::Value, edge: &str, coord: &str) -> Option<u64> {
    span.get(edge)
        .and_then(serde_json::Value::as_object)
        .and_then(|p| p.get(coord))
        .and_then(serde_json::Value::as_u64)
}

fn render_compile(result: &CompileResult) {
    let Some(output) = result.output.as_ref() else {
        return;
    };
    if output.written_to != "stdout" {
        return;
    }
    print!("{}", output.text);
}

fn render_convert(result: &ConvertResult) {
    print!("{}", result.text);
}

fn array_len(value: &serde_json::Value) -> usize {
    value.as_array().map_or(0, Vec::len)
}

fn count(value: &serde_json::Value, key: &str) -> usize {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize
}

fn render_analyze(result: &AnalyzeResult) {
    let p = &result.program;
    let facts = &result.facts;
    let symbols = facts
        .get("symbols")
        .and_then(serde_json::Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let rules = facts
        .get("rules")
        .and_then(serde_json::Value::as_array)
        .map_or(&[][..], Vec::as_slice);

    println!("\nProgram overview");
    println!(
        "  {} file(s), {} rule(s), {} global variable(s), {} player variable(s), {} subroutine(s)",
        count(p, "files"),
        count(p, "rules"),
        count(p, "globalVariables"),
        count(p, "playerVariables"),
        count(p, "subroutines"),
    );
    println!("  evidence: [static] parsed program inventory");

    let (mut b_tot, mut e_tot, mut l_tot, mut w_tot) = (0, 0, 0, 0);
    let mut hotspots = Vec::new();
    for r in rules {
        let f = &r["controlFlow"];
        let (b, e, l, w) = (
            count(f, "blocks"),
            count(f, "edges"),
            count(f, "loopBlocks"),
            count(f, "waitBlocks"),
        );
        b_tot += b;
        e_tot += e;
        l_tot += l;
        w_tot += w;
        hotspots.push((b + e, r["name"].as_str().unwrap_or("<unnamed>"), b, e, l, w));
    }
    hotspots.sort_by(|l, r| r.0.cmp(&l.0).then_with(|| l.1.cmp(r.1)));
    println!("\nControl-flow summary");
    println!("  {b_tot} blocks, {e_tot} edges, {l_tot} loop block(s), {w_tot} wait block(s)");
    println!("  Top rules (heuristic ranking: blocks + edges; facts are [static])");
    if hotspots.is_empty() {
        println!("    none");
    } else {
        for (_, name, b, e, l, w) in hotspots.iter().take(5) {
            println!("    {name}: {b} blocks, {e} edges, {l} loop block(s), {w} wait block(s)");
        }
    }

    let mut vars = symbols
        .iter()
        .filter(|s| {
            matches!(
                s["kind"].as_str(),
                Some("globalVariable" | "playerVariable")
            )
        })
        .map(|s| {
            let u = &s["usage"];
            let (rules, reads, writes) = (count(u, "rules"), count(u, "reads"), count(u, "writes"));
            (
                rules,
                reads + writes,
                s["kind"].as_str().unwrap_or("variable"),
                s["name"].as_str().unwrap_or("<unnamed>"),
                reads,
                writes,
            )
        })
        .collect::<Vec<_>>();
    vars.sort_by(|l, r| {
        r.0.cmp(&l.0)
            .then_with(|| r.1.cmp(&l.1))
            .then_with(|| l.3.cmp(r.3))
    });
    println!("\nState and coupling");
    println!("  Top variables (heuristic ranking: rules touched, then reads + writes)");
    if vars.is_empty() {
        println!("    none");
    } else {
        for (rules, _, kind, name, reads, writes) in vars.iter().take(5) {
            println!(
                "    {kind} {name}: {rules} rule(s), {reads} read(s), {writes} write(s) [static]"
            );
        }
    }
}

fn render_lint(result: &LintResult) {
    let findings = result.findings.as_array().cloned().unwrap_or_default();
    println!("\nLint findings");
    if findings.is_empty() {
        println!("  none");
    }
    for f in &findings {
        let code = f["code"].as_str().unwrap_or("finding");
        let sev = f["severity"].as_str().unwrap_or("info");
        let ev = f["evidence"].as_str().unwrap_or("exact");
        let msg = f["message"].as_str().unwrap_or_default();
        match f.get("boundedness").and_then(serde_json::Value::as_str) {
            Some(v) => println!("  {sev}[{code}] (evidence: {ev}) (boundedness: {v}): {msg}"),
            None => println!("  {sev}[{code}] (evidence: {ev}): {msg}"),
        }
        if let Some(span) = f.get("span") {
            print_span(span, "      ");
        }
    }
}

fn render_inspect(result: &InspectResult) {
    let rules = result.rules.as_array().cloned().unwrap_or_default();
    let symbols = result.symbols.as_array().cloned().unwrap_or_default();
    println!(
        "\nProgram structure\n  {} rule(s), {} symbol(s)",
        count(&result.program, "rules"),
        symbols.len()
    );
    for r in &rules {
        println!(
            "  rule {}: \"{}\"",
            r.get("id").and_then(serde_json::Value::as_u64).unwrap_or(0),
            r["name"].as_str().unwrap_or("<unnamed>")
        );
    }
    for s in &symbols {
        println!(
            "  {} {}: {}",
            s["kind"].as_str().unwrap_or("symbol"),
            s.get("id").and_then(serde_json::Value::as_u64).unwrap_or(0),
            s["name"].as_str().unwrap_or("<unnamed>")
        );
    }
}

fn render_diagnostic(diagnostic: &wright_driver::Diagnostic, color: bool) {
    let sev = match diagnostic.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    };
    let label = if color {
        let code = match diagnostic.severity {
            Severity::Error => "31",
            Severity::Warning => "33",
            Severity::Info => "36",
        };
        format!("\x1b[{code}m{sev}\x1b[0m")
    } else {
        sev.to_string()
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
    let num_w = line.to_string().len();
    println!("{indent}| {:>num_w$} | {text}", line);
    let prefix = text
        .chars()
        .take(col.saturating_sub(1) as usize)
        .map(|c| if c == '\t' { '\t' } else { ' ' })
        .collect::<String>();
    println!("{indent}| {:>num_w$} | {prefix}^", "");
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
    ) -> Envelope<LintResult> {
        Envelope {
            wright: wright_driver::result::VersionInfo {
                version: "test".to_string(),
                contract: "wright-result/v1".to_string(),
            },
            command: "lint".to_string(),
            ok,
            exit: if ok { 0 } else { 1 },
            diagnostics,
            result: LintResult {
                findings,
                ..LintResult::default()
            },
        }
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
        let envelope = summary_envelope(
            true,
            vec![diagnostic(Severity::Info)],
            serde_json::json!([
                {"severity": "info"},
                {"severity": "notice"}
            ]),
        );
        assert_eq!(summary_status(&envelope), "PASS");
    }

    #[test]
    fn summary_warning_over_info_is_warn() {
        let envelope = summary_envelope(
            true,
            vec![diagnostic(Severity::Warning)],
            serde_json::json!([{"severity": "info"}]),
        );
        assert_eq!(summary_status(&envelope), "WARN");
    }

    #[test]
    fn summary_error_over_warning_is_error() {
        let envelope = summary_envelope(
            true,
            vec![diagnostic(Severity::Error)],
            serde_json::json!([
                {"severity": "warning"},
                {"severity": "error"},
                {"severity": "info"}
            ]),
        );
        assert_eq!(summary_status(&envelope), "ERROR");
    }
}
