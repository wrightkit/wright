//! Terminal renderer snapshots and contracts (ADR-0030 decision 4, #795).
//!
//! Every scoped command renders the verdict tag, the headline, and its
//! hierarchy — snapshotted at the 100-column layout and the compact
//! layout below 90 columns, with ANSI colour on and off. The snapshots
//! keep ANSI escapes readable as `<ESC>` so they review like text; the
//! raw bytes are what the terminal sees.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn workshop(id: &str) -> String {
    std::fs::read_to_string(
        workspace_root()
            .join("tests/fixtures/workshop")
            .join(id)
            .with_extension("ws"),
    )
    .unwrap()
}

/// A scratch directory holding `files` under their relative names; runs
/// address inputs relatively so diagnostics spell `flow.ws:11:9`, never a
/// machine-specific absolute path.
fn work_dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wright-terminal-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, content) in files {
        std::fs::write(dir.join(name), content).unwrap();
    }
    dir
}

struct Outcome {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run(dir: &Path, args: &[&str], env: &[(&str, String)]) -> Outcome {
    let output = Command::new(wright())
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .env_remove("CI")
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITHUB_STEP_SUMMARY")
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR")
        .env_remove("WRIGHT_INSTALL_OS")
        .env_remove("WRIGHT_INSTALL_ARCH")
        .env_remove("WRIGHT_PROVIDER_DATA_DIR")
        .envs(env.iter().map(|(k, v)| (*k, v.clone())))
        .output()
        .expect("wright runs");
    Outcome {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).replace('\x1b', "<ESC>"),
        stderr: String::from_utf8_lossy(&output.stderr).replace('\x1b', "<ESC>"),
    }
}

/// One readable record per run: the exit code, then each stream.
fn transcript(out: &Outcome) -> String {
    format!(
        "exit: {}\n--- stdout ---\n{}--- stderr ---\n{}",
        out.code.unwrap_or(-1),
        out.stdout,
        out.stderr
    )
}

/// The width × colour matrix the issue asks for: 100 and 80 columns,
/// `--color never` and `--color always`. Snapshots land at
/// `snapshots/terminal__<name>_w<columns>_<color>.snap`.
fn snap(name: &str, dir: &Path, args: &[&str], env: &[(&str, String)]) {
    for (columns, color) in [
        (100u16, "never"),
        (80, "never"),
        (100, "always"),
        (80, "always"),
    ] {
        let mut full_args: Vec<&str> = args.to_vec();
        full_args.extend(["--renderer", "terminal", "--color", color]);
        let mut env = env.to_vec();
        env.push(("COLUMNS", columns.to_string()));
        let out = run(dir, &full_args, &env);
        insta::assert_snapshot!(format!("{name}_w{columns}_{color}"), transcript(&out));
    }
}

