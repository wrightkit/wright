//! The plain renderer (ADR-0030 decisions 1, 2, 5–7; #793): Wright's
//! agent-facing text surface as line records.
//!
//! Every line is `kind: key=value …`; the verdict line keeps its
//! `STATUS command` lead. A location is one `path:line:col` token, facts are
//! fields rather than sentences, a capped list is followed by a `truncated:`
//! record giving shown, total, and the exact expansion command, and
//! follow-up commands print as `next:` records. Records carry no
//! decoration, no `(s)` plural suffixes, and no wall-clock values; a failed
//! computation reports `unavailable` with its reason.
//!
//! Envelope diagnostics keep the text-mode stream contract — `diagnostic:`
//! records go to stderr while the result records stay on stdout — so a
//! `diagnostic:` line is never interleaved with the report it belongs to.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;
use wright_driver::result::{
    CallGraphResult, CfgResult, CheckResult, CompileResult, ConvertResult, CostResult, Envelope,
    EssentialResult, LintResult, RefsResult, RulesResult, SymbolsResult,
};
use wright_driver::{Diagnostic, SelectionOutcome, Status};

use crate::present::{
    RenderContext, ResultPresentation, count, display_location_path, finding_severity_rank,
    inline_location, is_real_source_path, query_items, render_node, rule_name, severity_rank,
    summary_status,
};
/// One buffered output line tagged with its stream, so diagnostics ride
/// stderr while the report body stays on stdout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Stream {
    Out,
    Err,
}

pub(crate) type Line = (Stream, String);

/// Emit the envelope as plain line records.
pub(crate) fn render<T: serde::Serialize + ResultPresentation + PlainReport>(
    envelope: &Envelope<T>,
    ctx: &RenderContext<'_>,
) {
    if envelope.result.is_artifact_stream() {
        // An artifact stream is the payload itself: it prints to stdout
        // unchanged while its diagnostics still become `diagnostic:` records.
        let mut err: Vec<Line> = Vec::new();
        diagnostic_records(envelope, ctx, &mut err);
        emit(&mut err);
        envelope.result.render_body(ctx);
        return;
    }
    let (out, err) = render_lines(envelope, ctx);
    for line in out {
        println!("{line}");
    }
    for line in err {
        eprintln!("{line}");
    }
}

/// The plain report as ordered `(stdout, stderr)` record strings before
/// printing — the GitHub Actions log group renders the same lines (#794).
/// Artifact streams are excluded: they own stdout byte-for-byte.
pub(crate) fn render_lines<T: serde::Serialize + ResultPresentation + PlainReport>(
    envelope: &Envelope<T>,
    ctx: &RenderContext<'_>,
) -> (Vec<String>, Vec<String>) {
    let mut out: Vec<Line> = Vec::new();
    let mut err: Vec<Line> = Vec::new();
    out.push((Stream::Out, verdict_line(envelope, ctx)));
    diagnostic_records(envelope, ctx, &mut err);
    // A gated result is a raised verdict, not a failed command: its real
    // result body still renders — the findings are the reason it failed
    // (ADR-0030 decision 13, matching render_report).
    if !envelope.ok && !envelope.gated {
        // A failed command reports its diagnostics, not the default
        // result's headline — a default payload would masquerade as a real
        // answer (#446). The headline is the diagnostic severity counts.
        if envelope.diagnostics.is_empty() {
            err.push((
                Stream::Err,
                format!(
                    "error: {}",
                    field("message", &format!("{} failed", envelope.command))
                ),
            ));
        } else {
            let mut counts = [0usize; 3];
            for diagnostic in &envelope.diagnostics {
                counts[severity_rank(diagnostic.severity) as usize] += 1;
            }
            let mut fields = severity_fields(counts[0], counts[1], counts[2]);
            // The affected-file count is envelope data, not result data —
            // a failure reports it exactly like a clean `check` does.
            let affected: BTreeSet<String> = envelope
                .diagnostics
                .iter()
                .filter_map(|diagnostic| diagnostic.span.as_ref())
                .filter(|span| is_real_source_path(&span.path))
                .map(|span| display_location_path(&span.path))
                .collect();
            if !affected.is_empty() {
                fields.push(format!("affected={}", affected.len()));
            }
            out.push((Stream::Out, format!("summary: {}", fields.join(" "))));
        }
    } else {
        envelope.result.plain_report(envelope, ctx, &mut out);
        comparison_records(envelope.comparison.as_ref(), &mut out);
    }
    for next in next_commands(envelope) {
        out.push((Stream::Out, format!("next: {next}")));
    }
    // The verdict and report lead; the diagnostic records annotate them.
    (
        out.into_iter().map(|(_, line)| line).collect(),
        err.into_iter().map(|(_, line)| line).collect(),
    )
}

/// The verdict word shares the renderers' tag set (ADR-0030 decision 4):
/// FAIL is the `--fail-on` gate's raised verdict, ERROR a genuine failure,
/// USAGE a flag error (exit 2), UNSUPPORTED a recognized-but-unsupported
/// operation (exit 3). A clean query command (`rules`, `symbols`, `refs`,
/// `cfg`, `callgraph`, `cost`, `inspect`, `lookup`) shows its own name
/// instead of a vacuous PASS.
fn verdict_word<T: serde::Serialize + ResultPresentation>(envelope: &Envelope<T>) -> String {
    if envelope.exit != 0 || !envelope.ok {
        if envelope.gated {
            return "FAIL".to_string();
        }
        return match envelope.exit {
            2 => "USAGE",
            3 => "UNSUPPORTED",
            _ => "ERROR",
        }
        .to_string();
    }
    match summary_status(envelope) {
        "WARN" => "WARN".to_string(),
        _ => match envelope.command.as_str() {
            "rules" | "symbols" | "refs" | "cfg" | "callgraph" | "cost" | "inspect" | "lookup" => {
                envelope.command.to_uppercase()
            }
            _ => "PASS".to_string(),
        },
    }
}

/// `<WORD> <command>` plus the input context fields (`root=`, and `kind=`/
/// `locale=` when the result carries a program origin).
fn verdict_line<T: serde::Serialize + ResultPresentation + PlainReport>(
    envelope: &Envelope<T>,
    ctx: &RenderContext<'_>,
) -> String {
    let mut line = format!("{} {}", verdict_word(envelope), envelope.command);
    if let Some(root) = ctx.source_base {
        line.push(' ');
        line.push_str(&field("root", &display_root(root)));
    }
    for (key, value) in envelope.result.origin_fields() {
        line.push(' ');
        line.push_str(&field(key, &value));
    }
    line
}

/// The include root spelled for an agent: `.` when it is the working
/// directory, else the display path (deterministic under a fixed cwd).
pub(crate) fn display_root(root: &Path) -> String {
    if let Ok(cwd) = std::env::current_dir()
        && root == cwd
    {
        return ".".to_string();
    }
    display_location_path(&root.display().to_string())
}

