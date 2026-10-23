use std::path::PathBuf;
use std::process::{Command, Stdio};

fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

fn temp_file(name: &str, content: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wright-refs-{}", std::process::id()));
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

const TICKS: &str = r#"variables {
    global:
        0: score
}
rule ("tick") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(score, 1);
    }
}
rule ("tick") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(score, Add(score, 1));
    }
}
"#;

#[test]
fn refs_text_carries_locations_and_canonical_rule_addresses() {
    let path = temp_file("ticks.ws", TICKS);
    let output = run(&["inspect", "refs", "score", path.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "refs resolves the variable: {}",
        command_result(&output)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let refs: Vec<&str> = stdout
        .lines()
        .filter(|line| line.starts_with("ref: "))
        .collect();
    assert_eq!(
        refs.len(),
        4,
        "declaration + two writes + one read: {stdout}"
    );
    for line in &refs {
        assert!(line.contains("at="), "every reference locates: {line}");
    }
    let writes: Vec<&&str> = refs.iter().filter(|line| line.contains("write")).collect();
    assert_eq!(writes.len(), 2, "{stdout}");
    for (line, address) in writes.iter().zip(["rule=tick#1", "rule=tick#2"]) {
        assert!(
            line.contains(address),
            "duplicate rules disambiguate as NAME#N: {line}"
        );
    }
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn refs_json_spans_are_never_null() {
    let path = temp_file("ticks.ws", TICKS);
    let output = run(&[
        "inspect",
        "refs",
        "score",
        path.to_str().unwrap(),
        "-f",
        "json",
    ]);
    assert!(
        output.status.success(),
        "refs resolves the variable: {}",
        command_result(&output)
    );
    let envelope: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout is one JSON envelope");
    for reference in envelope["result"]["references"].as_array().unwrap() {
        let span = &reference["span"];
        assert!(span["path"].is_string(), "span.path: {reference}");
        assert!(span["start"]["line"].is_u64(), "span.start: {reference}");
    }
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
