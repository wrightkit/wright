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
    CallGraphResult, CfgResult, CheckResult, CompileResult, ConvertResult, CostResult, Envelope,
    EssentialResult, LintResult, RefsResult, RulesResult, SymbolsResult,
};

use crate::cli::{ColorArg, CommonArgs, OutputFormatArg, RendererArg};

pub(crate) mod addresses;
mod common;
mod findings;
pub(crate) mod terminal;

pub(crate) use addresses::{edge_caller, rule_address, rule_name, tree_rule};
pub(crate) use common::{
    DETAIL_COMMANDS, block_flags, call_fan_counts, cfg_block_stats, cfg_edge_count,
    diagnostic_counts, diagnostic_files, finding_files, finding_record, fix_status_counts,
    fix_status_parts, grouped_findings, lookup_hint_args, meta_severity_counts,
    normalize_finding_spans, notable_callers, severity_label, severity_rank, shown_diagnostics,
    shown_of, sorted_findings, span_suffix, uniform_evidence,
};
pub(crate) use findings::{
    finding_head, finding_location_line, finding_location_span, finding_section, lint_footer,
};
#[cfg(test)]
use findings::{
    finding_location, renderable_group_spans, unmapped_positions, unmapped_positions_note,
};

use crate::plain;

const MAX_WIDTH: u16 = 100;
const COMPACT_BELOW: u16 = 90;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Presentation {
    pub(crate) format: OutputFormat,
    pub(crate) renderer: Renderer,
    pub(crate) color: bool,
    pub(crate) interactive: bool,
    pub(crate) all: bool,
    pub(crate) width: u16,
}

pub(crate) struct Activity {
    done: Arc<AtomicBool>,
    visible: Arc<AtomicBool>,
    status: Arc<Mutex<Option<ProgressEvent>>>,
    output: Arc<Mutex<()>>,
    #[cfg(test)]
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
        write_activity_line(&output, &done, None, None);
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
                write_activity_line(&t_output, &t_done, event, Some(SPINNER[f]));
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
        if event.phase == ProgressPhase::Confirmation {
            self.done.store(true, Ordering::Release);
            let _guard = self.output.lock().expect("lock");
            if self.visible.swap(false, Ordering::AcqRel) {
                let _ = write!(std::io::stderr(), "\r\x1b[2K\r");
                let _ = std::io::stderr().flush();
            }
            return;
        }
        *self.status.lock().expect("lock") = Some(event);
        write_activity_line(&self.output, &self.done, Some(event), None);
    }
}

fn write_activity_line(
    output: &Mutex<()>,
    done: &AtomicBool,
    event: Option<ProgressEvent>,
    spinner: Option<char>,
) {
    let _guard = output.lock().expect("lock");
    if done.load(Ordering::Acquire) {
        return;
    }
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
        ProgressPhase::Confirmation => "Waiting for confirmation",
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
pub(crate) enum Renderer {
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
    columns: Option<u16>,
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
            columns: std::env::var("COLUMNS")
                .ok()
                .and_then(|v| v.trim().parse::<u16>().ok())
                .filter(|c| *c > 0),
        }
    }
}

#[cfg(unix)]
fn terminal_columns() -> Option<u16> {
    rustix::termios::tcgetwinsize(std::io::stdout())
        .ok()
        .map(|size| size.ws_col)
        .filter(|col| *col > 0)
}

#[cfg(not(unix))]
fn terminal_columns() -> Option<u16> {
    None
}

impl Presentation {
    pub(crate) fn with_all(mut self, all: bool) -> Self {
        self.all = all;
        self
    }
    pub(crate) fn from_common(common: &CommonArgs) -> Self {
        Self::from_output(common.format, common.renderer, common.color)
    }

    pub(crate) fn from_output(
        format: OutputFormatArg,
        renderer: RendererArg,
        color: ColorArg,
    ) -> Self {
        let format = match format {
            OutputFormatArg::Text => OutputFormat::Text,
            OutputFormatArg::Json => OutputFormat::Json,
        };
        Self::resolve(format, renderer, color, RuntimeEnvironment::process())
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
        let columns = env.columns.or_else(|| {
            if env.stdout_terminal {
                terminal_columns()
            } else {
                None
            }
        });
        Self {
            format,
            renderer,
            color,
            all: false,
            interactive: renderer == Renderer::Terminal && env.stdout_terminal && !env.term_dumb,
            width: columns.unwrap_or(MAX_WIDTH).clamp(1, MAX_WIDTH),
        }
    }

    pub(crate) fn is_terminal(&self) -> bool {
        self.renderer == Renderer::Terminal
    }

