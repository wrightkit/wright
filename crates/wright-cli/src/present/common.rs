//! Presentation primitives shared by the terminal, plain, and GitHub
//! renderers: counting, grouping, and small formatting helpers that each
//! renderer used to carry its own copy of.

use std::collections::BTreeSet;

use serde_json::Value;
use wright_driver::{FindingMeta, Severity};

use super::addresses::edge_caller;
use super::{
    array_len, display_location_path, finding_location_span, inline_location, is_real_source_path,
};

/// Rank a typed severity for ordering: errors first (#1001).
pub(crate) fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    }
}

/// Severity counts over a typed finding-meta list — the renderers'
/// severity input; nothing re-parses the serialized `severity` (#1001).
pub(crate) fn meta_severity_counts(meta: &[FindingMeta]) -> [usize; 3] {
    let mut counts = [0usize; 3];
    for meta in meta {
        counts[severity_rank(meta.severity) as usize] += 1;
    }
    counts
}

/// The severity label a finding displays — colored when the presentation
/// is — from the typed severity (#1001); the serialized label is never
/// re-parsed.
pub(crate) fn severity_label(severity: Severity, color: bool) -> String {
    let label = severity.as_str();
    if color {
        let code = match severity {
            Severity::Error => "31",
            Severity::Warning => "33",
            Severity::Info => "36",
        };
        format!("\x1b[{code}m{label}\x1b[0m")
    } else {
        label.to_string()
    }
}

const DIAGNOSTIC_LIMIT: usize = 20;

/// Diagnostics in severity order. Unless `all` is set or the command streams
/// an artifact, at most [`DIAGNOSTIC_LIMIT`] are shown; the total is returned
/// when some were withheld.
pub(crate) fn shown_diagnostics<'a>(
    command: &str,
    diagnostics: &'a [wright_driver::Diagnostic],
    all: bool,
) -> (Vec<&'a wright_driver::Diagnostic>, Option<usize>) {
    let mut ordered: Vec<_> = diagnostics.iter().collect();
    ordered.sort_by_key(|diagnostic| severity_rank(diagnostic.severity));
    let bounded = !all && !matches!(command, "compile" | "convert");
    if bounded && ordered.len() > DIAGNOSTIC_LIMIT {
        let total = ordered.len();
        ordered.truncate(DIAGNOSTIC_LIMIT);
        (ordered, Some(total))
    } else {
        (ordered, None)
    }
}

pub(crate) const DETAIL_COMMANDS: &[&str] = &[
    "wright inspect symbols",
    "wright inspect refs <NAME>",
    "wright inspect cfg <RULE>",
    "wright inspect callgraph",
    "wright inspect cost",
];

pub(crate) fn diagnostic_counts(diagnostics: &[wright_driver::Diagnostic]) -> [usize; 3] {
    let mut counts = [0usize; 3];
    for diagnostic in diagnostics {
        counts[severity_rank(diagnostic.severity) as usize] += 1;
    }
    counts
}

pub(crate) fn diagnostic_files(diagnostics: &[wright_driver::Diagnostic]) -> BTreeSet<String> {
    diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.span.as_ref())
        .filter(|span| is_real_source_path(&span.path))
        .map(|span| display_location_path(&span.path))
        .collect()
}

pub(crate) fn finding_files(findings: &[serde_json::Value]) -> BTreeSet<&str> {
    findings
        .iter()
        .filter_map(|finding| finding.get("span"))
        .filter_map(|span| span.get("path"))
        .filter_map(serde_json::Value::as_str)
        .filter(|path| is_real_source_path(path))
        .collect()
}

pub(crate) fn fix_status_counts(
    fixes: &[wright_driver::result::LintFixOutcome],
) -> (usize, usize, usize) {
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
    (applied, previewed, refused)
}

pub(crate) fn fix_status_parts(counts: (usize, usize, usize)) -> Vec<String> {
    let (applied, previewed, refused) = counts;
    let mut parts = Vec::new();
    if applied > 0 {
        parts.push(format!("{applied} applied"));
    }
    if previewed > 0 {
        parts.push(format!("{previewed} previewed"));
    }
    if refused > 0 {
        parts.push(format!("{refused} refused"));
    }
    parts
}

/// `Some` `wright lookup` argument suffix when `diagnostic` rejects a name
/// (the `unknown-*` code family minus `unknown-language`), carrying a leading
/// space; `None` otherwise.
pub(crate) fn lookup_hint_args(diagnostic: &wright_driver::Diagnostic) -> Option<String> {
    let rejected = diagnostic
        .code
        .split('.')
        .any(|segment| segment.starts_with("unknown-") && segment != "unknown-language");
    if !rejected {
        return None;
    }
    let source = diagnostic.source.as_ref();
    let args = if source.is_some_and(|source| source.kind == "opy") {
        " --language opy".to_string()
    } else if let Some(locale) = source
        .and_then(|source| source.locale.as_deref())
        .filter(|locale| !locale.eq_ignore_ascii_case("en-US"))
    {
        format!(" --locale {locale}")
    } else {
        String::new()
    };
    Some(args)
}

pub(crate) fn span_suffix(item: &serde_json::Value, sep: &str) -> String {
    item.get("span")
        .and_then(inline_location)
        .map(|location| format!("{sep}{location}"))
        .unwrap_or_default()
}

