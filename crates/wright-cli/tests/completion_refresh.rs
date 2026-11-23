use std::process::{Command, Stdio};

#[test]
fn refresh_prints_fish_reload_for_stale_and_current_files() {
    let dir =
        std::env::temp_dir().join(format!("wright-completion-refresh-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let target = dir.join("wright.fish");
    let mut refresh = Command::new(env!("CARGO_BIN_EXE_wright"));
    refresh
        .args(["completion", "refresh"])
        .env("WRIGHT_COMPLETION_DIR", &dir)
        .stdin(Stdio::null());

    let missing = refresh.output().expect("wright runs");
    assert!(missing.status.success(), "{missing:?}");
    assert!(
        missing.stdout.is_empty() && missing.stderr.is_empty(),
        "{missing:?}"
    );
    assert!(!target.exists());

    std::fs::write(&target, "stale").unwrap();
    let output = refresh.output().expect("wright runs");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("refreshed fish completion"), "{stdout}");
    assert!(stdout.contains("exec fish"), "{stdout}");
    assert_ne!(std::fs::read_to_string(&target).unwrap(), "stale");

    let matching = refresh.output().expect("wright runs");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(matching.status.success(), "{matching:?}");
    let stdout = String::from_utf8_lossy(&matching.stdout);
    assert!(
        stdout.contains("`exec fish`"),
        "missing reload instruction for matching file: {stdout:?}"
    );
    assert!(!stdout.contains("refreshed fish completion"), "{stdout}");
}
