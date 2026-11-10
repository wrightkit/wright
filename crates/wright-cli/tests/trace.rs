use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

const SOURCE: &str = r#"rule ("nested") {
    event { Ongoing - Global; }
    actions {
        Set Global Variable(A, Filtered Array(All Players(All Teams), Is True For Any(Array(Current Array Element, True), Current Array Element)));
    }
}"#;

const BROKEN_SOURCE: &str = r#"rule ("broken") {
    event { Ongoing - Global; }
    actions {
        Set Global Variable(A, ;
    }
}"#;

fn wright(args: &[&str], source: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wright"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn trace_lines(output: &Output) -> Vec<Value> {
    String::from_utf8(output.stderr.clone())
        .unwrap()
        .lines()
        .filter(|line| line.starts_with('{'))
        .map(|line| serde_json::from_str(line).expect("each trace line is one JSON object"))
        .collect()
}

fn spans(lines: &[Value]) -> Vec<&Value> {
    lines.iter().filter(|line| line["type"] == "span").collect()
}

#[test]
fn json_trace_lines_follow_the_documented_shape() {
    let output = wright(
        &[
            "check",
            "-",
            "--kind",
            "workshop",
            "-f",
            "json",
            "--trace=json",
        ],
        SOURCE,
    );
    assert!(output.status.success(), "{:?}", output.status);
    let lines = trace_lines(&output);

    let environment = &lines[0];
    assert_eq!(environment["type"], "environment");
    assert_eq!(environment["schema"], "wright-trace/v1");
    for key in ["wrightVersion", "os", "arch", "allocator"] {
        assert!(environment["facts"][key].is_string(), "missing fact {key}");
    }

    let spans = spans(&lines);
    let root = spans[0];
    assert_eq!(root["name"], "check");
    assert_eq!(root["id"], 0);
    assert!(root["parent"].is_null());
    assert_eq!(root["status"], "ok");
    assert_eq!(root["facts"]["exitCode"], 0);
    for (index, span) in spans.iter().enumerate() {
        assert_eq!(span["id"], index, "ids are start-ordered");
        assert!(span["startNs"].is_u64());
        assert!(span["durationNs"].is_u64());
        assert!(span["facts"].is_object());
        if index > 0 {
            let parent = span["parent"]
                .as_u64()
                .expect("non-root spans have a parent");
            assert!(parent < index as u64, "a parent starts before its child");
        }
    }
    let names: Vec<&str> = spans
        .iter()
        .filter_map(|span| span["name"].as_str())
        .collect();
    for expected in ["catalog.load", "input.resolve", "parse", "validate"] {
        assert!(
            names.contains(&expected),
            "missing span {expected}: {names:?}"
        );
    }
}

#[test]
fn trace_leaves_stdout_and_exit_code_unchanged() {
    for args in [
        vec!["compile", "-", "--kind", "workshop", "-o", "-"],
        vec!["check", "-", "--kind", "workshop", "-f", "json"],
    ] {
        let plain = wright(&args, SOURCE);
        for format in ["--trace", "--trace=json"] {
            let mut traced_args = args.clone();
            traced_args.push(format);
            let traced = wright(&traced_args, SOURCE);
            assert_eq!(traced.stdout, plain.stdout, "stdout for {args:?} {format}");
            assert_eq!(
                traced.status.code(),
                plain.status.code(),
                "{args:?} {format}"
            );
        }
    }
}

#[test]
fn failing_compile_keeps_completed_spans_and_marks_the_failing_one() {
    let output = wright(
        &[
            "compile",
            "-",
            "--kind",
            "workshop",
            "-o",
            "-",
            "--trace=json",
        ],
        BROKEN_SOURCE,
    );
    assert_eq!(output.status.code(), Some(1));
    let lines = trace_lines(&output);
    let spans = spans(&lines);

    let root = spans[0];
    assert_eq!(root["name"], "compile");
    assert_eq!(root["facts"]["exitCode"], 1);
    let by_name = |name: &str| {
        spans
            .iter()
            .find(|span| span["name"] == name)
            .unwrap_or_else(|| panic!("missing span {name}"))
    };
    assert_eq!(by_name("catalog.load")["status"], "ok");
    assert_eq!(by_name("input.resolve")["status"], "ok");
    assert_eq!(by_name("parse")["status"], "error");
}

