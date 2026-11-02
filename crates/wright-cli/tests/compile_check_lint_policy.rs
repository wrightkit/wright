use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const SOURCE: &str = r#"rule("nested") {
    event { Ongoing - Global; }
    actions {
        Set Global Variable(A, Filtered Array(All Players(All Teams), Is True For Any(Array(Current Array Element, True), Current Array Element)));
    }
}"#;
const CODE: &str = "nested-binder-lambda";

fn input(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("wright-lint-policy-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("nested.ws");
    std::fs::write(&input, SOURCE).unwrap();
    input
}

fn run(command: &str, input: &Path, flags: &[&str]) -> Output {
    let mut process = Command::new(env!("CARGO_BIN_EXE_wright"));
    process
        .arg(command)
        .arg(input)
        .args(flags)
        .args(["-f", "json"]);
    if command == "compile" {
        process.args(["--no-cache", "--force", "-o"]);
        process.arg(input.with_extension("ow"));
    }
    process.output().unwrap()
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| panic!("{error}: {output:?}"))
}

#[test]
fn project_lint_policy_and_cli_overrides_match_lint() {
    let input = input("project");
    let config = input.with_extension("yaml");
    std::fs::write(
        &config,
        format!("rules:\n  '{CODE}':\n    severity: error\n"),
    )
    .unwrap();
    for overrides in [vec![], vec!["--disable-rule", CODE]] {
        let mut flags = vec!["--lint-config", config.to_str().unwrap()];
        flags.extend(overrides);
        let lint = run("lint", &input, &flags);
        let reference = json(&lint);
        for command in ["compile", "check"] {
            let output = run(command, &input, &flags);
            assert_eq!(output.status.code(), lint.status.code(), "{output:?}");
            let result = json(&output);
            assert_eq!(result["ok"], reference["ok"]);
            let findings = reference["result"]["findings"].as_array().unwrap();
            let diagnostics = result["diagnostics"].as_array().unwrap();
            assert_eq!(findings.len(), diagnostics.len(), "{result}");
            for (finding, diagnostic) in findings.iter().zip(diagnostics) {
                for field in ["code", "severity", "evidence"] {
                    assert_eq!(diagnostic[field], finding[field], "{command}: {field}");
                }
                let span = &diagnostic["span"];
                assert_eq!(
                    finding["location"],
                    format!(
                        "{}:{}:{}",
                        span["path"].as_str().unwrap(),
                        span["start"]["line"],
                        span["start"]["col"]
                    )
                );
            }
        }
    }
    for command in ["compile", "check"] {
        let output = run(command, &input, &["--disable-rule", "unknown-lint-rule"]);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("unknown rule id 'unknown-lint-rule'")
        );
    }
}

#[test]
fn lint_errors_preserve_compile_destination_and_check_verdict_before_selection() {
    let input = input("errors");
    let dest = input.with_extension("ow");
    std::fs::write(&dest, "existing output").unwrap();
    let policy = format!("{CODE}:error");
    let output = run("compile", &input, &["--rule-severity", &policy]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(json(&output)["result"]["output"].is_null());
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "existing output");
    let output = run("check", &input, &["--rule-severity", &policy, "--max", "0"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let result = json(&output);
    assert_eq!(result["diagnostics"], serde_json::json!([]));
    assert_eq!(result["selection"]["withheld"], 1);
}