    fn compact(&self) -> bool {
        self.width < COMPACT_BELOW
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

pub(crate) fn render<
    T: serde::Serialize + ResultPresentation + plain::PlainReport + terminal::TerminalReport,
>(
    envelope: &Envelope<T>,
    pres: Presentation,
    elapsed: Duration,
    source_base: Option<&Path>,
    subject: Option<&str>,
) {
    if pres.format == OutputFormat::Json {
        let mut value = serde_json::to_value(envelope).expect("serializes");
        if !matches!(envelope.command.as_str(), "compile" | "convert") {
            wright_driver::essential::envelope(&mut value, &envelope.diagnostics, pres.all);
        }
        if envelope.command == "check" {
            value["schema_version"] = serde_json::Value::String("1".to_string());
        }
        println!("{}", serde_json::to_string(&value).expect("serializes"));
        return;
    }
    let ctx = RenderContext {
        presentation: pres,
        elapsed,
        source_base,
        subject,
    };
    match pres.renderer {
        Renderer::GithubActions => render_github(envelope, &ctx),
        Renderer::Plain => plain::render(envelope, &ctx),
        Renderer::Terminal => terminal::render(envelope, &ctx),
    }
}

pub(crate) struct RenderContext<'a> {
    pub(crate) presentation: Presentation,
    pub(crate) elapsed: Duration,
    pub(crate) source_base: Option<&'a Path>,
    pub(crate) subject: Option<&'a str>,
}

pub(crate) fn diagnostics_on_stdout<T: serde::Serialize>(envelope: &Envelope<T>) -> bool {
    !envelope.ok && !envelope.gated && envelope.exit != 2 && envelope.exit != 4
}

fn dim(value: &str, color: bool) -> String {
    if color {
        format!("\x1b[2m{value}\x1b[0m")
    } else {
        value.to_string()
    }
}

struct GithubAnnotation {
    kind: &'static str,
    props: Vec<String>,
    title: String,
    message: String,
    path: String,
    line: u64,
    col: u64,
}

impl GithubAnnotation {
    fn key(&self) -> (u8, String, String, u64, u64) {
        (
            (self.kind != "error") as u8,
            self.title.clone(),
            self.path.clone(),
            self.line,
            self.col,
        )
    }
}

const GITHUB_ANNOTATION_LIMIT: usize = 10;

const GITHUB_SUMMARY_BUDGET: usize = 900_000;

fn bound_annotations(annotations: &mut Vec<GithubAnnotation>) -> usize {
    annotations.sort_by_key(|annotation| (annotation.kind != "error") as u8);
    let mut seen = std::collections::BTreeSet::new();
    annotations.retain(|annotation| seen.insert(annotation.key()));
    let mut errors = 0usize;
    let mut warnings = 0usize;
    let mut withheld = 0usize;
    annotations.retain(|annotation| {
        let shown = match annotation.kind {
            "error" => {
                errors += 1;
                errors <= GITHUB_ANNOTATION_LIMIT
            }
            _ => {
                warnings += 1;
                warnings <= GITHUB_ANNOTATION_LIMIT
            }
        };
        if shown {
            true
        } else {
            withheld += 1;
            false
        }
    });
    withheld
}

fn render_github<T: serde::Serialize + ResultPresentation + plain::PlainReport>(
    envelope: &Envelope<T>,
    ctx: &RenderContext<'_>,
) {
    let mut annotations: Vec<GithubAnnotation> = envelope
        .diagnostics
        .iter()
        .filter_map(diagnostic_annotation)
        .collect();
    for (finding, severity) in envelope.result.github_findings() {
        if let Some(annotation) = finding_annotation(&finding, severity) {
            annotations.push(annotation);
        }
    }
    let withheld = bound_annotations(&mut annotations);
    for annotation in &annotations {
        emit_workflow_annotation(annotation.kind, &annotation.props, &annotation.message);
    }
    let status = summary_status(envelope);
    let headline = envelope
        .result
        .metadata()
        .map(|metadata| format!(" {metadata}"))
        .unwrap_or_default();
    eprintln!(
        "::group::{}",
        escape_workflow_data(&format!("wright {} — {status}{headline}", envelope.command))
    );
    if let Some(artifact) = envelope.result.artifact() {
        print!("{artifact}");
    } else {
        let (out, err) = plain::render_lines(envelope, ctx);
        for line in out.iter().chain(err.iter()) {
            eprintln!("{line}");
        }
    }
    eprintln!("::endgroup::");
    if withheld > 0 {
        eprintln!(
            "::notice title=Wright::{}",
            escape_workflow_data(&format!(
                "{withheld} annotation(s) withheld — the log group and step summary carry the full report"
            ))
        );
    }
    emit_summary(envelope, ctx, &annotations);
}

fn diagnostic_annotation(diagnostic: &wright_driver::Diagnostic) -> Option<GithubAnnotation> {
    let kind = match diagnostic.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => return None,
    };
    let mut props = vec![format!(
        "title={}",
        escape_workflow_property(&diagnostic.code)
    )];
    let mut path = String::new();
    let (mut line, mut col) = (0, 0);
    if let Some(span) = diagnostic
        .span
        .as_ref()
        .filter(|s| is_real_source_path(&s.path))
    {
        path = display_location_path(&span.path);
        line = u64::from(span.start.line);
        col = u64::from(span.start.col);
        props.insert(0, format!("file={}", escape_workflow_property(&path)));
        props.push(format!("line={}", span.start.line));
        props.push(format!("col={}", span.start.col));
        props.push(format!("endLine={}", span.end.line));
        props.push(format!("endColumn={}", span.end.col));
    }
    Some(GithubAnnotation {
        kind,
        props,
        title: diagnostic.code.clone(),
        message: diagnostic.message.clone(),
        path,
        line,
        col,
    })
}

