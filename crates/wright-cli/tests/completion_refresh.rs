use std::process::{Command, Stdio};

#[test]
fn refresh_rewrites_a_stale_file_and_names_the_reload_command() {
    let dir =
        std::env::temp_dir().join(format!("wright-completion-refresh-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("wright.fish");
    std::fs::write(&target, "stale").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wright"))
        .args(["completion", "refresh"])
        .env("WRIGHT_COMPLETION_DIR", &dir)
        .stdin(Stdio::null())
        .output()
        .expect("wright runs");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("refreshed fish completion"), "{stdout}");
    assert!(stdout.contains("exec fish"), "{stdout}");
    assert_ne!(std::fs::read_to_string(&target).unwrap(), "stale");
    let _ = std::fs::remove_dir_all(&dir);
}
