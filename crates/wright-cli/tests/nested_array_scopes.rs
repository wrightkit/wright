use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn lint_reports_nested_current_array_element_import_risk() {
    let source = r#"
variables {
    global:
        0: pool
}
rule ("nested any in filter") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(pool, Filtered Array(All Players(Team 1), And(Is Alive(Current Array Element), Is True For Any(Array(Compare(Hero Of(Current Array Element), ==, Hero(Ana)), Is Alive(Current Array Element)), Current Array Element))));
        Set Global Variable(pool, Is True For Any(Array(Is Alive(Event Player)), Current Array Element));
        Set Global Variable(pool, Filtered Array(Sorted Array(All Players(Team 1), Current Array Element), Is Alive(Current Array Element)));
        Set Global Variable(pool, Filtered Array(All Players(Team 1), Is True For Any(Array(True), True)));
    }
}
"#;
    for (flags, exit, severity) in [
        (vec![], 0, Some("warning")),
        (
            vec!["--rule-severity", "nested-binder-lambda:error"],
            1,
            Some("error"),
        ),
        (vec!["--disable-rule", "nested-binder-lambda"], 0, None),
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_wright"))
            .args(["lint", "-", "-f", "json"])
            .args(flags)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(source.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(exit), "{output:?}");
        let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let findings: Vec<_> = envelope["result"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|finding| finding["code"] == "nested-binder-lambda")
            .collect();
        assert_eq!(findings.len(), usize::from(severity.is_some()));
        if let Some(severity) = severity {
            assert_eq!(findings[0]["severity"], severity);
            assert_eq!(findings[0]["evidence"], "static-indicator");
            let message = findings[0]["message"].as_str().unwrap();
            assert!(message.contains("isTrueForAny"));
            assert!(message.contains("filteredArray"));
            assert!(message.contains("Current Array Element"));
            assert!(message.contains("unconfirmed"));
            assert!(
                findings[0]["location"]
                    .as_str()
                    .unwrap()
                    .starts_with("<stdin>:11:")
            );
        }
    }
}