pub(crate) fn notable_callers<'a>(
    counts: &std::collections::BTreeMap<&'a str, usize>,
) -> Vec<(&'a str, usize)> {
    let mut ranked: Vec<(&'a str, usize)> = counts.iter().map(|(name, n)| (*name, *n)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    ranked.retain(|(_, n)| *n > 1);
    ranked
}

/// `(fan-in, fan-out)` name → edge-count maps for a call graph's `edges`.
pub(crate) fn call_fan_counts(
    edges: &[serde_json::Value],
) -> (
    std::collections::BTreeMap<&str, usize>,
    std::collections::BTreeMap<&str, usize>,
) {
    let mut fan_in = std::collections::BTreeMap::<&str, usize>::new();
    let mut fan_out = std::collections::BTreeMap::<&str, usize>::new();
    for edge in edges {
        *fan_in
            .entry(edge["callee"].as_str().unwrap_or("<unnamed>"))
            .or_default() += 1;
        *fan_out.entry(edge_caller(edge)).or_default() += 1;
    }
    (fan_in, fan_out)
}

pub(crate) fn cfg_edge_count(blocks: &[serde_json::Value]) -> usize {
    blocks
        .iter()
        .map(|block| array_len(&block["successors"]))
        .sum()
}

/// `(loop headers, wait blocks, call sites)` counts for a CFG block list.
pub(crate) fn cfg_block_stats(blocks: &[serde_json::Value]) -> (usize, usize, usize) {
    let loops = blocks
        .iter()
        .filter(|block| matches!(block["kind"].as_str(), Some("while" | "for")))
        .count();
    let waits = blocks
        .iter()
        .filter(|block| block["waits"].as_bool().unwrap_or(false))
        .count();
    let calls: usize = blocks.iter().map(|block| array_len(&block["calls"])).sum();
    (loops, waits, calls)
}

/// Flag names a CFG block carries (`waits`, `calls`); the join differs per renderer.
pub(crate) fn block_flags(block: &serde_json::Value) -> Vec<&'static str> {
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
    flags
}

/// Replaces every finding's `location` string with a synthesized `span` object
/// when it lacks one, so the shared renderers can read `span` uniformly.
pub(crate) fn normalize_finding_spans(findings: &mut serde_json::Value) {
    for finding in findings.as_array_mut().into_iter().flatten() {
        if let Some(span) = finding_location_span(finding) {
            finding["span"] = span;
        }
    }
}

pub(crate) fn shown_of(value: &serde_json::Value, key: &str) -> Option<(u64, u64)> {
    value["truncation"][key].as_object().map(|note| {
        (
            note["shown"].as_u64().unwrap_or(0),
            note["total"].as_u64().unwrap_or(0),
        )
    })
}

/// A result's finding rows paired with their typed severities (#1001),
/// sorted most severe first; `meta` supplies every severity, so no
/// renderer re-parses the serialized row.
pub(crate) fn sorted_findings<'f>(
    findings: &'f Value,
    meta: &'f [FindingMeta],
) -> Vec<(&'f Value, Severity)> {
    let mut pairs: Vec<(&Value, Severity)> = findings
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, row)| {
            (
                row,
                meta.get(index)
                    .map(|meta| meta.severity)
                    .unwrap_or(Severity::Warning),
            )
        })
        .collect();
    debug_assert_eq!(pairs.len(), meta.len(), "finding meta parallels the rows");
    pairs.sort_by_key(|(_, severity)| severity_rank(*severity));
    pairs
}

/// Consecutive severity-paired finding rows sharing `code` and `message`
/// group together, keeping each row's typed severity (#1001).
pub(crate) fn grouped_findings<'f>(
    findings: &'f [(&'f Value, Severity)],
) -> Vec<&'f [(&'f Value, Severity)]> {
    let mut groups = Vec::new();
    let mut index = 0;
    while index < findings.len() {
        let (first, _) = findings[index];
        let mut end = index + 1;
        while end < findings.len()
            && findings[end].0["code"] == first["code"]
            && findings[end].0["message"] == first["message"]
        {
            end += 1;
        }
        groups.push(&findings[index..end]);
        index = end;
    }
    groups
}

/// One plain-format `finding: …` record — the severity arrives typed
/// (#1001); every other field reads the serialized row verbatim.
pub(crate) fn finding_record(finding: &Value, severity: Severity) -> String {
    let severity = severity.as_str();
    let code = finding["code"].as_str().unwrap_or("finding");
    let repeated = finding.get("repeated").filter(|r| r.is_object());
    let mut items = vec![severity.to_string(), code.to_string()];
    if let Some(count) = repeated.and_then(|r| r["count"].as_u64()) {
        items.push(format!("count={count}"));
    }
    if let Some(span) = finding.get("span").filter(|span| span.is_object()) {
        if let Some(at) =
            crate::present::inline_location(span).map(|location| format!("at={location}"))
        {
            items.push(at);
        }
    } else if let Some(location) = finding["location"].as_str() {
        items.push(format!("at={location}"));
    }
    if let Some(repeated) = repeated {
        if let Some(span_kind) = repeated["spanKind"].as_str() {
            items.push(crate::plain::field("spanKind", span_kind));
        }
        if let Some(expression) = repeated["expression"].as_str() {
            items.push(crate::plain::field("expression", expression));
        }
    }
    const CARRIED: &[&str] = &[
        "code", "severity", "message", "span", "location", "fix", "repeated",
    ];
    if let Some(object) = finding.as_object() {
        for (key, value) in object {
            if CARRIED.contains(&key.as_str()) {
                continue;
            }
            match value {
                Value::String(text) => items.push(crate::plain::field(key, text)),
                Value::Number(_) | Value::Bool(_) => items.push(format!("{key}={value}")),
                _ => {}
            }
        }
    }
    if let Some(message) = finding["message"].as_str() {
        items.push(crate::plain::field("message", message));
    }
    format!("finding: {}", items.join(" "))
}

/// The one evidence class every finding shares, when they all agree.
pub(crate) fn uniform_evidence(findings: &[&Value]) -> Option<String> {
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
