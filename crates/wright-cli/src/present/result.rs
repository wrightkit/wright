use wright_driver::Severity;
use wright_driver::result::{
    CallGraphResult, CfgResult, CheckResult, CompileResult, ConvertResult, CostResult,
    EssentialResult, FindingMeta, LintResult, RefsResult, RulesResult, SymbolsResult,
};

use super::terminal::{heading, line};
use super::{
    RenderContext, SummaryStatus, cfg_edge_count, count, fix_status_counts, fix_status_parts,
    meta_severity_counts, query_items, rule_name,
};

pub(crate) fn render_next<'a>(
    ctx: &RenderContext<'_>,
    commands: impl Iterator<Item = Option<&'a str>>,
) {
    let mut commands = commands.peekable();
    if commands.peek().is_some() {
        heading(ctx, "Next");
        for command in commands.flatten() {
            line(ctx, 1, command);
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

fn findings_with_severity<'a>(
    findings: &'a serde_json::Value,
    meta: &'a [FindingMeta],
) -> impl Iterator<Item = (&'a serde_json::Value, Severity)> {
    findings
        .as_array()
        .into_iter()
        .flatten()
        .zip(meta)
        .map(|(finding, meta)| (finding, meta.severity))
}

pub(crate) trait ResultPresentation {
    fn next_commands(&self) -> impl Iterator<Item = &str> {
        std::iter::empty()
    }
    fn metadata(&self) -> Option<String> {
        None
    }
    /// Text the command streams to stdout in place of a report.
    fn artifact(&self) -> Option<&str> {
        None
    }
    /// The result's findings paired with their typed severities (#1001).
    fn github_findings(&self) -> impl Iterator<Item = (&serde_json::Value, Severity)> {
        std::iter::empty()
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

impl ResultPresentation for CheckResult {
    fn next_commands(&self) -> impl Iterator<Item = &str> {
        self.next.iter().map(String::as_str)
    }
}

impl ResultPresentation for LintResult {
    fn next_commands(&self) -> impl Iterator<Item = &str> {
        self.next.iter().map(String::as_str)
    }

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
    fn github_findings(&self) -> impl Iterator<Item = (&serde_json::Value, Severity)> {
        findings_with_severity(&self.findings, &self.findings_meta)
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
    fn next_commands(&self) -> impl Iterator<Item = &str> {
        self.result["next"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
    }

    fn github_findings(&self) -> impl Iterator<Item = (&serde_json::Value, Severity)> {
        let key = match self.command {
            wright_driver::result::EssentialCommand::Analyze => "risks",
            _ => "findings",
        };
        findings_with_severity(&self.result[key], &self.findings_meta)
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

impl ResultPresentation for RulesResult {
    fn metadata(&self) -> Option<String> {
        Some(format!("{} rule(s)", query_items(&self.0, "rules").len()))
    }
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
    fn github_findings(&self) -> impl Iterator<Item = (&serde_json::Value, Severity)> {
        findings_with_severity(&self.result["findings"], &self.findings_meta)
    }
    fn max_finding_severity(&self) -> Option<Severity> {
        max_finding_severity(&self.findings_meta, &self.selection)
    }
}
