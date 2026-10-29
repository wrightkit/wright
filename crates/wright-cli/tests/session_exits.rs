//! Session-construction exits follow the diagnostic's declared class
//! (#1028): `lint-rule-error` splits by failure kind — a missing or
//! unreadable `--rule-file` path is a usage error (2), while a file
//! that loads but is invalid content is a source error (1). Each of
//! the three session-construction surfaces must exit by the class the
//! diagnostic carries, not a fixed wrapper exit.

use std::path::{Path, PathBuf};
use std::process::Command;

fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(wright())
        .args(args)
        .output()
        .expect("wright runs")
}

fn temp_dir() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-session-exits-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn session_construction_exits_follow_the_diagnostic_class() {
    let dir = temp_dir();
    let missing = dir.join("missing.yaml");
    let invalid = dir.join("invalid.yaml");
    std::fs::write(&invalid, "id: [unclosed\n").unwrap();
    for command in [&["check"][..], &["lookup", "wait"][..], &["serve"][..]] {
        for (rule_file, code) in [(&missing, 2), (&invalid, 1)] {
            let args: Vec<String> = command
                .iter()
                .map(|arg| arg.to_string())
                .chain([
                    "--rule-file".to_string(),
                    rule_file.to_string_lossy().into_owned(),
                ])
                .collect();
            let output = run(&args.iter().map(String::as_str).collect::<Vec<_>>());
            assert_eq!(
                output.status.code(),
                Some(code),
                "{args:?}: status={} stderr={}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    let _ = std::fs::remove_dir_all(Path::new(&dir));
}