fn finding_annotation(finding: &serde_json::Value, severity: Severity) -> Option<GithubAnnotation> {
    let kind = match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => return None,
    };
    let located;
    let finding = match finding_location_span(finding) {
        Some(span) => {
            located = {
                let mut located = finding.clone();
                located["span"] = span;
                located
            };
            &located
        }
        None => finding,
    };
    let span = finding.get("span").filter(|span| span.is_object())?;
    let path = span
        .get("path")
        .and_then(serde_json::Value::as_str)
        .filter(|path| is_real_source_path(path))
        .map(display_location_path)?;
    let line = span_position(span, "start", "line").unwrap_or(1);
    let col = span_position(span, "start", "col").unwrap_or(1);
    let end_line = span_position(span, "end", "line").unwrap_or(line);
    let end_col = span_position(span, "end", "col").unwrap_or(col);
    let code = finding
        .get("code")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("finding");
    let message = finding
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    Some(GithubAnnotation {
        kind,
        props: vec![
            format!("file={}", escape_workflow_property(&path)),
            format!("line={line}"),
            format!("col={col}"),
            format!("endLine={end_line}"),
            format!("endColumn={end_col}"),
            format!("title={}", escape_workflow_property(code)),
        ],
        title: code.to_string(),
        message,
        path,
        line,
        col,
    })
}

fn emit_workflow_annotation(kind: &str, properties: &[String], message: &str) {
    eprintln!(
        "::{kind} {}::{}",
        properties.join(","),
        escape_workflow_data(message)
    );
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum SummaryStatus {
    Pass,
    Warn,
    /// The `--fail-on` gate's raised verdict (ADR-0030 decision 13): exit 1
    /// after a result that met the threshold. A genuine command failure
    /// (`Error`, any nonzero exit the driver produced) still outranks it.
    Fail,
    Error,
}

impl SummaryStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Warn => "WARN",
            Self::Fail => "FAIL",
            Self::Error => "ERROR",
        }
    }
}

impl From<Severity> for SummaryStatus {
    fn from(severity: Severity) -> Self {
        match severity {
            Severity::Error => Self::Error,
            Severity::Warning => Self::Warn,
            Severity::Info => Self::Pass,
        }
    }
}

fn max_finding_severity(
    meta: &[wright_driver::FindingMeta],
    selection: &Option<wright_driver::SelectionOutcome>,
) -> Option<Severity> {
    match selection {
        Some(selection) => selection.max_severity,
        None => meta.iter().map(|meta| meta.severity).max(),
    }
}

pub(crate) trait ResultPresentation {
    fn metadata(&self) -> Option<String> {
        None
    }
    /// Text the command streams to stdout in place of a report.
    fn artifact(&self) -> Option<&str> {
        None
    }
    /// The result's findings paired with their typed severities (#1001).
    fn github_findings(&self) -> Vec<(serde_json::Value, Severity)> {
        Vec::new()
    }
    fn max_finding_severity(&self) -> Option<Severity> {
        None
    }
    fn update_summary_status(&self, status: &mut SummaryStatus) {
        if let Some(severity) = self.max_finding_severity() {
            *status = (*status).max(SummaryStatus::from(severity));
        }
    }
}

impl ResultPresentation for CompileResult {
    fn metadata(&self) -> Option<String> {
        let output = self.output.as_ref().filter(|o| o.written_to != "stdout")?;
        Some(format!(
            "wrote {} (sha256 {})",
            output.written_to, output.sha256
        ))
    }
    fn artifact(&self) -> Option<&str> {
        self.output
            .as_ref()
            .filter(|output| output.written_to == "stdout")
            .map(|output| output.text.as_str())
    }
}

impl ResultPresentation for ConvertResult {
    fn artifact(&self) -> Option<&str> {
        Some(&self.text)
    }
}

impl ResultPresentation for CheckResult {}

impl ResultPresentation for LintResult {
    fn metadata(&self) -> Option<String> {
        let rules = count(&self.program, "rules");
        if let Some(selection) = &self.selection {
            return Some(format!(
                "{} finding(s) across {} rule(s)",
                selection.total, rules
            ));
        }
        let counts = meta_severity_counts(&self.findings_meta);
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
        if let Some(fixes) = &self.fixes {
            let fixes_part = fix_status_parts(fix_status_counts(fixes));
            metadata.push_str(&format!("; {} fix(es)", fixes.len()));
            if !fixes_part.is_empty() {
                metadata.push_str(&format!(" ({})", fixes_part.join(", ")));
            }
        }
        Some(metadata)
    }
    fn github_findings(&self) -> Vec<(serde_json::Value, Severity)> {
        self.findings
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .zip(&self.findings_meta)
            .map(|(finding, meta)| (finding, meta.severity))
            .collect()
    }
    fn max_finding_severity(&self) -> Option<Severity> {
        max_finding_severity(&self.findings_meta, &self.selection)
    }
}