/// `diagnostic:` records on stderr, errors first like every other renderer,
/// capped at the default page unless `--all` lifted it. `truncated:` and
/// `selected:` records state what the cap or the `--max` selection withheld.
fn diagnostic_records<T: serde::Serialize>(
    envelope: &Envelope<T>,
    ctx: &RenderContext<'_>,
    err: &mut Vec<Line>,
) {
    let mut ordered: Vec<&Diagnostic> = envelope.diagnostics.iter().collect();
    ordered.sort_by_key(|diagnostic| severity_rank(diagnostic.severity));
    let total = ordered.len();
    let bounded = !matches!(envelope.command.as_str(), "compile" | "convert");
    let limit = if ctx.presentation.all || !bounded {
        usize::MAX
    } else {
        20
    };
    for diagnostic in ordered.iter().take(limit) {
        err.push((Stream::Err, diagnostic_record(diagnostic)));
    }
    if bounded && !ctx.presentation.all && total > 20 {
        err.push((
            Stream::Err,
            truncated_record(&[("diagnostics", 20, total)], &expand_all(envelope)),
        ));
    }
    if let Some(selection) = envelope
        .selection
        .as_ref()
        .filter(|selection| selection.total > 0)
    {
        let mut fields = vec![format!("diagnostics={total}/{}", selection.total)];
        if let Some(by_code) = by_code_field(&selection.by_code) {
            fields.push(by_code);
        }
        if selection.withheld > 0 {
            fields.push(format!("withheld={}", selection.withheld));
            fields.push(field(
                "expand",
                &format!("wright {} --max {}", envelope.command, selection.total),
            ));
        }
        err.push((Stream::Err, format!("selected: {}", fields.join(" "))));
    }
}

/// One `diagnostic:` record: severity and code lead, then the location,
/// stage and origin metadata as fields. A pseudo-path stays a verbatim
/// `at=<stdin>:l:c` token — records never claim a file that does not exist.
fn diagnostic_record(diagnostic: &Diagnostic) -> String {
    let mut items = vec![
        diagnostic.severity.as_str().to_string(),
        diagnostic.code.clone(),
    ];
    if let Some(span) = &diagnostic.span {
        items.push(format!(
            "at={}:{}:{}",
            display_location_path(&span.path),
            span.start.line,
            span.start.col
        ));
    }
    items.push(field("stage", diagnostic.stage.as_str()));
    if let Some(source) = &diagnostic.source {
        if source.kind != "workshop" {
            items.push(field("source", &source.kind));
        }
        if let Some(locale) = source
            .locale
            .as_deref()
            .filter(|locale| !locale.eq_ignore_ascii_case("en-US"))
        {
            items.push(field("locale", locale));
        }
    }
    if let Some(status) = diagnostic.status {
        items.push(field("status", status_label(status)));
    }
    items.push(field("message", &diagnostic.message));
    format!("diagnostic: {}", items.join(" "))
}

/// The wire spelling of a support classification.
fn status_label(status: Status) -> &'static str {
    match status {
        Status::Supported => "supported",
        Status::Partial => "partial",
        Status::Unsupported => "unsupported",
    }
}

/// Follow-up commands as `next:` records (decision 2): the result's
/// driver-provided list (#792) when it carries one, else the command's
/// known follow-ups; the rejected-name lookup hints are additive.
fn next_commands<T: serde::Serialize>(envelope: &Envelope<T>) -> Vec<String> {
    let mut commands = Vec::new();
    let result = serde_json::to_value(&envelope.result).unwrap_or_default();
    if let Some(next) = result.get("next").and_then(Value::as_array) {
        commands.extend(next.iter().filter_map(Value::as_str).map(str::to_string));
    }
    match envelope.command.as_str() {
        "inspect" | "analyze" if commands.is_empty() => {
            for cmd in [
                "wright inspect symbols",
                "wright inspect refs <NAME>",
                "wright inspect cfg <RULE>",
                "wright inspect callgraph",
                "wright inspect cost",
            ] {
                commands.push(cmd.to_string());
            }
        }
        "check" | "compile" => {
            // ADR-0021 decision 6, in record form: a rejected-name
            // diagnostic is followed by the lookup that resolves it.
            // `unknown-language` is `WorkshopError::NotDetected` — there is
            // nothing `wright lookup` could resolve for it.
            let mut seen = BTreeSet::new();
            for diagnostic in &envelope.diagnostics {
                let rejected_name = diagnostic
                    .code
                    .split('.')
                    .any(|s| s.starts_with("unknown-") && s != "unknown-language");
                if !rejected_name {
                    continue;
                }
                let source = diagnostic.source.as_ref();
                let hint = if source.is_some_and(|s| s.kind == "opy") {
                    "wright lookup --language opy".to_string()
                } else if let Some(locale) = source
                    .and_then(|s| s.locale.as_deref())
                    .filter(|locale| !locale.eq_ignore_ascii_case("en-US"))
                {
                    format!("wright lookup --locale {locale}")
                } else {
                    "wright lookup".to_string()
                };
                if seen.insert(hint.clone()) {
                    commands.push(hint);
                }
            }
        }
        _ => {}
    }
    commands
}

/// The run-cache comparison (ADR-0027, #723) as `comparison:`/`change:`
/// records — absent entirely when no reference was produced.
fn comparison_records(comparison: Option<&Value>, out: &mut Vec<Line>) {
    let Some(comparison) = comparison.filter(|c| c.is_object()) else {
        return;
    };
    let delta = comparison["totalDelta"].as_i64().unwrap_or(0);
    let sign = if delta > 0 { "+" } else { "" };
    let reference = comparison["reference"].as_str().unwrap_or("previous");
    out.push((
        Stream::Out,
        format!(
            "comparison: {} delta={sign}{delta} previousTotal={}",
            field("reference", reference),
            comparison["previousTotal"].as_u64().unwrap_or(0)
        ),
    ));
    let mut changes = Vec::new();
    for change in comparison["changed"].as_array().into_iter().flatten() {
        changes.push(format!(
            "change: {} before={} after={}",
            field("rule", change["name"].as_str().unwrap_or("?")),
            change["before"],
            change["after"]
        ));
    }
    for name in comparison["added"].as_array().into_iter().flatten() {
        changes.push(format!(
            "change: {} added",
            field("rule", name.as_str().unwrap_or("?"))
        ));
    }
    for name in comparison["removed"].as_array().into_iter().flatten() {
        changes.push(format!(
            "change: {} removed",
            field("rule", name.as_str().unwrap_or("?"))
        ));
    }
    let total = changes.len();
    for change in changes.iter().take(5) {
        out.push((Stream::Out, change.clone()));
    }
    if total > 5 {
        out.push((Stream::Out, format!("truncated: changed=5/{total}")));
    }
}

fn emit(lines: &mut Vec<Line>) {
    for (stream, text) in lines.drain(..) {
        match stream {
            Stream::Out => println!("{text}"),
            Stream::Err => eprintln!("{text}"),
        }
    }
}

/// `key=value` with the value quoted only when it needs it: a bare token
/// carries no whitespace, `"`, `\`, or `=`.
pub(crate) fn field(key: &str, value: &str) -> String {
    format!("{key}={}", scalar(value))
}

