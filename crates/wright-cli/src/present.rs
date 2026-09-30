use std::collections::{BTreeMap, BTreeSet};
use std::io::{IsTerminal, Write};
use std::path::Path;
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
    AnalyzeResult, CallGraphResult, CfgResult, CheckResult, CompileResult, ConvertResult,
    CostResult, Envelope, InspectResult, LintResult, RefsResult, SymbolsResult,
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
    elapsed: Duration,
    source_base: Option<&Path>,
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
        Renderer::Terminal | Renderer::Plain => render_text(envelope, pres, elapsed, source_base),
    }
}

/// Inputs a result body or footer may need beyond the envelope: display
/// policy plus the base that root-relative `span.path` spellings resolve
/// against. The base is absent when the input never loaded.
pub(crate) struct RenderContext<'a> {
    pub(crate) presentation: Presentation,
    pub(crate) elapsed: Duration,
    pub(crate) source_base: Option<&'a Path>,
}

/// The human-first result hierarchy (#443): the verdict leads, blocking
/// errors come before non-blocking diagnostics, secondary metadata trails
/// each diagnostic, and execution metadata (affected files, elapsed) closes
/// the report.
fn render_text<T: serde::Serialize + ResultPresentation>(
    envelope: &Envelope<T>,
    pres: Presentation,
    elapsed: Duration,
    source_base: Option<&Path>,
) {
    let ctx = RenderContext {
        presentation: pres,
        elapsed,
        source_base,
    };
    if !matches!(envelope.command.as_str(), "compile" | "convert") {
        render_verdict(envelope, pres.color);
    }
    let mut ordered: Vec<&wright_driver::Diagnostic> = envelope.diagnostics.iter().collect();
    ordered.sort_by_key(|diagnostic| severity_rank(diagnostic.severity));
    for diagnostic in ordered {
        render_diagnostic(diagnostic, pres.color);
    }
    if let Some(selection) = &envelope.selection {
        if selection.withheld > 0 {
            eprintln!(
                "  ... {} diagnostic(s) withheld (--max)",
                selection.withheld
            );
        }
    }
    if !envelope.ok {
        if envelope.diagnostics.is_empty() {
            eprintln!("{}: failed", envelope.command);
        }
    } else {
        envelope.result.render_body(&ctx);
    }
    if envelope.command == "check" {
        render_check_footer(envelope, pres, elapsed);
    }
}

/// Diagnostics render in action order — errors first — without changing the
/// reported set or the driver's production order (#443).
fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    }
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
        "check" => check_metadata(envelope),
        _ => envelope.result.metadata().unwrap_or_default(),
    };
    println!("  {}", dim(&metadata, color));
}

/// The check verdict names the blocking error count first, then the
/// non-blocking severities (#443). Under a finding selection the envelope
/// keeps only the true total, so the count line falls back to it.
fn check_metadata<T: serde::Serialize + ResultPresentation>(envelope: &Envelope<T>) -> String {
    if let Some(selection) = &envelope.selection {
        return format!("{} diagnostic(s)", selection.total);
    }
    let mut counts = [0usize; 3];
    for diagnostic in &envelope.diagnostics {
        counts[severity_rank(diagnostic.severity) as usize] += 1;
    }
    let mut parts = Vec::new();
    if counts[0] > 0 {
        parts.push(format!("{} error(s)", counts[0]));
    }
    if counts[1] > 0 {
        parts.push(format!("{} warning(s)", counts[1]));
    }
    if counts[2] > 0 {
        parts.push(format!("{} info diagnostic(s)", counts[2]));
    }
    if parts.is_empty() {
        "0 diagnostic(s)".to_string()
    } else {
        parts.join(", ")
    }
}

/// The closing check summary carries only execution metadata: files the
/// reported diagnostics point at, plus elapsed time on the interactive
/// terminal (kept off plain output so it stays deterministic) (#443).
fn render_check_footer<T: serde::Serialize + ResultPresentation>(
    envelope: &Envelope<T>,
    pres: Presentation,
    elapsed: Duration,
) {
    let files: BTreeSet<String> = envelope
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.span.as_ref())
        .filter(|span| is_real_source_path(&span.path))
        .map(|span| display_location_path(&span.path))
        .collect();
    let mut parts = Vec::new();
    if !files.is_empty() {
        parts.push(format!("{} file(s) affected", files.len()));
    }
    if pres.interactive {
        parts.push(format!("{} ms", elapsed.as_millis()));
    }
    if !parts.is_empty() {
        println!("  {}", dim(&parts.join(" · "), pres.color));
    }
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
        envelope.result.render_body(&github_context());
    } else {
        eprintln!("{} {}", summary_status(envelope), envelope.command);
    }
    eprintln!("::endgroup::");
    emit_summary(envelope);
}

/// The GitHub Actions render path never reaches `render_body` for commands
/// that consume the context, so a bare placeholder is enough for the
/// compile/convert artifact bodies it does emit.
fn github_context() -> RenderContext<'static> {
    RenderContext {
        presentation: Presentation {
            format: OutputFormat::Text,
            renderer: Renderer::GithubActions,
            color: false,
            interactive: false,
        },
        elapsed: Duration::ZERO,
        source_base: None,
    }
}

fn emit_diagnostic_annotation(diagnostic: &wright_driver::Diagnostic) {
    let kind = workflow_severity(diagnostic.severity);
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
    emit_workflow_annotation(kind, &props, &diagnostic.message);
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
    let kind = workflow_severity(severity);
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
    emit_workflow_annotation(
        kind,
        &[
            format!("file={}", escape_workflow_property(path)),
            format!("line={line}"),
            format!("col={col}"),
            format!("endLine={end_line}"),
            format!("endColumn={end_col}"),
            format!("title={}", escape_workflow_property(code)),
        ],
        msg,
    );
}