fn essential_metadata(value: &serde_json::Value) -> String {
    let counts = &value["counts"];
    let mut parts = Vec::new();
    for (key, noun) in [("findings", "finding(s)"), ("risks", "risk(s)")] {
        if let Some(group) = counts.get(key) {
            let total = group["total"].as_u64().unwrap_or(0);
            let error = group["error"].as_u64().unwrap_or(0);
            let warning = group["warning"].as_u64().unwrap_or(0);
            let info = group["info"].as_u64().unwrap_or(0);
            let mut severities = Vec::new();
            if error > 0 {
                severities.push(format!("{error} error(s)"));
            }
            if warning > 0 {
                severities.push(format!("{warning} warning(s)"));
            }
            if info > 0 {
                severities.push(format!("{info} info {noun}"));
            }
            parts.push(if severities.is_empty() {
                format!("{total} {noun}")
            } else {
                severities.join(", ")
            });
        }
    }
    for (key, label) in [
        ("rules", "rule(s)"),
        ("symbols", "symbol(s)"),
        ("references", "reference(s)"),
        ("elements", "element(s)"),
        ("persistentObjects", "persistent object(s)"),
        ("skipped", "skipped"),
    ] {
        if let Some(total) = counts.get(key).and_then(serde_json::Value::as_u64) {
            parts.push(format!("{total} {label}"));
        }
    }
    parts.join(", ")
}

impl ResultPresentation for EssentialResult {
    fn github_findings(&self) -> Vec<(serde_json::Value, Severity)> {
        let key = match self.command {
            wright_driver::result::EssentialCommand::Analyze => "risks",
            _ => "findings",
        };
        self.result[key]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .zip(&self.findings_meta)
            .map(|(finding, meta)| (finding, meta.severity))
            .collect()
    }

    fn metadata(&self) -> Option<String> {
        Some(if let Some(selection) = &self.selection {
            let noun = match self.command {
                wright_driver::result::EssentialCommand::Analyze => "risk(s)",
                _ => "finding(s)",
            };
            format!("{} {noun}", selection.total)
        } else {
            essential_metadata(&self.result)
        })
    }
    fn max_finding_severity(&self) -> Option<Severity> {
        if let Some(selection) = &self.selection {
            return selection.max_severity;
        }
        self.max_severity
    }
}

pub(crate) fn inline_location(span: &serde_json::Value) -> Option<String> {
    let path = span
        .get("path")
        .and_then(serde_json::Value::as_str)
        .filter(|path| !path.is_empty())?;
    let line = span_position(span, "start", "line").unwrap_or(0);
    let col = span_position(span, "start", "col").unwrap_or(0);
    Some(format!("{}:{line}:{col}", display_location_path(path)))
}

fn symbol_kind_heading(kind: &str) -> &str {
    match kind {
        "globalVariable" => "Global variables",
        "playerVariable" => "Player variables",
        "subroutine" => "Subroutines",
        "rule" => "Rules",
        _ => kind,
    }
}

pub(crate) fn query_items<'a>(value: &'a serde_json::Value, key: &str) -> &'a [serde_json::Value] {
    value
        .as_array()
        .or_else(|| value.get(key).and_then(serde_json::Value::as_array))
        .or_else(|| value.get("items").and_then(serde_json::Value::as_array))
        .map_or(&[][..], Vec::as_slice)
}

impl ResultPresentation for RulesResult {
    fn metadata(&self) -> Option<String> {
        Some(format!("{} rule(s)", query_items(&self.0, "rules").len()))
    }
}

