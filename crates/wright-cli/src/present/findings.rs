//! Finding-section rendering for the terminal renderer: the grouped
//! `[severity][code]: message` blocks every finding-driven report shares.
//! Severities arrive typed beside each serialized row (#1001), so nothing
//! here re-parses the wire projection.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;
use wright_driver::Severity;

use super::terminal::{heading, line, note, pad, plural};
use super::{
    RenderContext, display_location_path, grouped_findings, is_real_source_path, severity_label,
    source_context, span_position, uniform_evidence,
};

/// The finding's `span`, synthesized from its `location` string when the
/// payload carries only the legacy spelling.
pub(crate) fn finding_location_span(finding: &Value) -> Option<Value> {
    if let Some(span) = finding.get("span").filter(|v| v.is_object()) {
        return Some(span.clone());
    }
    let mut parts = finding["location"].as_str()?.rsplitn(3, ':');
    let col = parts.next()?.parse::<u64>().ok()?;
    let line = parts.next()?.parse::<u64>().ok()?;
    let path = parts.next()?;
    Some(serde_json::json!({"path":path,"start":{"line":line,"col":col}}))
}

/// The group's spans that point at real source files, in order.
pub(crate) fn renderable_group_spans<'v>(group: &[&'v Value]) -> Vec<&'v Value> {
    group
        .iter()
        .filter_map(|finding| finding.get("span"))
        .filter(|span| span.is_object())
        .filter(|span| {
            span.get("path")
                .and_then(Value::as_str)
                .is_some_and(is_real_source_path)
        })
        .collect()
}

/// Print the `span`'s location line and optional source frame.
pub(crate) fn render_finding_location(
    span: &Value,
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

/// Resolve the `span`'s reported path to (display, readable) spellings
/// under `source_base` — `None` when the path names no real source file.
pub(crate) fn finding_location(
    span: &Value,
    source_base: Option<&Path>,
) -> Option<(String, Option<String>)> {
    let path = span.get("path").and_then(Value::as_str)?;
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

/// The group's pseudo-path positions (`<stdin>` etc.) that carry no real
/// source file, as (path, line, col) triples.
pub(crate) fn unmapped_positions(group: &[&Value]) -> Vec<(String, u64, u64)> {
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

/// The `at <path>:line:col …` note covering every unmapped position.
pub(crate) fn unmapped_positions_note(positions: &[(String, u64, u64)]) -> Option<String> {
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

pub(crate) fn lint_footer(ctx: &RenderContext<'_>, findings: &[&Value], skipped: usize) {
    let files: BTreeSet<&str> = findings
        .iter()
        .filter_map(|finding| finding.get("span"))
        .filter_map(|span| span.get("path"))
        .filter_map(serde_json::Value::as_str)
        .filter(|path| is_real_source_path(path))
        .collect();
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

pub(crate) fn finding_head(
    ctx: &RenderContext<'_>,
    finding: &Value,
    severity: Severity,
    occurrences: usize,
) -> String {
    let severity = severity_label(severity, ctx.presentation.color);
    let code = finding["code"].as_str().unwrap_or("finding");
    let message = finding["message"].as_str().unwrap_or_default();
    if occurrences > 1 {
        format!("{severity}[{code}] ({occurrences} findings): {message}")
    } else {
        format!("{severity}[{code}]: {message}")
    }
}

pub(crate) fn finding_location_line(
    ctx: &RenderContext<'_>,
    span: &Value,
    depth: usize,
    frame: bool,
) {
    render_finding_location(span, ctx, &pad(ctx, depth), frame);
}

pub(crate) fn finding_section(
    ctx: &RenderContext<'_>,
    title: &str,
    findings: &[(&Value, Severity)],
    withheld: Option<u64>,
) {
    let rows: Vec<&Value> = findings.iter().map(|(row, _)| *row).collect();
    let evidence = uniform_evidence(&rows);
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
    for group in grouped_findings(findings) {
        let (first, severity) = group[0];
        line(ctx, 1, &finding_head(ctx, first, severity, group.len()));
        let group_rows: Vec<&Value> = group.iter().map(|(row, _)| *row).collect();
        if group.len() == 1 {
            if let Some(span) = first.get("span").filter(|span| span.is_object()) {
                finding_location_line(ctx, span, 2, true);
            }
        } else {
            for span in renderable_group_spans(&group_rows) {
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
        if let Some(unmapped) = unmapped_positions_note(&unmapped_positions(&group_rows)) {
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
