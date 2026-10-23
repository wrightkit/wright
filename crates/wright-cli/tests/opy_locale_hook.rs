use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn provider() -> Option<PathBuf> {
    let installed = wright_driver::OpyProviderConfig::default()
        .installed()
        .expect("local provider inventory")
        .map(|provider| provider.executable);
    assert!(
        installed.is_some() || std::env::var_os("CI").is_none(),
        "CI must install the OPY provider before this suite"
    );
    installed
}

fn run(args: &[&str], provider: &Path) -> (bool, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_wright"))
        .args(args)
        .arg("--opy-provider")
        .arg(provider)
        .args(["-f", "json"])
        .arg(workspace_root().join("tests/fixtures/opy/locale-hook/main.opy"))
        .output()
        .expect("wright runs");
    let envelope = serde_json::from_slice(&output.stdout).expect("stdout is a JSON envelope");
    (output.status.success(), envelope)
}

fn assert_rejects_the_spelling(envelope: &Value) {
    let diagnostics = envelope["diagnostics"].as_array().expect("diagnostics");
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic["code"] == "unknown-enum-member"
                && diagnostic["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("Map.格里姆火山"))
        }),
        "expected unknown-enum-member for Map.格里姆火山: {diagnostics:?}"
    );
}

#[test]
fn check_and_compile_reject_the_spelling_the_locale_never_produced() {
    let Some(provider) = provider() else {
        eprintln!("SKIPPED locale-hook parity: no installed OPY provider");
        return;
    };
    let (ok, check) = run(&["check"], &provider);
    assert!(!ok, "check passed a project compile rejects: {check}");
    assert_rejects_the_spelling(&check);

    let out = std::env::temp_dir().join(format!("wright-locale-hook-{}.ow", std::process::id()));
    let (ok, compile) = run(&["compile", "-o", out.to_str().unwrap()], &provider);
    let _ = std::fs::remove_file(&out);
    assert!(!ok, "compile accepted the spelling: {compile}");
    assert_rejects_the_spelling(&compile);
}

#[test]
fn check_accepts_the_spelling_under_its_own_locale() {
    let Some(provider) = provider() else {
        eprintln!("SKIPPED locale-hook parity: no installed OPY provider");
        return;
    };
    let (ok, check) = run(&["check", "--locale", "zh-CN"], &provider);
    assert!(ok, "check --locale zh-CN failed: {check}");
}