#[test]
fn check_pass() {
    let dir = work_dir(
        "check-pass",
        &[("basic.ws", &workshop("synthetic/basic-rule"))],
    );
    snap("check_pass", &dir, &["check", "basic.ws"], &[]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn check_error() {
    let mixed = "settings {\n\
                 \tcustomGroup {\n\
                 \t\tmySetting: 1\n\
                 \t\totherSetting: 2\n\
                 \t}\n\
                 }\n\
                 rule (\"x\") {\n\
                 \tevent {\n\
                 \t\tOngoing - Global;\n\
                 \t}\n\
                 \tactions {\n\
                 \t\tSet Global Variable(A, 1);\n\
                 \t\tSet Global Variable(B, FutureValueThing(2));\n\
                 \t}\n\
                 }\n";
    let dir = work_dir("check-error", &[("mixed.ws", mixed)]);
    snap("check_error", &dir, &["check", "mixed.ws"], &[]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compile_writes_an_artifact() {
    let dir = work_dir(
        "compile",
        &[("flow.ws", &workshop("synthetic/control-flow"))],
    );
    snap(
        "compile",
        &dir,
        &["compile", "flow.ws", "-o", "out.ow"],
        &[],
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn convert_streams_the_artifact() {
    // The artifact keeps stdout pure; the tag only rides stderr when the
    // run has something to report.
    let dir = work_dir(
        "convert",
        &[("flow.ws", &workshop("synthetic/control-flow"))],
    );
    snap(
        "convert",
        &dir,
        &["convert", "--target", "opy", "flow.ws"],
        &[],
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lint_warn() {
    let dir = work_dir("lint", &[("flow.ws", &workshop("synthetic/control-flow"))]);
    snap("lint_warn", &dir, &["lint", "flow.ws"], &[]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lint_gated_fail() {
    let dir = work_dir(
        "lint-gated",
        &[("flow.ws", &workshop("synthetic/control-flow"))],
    );
    snap(
        "lint_gated_fail",
        &dir,
        &["lint", "flow.ws", "--fail-on", "warning"],
        &[],
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn analyze_warn() {
    let dir = work_dir(
        "analyze",
        &[("flow.ws", &workshop("synthetic/control-flow"))],
    );
    snap("analyze_warn", &dir, &["analyze", "flow.ws"], &[]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn inspect_summary() {
    let dir = work_dir(
        "inspect",
        &[("flow.ws", &workshop("synthetic/control-flow"))],
    );
    snap("inspect_summary", &dir, &["inspect", "flow.ws"], &[]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn inspect_cfg() {
    let dir = work_dir("cfg", &[("flow.ws", &workshop("synthetic/control-flow"))]);
    snap(
        "inspect_cfg",
        &dir,
        &["inspect", "cfg", "bounded while", "flow.ws"],
        &[],
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn inspect_cost() {
    let dir = work_dir("cost", &[("flow.ws", &workshop("synthetic/control-flow"))]);
    snap(
        "inspect_cost",
        &dir,
        &["inspect", "cost", "flow.ws", "--no-cache"],
        &[],
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn inspect_refs_error() {
    let dir = work_dir("refs", &[("flow.ws", &workshop("synthetic/control-flow"))]);
    snap(
        "inspect_refs_error",
        &dir,
        &["inspect", "refs", "Missing.symbol", "flow.ws"],
        &[],
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lookup_entries() {
    let dir = work_dir("lookup", &[]);
    snap("lookup", &dir, &["lookup", "Stop All Damage"], &[]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_pass() {
    // Both targets skip deterministically: Windows has no standalone
    // self-update, and the empty provider store holds nothing to update.
    let dir = work_dir("update", &[]);
    std::fs::create_dir_all(dir.join("provider-data")).unwrap();
    snap(
        "update_pass",
        &dir,
        &["update"],
        &[
            ("WRIGHT_INSTALL_OS", "plan9".to_string()),
            ("WRIGHT_INSTALL_ARCH", "x86_64".to_string()),
            (
                "WRIGHT_PROVIDER_DATA_DIR",
                dir.join("provider-data").to_string_lossy().into_owned(),
            ),
        ],
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_unsupported() {
    let dir = work_dir("update-unsupported", &[]);
    snap(
        "update_unsupported",
        &dir,
        &["update", "self"],
        &[
            ("WRIGHT_INSTALL_OS", "plan9".to_string()),
            ("WRIGHT_INSTALL_ARCH", "x86_64".to_string()),
        ],
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn usage_error() {
    let dir = work_dir("usage", &[("flow.ws", &workshop("synthetic/control-flow"))]);
    snap(
        "usage",
        &dir,
        &["lint", "flow.ws", "--rule-severity", "bogus"],
        &[],
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `COLUMNS` is the explicit width override; a terminal wider than the
/// 100-column cap still gets the capped layout.
#[test]
fn width_is_capped_at_100_columns() {
    let dir = work_dir(
        "width-cap",
        &[("flow.ws", &workshop("synthetic/control-flow"))],
    );
    let args = [
        "lint",
        "flow.ws",
        "--renderer",
        "terminal",
        "--color",
        "never",
    ];
    let wide = run(&dir, &args, &[("COLUMNS", "200".to_string())]);
    let capped = run(&dir, &args, &[("COLUMNS", "100".to_string())]);
    assert_eq!(wide.stdout, capped.stdout);
    assert_eq!(wide.stderr, capped.stderr);
    let narrow = run(&dir, &args, &[("COLUMNS", "80".to_string())]);
    assert!(
        narrow.stdout.contains("  --> "),
        "the compact layout aligns children at the item column: {}",
        narrow.stdout
    );
    assert!(
        !narrow.stdout.contains("    --> "),
        "the compact layout drops the deeper indent: {}",
        narrow.stdout
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `NO_COLOR` disables automatic colour; an explicit `--color always`
/// still wins, matching every other surface's contract.
#[test]
fn no_color_and_color_flag_both_apply() {
    let dir = work_dir(
        "color-env",
        &[("flow.ws", &workshop("synthetic/control-flow"))],
    );
    let args = ["lint", "flow.ws", "--renderer", "terminal"];
    let plain_env = run(&dir, &args, &[("NO_COLOR", "1".to_string())]);
    assert!(
        !plain_env.stdout.contains("<ESC>"),
        "NO_COLOR keeps the tag unfilled: {}",
        plain_env.stdout
    );
    let forced = run(
        &dir,
        &[
            "lint",
            "flow.ws",
            "--renderer",
            "terminal",
            "--color",
            "always",
        ],
        &[("NO_COLOR", "1".to_string())],
    );
    assert!(
        forced.stdout.contains("<ESC>[43;30m WARN <ESC>[0m"),
        "--color always fills the tag: {}",
        forced.stdout
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The verdict word never depends on colour alone: the same line is
/// legible in the no-colour report.
#[test]
fn status_survives_without_colour() {
    let dir = work_dir(
        "no-color",
        &[("flow.ws", &workshop("synthetic/control-flow"))],
    );
    let args = [
        "lint",
        "flow.ws",
        "--renderer",
        "terminal",
        "--color",
        "never",
    ];
    let out = run(&dir, &args, &[("COLUMNS", "100".to_string())]);
    assert!(
        out.stdout.starts_with(" WARN  lint\n"),
        "the word carries the status: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("<ESC>"),
        "no escape reaches the no-colour report: {}",
        out.stdout
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Parity with plain and JSON (ADR-0030): the terminal report agrees on
/// the verdict, the exit code, and the reported counts for the same run.
#[test]
fn verdict_exit_and_counts_match_plain_and_json() {
    let dir = work_dir(
        "parity",
        &[("flow.ws", &workshop("synthetic/control-flow"))],
    );
    // (args, terminal verdict line, plain verdict word, expected exit,
    //  terminal count fragment, JSON diagnostics/findings count)
    struct Case {
        args: &'static [&'static str],
        tag_line: &'static str,
        plain_word: &'static str,
        exit: i32,
        count_line: &'static str,
        reported: usize,
    }
    let cases = [
        Case {
            args: &["check", "flow.ws"],
            tag_line: " PASS  check",
            plain_word: "PASS check",
            exit: 0,
            count_line: "0 diagnostics",
            reported: 0,
        },
        Case {
            args: &["lint", "flow.ws"],
            tag_line: " WARN  lint",
            plain_word: "WARN lint",
            exit: 0,
            count_line: "1 warning",
            reported: 1,
        },
        Case {
            args: &["analyze", "flow.ws"],
            tag_line: " WARN  analyze",
            plain_word: "WARN analyze",
            exit: 0,
            count_line: "1 warning",
            reported: 1,
        },
        Case {
            args: &["lint", "flow.ws", "--fail-on", "warning"],
            tag_line: " FAIL  lint",
            plain_word: "FAIL lint",
            exit: 1,
            count_line: "1 warning",
            reported: 1,
        },
    ];
    for case in cases {
        let args = case.args;
        let terminal = run(
            &dir,
            &[args, &["--renderer", "terminal", "--color", "never"][..]].concat(),
            &[],
        );
        let plain = run(&dir, &[args, &["--renderer", "plain"][..]].concat(), &[]);
        let json = run(&dir, &[args, &["--format", "json"][..]].concat(), &[]);

        assert_eq!(terminal.code, Some(case.exit), "{args:?} terminal exit");
        assert_eq!(plain.code, Some(case.exit), "{args:?} plain exit");
        assert_eq!(json.code, Some(case.exit), "{args:?} JSON exit");
        assert!(
            terminal.stdout.starts_with(&format!("{}\n", case.tag_line)),
            "{args:?} verdict tag: {}",
            terminal.stdout
        );
        assert!(
            plain.stdout.starts_with(&format!("{} ", case.plain_word)),
            "{args:?} plain verdict: {}",
            plain.stdout
        );
        assert!(
            terminal.stdout.contains(case.count_line),
            "{args:?} count line: {}",
            terminal.stdout
        );
        let envelope: serde_json::Value =
            serde_json::from_str(&json.stdout).expect("stdout is one JSON envelope");
        assert_eq!(envelope["exit"], case.exit, "{args:?} JSON exit field");
        let json_count = ["diagnostics"]
            .iter()
            .map(|field| envelope[field].as_array().map(Vec::len).unwrap_or_default())
            .chain(["findings", "risks"].iter().map(|field| {
                envelope["result"][field]
                    .as_array()
                    .map(Vec::len)
                    .unwrap_or_default()
            }))
            .max()
            .unwrap_or_default();
        assert_eq!(json_count, case.reported, "{args:?} JSON counts");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
