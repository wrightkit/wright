//! Rendering of the execution trace (#1079). The driver records spans;
//! this module writes them to stderr as an indented text tree or as
//! newline-delimited JSON (`wright-trace/v1`).

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
    let mut out = io::stderr().lock();
    let _ = match format {
        OutputFormatArg::Text => write_text(&report, &mut out),
        OutputFormatArg::Json => write_json(&report, &mut out),
    };
}

fn write_text(report: &TraceReport, out: &mut impl Write) -> io::Result<()> {
    writeln!(out, "trace ({SCHEMA})")?;
    for (key, value) in &report.environment {
        writeln!(out, "  {key}: {}", fact_text(value))?;
    }
    for root in report.spans.iter().filter(|span| span.parent.is_none()) {
        write_span_text(report, root, 0, out)?;
    }
    Ok(())
}

fn write_span_text(
    report: &TraceReport,
    span: &SpanReport,
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
        write!(out, " {key}={}", fact_text(value))?;
    }
    writeln!(out)?;
    for child in report
        .spans
        .iter()
        .filter(|candidate| candidate.parent == Some(span.id))
    {
        write_span_text(report, child, depth + 1, out)?;
    }
    Ok(())
}

fn write_json(report: &TraceReport, out: &mut impl Write) -> io::Result<()> {
    let environment: Map<String, Value> = report
        .environment
        .iter()
        .map(|(key, value)| ((*key).to_string(), fact_json(value)))
        .collect();
    write_line(
        out,
        &json!({ "type": "environment", "schema": SCHEMA, "facts": environment }),
    )?;
    for span in &report.spans {
        let facts: Map<String, Value> = span
            .facts
            .iter()
            .map(|(key, value)| ((*key).to_string(), fact_json(value)))
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

fn fact_text(fact: &Fact) -> String {
    match fact {
        Fact::Count(count) => count.to_string(),
        Fact::Text(text) => text.clone(),
    }
}

fn fact_json(fact: &Fact) -> Value {
    match fact {
        Fact::Count(count) => json!(count),
        Fact::Text(text) => json!(text),
    }
}