pub(crate) fn render_node(node: &serde_json::Value) -> String {
    if let Some(call) = node["call"].as_str() {
        let args = node["args"]
            .as_array()
            .map(|args| args.iter().map(render_node).collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        return format!("{call}({args})");
    }
    if let (Some(domain), Some(member)) = (node["enum"].as_str(), node["member"].as_str()) {
        return format!("{domain}.{member}");
    }
    for key in ["variable", "subroutine", "string", "localizedString"] {
        if let Some(text) = node[key].as_str() {
            return format!("{key}:{text}");
        }
    }
    node.to_string()
}

impl ResultPresentation for SymbolsResult {
    fn metadata(&self) -> Option<String> {
        Some(format!(
            "{} symbol(s)",
            query_items(&self.0, "symbols").len()
        ))
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
}

impl ResultPresentation for CfgResult {
    fn metadata(&self) -> Option<String> {
        let blocks = self.0["blocks"].as_array().map_or(&[][..], Vec::as_slice);
        Some(format!(
            "{} block(s), {} edge(s)",
            blocks.len(),
            cfg_edge_count(blocks)
        ))
    }
}

impl ResultPresentation for CallGraphResult {
    fn metadata(&self) -> Option<String> {
        Some(format!("{} edge(s)", query_items(&self.0, "edges").len()))
    }
}

impl ResultPresentation for CostResult {
    fn metadata(&self) -> Option<String> {
        if let Some(rule) = self.result.get("rule").filter(|rule| rule.is_object()) {
            return Some(format!(
                "{} · {} element(s)",
                rule_name(rule),
                count(rule, "count"),
            ));
        }
        let exact = &self.result["exact"];
        let emitted = match exact["emittedBytes"].as_u64() {
            Some(bytes) => format!("{bytes} emitted byte(s)"),
            None => "emitted byte(s) unavailable".to_string(),
        };
        Some(format!(
            "{emitted}, {} action(s), {} rule(s), {} wait(s)",
            count(exact, "programActions"),
            count(exact, "programRules"),
            count(exact, "waitActions"),
        ))
    }
    fn github_findings(&self) -> Vec<(serde_json::Value, Severity)> {
        self.result["findings"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .zip(&self.findings_meta)
            .map(|(finding, meta)| (finding, meta.severity))
            .collect()
    }
    fn max_finding_severity(&self) -> Option<Severity> {
        max_finding_severity(&self.findings_meta, &self.selection)
    }
}

pub(crate) fn gate<T: serde::Serialize + ResultPresentation>(
    envelope: &mut Envelope<T>,
    threshold: Severity,
) {
    if envelope.exit != wright_driver::result::exit::SUCCESS {
        return;
    }
    let met = max_envelope_severity(envelope).is_some_and(|severity| severity >= threshold);
    if met {
        envelope.ok = false;
        envelope.exit = wright_driver::result::exit::SOURCE_ERROR;
        envelope.gated = true;
    }
}

fn max_envelope_severity<T: serde::Serialize + ResultPresentation>(
    envelope: &Envelope<T>,
) -> Option<Severity> {
    let diagnostics = match &envelope.selection {
        Some(selection) => selection.max_severity,
        None => envelope
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.severity)
            .max(),
    };
    diagnostics
        .into_iter()
        .chain(envelope.result.max_finding_severity())
        .max()
}

pub(crate) fn summary_status<T: serde::Serialize + ResultPresentation>(
    envelope: &Envelope<T>,
) -> &'static str {
    let mut status = if envelope.ok {
        SummaryStatus::Pass
    } else {
        SummaryStatus::Error
    };
    match &envelope.selection {
        Some(selection) => {
            if let Some(severity) = selection.max_severity {
                status = status.max(severity.into());
            }
        }
        None => {
            for diag in &envelope.diagnostics {
                status = status.max(diag.severity.into());
            }
        }
    }
    envelope.result.update_summary_status(&mut status);
    if envelope.gated {
        status = SummaryStatus::Fail;
    }
    status.as_str()
}

fn emit_summary<T: serde::Serialize + ResultPresentation + plain::PlainReport>(
    envelope: &Envelope<T>,
    ctx: &RenderContext<'_>,
    annotations: &[GithubAnnotation],
) {
    let status = summary_status(envelope);
    let input = ctx
        .source_base
        .map(plain::display_root)
        .or_else(|| ctx.subject.map(str::to_string))
        .unwrap_or_default();
    let mut section = format!(
        "## Wright `{}` — **{status}** (exit {})\n\n",
        if input.is_empty() {
            envelope.command.clone()
        } else {
            format!("{} {input}", envelope.command)
        },
        envelope.exit
    );
    if let Some(metadata) = envelope.result.metadata() {
        section.push_str(&format!("{metadata}\n\n"));
    }
    if let Some(comparison) = envelope.comparison.as_ref().filter(|c| c.is_object()) {
        let delta = comparison["totalDelta"].as_i64().unwrap_or(0);
        let sign = if delta > 0 { "+" } else { "" };
        let changes = ["changed", "added", "removed"]
            .iter()
            .map(|key| comparison[key].as_array().map_or(0, Vec::len))
            .sum::<usize>();
        section.push_str(&format!(
            "**Baseline {}**: total {sign}{delta} ({changes} change(s))\n\n",
            comparison["reference"].as_str().unwrap_or("previous"),
        ));
    }
    for annotation in annotations.iter().take(5) {
        let location = if annotation.path.is_empty() {
            String::new()
        } else {
            let location = format!("{}:{}:{}", annotation.path, annotation.line, annotation.col);
            format!(
                " {}",
                github_blob_url(&annotation.path, annotation.line)
                    .map(|url| format!("[`{location}`]({url})"))
                    .unwrap_or_else(|| format!("`{location}`"))
            )
        };
        section.push_str(&format!(
            "- `{}`{location} — {}\n",
            annotation.title, annotation.message
        ));
    }
    if !annotations.is_empty() {
        section.push('\n');
    }
    if envelope.result.artifact().is_some() {
        section.push_str("The command's artifact streamed to stdout.\n");
    } else {
        let (out, err) = plain::render_lines(envelope, ctx);
        let mut report = String::new();
        let mut shown = 0usize;
        let total = out.len() + err.len();
        for line in out.iter().chain(err.iter()) {
            let next = report.len() + line.len() + 1;
            if next > GITHUB_SUMMARY_BUDGET {
                break;
            }
            report.push_str(line);
            report.push('\n');
            shown += 1;
        }
        if shown < total {
            report.push_str(&format!("… {} line(s) omitted\n", total - shown));
        }
        section.push_str(&format!(
            "<details><summary>Full report</summary>\n\n```text\n{report}```\n\n</details>\n"
        ));
    }
    if let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        let result = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| write!(file, "{section}"));
        if let Err(error) = result {
            eprintln!(
                "::warning title=Wright summary::{}",
                escape_workflow_data(&error.to_string())
            );
        }
    } else {
        eprintln!(
            "::notice title=Wright summary::{}",
            escape_workflow_data(&format!(
                "Wright `{}`: **{status}** (exit {})",
                envelope.command, envelope.exit
            ))
        );
    }
}

