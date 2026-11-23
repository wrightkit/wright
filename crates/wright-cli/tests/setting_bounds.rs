//! Hero setting values outside the range the game client accepts must fail
//! `check`, `lint`, and `compile` for every source language (#1128). The
//! client rejects the whole mode on import, so the bound is an error.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const COMMANDS: [&str; 3] = ["check", "lint", "compile"];

fn workshop_source(speed: u32) -> String {
    format!(
        "settings {{\n    main {{\n        Mode Name: \"t\"\n        Description: \"t\"\n    }}\n    \
         modes {{\n        Skirmish {{\n            enabled maps {{\n                Workshop Island\n            }}\n        }}\n    }}\n    \
         heroes {{\n        Team 1 {{\n            Soldier: 76 {{\n                Movement Speed: {speed}%\n            }}\n        }}\n    }}\n}}\n\n\
         rule(\"x\") {{\n    event {{ Ongoing - Global; }}\n}}\n"
    )
}

fn base(command: &str) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wright"));
    cmd.arg(command);
    cmd
}

fn finish(mut cmd: Command, command: &str) -> Command {
    // `-o -` keeps `compile` from writing next to the input.
    if command == "compile" {
        cmd.args(["-o", "-"]);
    }
    cmd.args(["--renderer", "plain", "--color", "never"]);
    cmd
}

fn run_workshop(command: &str, speed: u32) -> Output {
    let mut cmd = finish(base(command), command);
    let mut child = cmd
        .args(["-", "--kind", "workshop"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wright runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(workshop_source(speed).as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/opy/setting-bounds")
        .join(name)
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

fn run_opy(command: &str, file: &str, provider: &Path) -> Output {
    let mut cmd = finish(base(command), command);
    cmd.arg(fixture(file)).arg("--opy-provider").arg(provider);
    cmd.output().expect("wright runs")
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn assert_names_setting_and_bound(output: &Output) {
    let text = text(output);
    assert!(
        text.lines()
            .any(|line| line.starts_with("diagnostic: error")
                && line.contains("movementSpeed%")
                && line.contains("minimum 50")),
        "expected an error naming movementSpeed% and its minimum: {text}"
    );
}

#[test]
fn workshop_input_rejects_movement_speed_below_the_client_minimum() {
    for command in COMMANDS {
        let output = run_workshop(command, 45);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{command}: {}",
            text(&output)
        );
        assert_names_setting_and_bound(&output);
        // Points at the offending setting line, not at the whole block.
        assert!(
            text(&output).contains("at=<stdin>:16:"),
            "{command}: {}",
            text(&output)
        );
    }
}

#[test]
fn workshop_input_accepts_movement_speed_at_the_client_minimum() {
    for command in COMMANDS {
        let output = run_workshop(command, 50);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{command}: {}",
            text(&output)
        );
    }
}

#[test]
fn opy_input_rejects_movement_speed_below_the_client_minimum() {
    let Some(provider) = provider() else {
        eprintln!("SKIPPED setting-bounds OPY: no installed OPY provider");
        return;
    };
    for command in COMMANDS {
        let output = run_opy(command, "below.opy", &provider);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{command}: {}",
            text(&output)
        );
        assert_names_setting_and_bound(&output);
    }
}

#[test]
fn opy_input_accepts_movement_speed_at_the_client_minimum() {
    let Some(provider) = provider() else {
        eprintln!("SKIPPED setting-bounds OPY: no installed OPY provider");
        return;
    };
    for command in COMMANDS {
        let output = run_opy(command, "at.opy", &provider);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{command}: {}",
            text(&output)
        );
    }
}
