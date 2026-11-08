use std::process::{Command, Stdio};

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_wright"))
        .args(args)
        .stdin(Stdio::null())
        .env_remove("CI")
        .env_remove("GITHUB_ACTIONS")
        .env_remove("WRIGHT_NO_CACHE")
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

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("wright-compile-baseline-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn compile_baseline_failures_leave_the_destination_untouched() {
    let dir = temp_dir();
    let source = "rule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Wait(0.016);\n    }\n}\n";
    let input = dir.join("a.ws");
    std::fs::write(&input, source).unwrap();
    let input = input.to_str().unwrap();
    let out = dir.join("out.txt");
    std::fs::write(&out, "sentinel").unwrap();
    let out_arg = out.to_str().unwrap();

    let save = run(&["inspect", "cost", input, "--save-baseline", "v1"]);
    assert!(save.status.success(), "{}", command_result(&save));

    let disabled = run(&[
        "compile",
        input,
        "-o",
        out_arg,
        "--force",
        "--no-cache",
        "--baseline",
        "v1",
    ]);
    let stderr = command_result(&disabled);
    assert_eq!(
        disabled.status.code(),
        Some(1),
        "{}",
        command_result(&disabled)
    );
    assert!(stderr.contains("caching is disabled"), "{stderr}");
    assert!(!stderr.contains("unknown baseline"), "{stderr}");
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "sentinel");

    let missing = run(&[
        "compile",
        input,
        "-o",
        out_arg,
        "--force",
        "--baseline",
        "nope",
    ]);
    let stderr = command_result(&missing);
    assert!(stderr.contains("unknown baseline 'nope'"), "{stderr}");
    assert_ne!(missing.status.code(), Some(0));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "sentinel");
    let _ = std::fs::remove_dir_all(&dir);
}
