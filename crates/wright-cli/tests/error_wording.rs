//! Selector and usage errors name the working command (#888): an extra
//! positional names the flag that narrows the query, an unknown
//! rule/symbol names the listing operation in the surface's own spelling
//! (`wright inspect rules` on the CLI, the `rules` operation through
//! `wright serve`), the refusal lists declared names bounded with the
//! total, an ambiguous rule lists every candidate's `NAME#N` address and
//! `path:line:column`, a `file` selector matches exact file spellings
//! only, and an inapplicable flag combination names the applicable form.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::json;

fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

fn temp_file(name: &str, content: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-wording-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(wright())
        .args(args)
        .stdin(Stdio::null())
        .env_remove("CI")
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITHUB_STEP_SUMMARY")
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR")
        .output()
        .expect("wright runs")
}

fn command_result(output: &std::process::Output) -> String {
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn parse_json(output: &[u8]) -> serde_json::Value {
    serde_json::from_slice(output).expect("stdout is one JSON envelope")
}

fn serve(input: &Path, lines: &[String]) -> Vec<serde_json::Value> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let cwd = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "wording-serve-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&cwd).unwrap();
    let mut child = Command::new(wright())
        .args(["serve", "--transport", "stdio"])
        .arg(input)
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wright serve spawns");
    let mut stdin = child.stdin.take().unwrap();
    for line in lines {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&cwd);
    assert!(
        output.status.success(),
        "wright serve failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON response"))
        .collect()
}

/// Two rules named `tick` — enough to exercise the ambiguous and the
/// declared-name listing paths.
const DUP_RULES: &str = r#"rule ("tick") {
    event {
        Ongoing - Global;
    }
    actions {
        Wait(0.016, Abort When False);
    }
}
rule ("tick") {
    event {
        Ongoing - Global;
    }
    actions {
        Wait(0.032, Abort When False);
    }
}
"#;

/// `N` rules named `rule01`… — a declared-name set beyond the ~20 bound,
/// generated rather than checked in (#888).
fn many_rules(count: usize) -> String {
    (1..=count)
        .map(|index| {
            format!(
                "rule (\"rule{index:02}\") {{\n    event {{\n        Ongoing - Global;\n    }}\n}}\n"
            )
        })
        .collect()
}

