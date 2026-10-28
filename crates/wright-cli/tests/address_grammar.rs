use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::json;

fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

fn temp_file(name: &str, content: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-grammar-{}-{}",
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

fn parse_json(output: &[u8]) -> serde_json::Value {
    serde_json::from_slice(output).expect("stdout is one JSON envelope")
}

fn serve(input: &Path, lines: &[String]) -> Vec<serde_json::Value> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let cwd = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "grammar-serve-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&cwd).unwrap();
    let mut child = Command::new(wright())
        .args(["serve", "--transport", "stdio"])
        .arg(input)
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wright serve spawns");
    let mut stdin = child.stdin.take().unwrap();
    for line in lines {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&cwd);
    assert!(
        output.status.success(),
        "wright serve failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON response"))
        .collect()
}

const HASH_COLLISION: &str = r#"rule ("a") {
    event {
        Ongoing - Global;
    }
    actions {
        Wait(0.016, Abort When False);
    }
}
rule ("a") {
    event {
        Ongoing - Global;
    }
    actions {
        Wait(0.032, Abort When False);
    }
}
rule ("a#2") {
    event {
        Ongoing - Global;
    }
    actions {
        Wait(0.048, Abort When False);
    }
}
rule ("") {
    event {
        Ongoing - Global;
    }
    actions {
        Wait(0.064, Abort When False);
    }
}
"#;

const DUP_CALLERS: &str = r#"subroutines {
    0: reset
}
rule ("tick") {
    event {
        Ongoing - Global;
    }
    actions {
        Start Rule(reset, Restart Rule);
    }
}
rule ("tick") {
    event {
        Ongoing - Global;
    }
    actions {
        Start Rule(reset, Restart Rule);
    }
}
"#;

const KIND_COLLISION: &str = r#"variables {
    global:
        0: x
}
subroutines {
    0: x
}
rule ("uses x") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(x, 1);
    }
}
"#;

#[test]
fn name_ordinals_select_and_out_of_range_refuses() {
    let path = temp_file("dup.ws", DUP_CALLERS);
    let path = path.to_str().unwrap();

    for (rule, id) in [("tick#1", 0), ("tick#2", 1)] {
        let output = run(&["inspect", "rules", path, "--rule", rule, "-f", "json"]);
        assert!(
            output.status.success(),
            "{rule}: {}",
            command_result(&output)
        );
        let rules = parse_json(&output.stdout)["result"]["rules"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["id"], id);
        assert_eq!(rules[0]["address"], rule);
    }
    for (rule, code) in [("tick", "ambiguous-rule"), ("tick#3", "unknown-rule")] {
        let output = run(&["inspect", "rules", path, "--rule", rule, "-f", "json"]);
        assert_eq!(output.status.code(), Some(1), "{rule}");
        let envelope = parse_json(&output.stdout);
        assert_eq!(envelope["diagnostics"][0]["code"], code, "{rule}");
    }

    let responses = serve(
        Path::new(path),
        &[
            json!({"op":"rules","rule":"tick#2"}).to_string(),
            json!({"op":"rules","rule":"tick"}).to_string(),
            json!({"op":"rules","rule":"tick#9"}).to_string(),
        ],
    );
    let selected = &responses[0]["result"]["rules"];
    assert_eq!(selected.as_array().unwrap().len(), 1);
    assert_eq!(selected[0]["address"], "tick#2");
    assert_eq!(responses[1]["error"]["code"], "ambiguous-rule");
    assert_eq!(responses[2]["error"]["code"], "unknown-rule");

    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
}

