use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;
use wright_driver::result::{
    CallGraphResult, CfgResult, CheckResult, CompileResult, ConvertResult, CostResult, Envelope,
    EssentialCommand, EssentialResult, LintResult, RefsResult, RulesResult, SymbolsResult,
};
use wright_driver::{Diagnostic, SelectionOutcome, Severity, Status};

use crate::present::{
    DETAIL_COMMANDS, RenderContext, ResultPresentation, block_flags, call_fan_counts,
    cfg_block_stats, cfg_edge_count, count, diagnostic_counts, diagnostic_files,
    diagnostics_on_stdout, display_location_path, edge_caller, finding_files, finding_record,
    fix_status_counts, inline_location, is_real_source_path, lint_finding_failure,
    lookup_hint_args, meta_severity_counts, notable_callers, query_items, render_node,
    rule_address, rule_name, severity_rank, shown_diagnostics, summary_status, tree_rule,
};

pub(crate) fn render<T: serde::Serialize + ResultPresentation + PlainReport>(
    envelope: &Envelope<T>,
    ctx: &RenderContext<'_>,
) {
    if let Some(artifact) = envelope.result.artifact() {
        let mut err = Vec::new();
        diagnostic_records(envelope, ctx, &mut err);
        for line in err {
            eprintln!("{line}");
        }
        print!("{artifact}");
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

pub(crate) fn render_lines<T: serde::Serialize + ResultPresentation + PlainReport>(
    envelope: &Envelope<T>,
    ctx: &RenderContext<'_>,
) -> (Vec<String>, Vec<String>) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    out.push(verdict_line(envelope, ctx));
    let failed_to_stdout = diagnostics_on_stdout(envelope);
    if !envelope.ok && !envelope.gated && !lint_finding_failure(envelope) {
        if envelope.diagnostics.is_empty() {
            (if failed_to_stdout { &mut out } else { &mut err }).push(format!(
                "error: {}",
                field("message", &format!("{} failed", envelope.command))
            ));
        } else {
            let counts = diagnostic_counts(&envelope.diagnostics);
            let mut fields = severity_fields(counts[0], counts[1], counts[2]);
            let affected = diagnostic_files(&envelope.diagnostics);
            if !affected.is_empty() {
                fields.push(format!("affected={}", affected.len()));
            }
            out.push(format!("summary: {}", fields.join(" ")));
        }
    } else {
        envelope.result.plain_report(envelope, ctx, &mut out);
        comparison_records(envelope.comparison.as_ref(), &mut out);
    }
    diagnostic_records(
        envelope,
        ctx,
        if failed_to_stdout { &mut out } else { &mut err },
    );
    for next in next_commands(envelope) {
        out.push(format!("next: {next}"));
    }
    (out, err)
}

fn verdict_word<T: serde::Serialize + ResultPresentation>(envelope: &Envelope<T>) -> String {
    if envelope.exit != 0 || !envelope.ok {
        return match envelope.exit {
            _ if envelope.gated => "FAIL",
            2 => "USAGE",
            3 => "UNSUPPORTED",
            _ => "ERROR",
        }
        .to_owned();
    }
    match summary_status(envelope) {
        "WARN" => "WARN".into(),
        _ if crate::cli::is_query_command(&envelope.command) => envelope.command.to_uppercase(),
        _ => "PASS".into(),
    }
}
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

pub(crate) fn display_root(root: &Path) -> String {
    if std::env::current_dir().is_ok_and(|cwd| cwd == root) {
        return ".".into();
    }
    display_location_path(&root.display().to_string())
}

fn diagnostic_records<T: serde::Serialize>(
    envelope: &Envelope<T>,
    ctx: &RenderContext<'_>,
    err: &mut Vec<String>,
) {
    let (shown, withheld) = shown_diagnostics(
        &envelope.command,
        &envelope.diagnostics,
        ctx.presentation.all,
    );
    for diagnostic in shown {
        err.push(diagnostic_record(diagnostic));
    }
    if let Some(total) = withheld {
        err.push(truncated_record(
            &[("diagnostics", 20, total)],
            &expand_all(envelope),
        ));
    }
    if let Some(selection) = envelope
        .selection
        .as_ref()
        .filter(|selection| selection.total > 0)
    {
        let mut fields = vec![format!(
            "diagnostics={}/{}",
            envelope.diagnostics.len(),
            selection.total
        )];
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
        err.push(format!("selected: {}", fields.join(" ")));
    }
}

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

fn status_label(status: Status) -> &'static str {
    match status {
        Status::Supported => "supported",
        Status::Partial => "partial",
        Status::Unsupported => "unsupported",
    }
}

