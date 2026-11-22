//! `wright agent hook post-edit` with a broken lint policy: a Wright failure
//! (exit 1, `wright: <message>` on stderr, nothing for the model), never a
//! finding report, for both harnesses and every policy error class (#1068).

use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn a_broken_lint_policy_is_a_wright_failure_for_both_harnesses() {
    let dir = std::env::temp_dir().join(format!("wright-hook-policy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("in.ws");
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/workshop/synthetic/control-flow.ws"),
        &input,
    )
    .unwrap();
    let invalid = dir.join("invalid.yaml");
    std::fs::write(&invalid, "id: [unclosed\n").unwrap();
    let missing = dir.join("missing.yaml");
    let payload = serde_json::json!({"tool_input": {"file_path": input}}).to_string();
    for harness in ["claude", "codex"] {
        for (rule_file, expected) in [(&invalid, "invalid rule YAML"), (&missing, "missing.yaml")] {
            let mut child = Command::new(env!("CARGO_BIN_EXE_wright"))
                .args(["agent", "hook", "post-edit", "--harness", harness, "--"])
                .arg(&input)
                .arg("--rule-file")
                .arg(rule_file)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(payload.as_bytes())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            let stderr = String::from_utf8_lossy(&output.stderr);
            let what = format!("{harness} {expected}: {output:?}");
            assert_eq!(output.status.code(), Some(1), "{what}");
            assert!(output.stdout.is_empty(), "{what}");
            assert!(stderr.starts_with("wright: "), "{what}");
            assert!(stderr.contains(expected), "{what}");
            assert_eq!(stderr.lines().count(), 1, "one message line: {what}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
