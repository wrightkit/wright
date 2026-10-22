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
        "no unrendered marker or bare file index: {message}"
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
