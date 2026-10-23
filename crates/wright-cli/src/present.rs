use std::collections::BTreeSet;
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
pub(crate) mod terminal;

pub(crate) use addresses::{edge_caller, rule_address, rule_name, tree_rule};

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
            wright_driver::essential::envelope(&mut value, pres.all);
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
        Renderer::Terminal => terminal::render(envelope, pres, elapsed, source_base, subject),
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

pub(crate) fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    }
}

fn dim(value: &str, color: bool) -> String {
    if color {
        format!("\x1b[2m{value}\x1b[0m")
    } else {
        value.to_string()
    }
}

struct Marks {
    head: String,
    pad: String,
}

impl Presentation {
    fn section_marks(&self) -> Marks {
        self.connector("  ")
    }

    fn connector(&self, base: &str) -> Marks {
        Marks {
            head: base.to_string(),
            pad: format!("{base}  "),
        }
    }

    fn footer_head(&self) -> &'static str {
        "  "
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
    for finding in envelope.result.github_findings() {
        if let Some(annotation) = finding_annotation(&finding) {
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
    if envelope.result.is_artifact_stream() {
        envelope.result.render_body(&github_context(ctx.subject));
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

fn github_context(subject: Option<&str>) -> RenderContext<'_> {
    RenderContext {
        presentation: Presentation {
            format: OutputFormat::Text,
            renderer: Renderer::GithubActions,
            color: false,
            all: false,
            interactive: false,
            width: MAX_WIDTH,
        },
        elapsed: Duration::ZERO,
        source_base: None,
        subject,
    }
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

fn finding_annotation(finding: &serde_json::Value) -> Option<GithubAnnotation> {
    let kind = match finding_severity(finding) {
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

fn severity_level(severity: Severity) -> u8 {
    match severity {
        Severity::Info => 0,
        Severity::Warning => 1,
        Severity::Error => 2,
    }
}

fn finding_severity(finding: &serde_json::Value) -> Severity {
    match finding.get("severity").and_then(serde_json::Value::as_str) {
        Some("error") => Severity::Error,
        Some("info") | Some("notice") => Severity::Info,
        _ => Severity::Warning,
    }
}

fn max_finding_severity(
    findings: &serde_json::Value,
    selection: &Option<wright_driver::SelectionOutcome>,
) -> Option<Severity> {
    match selection {
        Some(selection) => selection.max_severity,
        None => findings
            .as_array()
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .map(finding_severity)
            .max_by_key(|severity| severity_level(*severity)),
    }
}

pub(crate) trait ResultPresentation {
    fn metadata(&self) -> Option<String> {
        None
    }
    fn render_body(&self, ctx: &RenderContext<'_>);
    fn is_artifact_stream(&self) -> bool {
        false
    }
    fn github_findings(&self) -> Vec<serde_json::Value> {
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
    fn is_artifact_stream(&self) -> bool {
        self.output
            .as_ref()
            .is_some_and(|o| o.written_to == "stdout")
    }
    fn render_body(&self, _ctx: &RenderContext<'_>) {
        render_compile(self);
    }
}

impl ResultPresentation for ConvertResult {
    fn is_artifact_stream(&self) -> bool {
        true
    }
    fn render_body(&self, _ctx: &RenderContext<'_>) {
        render_convert(self);
    }
}

impl ResultPresentation for CheckResult {
    fn render_body(&self, _ctx: &RenderContext<'_>) {}
}

impl ResultPresentation for LintResult {
    fn metadata(&self) -> Option<String> {
        let rules = count(&self.program, "rules");
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
        if let Some(fixes) = &self.fixes {
            let mut applied = 0usize;
            let mut previewed = 0usize;
            let mut refused = 0usize;
            for fix in fixes {
                match fix.status {
                    wright_driver::result::LintFixStatus::Applied => applied += 1,
                    wright_driver::result::LintFixStatus::Preview => previewed += 1,
                    wright_driver::result::LintFixStatus::Refused => refused += 1,
                }
            }
            let mut fixes_part = Vec::new();
            if applied > 0 {
                fixes_part.push(format!("{applied} applied"));
            }
            if previewed > 0 {
                fixes_part.push(format!("{previewed} previewed"));
            }
            if refused > 0 {
                fixes_part.push(format!("{refused} refused"));
            }
            metadata.push_str(&format!("; {} fix(es)", fixes.len()));
            if !fixes_part.is_empty() {
                metadata.push_str(&format!(" ({})", fixes_part.join(", ")));
            }
        }
        Some(metadata)
    }
    fn render_body(&self, ctx: &RenderContext<'_>) {
        render_lint(self, ctx);
        render_lint_fixes(self, ctx);
    }
    fn github_findings(&self) -> Vec<serde_json::Value> {
        self.findings.as_array().cloned().unwrap_or_default()
    }
    fn max_finding_severity(&self) -> Option<Severity> {
        max_finding_severity(&self.findings, &self.selection)
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
    fn github_findings(&self) -> Vec<serde_json::Value> {
        ["findings", "risks"]
            .iter()
            .flat_map(|key| self.result[key].as_array().cloned().unwrap_or_default())
            .collect()
    }

    fn metadata(&self) -> Option<String> {
        Some(if let Some(selection) = &self.selection {
            let noun = if self.result["counts"].get("risks").is_some() {
                "risk(s)"
            } else {
                "finding(s)"
            };
            format!("{} {noun}", selection.total)
        } else {
            essential_metadata(&self.result)
        })
    }
    fn render_body(&self, ctx: &RenderContext<'_>) {
        render_essential(self, ctx);
    }
    fn max_finding_severity(&self) -> Option<Severity> {
        if let Some(selection) = &self.selection {
            return selection.max_severity;
        }
        ["findings", "risks"]
            .iter()
            .filter_map(|key| self.result["counts"].get(key))
            .filter_map(|group| {
                [
                    ("error", Severity::Error),
                    ("warning", Severity::Warning),
                    ("info", Severity::Info),
                ]
                .into_iter()
                .find(|(label, _)| group[label].as_u64().unwrap_or(0) > 0)
                .map(|(_, severity)| severity)
            })
            .max_by_key(|severity| severity_level(*severity))
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

fn print_query_withheld(value: &serde_json::Value, noun: &str) {
    print_truncation(value);
    if let Some(withheld) = value["selection"]["withheld"]
        .as_u64()
        .filter(|withheld| *withheld > 0)
    {
        println!("  ... {withheld} {noun} withheld (--max)");
    }
}

impl ResultPresentation for RulesResult {
    fn metadata(&self) -> Option<String> {
        Some(format!("{} rule(s)", query_items(&self.0, "rules").len()))
    }
    fn render_body(&self, ctx: &RenderContext<'_>) {
        let rules = query_items(&self.0, "rules");
        println!("\nRules");
        if rules.is_empty() {
            println!("  none");
            return;
        }
        for rule in rules {
            let marks = ctx.presentation.section_marks();
            let name = rule_name(rule);
            let event = rule["event"].as_str().unwrap_or("?");
            match rule.get("span").and_then(inline_location) {
                Some(location) => println!("{}{name} [{event}] --> {location}", marks.head),
                None => println!("{}{name} [{event}]", marks.head),
            }
            let content = &rule["content"];
            for condition in content["conditions"].as_array().into_iter().flatten() {
                println!("{}if {}", marks.pad, render_node(&condition["value"]));
            }
            for action in content["actions"].as_array().into_iter().flatten() {
                println!("{}{}", marks.pad, render_node(action));
            }
        }
        print_query_withheld(&self.0, "rule(s)");
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
    fn render_body(&self, _ctx: &RenderContext<'_>) {
        let symbols = query_items(&self.0, "symbols");
        println!("\nSymbols");
        if symbols.is_empty() {
            println!("  none");
            return;
        }
        let mut groups: Vec<(&str, Vec<&serde_json::Value>)> = Vec::new();
        for symbol in symbols {
            let kind = symbol["kind"].as_str().unwrap_or("symbol");
            match groups.iter_mut().find(|(k, _)| *k == kind) {
                Some((_, members)) => members.push(symbol),
                None => groups.push((kind, vec![symbol])),
            }
        }
        for (kind, members) in groups {
            println!("  {} ({})", symbol_kind_heading(kind), members.len());
            for symbol in members.iter() {
                let name = symbol["name"]
                    .as_str()
                    .filter(|name| !name.is_empty())
                    .unwrap_or("<unnamed>");
                match symbol.get("span").and_then(inline_location) {
                    Some(location) => println!("    {name} --> {location}"),
                    None => println!("    {name}"),
                }
            }
        }
        print_query_withheld(&self.0, "symbol(s)");
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
            "\nReferences to {} ({})",
            self.0["symbol"].as_str().unwrap_or("<unknown>"),
            self.0["kind"].as_str().unwrap_or("symbol"),
        );
        if references.is_empty() {
            println!("  none");
            return;
        }
        for reference in references.iter() {
            let owner = rule_address(&reference["owningRule"])
                .map(String::from)
                .or_else(|| reference["rule"].as_u64().map(|rule| rule.to_string()));
            let context = match (owner, reference["action"].as_u64()) {
                (Some(rule), Some(action)) => format!(" (rule {rule}, action {action})"),
                (Some(rule), None) => format!(" (rule {rule})"),
                _ => String::new(),
            };
            let location = reference
                .get("span")
                .and_then(inline_location)
                .map(|location| format!(" --> {location}"))
                .unwrap_or_default();
            println!(
                "  {}{}{}",
                reference["kind"].as_str().unwrap_or("reference"),
                location,
                context
            );
        }
        print_query_withheld(&self.0, "reference(s)");
    }
}

fn notable_callers<'a>(
    counts: &std::collections::BTreeMap<&'a str, usize>,
) -> Vec<(&'a str, usize)> {
    let mut ranked: Vec<(&'a str, usize)> = counts.iter().map(|(name, n)| (*name, *n)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    ranked.retain(|(_, n)| *n > 1);
    ranked
}

fn cfg_edge_count(blocks: &[serde_json::Value]) -> usize {
    blocks
        .iter()
        .map(|block| array_len(&block["successors"]))
        .sum()
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
    fn render_body(&self, ctx: &RenderContext<'_>) {
        let blocks = self.0["blocks"].as_array().map_or(&[][..], Vec::as_slice);
        match ctx.subject {
            Some(rule) => println!("\nControl-flow graph of rule \"{rule}\""),
            None => println!("\nControl-flow graph"),
        }
        if blocks.is_empty() {
            println!("  none");
            return;
        }
        let loops = blocks
            .iter()
            .filter(|block| matches!(block["kind"].as_str(), Some("while" | "for")))
            .count();
        let waits = blocks
            .iter()
            .filter(|block| block["waits"].as_bool().unwrap_or(false))
            .count();
        let calls: usize = blocks.iter().map(|block| array_len(&block["calls"])).sum();
        println!(
            "  {} block(s), {} edge(s): {} loop header(s), {} wait block(s), {} call site(s)",
            blocks.len(),
            cfg_edge_count(blocks),
            loops,
            waits,
            calls
        );
        println!(
            "  entry: block {}, exit: block {}",
            self.0["entry"].as_u64().unwrap_or(0),
            self.0["exit"].as_u64().unwrap_or(0)
        );
        for block in blocks {
            let marks = ctx.presentation.section_marks();
            let mut flags = Vec::new();
            if block["waits"].as_bool().unwrap_or(false) {
                flags.push("waits");
            }
            if block["calls"]
                .as_array()
                .is_some_and(|calls| !calls.is_empty())
            {
                flags.push("calls");
            }
            let flags = if flags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", flags.join(" "))
            };
            println!(
                "{}block {} ({}): {} action(s){}",
                marks.head,
                block["id"].as_u64().unwrap_or(0),
                block["kind"].as_str().unwrap_or("block"),
                array_len(&block["actions"]),
                flags
            );
            if let Some(successors) = block["successors"]
                .as_array()
                .filter(|successors| !successors.is_empty())
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
                println!("{}-> {successors}", marks.pad);
            }
        }
        print_query_withheld(&self.0, "block(s)");
    }
}

impl ResultPresentation for CallGraphResult {
    fn metadata(&self) -> Option<String> {
        Some(format!("{} edge(s)", query_items(&self.0, "edges").len()))
    }
    fn render_body(&self, _ctx: &RenderContext<'_>) {
        let edges = query_items(&self.0, "edges");
        println!("\nCall graph");
        if edges.is_empty() {
            println!("  none");
            return;
        }
        let mut fan_in = std::collections::BTreeMap::<&str, usize>::new();
        let mut fan_out = std::collections::BTreeMap::<&str, usize>::new();
        for edge in edges {
            *fan_in
                .entry(edge["callee"].as_str().unwrap_or("<unnamed>"))
                .or_default() += 1;
            *fan_out.entry(edge_caller(edge)).or_default() += 1;
        }
        println!(
            "  {} edge(s): {} calling rule(s), {} subroutine(s) called",
            edges.len(),
            fan_out.len(),
            fan_in.len()
        );
        let most_called = notable_callers(&fan_in);
        if !most_called.is_empty() {
            println!("\n  Most-called subroutines");
            for (name, n) in most_called.iter() {
                println!("    {name}: {n} caller(s)");
            }
        }
        let busiest = notable_callers(&fan_out);
        if !busiest.is_empty() {
            println!("\n  Busiest callers");
            for (name, n) in busiest.iter() {
                println!("    {name}: calls {n} subroutine(s)");
            }
        }
        println!("\n  Edges");
        for edge in edges.iter() {
            println!(
                "    {} -> {}",
                edge_caller(edge),
                edge["callee"].as_str().unwrap_or("<unnamed>")
            );
        }
        print_query_withheld(&self.0, "edge(s)");
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
    fn render_body(&self, ctx: &RenderContext<'_>) {
        if self.result.get("rule").is_some_and(|rule| rule.is_object()) {
            render_cost_rule_body(&self.result, ctx);
            return;
        }
        let exact = &self.result["exact"];
        println!("\nGenerated resources (exact)");
        match exact["emittedBytes"].as_u64() {
            Some(bytes) => println!("  emitted bytes: {bytes}"),
            None => println!(
                "  emitted bytes: unavailable — {}",
                exact["emittedBytesUnavailable"]
                    .as_str()
                    .unwrap_or("the emitter does not model this program")
            ),
        }
        println!("  actions: {}", count(exact, "programActions"));
        println!("  rules: {}", count(exact, "programRules"));
        println!("  waits: {}", count(exact, "waitActions"));

        let elements = &self.result["elementCount"];
        if let Some(total) = elements["total"].as_u64() {
            println!(
                "  {total} element(s) of the {}-element budget [exact] — canonical structure, not a statement of client importability",
                count(elements, "budget"),
            );
            for rule in elements["rules"].as_array().into_iter().flatten() {
                match rule.get("span").and_then(inline_location) {
                    Some(location) => println!(
                        "    {} element(s)  {}  {location}",
                        count(rule, "count"),
                        rule_name(rule),
                    ),
                    None => println!(
                        "    {} element(s)  {}",
                        count(rule, "count"),
                        rule_name(rule),
                    ),
                }
            }
            if let Some(note) = elements["truncation"]["rules"].as_object() {
                println!(
                    "    showing {} of {}; --all to see every item",
                    note["shown"], note["total"]
                );
            }
        } else {
            let reason = self.result["elementCountUnavailable"]
                .as_str()
                .unwrap_or("counter does not model this program");
            match elements["budget"].as_u64() {
                Some(budget) => println!(
                    "  element count unavailable: {reason} — the {budget}-element budget still applies"
                ),
                None => println!("  element count unavailable: {reason}"),
            }
        }

        let mut findings = self.result["findings"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        findings.sort_by_key(|finding| match finding["severity"].as_str() {
            Some("error") => 0u8,
            Some("warning") => 1,
            _ => 2,
        });
        println!("\nStatic findings (evidence: static)");
        if findings.is_empty() {
            println!("  none");
        }
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
            let group = &findings[index..end];
            let severity = first["severity"].as_str().unwrap_or("info");
            let code = first["code"].as_str().unwrap_or("finding");
            let message = first["message"].as_str().unwrap_or_default();
            match group.len() {
                1 => println!("  {severity}[{code}]: {message}"),
                count => println!("  {severity}[{code}] ({count} occurrences): {message}"),
            }
            for finding in group {
                if let Some(location) = finding.get("span").and_then(inline_location) {
                    println!("    --> {location}");
                }
            }
            index = end;
        }
        print_truncation(&self.result);
        if let Some(withheld) = self.result["selection"]["withheld"]
            .as_u64()
            .filter(|withheld| *withheld > 0)
        {
            println!("  ... {withheld} finding(s) withheld (--max)");
        }
        let skipped = array_len(&self.result["skipped"]);
        if skipped > 0 {
            println!("  {skipped} rule evaluation(s) skipped");
        }
    }
    fn github_findings(&self) -> Vec<serde_json::Value> {
        self.result["findings"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
    fn max_finding_severity(&self) -> Option<Severity> {
        max_finding_severity(&self.result["findings"], &self.selection)
    }
}

fn render_cost_rule_body(result: &serde_json::Value, ctx: &RenderContext<'_>) {
    let rule = &result["rule"];
    let marks = ctx.presentation.section_marks();
    println!("\nElement cost");
    match rule.get("span").and_then(inline_location) {
        Some(location) => println!(
            "{}{} · {} element(s) · {location}",
            marks.head,
            rule_name(rule),
            count(rule, "count"),
        ),
        None => println!(
            "{}{} · {} element(s)",
            marks.head,
            rule_name(rule),
            count(rule, "count"),
        ),
    }
    let statements = result["statements"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let by_node = result["byNode"].as_array().map_or(&[][..], Vec::as_slice);
    let shown = |key: &str| {
        result["truncation"][key].as_object().map(|note| {
            (
                note["shown"].as_u64().unwrap_or(0),
                note["total"].as_u64().unwrap_or(0),
            )
        })
    };
    println!("\nCostliest statements");
    if statements.is_empty() {
        println!("  none");
    }
    for statement in statements {
        match statement.get("span").and_then(inline_location) {
            Some(location) => println!(
                "{}{} element(s)  {}  {location}",
                marks.head,
                count(statement, "count"),
                statement["name"].as_str().unwrap_or("?"),
            ),
            None => println!(
                "{}{} element(s)  {}",
                marks.head,
                count(statement, "count"),
                statement["name"].as_str().unwrap_or("?"),
            ),
        }
    }
    if let Some((shown, total)) = shown("statements") {
        println!("  showing {shown} of {total}; --all to see every item");
    }
    println!("\nBy node name");
    if by_node.is_empty() {
        println!("  none");
    }
    for entry in by_node {
        let occurrences = entry["occurrences"].as_u64().unwrap_or(1);
        let times = if occurrences > 1 {
            format!(" ×{occurrences}")
        } else {
            String::new()
        };
        println!(
            "{}{} element(s)  {}{}",
            marks.head,
            count(entry, "count"),
            entry["name"].as_str().unwrap_or("?"),
            times,
        );
    }
    if let Some((shown, total)) = shown("byNode") {
        println!("  showing {shown} of {total}; --all to see every item");
    }
}

pub(crate) fn gate<T: serde::Serialize + ResultPresentation>(
    envelope: &mut Envelope<T>,
    threshold: Severity,
) {
    if envelope.exit != wright_driver::result::exit::SUCCESS {
        return;
    }
    let met = max_envelope_severity(envelope)
        .is_some_and(|severity| severity_level(severity) >= severity_level(threshold));
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
            .max_by_key(|severity| severity_level(*severity)),
    };
    diagnostics
        .into_iter()
        .chain(envelope.result.max_finding_severity())
        .max_by_key(|severity| severity_level(*severity))
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
    if envelope.result.is_artifact_stream() {
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

pub(crate) fn finding_severity_rank(finding: &serde_json::Value) -> u8 {
    match finding.get("severity").and_then(serde_json::Value::as_str) {
        Some("error") => 0,
        Some("info") | Some("notice") => 2,
        _ => 1,
    }
}

fn render_lint_fixes(result: &LintResult, ctx: &RenderContext<'_>) {
    let Some(fixes) = &result.fixes else {
        return;
    };
    println!("\nLint fixes");
    if fixes.is_empty() {
        println!("  none offered");
        return;
    }
    for fix in fixes {
        let marks = ctx.presentation.section_marks();
        let status = match fix.status {
            wright_driver::result::LintFixStatus::Preview => "fix",
            wright_driver::result::LintFixStatus::Applied => "applied",
            wright_driver::result::LintFixStatus::Refused => "refused",
        };
        println!("{}{status}[{}]: {}", marks.head, fix.code, fix.summary);
        if let Some(span) = fix.span.as_ref().filter(|span| span.is_object()) {
            render_finding_location(span, ctx, &marks.pad, false);
        }
        for preview in fix.preview.iter().flatten() {
            println!("{}{}", marks.pad, preview.source);
            render_diff_lines(
                &preview.original,
                &preview.new_text,
                &format!("{}  ", marks.pad),
            );
        }
        for diagnostic in &fix.diagnostics {
            println!(
                "{}refused — {}: {}",
                marks.pad, diagnostic.code, diagnostic.message
            );
        }
    }
}

fn render_lint(result: &LintResult, ctx: &RenderContext<'_>) {
    let mut findings = result.findings.as_array().cloned().unwrap_or_default();
    findings.sort_by_key(finding_severity_rank);
    println!("\nLint findings");
    if findings.is_empty() {
        println!("  none");
    }
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
        let marks = ctx.presentation.section_marks();
        render_finding_group(&findings[index..end], ctx, &marks);
        index = end;
    }
    if let Some(selection) = &result.selection
        && selection.withheld > 0
    {
        println!("  ... {} finding(s) withheld (--max)", selection.withheld);
    }
    let files: BTreeSet<&str> = findings
        .iter()
        .filter_map(|finding| finding.get("span"))
        .filter_map(|span| span.get("path"))
        .filter_map(serde_json::Value::as_str)
        .filter(|path| is_real_source_path(path))
        .collect();
    let parts = lint_footer_parts(files.len(), array_len(&result.skipped), ctx);
    if !parts.is_empty() {
        println!(
            "{}{}",
            ctx.presentation.footer_head(),
            dim(&parts.join(" · "), ctx.presentation.color)
        );
    }
}

fn render_essential(result: &EssentialResult, ctx: &RenderContext<'_>) {
    let value = &result.result;
    if value.get("hotspots").is_some() {
        render_essential_analyze(value, ctx);
    } else if value.get("program").is_some() {
        let p = &value["program"];
        println!(
            "\nProgram overview\n  {} file(s), {} rule(s), {} global variable(s), {} player variable(s), {} subroutine(s)",
            count(p, "files"),
            count(p, "rules"),
            count(p, "globalVariables"),
            count(p, "playerVariables"),
            count(p, "subroutines")
        );
        println!("\nRules");
        for rule in value["rules"].as_array().into_iter().flatten() {
            match rule_address(rule) {
                Some(name) => println!("  {name}"),
                None => println!("  rule {}", rule["id"]),
            }
        }
    } else {
        let mut findings = value["findings"].clone();
        for finding in findings.as_array_mut().into_iter().flatten() {
            if let Some(span) = finding_location_span(finding) {
                finding["span"] = span;
            }
        }
        render_lint(
            &LintResult {
                findings,
                selection: result.selection.clone(),
                ..Default::default()
            },
            ctx,
        );
    }
    if value.get("program").is_some() && value.get("hotspots").is_none() {
        println!(
            "\nDetail commands\n  wright inspect symbols\n  wright inspect refs <NAME>\n  wright inspect cfg <RULE>\n  wright inspect callgraph\n  wright inspect cost"
        );
    }
    print_truncation(value);
    if let Some(expand) = value["expand"].as_str() {
        println!(
            "  {}",
            dim(&expand.replace(" / all: true", ""), ctx.presentation.color)
        );
    }
}

fn finding_location_span(finding: &serde_json::Value) -> Option<serde_json::Value> {
    if let Some(span) = finding.get("span").filter(|v| v.is_object()) {
        return Some(span.clone());
    }
    let mut parts = finding["location"].as_str()?.rsplitn(3, ':');
    let col = parts.next()?.parse::<u64>().ok()?;
    let line = parts.next()?.parse::<u64>().ok()?;
    let path = parts.next()?;
    Some(serde_json::json!({"path":path,"start":{"line":line,"col":col}}))
}

fn print_truncation(value: &serde_json::Value) {
    for (key, note) in value["truncation"].as_object().into_iter().flatten() {
        println!(
            "  {key}: showing {} of {}; --all to see every item",
            note["shown"], note["total"]
        );
    }
}

fn render_essential_analyze(value: &serde_json::Value, ctx: &RenderContext<'_>) {
    let p = &value["program"];
    println!(
        "\nProgram overview\n  {} file(s), {} rule(s), {} global variable(s), {} player variable(s), {} subroutine(s), {} persistent object(s)",
        count(p, "files"),
        count(p, "rules"),
        count(p, "globalVariables"),
        count(p, "playerVariables"),
        count(p, "subroutines"),
        count(&value["counts"], "persistentObjects")
    );
    println!("  evidence: [static] parsed program inventory");
    let cost = &value["cost"];
    println!("\nWorkshop cost");
    if let Some(elements) = cost["elementCount"].as_u64() {
        println!(
            "  {elements} element(s) [exact] — canonical element count; structural size, not runtime cost"
        );
    } else {
        println!(
            "  element count unavailable: {}",
            cost["unavailableReason"]
                .as_str()
                .unwrap_or("counter does not model this program")
        );
    }
    println!(
        "  {} action(s), {} condition(s), {} wait(s) [static]",
        count(&cost["counts"], "actions"),
        count(&cost["counts"], "conditions"),
        count(&cost["counts"], "waits")
    );
    let element_total = cost["elementCount"].as_u64();
    println!(
        "\nHotspots\n  {}",
        if element_total.is_some() {
            "ranked by Workshop element count [exact counts; the ranking itself is a heuristic]"
        } else {
            "ranked by control-flow size [heuristic: blocks + edges]"
        }
    );
    for rule in value["hotspots"].as_array().into_iter().flatten() {
        let flow = &rule["controlFlow"];
        let (blocks, edges) = (count(flow, "blocks"), count(flow, "edges"));
        let risks = count(rule, "risks");
        let location = location_suffix(rule.get("span"));
        match rule["elements"].as_u64() {
            Some(elements) => {
                let share = element_total
                    .filter(|total| *total > 0)
                    .map(|total| format!(" ({:.1}%)", elements as f64 * 100.0 / total as f64))
                    .unwrap_or_default();
                println!(
                    "    \"{}\": {elements} element(s){share}, {blocks} block(s), {edges} edge(s), {risks} risk(s){location}",
                    rule_name(rule),
                );
            }
            None => println!(
                "    \"{}\": {blocks} block(s), {edges} edge(s), {risks} risk(s){location}",
                rule_name(rule),
            ),
        }
    }
    let c = &value["complexity"];
    println!(
        "\nComplexity\n  {} block(s), {} edge(s), {} loop block(s), {} wait block(s) [static]",
        count(c, "blocks"),
        count(c, "edges"),
        count(c, "loopBlocks"),
        count(c, "waitBlocks")
    );
    if value["conditionTrees"]
        .as_array()
        .is_some_and(|trees| !trees.is_empty())
    {
        println!("  top condition tree(s) by element count [exact]");
    }
    for tree in value["conditionTrees"].as_array().into_iter().flatten() {
        println!(
            "    \"{}\" condition {}: {} element(s){}",
            tree_rule(tree),
            count(tree, "index") + 1,
            count(tree, "elements"),
            location_suffix(tree.get("span"))
        );
    }
    println!(
        "\nPerformance and stability risks\n  heuristic/static indicators — not measured runtime cost or behavior"
    );
    let risks = value["risks"].as_array().map_or(&[][..], Vec::as_slice);
    for risk in risks {
        let marks = ctx.presentation.section_marks();
        println!(
            "{}{}[{}] (evidence: {}): {}",
            marks.head,
            risk["severity"].as_str().unwrap_or("info"),
            risk["code"].as_str().unwrap_or("finding"),
            risk["evidence"].as_str().unwrap_or("static"),
            risk["message"].as_str().unwrap_or_default()
        );
        if let Some(span) = finding_location_span(risk) {
            print_location(&span, &format!("{}  ", marks.pad));
        }
    }
    if count(&value["persistentObjects"], "total") > 0 {
        println!(
            "  {}",
            persistent_object_summary(&value["persistentObjects"])
        );
    }
    if value["risks"].as_array().is_some_and(Vec::is_empty)
        && count(&value["persistentObjects"], "total") == 0
    {
        println!("  none");
    }
    if let Some(withheld) = value["selection"]["withheld"].as_u64().filter(|n| *n > 0) {
        println!("  ... {withheld} finding(s) withheld (--max)");
    }
    println!(
        "\nState and coupling\n  Top variables (heuristic ranking: rules touched, then reads + writes)"
    );
    for variable in value["variables"].as_array().into_iter().flatten() {
        let u = &variable["usage"];
        println!(
            "    {} {}: {} rule(s), {} read(s), {} write(s) [static]",
            variable["kind"].as_str().unwrap_or("variable"),
            variable["name"].as_str().unwrap_or("<unnamed>"),
            count(u, "rules"),
            count(u, "reads"),
            count(u, "writes")
        );
    }
}

fn render_finding_group(group: &[serde_json::Value], ctx: &RenderContext<'_>, marks: &Marks) {
    let first = &group[0];
    let code = first["code"].as_str().unwrap_or("finding");
    let msg = first["message"].as_str().unwrap_or_default();
    let sev = finding_severity_label(first, ctx.presentation.color);
    if group.len() == 1 {
        println!("{}{sev}[{code}]: {msg}", marks.head);
        if let Some(span) = first.get("span").filter(|span| span.is_object()) {
            render_finding_location(span, ctx, &marks.pad, true);
        }
    } else {
        println!(
            "{}{sev}[{code}] ({} findings): {msg}",
            marks.head,
            group.len()
        );
        let located = renderable_group_spans(group);
        for span in located.iter() {
            render_finding_location(span, ctx, &marks.pad, false);
        }
    }
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
        "{}{}",
        marks.pad,
        dim(&format!("= {}", notes.join(" · ")), ctx.presentation.color)
    );
}

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

fn unmapped_positions_note(positions: &[(String, u64, u64)]) -> Option<String> {
    let first_path = &positions.first()?.0;
    let shared = positions.iter().all(|(path, ..)| path == first_path);
    let mut text = String::new();
    for (index, (path, line, col)) in positions.iter().enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        if shared {
            text.push_str(&format!("{line}:{col}"));
        } else {
            text.push_str(&format!("{path}:{line}:{col}"));
        }
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

pub(crate) fn display_location_path(path: &str) -> String {
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
            gated: false,
            result: LintResult {
                findings,
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
    fn gate_reads_the_essential_projection_counts() {
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
                all: false,
                width: MAX_WIDTH,
            },
            elapsed: Duration::from_millis(42),
            source_base: None,
            subject: None,
        };
        assert_eq!(
            lint_footer_parts(2, 1, &ctx(true)),
            vec![
                "2 file(s) affected".to_string(),
                "1 rule evaluation(s) skipped".to_string(),
                "42 ms".to_string()
            ]
        );
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

        let pseudo = serde_json::json!({"path": "<stdin>", "start": {"line": 1, "col": 1}});
        assert!(finding_location(&pseudo, Some(&root)).is_none());
        let positions = unmapped_positions(&[serde_json::json!({"span": pseudo})]);
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
    fn unmapped_positions_note_preserves_every_selected_position() {
        let finding = |line: u64| serde_json::json!({"span": {"path": "<stdin>", "start": {"line": line, "col": 1}}});
        let group: Vec<_> = (1..=12).map(finding).collect();
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
