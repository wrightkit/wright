use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn corpus_workshop(fixture_id: &str) -> String {
    std::fs::read_to_string(
        workspace_root()
            .join("tests/fixtures/workshop")
            .join(fixture_id)
            .with_extension("ws"),
    )
    .unwrap()
}

fn fixture_dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wright-plain-test-{}", name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (file, contents) in files {
        std::fs::write(dir.join(file), contents).unwrap();
    }
    dir
}

fn run_in(dir: &Path, args: &[&str]) -> Output {
    run_in_env(dir, args, &[])
}

fn run_in_env(dir: &Path, args: &[&str], variables: &[(&str, &str)]) -> Output {
    let mut command = Command::new(wright());
    command
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .env_remove("CI")
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITHUB_STEP_SUMMARY")
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR");
    for (name, value) in variables {
        command.env(name, value);
    }
    command.output().expect("wright runs")
}

fn transcript(output: &Output) -> String {
    format!(
        "exit {}\n== stdout\n{}\n== stderr\n{}",
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

const MIXED_CHECK: &str = "variables {\n\
     \tglobal:\n\
     \t\t0: B\n\
     }\n\
     \n\
     rule (\"r\") {\n\
     \tevent {\n\
     \t\tOngoing - Global;\n\
     \t}\n\
     \tactions {\n\
     \t\tSet Global Variable(B, FutureValueThing(2));\n\
     \t\tWhile(True);\n\
     \t\t\tSet Global Variable(B, 1);\n\
     \t\tEnd;\n\
     \t}\n\
     }\n";

#[test]
fn check_pass() {
    let dir = fixture_dir(
        "check-pass",
        &[("ok.ws", &corpus_workshop("synthetic/basic-rule"))],
    );
    insta::assert_snapshot!(transcript(&run_in(&dir, &["check", "ok.ws"])));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn check_fail_reports_diagnostics_and_next() {
    let dir = fixture_dir("check-fail", &[("mixed.ws", MIXED_CHECK)]);
    insta::assert_snapshot!(transcript(&run_in(&dir, &["check", "mixed.ws"])));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn check_truncates_the_diagnostic_stream() {
    let mut actions = String::new();
    for index in 0..25 {
        actions.push_str(&format!("        FutureValueThing{index}(1);\n"));
    }
    let source = format!(
        "rule (\"r\") {{\n    event {{\n        Ongoing - Global;\n    }}\n    actions {{\n{actions}    }}\n}}\n"
    );
    let dir = fixture_dir("check-truncated", &[("many.ws", &source)]);
    insta::assert_snapshot!(transcript(&run_in(&dir, &["check", "many.ws"])));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compile_reports_the_output_record() {
    let dir = fixture_dir(
        "compile",
        &[("mode.ws", &corpus_workshop("synthetic/basic-rule"))],
    );
    insta::assert_snapshot!(transcript(&run_in(&dir, &["compile", "mode.ws"])));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lint_reports_finding_records() {
    let dir = fixture_dir(
        "lint",
        &[("flow.ws", &corpus_workshop("synthetic/control-flow"))],
    );
    insta::assert_snapshot!(transcript(&run_in(&dir, &["lint", "flow.ws"])));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lint_selection_reports_selected_and_by_code() {
    let dir = fixture_dir(
        "lint-selection",
        &[("cake.ws", &corpus_workshop("real-world/overpy-cake"))],
    );
    insta::assert_snapshot!(transcript(&run_in(
        &dir,
        &["lint", "cake.ws", "--max", "3"]
    )));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lint_fix_reports_fix_and_patch_records() {
    const FIXABLE: &str = "variables {\n\
         \tglobal:\n\
         \t\t0: index\n\
         \t\t1: other\n\
         }\n\
         rule (\"duplicates\") {\n\
         \tevent {\n\
         \t\tOngoing - Global;\n\
         \t}\n\
         \tactions {\n\
         \t\tIf(Compare(Global.index, ==, 0));\n\
         \t\t\tSet Global Variable(other, 1);\n\
         \t\tElse If(Compare(Global.index, ==, 0));\n\
         \t\t\tSet Global Variable(other, 2);\n\
         \t\tEnd;\n\
         \t}\n\
         }\n";
    let dir = fixture_dir("lint-fix", &[("fixable.ws", FIXABLE)]);
    insta::assert_snapshot!(transcript(&run_in(&dir, &["lint", "fixable.ws", "--fix"])));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn analyze_reports_the_essential_sections() {
    let dir = fixture_dir(
        "analyze",
        &[("cake.ws", &corpus_workshop("real-world/overpy-cake"))],
    );
    let output = run_in(&dir, &["analyze", "cake.ws"]);
    assert!(
        output.stdout.len() <= 8 * 1024,
        "the analyze report stays bounded: {} bytes",
        output.stdout.len()
    );
    insta::assert_snapshot!(transcript(&output));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn inspect_reports_inventory_and_next_commands() {
    let dir = fixture_dir(
        "inspect",
        &[("subs.ws", &corpus_workshop("synthetic/subroutines"))],
    );
    insta::assert_snapshot!(transcript(&run_in(&dir, &["inspect", "subs.ws"])));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn inspect_symbols() {
    let dir = fixture_dir(
        "symbols",
        &[("cake.ws", &corpus_workshop("real-world/overpy-cake"))],
    );
    insta::assert_snapshot!(transcript(&run_in(
        &dir,
        &["inspect", "symbols", "cake.ws"]
    )));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn inspect_refs() {
    let dir = fixture_dir(
        "refs",
        &[("cake.ws", &corpus_workshop("real-world/overpy-cake"))],
    );
    insta::assert_snapshot!(transcript(&run_in(
        &dir,
        &["inspect", "refs", "cakePos", "cake.ws"]
    )));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn inspect_cfg() {
    let dir = fixture_dir(
        "cfg",
        &[("subs.ws", &corpus_workshop("synthetic/subroutines"))],
    );
    insta::assert_snapshot!(transcript(&run_in(
        &dir,
        &["inspect", "cfg", "first", "subs.ws"]
    )));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn inspect_callgraph() {
    let dir = fixture_dir(
        "callgraph",
        &[("subs.ws", &corpus_workshop("synthetic/subroutines"))],
    );
    insta::assert_snapshot!(transcript(&run_in(
        &dir,
        &["inspect", "callgraph", "subs.ws"]
    )));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn inspect_cost() {
    let dir = fixture_dir(
        "cost",
        &[("cake.ws", &corpus_workshop("real-world/overpy-cake"))],
    );
    insta::assert_snapshot!(transcript(&run_in(&dir, &["inspect", "cost", "cake.ws"])));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn inspect_cost_reports_the_comparison_records() {
    let dir = fixture_dir(
        "cost-compare",
        &[(
            "t.ws",
            "rule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Wait(0.016);\n    }\n}\n",
        )],
    );
    assert!(run_in(&dir, &["inspect", "cost", "t.ws"]).status.success());
    insta::assert_snapshot!(transcript(&run_in(&dir, &["inspect", "cost", "t.ws"])));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lookup_reports_entry_records() {
    let dir = fixture_dir("lookup", &[]);
    insta::assert_snapshot!(transcript(&run_in(&dir, &["lookup", "wait"])));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lookup_errors_are_records() {
    let dir = fixture_dir("lookup-error", &[]);
    insta::assert_snapshot!(transcript(&run_in(&dir, &["lookup", "--within", "nope"])));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_refusals_are_error_records() {
    let dir = fixture_dir("update-error", &[]);
    insta::assert_snapshot!(transcript(&run_in_env(
        &dir,
        &["update", "self", "--check"],
        &[
            ("WRIGHT_INSTALL_OS", "plan9"),
            ("WRIGHT_INSTALL_ARCH", "m68k")
        ]
    )));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn plain_and_json_agree_on_verdict_exit_and_counts() {
    let dir = fixture_dir(
        "parity",
        &[
            ("ok.ws", &corpus_workshop("synthetic/basic-rule")),
            ("mixed.ws", MIXED_CHECK),
            ("flow.ws", &corpus_workshop("synthetic/control-flow")),
            ("subs.ws", &corpus_workshop("synthetic/subroutines")),
            ("cake.ws", &corpus_workshop("real-world/overpy-cake")),
        ],
    );
    let cases: &[&[&str]] = &[
        &["check", "ok.ws"],
        &["check", "mixed.ws"],
        &["lint", "flow.ws"],
        &["inspect", "subs.ws"],
        &["inspect", "cost", "cake.ws"],
        &["analyze", "flow.ws"],
    ];
    for args in cases {
        let plain = run_in(&dir, args);
        let json = run_in(&dir, &[args.to_vec(), vec!["-f", "json"]].concat());
        assert_eq!(
            plain.status.code(),
            json.status.code(),
            "{args:?}: exit codes agree"
        );
        let envelope: serde_json::Value =
            serde_json::from_slice(&json.stdout).expect("{args:?}: JSON parses");
        let stdout = String::from_utf8_lossy(&plain.stdout);
        let verdict = stdout.lines().next().unwrap_or_default();
        let word = verdict.split_whitespace().next().unwrap_or_default();
        let ok = envelope["ok"].as_bool().unwrap();
        let exit = envelope["exit"].as_u64().unwrap();
        if ok && exit == 0 {
            let query = if args[0] == "inspect" {
                match args.get(1) {
                    Some(&sub @ ("symbols" | "refs" | "cfg" | "callgraph" | "cost" | "rules")) => {
                        sub
                    }
                    _ => "inspect",
                }
            } else {
                args[0]
            };
            assert!(
                ["PASS", "WARN"].contains(&word) || word == query.to_uppercase(),
                "{args:?}: a clean envelope's verdict is PASS/WARN/query name, got {verdict}"
            );
        } else {
            let expected: &[&str] = match exit {
                1 => &["FAIL", "ERROR"],
                2 => &["USAGE"],
                3 => &["UNSUPPORTED"],
                _ => &["ERROR"],
            };
            assert!(
                expected.contains(&word),
                "{args:?}: {verdict} is consistent with ok={ok} exit={exit}"
            );
        }

        let summary = stdout
            .lines()
            .find_map(|line| line.strip_prefix("summary: "))
            .expect("{args:?}: a summary record");
        let fields: std::collections::HashMap<&str, u64> = summary
            .split_whitespace()
            .filter_map(|pair| {
                let (key, value) = pair.split_once('=')?;
                Some((key, value.parse().ok()?))
            })
            .collect();
        let expected: Vec<(&str, u64)> = match args[0] {
            "check" => ["errors", "warnings", "notes"]
                .into_iter()
                .map(|key| {
                    let severity = match key {
                        "errors" => "error",
                        "warnings" => "warning",
                        _ => "info",
                    };
                    (
                        key,
                        envelope["diagnostics"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter(|d| d["severity"] == severity)
                            .count() as u64,
                    )
                })
                .collect(),
            "lint" => vec![(
                "findings",
                envelope["result"]["counts"]["findings"]["total"]
                    .as_u64()
                    .unwrap_or_else(|| {
                        envelope["result"]["findings"]
                            .as_array()
                            .map_or(0, |f| f.len() as u64)
                    }),
            )],
            "analyze" => ["errors", "warnings"]
                .into_iter()
                .map(|key| {
                    let severity = match key {
                        "errors" => "error",
                        _ => "warning",
                    };
                    (
                        key,
                        envelope["result"]["counts"]["risks"][severity]
                            .as_u64()
                            .unwrap_or(0),
                    )
                })
                .collect(),
            "inspect" if args.get(1) == Some(&"cost") => [
                ("actions", "programActions"),
                ("rules", "programRules"),
                ("waits", "waitActions"),
            ]
            .into_iter()
            .map(|(key, member)| {
                (
                    key,
                    envelope["result"]["exact"][member].as_u64().unwrap_or(0),
                )
            })
            .collect(),
            _ => Vec::new(),
        };
        for (key, count) in expected {
            assert_eq!(
                fields.get(key).copied(),
                Some(count),
                "{args:?}: summary {key}= mirrors the JSON count"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
