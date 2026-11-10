//! Rendering of the execution trace (#1079). The driver records spans;
//! this module writes them to stderr as an indented text tree or as
//! newline-delimited JSON (`wright-trace/v1`). Text facts are written with
//! the user's home directory prefix replaced by `~`, so a trace can be pasted
//! into a public issue without naming the local account.

use std::io::{self, Write};

use serde_json::{Map, Value, json};
use wright_driver::{Fact, SpanReport, Trace, TraceReport};

use crate::cli::OutputFormatArg;

const SCHEMA: &str = "wright-trace/v1";

/// A trace for `format`, or a disabled one when no format was requested.
/// Environment facts that the CLI owns are recorded here.
pub(crate) fn start(format: Option<OutputFormatArg>) -> Trace {
    let trace = match format {
        Some(_) => Trace::enabled(),
        None => Trace::disabled(),
    };
    trace.set_fact("wrightVersion", env!("CARGO_PKG_VERSION"));
    trace.set_fact("os", std::env::consts::OS);
    trace.set_fact("arch", std::env::consts::ARCH);
    trace.set_fact("allocator", "mimalloc");
    trace
}

/// Write the trace to stderr in `format`. Nothing is written when no format
/// was requested. Write failures are ignored: the trace never changes the
/// command's outcome.
pub(crate) fn print(trace: &Trace, format: Option<OutputFormatArg>) {
    let (Some(format), Some(report)) = (format, trace.report()) else {
        return;
    };
    let home = home_dir();
    let mut out = io::stderr().lock();
    let _ = match format {
        OutputFormatArg::Text => write_text(&report, home.as_deref(), &mut out),
        OutputFormatArg::Json => write_json(&report, home.as_deref(), &mut out),
    };
}

fn home_dir() -> Option<String> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .filter(|home| !home.is_empty())
}

fn write_text(report: &TraceReport, home: Option<&str>, out: &mut impl Write) -> io::Result<()> {
    writeln!(out, "trace ({SCHEMA})")?;
    for (key, value) in &report.environment {
        writeln!(out, "  {key}: {}", fact_text(value, home))?;
    }
    for root in report.spans.iter().filter(|span| span.parent.is_none()) {
        write_span_text(report, root, home, 0, out)?;
    }
    Ok(())
}

fn write_span_text(
    report: &TraceReport,
    span: &SpanReport,
    home: Option<&str>,
    depth: usize,
    out: &mut impl Write,
) -> io::Result<()> {
    let label = format!("{}{}", "  ".repeat(depth), span.name);
    write!(
        out,
        "{label:<44} +{:>10.3} ms {:>10.3} ms {}",
        millis(span.start_ns),
        millis(span.duration_ns),
        if span.failed { "error" } else { "ok" },
    )?;
    for (key, value) in &span.facts {
        write!(out, " {key}={}", fact_text(value, home))?;
    }
    writeln!(out)?;
    for child in report
        .spans
        .iter()
        .filter(|candidate| candidate.parent == Some(span.id))
    {
        write_span_text(report, child, home, depth + 1, out)?;
    }
    Ok(())
}

fn write_json(report: &TraceReport, home: Option<&str>, out: &mut impl Write) -> io::Result<()> {
    let environment: Map<String, Value> = report
        .environment
        .iter()
        .map(|(key, value)| ((*key).to_string(), fact_json(value, home)))
        .collect();
    write_line(
        out,
        &json!({ "type": "environment", "schema": SCHEMA, "facts": environment }),
    )?;
    for span in &report.spans {
        let facts: Map<String, Value> = span
            .facts
            .iter()
            .map(|(key, value)| ((*key).to_string(), fact_json(value, home)))
            .collect();
        write_line(
            out,
            &json!({
                "type": "span",
                "id": span.id,
                "parent": span.parent,
                "name": span.name,
                "startNs": span.start_ns,
                "durationNs": span.duration_ns,
                "status": if span.failed { "error" } else { "ok" },
                "facts": facts,
            }),
        )?;
    }
    Ok(())
}

fn write_line(out: &mut impl Write, value: &Value) -> io::Result<()> {
    writeln!(out, "{value}")
}

fn millis(nanos: u64) -> f64 {
    nanos as f64 / 1_000_000.0
}

/// Replace each occurrence of `home` that ends at a path boundary with `~`.
/// `/Users/al` does not match inside `/Users/alice`.
fn redact_home(text: &str, home: Option<&str>) -> String {
    let Some(home) = home else {
        return text.to_string();
    };
    let mut redacted = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find(home) {
        let after = &rest[index + home.len()..];
        redacted.push_str(&rest[..index]);
        if after.is_empty() || after.starts_with(['/', '\\']) {
            redacted.push('~');
        } else {
            redacted.push_str(home);
        }
        rest = after;
    }
    redacted.push_str(rest);
    redacted
}

fn fact_text(fact: &Fact, home: Option<&str>) -> String {
    match fact {
        Fact::Count(count) => count.to_string(),
        Fact::Text(text) => redact_home(text, home),
    }
}

fn fact_json(fact: &Fact, home: Option<&str>) -> Value {
    match fact {
        Fact::Count(count) => json!(count),
        Fact::Text(text) => json!(redact_home(text, home)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_prefix_is_replaced_only_at_a_path_boundary() {
        let home = Some("/Users/alice");
        assert_eq!(
            redact_home("/Users/alice/Library/opy-provider", home),
            "~/Library/opy-provider"
        );
        assert_eq!(redact_home("/Users/alice", home), "~");
        assert_eq!(redact_home("/Users/alicia/x", home), "/Users/alicia/x");
        assert_eq!(
            redact_home("C:\\Users\\alice\\x", Some("C:\\Users\\alice")),
            "~\\x"
        );
        assert_eq!(redact_home("/opt/opy", None), "/opt/opy");
    }

    #[test]
    fn rendered_trace_never_names_the_home_directory() {
        let report = TraceReport {
            environment: vec![(
                "providerPath",
                Fact::Text("/Users/alice/Library/Application Support/opy-provider".into()),
            )],
            spans: vec![SpanReport {
                id: 0,
                parent: None,
                name: "compile",
                start_ns: 0,
                duration_ns: 1,
                failed: false,
                facts: vec![("path", Fact::Text("/Users/alice/work/main.opy".into()))],
            }],
        };
        let home = Some("/Users/alice");
        let mut text = Vec::new();
        write_text(&report, home, &mut text).unwrap();
        let mut json_lines = Vec::new();
        write_json(&report, home, &mut json_lines).unwrap();
        for rendered in [text, json_lines] {
            let rendered = String::from_utf8(rendered).unwrap();
            assert!(!rendered.contains("/Users/alice"), "{rendered}");
            assert!(rendered.contains("~/Library/Application Support/opy-provider"));
            assert!(rendered.contains("~/work/main.opy"));
        }
    }
}