fn next_commands<T: serde::Serialize>(envelope: &Envelope<T>) -> Vec<String> {
    let mut commands = Vec::new();
    let result = serde_json::to_value(&envelope.result).unwrap_or_default();
    if let Some(next) = result.get("next").and_then(Value::as_array) {
        commands.extend(next.iter().filter_map(Value::as_str).map(str::to_string));
    }
    match envelope.command.as_str() {
        "inspect" | "analyze" if commands.is_empty() => {
            for cmd in DETAIL_COMMANDS {
                commands.push(cmd.to_string());
            }
        }
        "check" | "compile" => {
            let mut seen = BTreeSet::new();
            for diagnostic in &envelope.diagnostics {
                let Some(args) = lookup_hint_args(diagnostic) else {
                    continue;
                };
                let hint = format!("wright lookup{args}");
                if seen.insert(hint.clone()) {
                    commands.push(hint);
                }
            }
        }
        _ => {}
    }
    commands
}

fn comparison_records(comparison: Option<&Value>, out: &mut Vec<String>) {
    let Some(comparison) = comparison.filter(|c| c.is_object()) else {
        return;
    };
    let delta = comparison["totalDelta"].as_i64().unwrap_or(0);
    let sign = if delta > 0 { "+" } else { "" };
    let reference = comparison["reference"].as_str().unwrap_or("previous");
    out.push(format!(
        "comparison: {} delta={sign}{delta} previousTotal={}",
        field("reference", reference),
        comparison["previousTotal"].as_u64().unwrap_or(0)
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
        out.push(change.clone());
    }
    if total > 5 {
        out.push(format!("truncated: changed=5/{total}"));
    }
}

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

pub(crate) fn truncated_record(lists: &[(&str, usize, usize)], expand: &str) -> String {
    let mut parts: Vec<String> = lists
        .iter()
        .map(|(name, shown, total)| format!("{name}={shown}/{total}"))
        .collect();
    if !expand.is_empty() {
        parts.push(field("expand", expand));
    }
    format!("truncated: {}", parts.join(" "))
}

pub(crate) fn expand_all<T: serde::Serialize>(envelope: &Envelope<T>) -> String {
    let command = match envelope.command.as_str() {
        name @ ("rules" | "symbols" | "refs" | "cfg" | "callgraph" | "cost") => {
            format!("inspect {name}")
        }
        name if name.starts_with("package-") => format!("lint {}", name.replacen('-', " ", 1)),
        name => name.to_string(),
    };
    format!("wright {command} --all")
}

fn at_field(span: &Value) -> Option<String> {
    inline_location(span).map(|location| format!("at={location}"))
}

fn severity_fields(error: usize, warning: usize, info: usize) -> Vec<String> {
    vec![
        format!("errors={error}"),
        format!("warnings={warning}"),
        format!("notes={info}"),
    ]
}

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

fn selected_record(
    noun: &str,
    shown: usize,
    selection: &SelectionOutcome,
    command: &str,
) -> String {
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
    format!("selected: {}", fields.join(" "))
}

fn truncation_records(value: &Value, command: &str, out: &mut Vec<String>) {
    for key in truncation_keys(value) {
        out.push(truncation_line(value, key, command));
    }
}

fn truncation_keys(value: &Value) -> Vec<&str> {
    value["truncation"]
        .as_object()
        .map(|notes| notes.keys().map(String::as_str).collect())
        .unwrap_or_default()
}

fn truncation_line(value: &Value, key: &str, command: &str) -> String {
    let note = &value["truncation"][key];
    let mut parts = vec![format!(
        "{key}={}/{}",
        note["shown"].as_u64().unwrap_or(0),
        note["total"].as_u64().unwrap_or(0)
    )];
    if let Some(field) = by_code_counts(note).as_ref().and_then(by_code_field) {
        parts.push(field);
    }
    parts.push(field("expand", &format!("wright {command} --all")));
    format!("truncated: {}", parts.join(" "))
}

fn program_record(program: &Value, persistent_objects: Option<u64>) -> Option<String> {
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
    Some(format!("program: {}", fields.join(" ")))
}

fn cost_record(elements: Option<u64>, reason: Option<(&str, &str)>, extra: Vec<String>) -> String {
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
    format!("cost: {}", fields.join(" "))
}

pub(crate) trait PlainReport {
    fn origin_fields(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }
    fn plain_report(
        &self,
        envelope: &Envelope<Self>,
        ctx: &RenderContext<'_>,
        out: &mut Vec<String>,
    ) where
        Self: serde::Serialize + Sized;
}

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
        out: &mut Vec<String>,
    ) {
        let mut fields = match &envelope.selection {
            Some(selection) => vec![format!("diagnostics={}", selection.total)],
            None => {
                let counts = diagnostic_counts(&envelope.diagnostics);
                severity_fields(counts[0], counts[1], counts[2])
            }
        };
        let affected = diagnostic_files(&envelope.diagnostics);
        if !affected.is_empty() {
            fields.push(format!("affected={}", affected.len()));
        }
        out.push(format!("summary: {}", fields.join(" ")));
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
        out: &mut Vec<String>,
    ) {
        let counts = diagnostic_counts(&envelope.diagnostics);
        out.push(format!(
            "summary: {}",
            severity_fields(counts[0], counts[1], counts[2]).join(" ")
        ));
        if let Some(output) = self.output.as_ref().filter(|o| o.written_to != "stdout") {
            let mut fields = vec![
                field("path", &output.written_to),
                field("sha256", &output.sha256),
            ];
            if !output.locale.is_empty() {
                fields.push(field("locale", &output.locale));
            }
            out.push(format!("output: {}", fields.join(" ")));
        }
    }
}

