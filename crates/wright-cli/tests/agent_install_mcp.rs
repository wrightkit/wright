use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

fn temp_dir() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-agent-install-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn agent_install_mcp_configures_a_project_whose_server_answers_mcp() {
    use std::io::{BufRead, BufReader};
    let project = temp_dir();
    let elsewhere = temp_dir();
    let install = |args: &[&str]| {
        Command::new(wright())
            .args(["agent", "install"])
            .args(args)
            .current_dir(&project)
            .stdin(Stdio::null())
            .output()
            .expect("wright runs")
    };

    std::fs::write(
        project.join("session.ws"),
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/workshop/synthetic/control-flow.ws"),
        )
        .unwrap(),
    )
    .unwrap();
    std::fs::write(project.join("main.opy"), "").unwrap();
    let refused = install(&["--mcp", "claude", "--no-guide"]);
    assert_eq!(refused.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("input-kind-ambiguous") && stderr.contains("-- "),
        "{stderr}"
    );
    assert!(!project.join(".mcp.json").exists());
    let refused = install(&["--mcp", "claude", "--", "main.opy", "--kind", "workshop"]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(!project.join(".mcp.json").exists());
    std::fs::remove_file(project.join("main.opy")).unwrap();
    let output = install(&["--mcp", "claude", "--no-guide"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!project.join(".agents").exists());
    let first = std::fs::read(project.join(".mcp.json")).unwrap();
    assert!(install(&["--mcp", "claude", "--no-guide"]).status.success());
    assert_eq!(std::fs::read(project.join(".mcp.json")).unwrap(), first);
    assert!(install(&["--mcp", "claude"]).status.success());
    assert!(project.join(".agents/skills/wright/SKILL.md").is_file());

    let config: serde_json::Value = serde_json::from_slice(&first).unwrap();
    let entry = &config["mcpServers"]["wright"];
    assert_eq!(entry["command"], "wright");
    let project_dir = project.to_str().unwrap();
    let args: Vec<String> = entry["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| {
            arg.as_str()
                .unwrap()
                .replace("${CLAUDE_PROJECT_DIR:-.}", project_dir)
        })
        .collect();
    let mut child = Command::new(wright())
        .args(&args)
        .current_dir(&elsewhere)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("configured server spawns");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut exchange = |request: serde_json::Value| -> serde_json::Value {
        writeln!(stdin, "{request}").unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).expect("JSON response")
    };
    let init = exchange(serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}},
    }));
    assert_eq!(init["result"]["serverInfo"]["name"], "wright-tool-service");
    let list = exchange(serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    assert!(
        list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "wright_project")
    );
    let call = exchange(serde_json::json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "wright_project", "arguments": {}},
    }));
    assert!(call["result"].get("isError").is_none(), "{call}");
    drop(stdin);
    let _ = child.wait();

    let mut config = config;
    config["mcpServers"]["wright"]["command"] = "other".into();
    std::fs::write(project.join(".mcp.json"), config.to_string()).unwrap();
    let refused = install(&["--mcp", "claude", "--no-guide"]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        install(&["--mcp", "claude", "--no-guide", "--force"])
            .status
            .success()
    );
    config["mcpServers"] = serde_json::json!({});
    std::fs::write(project.join(".mcp.json"), config.to_string()).unwrap();
    assert!(install(&["--mcp", "claude", "--no-guide"]).status.success());

    let output = install(&["--mcp", "emacs"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("claude"));
    assert_eq!(install(&["--no-guide"]).status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&project);
    let _ = std::fs::remove_dir_all(&elsewhere);
}
