//! Human and machine rendering of driver result envelopes.
//!
//! Human-readable text and machine-readable JSON are two presentations of the
//! same typed result model: the CLI never formats results independently of the
//! driver's contract. JSON mode prints one pretty-printed `wright-result/v1`
//! envelope to stdout and keeps stderr empty; text mode writes the command
//! result to stdout and diagnostics to stderr.

use wright_driver::Severity;
use wright_driver::config::OutputFormat;
use wright_driver::result::Envelope;

/// Render one result envelope in the requested format.
pub(crate) fn render<T: serde::Serialize>(envelope: &Envelope<T>, format: OutputFormat) {
    match format {
        OutputFormat::Json => {
            let value = serde_json::to_value(envelope).expect("envelope serializes");
            let text = serde_json::to_string_pretty(&value).expect("envelope serializes");
            println!("{text}");
        }
        OutputFormat::Text => render_text(envelope),
    }
}

fn render_text<T: serde::Serialize>(envelope: &Envelope<T>) {
    for diagnostic in &envelope.diagnostics {
        render_diagnostic(diagnostic);
    }
    if !envelope.ok {
        if envelope.diagnostics.is_empty() {
            eprintln!("{}: failed", envelope.command);
        }
        return;
    }

    match envelope.command.as_str() {
        "compile" => render_compile(envelope),
        "check" => println!("check: ok"),
        "analyze" => render_analyze(envelope),
        "inspect" => render_inspect(envelope),
        other => println!("{other}: ok"),
    }
}

fn render_compile<T: serde::Serialize>(envelope: &Envelope<T>) {
    let value = serde_json::to_value(envelope).expect("envelope serializes");
    let Some(output) = value.pointer("/result/output") else {
        eprintln!("compile: failed (no output produced)");
        return;
    };
    let text = output
        .get("text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    // The compiled artifact is the command result: write it verbatim.
    print!("{text}");
    if !text.ends_with('\n') {
        println!();
    }
}

fn render_analyze<T: serde::Serialize>(envelope: &Envelope<T>) {
    let value = serde_json::to_value(envelope).expect("envelope serializes");
    let findings = value
        .pointer("/result/findings")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    println!(
        "analyze: {} finding(s), {} diagnostic(s)",
        findings.len(),
        envelope.diagnostics.len()
    );
    for finding in &findings {
        let code = finding
            .get("code")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("finding");
        let severity = finding
            .get("severity")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("info");
        let message = finding
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        println!("  {severity}[{code}]: {message}");
        if let Some(span) = finding.get("span") {
            print_span(span, "      ");
        }
    }
}

fn render_inspect<T: serde::Serialize>(envelope: &Envelope<T>) {
    let value = serde_json::to_value(envelope).expect("envelope serializes");
    let program = value
        .pointer("/result/program")
        .cloned()
        .unwrap_or_default();
    let rules = value
        .pointer("/result/rules")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let symbols = value
        .pointer("/result/symbols")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();

    let rules_count = program
        .get("rules")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    println!(
        "inspect: {rules_count} rule(s), {} symbol(s)",
        symbols.len()
    );
    for rule in &rules {
        let id = rule
            .get("id")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let name = rule
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("<unnamed>");
        println!("  rule {id}: \"{name}\"");
    }
    for symbol in &symbols {
        let id = symbol
            .get("id")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let kind = symbol
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("symbol");
        let name = symbol
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("<unnamed>");
        println!("  {kind} {id}: {name}");
    }
}

fn render_diagnostic(diagnostic: &wright_driver::Diagnostic) {
    let severity = match diagnostic.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    };
    eprintln!(
        "{severity}[{}] ({}): {}",
        diagnostic.code,
        diagnostic.stage.as_str(),
        diagnostic.message
    );
    if let Some(span) = &diagnostic.span {
        eprintln!("  --> {}:{}:{}", span.path, span.start.line, span.start.col);
    }
}

fn print_span(span: &serde_json::Value, indent: &str) {
    let path = span
        .get("path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<span>");
    let line = span
        .pointer("/start/line")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let col = span
        .pointer("/start/col")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    println!("{indent}--> {path}:{line}:{col}");
}