impl PlainReport for ConvertResult {
    fn plain_report(
        &self,
        _envelope: &Envelope<Self>,
        _ctx: &RenderContext<'_>,
        _out: &mut Vec<String>,
    ) {
    }
}

impl PlainReport for LintResult {
    fn origin_fields(&self) -> Vec<(&'static str, String)> {
        origin_from_program(&self.program)
    }
    fn plain_report(
        &self,
        _envelope: &Envelope<Self>,
        _ctx: &RenderContext<'_>,
        out: &mut Vec<String>,
    ) {
        let findings = self.findings.as_array().map_or(&[][..], Vec::as_slice);
        let counts = meta_severity_counts(&self.findings_meta);
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
            let (applied, previewed, refused) = fix_status_counts(fixes);
            fields.push(format!("fixes={}", fixes.len()));
            fields.push(format!("applied={applied}"));
            fields.push(format!("previewed={previewed}"));
            fields.push(format!("refused={refused}"));
        }
        let affected = finding_files(findings);
        if !affected.is_empty() {
            fields.push(format!("affected={}", affected.len()));
        }
        out.push(format!("summary: {}", fields.join(" ")));

        let mut sorted: Vec<(&serde_json::Value, Severity)> = findings
            .iter()
            .zip(&self.findings_meta)
            .map(|(finding, meta)| (finding, meta.severity))
            .collect();
        debug_assert_eq!(sorted.len(), findings.len());
        sorted.sort_by_key(|(_, severity)| severity_rank(*severity));
        for (finding, severity) in &sorted {
            out.push(finding_record(finding, *severity));
        }
        if let Some(selection) = &self.selection {
            out.push(selected_record(
                "findings",
                findings.len(),
                selection,
                "lint",
            ));
        }
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
            out.push(format!("fix: {}", items.join(" ")));
            for preview in fix.preview.iter().flatten() {
                out.push(format!("patch: {}", field("source", &preview.source)));
                for line in crate::present::diff_rows(&preview.original, &preview.new_text) {
                    out.push(format!("patch: {line}"));
                }
            }
            for diagnostic in &fix.diagnostics {
                out.push(format!(
                    "refusal: {} {}",
                    diagnostic.code,
                    field("message", &diagnostic.message)
                ));
            }
        }
    }
}