fn emit_workflow_annotation(kind: &str, properties: &[String], message: &str) {
    eprintln!(
        "::{kind} {}::{}",
        properties.join(","),
        escape_workflow_data(message)
    );
}

fn workflow_severity(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "notice",
    }
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
    fn render_body(&self, ctx: &RenderContext<'_>);
    fn render_github_findings(&self) {}
    fn update_summary_status(&self, _status: &mut SummaryStatus) {}
}

impl ResultPresentation for CompileResult {
    fn render_body(&self, _ctx: &RenderContext<'_>) {
        render_compile(self);
    }
}

impl ResultPresentation for ConvertResult {
    fn render_body(&self, _ctx: &RenderContext<'_>) {
        render_convert(self);
    }
}

impl ResultPresentation for CheckResult {
    fn render_body(&self, _ctx: &RenderContext<'_>) {}
}

impl ResultPresentation for AnalyzeResult {
    fn metadata(&self) -> Option<String> {
        let rules = count(&self.program, "rules");
        let symbols = self.facts.get("symbols").map_or(0, array_len);
        Some(match self.facts["cost"]["elementCount"].as_u64() {
            Some(elements) => format!(
                "{rules} rule(s), {elements} element(s), {symbols} symbol(s); ranked semantic report"
            ),
            None => format!("{rules} rule(s), {symbols} symbol(s); ranked semantic report"),
        })
    }
    fn render_body(&self, _ctx: &RenderContext<'_>) {
        render_analyze(self);
    }
}

impl ResultPresentation for LintResult {
    /// The lint verdict leads with finding counts by severity — the counts a
    /// human needs to judge priority. Under a selection the per-severity
    /// breakdown of the full set is unknown, so the verdict reports the true
    /// total instead of counting the selected remainder.
    fn metadata(&self) -> Option<String> {
        let rules = array_len(&self.rules);
        if let Some(selection) = &self.selection {
            return Some(format!(
                "{} finding(s) across {} rule(s)",
                selection.total, rules
            ));
        }
        let mut counts = [0usize; 3];
        for finding in self.findings.as_array().map_or(&[][..], Vec::as_slice) {
            counts[finding_severity_rank(finding) as usize] += 1;
        }
        let mut parts = Vec::new();
        if counts[0] > 0 {
            parts.push(format!("{} error(s)", counts[0]));
        }
        if counts[1] > 0 {
            parts.push(format!("{} warning(s)", counts[1]));
        }
        if counts[2] > 0 {
            parts.push(format!("{} info finding(s)", counts[2]));
        }
        let mut metadata = if parts.is_empty() {
            "0 finding(s)".to_string()
        } else {
            parts.join(", ")
        };
        metadata.push_str(&format!(" across {rules} rule(s)"));
        Some(metadata)
    }
    fn render_body(&self, ctx: &RenderContext<'_>) {
        render_lint(self, ctx);
    }
    fn render_github_findings(&self) {
        if let Some(findings) = self.findings.as_array() {
            for finding in findings {
                emit_finding_annotation(finding);
            }
        }
    }
    fn update_summary_status(&self, status: &mut SummaryStatus) {
        match &self.selection {
            // Selection narrows the reported list only; the verdict keeps the
            // full set's highest severity (#430).
            Some(selection) => {
                if let Some(severity) = selection.max_severity {
                    *status =
                        (*status).max(SummaryStatus::from_finding_severity(severity.as_str()));
                }
            }
            None => {
                if let Some(findings) = self.findings.as_array() {
                    for finding in findings {
                        if let Some(sev) =
                            finding.get("severity").and_then(serde_json::Value::as_str)
                        {
                            *status = (*status).max(SummaryStatus::from_finding_severity(sev));
                        }
                    }
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
    fn render_body(&self, _ctx: &RenderContext<'_>) {
        render_inspect(self);
    }
}

impl ResultPresentation for SymbolsResult {
    fn metadata(&self) -> Option<String> {
        Some(format!("{} symbol(s)", array_len(&self.0)))
    }
    fn render_body(&self, _ctx: &RenderContext<'_>) {
        let symbols = self.0.as_array().map_or(&[][..], Vec::as_slice);
        println!("\nSymbols");
        if symbols.is_empty() {
            println!("  none");
        }
        for symbol in symbols {
            println!(
                "  {} {}",
                symbol["kind"].as_str().unwrap_or("symbol"),
                symbol["name"].as_str().unwrap_or("<unnamed>")
            );
            if let Some(span) = symbol.get("span").filter(|span| span.is_object()) {
                print_location(span, "    ");
            }
        }
    }
}

impl ResultPresentation for RefsResult {
    fn metadata(&self) -> Option<String> {
        Some(format!(
            "{} ({}): {} read(s), {} write(s), {} call(s) across {} rule(s)",
            self.0["symbol"].as_str().unwrap_or("<unknown>"),
            self.0["kind"].as_str().unwrap_or("symbol"),
            count(&self.0, "reads"),
            count(&self.0, "writes"),
            count(&self.0, "calls"),
            count(&self.0, "rules"),
        ))
    }
    fn render_body(&self, _ctx: &RenderContext<'_>) {
        let references = self.0["references"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        println!(
            "\nReferences to {}",
            self.0["symbol"].as_str().unwrap_or("<unknown>")
        );
        if references.is_empty() {
            println!("  none");
        }
        for reference in references {
            let context = match (reference["rule"].as_u64(), reference["action"].as_u64()) {
                (Some(rule), Some(action)) => format!(" (rule {rule}, action {action})"),
                (Some(rule), None) => format!(" (rule {rule})"),
                _ => String::new(),
            };
            println!(
                "  {}{}",
                reference["kind"].as_str().unwrap_or("reference"),
                context
            );
            if let Some(span) = reference.get("span").filter(|span| span.is_object()) {
                print_location(span, "    ");
            }
        }
    }
}

impl ResultPresentation for CfgResult {
    fn metadata(&self) -> Option<String> {
        Some(format!("{} block(s)", array_len(&self.0["blocks"])))
    }
    fn render_body(&self, _ctx: &RenderContext<'_>) {
        let blocks = self.0["blocks"].as_array().map_or(&[][..], Vec::as_slice);
        println!("\nControl-flow graph");
        if blocks.is_empty() {
            println!("  none");
        }
        for block in blocks {
            let mut flags = Vec::new();
            if block["waits"].as_bool().unwrap_or(false) {
                flags.push("waits");
            }
            if block["calls"].as_bool().unwrap_or(false) {
                flags.push("calls");
            }
            let flags = if flags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", flags.join(" "))
            };
            println!(
                "  block {} ({}): {} action(s){}",
                block["id"].as_u64().unwrap_or(0),
                block["kind"].as_str().unwrap_or("block"),
                array_len(&block["actions"]),
                flags
            );
            if let Some(successors) = block["successors"].as_array()
                && !successors.is_empty()
            {
                let successors = successors
                    .iter()
                    .map(|successor| {
                        format!(
                            "{} ({})",
                            successor["to"].as_u64().unwrap_or(0),
                            successor["kind"].as_str().unwrap_or("edge")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                println!("    -> {successors}");
            }
        }
    }
}

impl ResultPresentation for CallGraphResult {
    fn metadata(&self) -> Option<String> {
        Some(format!("{} edge(s)", array_len(&self.0)))
    }
    fn render_body(&self, _ctx: &RenderContext<'_>) {
        let edges = self.0.as_array().map_or(&[][..], Vec::as_slice);
        println!("\nCall graph");
        if edges.is_empty() {
            println!("  none");
        }
        for edge in edges {
            println!(
                "  {} -> {}",
                edge["caller"].as_str().unwrap_or("<unnamed>"),
                edge["callee"].as_str().unwrap_or("<unnamed>")
            );
        }
    }
}

impl ResultPresentation for CostResult {
    fn metadata(&self) -> Option<String> {
        let exact = &self.0["exact"];
        Some(format!(
            "{} emitted byte(s), {} action(s), {} rule(s), {} wait(s)",
            count(exact, "emittedBytes"),
            count(exact, "programActions"),
            count(exact, "programRules"),
            count(exact, "waitActions"),
        ))
    }
    fn render_body(&self, _ctx: &RenderContext<'_>) {
        let exact = &self.0["exact"];
        println!("\nGenerated resources (exact)");
        println!("  emitted bytes: {}", count(exact, "emittedBytes"));
        println!("  actions: {}", count(exact, "programActions"));
        println!("  rules: {}", count(exact, "programRules"));
        println!("  waits: {}", count(exact, "waitActions"));
        let findings = self.0["findings"].as_array().map_or(&[][..], Vec::as_slice);
        println!("\nStatic findings (evidence: static)");
        if findings.is_empty() {
            println!("  none");
        }
        for finding in findings {
            println!(
                "  {}[{}]: {}",
                finding["severity"].as_str().unwrap_or("info"),
                finding["code"].as_str().unwrap_or("finding"),
                finding["message"].as_str().unwrap_or_default()
            );
        }
        if let Some(withheld) = self.0["selection"]["withheld"]
            .as_u64()
            .filter(|withheld| *withheld > 0)
        {
            println!("  ... {withheld} finding(s) withheld (--max)");
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
    match &envelope.selection {
        // The verdict reflects the full diagnostic set, not the selected
        // remainder (#430).
        Some(selection) => {
            if let Some(severity) = selection.max_severity {
                status = status.max(match severity {
                    Severity::Error => SummaryStatus::Error,
                    Severity::Warning => SummaryStatus::Warn,
                    Severity::Info => SummaryStatus::Pass,
                });
            }
        }
        None => {
            for diag in &envelope.diagnostics {
                status = status.max(match diag.severity {
                    Severity::Error => SummaryStatus::Error,
                    Severity::Warning => SummaryStatus::Warn,
                    Severity::Info => SummaryStatus::Pass,
                });
            }
        }
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

/// The `analyze` human report (#445): a bounded, layered view that leads
/// with Workshop cost, then structural complexity, stability/performance
/// risk indicators, and cross-cutting state. Every line marks whether it is
/// an exact count, a static fact, or a heuristic, and locations use real
/// resolved paths — unmapped structures stay explicitly unmapped.
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
    let objects = facts
        .get("persistentObjects")
        .and_then(serde_json::Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let risks = facts
        .get("risks")
        .and_then(serde_json::Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let cost = &facts["cost"];
    let element_total = cost["elementCount"].as_u64();

    println!("\nProgram overview");
    println!(
        "  {} file(s), {} rule(s), {} global variable(s), {} player variable(s), {} subroutine(s), {} persistent object(s)",
        count(p, "files"),
        count(p, "rules"),
        count(p, "globalVariables"),
        count(p, "playerVariables"),
        count(p, "subroutines"),
        objects.len(),
    );
    println!("  evidence: [static] parsed program inventory");

    println!("\nWorkshop cost");
    match element_total {
        Some(total) => println!(
            "  {total} element(s) [exact] — canonical element count; structural size, not runtime cost"
        ),
        None => println!(
            "  element count unavailable: {}",
            cost["unavailableReason"]
                .as_str()
                .unwrap_or("the program contains constructs the counter does not model")
        ),
    }
    let counts = &cost["counts"];
    println!(
        "  {} action(s), {} condition(s), {} wait(s) [static]",
        count(counts, "actions"),
        count(counts, "conditions"),
        count(counts, "waits"),
    );

    // Hotspots rank rules by element cost when the canonical count exists,
    // else by control-flow size; each entry keeps its measurements visible.
    let risk_count = |rule: &serde_json::Value| {
        risks
            .iter()
            .filter(|finding| finding["rule"].as_u64() == rule["id"].as_u64())
            .count()
    };
    let mut hotspots: Vec<&serde_json::Value> = rules.iter().collect();
    hotspots.sort_by(|left, right| {
        hotspot_key(right)
            .cmp(&hotspot_key(left))
            .then_with(|| rule_name(left).cmp(rule_name(right)))
    });
    println!("\nHotspots");
    println!(
        "  {}",
        if element_total.is_some() {
            "ranked by Workshop element count [exact counts; the ranking itself is a heuristic]"
        } else {
            "ranked by control-flow size [heuristic: blocks + edges]"
        }
    );
    if hotspots.is_empty() {
        println!("    none");
    } else {
        for rule in hotspots.iter().take(5) {
            let flow = &rule["controlFlow"];
            let (b, e) = (count(flow, "blocks"), count(flow, "edges"));
            let location = location_suffix(rule.get("span"));
            match rule["elements"].as_u64() {
                Some(elements) => {
                    let share = element_total
                        .filter(|total| *total > 0)
                        .map(|total| format!(" ({:.1}%)", elements as f64 * 100.0 / total as f64))
                        .unwrap_or_default();
                    println!(
                        "    \"{}\": {} element(s){share}, {b} block(s), {e} edge(s), {} risk(s){location}",
                        rule_name(rule),
                        elements,
                        risk_count(rule),
                    );
                }
                None => println!(
                    "    \"{}\": {b} block(s), {e} edge(s), {} risk(s){location}",
                    rule_name(rule),
                    risk_count(rule),
                ),
            }
        }
    }

    println!("\nComplexity");
    let (mut b_tot, mut e_tot, mut l_tot, mut w_tot) = (0, 0, 0, 0);
    for rule in rules {
        let flow = &rule["controlFlow"];
        b_tot += count(flow, "blocks");
        e_tot += count(flow, "edges");
        l_tot += count(flow, "loopBlocks");
        w_tot += count(flow, "waitBlocks");
    }
    println!(
        "  {b_tot} block(s), {e_tot} edge(s), {l_tot} loop block(s), {w_tot} wait block(s) [static]"
    );
    let mut trees: Vec<(&str, u64, u64, Option<&serde_json::Value>)> = rules
        .iter()
        .flat_map(|rule| {
            let name = rule_name(rule);
            rule["conditions"]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |condition| {
                    (
                        name,
                        condition["index"].as_u64().unwrap_or(0),
                        condition["elements"].as_u64().unwrap_or(0),
                        condition.get("span"),
                    )
                })
        })
        .collect();
    trees.sort_by(|l, r| {
        r.2.cmp(&l.2)
            .then_with(|| l.0.cmp(r.0))
            .then_with(|| l.1.cmp(&r.1))
    });
    let trees: Vec<_> = trees
        .into_iter()
        .filter(|tree| tree.2 > 0)
        .take(3)
        .collect();
    if !trees.is_empty() {
        println!("  top condition tree(s) by element count [exact]");
        for (name, index, elements, span) in trees {
            println!(
                "    \"{name}\" condition {}: {elements} element(s){}",
                index + 1,
                location_suffix(span)
            );
        }
    }

    println!("\nPerformance and stability risks");
    println!("  heuristic/static indicators — not measured runtime cost or behavior");
    let mut any_risk = false;
    let mut ordered: Vec<&serde_json::Value> = risks.iter().collect();
    ordered.sort_by_key(|finding| match finding["severity"].as_str() {
        Some("error") => 0,
        Some("warning") => 1,
        _ => 2,
    });
    for finding in ordered.iter().take(5) {
        any_risk = true;
        let severity = finding["severity"].as_str().unwrap_or("info");
        let code = finding["code"].as_str().unwrap_or("finding");
        let evidence = finding["evidence"].as_str().unwrap_or("exact");
        let message = finding["message"].as_str().unwrap_or_default();
        match finding["boundedness"].as_str() {
            Some(boundedness) => println!(
                "  {severity}[{code}] (evidence: {evidence}) (boundedness: {boundedness}): {message}"
            ),
            None => println!("  {severity}[{code}] (evidence: {evidence}): {message}"),
        }
        if let Some(span) = finding.get("span").filter(|span| span.is_object()) {
            print_location(span, "      ");
        }
    }
    if ordered.len() > 5 {
        println!(
            "  ... {} more — `wright lint` reports every finding",
            ordered.len() - 5
        );
        any_risk = true;
    }
    if !objects.is_empty() {
        any_risk = true;
        println!("  {}", persistent_object_summary(objects));
    }
    if !any_risk {
        println!("  none");
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

fn hotspot_key(rule: &serde_json::Value) -> usize {
    rule["elements"].as_u64().map_or_else(
        || {
            let flow = &rule["controlFlow"];
            count(flow, "blocks") + count(flow, "edges")
        },
        |elements| elements as usize,
    )
}

fn rule_name(rule: &serde_json::Value) -> &str {
    match rule["name"].as_str() {
        Some("") | None => "<unnamed>",
        Some(name) => name,
    }
}

/// The inline location for a report entry: a real authored path when the
/// fact carries one, a verbatim `<…>` marker for provider artifacts, or an
/// explicit unmapped note when there is no location at all.
fn location_suffix(span: Option<&serde_json::Value>) -> String {
    let Some(span) = span.filter(|span| span.is_object()) else {
        return " (unmapped)".to_string();
    };
    let line = span_position(span, "start", "line").unwrap_or(0);
    let col = span_position(span, "start", "col").unwrap_or(0);
    match span.get("path").and_then(serde_json::Value::as_str) {
        Some(path) if is_real_source_path(path) => {
            format!(" at {}:{line}:{col}", display_location_path(path))
        }
        Some(path) if !path.is_empty() => format!(" at {path}:{line}:{col}"),
        _ => " (unmapped)".to_string(),
    }
}

/// A one-line persistent-object summary for the risks section: kind counts,
/// then the statically knowable fan-out/identity facts (#445). These are
/// structural facts, not defect claims (#262).
fn persistent_object_summary(objects: &[serde_json::Value]) -> String {
    let mut kinds = BTreeMap::<&str, usize>::new();
    for object in objects {
        *kinds
            .entry(object["kind"].as_str().unwrap_or("object"))
            .or_default() += 1;
    }
    let kinds = kinds
        .iter()
        .map(|(kind, count)| format!("{count} {kind}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut notes = vec![format!("{} site(s): {}", objects.len(), kinds)];
    let without_identity = objects
        .iter()
        .filter(|object| object["identityRetained"].as_bool() == Some(false))
        .count();
    if without_identity > 0 {
        notes.push(format!("{without_identity} without retained identity"));
    }
    let broadly_visible = objects
        .iter()
        .filter(|object| object["visibility"].as_str() == Some("all-players"))
        .count();
    if broadly_visible > 0 {
        notes.push(format!("{broadly_visible} visible to all players"));
    }
    format!("{} [static]", notes.join("; "))
}

/// Location lines a collapsed finding group may list before the rest fold
/// into a count line — a repeated rule must not dominate the screen.
const MAX_GROUP_LOCATIONS: usize = 10;

/// The lint footer's metadata parts: affected files, skipped rule
/// evaluations, and elapsed time — interactive terminals only, so plain
/// output stays deterministic.
fn lint_footer_parts(files: usize, skipped: usize, ctx: &RenderContext<'_>) -> Vec<String> {
    let mut parts = Vec::new();
    if files > 0 {
        parts.push(format!("{files} file(s) affected"));
    }
    if skipped > 0 {
        parts.push(format!("{skipped} rule evaluation(s) skipped"));
    }
    if ctx.presentation.interactive {
        parts.push(format!("{} ms", ctx.elapsed.as_millis()));
    }
    parts
}

/// The severity rank a finding sorts by in the human view: errors first,
/// unknown severities conservatively ordered as warnings, and informational
/// severities last. The driver's reported set and order are unchanged
/// — this is a presentation-layer reorder only.
fn finding_severity_rank(finding: &serde_json::Value) -> u8 {
    match finding.get("severity").and_then(serde_json::Value::as_str) {
        Some("error") => 0,
        Some("info") | Some("notice") => 2,
        _ => 1,
    }
}

fn render_lint(result: &LintResult, ctx: &RenderContext<'_>) {
    let mut findings = result.findings.as_array().cloned().unwrap_or_default();
    findings.sort_by_key(finding_severity_rank);
    println!("\nLint findings");
    if findings.is_empty() {
        println!("  none");
    }
    // Consecutive findings sharing a rule id and message collapse into one
    // entry that lists its locations (#430); severity sorting keeps groups
    // consecutive because one rule id has one effective severity.
    let mut index = 0;
    while index < findings.len() {
        let first = &findings[index];
        let mut end = index + 1;
        while end < findings.len()
            && findings[end]["code"] == first["code"]
            && findings[end]["message"] == first["message"]
        {
            end += 1;
        }
        render_finding_group(&findings[index..end], ctx);
        index = end;
    }
    if let Some(selection) = &result.selection {
        if selection.withheld > 0 {
            println!("  ... {} finding(s) withheld (--max)", selection.withheld);
        }
    }
    // The closing summary carries only execution metadata: files the
    // reported findings point at, rules that could not evaluate, and elapsed
    // time on interactive terminals.
    let files: BTreeSet<&str> = findings
        .iter()
        .filter_map(|finding| finding.get("span"))
        .filter_map(|span| span.get("path"))
        .filter_map(serde_json::Value::as_str)
        .filter(|path| is_real_source_path(path))
        .collect();
    let parts = lint_footer_parts(files.len(), array_len(&result.skipped), ctx);
    if !parts.is_empty() {
        println!("  {}", dim(&parts.join(" · "), ctx.presentation.color));
    }
}

/// One finding entry: severity, rule id, and message lead; locations and a
/// source frame follow; evidence metadata trails dimmed so it never
/// outweighs the message. A repeated group names its finding count — pseudo-path
/// and spanless members join the group but never render a `-->` line.
fn render_finding_group(group: &[serde_json::Value], ctx: &RenderContext<'_>) {
    let first = &group[0];
    let code = first["code"].as_str().unwrap_or("finding");
    let msg = first["message"].as_str().unwrap_or_default();
    let sev = finding_severity_label(first, ctx.presentation.color);
    if group.len() == 1 {
        println!("  {sev}[{code}]: {msg}");
        if let Some(span) = first.get("span").filter(|span| span.is_object()) {
            render_finding_location(span, ctx, "    ", true);
        }
    } else {
        println!("  {sev}[{code}] ({} findings): {msg}", group.len());
        let located = renderable_group_spans(group);
        for span in located.iter().take(MAX_GROUP_LOCATIONS) {
            render_finding_location(span, ctx, "    ", false);
        }
        if located.len() > MAX_GROUP_LOCATIONS {
            println!(
                "    ... {} more location(s)",
                located.len() - MAX_GROUP_LOCATIONS
            );
        }
    }
    // Secondary notes — the evidence class, boundedness evidence, and spans
    // that point at no readable source file — render dimmed under the
    // problem line rather than competing with it.
    let mut notes = vec![format!(
        "evidence: {}",
        first["evidence"].as_str().unwrap_or("exact")
    )];
    if let Some(boundedness) = first.get("boundedness").and_then(serde_json::Value::as_str) {
        notes.push(format!("boundedness: {boundedness}"));
    }
    if let Some(note) = unmapped_positions_note(&unmapped_positions(group)) {
        notes.push(note);
    }
    println!(
        "    {}",
        dim(&format!("= {}", notes.join(" · ")), ctx.presentation.color)
    );
}

/// Object spans in the group that render as `-->` file locations — the basis
/// for the listed locations and the "more locations" fold.
fn renderable_group_spans(group: &[serde_json::Value]) -> Vec<&serde_json::Value> {
    group
        .iter()
        .filter_map(|finding| finding.get("span"))
        .filter(|span| span.is_object())
        .filter(|span| {
            span.get("path")
                .and_then(serde_json::Value::as_str)
                .is_some_and(is_real_source_path)
        })
        .collect()
}

/// A location line plus, for a lone finding, a one-line source frame when
/// the finding's path resolves to a readable file under the session's bases.
/// Collapsed groups omit frames so a repeated rule stays bounded.
fn render_finding_location(
    span: &serde_json::Value,
    ctx: &RenderContext<'_>,
    indent: &str,
    frame: bool,
) {
    let Some((display, readable)) = finding_location(span, ctx.source_base) else {
        return;
    };
    let line = span_position(span, "start", "line").unwrap_or(0);
    let col = span_position(span, "start", "col").unwrap_or(0);
    println!("{indent}--> {display}:{line}:{col}");
    if !frame {
        return;
    }
    if let Some(context) =
        readable.and_then(|path| source_context(&path, line as u32, col as u32, indent))
    {
        println!("{context}");
    }
}

/// Resolve the user-facing spelling of a finding's `span.path` and the path
/// the source frame reads, or `None` for spans without a real path (their
/// pseudo-path renders as a secondary note instead). Reported paths are
/// root-relative to the input include root; the display prefers the first
/// spelling that resolves to a real file — root-relative first, then the
/// reported spelling itself — so subdirectory inputs keep an actionable
/// location.
fn finding_location(
    span: &serde_json::Value,
    source_base: Option<&Path>,
) -> Option<(String, Option<String>)> {
    let path = span.get("path").and_then(serde_json::Value::as_str)?;
    if !is_real_source_path(path) {
        return None;
    }
    let reported = Path::new(path);
    let candidates: Vec<String> = if reported.is_absolute() {
        vec![path.to_string()]
    } else {
        source_base
            .map(|base| base.join(reported).display().to_string())
            .into_iter()
            .chain([path.to_string()])
            .collect()
    };
    for candidate in &candidates {
        if Path::new(candidate).is_file() {
            return Some((display_location_path(candidate), Some(candidate.clone())));
        }
    }
    Some((display_location_path(path), None))
}

/// Every pseudo-path position in the group (`<stdin>`, `<provider-artifact>`,
/// `<file N>`) as `(path, line, col)`, so an unmapped input still reports
/// where a repeated rule matched instead of folding positions away.
fn unmapped_positions(group: &[serde_json::Value]) -> Vec<(String, u64, u64)> {
    group
        .iter()
        .filter_map(|finding| finding.get("span"))
        .filter(|span| span.is_object())
        .filter_map(|span| {
            let path = span.get("path")?.as_str()?;
            (!is_real_source_path(path)).then(|| {
                (
                    path.to_string(),
                    span_position(span, "start", "line").unwrap_or(0),
                    span_position(span, "start", "col").unwrap_or(0),
                )
            })
        })
        .collect()
}

/// Fold pseudo-path positions into one secondary note matching the
/// diagnostic treatment: positions are reported but never dressed up as file
/// locations. Positions sharing one pseudo-path collapse to `path:l:c, l:c`;
/// the list stays bounded at `MAX_GROUP_LOCATIONS` entries.
fn unmapped_positions_note(positions: &[(String, u64, u64)]) -> Option<String> {
    let first_path = &positions.first()?.0;
    let shared = positions.iter().all(|(path, ..)| path == first_path);
    let mut text = String::new();
    for (index, (path, line, col)) in positions.iter().take(MAX_GROUP_LOCATIONS).enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        if shared {
            text.push_str(&format!("{line}:{col}"));
        } else {
            text.push_str(&format!("{path}:{line}:{col}"));
        }
    }
    if positions.len() > MAX_GROUP_LOCATIONS {
        text.push_str(&format!(
            ", +{} more",
            positions.len() - MAX_GROUP_LOCATIONS
        ));
    }
    Some(if shared {
        format!("at {first_path}:{text} (no source file)")
    } else {
        format!("at {text} (no source file)")
    })
}

fn finding_severity_label(finding: &serde_json::Value, color: bool) -> String {
    let severity = finding
        .get("severity")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("warning");
    if color {
        let code = match severity {
            "error" => "31",
            "info" | "notice" => "36",
            _ => "33",
        };
        format!("\x1b[{code}m{severity}\x1b[0m")
    } else {
        severity.to_string()
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
    // The summary stays small; each area names the query subcommand that
    // serves its full detail (#429).
    println!("\nDetail commands");
    println!("  wright inspect symbols [--only KIND]   the full or filtered symbol list");
    println!("  wright inspect refs <NAME>             references and usage counts for one symbol");
    println!("  wright inspect cfg <RULE>              the control-flow graph of one rule");
    println!("  wright inspect callgraph               the subroutine call graph");
    println!("  wright inspect cost                    generated-resource counts and findings");
}

fn render_diagnostic(diagnostic: &wright_driver::Diagnostic, color: bool) {
    let sev = diagnostic.severity.as_str();
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
    eprintln!("{label}[{}]: {}", diagnostic.code, diagnostic.message);
    // Secondary notes — locations without a mapped source file, the owning
    // stage, and non-native input origins — render dimmed under the problem
    // line rather than competing with it (#443).
    let mut notes = Vec::new();
    if let Some(span) = &diagnostic.span {
        if is_real_source_path(&span.path) {
            eprintln!(
                "  --> {}:{}:{}",
                display_location_path(&span.path),
                span.start.line,
                span.start.col
            );
            // The source frame stays on the diagnostic stream so the context
            // is emitted directly with the diagnostic it explains (#443).
            if let Some(context) = source_context(&span.path, span.start.line, span.start.col, "  ")
            {
                eprintln!("{context}");
            }
        } else {
            notes.push(format!(
                "at {}:{}:{} (no source file)",
                span.path, span.start.line, span.start.col
            ));
        }
    }
    notes.push(diagnostic.stage.as_str().to_string());
    if let Some(source) = &diagnostic.source {
        if source.kind != "workshop" {
            notes.push(format!("{kind} source", kind = source.kind));
        }
    }
    eprintln!("  {}", dim(&format!("= {}", notes.join(" · ")), color));
}

/// The user-facing spelling of a reported path: cwd-relative when possible so
/// locations read the same regardless of which stage produced them (#443).
fn display_location_path(path: &str) -> String {
    wright_driver::input::display_path(Path::new(path))
}

fn print_location(span: &serde_json::Value, indent: &str) {
    let path = span
        .get("path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<span>");
    let line = span_position(span, "start", "line").unwrap_or(0);
    let col = span_position(span, "start", "col").unwrap_or(0);
    println!("{indent}--> {path}:{line}:{col}");
}

fn source_context(path: &str, line: u32, col: u32, indent: &str) -> Option<String> {
    let source = std::fs::read_to_string(path).ok()?;
    let text = source.lines().nth(line.saturating_sub(1) as usize)?;
    let num_w = line.to_string().len();
    let prefix = text
        .chars()
        .take(col.saturating_sub(1) as usize)
        .map(|c| if c == '\t' { '\t' } else { ' ' })
        .collect::<String>();
    Some(format!(
        "{indent}| {:>num_w$} | {text}\n{indent}| {:>num_w$} | {prefix}^",
        line, ""
    ))
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
            selection: None,
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

    fn check_envelope(
        diagnostics: Vec<wright_driver::Diagnostic>,
        selection: Option<wright_driver::SelectionOutcome>,
    ) -> Envelope<CheckResult> {
        Envelope {
            wright: wright_driver::result::VersionInfo {
                version: "test".to_string(),
                contract: "wright-result/v1".to_string(),
            },
            command: "check".to_string(),
            ok: true,
            exit: 0,
            diagnostics,
            selection,
            result: CheckResult::default(),
        }
    }

    #[test]
    fn check_metadata_counts_errors_first() {
        let envelope = check_envelope(
            vec![
                diagnostic(Severity::Warning),
                diagnostic(Severity::Error),
                diagnostic(Severity::Warning),
                diagnostic(Severity::Info),
            ],
            None,
        );
        assert_eq!(
            check_metadata(&envelope),
            "1 error(s), 2 warning(s), 1 info diagnostic(s)"
        );
        let clean = check_envelope(Vec::new(), None);
        assert_eq!(check_metadata(&clean), "0 diagnostic(s)");
    }

    #[test]
    fn check_metadata_under_selection_reports_the_true_total() {
        let envelope = check_envelope(
            vec![diagnostic(Severity::Error)],
            Some(wright_driver::SelectionOutcome {
                total: 5,
                withheld: 2,
                max_severity: Some(Severity::Error),
            }),
        );
        assert_eq!(check_metadata(&envelope), "5 diagnostic(s)");
    }

    fn lint_result(
        findings: serde_json::Value,
        selection: Option<wright_driver::SelectionOutcome>,
    ) -> LintResult {
        LintResult {
            rules: serde_json::json!([
                {"id": "a", "effectiveSeverity": "warning"},
                {"id": "b", "effectiveSeverity": "error"},
            ]),
            findings,
            selection,
            ..LintResult::default()
        }
    }

    #[test]
    fn lint_metadata_counts_findings_by_severity() {
        let result = lint_result(
            serde_json::json!([
                {"severity": "warning"},
                {"severity": "error"},
                {"severity": "warning"},
                {"severity": "info"},
            ]),
            None,
        );
        assert_eq!(
            result.metadata().unwrap(),
            "1 error(s), 2 warning(s), 1 info finding(s) across 2 rule(s)"
        );
        let clean = lint_result(serde_json::json!([]), None);
        assert_eq!(clean.metadata().unwrap(), "0 finding(s) across 2 rule(s)");
    }

    #[test]
    fn lint_metadata_under_selection_reports_the_true_total() {
        let result = lint_result(
            serde_json::json!([{"severity": "error"}]),
            Some(wright_driver::SelectionOutcome {
                total: 7,
                withheld: 6,
                max_severity: Some(Severity::Error),
            }),
        );
        assert_eq!(result.metadata().unwrap(), "7 finding(s) across 2 rule(s)");
    }

    #[test]
    fn finding_severity_rank_orders_errors_first_and_unknowns_as_warnings() {
        let rank = |severity: serde_json::Value| {
            finding_severity_rank(&serde_json::json!({"severity": severity}))
        };
        assert_eq!(rank(serde_json::json!("error")), 0);
        assert_eq!(rank(serde_json::json!("warning")), 1);
        assert_eq!(rank(serde_json::json!("unexpected")), 1);
        assert_eq!(rank(serde_json::json!("info")), 2);
        assert_eq!(rank(serde_json::json!("notice")), 2);
        assert_eq!(rank(serde_json::Value::Null), 1);
    }

    #[test]
    fn lint_footer_parts_carry_files_skipped_and_interactive_elapsed() {
        let ctx = |interactive| RenderContext {
            presentation: Presentation {
                format: OutputFormat::Text,
                renderer: Renderer::Terminal,
                color: false,
                interactive,
            },
            elapsed: Duration::from_millis(42),
            source_base: None,
        };
        assert_eq!(
            lint_footer_parts(2, 1, &ctx(true)),
            vec![
                "2 file(s) affected".to_string(),
                "1 rule evaluation(s) skipped".to_string(),
                "42 ms".to_string()
            ]
        );
        // Plain output omits wall-clock values so it stays deterministic.
        assert_eq!(
            lint_footer_parts(2, 0, &ctx(false)),
            vec!["2 file(s) affected".to_string()]
        );
        assert!(lint_footer_parts(0, 0, &ctx(false)).is_empty());
    }

    #[test]
    fn finding_location_resolves_reported_paths_against_the_source_base() {
        let root = std::env::temp_dir().join(format!("wright-finding-{}", std::process::id()));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let absolute = root.join("sub/f.ws");
        std::fs::write(&absolute, "x").unwrap();
        let span = serde_json::json!({"path": "f.ws", "start": {"line": 1, "col": 1}, "end": {"line": 1, "col": 2}});
        let (display, readable) = finding_location(&span, Some(&root.join("sub"))).unwrap();
        assert!(
            readable.is_some_and(|path| Path::new(&path).is_file()),
            "the root-relative spelling resolves under the source base"
        );
        assert!(display.ends_with("sub/f.ws"), "{display}");

        // Pseudo-paths have no file location; they render as notes instead.
        let pseudo = serde_json::json!({"path": "<stdin>", "start": {"line": 1, "col": 1}});
        assert!(finding_location(&pseudo, Some(&root)).is_none());
        let positions = unmapped_positions(&[serde_json::json!({"span": pseudo})]);
        assert_eq!(
            unmapped_positions_note(&positions).unwrap(),
            "at <stdin>:1:1 (no source file)"
        );

        // A missing file keeps the reported spelling with no frame.
        let missing = serde_json::json!({"path": "gone.ws", "start": {"line": 1, "col": 1}});
        let (display, readable) = finding_location(&missing, Some(&root)).unwrap();
        assert_eq!(display, "gone.ws");
        assert!(readable.is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn renderable_group_spans_counts_only_real_source_locations() {
        let real = |path: &str| serde_json::json!({"span": {"path": path, "start": {"line": 1, "col": 1}}});
        let group = vec![
            real("a.ws"),
            real("<stdin>"),
            serde_json::json!({"span": null}),
            real("b.ws"),
        ];
        assert_eq!(renderable_group_spans(&group).len(), 2);
    }

    #[test]
    fn unmapped_positions_note_collapses_a_shared_pseudo_path() {
        let finding = |line: u64| serde_json::json!({"span": {"path": "<stdin>", "start": {"line": line, "col": 1}}});
        let group = vec![finding(11), finding(14)];
        assert_eq!(
            unmapped_positions_note(&unmapped_positions(&group)).unwrap(),
            "at <stdin>:11:1, 14:1 (no source file)"
        );
    }

    #[test]
    fn unmapped_positions_note_bounds_the_position_list() {
        let finding = |line: u64| serde_json::json!({"span": {"path": "<stdin>", "start": {"line": line, "col": 1}}});
        let group: Vec<_> = (1..=12).map(finding).collect();
        let note = unmapped_positions_note(&unmapped_positions(&group)).unwrap();
        assert!(
            note.contains("+2 more"),
            "positions beyond the cap fold into a count: {note}"
        );
    }
}
