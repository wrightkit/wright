//! The one-pipeline lint contract (#1002): `wright lint --format json`
//! and the `lint` tool request share the driver's lint pipeline, so for
//! one input and one lint configuration both report the same `ok` and
//! `exit`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/workshop/synthetic/control-flow.ws")
}

fn serve_lint(args: &[&str], input: &std::path::Path) -> serde_json::Value {
    use std::io::Write;
    let mut child = Command::new(wright())
        .arg("serve")
        .arg("--transport")
        .arg("stdio")
        .args(args)
        .arg(input)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("wright serve spawns");
    writeln!(
        child.stdin.as_mut().unwrap(),
        r#"{{"op":"lint","all":true}}"#
    )
    .unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "serve exited: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_str(
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn lint_cli_and_tool_request_agree_on_the_verdict() {
    let input = fixture();
    for args in [
        Vec::<&str>::new(),
        vec!["--rule-severity", "min-wait-loop:error"],
    ] {
        let output = Command::new(wright())
            .arg("lint")
            .arg("-f")
            .arg("json")
            .args(&args)
            .arg(&input)
            .stdin(Stdio::null())
            .output()
            .expect("wright lint runs");
        let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            output.status.code().unwrap() as u64,
            envelope["exit"].as_u64().unwrap(),
            "the process exit is the envelope exit: {envelope}"
        );

        let lint = serve_lint(&args, &input);
        let lint = &lint["result"];
        assert_eq!(lint["ok"], envelope["ok"], "ok disagrees: {lint}");
        assert_eq!(lint["exit"], envelope["exit"], "exit disagrees: {lint}");
    }
}
