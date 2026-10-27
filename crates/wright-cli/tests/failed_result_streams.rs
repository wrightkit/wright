use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn temp_file(name: &str, content: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-cli-test-{}-{}",
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

const MIXED_CHECK_SOURCE: &str = r#"settings {
    customGroup {
        mySetting: 1
        otherSetting: 2
    }
}
rule ("x") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(A, 1);
        Set Global Variable(B, FutureValueThing(2));
    }
}
"#;

const VALID_LOOP: &str = "rule (\"r\") {
    event { Ongoing - Global; }
    conditions { True == True; }
    actions {
        While(True);
            Wait(1);
        End;
    }
}
";

#[test]
fn failed_check_leads_with_verdict_then_diagnostics_then_next() {
    let path = temp_file("mixed.txt", MIXED_CHECK_SOURCE);
    let output = run(&[
        "check",
        path.to_str().unwrap(),
        "--renderer",
        "plain",
        "--color",
        "never",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    assert!(
        lines
            .next()
            .unwrap_or_default()
            .starts_with("ERROR check root="),
        "{stdout}"
    );
    assert_eq!(
        lines.next(),
        Some("summary: errors=1 warnings=2 notes=0 affected=1"),
        "{stdout}"
    );
    let error_at = stdout
        .find("diagnostic: error ")
        .expect("an error diagnostic");
    let warning_at = stdout
        .find("diagnostic: warning ")
        .expect("a warning diagnostic");
    assert!(
        error_at < warning_at,
        "blocking errors precede warnings: {stdout}"
    );
    assert!(
        stdout.contains("at=") && stdout.contains("mixed.txt:"),
        "a mapped location is one path:line:col token: {stdout}"
    );
    assert!(
        stdout.contains("unknown-value.futurevaluething"),
        "the rejected name's code rides the record: {stdout}"
    );
    assert!(
        stdout.contains("stage=analysis"),
        "the stage is a field, not a sentence: {stdout}"
    );
    assert!(output.stderr.is_empty(), "{}", command_result(&output));
    let mut lines = lines.skip_while(|line| line.starts_with("diagnostic:"));
    assert_eq!(lines.next(), Some("next: wright lint"), "{stdout}");
    for expected in ["next: wright analyze", "next: wright inspect"] {
        assert_eq!(lines.next(), Some(expected), "{stdout}");
    }
    assert_eq!(lines.next(), Some("next: wright lookup"), "{stdout}");
    assert!(lines.next().is_none(), "the report stays compact: {stdout}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn failed_text_result_puts_diagnostics_on_stdout() {
    let path = temp_file("loop.txt", VALID_LOOP);
    let output = run(&[
        "inspect",
        "cost",
        path.to_str().unwrap(),
        "--rule",
        "nonexistent",
    ]);
    assert_eq!(output.status.code(), Some(2), "{}", command_result(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("USAGE cost") || stderr.contains("invalid-selection"),
        "the usage refusal is reported: {stdout}{stderr}"
    );
    assert!(
        stderr.contains("invalid-selection") || stdout.contains("invalid-selection"),
        "the refusal names its code: {stdout}{stderr}"
    );

    let output = run(&["inspect", "cost", path.to_str().unwrap(), "--severity"]);
    assert_eq!(output.status.code(), Some(2), "{}", command_result(&output));
    assert!(
        !String::from_utf8_lossy(&output.stderr).is_empty(),
        "a usage error reports on stderr: {}",
        command_result(&output)
    );

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn machine_contract_states_run_cache_dependence() {
    let doc =
        std::fs::read_to_string(workspace_root().join("docs/cli/machine-contract.md")).unwrap();
    for needle in ["`.wright/`", "`--no-cache`", "`WRIGHT_NO_CACHE`", "`CI`"] {
        assert!(
            doc.contains(needle),
            "machine-contract.md states the cache dependence: {needle}"
        );
    }
    let mut pending = vec![workspace_root().join("docs/cli")];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "md") {
                let text = std::fs::read_to_string(&path).unwrap();
                if text.contains("byte-deterministic") || text.contains("byte-identical") {
                    assert!(
                        text.contains(".wright/"),
                        "{} claims byte determinism without the .wright/ caveat",
                        path.display()
                    );
                }
            }
        }
    }
}