fn github_blob_url(path: &str, line: u64) -> Option<String> {
    if path.is_empty() || Path::new(path).is_absolute() {
        return None;
    }
    let server = std::env::var("GITHUB_SERVER_URL").ok()?;
    let repository = std::env::var("GITHUB_REPOSITORY").ok()?;
    let sha = std::env::var("GITHUB_SHA").ok()?;
    Some(format!(
        "{server}/{repository}/blob/{sha}/{}#L{line}",
        path.trim_start_matches("./")
    ))
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

pub(crate) fn is_real_source_path(path: &str) -> bool {
    !path.is_empty() && !path.starts_with('<')
}

fn span_position(span: &serde_json::Value, edge: &str, coord: &str) -> Option<u64> {
    span.get(edge)
        .and_then(serde_json::Value::as_object)
        .and_then(|p| p.get(coord))
        .and_then(serde_json::Value::as_u64)
}

fn render_diff_lines(original: &str, new_text: &str, indent: &str) {
    for row in diff_rows(original, new_text) {
        println!("{indent}{row}");
    }
}

pub(crate) fn diff_rows(original: &str, new_text: &str) -> Vec<String> {
    let old_lines: Vec<&str> = original.split('\n').collect();
    let new_lines: Vec<&str> = new_text.split('\n').collect();
    let mut lo = 0usize;
    while lo < old_lines.len().min(new_lines.len()) && old_lines[lo] == new_lines[lo] {
        lo += 1;
    }
    let mut old_hi = old_lines.len();
    let mut new_hi = new_lines.len();
    while old_hi > lo && new_hi > lo && old_lines[old_hi - 1] == new_lines[new_hi - 1] {
        old_hi -= 1;
        new_hi -= 1;
    }
    let mut rows = Vec::new();
    for (removed, added) in align_lines(&old_lines[lo..old_hi], &new_lines[lo..new_hi]) {
        if let Some(index) = removed {
            rows.push(format!("{:>4} - {}", lo + index + 1, old_lines[lo + index]));
        }
        if let Some(index) = added {
            rows.push(format!("{:>4} + {}", lo + index + 1, new_lines[lo + index]));
        }
    }
    rows
}

fn align_lines<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(Option<usize>, Option<usize>)> {
    const MAX_CELLS: usize = 4_000_000;
    let (n, m) = (old.len(), new.len());
    if n.checked_mul(m).is_none_or(|cells| cells > MAX_CELLS) {
        return (0..n)
            .map(|index| (Some(index), None))
            .chain((0..m).map(|index| (None, Some(index))))
            .collect();
    }
    let mut lcs_suffix = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs_suffix[i][j] = if old[i] == new[j] {
                lcs_suffix[i + 1][j + 1] + 1
            } else {
                lcs_suffix[i + 1][j].max(lcs_suffix[i][j + 1])
            };
        }
    }
    let mut rows = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if old[i] == new[j] {
            i += 1;
            j += 1;
        } else if lcs_suffix[i + 1][j] >= lcs_suffix[i][j + 1] {
            rows.push((Some(i), None));
            i += 1;
        } else {
            rows.push((None, Some(j)));
            j += 1;
        }
    }
    rows.extend((i..n).map(|index| (Some(index), None)));
    rows.extend((j..m).map(|index| (None, Some(index))));
    rows
}

fn array_len(value: &serde_json::Value) -> usize {
    value.as_array().map_or(0, Vec::len)
}

pub(crate) fn count(value: &serde_json::Value, key: &str) -> usize {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize
}

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