impl PlainReport for EssentialResult {
    fn origin_fields(&self) -> Vec<(&'static str, String)> {
        origin_from_program(&self.result["program"])
    }
    fn plain_report(
        &self,
        _envelope: &Envelope<Self>,
        _ctx: &RenderContext<'_>,
        out: &mut Vec<String>,
    ) {
        match self.command {
            EssentialCommand::Analyze => essential_analyze(self, out),
            EssentialCommand::Inspect => essential_inspect(self, out),
            EssentialCommand::Lint => essential_lint(self, out),
        }
    }
}

fn essential_lint(result: &EssentialResult, out: &mut Vec<String>) {
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
    out.push(format!("summary: {}", fields.join(" ")));
    for (finding, meta) in findings.iter().zip(&result.findings_meta) {
        out.push(finding_record(finding, meta.severity));
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
        out.push(format!("fix: {}", items.join(" ")));
        for preview in fix["preview"].as_array().into_iter().flatten() {
            out.push(format!(
                "patch: {}",
                field("source", preview["source"].as_str().unwrap_or("?"))
            ));
            for line in crate::present::diff_rows(
                preview["original"].as_str().unwrap_or_default(),
                preview["newText"].as_str().unwrap_or_default(),
            ) {
                out.push(format!("patch: {line}"));
            }
        }
        for diagnostic in fix["diagnostics"].as_array().into_iter().flatten() {
            out.push(format!(
                "refusal: {} {}",
                diagnostic["code"].as_str().unwrap_or("refusal"),
                field(
                    "message",
                    diagnostic["message"].as_str().unwrap_or_default()
                )
            ));
        }
    }
}

fn essential_inspect(result: &EssentialResult, out: &mut Vec<String>) {
    let counts = &result.result["counts"];
    out.push(format!(
        "summary: rules={} symbols={} references={}",
        counts["rules"].as_u64().unwrap_or(0),
        counts["symbols"].as_u64().unwrap_or(0),
        counts["references"].as_u64().unwrap_or(0)
    ));
    if let Some(program) = program_record(&result.result["program"], None) {
        out.push(program);
    }
    for rule in result.result["rules"].as_array().into_iter().flatten() {
        let mut items = vec![field("name", rule_name(rule))];
        if let Some(id) = rule["id"].as_u64() {
            items.push(format!("id={id}"));
        }
        out.push(format!("rule: {}", items.join(" ")));
    }
    truncation_records(&result.result, "inspect", out);
}

fn essential_analyze(result: &EssentialResult, out: &mut Vec<String>) {
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
    out.push(format!("summary: {}", fields.join(" ")));

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

    for (risk, meta) in value["risks"]
        .as_array()
        .into_iter()
        .flatten()
        .zip(&result.findings_meta)
    {
        out.push(finding_record(risk, meta.severity));
    }
    if truncation_keys(value).contains(&"risks") {
        out.push(truncation_line(value, "risks", "analyze"));
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
        out.push(format!("hotspot: {}", items.join(" ")));
    }
    if truncation_keys(value).contains(&"hotspots") {
        out.push(truncation_line(value, "hotspots", "analyze"));
    }
    for tree in value["conditionTrees"].as_array().into_iter().flatten() {
        let mut items = vec![
            field("rule", tree_rule(tree)),
            format!("index={}", count(tree, "index") + 1),
            format!("elements={}", count(tree, "elements")),
        ];
        if let Some(at) = tree.get("span").and_then(at_field) {
            items.push(at);
        }
        out.push(format!("tree: {}", items.join(" ")));
    }
    if truncation_keys(value).contains(&"conditionTrees") {
        out.push(truncation_line(value, "conditionTrees", "analyze"));
    }
    let complexity = &value["complexity"];
    out.push(format!(
        "complexity: blocks={} edges={} loopBlocks={} waitBlocks={}",
        count(complexity, "blocks"),
        count(complexity, "edges"),
        count(complexity, "loopBlocks"),
        count(complexity, "waitBlocks")
    ));
    for variable in value["variables"].as_array().into_iter().flatten() {
        let usage = &variable["usage"];
        out.push(format!(
            "variable: {} {} rules={} reads={} writes={}",
            field("kind", variable["kind"].as_str().unwrap_or("variable")),
            field("name", variable["name"].as_str().unwrap_or("<unnamed>")),
            count(usage, "rules"),
            count(usage, "reads"),
            count(usage, "writes")
        ));
    }
    if truncation_keys(value).contains(&"variables") {
        out.push(truncation_line(value, "variables", "analyze"));
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
        out.push(format!("objects: {}", fields.join(" ")));
    }
}