#[test]
fn text_trace_renders_an_indented_tree_on_stderr() {
    let output = wright(
        &[
            "check",
            "-",
            "--kind",
            "workshop",
            "--trace",
            "--renderer",
            "plain",
        ],
        SOURCE,
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("trace (wright-trace/v1)"), "{stderr}");
    assert!(
        stderr.lines().any(|line| line.starts_with("check ")),
        "{stderr}"
    );
    assert!(
        stderr
            .lines()
            .any(|line| line.starts_with("  catalog.load ")),
        "{stderr}"
    );
}

const SPAN_KEYS: &[&str] = &[
    "durationNs",
    "facts",
    "id",
    "name",
    "parent",
    "startNs",
    "status",
    "type",
];
const ENVIRONMENT_KEYS: &[&str] = &["facts", "schema", "type"];
const ENVIRONMENT_FACT_KEYS: &[&str] = &[
    "allocator",
    "arch",
    "inputKind",
    "lppVersion",
    "locale",
    "os",
    "outputBytes",
    "providerId",
    "providerPath",
    "providerVersion",
    "sourceFiles",
    "wrightVersion",
];

fn sorted_keys(value: &Value) -> Vec<&str> {
    let mut keys: Vec<&str> = value
        .as_object()
        .expect("a JSON object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys
}

#[test]
fn json_trace_lines_have_exactly_the_documented_keys() {
    let output = wright(
        &[
            "check",
            "-",
            "--kind",
            "workshop",
            "-f",
            "json",
            "--trace=json",
        ],
        SOURCE,
    );
    let lines = trace_lines(&output);
    let environment = &lines[0];
    assert_eq!(sorted_keys(environment), ENVIRONMENT_KEYS);
    for key in sorted_keys(&environment["facts"]) {
        assert!(
            ENVIRONMENT_FACT_KEYS.contains(&key),
            "undocumented fact {key}"
        );
    }
    for span in spans(&lines) {
        assert_eq!(sorted_keys(span), SPAN_KEYS);
    }
}

#[test]
fn trace_on_stderr_keeps_the_diagnostics() {
    let output = wright(
        &[
            "check",
            "-",
            "--kind",
            "workshop",
            "--trace=json",
            "--renderer",
            "plain",
            "--color",
            "never",
        ],
        SOURCE,
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.lines().any(|line| line.starts_with("diagnostic:")),
        "diagnostics missing from stderr: {stderr}"
    );
    assert!(stderr.lines().any(|line| line.starts_with('{')), "{stderr}");
}

#[test]
fn trace_off_stderr_has_no_trace_lines_and_is_a_prefix_of_trace_on() {
    for format in [None, Some("json")] {
        let mut base = vec![
            "check",
            "-",
            "--kind",
            "workshop",
            "--renderer",
            "plain",
            "--color",
            "never",
        ];
        if let Some(format) = format {
            base.extend(["-f", format]);
        }
        let off = wright(&base, SOURCE);
        let off_stderr = String::from_utf8(off.stderr).unwrap();
        assert!(
            !off_stderr.lines().any(|line| line.starts_with('{')),
            "{off_stderr}"
        );
        assert!(!off_stderr.contains("trace ("), "{off_stderr}");

        let mut traced_args = base.clone();
        traced_args.push("--trace=json");
        let on = wright(&traced_args, SOURCE);
        let on_stderr = String::from_utf8(on.stderr).unwrap();
        assert!(on_stderr.starts_with(&off_stderr), "format {format:?}");
    }
}

#[test]
fn space_separated_trace_value_is_a_usage_error() {
    let output = wright(
        &["check", "-", "--kind", "workshop", "--trace", "json"],
        SOURCE,
    );
    assert_eq!(output.status.code(), Some(2));
}
