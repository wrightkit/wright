use std::collections::BTreeSet;

use serde_json::Value;
use wright_driver::result::{
    CallGraphResult, CfgResult, CheckResult, CompileResult, ConvertResult, CostResult, Envelope,
    EssentialResult, LintResult, RefsResult, RulesResult, SymbolsResult,
};

use super::{
    DETAIL_COMMANDS, Presentation, RenderContext, ResultPresentation, array_len, block_flags,
    call_fan_counts, cfg_block_stats, cfg_edge_count, count, diagnostic_counts, diagnostic_files,
    diagnostics_on_stdout, dim, display_location_path, edge_caller, finding_files,
    finding_location_span, finding_severity_label, finding_severity_rank, findings_severity_counts,
    fix_status_counts, fix_status_parts, group_runs, is_real_source_path, location_suffix,
    lookup_hint_args, normalize_finding_spans, notable_callers, persistent_object_summary,
    query_items, render_diff_lines, render_finding_location, render_node, renderable_group_spans,
    rule_address, rule_name, severity_rank, shown_of, source_context, span_suffix, summary_status,
    symbol_kind_heading, tree_rule, unmapped_positions, unmapped_positions_note,
};

fn tag_word<T: serde::Serialize + ResultPresentation>(envelope: &Envelope<T>) -> String {
    if envelope.gated {
        return "FAIL".to_string();
    }
    if !envelope.ok {
        return match envelope.exit {
            wright_driver::result::exit::USAGE => "USAGE",
            wright_driver::result::exit::UNSUPPORTED => "UNSUPPORTED",
            _ => "ERROR",
        }
        .to_string();
    }
    if crate::cli::is_query_command(&envelope.command) {
        return envelope.command.to_uppercase();
    }
    summary_status(envelope).to_string()
}

pub(crate) fn tag(word: &str, color: bool) -> String {
    let text = format!(" {word} ");
    if !color {
        return text;
    }
    let (bg, fg) = match word {
        "PASS" => (42, 30),
        "WARN" => (43, 30),
        "FAIL" | "USAGE" | "ERROR" => (41, 37),
        _ => (46, 30),
    };
    format!("\x1b[{bg};{fg}m{text}\x1b[0m")
}

fn verdict_line<T: serde::Serialize + ResultPresentation>(
    envelope: &Envelope<T>,
    color: bool,
) -> String {
    let word = tag_word(envelope);
    if word.eq_ignore_ascii_case(&envelope.command) {
        tag(&word, color).trim_end().to_string()
    } else {
        format!("{} {}", tag(&word, color), envelope.command)
    }
}

pub(crate) fn report_error(pres: Presentation, word: &str, subject: &str, message: &str) {
    if pres.is_terminal() {
        eprintln!("{} {subject}", tag(word, pres.color));
        eprintln!("  {message}");
    } else {
        eprintln!("wright: {message}");
    }
}

pub(crate) fn exit_word(code: u8) -> &'static str {
    match code {
        wright_driver::result::exit::USAGE => "USAGE",
        wright_driver::result::exit::UNSUPPORTED => "UNSUPPORTED",
        wright_driver::result::exit::SUCCESS => "PASS",
        _ => "ERROR",
    }
}

pub(crate) fn plural(n: usize, singular: &str, plural: &str) -> String {
    if n == 1 {
        format!("{n} {singular}")
    } else {
        format!("{n} {plural}")
    }
}

fn pad(ctx: &RenderContext<'_>, depth: usize) -> String {
    if depth == 0 {
        return String::new();
    }
    if ctx.presentation.compact() {
        "  ".to_string()
    } else {
        "  ".repeat(depth)
    }
}

pub(crate) fn line(ctx: &RenderContext<'_>, depth: usize, text: &str) {
    println!("{}{text}", pad(ctx, depth));
}

pub(crate) fn note(ctx: &RenderContext<'_>, depth: usize, text: &str) {
    println!("{}{}", pad(ctx, depth), dim(text, ctx.presentation.color));
}

pub(crate) fn heading(ctx: &RenderContext<'_>, text: &str) {
    if ctx.presentation.color {
        println!("\n\x1b[1m{text}\x1b[0m");
    } else {
        println!("\n{text}");
    }
}

fn dprint(text: &str, to_stdout: bool) {
    if to_stdout {
        println!("{text}");
    } else {
        eprintln!("{text}");
    }
}

fn dline(ctx: &RenderContext<'_>, depth: usize, text: &str, to_stdout: bool) {
    dprint(&format!("{}{text}", pad(ctx, depth)), to_stdout);
}

fn dnote(ctx: &RenderContext<'_>, depth: usize, text: &str, to_stdout: bool) {
    dprint(
        &format!("{}{}", pad(ctx, depth), dim(text, ctx.presentation.color)),
        to_stdout,
    );
}