#[test]
fn literal_hash_names_and_unnamed_ordinals() {
    let only = temp_file(
        "literal.ws",
        "rule (\"a#2\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Wait(0.016, Abort When False);\n    }\n}\n",
    );
    let output = run(&[
        "inspect",
        "rules",
        only.to_str().unwrap(),
        "--rule",
        "a#2",
        "-f",
        "json",
    ]);
    assert!(
        output.status.success(),
        "the lone literal is its own address: {}",
        command_result(&output)
    );
    assert_eq!(
        parse_json(&output.stdout)["result"]["rules"][0]["name"],
        "a#2"
    );
    let _ = std::fs::remove_dir_all(only.parent().unwrap());

    let path = temp_file("hash.ws", HASH_COLLISION);
    let output = run(&["inspect", "rules", path.to_str().unwrap(), "-f", "json"]);
    assert!(output.status.success(), "{}", command_result(&output));
    let rules = parse_json(&output.stdout)["result"]["rules"]
        .as_array()
        .unwrap()
        .clone();
    let addresses: Vec<&str> = rules
        .iter()
        .map(|rule| rule["address"].as_str().unwrap())
        .collect();
    assert_eq!(addresses, ["a#1", "a#2", "a#2#1", "#1"]);

    for rule in rules {
        let address = rule["address"].as_str().unwrap();
        let output = run(&[
            "inspect",
            "rules",
            path.to_str().unwrap(),
            "--rule",
            address,
            "-f",
            "json",
        ]);
        let envelope = parse_json(&output.stdout);
        if envelope["ok"] == true {
            let selected = envelope["result"]["rules"].as_array().unwrap();
            assert_eq!(selected.len(), 1, "{address}");
            assert_eq!(selected[0]["id"], rule["id"], "{address}");
        } else {
            assert_eq!(
                envelope["diagnostics"][0]["code"], "ambiguous-rule",
                "{address} fails only as ambiguous: {envelope}"
            );
        }
    }

    let output = run(&[
        "inspect",
        "rules",
        path.to_str().unwrap(),
        "--rule",
        "a#2",
        "-f",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let envelope = parse_json(&output.stdout);
    let message = envelope["diagnostics"][0]["message"].as_str().unwrap();
    assert_eq!(envelope["diagnostics"][0]["code"], "ambiguous-rule");
    for candidate in ["a#2", "a#2#1"] {
        assert!(message.contains(candidate), "{message}");
    }
    let output = run(&[
        "inspect",
        "rules",
        path.to_str().unwrap(),
        "--rule",
        "a#2#1",
        "-f",
        "json",
    ]);
    assert!(output.status.success(), "{}", command_result(&output));
    assert_eq!(
        parse_json(&output.stdout)["result"]["rules"][0]["name"],
        "a#2"
    );

    let output = run(&["inspect", "cfg", "#1", path.to_str().unwrap(), "-f", "json"]);
    assert!(
        output.status.success(),
        "cfg resolves the unnamed rule: {}",
        command_result(&output)
    );

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn callgraph_caller_refuses_the_merge() {
    let path = temp_file("callers.ws", DUP_CALLERS);

    let output = run(&[
        "inspect",
        "callgraph",
        path.to_str().unwrap(),
        "--caller",
        "tick",
        "-f",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(2));
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["diagnostics"][0]["code"], "invalid-selection");
    for candidate in ["tick#1", "tick#2"] {
        assert!(
            envelope["diagnostics"][0]["message"]
                .as_str()
                .unwrap()
                .contains(candidate),
            "the refusal lists the candidate addresses: {envelope}"
        );
    }

    for (caller, address) in [("tick#1", "tick#1"), ("tick#2", "tick#2")] {
        let output = run(&[
            "inspect",
            "callgraph",
            path.to_str().unwrap(),
            "--caller",
            caller,
            "-f",
            "json",
        ]);
        assert!(
            output.status.success(),
            "{caller}: {}",
            command_result(&output)
        );
        let edges = parse_json(&output.stdout)["result"]["edges"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0]["callerAddress"], address);
        assert_eq!(edges[0]["callee"], "reset");
    }

    let responses = serve(
        &path,
        &[
            json!({"op":"callGraph","caller":"tick"}).to_string(),
            json!({"op":"callGraph","caller":"tick#2"}).to_string(),
        ],
    );
    assert_eq!(responses[0]["error"]["code"], "invalid-selection");
    let edges = responses[1]["result"]["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0]["callerAddress"], "tick#2");

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn kind_names_disambiguate_symbol_collisions() {
    let path = temp_file("kinds.ws", KIND_COLLISION);

    let output = run(&["inspect", "refs", "x", path.to_str().unwrap(), "-f", "json"]);
    assert_eq!(output.status.code(), Some(1), "{}", command_result(&output));
    let envelope = parse_json(&output.stdout);
    let message = envelope["diagnostics"][0]["message"].as_str().unwrap();
    assert_eq!(envelope["diagnostics"][0]["code"], "ambiguous-symbol");
    for candidate in ["globalVariable:x", "subroutine:x"] {
        assert!(message.contains(candidate), "{message}");
    }

    for symbol in ["globalVariable:x", "subroutine:x"] {
        let output = run(&[
            "inspect",
            "refs",
            symbol,
            path.to_str().unwrap(),
            "-f",
            "json",
        ]);
        assert!(
            output.status.success(),
            "{symbol}: {}",
            command_result(&output)
        );
    }

    let responses = serve(
        &path,
        &[
            json!({"op":"references","symbol":"x"}).to_string(),
            json!({"op":"references","symbol":"globalVariable:x"}).to_string(),
            json!({"op":"usage","symbol":"subroutine:x"}).to_string(),
        ],
    );
    assert_eq!(responses[0]["error"]["code"], "ambiguous-symbol");
    let message = responses[0]["error"]["message"].as_str().unwrap();
    for candidate in ["globalVariable:x", "subroutine:x"] {
        assert!(message.contains(candidate), "{message}");
    }
    assert_eq!(responses[1]["result"]["kind"], "globalVariable");
    assert_eq!(responses[2]["result"]["kind"], "subroutine");

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn unmatched_files_refuse_on_cli_and_serve() {
    let clean = temp_file(
        "clean.ws",
        "rule (\"quiet\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Wait(1, Abort When False);\n    }\n}\n",
    );
    let clean = clean.to_str().unwrap();

    let output = run(&["analyze", clean, "--file", "clean.ws", "-f", "json"]);
    assert!(
        output.status.success(),
        "a matched file with no findings succeeds: {}",
        command_result(&output)
    );
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["result"]["counts"]["risks"]["total"], 0);

    for command in [
        vec!["analyze", clean, "--file", "elsewhere.ws"],
        vec!["inspect", "symbols", clean, "--file", "elsewhere.ws"],
        vec!["inspect", "rules", clean, "--file", "elsewhere.ws"],
    ] {
        let output = run(&[&command[..], &["-f", "json"]].concat());
        assert_eq!(output.status.code(), Some(2), "{command:?}");
        let envelope = parse_json(&output.stdout);
        assert_eq!(envelope["diagnostics"][0]["code"], "invalid-selection");
        assert!(
            envelope["diagnostics"][0]["message"]
                .as_str()
                .unwrap()
                .contains("elsewhere.ws"),
            "{command:?}: {envelope}"
        );
    }

    let responses = serve(
        Path::new(clean),
        &[
            json!({"op":"symbols","file":"clean.ws"}).to_string(),
            json!({"op":"symbols","file":"elsewhere.ws"}).to_string(),
            json!({"op":"rules","file":"elsewhere.ws"}).to_string(),
            json!({"op":"analyze","file":"elsewhere.ws"}).to_string(),
            json!({"op":"analyze","file":"clean.ws"}).to_string(),
        ],
    );
    assert!(
        !responses[0]["result"]["symbols"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    for response in &responses[1..4] {
        assert_eq!(response["error"]["code"], "invalid-selection", "{response}");
    }
    assert!(
        responses[4].get("error").is_none() && responses[4]["result"].is_object(),
        "a matched file with no findings succeeds: {}",
        responses[4]
    );

    let _ = std::fs::remove_dir_all(Path::new(clean).parent().unwrap());
}

#[test]
fn cost_estimate_rule_uses_the_same_grammar() {
    let path = temp_file("cost.ws", DUP_CALLERS);

    let responses = serve(
        &path,
        &[
            json!({"op":"costEstimate","rule":"tick"}).to_string(),
            json!({"op":"costEstimate","rule":"tick#2"}).to_string(),
        ],
    );
    assert_eq!(responses[0]["error"]["code"], "invalid-selection");
    let message = responses[0]["error"]["message"].as_str().unwrap();
    for candidate in ["tick#1", "tick#2"] {
        assert!(message.contains(candidate), "{message}");
    }
    assert_eq!(responses[1]["result"]["rule"]["address"], "tick#2");

    let output = run(&[
        "inspect",
        "cost",
        path.to_str().unwrap(),
        "--rule",
        "tick#1",
        "-f",
        "json",
    ]);
    assert!(output.status.success(), "{}", command_result(&output));
    assert_eq!(
        parse_json(&output.stdout)["result"]["rule"]["address"],
        "tick#1"
    );

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn corpus_workshop(fixture_id: &str) -> String {
    std::fs::read_to_string(
        workspace_root()
            .join("tests/fixtures/workshop")
            .join(fixture_id)
            .with_extension("ws"),
    )
    .unwrap()
}
#[test]
fn query_commands_reject_unknown_and_ambiguous_names() {
    let path = temp_file("cake.txt", &corpus_workshop("real-world/overpy-cake"));
    let path = path.to_str().unwrap();
    for (command_args, code) in [
        (&["inspect", "refs", "nope"][..], "unknown-symbol"),
        (&["inspect", "cfg", "nope"][..], "unknown-rule"),
    ] {
        let output = run(&[command_args, &[path, "-f", "json"]].concat());
        assert_eq!(
            output.status.code(),
            Some(1),
            "{command_args:?}: {}",
            command_result(&output)
        );
        assert!(output.stderr.is_empty(), "JSON mode: no stderr");
        let envelope = parse_json(&output.stdout);
        assert_eq!(envelope["ok"], false);
        assert_eq!(envelope["diagnostics"][0]["code"], code);
        assert_eq!(envelope["diagnostics"][0]["stage"], "analysis");
        assert!(
            envelope["result"].is_null(),
            "a failed query carries no result"
        );
    }

    let dup = temp_file(
        "dup.ws",
        r#"variables {
    global:
        0: dup
}
rule ("dup") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(dup, 1);
    }
}
rule ("dup") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(dup, 2);
    }
}
"#,
    );
    let output = run(&[
        "inspect",
        "refs",
        "dup",
        dup.to_str().unwrap(),
        "-f",
        "json",
    ]);
    assert!(
        output.status.success(),
        "refs resolves the variable: {}",
        command_result(&output)
    );
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["result"]["kind"], "globalVariable");

    let output = run(&["inspect", "cfg", "dup", dup.to_str().unwrap(), "-f", "json"]);
    assert_eq!(output.status.code(), Some(1));
    let envelope = parse_json(&output.stdout);
    assert_eq!(envelope["diagnostics"][0]["code"], "ambiguous-rule");
    for candidate in ["dup#1", "dup#2"] {
        assert!(
            envelope["diagnostics"][0]["message"]
                .as_str()
                .unwrap()
                .contains(candidate),
            "the diagnostic lists the candidates' addresses: {}",
            envelope["diagnostics"][0]["message"]
        );
    }
    let output = run(&[
        "inspect",
        "cfg",
        "dup#1",
        dup.to_str().unwrap(),
        "-f",
        "json",
    ]);
    assert!(
        output.status.success(),
        "cfg resolves dup#1: {}",
        command_result(&output)
    );

    let output = run(&["inspect", "refs", "nope", path]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        output.stderr.is_empty(),
        "a failed result leaves stderr clean: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ERROR refs"), "{stdout}");
    assert!(stdout.contains("unknown-symbol"), "{stdout}");
    assert!(stdout.contains("unknown symbol 'nope'"), "{stdout}");
    assert!(
        !stdout.contains("<unknown>") && !stdout.contains("read(s)"),
        "a failed query prints no default result metadata: {stdout}"
    );

    let _ = std::fs::remove_dir_all(Path::new(path).parent().unwrap());
    let _ = std::fs::remove_dir_all(dup.parent().unwrap());
}