// AC1: `inspect cost "<name>" <path>` refuses at usage level and names
// the flag that narrows the query.
#[test]
fn extra_positional_names_the_narrowing_flag() {
    let path = temp_file("tick.ws", DUP_RULES);
    let output = run(&["inspect", "cost", "\"tick\"", path.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2), "{}", command_result(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--rule"),
        "the usage error names --rule: {stderr}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

// AC2/AC3: an unknown `--rule` with no near match is `invalid-selection`
// naming `wright inspect rules` — never the agent-surface operation
// wording — and listing the declared names, bounded past ~20 with the
// total and the unbounded spelling.
#[test]
fn unknown_rule_names_the_cli_list_command_and_declared_names() {
    // Three declared names all fit the listing bound.
    let small = temp_file(
        "small.ws",
        r#"rule ("alpha") {
    event {
        Ongoing - Global;
    }
}
rule ("beta") {
    event {
        Ongoing - Global;
    }
}
rule ("gamma") {
    event {
        Ongoing - Global;
    }
}
"#,
    );
    let output = run(&[
        "inspect",
        "cost",
        small.to_str().unwrap(),
        "--rule",
        "zzz-no-such-rule",
        "-f",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["diagnostics"][0]["code"], "invalid-selection");
    let message = envelope["diagnostics"][0]["message"].as_str().unwrap();
    assert!(
        message.contains("wright inspect rules"),
        "CLI names its own command: {message}"
    );
    assert!(
        !message.contains("the 'rules' operation"),
        "no agent-surface spelling on the CLI: {message}"
    );
    for name in ["'alpha'", "'beta'", "'gamma'"] {
        assert!(message.contains(name), "declared name listed: {message}");
    }
    assert!(
        message.contains("(3 total)"),
        "the total is named: {message}"
    );
    assert!(
        !message.contains("--all"),
        "a complete listing needs no expansion hint: {message}"
    );

    // Past the bound the list truncates, reports the total, and names
    // the unbounded listing.
    let many = temp_file("many.ws", &many_rules(25));
    let output = run(&[
        "inspect",
        "cost",
        many.to_str().unwrap(),
        "--rule",
        "zzz-no-such-rule",
        "-f",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let message = parse_json(&output.stdout)["diagnostics"][0]["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        message.contains("(25 total)"),
        "the total is named: {message}"
    );
    assert!(
        message.contains("'rule20'"),
        "the bound lists 20: {message}"
    );
    assert!(
        !message.contains("'rule21'"),
        "the bound withholds the rest: {message}"
    );
    assert!(
        message.contains("wright inspect rules --all"),
        "the expansion hint names the unbounded form: {message}"
    );

    let _ = std::fs::remove_dir_all(small.parent().unwrap());
    let _ = std::fs::remove_dir_all(many.parent().unwrap());
}

// AC4: the same unknown rule through `wright serve` names the `rules`
// operation, not the CLI command — one driver result, two renderings.
#[test]
fn serve_names_the_operation_not_the_command() {
    let path = temp_file("tick.ws", DUP_RULES);
    let responses = serve(
        &path,
        &[
            json!({"op":"costEstimate","rule":"zzz-no-such-rule"}).to_string(),
            json!({"op":"cfg","rule":"zzz-no-such-rule"}).to_string(),
        ],
    );
    assert_eq!(responses[0]["error"]["code"], "invalid-selection");
    assert_eq!(responses[1]["error"]["code"], "unknown-rule");
    for response in &responses {
        let message = response["error"]["message"].as_str().unwrap();
        assert!(
            message.contains("the 'rules' operation"),
            "serve keeps operation names: {message}"
        );
        assert!(
            !message.contains("wright inspect"),
            "no CLI spelling on the agent surface: {message}"
        );
        assert!(
            message.contains("'tick'"),
            "the declared name is listed: {message}"
        );
    }
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

// AC5: `inspect cost --help` and `inspect --help` each show one
// narrowed-query example that takes no INPUT.
#[test]
fn help_shows_inputless_narrowed_examples() {
    for args in [vec!["inspect", "cost", "--help"], vec!["inspect", "--help"]] {
        let output = run(&args);
        assert!(output.status.success(), "{args:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let example = stdout
            .lines()
            .find(|line| line.contains("wright inspect") && line.contains("--rule"))
            .unwrap_or_else(|| panic!("{args:?} shows a narrowed example: {stdout}"));
        assert!(
            !example.contains("INPUT") && !example.ends_with(".ws"),
            "the narrowed example omits INPUT: {example}"
        );
    }
}

// AC6: duplicate rule names refuse with every candidate's `NAME#N`
// address and `path:line:column` — a file index is not a path.
#[test]
fn ambiguous_rule_lists_candidates_with_locations() {
    let path = temp_file("dup.ws", DUP_RULES);
    let output = run(&[
        "inspect",
        "cost",
        path.to_str().unwrap(),
        "--rule",
        "tick",
        "-f",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let message = parse_json(&output.stdout)["diagnostics"][0]["message"]
        .as_str()
        .unwrap()
        .to_string();
    for candidate in ["tick#1 at dup.ws:", "tick#2 at dup.ws:"] {
        assert!(
            message.contains(candidate),
            "the candidate carries path:line:column: {message}"
        );
    }
    assert!(
        !message.contains("{file:") && !message.contains(" at 0:"),
        "no placeholder or bare file index: {message}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

// AC7: a `file` selector naming no source of the loaded program is
// `invalid-selection` listing the program's source files.
#[test]
fn unmatched_file_lists_project_sources() {
    let path = temp_file("main.ws", DUP_RULES);
    let output = run(&[
        "inspect",
        "symbols",
        path.to_str().unwrap(),
        "--file",
        "nope.ws",
        "-f",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["diagnostics"][0]["code"], "invalid-selection");
    let message = envelope["diagnostics"][0]["message"].as_str().unwrap();
    assert!(
        message.contains("nope.ws") && message.contains("main.ws"),
        "the refusal lists the program's source files: {message}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

// AC8: file matching is exact identity — a bare basename never
// suffix-matches `src/lib.ws` under the project root.
#[test]
fn file_selector_has_no_suffix_matching() {
    let dir = std::env::temp_dir().join(format!("wright-wording-project-{}", std::process::id()));
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let input = src.join("lib.ws");
    std::fs::write(&input, DUP_RULES).unwrap();

    let output = run(&[
        "inspect",
        "symbols",
        input.to_str().unwrap(),
        "--root",
        dir.to_str().unwrap(),
        "--file",
        "lib.ws",
        "-f",
        "json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "no suffix match: {}",
        command_result(&output)
    );
    assert_eq!(
        parse_json(&output.stdout)["diagnostics"][0]["code"],
        "invalid-selection"
    );

    // The project-root-relative spelling names the same file exactly.
    let output = run(&[
        "inspect",
        "symbols",
        input.to_str().unwrap(),
        "--root",
        dir.to_str().unwrap(),
        "--file",
        "src/lib.ws",
        "-f",
        "json",
    ]);
    assert!(
        output.status.success(),
        "src/lib.ws selects the file: {}",
        command_result(&output)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// AC9: `--rule` combined with finding selection is a usage error naming
// the applicable forms.
#[test]
fn inapplicable_combination_names_the_applicable_form() {
    let path = temp_file("tick.ws", DUP_RULES);
    let path = path.to_str().unwrap();
    let output = run(&[
        "inspect", "cost", path, "--rule", "tick#1", "--file", "tick.ws",
    ]);
    assert_eq!(output.status.code(), Some(2), "{}", command_result(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--rule") && stderr.contains("--file") && stderr.contains("without --rule"),
        "the usage error names the applicable forms: {stderr}"
    );

    // The same `--file` without `--rule` keeps reporting the file's
    // static findings — the combination, not the flag, is refused.
    let output = run(&["inspect", "cost", path, "--file", "tick.ws", "-f", "json"]);
    assert!(
        output.status.success(),
        "--file alone still selects findings: {}",
        command_result(&output)
    );

    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

// Caller input reaches `message` verbatim (#888): diagnostics carry no
// executable placeholders, so a `--file` value literally spelling
// `{file:N}`/`{list:OP}` stays text — never a substitution, never a
// rendered command.
#[test]
fn placeholder_spelling_file_argument_stays_literal() {
    let path = temp_file("main.ws", DUP_RULES);
    for argument in ["{file:0}", "{list:rules}"] {
        let output = run(&[
            "inspect",
            "symbols",
            path.to_str().unwrap(),
            "--file",
            argument,
            "-f",
            "json",
        ]);
        assert_eq!(output.status.code(), Some(1), "{}", command_result(&output));
        let envelope = parse_json(&output.stdout);
        assert_eq!(envelope["diagnostics"][0]["code"], "invalid-selection");
        let message = envelope["diagnostics"][0]["message"].as_str().unwrap();
        assert!(
            message.contains(&format!("'{argument}'")),
            "the literal argument text: {message}"
        );
        assert!(
            !message.contains("wright inspect rules"),
            "the placeholder spelling did not render a command: {message}"
        );
    }
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

// The print-and-pass-back contract (ADR-0035, #888): a rule literally
// named `a{list:rules}b` prints its `NAME#N` addresses verbatim — the
// brace string inside the name never renders — and feeding a printed
// address back selects that rule.
#[test]
fn brace_named_rule_round_trips_through_its_address() {
    let path = temp_file(
        "brace.ws",
        r#"rule ("a{list:rules}b") {
    event {
        Ongoing - Global;
    }
}
rule ("a{list:rules}b") {
    event {
        Ongoing - Global;
    }
}
"#,
    );
    let output = run(&[
        "inspect",
        "cost",
        path.to_str().unwrap(),
        "--rule",
        "a{list:rules}b",
        "-f",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(1), "{}", command_result(&output));
    let message = parse_json(&output.stdout)["diagnostics"][0]["message"]
        .as_str()
        .unwrap()
        .to_string();
    for candidate in ["a{list:rules}b#1", "a{list:rules}b#2"] {
        assert!(
            message.contains(candidate),
            "the candidate address prints verbatim: {message}"
        );
    }
    assert!(
        !message.contains("wright inspect rules"),
        "the placeholder spelling inside the name did not render: {message}"
    );

    let output = run(&[
        "inspect",
        "cost",
        path.to_str().unwrap(),
        "--rule",
        "a{list:rules}b#1",
        "-f",
        "json",
    ]);
    assert!(
        output.status.success(),
        "the printed address selects the rule: {}",
        command_result(&output)
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

// The refusal metadata is structured (#888): `message` is fully
// rendered text and the candidates/listing operation live in sibling
// fields a client can consume programmatically.
#[test]
fn refusal_fields_are_structured_metadata() {
    let path = temp_file("dup.ws", DUP_RULES);

    let output = run(&[
        "inspect",
        "cost",
        path.to_str().unwrap(),
        "--rule",
        "tick",
        "-f",
        "json",
    ]);
    let envelope = parse_json(&output.stdout);
    let diagnostic = &envelope["diagnostics"][0];
    assert_eq!(diagnostic["code"], "invalid-selection");
    let candidates = diagnostic["candidates"].as_array().unwrap();
    for (index, candidate) in candidates.iter().enumerate() {
        assert_eq!(
            candidate["address"].as_str().unwrap(),
            format!("tick#{}", index + 1)
        );
        assert_eq!(candidate["path"].as_str().unwrap(), "dup.ws");
        assert!(candidate["line"].is_u64() && candidate["column"].is_u64());
    }

    let output = run(&[
        "inspect",
        "cost",
        path.to_str().unwrap(),
        "--rule",
        "zzz-no-such-rule",
        "-f",
        "json",
    ]);
    let envelope = parse_json(&output.stdout);
    let diagnostic = &envelope["diagnostics"][0];
    assert_eq!(diagnostic["listOp"], "rules");
    assert_eq!(diagnostic["name"], "zzz-no-such-rule");
    assert_eq!(diagnostic["declaredTotal"], 1);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

// `check --rule-id` reaches `report_error` rather than the envelope
// pass (#888): caller text spelling a placeholder still prints
// verbatim — no doubled braces, no rendered command.
#[test]
fn check_rule_id_placeholder_spelling_stays_literal() {
    let path = temp_file("tick.ws", DUP_RULES);
    let output = run(&["check", path.to_str().unwrap(), "--rule-id", "{list:x}"]);
    assert_eq!(output.status.code(), Some(2), "{}", command_result(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("{list:x}"),
        "the literal text prints verbatim: {stderr}"
    );
    assert!(
        !stderr.contains("{{list:x}}") && !stderr.contains("wright inspect"),
        "no doubled braces or rendered command: {stderr}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

// File identities spell like anything (#888): a source file literally
// named `{list:rules}x.ws` or `w{{i}}d.ws` selects verbatim and lists
// verbatim — the name never renders and never collapses.
#[test]
fn brace_spelling_source_files_stay_literal() {
    for name in ["{list:rules}x.ws", "w{{i}}d.ws"] {
        let path = temp_file(name, DUP_RULES);
        let output = run(&[
            "inspect",
            "symbols",
            path.to_str().unwrap(),
            "--file",
            "nope.ws",
            "-f",
            "json",
        ]);
        assert_eq!(output.status.code(), Some(1), "{}", command_result(&output));
        let message = parse_json(&output.stdout)["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(
            message.contains(name),
            "the file spelling lists verbatim: {message}"
        );
        assert!(
            !message.contains("wright inspect"),
            "the file spelling did not render a command: {message}"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}

// `editStatements` resolves its rule addresses against the caller's
// `sources` program, not the served snapshot (#888): a rule literally
// named `a{file:0}b` prints its `NAME#N` candidates verbatim — the
// placeholder spelling is text, never a file substitution.
#[test]
fn edit_statements_resolves_candidates_against_supplied_sources() {
    let path = temp_file("main.ws", DUP_RULES);
    let supplied = r#"rule ("a{file:0}b") {
    event {
        Ongoing - Global;
    }
    actions {
        Wait(0.016, Abort When False);
    }
}
rule ("a{file:0}b") {
    event {
        Ongoing - Global;
    }
    actions {
        Wait(0.032, Abort When False);
    }
}
"#;
    // `inputIdentity` is the freshness precondition on the program the
    // addresses name — the supplied `sources` text's own identity here.
    let identity = wright_driver::input_identity(supplied);
    let responses = serve(
        &path,
        &[json!({
            "op": "editStatements",
            "inputIdentity": identity,
            "sources": {path.to_str().unwrap(): supplied},
            "edits": [{"rule": "a{file:0}b", "list": "actions", "index": 0, "change": "delete"}]
        })
        .to_string()],
    );
    let error = &responses[0]["error"];
    assert_eq!(error["code"], "ambiguous-rule", "{responses:?}");
    let message = error["message"].as_str().unwrap();
    for candidate in ["a{file:0}b#1", "a{file:0}b#2"] {
        assert!(
            message.contains(candidate),
            "the candidate prints verbatim: {message}"
        );
    }
    assert!(
        !message.contains("{{") && !message.contains("wright inspect"),
        "no doubled braces or rendered command: {message}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