pub(crate) trait TerminalReport {
    fn artifact(&self) -> Option<&str> {
        None
    }
    fn headline(&self) -> Option<String> {
        None
    }
    fn body(&self, ctx: &RenderContext<'_>);
}

pub(crate) fn render<T: serde::Serialize + ResultPresentation + TerminalReport>(
    envelope: &Envelope<T>,
    ctx: &RenderContext<'_>,
) {
    if let Some(artifact) = envelope.result.artifact() {
        if !envelope.ok || !envelope.diagnostics.is_empty() {
            eprintln!("{}", verdict_line(envelope, ctx.presentation.color));
            diagnostics(ctx, envelope, false);
        }
        print!("{artifact}");
        return;
    }
    println!("{}", verdict_line(envelope, ctx.presentation.color));
    if let Some(text) = headline(envelope) {
        println!("{}{}", pad(ctx, 1), dim(&text, ctx.presentation.color));
    }
    let to_stdout = diagnostics_on_stdout(envelope);
    diagnostics(ctx, envelope, to_stdout);
    if envelope.ok || envelope.gated {
        envelope.result.body(ctx);
        comparison(ctx, envelope.comparison.as_ref());
    } else if envelope.diagnostics.is_empty() {
        dprint(&format!("{}: failed", envelope.command), to_stdout);
    }
    if envelope.command == "check" {
        check_footer(ctx, envelope);
    }
}

fn headline<T: serde::Serialize + ResultPresentation + TerminalReport>(
    envelope: &Envelope<T>,
) -> Option<String> {
    match envelope.command.as_str() {
        "check" => Some(diagnostic_summary(envelope)),
        "convert" => None,
        _ if !envelope.ok && !envelope.gated => {
            if envelope.diagnostics.is_empty() {
                None
            } else {
                Some(diagnostic_summary(envelope))
            }
        }
        _ => envelope.result.headline(),
    }
}

fn diagnostic_summary<T: serde::Serialize + ResultPresentation>(envelope: &Envelope<T>) -> String {
    if let Some(selection) = &envelope.selection {
        return plural(selection.total, "diagnostic", "diagnostics");
    }
    severity_counts(diagnostic_counts(&envelope.diagnostics), "diagnostic")
}

fn severity_counts(counts: [usize; 3], noun: &str) -> String {
    let mut parts = Vec::new();
    if counts[0] > 0 {
        parts.push(plural(counts[0], "error", "errors"));
    }
    if counts[1] > 0 {
        parts.push(plural(counts[1], "warning", "warnings"));
    }
    if counts[2] > 0 {
        parts.push(plural(counts[2], "info finding", "info findings"));
    }
    if parts.is_empty() {
        plural(0, noun, &format!("{noun}s"))
    } else {
        parts.join(", ")
    }
}

fn check_footer<T: serde::Serialize + ResultPresentation>(
    ctx: &RenderContext<'_>,
    envelope: &Envelope<T>,
) {
    let files = diagnostic_files(&envelope.diagnostics);
    let mut parts = Vec::new();
    if !files.is_empty() {
        parts.push(plural(files.len(), "file affected", "files affected"));
    }
    if ctx.presentation.interactive {
        parts.push(format!("{} ms", ctx.elapsed.as_millis()));
    }
    if !parts.is_empty() {
        note(ctx, 1, &parts.join(" · "));
    }
}

fn lint_footer(ctx: &RenderContext<'_>, findings: &[Value], skipped: usize) {
    let files = finding_files(findings);
    let mut parts = Vec::new();
    if !files.is_empty() {
        parts.push(plural(files.len(), "file affected", "files affected"));
    }
    if skipped > 0 {
        parts.push(plural(
            skipped,
            "rule evaluation skipped",
            "rule evaluations skipped",
        ));
    }
    if ctx.presentation.interactive {
        parts.push(format!("{} ms", ctx.elapsed.as_millis()));
    }
    if !parts.is_empty() {
        note(ctx, 1, &parts.join(" · "));
    }
}

fn diagnostics<T: serde::Serialize + ResultPresentation>(
    ctx: &RenderContext<'_>,
    envelope: &Envelope<T>,
    to_stdout: bool,
) {
    let mut ordered: Vec<&wright_driver::Diagnostic> = envelope.diagnostics.iter().collect();
    ordered.sort_by_key(|diagnostic| severity_rank(diagnostic.severity));
    let total = ordered.len();
    let bounded = !matches!(envelope.command.as_str(), "compile" | "convert");
    let limit = if ctx.presentation.all || !bounded {
        usize::MAX
    } else {
        20
    };
    for (index, diagnostic) in ordered.into_iter().take(limit).enumerate() {
        if index > 0 {
            dprint("", to_stdout);
        }
        diagnostic_item(ctx, diagnostic, to_stdout);
        if matches!(envelope.command.as_str(), "check" | "compile")
            && let Some(args) = lookup_hint_args(diagnostic)
        {
            dnote(
                ctx,
                1,
                &format!(
                    "hint: run 'wright lookup{args}' with the rejected name to find its spelling and signature"
                ),
                to_stdout,
            );
        }
    }
    if bounded && !ctx.presentation.all && total > 20 {
        dline(
            ctx,
            1,
            &format!("diagnostics: showing 20 of {total}; --all to see every item"),
            to_stdout,
        );
    }
    if let Some(selection) = &envelope.selection
        && selection.withheld > 0
    {
        dline(
            ctx,
            1,
            &format!(
                "... {} withheld (--max)",
                plural(selection.withheld, "diagnostic", "diagnostics")
            ),
            to_stdout,
        );
    }
}

fn diagnostic_item(
    ctx: &RenderContext<'_>,
    diagnostic: &wright_driver::Diagnostic,
    to_stdout: bool,
) {
    let severity = diagnostic.severity.as_str();
    let label = if ctx.presentation.color {
        let code = match diagnostic.severity {
            wright_driver::Severity::Error => 31,
            wright_driver::Severity::Warning => 33,
            wright_driver::Severity::Info => 36,
        };
        format!("\x1b[{code}m{severity}\x1b[0m")
    } else {
        severity.to_string()
    };
    dline(
        ctx,
        0,
        &format!("{label}[{}]: {}", diagnostic.code, diagnostic.message),
        to_stdout,
    );
    let mut notes = Vec::new();
    if let Some(span) = &diagnostic.span {
        if is_real_source_path(&span.path) {
            dline(
                ctx,
                1,
                &format!(
                    "--> {}:{}:{}",
                    display_location_path(&span.path),
                    span.start.line,
                    span.start.col
                ),
                to_stdout,
            );
            if let Some(context) =
                source_context(&span.path, span.start.line, span.start.col, &pad(ctx, 1))
            {
                dprint(&context, to_stdout);
            }
        } else {
            notes.push(format!(
                "at {}:{}:{} (no source file)",
                span.path, span.start.line, span.start.col
            ));
        }
    }
    notes.push(diagnostic.stage.as_str().to_string());
    if let Some(source) = &diagnostic.source
        && source.kind != "workshop"
    {
        notes.push(format!("{kind} source", kind = source.kind));
    }
    dnote(ctx, 1, &format!("= {}", notes.join(" · ")), to_stdout);
}

fn elements(n: usize) -> String {
    plural(n, "element", "elements")
}

fn finding_head(ctx: &RenderContext<'_>, finding: &Value, occurrences: usize) -> String {
    let severity = finding_severity_label(finding, ctx.presentation.color);
    let code = finding["code"].as_str().unwrap_or("finding");
    let message = finding["message"].as_str().unwrap_or_default();
    if occurrences > 1 {
        format!("{severity}[{code}] ({occurrences} findings): {message}")
    } else {
        format!("{severity}[{code}]: {message}")
    }
}

fn finding_location_line(ctx: &RenderContext<'_>, span: &Value, depth: usize, frame: bool) {
    render_finding_location(span, ctx, &pad(ctx, depth), frame);
}

fn uniform_evidence(findings: &[Value]) -> Option<String> {
    let mut classes = findings
        .iter()
        .map(|finding| finding["evidence"].as_str().unwrap_or("exact"))
        .collect::<BTreeSet<_>>();
    if classes.len() == 1 {
        classes.pop_first().map(str::to_string)
    } else {
        None
    }
}

fn finding_section(
    ctx: &RenderContext<'_>,
    title: &str,
    findings: &[Value],
    withheld: Option<u64>,
) {
    let evidence = uniform_evidence(findings);
    let title = match &evidence {
        Some(evidence) => format!("{title} (evidence: {evidence})"),
        None => title.to_string(),
    };
    heading(ctx, &title);
    if findings.is_empty() {
        line(ctx, 1, "none");
        return;
    }
    let mixed = evidence.is_none();
    for group in group_runs(findings) {
        let first = &group[0];
        line(ctx, 1, &finding_head(ctx, first, group.len()));
        if group.len() == 1 {
            if let Some(span) = first.get("span").filter(|span| span.is_object()) {
                finding_location_line(ctx, span, 2, true);
            }
        } else {
            for span in renderable_group_spans(group) {
                finding_location_line(ctx, span, 2, false);
            }
        }
        let mut notes = Vec::new();
        if mixed {
            notes.push(format!(
                "evidence: {}",
                first["evidence"].as_str().unwrap_or("exact")
            ));
        }
        if let Some(boundedness) = first.get("boundedness").and_then(Value::as_str) {
            notes.push(format!("boundedness: {boundedness}"));
        }
        if let Some(repeated) = first.get("repeated").and_then(Value::as_u64)
            && repeated > 1
        {
            notes.push(format!("occurs at {repeated} sites"));
        }
        if let Some(unmapped) = unmapped_positions_note(&unmapped_positions(group)) {
            notes.push(unmapped);
        }
        if !notes.is_empty() {
            note(ctx, 2, &format!("= {}", notes.join(" · ")));
        }
    }
    if let Some(withheld) = withheld.filter(|withheld| *withheld > 0) {
        line(
            ctx,
            1,
            &format!(
                "... {} withheld (--max)",
                plural(withheld as usize, "finding", "findings")
            ),
        );
    }
}

fn truncation_notes(ctx: &RenderContext<'_>, value: &Value) {
    let Some(notes) = value["truncation"].as_object() else {
        return;
    };
    for (key, truncation) in notes {
        let mut text = format!(
            "{key}: showing {} of {}",
            truncation["shown"], truncation["total"]
        );
        if let Some(by_code) = truncation["byCode"]
            .as_object()
            .filter(|codes| !codes.is_empty())
        {
            let parts = by_code
                .iter()
                .map(|(code, n)| format!("{code}: {n}"))
                .collect::<Vec<_>>()
                .join(", ");
            text.push_str(&format!(" ({parts})"));
        }
        text.push_str("; --all to see every item");
        note(ctx, 1, &text);
    }
}

fn withheld_note(ctx: &RenderContext<'_>, value: &Value, noun: &str) {
    if let Some(withheld) = value["selection"]["withheld"]
        .as_u64()
        .filter(|withheld| *withheld > 0)
    {
        line(
            ctx,
            1,
            &format!(
                "... {} withheld (--max)",
                plural(withheld as usize, noun, &format!("{noun}s"))
            ),
        );
    }
}

pub(crate) fn next(ctx: &RenderContext<'_>, value: &Value) {
    let Some(commands) = value["next"]
        .as_array()
        .filter(|commands| !commands.is_empty())
    else {
        return;
    };
    heading(ctx, "Next");
    for item in commands {
        if let Some(command) = item.as_str().or_else(|| item["command"].as_str()) {
            line(ctx, 1, command);
        }
    }
}

fn comparison(ctx: &RenderContext<'_>, comparison: Option<&Value>) {
    let Some(comparison) = comparison.filter(|c| c.is_object()) else {
        return;
    };
    let delta = comparison["totalDelta"].as_i64().unwrap_or(0);
    let sign = if delta > 0 { "+" } else { "" };
    let reference = comparison["reference"].as_str().unwrap_or("previous");
    println!(
        "\nElement cost: {sign}{delta} vs {reference} run ({} total)",
        comparison["previousTotal"].as_u64().unwrap_or(0)
    );
    let mut lines = Vec::new();
    for change in comparison["changed"].as_array().into_iter().flatten() {
        let name = change["name"].as_str().unwrap_or("?");
        let d = change["delta"].as_i64().unwrap_or(0);
        let s = if d > 0 { "+" } else { "" };
        lines.push(format!(
            "  {name}: {} -> {} ({s}{d})",
            change["before"], change["after"]
        ));
    }
    for name in comparison["added"].as_array().into_iter().flatten() {
        lines.push(format!("  {}: added", name.as_str().unwrap_or("?")));
    }
    for name in comparison["removed"].as_array().into_iter().flatten() {
        lines.push(format!("  {}: removed", name.as_str().unwrap_or("?")));
    }
    for line in lines.iter().take(5) {
        println!("{line}");
    }
    if lines.len() > 5 {
        line(
            ctx,
            1,
            &format!(
                "... {}",
                plural(lines.len() - 5, "more changed rule", "more changed rules")
            ),
        );
    }
}

fn program_overview(ctx: &RenderContext<'_>, program: &Value, persistent_objects: Option<usize>) {
    heading(ctx, "Program overview");
    let mut parts = vec![
        plural(count(program, "files"), "file", "files"),
        plural(count(program, "rules"), "rule", "rules"),
        plural(
            count(program, "globalVariables"),
            "global variable",
            "global variables",
        ),
        plural(
            count(program, "playerVariables"),
            "player variable",
            "player variables",
        ),
        plural(count(program, "subroutines"), "subroutine", "subroutines"),
    ];
    if let Some(objects) = persistent_objects {
        parts.push(plural(objects, "persistent object", "persistent objects"));
    }
    line(ctx, 1, &parts.join(", "));
}

impl TerminalReport for CheckResult {
    fn body(&self, ctx: &RenderContext<'_>) {
        let value = serde_json::to_value(self).unwrap_or_default();
        if let Some(program) = value.get("program").filter(|program| program.is_object()) {
            program_overview(ctx, program, None);
        }
        next(ctx, &value);
    }
}

impl TerminalReport for CompileResult {
    fn artifact(&self) -> Option<&str> {
        self.output
            .as_ref()
            .filter(|output| output.written_to == "stdout")
            .map(|output| output.text.as_str())
    }
    fn headline(&self) -> Option<String> {
        self.output
            .as_ref()
            .filter(|output| output.written_to != "stdout")
            .map(|output| format!("wrote {} (sha256 {})", output.written_to, output.sha256))
    }
    fn body(&self, _ctx: &RenderContext<'_>) {}
}

impl TerminalReport for ConvertResult {
    fn artifact(&self) -> Option<&str> {
        Some(&self.text)
    }
    fn body(&self, _ctx: &RenderContext<'_>) {}
}

impl TerminalReport for LintResult {
    fn headline(&self) -> Option<String> {
        let rules = array_len(&self.rules);
        let mut metadata = if let Some(selection) = &self.selection {
            plural(selection.total, "finding", "findings")
        } else {
            severity_counts(
                findings_severity_counts(self.findings.as_array().map_or(&[][..], Vec::as_slice)),
                "finding",
            )
        };
        metadata.push_str(&format!(" across {}", plural(rules, "rule", "rules")));
        if let Some(fixes) = &self.fixes {
            let parts = fix_status_parts(fix_status_counts(fixes));
            metadata.push_str(&format!("; {}", plural(fixes.len(), "fix", "fixes")));
            if !parts.is_empty() {
                metadata.push_str(&format!(" ({})", parts.join(", ")));
            }
        }
        Some(metadata)
    }
    fn body(&self, ctx: &RenderContext<'_>) {
        let findings = sorted_findings(&self.findings);
        finding_section(
            ctx,
            "Lint findings",
            &findings,
            withheld(self.selection.as_ref()),
        );
        lint_footer(ctx, &findings, array_len(&self.skipped));
        if let Some(fixes) = &self.fixes {
            lint_fixes(ctx, fixes);
        }
        next(ctx, &serde_json::to_value(self).unwrap_or_default());
    }
}

fn sorted_findings(findings: &Value) -> Vec<Value> {
    let mut findings = findings.as_array().cloned().unwrap_or_default();
    findings.sort_by_key(finding_severity_rank);
    findings
}

fn withheld(selection: Option<&wright_driver::SelectionOutcome>) -> Option<u64> {
    selection.and_then(|selection| (selection.withheld > 0).then_some(selection.withheld as u64))
}

fn lint_fixes(ctx: &RenderContext<'_>, fixes: &[wright_driver::result::LintFixOutcome]) {
    heading(ctx, "Lint fixes");
    if fixes.is_empty() {
        line(ctx, 1, "none offered");
        return;
    }
    for fix in fixes {
        let status = match fix.status {
            wright_driver::result::LintFixStatus::Preview => "fix",
            wright_driver::result::LintFixStatus::Applied => "applied",
            wright_driver::result::LintFixStatus::Refused => "refused",
        };
        line(ctx, 1, &format!("{status}[{}]: {}", fix.code, fix.summary));
        if let Some(span) = fix.span.as_ref().filter(|span| span.is_object()) {
            finding_location_line(ctx, span, 2, false);
        }
        for preview in fix.preview.iter().flatten() {
            line(ctx, 2, &preview.source);
            render_diff_lines(&preview.original, &preview.new_text, &pad(ctx, 3));
        }
        for diagnostic in &fix.diagnostics {
            line(
                ctx,
                2,
                &format!("refused — {}: {}", diagnostic.code, diagnostic.message),
            );
        }
    }
}

impl TerminalReport for EssentialResult {
    fn headline(&self) -> Option<String> {
        if let Some(selection) = &self.selection {
            return Some(plural(selection.total, "finding", "findings"));
        }
        let counts = &self.result["counts"];
        let mut parts = Vec::new();
        for key in ["findings", "risks"] {
            if let Some(group) = counts.get(key) {
                let total = group["total"].as_u64().unwrap_or(0) as usize;
                let severities = [
                    group["error"].as_u64().unwrap_or(0) as usize,
                    group["warning"].as_u64().unwrap_or(0) as usize,
                    group["info"].as_u64().unwrap_or(0) as usize,
                ];
                parts.push(if severities.iter().all(|n| *n == 0) {
                    plural(total, "finding", "findings")
                } else {
                    severity_counts(severities, "finding")
                });
            }
        }
        for (key, singular, plural_word) in [
            ("rules", "rule", "rules"),
            ("symbols", "symbol", "symbols"),
            ("references", "reference", "references"),
            ("elements", "element", "elements"),
            (
                "persistentObjects",
                "persistent object",
                "persistent objects",
            ),
            ("skipped", "skipped", "skipped"),
        ] {
            if let Some(total) = counts.get(key).and_then(Value::as_u64) {
                parts.push(plural(total as usize, singular, plural_word));
            }
        }
        Some(parts.join(", "))
    }
    fn body(&self, ctx: &RenderContext<'_>) {
        let value = &self.result;
        if value.get("hotspots").is_some() {
            analyze_body(ctx, value);
        } else if let Some(program) = value.get("program").filter(|p| p.is_object()) {
            program_overview(ctx, program, None);
            if let Some(rules) = value["rules"].as_array() {
                heading(ctx, "Rules");
                for rule in rules {
                    let name = match rule_address(rule) {
                        Some(name) => name.to_string(),
                        None => format!("rule {}", rule["id"]),
                    };
                    line(ctx, 1, &name);
                }
            }
            if value["next"].as_array().is_none_or(Vec::is_empty) {
                heading(ctx, "Detail commands");
                for command in DETAIL_COMMANDS {
                    line(ctx, 1, command);
                }
            }
        } else {
            let mut findings = value["findings"].clone();
            normalize_finding_spans(&mut findings);
            let findings = sorted_findings(&findings);
            finding_section(
                ctx,
                "Lint findings",
                &findings,
                withheld(self.selection.as_ref()),
            );
            lint_footer(ctx, &findings, 0);
        }
        truncation_notes(ctx, value);
        if let Some(expand) = value["expand"].as_str() {
            note(ctx, 1, &expand.replace(" / all: true", ""));
        }
        next(ctx, value);
    }
}

fn analyze_body(ctx: &RenderContext<'_>, value: &Value) {
    program_overview(
        ctx,
        &value["program"],
        Some(count(&value["counts"], "persistentObjects")),
    );
    note(ctx, 1, "evidence: [static] parsed program inventory");

    let cost = &value["cost"];
    heading(ctx, "Workshop cost");
    if let Some(elements) = cost["elementCount"].as_u64() {
        line(
            ctx,
            1,
            &format!(
                "{elements} elements [exact] — canonical element count; structural size, not runtime cost"
            ),
        );
    } else {
        line(
            ctx,
            1,
            &format!(
                "element count unavailable: {}",
                cost["unavailableReason"]
                    .as_str()
                    .unwrap_or("counter does not model this program")
            ),
        );
    }
    line(
        ctx,
        1,
        &format!(
            "{}, {}, {} [static]",
            plural(count(&cost["counts"], "actions"), "action", "actions"),
            plural(
                count(&cost["counts"], "conditions"),
                "condition",
                "conditions"
            ),
            plural(count(&cost["counts"], "waits"), "wait", "waits"),
        ),
    );

    let element_total = cost["elementCount"].as_u64();
    heading(ctx, "Hotspots");
    note(
        ctx,
        1,
        if element_total.is_some() {
            "ranked by Workshop element count [exact counts; the ranking itself is a heuristic]"
        } else {
            "ranked by control-flow size [heuristic: blocks + edges]"
        },
    );
    for rule in value["hotspots"].as_array().into_iter().flatten() {
        let flow = &rule["controlFlow"];
        let (blocks, edges) = (count(flow, "blocks"), count(flow, "edges"));
        let risks = count(rule, "risks");
        let location = location_suffix(rule.get("span"));
        let shape = format!(
            "{}, {}, {}",
            plural(blocks, "block", "blocks"),
            plural(edges, "edge", "edges"),
            plural(risks, "risk", "risks"),
        );
        match rule["elements"].as_u64() {
            Some(count) => {
                let share = element_total
                    .filter(|total| *total > 0)
                    .map(|total| format!(" ({:.1}%)", count as f64 * 100.0 / total as f64))
                    .unwrap_or_default();
                line(
                    ctx,
                    2,
                    &format!(
                        "\"{}\": {}{share}, {shape}{location}",
                        rule_name(rule),
                        elements(count as usize)
                    ),
                );
            }
            None => line(
                ctx,
                2,
                &format!("\"{}\": {shape}{location}", rule_name(rule)),
            ),
        }
    }

    let c = &value["complexity"];
    heading(ctx, "Complexity");
    line(
        ctx,
        1,
        &format!(
            "{}, {}, {}, {} [static]",
            plural(count(c, "blocks"), "block", "blocks"),
            plural(count(c, "edges"), "edge", "edges"),
            plural(count(c, "loopBlocks"), "loop block", "loop blocks"),
            plural(count(c, "waitBlocks"), "wait block", "wait blocks"),
        ),
    );
    if value["conditionTrees"]
        .as_array()
        .is_some_and(|trees| !trees.is_empty())
    {
        line(ctx, 1, "top condition trees by element count [exact]");
    }
    for tree in value["conditionTrees"].as_array().into_iter().flatten() {
        line(
            ctx,
            2,
            &format!(
                "\"{}\" condition {}: {}{}",
                tree_rule(tree),
                count(tree, "index") + 1,
                elements(count(tree, "elements")),
                location_suffix(tree.get("span")),
            ),
        );
    }

    let risks = value["risks"].as_array().map_or(&[][..], Vec::as_slice);
    let evidence = uniform_evidence(risks);
    let title = match &evidence {
        Some(evidence) => format!("Performance and stability risks (evidence: {evidence})"),
        None => "Performance and stability risks".to_string(),
    };
    heading(ctx, &title);
    note(
        ctx,
        1,
        "heuristic/static indicators — not measured runtime cost or behavior",
    );
    let mixed = evidence.is_none();
    for risk in risks {
        line(ctx, 2, &finding_head(ctx, risk, 1));
        if let Some(span) = finding_location_span(risk) {
            finding_location_line(ctx, &span, 3, false);
        }
        if mixed {
            note(
                ctx,
                3,
                &format!(
                    "= evidence: {}",
                    risk["evidence"].as_str().unwrap_or("static")
                ),
            );
        }
    }
    if count(&value["persistentObjects"], "total") > 0 {
        note(
            ctx,
            1,
            &persistent_object_summary(&value["persistentObjects"]),
        );
    }
    if risks.is_empty() && count(&value["persistentObjects"], "total") == 0 {
        line(ctx, 1, "none");
    }
    if let Some(withheld_n) = value["selection"]["withheld"].as_u64().filter(|n| *n > 0) {
        line(
            ctx,
            1,
            &format!(
                "... {} withheld (--max)",
                plural(withheld_n as usize, "finding", "findings")
            ),
        );
    }

    heading(ctx, "State and coupling");
    note(
        ctx,
        1,
        "Top variables (heuristic ranking: rules touched, then reads + writes)",
    );
    for variable in value["variables"].as_array().into_iter().flatten() {
        let u = &variable["usage"];
        line(
            ctx,
            2,
            &format!(
                "{} {}: {}, {}, {} [static]",
                variable["kind"].as_str().unwrap_or("variable"),
                variable["name"].as_str().unwrap_or("<unnamed>"),
                plural(count(u, "rules"), "rule", "rules"),
                plural(count(u, "reads"), "read", "reads"),
                plural(count(u, "writes"), "write", "writes"),
            ),
        );
    }
}

impl TerminalReport for RulesResult {
    fn headline(&self) -> Option<String> {
        Some(plural(query_items(&self.0, "rules").len(), "rule", "rules"))
    }
    fn body(&self, ctx: &RenderContext<'_>) {
        let rules = query_items(&self.0, "rules");
        heading(ctx, "Rules");
        if rules.is_empty() {
            line(ctx, 1, "none");
            return;
        }
        for rule in rules {
            let name = rule_name(rule);
            let event = rule["event"].as_str().unwrap_or("?");
            line(
                ctx,
                1,
                &format!("{name} [{event}]{}", span_suffix(rule, " --> ")),
            );
            let content = &rule["content"];
            for condition in content["conditions"].as_array().into_iter().flatten() {
                line(ctx, 2, &format!("if {}", render_node(&condition["value"])));
            }
            for action in content["actions"].as_array().into_iter().flatten() {
                line(ctx, 2, &render_node(action));
            }
        }
        truncation_notes(ctx, &self.0);
        withheld_note(ctx, &self.0, "rule");
    }
}

impl TerminalReport for SymbolsResult {
    fn headline(&self) -> Option<String> {
        Some(plural(
            query_items(&self.0, "symbols").len(),
            "symbol",
            "symbols",
        ))
    }
    fn body(&self, ctx: &RenderContext<'_>) {
        let symbols = query_items(&self.0, "symbols");
        heading(ctx, "Symbols");
        if symbols.is_empty() {
            line(ctx, 1, "none");
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
            line(
                ctx,
                1,
                &format!("{} ({})", symbol_kind_heading(kind), members.len()),
            );
            for symbol in members.iter() {
                let name = symbol["name"]
                    .as_str()
                    .filter(|name| !name.is_empty())
                    .unwrap_or("<unnamed>");
                line(ctx, 2, &format!("{name}{}", span_suffix(symbol, " --> ")));
            }
        }
        truncation_notes(ctx, &self.0);
        withheld_note(ctx, &self.0, "symbol");
    }
}

impl TerminalReport for RefsResult {
    fn headline(&self) -> Option<String> {
        Some(format!(
            "{} ({}): {}, {}, {} across {}",
            self.0["symbol"].as_str().unwrap_or("<unknown>"),
            self.0["kind"].as_str().unwrap_or("symbol"),
            plural(count(&self.0, "reads"), "read", "reads"),
            plural(count(&self.0, "writes"), "write", "writes"),
            plural(count(&self.0, "calls"), "call", "calls"),
            plural(count(&self.0, "rules"), "rule", "rules"),
        ))
    }
    fn body(&self, ctx: &RenderContext<'_>) {
        let references = self.0["references"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        heading(
            ctx,
            &format!(
                "References to {} ({})",
                self.0["symbol"].as_str().unwrap_or("<unknown>"),
                self.0["kind"].as_str().unwrap_or("symbol"),
            ),
        );
        if references.is_empty() {
            line(ctx, 1, "none");
            return;
        }
        for reference in references.iter() {
            let context = match rule_address(&reference["owningRule"]) {
                Some(name) => format!(" (rule {name})"),
                _ => match (reference["rule"].as_u64(), reference["action"].as_u64()) {
                    (Some(rule), Some(action)) => format!(" (rule {rule}, action {action})"),
                    (Some(rule), None) => format!(" (rule {rule})"),
                    _ => String::new(),
                },
            };
            let location = span_suffix(reference, " --> ");
            line(
                ctx,
                1,
                &format!(
                    "{}{}{}",
                    reference["kind"].as_str().unwrap_or("reference"),
                    location,
                    context
                ),
            );
        }
        truncation_notes(ctx, &self.0);
        withheld_note(ctx, &self.0, "reference");
    }
}

impl TerminalReport for CfgResult {
    fn headline(&self) -> Option<String> {
        let blocks = self.0["blocks"].as_array().map_or(&[][..], Vec::as_slice);
        Some(format!(
            "{}, {}",
            plural(blocks.len(), "block", "blocks"),
            plural(cfg_edge_count(blocks), "edge", "edges"),
        ))
    }
    fn body(&self, ctx: &RenderContext<'_>) {
        let blocks = self.0["blocks"].as_array().map_or(&[][..], Vec::as_slice);
        match ctx.subject {
            Some(rule) => heading(ctx, &format!("Control-flow graph of rule \"{rule}\"")),
            None => heading(ctx, "Control-flow graph"),
        }
        if blocks.is_empty() {
            line(ctx, 1, "none");
            return;
        }
        let (loops, waits, calls) = cfg_block_stats(blocks);
        line(
            ctx,
            1,
            &format!(
                "{}, {}: {}, {}, {}",
                plural(blocks.len(), "block", "blocks"),
                plural(cfg_edge_count(blocks), "edge", "edges"),
                plural(loops, "loop header", "loop headers"),
                plural(waits, "wait block", "wait blocks"),
                plural(calls, "call site", "call sites"),
            ),
        );
        line(
            ctx,
            1,
            &format!(
                "entry: block {}, exit: block {}",
                self.0["entry"].as_u64().unwrap_or(0),
                self.0["exit"].as_u64().unwrap_or(0),
            ),
        );
        for block in blocks {
            let flags = block_flags(block);
            let flags = if flags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", flags.join(" "))
            };
            line(
                ctx,
                1,
                &format!(
                    "block {} ({}): {}{}",
                    block["id"].as_u64().unwrap_or(0),
                    block["kind"].as_str().unwrap_or("block"),
                    plural(array_len(&block["actions"]), "action", "actions"),
                    flags,
                ),
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
                line(ctx, 2, &format!("-> {successors}"));
            }
        }
        truncation_notes(ctx, &self.0);
        withheld_note(ctx, &self.0, "block");
    }
}

impl TerminalReport for CallGraphResult {
    fn headline(&self) -> Option<String> {
        Some(plural(query_items(&self.0, "edges").len(), "edge", "edges"))
    }
    fn body(&self, ctx: &RenderContext<'_>) {
        let edges = query_items(&self.0, "edges");
        heading(ctx, "Call graph");
        if edges.is_empty() {
            line(ctx, 1, "none");
            return;
        }
        let (fan_in, fan_out) = call_fan_counts(edges);
        line(
            ctx,
            1,
            &format!(
                "{}: {}, {}",
                plural(edges.len(), "edge", "edges"),
                plural(fan_out.len(), "calling rule", "calling rules"),
                plural(fan_in.len(), "subroutine called", "subroutines called"),
            ),
        );
        let most_called = notable_callers(&fan_in);
        if !most_called.is_empty() {
            line(ctx, 1, "Most-called subroutines");
            for (name, n) in most_called.iter() {
                line(
                    ctx,
                    2,
                    &format!("{name}: {}", plural(*n, "caller", "callers")),
                );
            }
        }
        let busiest = notable_callers(&fan_out);
        if !busiest.is_empty() {
            line(ctx, 1, "Busiest callers");
            for (name, n) in busiest.iter() {
                line(
                    ctx,
                    2,
                    &format!("{name}: calls {}", plural(*n, "subroutine", "subroutines")),
                );
            }
        }
        line(ctx, 1, "Edges");
        for edge in edges.iter() {
            line(
                ctx,
                2,
                &format!(
                    "{} -> {}",
                    edge_caller(edge),
                    edge["callee"].as_str().unwrap_or("<unnamed>")
                ),
            );
        }
        truncation_notes(ctx, &self.0);
        withheld_note(ctx, &self.0, "edge");
    }
}

impl TerminalReport for CostResult {
    fn headline(&self) -> Option<String> {
        if let Some(rule) = self.result.get("rule").filter(|rule| rule.is_object()) {
            return Some(format!(
                "{} · {}",
                rule_name(rule),
                elements(count(rule, "count")),
            ));
        }
        let exact = &self.result["exact"];
        let emitted = match exact["emittedBytes"].as_u64() {
            Some(bytes) => plural(bytes as usize, "emitted byte", "emitted bytes"),
            None => "emitted bytes unavailable".to_string(),
        };
        Some(format!(
            "{emitted}, {}, {}, {}",
            plural(count(exact, "programActions"), "action", "actions"),
            plural(count(exact, "programRules"), "rule", "rules"),
            plural(count(exact, "waitActions"), "wait", "waits"),
        ))
    }
    fn body(&self, ctx: &RenderContext<'_>) {
        if self.result.get("rule").is_some_and(|rule| rule.is_object()) {
            cost_rule_body(&self.result, ctx);
            return;
        }
        let exact = &self.result["exact"];
        heading(ctx, "Generated resources (exact)");
        match exact["emittedBytes"].as_u64() {
            Some(bytes) => line(ctx, 1, &format!("emitted bytes: {bytes}")),
            None => line(
                ctx,
                1,
                &format!(
                    "emitted bytes: unavailable — {}",
                    exact["emittedBytesUnavailable"]
                        .as_str()
                        .unwrap_or("the emitter does not model this program")
                ),
            ),
        }
        line(
            ctx,
            1,
            &format!("actions: {}", count(exact, "programActions")),
        );
        line(ctx, 1, &format!("rules: {}", count(exact, "programRules")));
        line(ctx, 1, &format!("waits: {}", count(exact, "waitActions")));

        let elements_value = &self.result["elementCount"];
        if let Some(total) = elements_value["total"].as_u64() {
            line(
                ctx,
                1,
                &format!(
                    "{total} elements of the {}-element budget [exact] — canonical structure, not a statement of client importability",
                    count(elements_value, "budget"),
                ),
            );
            for rule in elements_value["rules"].as_array().into_iter().flatten() {
                line(
                    ctx,
                    2,
                    &format!(
                        "{}  {}{}",
                        elements(count(rule, "count")),
                        rule_name(rule),
                        span_suffix(rule, "  ")
                    ),
                );
            }
            if let Some(note) = elements_value["truncation"]["rules"].as_object() {
                line(
                    ctx,
                    2,
                    &format!(
                        "showing {} of {}; --all to see every item",
                        note["shown"], note["total"]
                    ),
                );
            }
        } else {
            let reason = self.result["elementCountUnavailable"]
                .as_str()
                .unwrap_or("counter does not model this program");
            match elements_value["budget"].as_u64() {
                Some(budget) => line(
                    ctx,
                    1,
                    &format!(
                        "element count unavailable: {reason} — the {budget}-element budget still applies"
                    ),
                ),
                None => line(ctx, 1, &format!("element count unavailable: {reason}")),
            }
        }

        let findings = sorted_findings(&self.result["findings"]);
        finding_section(
            ctx,
            "Static findings",
            &findings,
            withheld(self.selection.as_ref()),
        );
        truncation_notes(ctx, &self.result);
    }
}

fn cost_rule_body(result: &Value, ctx: &RenderContext<'_>) {
    let rule = &result["rule"];
    heading(ctx, "Element cost");
    line(
        ctx,
        1,
        &format!(
            "{} · {}{}",
            rule_name(rule),
            elements(count(rule, "count")),
            span_suffix(rule, " · ")
        ),
    );
    let statements = result["statements"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let by_node = result["byNode"].as_array().map_or(&[][..], Vec::as_slice);
    heading(ctx, "Costliest statements");
    if statements.is_empty() {
        line(ctx, 1, "none");
    }
    for statement in statements {
        line(
            ctx,
            1,
            &format!(
                "{}  {}{}",
                elements(count(statement, "count")),
                statement["name"].as_str().unwrap_or("?"),
                span_suffix(statement, "  ")
            ),
        );
    }
    if let Some((shown, total)) = shown_of(result, "statements") {
        line(
            ctx,
            1,
            &format!("showing {shown} of {total}; --all to see every item"),
        );
    }
    heading(ctx, "By node name");
    if by_node.is_empty() {
        line(ctx, 1, "none");
    }
    for entry in by_node {
        let occurrences = entry["occurrences"].as_u64().unwrap_or(1);
        let times = if occurrences > 1 {
            format!(" ×{occurrences}")
        } else {
            String::new()
        };
        line(
            ctx,
            1,
            &format!(
                "{}  {}{}",
                elements(count(entry, "count")),
                entry["name"].as_str().unwrap_or("?"),
                times,
            ),
        );
    }
    if let Some((shown, total)) = shown_of(result, "byNode") {
        line(
            ctx,
            1,
            &format!("showing {shown} of {total}; --all to see every item"),
        );
    }
}

pub(crate) fn lookup(result: &Value, pres: Presentation) {
    if let Some(unavailable) = result.get("unavailable") {
        eprintln!("{} lookup", tag("UNSUPPORTED", pres.color));
        eprintln!(
            "  {} ({})",
            unavailable["message"].as_str().unwrap_or_default(),
            result["owner"].as_str().unwrap_or_default()
        );
        return;
    }
    let entries = result["entries"].as_array().map_or(&[][..], Vec::as_slice);
    println!("{}", tag("LOOKUP", pres.color).trim_end());
    println!("  {}", plural(entries.len(), "entry", "entries"));
    for entry in entries {
        let spelling = entry["spelling"].as_str().unwrap_or_default();
        let head = entry["signature"].as_str().unwrap_or(spelling);
        let mut line = format!("{head} [{}]", entry["kind"].as_str().unwrap_or_default());
        let identity = entry["identity"].as_str().unwrap_or_default();
        if identity != spelling {
            line.push_str(&format!("  {identity}"));
        }
        if let Some(display) = entry["displayName"].as_str()
            && display != spelling
        {
            line.push_str(&format!("  — {display}"));
        }
        println!("  {line}");
    }
}

pub(crate) fn lookup_error(pres: Presentation, word: &str, code: &str, message: &str) {
    if pres.is_terminal() {
        eprintln!("{} lookup", tag(word, pres.color));
        eprintln!("  {code}: {message}");
    } else {
        eprintln!("wright: {code}: {message}");
    }
}