impl PlainReport for RulesResult {
    fn plain_report(
        &self,
        _envelope: &Envelope<Self>,
        _ctx: &RenderContext<'_>,
        out: &mut Vec<String>,
    ) {
        let rules = query_items(&self.0, "rules");
        out.push(format!("summary: rules={}", rules.len()));
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
            out.push(format!("rule: {}", items.join(" ")));
            let content = &rule["content"];
            for condition in content["conditions"].as_array().into_iter().flatten() {
                out.push(format!(
                    "condition: {} {}",
                    field("rule", rule_name(rule)),
                    field("expr", &render_node(&condition["value"]))
                ));
            }
            for action in content["actions"].as_array().into_iter().flatten() {
                out.push(format!(
                    "action: {} {}",
                    field("rule", rule_name(rule)),
                    field("expr", &render_node(action))
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
        out: &mut Vec<String>,
    ) {
        let symbols = query_items(&self.0, "symbols");
        out.push(format!("summary: symbols={}", symbols.len()));
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
            out.push(format!("symbol: {}", items.join(" ")));
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
        out: &mut Vec<String>,
    ) {
        out.push(format!(
            "summary: {} {} reads={} writes={} calls={} rules={}",
            field("symbol", self.0["symbol"].as_str().unwrap_or("<unknown>")),
            field("kind", self.0["kind"].as_str().unwrap_or("symbol")),
            count(&self.0, "reads"),
            count(&self.0, "writes"),
            count(&self.0, "calls"),
            count(&self.0, "rules"),
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
            if let Some(address) = rule_address(&reference["owningRule"]) {
                items.push(field("rule", address));
            } else if let Some(rule) = reference["rule"].as_u64() {
                items.push(format!("rule={rule}"));
            }
            if let Some(action) = reference["action"].as_u64() {
                items.push(format!("action={action}"));
            }
            if let Some(value) = reference.get("value").filter(|v| !v.is_null()) {
                items.push(field("value", &value.to_string()));
            }
            out.push(format!("ref: {}", items.join(" ")));
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
        out: &mut Vec<String>,
    ) {
        let blocks = self.0["blocks"].as_array().map_or(&[][..], Vec::as_slice);
        let edges = cfg_edge_count(blocks);
        let (loops, waits, calls) = cfg_block_stats(blocks);
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
        out.push(format!("summary: {}", fields.join(" ")));
        for block in blocks {
            let mut items = vec![
                format!("id={}", block["id"].as_u64().unwrap_or(0)),
                field("kind", block["kind"].as_str().unwrap_or("block")),
                format!(
                    "actions={}",
                    block["actions"].as_array().map_or(0, Vec::len)
                ),
            ];
            let flags = block_flags(block);
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
            out.push(format!("block: {}", items.join(" ")));
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
        out: &mut Vec<String>,
    ) {
        let edges = query_items(&self.0, "edges");
        let (fan_in, fan_out) = call_fan_counts(edges);
        out.push(format!(
            "summary: edges={} callers={} callees={}",
            edges.len(),
            fan_out.len(),
            fan_in.len()
        ));
        for (name, n) in notable_callers(&fan_in) {
            out.push(format!("fan-in: {} callers={n}", field("subroutine", name)));
        }
        for (name, n) in notable_callers(&fan_out) {
            out.push(format!("fan-out: {} calls={n}", field("rule", name)));
        }
        for edge in edges {
            out.push(format!(
                "edge: {} {}",
                field("caller", edge_caller(edge)),
                field("callee", edge["callee"].as_str().unwrap_or("<unnamed>"))
            ));
        }
        truncation_records(&self.0, "inspect callgraph", out);
        selected_from_payload(&self.0, "edges", edges.len(), out);
    }
}

fn by_code_counts(value: &Value) -> Option<std::collections::BTreeMap<String, usize>> {
    value
        .get("byCode")
        .and_then(Value::as_object)
        .map(|by_code| {
            by_code
                .iter()
                .filter_map(|(code, n)| n.as_u64().map(|n| (code.clone(), n as usize)))
                .collect()
        })
}

fn selected_from_payload(value: &Value, noun: &str, shown: usize, out: &mut Vec<String>) {
    let selection = &value["selection"];
    let total = selection["total"].as_u64();
    let withheld = selection["withheld"].as_u64().unwrap_or(0);
    let Some(total) = total else { return };
    let mut fields = vec![format!("{noun}={shown}/{total}")];
    if let Some(field) = by_code_counts(selection).as_ref().and_then(by_code_field) {
        fields.push(field);
    }
    if withheld > 0 {
        fields.push(format!("withheld={withheld}"));
    }
    out.push(format!("selected: {}", fields.join(" ")));
}

impl PlainReport for CostResult {
    fn plain_report(
        &self,
        _envelope: &Envelope<Self>,
        _ctx: &RenderContext<'_>,
        out: &mut Vec<String>,
    ) {
        let result = &self.result;
        if let Some(rule) = result.get("rule").filter(|rule| rule.is_object()) {
            let mut fields = vec![
                field("rule", rule_name(rule)),
                format!("elements={}", count(rule, "count")),
            ];
            if let Some(at) = rule.get("span").and_then(at_field) {
                fields.push(at);
            }
            out.push(format!("summary: {}", fields.join(" ")));
            for statement in result["statements"].as_array().into_iter().flatten() {
                let mut items = vec![
                    field("name", statement["name"].as_str().unwrap_or("?")),
                    format!("elements={}", count(statement, "count")),
                ];
                if let Some(at) = statement.get("span").and_then(at_field) {
                    items.push(at);
                }
                out.push(format!("statement: {}", items.join(" ")));
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
                out.push(format!("byNode: {}", items.join(" ")));
            }
            truncation_records(result, "inspect cost", out);
            return;
        }
        let exact = &result["exact"];
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
        let skipped = result["skipped"].as_array().map_or(0, Vec::len);
        if skipped > 0 {
            fields.push(format!("skipped={skipped}"));
        }
        out.push(format!("summary: {}", fields.join(" ")));
        let elements = &result["elementCount"];
        if let Some(total) = elements["total"].as_u64() {
            out.push(format!(
                "cost: elements={total} budget={}",
                count(elements, "budget")
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
                out.push(format!("share: {}", items.join(" ")));
            }
            if let Some(note) = elements["truncation"]["rules"].as_object() {
                out.push(truncated_record(
                    &[(
                        "rules",
                        note["shown"].as_u64().unwrap_or(0) as usize,
                        note["total"].as_u64().unwrap_or(0) as usize,
                    )],
                    "wright inspect cost --all",
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
        let mut findings: Vec<(serde_json::Value, Severity)> = result["findings"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .zip(&self.findings_meta)
            .map(|(finding, meta)| (finding, meta.severity))
            .collect();
        findings.sort_by_key(|(_, severity)| severity_rank(*severity));
        for (finding, severity) in &findings {
            out.push(finding_record(finding, *severity));
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
        let mut envelope = envelope("lint", 1, LintResult::default());
        envelope.gated = true;
        assert_eq!(verdict_word(&envelope), "FAIL");
    }

    #[test]
    fn query_commands_show_their_name_on_success() {
        let mut envelope = envelope("symbols", 0, SymbolsResult::default());
        assert_eq!(verdict_word(&envelope), "SYMBOLS");
        envelope.diagnostics.push(Diagnostic::warning(
            "w",
            wright_driver::Stage::Analysis,
            "w",
        ));
        assert_eq!(verdict_word(&envelope), "WARN");
    }

    #[test]
    fn finding_record_carries_structured_fields_verbatim() {
        let record = finding_record(
            &serde_json::json!({
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
            }),
            Severity::Warning,
        );
        assert_eq!(
            record,
            "finding: warning repeated-value count=3 at=src/ana.opy:42:5 spanKind=enclosing-loop expression=\"distance(x, y)\" evidence=static-indicator message=\"expr evaluated repeatedly\""
        );
    }
}
