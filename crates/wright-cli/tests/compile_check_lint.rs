use std::process::{Command, Output, Stdio};

use serde_json::Value;

const SOURCE: &str = r#"rule ("nested") {
    event { Ongoing - Global; }
    actions {
        Set Global Variable(A, Filtered Array(All Players(All Teams), Is True For Any(Array(Current Array Element, True), Current Array Element)));
    }
}"#;

fn run(command: &str, args: &[&str], json: bool) -> Output {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_wright"))
        .args([command, "-", "--kind", "workshop"])
        .args(args)
        .args(if json {
            vec!["-f", "json"]
        } else {
            vec!["--renderer", "plain", "--color", "never"]
        })
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(SOURCE.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn envelope(command: &str, args: &[&str]) -> Value {
    let output = run(command, args, true);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{command}: {error}: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(
        output.status.code().unwrap(),
        value["exit"].as_i64().unwrap() as i32
    );
    value
}

#[test]
fn check_and_compile_carry_the_same_lint_diagnostics_and_verdict() {
    for args in [
        vec![],
        vec!["--rule-severity", "nested-binder-lambda:error"],
        vec!["--rule-severity", "nested-binder-lambda:off"],
        vec!["--disable-rule", "nested-binder-lambda"],
        vec!["--fail-on", "warning"],
    ] {
        let lint = envelope("lint", &args);
        let findings = lint["result"]["findings"].as_array().unwrap();
        for command in ["check", "compile"] {
            let dir =
                std::env::temp_dir().join(format!("wright-compile-lint-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let output_path = dir.join("output.ow");
            let mut flags = args.clone();
            if command == "compile" {
                flags.extend(["-o", output_path.to_str().unwrap(), "--force", "--no-cache"]);
            }
            let previous_output = std::fs::read(&output_path).ok();
            let result = envelope(command, &flags);
            assert_eq!(result["ok"], lint["ok"], "{command}: {args:?}: {result}");
            assert_eq!(result["exit"], lint["exit"], "{command}: {args:?}");
            let diagnostics = result["diagnostics"].as_array().unwrap();
            assert_eq!(diagnostics.len(), findings.len(), "{command}: {result}");
            for (diagnostic, finding) in diagnostics.iter().zip(findings) {
                for field in ["code", "severity", "message", "evidence", "boundedness"] {
                    assert_eq!(diagnostic[field], finding[field], "{command}: {field}");
                }
                assert_eq!(diagnostic["stage"], "analysis");
            }
            if command == "compile" && args.contains(&"nested-binder-lambda:error") {
                assert!(result["result"]["output"].is_null(), "{result}");
                assert_eq!(std::fs::read(&output_path).ok(), previous_output);
            }
        }
    }
}

#[test]
fn check_summary_counts_the_default_lint_warning() {
    let output = run("check", &[], false);
    assert!(output.status.success());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("nested-binder-lambda"), "{text}");
    assert!(text.contains("warnings=1"), "{text}");
}