fn persistent_object_summary(objects: &serde_json::Value) -> String {
    let kinds = objects["byKind"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(kind, n)| format!("{n} {kind}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut notes = vec![format!("{} site(s): {kinds}", count(objects, "total"))];
    let without_identity = count(objects, "withoutRetainedIdentity");
    if without_identity > 0 {
        notes.push(format!("{without_identity} without retained identity"));
    }
    let visible = count(objects, "visibleToAllPlayers");
    if visible > 0 {
        notes.push(format!("{visible} visible to all players"));
    }
    format!("{} [static]", notes.join("; "))
}

pub(crate) fn display_location_path(path: &str) -> String {
    wright_driver::input::display_path(Path::new(path))
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
            columns: None,
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

    /// Build the typed finding metas a `GetFindings` fetch would have
    /// produced for `findings` (#1001): tests construct the parallel
    /// channel explicitly, the same way `SemanticService` derives it.
    fn findings_meta(findings: &serde_json::Value) -> Vec<wright_driver::FindingMeta> {
        findings
            .as_array()
            .into_iter()
            .flatten()
            .map(|finding| wright_driver::FindingMeta {
                severity: Severity::from_label(finding["severity"].as_str()),
                code: finding["code"].as_str().unwrap_or_default().to_string(),
                span_file: None,
            })
            .collect()
    }

    fn summary_envelope(
        ok: bool,
        diagnostics: Vec<wright_driver::Diagnostic>,
        findings: serde_json::Value,
    ) -> Envelope<LintResult> {
        let findings_meta = findings_meta(&findings);
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
            gated: false,
            result: LintResult {
                findings,
                findings_meta,
                ..LintResult::default()
            },
            comparison: None,
        }
    }

    fn diagnostic(severity: Severity) -> wright_driver::Diagnostic {
        let mut diagnostic =
            wright_driver::Diagnostic::error("test", wright_driver::Stage::Analysis, "test");
        diagnostic.severity = severity;
        diagnostic
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

    #[test]
    fn gate_raises_warn_to_fail_on_the_envelope_and_verdict() {
        let mut envelope = summary_envelope(
            true,
            vec![diagnostic(Severity::Warning)],
            serde_json::json!([{"severity": "warning"}]),
        );
        gate(&mut envelope, Severity::Warning);
        assert!(envelope.gated);
        assert_eq!(envelope.exit, 1);
        assert!(!envelope.ok);
        assert_eq!(summary_status(&envelope), "FAIL");
    }

    #[test]
    fn gate_fires_on_findings_without_diagnostics() {
        let mut envelope = summary_envelope(
            true,
            Vec::new(),
            serde_json::json!([{"severity": "warning"}]),
        );
        gate(&mut envelope, Severity::Warning);
        assert_eq!(envelope.exit, 1);
        assert_eq!(summary_status(&envelope), "FAIL");
    }

    #[test]
    fn gate_counts_info_severity_for_fail_on_info() {
        let mut envelope =
            summary_envelope(true, Vec::new(), serde_json::json!([{"severity": "info"}]));
        gate(&mut envelope, Severity::Info);
        assert_eq!(envelope.exit, 1);
        assert_eq!(summary_status(&envelope), "FAIL");
    }

    #[test]
    fn gate_judges_the_full_set_not_the_selected_remainder() {
        let mut envelope = check_envelope(
            vec![diagnostic(Severity::Info)],
            Some(wright_driver::SelectionOutcome {
                total: 2,
                withheld: 0,
                by_code: Default::default(),
                max_severity: Some(Severity::Warning),
            }),
        );
        gate(&mut envelope, Severity::Warning);
        assert_eq!(envelope.exit, 1);
        assert_eq!(summary_status(&envelope), "FAIL");
    }

    #[test]
    fn gate_reads_findings_severity_recorded_by_a_result_selection() {
        let mut envelope =
            summary_envelope(true, Vec::new(), serde_json::json!([{"severity": "info"}]));
        envelope.result.selection = Some(wright_driver::SelectionOutcome {
            total: 3,
            withheld: 2,
            by_code: Default::default(),
            max_severity: Some(Severity::Warning),
        });
        gate(&mut envelope, Severity::Warning);
        assert_eq!(envelope.exit, 1);
        assert_eq!(summary_status(&envelope), "FAIL");
    }

    #[test]
    fn gate_never_lowers_a_failing_result() {
        let mut envelope = summary_envelope(
            false,
            vec![diagnostic(Severity::Error)],
            serde_json::json!([]),
        );
        gate(&mut envelope, Severity::Info);
        assert!(!envelope.gated);
        assert_eq!(envelope.exit, 1);
        assert_eq!(summary_status(&envelope), "ERROR");
    }

    #[test]
    fn gate_leaves_a_result_below_the_threshold_untouched() {
        let mut envelope = summary_envelope(
            true,
            vec![diagnostic(Severity::Warning)],
            serde_json::json!([{"severity": "warning"}]),
        );
        gate(&mut envelope, Severity::Error);
        assert!(!envelope.gated);
        assert_eq!(envelope.exit, 0);
        assert_eq!(summary_status(&envelope), "WARN");
    }

    #[test]
    fn gate_leaves_a_clean_result_passing() {
        let mut envelope = summary_envelope(true, Vec::new(), serde_json::json!([]));
        gate(&mut envelope, Severity::Info);
        assert!(!envelope.gated);
        assert_eq!(envelope.exit, 0);
        assert_eq!(summary_status(&envelope), "PASS");
    }

    #[test]
    fn gate_reads_the_essential_projection_severity() {
        let mut envelope = Envelope {
            wright: wright_driver::result::VersionInfo {
                version: "test".to_string(),
                contract: "wright-result/v1".to_string(),
            },
            command: "analyze".to_string(),
            ok: true,
            exit: 0,
            diagnostics: Vec::new(),
            selection: None,
            gated: false,
            result: wright_driver::result::EssentialResult {
                result: serde_json::json!({
                    "counts": {"risks": {"total": 2, "warning": 2, "error": 0, "info": 0}}
                }),
                selection: None,
                command: wright_driver::result::EssentialCommand::Analyze,
                findings_meta: Vec::new(),
                max_severity: Some(Severity::Warning),
            },
            comparison: None,
        };
        gate(&mut envelope, Severity::Warning);
        assert_eq!(envelope.exit, 1);
        assert_eq!(summary_status(&envelope), "FAIL");
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
            gated: false,
            result: CheckResult::default(),
            comparison: None,
        }
    }

    fn lint_result(
        findings: serde_json::Value,
        selection: Option<wright_driver::SelectionOutcome>,
    ) -> LintResult {
        LintResult {
            program: serde_json::json!({"rules": 2}),
            rules: serde_json::json!([
                {"id": "a", "effectiveSeverity": "warning"},
                {"id": "b", "effectiveSeverity": "error"},
            ]),
            findings_meta: findings_meta(&findings),
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
                by_code: Default::default(),
                max_severity: Some(Severity::Error),
            }),
        );
        assert_eq!(result.metadata().unwrap(), "7 finding(s) across 2 rule(s)");
    }

    #[test]
    fn severity_rank_orders_errors_first() {
        assert_eq!(severity_rank(Severity::Error), 0);
        assert_eq!(severity_rank(Severity::Warning), 1);
        assert_eq!(severity_rank(Severity::Info), 2);
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

        let pseudo = serde_json::json!({"path": "<stdin>", "start": {"line": 1, "col": 1}});
        assert!(finding_location(&pseudo, Some(&root)).is_none());
        let wrapped = serde_json::json!({"span": pseudo});
        let positions = unmapped_positions(&[&wrapped]);
        assert_eq!(
            unmapped_positions_note(&positions).unwrap(),
            "at <stdin>:1:1 (no source file)"
        );

        let missing = serde_json::json!({"path": "gone.ws", "start": {"line": 1, "col": 1}});
        let (display, readable) = finding_location(&missing, Some(&root)).unwrap();
        assert_eq!(display, "gone.ws");
        assert!(readable.is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn renderable_group_spans_counts_only_real_source_locations() {
        let real = |path: &str| serde_json::json!({"span": {"path": path, "start": {"line": 1, "col": 1}}});
        let group = [
            real("a.ws"),
            real("<stdin>"),
            serde_json::json!({"span": null}),
            real("b.ws"),
        ];
        let group: Vec<_> = group.iter().collect();
        assert_eq!(renderable_group_spans(&group).len(), 2);
    }

    #[test]
    fn unmapped_positions_note_collapses_a_shared_pseudo_path() {
        let finding = |line: u64| serde_json::json!({"span": {"path": "<stdin>", "start": {"line": line, "col": 1}}});
        let group = [finding(11), finding(14)];
        let group: Vec<_> = group.iter().collect();
        assert_eq!(
            unmapped_positions_note(&unmapped_positions(&group)).unwrap(),
            "at <stdin>:11:1, 14:1 (no source file)"
        );
    }

    #[test]
    fn unmapped_positions_note_preserves_every_selected_position() {
        let finding = |line: u64| serde_json::json!({"span": {"path": "<stdin>", "start": {"line": line, "col": 1}}});
        let group: Vec<_> = (1..=12).map(finding).collect();
        let group: Vec<_> = group.iter().collect();
        let note = unmapped_positions_note(&unmapped_positions(&group)).unwrap();
        for line in 1..=12 {
            assert!(
                note.contains(&format!("{line}:1")),
                "selected position {line}:1 is missing: {note}"
            );
        }
        assert!(
            !note.contains("more"),
            "the renderer must not impose an additional cap: {note}"
        );
    }

    #[test]
    fn align_lines_keeps_unchanged_context_out_of_the_diff() {
        let old = vec!["a", "b", "c", "d", "e", "f", "g"];
        let new = vec!["a", "b", "c", "f", "g"];
        assert_eq!(
            align_lines(&old, &new),
            vec![(Some(3), None), (Some(4), None)]
        );
    }

    #[test]
    fn align_lines_skips_matching_lines_inside_a_changed_region() {
        let old = vec!["x", "keep", "y"];
        let new = vec!["u", "keep", "v"];
        assert_eq!(
            align_lines(&old, &new),
            vec![
                (Some(0), None),
                (None, Some(0)),
                (Some(2), None),
                (None, Some(2))
            ]
        );
    }

    #[test]
    fn align_lines_pairs_a_pure_insertion_without_removing() {
        let old = vec!["a", "d"];
        let new = vec!["a", "b", "c", "d"];
        assert_eq!(
            align_lines(&old, &new),
            vec![(None, Some(1)), (None, Some(2))]
        );
    }

    fn annotation(kind: &'static str, title: &str, line: u64) -> GithubAnnotation {
        GithubAnnotation {
            kind,
            props: Vec::new(),
            title: title.to_string(),
            message: String::new(),
            path: "flow.ws".to_string(),
            line,
            col: 1,
        }
    }

    #[test]
    fn annotations_dedup_by_rule_and_location() {
        let mut annotations = vec![
            annotation("warning", "a", 1),
            annotation("warning", "a", 1),
            annotation("warning", "a", 2),
        ];
        assert_eq!(bound_annotations(&mut annotations), 0);
        assert_eq!(annotations.len(), 2);
    }

    #[test]
    fn annotations_bound_each_severity_class_separately() {
        let mut annotations: Vec<GithubAnnotation> = (1..=12)
            .map(|line| annotation("warning", "a", line))
            .chain((1..=3).map(|line| annotation("error", "b", line)))
            .collect();
        assert_eq!(bound_annotations(&mut annotations), 2);
        assert_eq!(annotations.len(), 13);
        assert!(annotations[..3].iter().all(|a| a.kind == "error"));
        assert!(annotations[3..].iter().all(|a| a.kind == "warning"));
    }
}