fn scalar(value: &str) -> String {
    if !value.is_empty()
        && !value
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '"' | '\\' | '='))
    {
        return value.to_string();
    }
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for c in value.chars() {
        match c {
            '"' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '\n' => quoted.push_str("\\n"),
            '\t' => quoted.push_str("\\t"),
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

/// `truncated: <list>=<shown>/<total> … expand="<command>"` — the record a
/// capped list is followed by.
fn truncated_record(lists: &[(&str, usize, usize)], expand: &str) -> String {
    let mut parts: Vec<String> = lists
        .iter()
        .map(|(name, shown, total)| format!("{name}={shown}/{total}"))
        .collect();
    if !expand.is_empty() {
        parts.push(field("expand", expand));
    }
    format!("truncated: {}", parts.join(" "))
}

/// The default expansion command for a capped list of this command's
/// output: `--all` lifts the default cap; a `--max` selection is raised by
/// `selected:`'s own expand field.
fn expand_all<T: serde::Serialize>(envelope: &Envelope<T>) -> String {
    format!("wright {} --all", cli_command(envelope))
}

/// The CLI spelling of an envelope command name (`symbols` runs as
/// `wright inspect symbols`).
fn cli_command<T: serde::Serialize>(envelope: &Envelope<T>) -> String {
    match envelope.command.as_str() {
        name @ ("rules" | "symbols" | "refs" | "cfg" | "callgraph" | "cost") => {
            format!("inspect {name}")
        }
        name => name.to_string(),
    }
}

/// `at=path:line:col` for a JSON span, or `None` without a path.
fn at_field(span: &Value) -> Option<String> {
    inline_location(span).map(|location| format!("at={location}"))
}

/// Severity counts as `errors=E warnings=W notes=N` fields — `notes` is the
/// record name for info-severity items (the JSON member stays `info`).
fn severity_fields(error: usize, warning: usize, info: usize) -> Vec<String> {
    vec![
        format!("errors={error}"),
        format!("warnings={warning}"),
        format!("notes={info}"),
    ]
}

/// One `finding:` record per reported finding — severity and code lead,
/// the `repeated` member's facts ride as `count=`/`spanKind=`/`expression=`
/// fields around the `at=` location, every other scalar member becomes a
/// field verbatim (`evidence`, `boundedness`, and any structured fields the
/// driver adds), and the message closes the line.
fn finding_record(finding: &Value) -> String {
    let severity = finding["severity"].as_str().unwrap_or("warning");
    let code = finding["code"].as_str().unwrap_or("finding");
    let repeated = finding.get("repeated").filter(|r| r.is_object());
    let mut items = vec![severity.to_string(), code.to_string()];
    if let Some(count) = repeated.and_then(|r| r["count"].as_u64()) {
        items.push(format!("count={count}"));
    }
    if let Some(span) = finding.get("span").filter(|span| span.is_object()) {
        if let Some(at) = at_field(span) {
            items.push(at);
        }
    } else if let Some(location) = finding["location"].as_str() {
        items.push(format!("at={location}"));
    }
    if let Some(repeated) = repeated {
        if let Some(span_kind) = repeated["spanKind"].as_str() {
            items.push(field("spanKind", span_kind));
        }
        if let Some(expression) = repeated["expression"].as_str() {
            items.push(field("expression", expression));
        }
    }
    // Structured members become fields verbatim so driver additions ride
    // the record without a renderer change. Well-known members the record
    // already carries (severity, code, message, span, location, repeated)
    // are skipped.
    const CARRIED: &[&str] = &[
        "code", "severity", "message", "span", "location", "fix", "repeated",
    ];
    if let Some(object) = finding.as_object() {
        for (key, value) in object {
            if CARRIED.contains(&key.as_str()) {
                continue;
            }
            match value {
                Value::String(text) => items.push(field(key, text)),
                Value::Number(_) | Value::Bool(_) => items.push(format!("{key}={value}")),
                _ => {}
            }
        }
    }
    if let Some(message) = finding["message"].as_str() {
        items.push(field("message", message));
    }
    format!("finding: {}", items.join(" "))
}

/// The `byCode=` field for a `selected:`/`truncated:` record: per-code
/// counts over the complete coded set, so a cap cannot hide a code's real
/// count (#791). `None` when the set carries no codes.
fn by_code_field(counts: &std::collections::BTreeMap<String, usize>) -> Option<String> {
    if counts.is_empty() {
        return None;
    }
    let joined = counts
        .iter()
        .map(|(code, n)| format!("{code}:{n}"))
        .collect::<Vec<_>>()
        .join(",");
    Some(format!("byCode={joined}"))
}

/// `selected:` record for a finding selection applied inside a result
/// (`--max` withholds; `--severity`/`--rule-id`/`--file` narrow). The
/// expansion repeats the command with `--max <total>` to lift the withhold.
fn selected_record(noun: &str, shown: usize, selection: &SelectionOutcome, command: &str) -> Line {
    let mut fields = vec![format!("{noun}={shown}/{}", selection.total)];
    if let Some(by_code) = by_code_field(&selection.by_code) {
        fields.push(by_code);
    }
    if selection.withheld > 0 {
        fields.push(format!("withheld={}", selection.withheld));
        fields.push(field(
            "expand",
            &format!("wright {command} --max {}", selection.total),
        ));
    }
    (Stream::Out, format!("selected: {}", fields.join(" ")))
}

/// `truncated:` records for a payload's `truncation` notes: one per capped
/// list, named by the payload's own key, each giving the expansion command.
fn truncation_records(value: &Value, command: &str, out: &mut Vec<Line>) {
    for key in truncation_keys(value) {
        out.push((Stream::Out, truncation_line(value, key, command)));
    }
}

/// The capped-list keys a payload's `truncation` member declares.
fn truncation_keys(value: &Value) -> Vec<&str> {
    value["truncation"]
        .as_object()
        .map(|notes| notes.keys().map(String::as_str).collect())
        .unwrap_or_default()
}

/// The `truncated:` record for one capped list of `command`'s output. A
/// coded list's `byCode` counts describe the full set the cap hid (#791).
fn truncation_line(value: &Value, key: &str, command: &str) -> String {
    let note = &value["truncation"][key];
    let mut parts = vec![format!(
        "{key}={}/{}",
        note["shown"].as_u64().unwrap_or(0),
        note["total"].as_u64().unwrap_or(0)
    )];
    if let Some(by_code) = note.get("byCode").and_then(Value::as_object) {
        let counts = by_code
            .iter()
            .filter_map(|(code, n)| n.as_u64().map(|n| (code.clone(), n as usize)))
            .collect();
        if let Some(field) = by_code_field(&counts) {
            parts.push(field);
        }
    }
    parts.push(field("expand", &format!("wright {command} --all")));
    format!("truncated: {}", parts.join(" "))
}

/// The `program` inventory as a `program:` record — present only when the
/// payload carries one.
fn program_record(program: &Value, persistent_objects: Option<u64>) -> Option<Line> {
    if !program.is_object() {
        return None;
    }
    let mut fields = vec![
        format!("files={}", count(program, "files")),
        format!("rules={}", count(program, "rules")),
        format!("globalVariables={}", count(program, "globalVariables")),
        format!("playerVariables={}", count(program, "playerVariables")),
        format!("subroutines={}", count(program, "subroutines")),
    ];
    if let Some(objects) = persistent_objects {
        fields.push(format!("persistentObjects={objects}"));
    }
    Some((Stream::Out, format!("program: {}", fields.join(" "))))
}

/// `cost:` record for the canonical element count — `unavailable` with its
/// reason under the payload's own member name (`elementCountUnavailable`,
/// `unavailableReason`) when the computation failed, and the client budget
/// either way (ADR-0030 decision 3: a known limit is reported even when the
/// count is not).
fn cost_record(elements: Option<u64>, reason: Option<(&str, &str)>, extra: Vec<String>) -> Line {
    let mut fields = vec![match elements {
        Some(elements) => format!("elements={elements}"),
        None => "elements=unavailable".to_string(),
    }];
    fields.push(format!(
        "budget={}",
        wright_driver::WORKSHOP_CLIENT_ELEMENT_LIMIT
    ));
    fields.extend(extra);
    if let Some((key, reason)) = reason {
        fields.push(field(key, reason));
    }
    (Stream::Out, format!("cost: {}", fields.join(" ")))
}

/// The per-command `summary:` record plus the result's section records.
/// `envelope` is in scope because a headline may fold in envelope facts
/// (the diagnostic counts on `check`).
pub(crate) trait PlainReport {
    /// `kind=`/`locale=` fields for the verdict line when the result
    /// carries a program origin.
    fn origin_fields(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }
    /// Append this result's `summary:` and section records.
    fn plain_report(&self, envelope: &Envelope<Self>, ctx: &RenderContext<'_>, out: &mut Vec<Line>)
    where
        Self: serde::Serialize + Sized;
}

/// `origin` member of a `program` object, as verdict fields.
fn origin_from_program(program: &Value) -> Vec<(&'static str, String)> {
    let origin = &program["origin"];
    let mut fields = Vec::new();
    if let Some(kind) = origin["kind"].as_str() {
        fields.push(("kind", kind.to_string()));
    }
    if let Some(locale) = origin["locale"].as_str() {
        fields.push(("locale", locale.to_string()));
    }
    fields
}

impl PlainReport for CheckResult {
    fn origin_fields(&self) -> Vec<(&'static str, String)> {
        self.program
            .as_ref()
            .map(|program| origin_from_program(program))
            .unwrap_or_default()
    }
    fn plain_report(
        &self,
        envelope: &Envelope<Self>,
        _ctx: &RenderContext<'_>,
        out: &mut Vec<Line>,
    ) {
        let mut fields = match &envelope.selection {
            // Under a selection the per-severity breakdown of the full set
            // is unknown; the true total is the headline (#430).
            Some(selection) => vec![format!("diagnostics={}", selection.total)],
            None => {
                let mut counts = [0usize; 3];
                for diagnostic in &envelope.diagnostics {
                    counts[severity_rank(diagnostic.severity) as usize] += 1;
                }
                severity_fields(counts[0], counts[1], counts[2])
            }
        };
        let affected: BTreeSet<String> = envelope
            .diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.span.as_ref())
            .filter(|span| is_real_source_path(&span.path))
            .map(|span| display_location_path(&span.path))
            .collect();
        if !affected.is_empty() {
            fields.push(format!("affected={}", affected.len()));
        }
        out.push((Stream::Out, format!("summary: {}", fields.join(" "))));
        // `check` carries the same program summary `analyze` reports (#792).
        if let Some(program) = self
            .program
            .as_ref()
            .and_then(|program| program_record(program, None))
        {
            out.push(program);
        }
    }
}

impl PlainReport for CompileResult {
    fn origin_fields(&self) -> Vec<(&'static str, String)> {
        self.output
            .as_ref()
            .and_then(|output| {
                (!output.locale.is_empty()).then(|| ("locale", output.locale.clone()))
            })
            .into_iter()
            .collect()
    }
    fn plain_report(
        &self,
        envelope: &Envelope<Self>,
        _ctx: &RenderContext<'_>,
        out: &mut Vec<Line>,
    ) {
        let mut counts = [0usize; 3];
        for diagnostic in &envelope.diagnostics {
            counts[severity_rank(diagnostic.severity) as usize] += 1;
        }
        out.push((
            Stream::Out,
            format!(
                "summary: {}",
                severity_fields(counts[0], counts[1], counts[2]).join(" ")
            ),
        ));
        if let Some(output) = self.output.as_ref().filter(|o| o.written_to != "stdout") {
            let mut fields = vec![
                field("path", &output.written_to),
                field("sha256", &output.sha256),
            ];
            if !output.locale.is_empty() {
                fields.push(field("locale", &output.locale));
            }
            out.push((Stream::Out, format!("output: {}", fields.join(" "))));
        }
    }
}

impl PlainReport for ConvertResult {
    fn plain_report(
        &self,
        _envelope: &Envelope<Self>,
        _ctx: &RenderContext<'_>,
        _out: &mut Vec<Line>,
    ) {
    }
}

/// Finding-severity totals over a findings array.
fn findings_severity_counts(findings: &[Value]) -> [usize; 3] {
    let mut counts = [0usize; 3];
    for finding in findings {
        counts[finding_severity_rank(finding) as usize] += 1;
    }
    counts
}

impl PlainReport for LintResult {
    fn origin_fields(&self) -> Vec<(&'static str, String)> {
        origin_from_program(&self.program)
    }
    fn plain_report(
        &self,
        _envelope: &Envelope<Self>,
        _ctx: &RenderContext<'_>,
        out: &mut Vec<Line>,
    ) {
        let findings = self.findings.as_array().map_or(&[][..], Vec::as_slice);
        let counts = findings_severity_counts(findings);
        let total = self
            .selection
            .as_ref()
            .map_or(findings.len(), |selection| selection.total);
        let mut fields = vec![format!("findings={total}")];
        if self.selection.is_none() {
            fields.extend(severity_fields(counts[0], counts[1], counts[2]));
        }
        fields.push(format!(
            "rules={}",
            self.rules.as_array().map_or(0, Vec::len)
        ));
        let skipped = self.skipped.as_array().map_or(0, Vec::len);
        if skipped > 0 {
            fields.push(format!("skipped={skipped}"));
        }
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
            fields.push(format!("fixes={}", fixes.len()));
            fields.push(format!("applied={applied}"));
            fields.push(format!("previewed={previewed}"));
            fields.push(format!("refused={refused}"));
        }
        let affected: BTreeSet<&str> = findings
            .iter()
            .filter_map(|finding| finding.get("span"))
            .filter_map(|span| span.get("path"))
            .filter_map(Value::as_str)
            .filter(|path| is_real_source_path(path))
            .collect();
        if !affected.is_empty() {
            fields.push(format!("affected={}", affected.len()));
        }
        out.push((Stream::Out, format!("summary: {}", fields.join(" "))));

        let mut sorted = findings.to_vec();
        sorted.sort_by_key(finding_severity_rank);
        for finding in &sorted {
            out.push((Stream::Out, finding_record(finding)));
        }
        if let Some(selection) = &self.selection {
            out.push(selected_record(
                "findings",
                findings.len(),
                selection,
                "lint",
            ));
        }
        // `lint` carries the same program summary `analyze` reports (#792):
        // the `rules=` summary field counts lint rules, not program rules.
        if let Some(program) = program_record(&self.program, None) {
            out.push(program);
        }
        for fix in self.fixes.iter().flatten() {
            let status = match fix.status {
                wright_driver::result::LintFixStatus::Preview => "preview",
                wright_driver::result::LintFixStatus::Applied => "applied",
                wright_driver::result::LintFixStatus::Refused => "refused",
            };
            let mut items = vec![status.to_string(), fix.code.clone()];
            if let Some(span) = fix.span.as_ref().filter(|span| span.is_object())
                && let Some(at) = at_field(span)
            {
                items.push(at);
            }
            items.push(field("summary", &fix.summary));
            out.push((Stream::Out, format!("fix: {}", items.join(" "))));
            for preview in fix.preview.iter().flatten() {
                out.push((
                    Stream::Out,
                    format!("patch: {}", field("source", &preview.source)),
                ));
                for line in crate::present::diff_rows(&preview.original, &preview.new_text) {
                    out.push((Stream::Out, format!("patch: {line}")));
                }
            }
            for diagnostic in &fix.diagnostics {
                out.push((
                    Stream::Out,
                    format!(
                        "refusal: {} {}",
                        diagnostic.code,
                        field("message", &diagnostic.message)
                    ),
                ));
            }
        }
    }
}

/// The essential payload (`analyze`, `lint`, bare `inspect`) renders from
/// its projected `result` value.
impl PlainReport for EssentialResult {
    fn origin_fields(&self) -> Vec<(&'static str, String)> {
        origin_from_program(&self.result["program"])
    }
    fn plain_report(
        &self,
        envelope: &Envelope<Self>,
        _ctx: &RenderContext<'_>,
        out: &mut Vec<Line>,
    ) {
        let value = &self.result;
        if value.get("hotspots").is_some() {
            essential_analyze(self, out);
        } else if value.get("program").is_some() {
            essential_inspect(self, out);
        } else {
            // The lint essential payload: counts + findings.
            essential_lint(self, out);
        }
        let _ = envelope;
    }
}

/// The `lint` essential projection — `findings` entries carry a `location`
/// string instead of a span object.
fn essential_lint(result: &EssentialResult, out: &mut Vec<Line>) {
    let counts = &result.result["counts"]["findings"];
    let findings = result.result["findings"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let total = result.selection.as_ref().map_or_else(
        || counts["total"].as_u64().unwrap_or(0) as usize,
        |s| s.total,
    );
    let mut fields = vec![format!("findings={total}")];
    if result.selection.is_none() {
        fields.extend(severity_fields(
            counts["error"].as_u64().unwrap_or(0) as usize,
            counts["warning"].as_u64().unwrap_or(0) as usize,
            counts["info"].as_u64().unwrap_or(0) as usize,
        ));
    }
    // `fixes` rides the essential payload under `lint --fix` (#556) — the
    // essential lint path is the one `lint --fix` does not take, but keep
    // the projection tolerant of the member.
    let fixes = result.result["fixes"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    if !fixes.is_empty() {
        let mut applied = 0usize;
        let mut previewed = 0usize;
        let mut refused = 0usize;
        for fix in fixes {
            match fix["status"].as_str() {
                Some("applied") => applied += 1,
                Some("preview") => previewed += 1,
                _ => refused += 1,
            }
        }
        fields.push(format!("fixes={}", fixes.len()));
        fields.push(format!("applied={applied}"));
        fields.push(format!("previewed={previewed}"));
        fields.push(format!("refused={refused}"));
    }
    let affected: BTreeSet<&str> = findings
        .iter()
        .filter_map(|finding| finding["location"].as_str())
        .filter_map(|location| location.rsplitn(3, ':').nth(2))
        .filter(|path| is_real_source_path(path))
        .collect();
    if !affected.is_empty() {
        fields.push(format!("affected={}", affected.len()));
    }
    out.push((Stream::Out, format!("summary: {}", fields.join(" "))));
    for finding in findings {
        out.push((Stream::Out, finding_record(finding)));
    }
    truncation_records(&result.result, "lint", out);
    if let Some(selection) = &result.selection {
        out.push(selected_record(
            "findings",
            findings.len(),
            selection,
            "lint",
        ));
    }
    for fix in fixes {
        let mut items = vec![
            fix["status"].as_str().unwrap_or("preview").to_string(),
            fix["code"].as_str().unwrap_or("fix").to_string(),
        ];
        if let Some(location) = fix["location"].as_str() {
            items.push(format!("at={location}"));
        }
        if let Some(summary) = fix["summary"].as_str() {
            items.push(field("summary", summary));
        }
        out.push((Stream::Out, format!("fix: {}", items.join(" "))));
        for preview in fix["preview"].as_array().into_iter().flatten() {
            out.push((
                Stream::Out,
                format!(
                    "patch: {}",
                    field("source", preview["source"].as_str().unwrap_or("?"))
                ),
            ));
            for line in crate::present::diff_rows(
                preview["original"].as_str().unwrap_or_default(),
                preview["newText"].as_str().unwrap_or_default(),
            ) {
                out.push((Stream::Out, format!("patch: {line}")));
            }
        }
        for diagnostic in fix["diagnostics"].as_array().into_iter().flatten() {
            out.push((
                Stream::Out,
                format!(
                    "refusal: {} {}",
                    diagnostic["code"].as_str().unwrap_or("refusal"),
                    field(
                        "message",
                        diagnostic["message"].as_str().unwrap_or_default()
                    )
                ),
            ));
        }
    }
}

/// The bare `inspect` overview: inventory, a bounded rule preview, and the
/// detail commands (emitted by the envelope layer as `next:` records).
fn essential_inspect(result: &EssentialResult, out: &mut Vec<Line>) {
    let counts = &result.result["counts"];
    out.push((
        Stream::Out,
        format!(
            "summary: rules={} symbols={} references={}",
            counts["rules"].as_u64().unwrap_or(0),
            counts["symbols"].as_u64().unwrap_or(0),
            counts["references"].as_u64().unwrap_or(0)
        ),
    ));
    if let Some(program) = program_record(&result.result["program"], None) {
        out.push(program);
    }
    for rule in result.result["rules"].as_array().into_iter().flatten() {
        let mut items = vec![field(
            "name",
            rule["name"]
                .as_str()
                .filter(|name| !name.is_empty())
                .unwrap_or("<unnamed>"),
        )];
        if let Some(id) = rule["id"].as_u64() {
            items.push(format!("id={id}"));
        }
        out.push((Stream::Out, format!("rule: {}", items.join(" "))));
    }
    truncation_records(&result.result, "inspect", out);
}

/// The `analyze` report (ADR-0030 decision 2): the headline counts and cost
/// first, then findings (action), then the ranked reference sections.
fn essential_analyze(result: &EssentialResult, out: &mut Vec<Line>) {
    let value = &result.result;
    let risk_counts = &value["counts"]["risks"];
    let cost = &value["cost"];
    let elements = cost["elementCount"].as_u64();
    let mut fields = vec![
        format!("files={}", count(&value["program"], "files")),
        format!("rules={}", count(&value["program"], "rules")),
        format!(
            "globalVariables={}",
            count(&value["program"], "globalVariables")
        ),
        format!(
            "playerVariables={}",
            count(&value["program"], "playerVariables")
        ),
        format!("subroutines={}", count(&value["program"], "subroutines")),
    ];
    fields.extend(severity_fields(
        risk_counts["error"].as_u64().unwrap_or(0) as usize,
        risk_counts["warning"].as_u64().unwrap_or(0) as usize,
        risk_counts["info"].as_u64().unwrap_or(0) as usize,
    ));
    fields.push(match elements {
        Some(elements) => format!("cost={elements}"),
        None => "cost=unavailable".to_string(),
    });
    out.push((Stream::Out, format!("summary: {}", fields.join(" "))));

    let cost_counts = &cost["counts"];
    out.push(cost_record(
        elements,
        cost["unavailableReason"]
            .as_str()
            .map(|reason| ("unavailableReason", reason)),
        vec![
            format!("actions={}", count(cost_counts, "actions")),
            format!("conditions={}", count(cost_counts, "conditions")),
            format!("waits={}", count(cost_counts, "waits")),
        ],
    ));

    for risk in value["risks"].as_array().into_iter().flatten() {
        out.push((Stream::Out, finding_record(risk)));
    }
    if truncation_keys(value).contains(&"risks") {
        out.push((Stream::Out, truncation_line(value, "risks", "analyze")));
    }
    if let Some(selection) = &result.selection {
        out.push(selected_record(
            "findings",
            value["risks"].as_array().map_or(0, Vec::len),
            selection,
            "analyze",
        ));
    }

    let element_total = elements;
    for rule in value["hotspots"].as_array().into_iter().flatten() {
        let flow = &rule["controlFlow"];
        let mut items = vec![field("name", rule_name(rule))];
        match rule["elements"].as_u64() {
            Some(rule_elements) => {
                items.push(format!("elements={rule_elements}"));
                if let Some(total) = element_total.filter(|total| *total > 0) {
                    items.push(format!(
                        "share={:.1}",
                        rule_elements as f64 * 100.0 / total as f64
                    ));
                }
            }
            None => items.push("elements=unavailable".to_string()),
        }
        items.push(format!("blocks={}", count(flow, "blocks")));
        items.push(format!("edges={}", count(flow, "edges")));
        items.push(format!("risks={}", count(rule, "risks")));
        if let Some(at) = rule.get("span").and_then(at_field) {
            items.push(at);
        }
        out.push((Stream::Out, format!("hotspot: {}", items.join(" "))));
    }
    // A capped list is followed by its `truncated:` record — shown, total,
    // and the exact expansion command.
    if truncation_keys(value).contains(&"hotspots") {
        out.push((Stream::Out, truncation_line(value, "hotspots", "analyze")));
    }
    for tree in value["conditionTrees"].as_array().into_iter().flatten() {
        let mut items = vec![
            field(
                "rule",
                match tree["rule"].as_str() {
                    Some("") | None => "<unnamed>",
                    Some(name) => name,
                },
            ),
            format!("index={}", count(tree, "index") + 1),
            format!("elements={}", count(tree, "elements")),
        ];
        if let Some(at) = tree.get("span").and_then(at_field) {
            items.push(at);
        }
        out.push((Stream::Out, format!("tree: {}", items.join(" "))));
    }
    if truncation_keys(value).contains(&"conditionTrees") {
        out.push((
            Stream::Out,
            truncation_line(value, "conditionTrees", "analyze"),
        ));
    }
    let complexity = &value["complexity"];
    out.push((
        Stream::Out,
        format!(
            "complexity: blocks={} edges={} loopBlocks={} waitBlocks={}",
            count(complexity, "blocks"),
            count(complexity, "edges"),
            count(complexity, "loopBlocks"),
            count(complexity, "waitBlocks")
        ),
    ));
    for variable in value["variables"].as_array().into_iter().flatten() {
        let usage = &variable["usage"];
        out.push((
            Stream::Out,
            format!(
                "variable: {} {} rules={} reads={} writes={}",
                field("kind", variable["kind"].as_str().unwrap_or("variable")),
                field("name", variable["name"].as_str().unwrap_or("<unnamed>")),
                count(usage, "rules"),
                count(usage, "reads"),
                count(usage, "writes")
            ),
        ));
    }
    if truncation_keys(value).contains(&"variables") {
        out.push((Stream::Out, truncation_line(value, "variables", "analyze")));
    }
    let objects = &value["persistentObjects"];
    if count(objects, "total") > 0 {
        let kinds = objects["byKind"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(kind, n)| format!("{kind}:{n}"))
            .collect::<Vec<_>>()
            .join(",");
        let mut fields = vec![format!("total={}", count(objects, "total"))];
        if !kinds.is_empty() {
            fields.push(format!("kinds={kinds}"));
        }
        fields.push(format!(
            "withoutRetainedIdentity={}",
            count(objects, "withoutRetainedIdentity")
        ));
        fields.push(format!(
            "visibleToAllPlayers={}",
            count(objects, "visibleToAllPlayers")
        ));
        out.push((Stream::Out, format!("objects: {}", fields.join(" "))));
    }
}

impl PlainReport for RulesResult {
    fn plain_report(
        &self,
        _envelope: &Envelope<Self>,
        _ctx: &RenderContext<'_>,
        out: &mut Vec<Line>,
    ) {
        let rules = query_items(&self.0, "rules");
        out.push((Stream::Out, format!("summary: rules={}", rules.len())));
        for rule in rules {
            let mut items = vec![field("name", rule_name(rule))];
            if let Some(event) = rule["event"].as_str() {
                items.push(field("event", event));
            }
            if let Some(id) = rule["id"].as_u64() {
                items.push(format!("id={id}"));
            }
            if let Some(at) = rule.get("span").and_then(at_field) {
                items.push(at);
            }
            out.push((Stream::Out, format!("rule: {}", items.join(" "))));
            let content = &rule["content"];
            for condition in content["conditions"].as_array().into_iter().flatten() {
                out.push((
                    Stream::Out,
                    format!(
                        "condition: {} {}",
                        field("rule", rule_name(rule)),
                        field("expr", &render_node(&condition["value"]))
                    ),
                ));
            }
            for action in content["actions"].as_array().into_iter().flatten() {
                out.push((
                    Stream::Out,
                    format!(
                        "action: {} {}",
                        field("rule", rule_name(rule)),
                        field("expr", &render_node(action))
                    ),
                ));
            }
        }
        truncation_records(&self.0, "inspect rules", out);
        selected_from_payload(&self.0, "rules", rules.len(), out);
    }
}

impl PlainReport for SymbolsResult {
    fn plain_report(
        &self,
        _envelope: &Envelope<Self>,
        _ctx: &RenderContext<'_>,
        out: &mut Vec<Line>,
    ) {
        let symbols = query_items(&self.0, "symbols");
        out.push((Stream::Out, format!("summary: symbols={}", symbols.len())));
        for symbol in symbols {
            let mut items = vec![
                symbol["kind"].as_str().unwrap_or("symbol").to_string(),
                field(
                    "name",
                    symbol["name"]
                        .as_str()
                        .filter(|name| !name.is_empty())
                        .unwrap_or("<unnamed>"),
                ),
            ];
            if let Some(at) = symbol.get("span").and_then(at_field) {
                items.push(at);
            }
            out.push((Stream::Out, format!("symbol: {}", items.join(" "))));
        }
        truncation_records(&self.0, "inspect symbols", out);
        selected_from_payload(&self.0, "symbols", symbols.len(), out);
    }
}

impl PlainReport for RefsResult {
    fn plain_report(
        &self,
        _envelope: &Envelope<Self>,
        _ctx: &RenderContext<'_>,
        out: &mut Vec<Line>,
    ) {
        out.push((
            Stream::Out,
            format!(
                "summary: {} {} reads={} writes={} calls={} rules={}",
                field("symbol", self.0["symbol"].as_str().unwrap_or("<unknown>")),
                field("kind", self.0["kind"].as_str().unwrap_or("symbol")),
                count(&self.0, "reads"),
                count(&self.0, "writes"),
                count(&self.0, "calls"),
                count(&self.0, "rules"),
            ),
        ));
        let references = self.0["references"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        for reference in references {
            let mut items = vec![
                reference["kind"]
                    .as_str()
                    .unwrap_or("reference")
                    .to_string(),
            ];
            if let Some(at) = reference.get("span").and_then(at_field) {
                items.push(at);
            }
            // `owningRule` (#792) names the rule the reference belongs to —
            // the identity `inspect cfg <RULE>` consumes; the numeric `rule`
            // id is the fallback when the driver reports no owning rule.
            if let Some(name) = reference["owningRule"]["name"].as_str() {
                items.push(field("rule", name));
            } else if let Some(rule) = reference["rule"].as_u64() {
                items.push(format!("rule={rule}"));
            }
            if let Some(action) = reference["action"].as_u64() {
                items.push(format!("action={action}"));
            }
            if let Some(value) = reference.get("value").filter(|v| !v.is_null()) {
                items.push(field("value", &value.to_string()));
            }
            out.push((Stream::Out, format!("ref: {}", items.join(" "))));
        }
        truncation_records(&self.0, "inspect refs", out);
        selected_from_payload(&self.0, "references", references.len(), out);
    }
}

impl PlainReport for CfgResult {
    fn plain_report(
        &self,
        _envelope: &Envelope<Self>,
        ctx: &RenderContext<'_>,
        out: &mut Vec<Line>,
    ) {
        let blocks = self.0["blocks"].as_array().map_or(&[][..], Vec::as_slice);
        let edges: usize = blocks
            .iter()
            .map(|block| block["successors"].as_array().map_or(0, Vec::len))
            .sum();
        let loops = blocks
            .iter()
            .filter(|block| matches!(block["kind"].as_str(), Some("while" | "for")))
            .count();
        let waits = blocks
            .iter()
            .filter(|block| block["waits"].as_bool().unwrap_or(false))
            .count();
        let calls: usize = blocks
            .iter()
            .map(|block| block["calls"].as_array().map_or(0, Vec::len))
            .sum();
        let mut fields = Vec::new();
        if let Some(rule) = ctx.subject {
            fields.push(field("rule", rule));
        }
        fields.extend([
            format!("blocks={}", blocks.len()),
            format!("edges={edges}"),
            format!("loopHeaders={loops}"),
            format!("waits={waits}"),
            format!("callSites={calls}"),
            format!("entry={}", self.0["entry"].as_u64().unwrap_or(0)),
            format!("exit={}", self.0["exit"].as_u64().unwrap_or(0)),
        ]);
        out.push((Stream::Out, format!("summary: {}", fields.join(" "))));
        for block in blocks {
            let mut items = vec![
                format!("id={}", block["id"].as_u64().unwrap_or(0)),
                field("kind", block["kind"].as_str().unwrap_or("block")),
                format!(
                    "actions={}",
                    block["actions"].as_array().map_or(0, Vec::len)
                ),
            ];
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
            if !flags.is_empty() {
                items.push(format!("flags={}", flags.join(",")));
            }
            if let Some(successors) = block["successors"]
                .as_array()
                .filter(|successors| !successors.is_empty())
            {
                let successors = successors
                    .iter()
                    .map(|successor| {
                        format!(
                            "{}:{}",
                            successor["to"].as_u64().unwrap_or(0),
                            successor["kind"].as_str().unwrap_or("edge")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                items.push(format!("to={successors}"));
            }
            out.push((Stream::Out, format!("block: {}", items.join(" "))));
        }
        truncation_records(&self.0, "inspect cfg", out);
        selected_from_payload(&self.0, "blocks", blocks.len(), out);
    }
}

impl PlainReport for CallGraphResult {
    fn plain_report(
        &self,
        _envelope: &Envelope<Self>,
        _ctx: &RenderContext<'_>,
        out: &mut Vec<Line>,
    ) {
        let edges = query_items(&self.0, "edges");
        let mut fan_in: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        let mut fan_out: std::collections::BTreeMap<&str, usize> =
            std::collections::BTreeMap::new();
        for edge in edges {
            *fan_in
                .entry(edge["callee"].as_str().unwrap_or("<unnamed>"))
                .or_default() += 1;
            *fan_out
                .entry(edge["caller"].as_str().unwrap_or("<unnamed>"))
                .or_default() += 1;
        }
        out.push((
            Stream::Out,
            format!(
                "summary: edges={} callers={} callees={}",
                edges.len(),
                fan_out.len(),
                fan_in.len()
            ),
        ));
        for (name, n) in notable(&fan_in) {
            out.push((
                Stream::Out,
                format!("fan-in: {} callers={n}", field("subroutine", name)),
            ));
        }
        for (name, n) in notable(&fan_out) {
            out.push((
                Stream::Out,
                format!("fan-out: {} calls={n}", field("rule", name)),
            ));
        }
        for edge in edges {
            out.push((
                Stream::Out,
                format!(
                    "edge: {} {}",
                    field("caller", edge["caller"].as_str().unwrap_or("<unnamed>")),
                    field("callee", edge["callee"].as_str().unwrap_or("<unnamed>"))
                ),
            ));
        }
        truncation_records(&self.0, "inspect callgraph", out);
        selected_from_payload(&self.0, "edges", edges.len(), out);
    }
}

/// Call-graph entries with a count above one, ranked by count then name.
fn notable<'a>(counts: &std::collections::BTreeMap<&'a str, usize>) -> Vec<(&'a str, usize)> {
    let mut ranked: Vec<(&'a str, usize)> = counts.iter().map(|(name, n)| (*name, *n)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    ranked.retain(|(_, n)| *n > 1);
    ranked
}

/// `selected:` record for a query payload's `selection` member (the
/// `{items, selection}` wrap a `--max` or filter request produces).
fn selected_from_payload(value: &Value, noun: &str, shown: usize, out: &mut Vec<Line>) {
    let selection = &value["selection"];
    let total = selection["total"].as_u64();
    let withheld = selection["withheld"].as_u64().unwrap_or(0);
    let Some(total) = total else { return };
    let mut fields = vec![format!("{noun}={shown}/{total}")];
    if let Some(by_code) = selection.get("byCode").and_then(Value::as_object) {
        let counts = by_code
            .iter()
            .filter_map(|(code, n)| n.as_u64().map(|n| (code.clone(), n as usize)))
            .collect();
        if let Some(field) = by_code_field(&counts) {
            fields.push(field);
        }
    }
    if withheld > 0 {
        fields.push(format!("withheld={withheld}"));
    }
    out.push((Stream::Out, format!("selected: {}", fields.join(" "))));
}

impl PlainReport for CostResult {
    fn plain_report(
        &self,
        _envelope: &Envelope<Self>,
        _ctx: &RenderContext<'_>,
        out: &mut Vec<Line>,
    ) {
        let result = &self.result;
        // The `--rule` drill-down (#724): identity, costliest statements,
        // and per-name own-cost shares.
        if let Some(rule) = result.get("rule").filter(|rule| rule.is_object()) {
            let mut fields = vec![
                field("rule", rule_name(rule)),
                format!("elements={}", count(rule, "count")),
            ];
            if let Some(at) = rule.get("span").and_then(at_field) {
                fields.push(at);
            }
            out.push((Stream::Out, format!("summary: {}", fields.join(" "))));
            for statement in result["statements"].as_array().into_iter().flatten() {
                let mut items = vec![
                    field("name", statement["name"].as_str().unwrap_or("?")),
                    format!("elements={}", count(statement, "count")),
                ];
                if let Some(at) = statement.get("span").and_then(at_field) {
                    items.push(at);
                }
                out.push((Stream::Out, format!("statement: {}", items.join(" "))));
            }
            for entry in result["byNode"].as_array().into_iter().flatten() {
                let occurrences = entry["occurrences"].as_u64().unwrap_or(1);
                let mut items = vec![
                    field("name", entry["name"].as_str().unwrap_or("?")),
                    format!("elements={}", count(entry, "count")),
                ];
                if occurrences > 1 {
                    items.push(format!("occurrences={occurrences}"));
                }
                out.push((Stream::Out, format!("byNode: {}", items.join(" "))));
            }
            truncation_records(result, "inspect cost", out);
            return;
        }
        let exact = &result["exact"];
        // A failed emission reports `null` plus `emittedBytesUnavailable` —
        // `unavailable` and the reason under the payload's own member name,
        // never a fabricated zero (ADR-0030 decision 3).
        let mut fields = vec![match exact["emittedBytes"].as_u64() {
            Some(bytes) => format!("emittedBytes={bytes}"),
            None => "emittedBytes=unavailable".to_string(),
        }];
        if let Some(reason) = exact["emittedBytesUnavailable"].as_str() {
            fields.push(field("emittedBytesUnavailable", reason));
        }
        fields.extend([
            format!("actions={}", count(exact, "programActions")),
            format!("rules={}", count(exact, "programRules")),
            format!("waits={}", count(exact, "waitActions")),
        ]);
        // Skipped rule evaluations are disclosed like `lint`'s (#788).
        let skipped = result["skipped"].as_array().map_or(0, Vec::len);
        if skipped > 0 {
            fields.push(format!("skipped={skipped}"));
        }
        out.push((Stream::Out, format!("summary: {}", fields.join(" "))));
        let elements = &result["elementCount"];
        // The count is `Some(total)` or absent — unavailable, never zero —
        // while the known budget still reports (ADR-0030 decision 3).
        if let Some(total) = elements["total"].as_u64() {
            out.push((
                Stream::Out,
                format!(
                    "cost: elements={total} budget={}",
                    count(elements, "budget")
                ),
            ));
            for rule in elements["rules"].as_array().into_iter().flatten() {
                let mut items = vec![
                    field("rule", rule_name(rule)),
                    format!("index={}", count(rule, "index")),
                    format!("elements={}", count(rule, "count")),
                ];
                if let Some(at) = rule.get("span").and_then(at_field) {
                    items.push(at);
                }
                out.push((Stream::Out, format!("share: {}", items.join(" "))));
            }
            if let Some(note) = elements["truncation"]["rules"].as_object() {
                out.push((
                    Stream::Out,
                    truncated_record(
                        &[(
                            "rules",
                            note["shown"].as_u64().unwrap_or(0) as usize,
                            note["total"].as_u64().unwrap_or(0) as usize,
                        )],
                        "wright inspect cost --all",
                    ),
                ));
            }
        } else {
            out.push(cost_record(
                None,
                Some((
                    "elementCountUnavailable",
                    result["elementCountUnavailable"]
                        .as_str()
                        .unwrap_or("counter does not model this program"),
                )),
                Vec::new(),
            ));
        }
        let mut findings = result["findings"].as_array().cloned().unwrap_or_default();
        findings.sort_by_key(finding_severity_rank);
        for finding in &findings {
            out.push((Stream::Out, finding_record(finding)));
        }
        truncation_records(result, "inspect cost", out);
        if let Some(selection) = &self.selection {
            out.push(selected_record(
                "findings",
                findings.len(),
                selection,
                "inspect cost",
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_stay_bare_unless_they_need_quoting() {
        assert_eq!(field("path", "src/main.opy"), "path=src/main.opy");
        assert_eq!(field("at", "a.ws:1:2"), "at=a.ws:1:2");
        assert_eq!(field("share", "+3"), "share=+3");
        assert_eq!(field("message", "two words"), "message=\"two words\"");
        assert_eq!(field("q", "say \"hi\""), "q=\"say \\\"hi\\\"\"");
        assert_eq!(field("e", ""), "e=\"\"");
        assert_eq!(field("k", "a=b"), "k=\"a=b\"");
        assert_eq!(field("bs", "a\\b"), "bs=\"a\\\\b\"");
    }

    fn envelope<T: serde::Serialize>(command: &str, exit: u8, result: T) -> Envelope<T> {
        Envelope {
            wright: wright_driver::result::VersionInfo {
                version: "test".to_string(),
                contract: "wright-result/v1".to_string(),
            },
            command: command.to_string(),
            ok: exit == 0,
            exit,
            diagnostics: Vec::new(),
            selection: None,
            gated: false,
            result,
            comparison: None,
        }
    }

    #[test]
    fn verdict_word_maps_exit_codes() {
        assert_eq!(
            verdict_word(&envelope("check", 0, CheckResult::default())),
            "PASS"
        );
        assert_eq!(
            verdict_word(&envelope("check", 1, CheckResult::default())),
            "ERROR"
        );
        assert_eq!(
            verdict_word(&envelope("check", 2, CheckResult::default())),
            "USAGE"
        );
        assert_eq!(
            verdict_word(&envelope("check", 3, CheckResult::default())),
            "UNSUPPORTED"
        );
        assert_eq!(
            verdict_word(&envelope("check", 4, CheckResult::default())),
            "ERROR"
        );
    }

    #[test]
    fn a_gated_result_reports_fail_like_every_renderer() {
        // `--fail-on` raised a WARN result to exit 1: the verdict is FAIL
        // (the gate's word), not the ERROR a genuine failure reports.
        let mut envelope = envelope("lint", 1, LintResult::default());
        envelope.gated = true;
        assert_eq!(verdict_word(&envelope), "FAIL");
    }

    #[test]
    fn query_commands_show_their_name_on_success() {
        let mut envelope = envelope("symbols", 0, SymbolsResult::default());
        assert_eq!(verdict_word(&envelope), "SYMBOLS");
        envelope.diagnostics.push(Diagnostic {
            code: "w".to_string(),
            stage: wright_driver::Stage::Analysis,
            severity: wright_driver::Severity::Warning,
            message: "w".to_string(),
            status: None,
            span: None,
            source: None,
        });
        assert_eq!(verdict_word(&envelope), "WARN");
    }

    #[test]
    fn finding_record_carries_structured_fields_verbatim() {
        // The driver's `repeated{expression,count,spanKind}` member (#820)
        // flattens to fields around the location; other scalar members
        // sort into the field section before the closing message.
        let record = finding_record(&serde_json::json!({
            "code": "repeated-value",
            "severity": "warning",
            "location": "src/ana.opy:42:5",
            "message": "expr evaluated repeatedly",
            "evidence": "static-indicator",
            "repeated": {
                "expression": "distance(x, y)",
                "count": 3,
                "spanKind": "enclosing-loop"
            }
        }));
        assert_eq!(
            record,
            "finding: warning repeated-value count=3 at=src/ana.opy:42:5 spanKind=enclosing-loop expression=\"distance(x, y)\" evidence=static-indicator message=\"expr evaluated repeatedly\""
        );
    }
}
