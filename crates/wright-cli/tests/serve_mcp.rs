//! `wright serve --transport mcp`: the MCP adapter's tool catalog, request
//! mapping, refusal codes, and long-lived-session behavior over the same
//! in-process service the other transports expose.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::json;

fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn corpus_workshop(id: &str) -> PathBuf {
    workspace_root()
        .join("tests/fixtures/workshop")
        .join(format!("{id}.ws"))
}

fn run_lines(transport: &str, input: &Path, lines: &[&str]) -> Vec<serde_json::Value> {
    // `compile` writes `<entry stem>.ow` in the working directory, so every
    // service process gets its own.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let cwd = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "serve-mcp-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&cwd).unwrap();
    let mut child = Command::new(wright())
        .args(["serve", "--transport", transport])
        .arg(input)
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wright-serve spawns");
    let mut stdin = child.stdin.take().unwrap();
    for line in lines {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&cwd);
    assert!(
        output.status.success(),
        "wright-serve failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON response"))
        .collect()
}

fn mcp_call(id: u64, name: &str, arguments: serde_json::Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    })
    .to_string()
}

/// The tool result's JSON payload: the single text content block parsed back
/// into a value.
fn mcp_payload(response: &serde_json::Value) -> serde_json::Value {
    serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
        .expect("tool content is JSON")
}

#[test]
fn mcp_transport_lists_the_initial_tool_set_within_capabilities() {
    let input = corpus_workshop("synthetic/control-flow");
    let responses = run_lines(
        "mcp",
        &input,
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#,
        ],
    );
    // The notification produced no response.
    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(
        responses[0]["result"]["serverInfo"]["name"],
        "wright-tool-service"
    );
    assert!(responses[0]["result"]["capabilities"]["tools"].is_object());
    assert!(responses[2]["result"].is_object());

    let tools = responses[1]["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "wright_project",
            "wright_rules",
            "wright_symbols",
            "wright_references",
            "wright_usage",
            "wright_cfg",
            "wright_findings",
            "wright_persistent_objects",
            "wright_lint",
            "wright_lint_rules",
            "wright_call_graph",
            "wright_cost_estimate",
            "wright_target_metadata",
            "wright_check",
            "wright_analyze",
            "wright_inspect",
            "wright_validate_edit_transaction",
            "wright_provider_validate_edit",
            "wright_edit_statements",
            "wright_lookup",
        ]
    );
    let caps = run_lines("stdio", &input, &[r#"{"op":"capabilities"}"#]);
    let operations: Vec<String> = caps[0]["result"]["operations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|op| op.as_str().unwrap().to_string())
        .collect();
    for tool in tools {
        let op = tool["name"].as_str().unwrap().trim_start_matches("wright_");
        let op = match op {
            "persistent_objects" => "persistentObjects",
            "lint_rules" => "lintRules",
            "call_graph" => "callGraph",
            "cost_estimate" => "costEstimate",
            "target_metadata" => "targetMetadata",
            "validate_edit_transaction" => "validateEditTransaction",
            "provider_validate_edit" => "providerValidateEdit",
            "edit_statements" => "editStatements",
            other => other,
        };
        assert!(
            operations.iter().any(|advertised| advertised == op),
            "{} not advertised",
            tool["name"]
        );
        // Schemas derive from the request definitions: no `op`, and the edit
        // tools omit `sources`.
        assert!(tool["inputSchema"]["properties"].get("op").is_none());
        assert!(tool["description"].is_string());
    }
    let validate_tool = tools
        .iter()
        .find(|t| t["name"] == "wright_validate_edit_transaction")
        .unwrap();
    assert!(
        validate_tool["inputSchema"]["properties"]
            .get("sources")
            .is_none(),
        "wright_validate_edit_transaction omits sources"
    );
    // #548: the provider tool omits `documents`/`sources` — the service
    // derives them from the session's loaded project.
    let provider_tool = tools
        .iter()
        .find(|t| t["name"] == "wright_provider_validate_edit")
        .unwrap();
    assert!(
        provider_tool["inputSchema"]["properties"]
            .get("documents")
            .is_none()
            && provider_tool["inputSchema"]["properties"]
                .get("sources")
                .is_none(),
        "wright_provider_validate_edit omits documents and sources"
    );
    // ADR-0029: `editStatements`'s MCP schema omits `sources` like the other
    // edit tools and keeps `inputIdentity`/`edits` required.
    let edit_statements = tools
        .iter()
        .find(|t| t["name"] == "wright_edit_statements")
        .unwrap();
    assert!(
        edit_statements["inputSchema"]["properties"]
            .get("sources")
            .is_none(),
        "wright_edit_statements omits sources"
    );
    assert_eq!(
        edit_statements["inputSchema"]["required"],
        json!(["inputIdentity", "edits"])
    );
    // #532: the report tools expose `all` in their derived input schemas.
    for name in ["wright_analyze", "wright_inspect", "wright_lint"] {
        let tool = tools.iter().find(|t| t["name"] == name).unwrap();
        assert_eq!(
            tool["inputSchema"]["properties"]["all"]["type"], "boolean",
            "{name} has no all field"
        );
    }
    // #529: `wright_lookup`'s schema derives from `LookupRequest` — `op` is
    // dropped, `language` stays required.
    let lookup = tools.iter().find(|t| t["name"] == "wright_lookup").unwrap();
    assert_eq!(lookup["inputSchema"]["required"], json!(["language"]));
    assert!(
        lookup["inputSchema"]["properties"].get("op").is_none()
            && lookup["inputSchema"]["properties"].get("query").is_some()
            && lookup["inputSchema"]["properties"].get("within").is_some()
    );
}

#[test]
fn mcp_transport_results_match_the_service_contract() {
    let input = corpus_workshop("synthetic/control-flow");
    let source = std::fs::read_to_string(&input).unwrap();
    let key = input.to_string_lossy().into_owned();
    let cases: Vec<(&str, serde_json::Value)> = vec![
        ("wright_project", serde_json::json!({})),
        ("wright_symbols", serde_json::json!({"kind": "rule"})),
        ("wright_references", serde_json::json!({"symbol": "index"})),
        ("wright_usage", serde_json::json!({"symbol": "index"})),
        ("wright_call_graph", serde_json::json!({})),
        ("wright_check", serde_json::json!({})),
        // #532: the report tools carry `all` through to the same service
        // result the stdio transport reports.
        ("wright_analyze", serde_json::json!({"all": true})),
        ("wright_inspect", serde_json::json!({"all": true})),
        ("wright_lint", serde_json::json!({"severity": "info"})),
        ("wright_lint", serde_json::json!({"all": true})),
        ("wright_cost_estimate", serde_json::json!({})),
        // #472: the edit tools run without `sources` — the service reads the
        // on-disk text.
        (
            "wright_validate_edit_transaction",
            serde_json::json!({
                "transaction": {
                    "edits": [{
                        "kind": "edit",
                        "source": key,
                        "source_identity": wright_driver::input_identity(&source),
                        "range": {
                            "start_line": 6, "start_col": 8,
                            "end_line": 6, "end_col": 21,
                        },
                        "new_text": "bounded loop",
                    }]
                }
            }),
        ),
        // ADR-0029: `inputIdentity` is the freshness precondition; the edit
        // tool's MCP schema omits `sources` like the other edit tools.
        (
            "wright_edit_statements",
            serde_json::json!({
                "inputIdentity": wright_driver::input_identity(&source),
                "edits": [{
                    "rule": "bounded while", "list": "actions", "index": 0,
                    "change": "insert",
                    "statements": [{"call": "wait", "args": [0.5, {"enum": "Wait", "member": "IGNORE_CONDITION"}]}],
                }],
            }),
        ),
        // #529: vocabulary lookup needs no project state — the payload is
        // the same result object stdio returns.
        (
            "wright_lookup",
            serde_json::json!({"language": "workshop", "query": "wait"}),
        ),
    ];
    let mut mcp_lines = vec![];
    let mut stdio_lines = vec![];
    for (id, (tool, arguments)) in cases.iter().enumerate() {
        mcp_lines.push(mcp_call(id as u64 + 1, tool, arguments.clone()));
        let op = match tool.trim_start_matches("wright_") {
            "call_graph" => "callGraph",
            "cost_estimate" => "costEstimate",
            "validate_edit_transaction" => "validateEditTransaction",
            "provider_validate_edit" => "providerValidateEdit",
            "edit_statements" => "editStatements",
            other => other,
        };
        let mut request = arguments.clone();
        request["op"] = serde_json::json!(op);
        stdio_lines.push(serde_json::to_string(&request).unwrap());
    }
    let mcp = run_lines(
        "mcp",
        &input,
        &mcp_lines.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let stdio = run_lines(
        "stdio",
        &input,
        &stdio_lines.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    assert_eq!(mcp.len(), cases.len());
    for (i, (tool, _)) in cases.iter().enumerate() {
        let response = &mcp[i];
        assert_eq!(response["id"], i as u64 + 1);
        assert!(response.get("error").is_none(), "{tool}: {response}");
        assert!(
            response["result"].get("isError").is_none(),
            "{tool} is not a refusal: {response}"
        );
        assert_eq!(
            mcp_payload(response),
            stdio[i]["result"],
            "{tool}: MCP payload equals the service result"
        );
    }
}

#[test]
fn mcp_transport_preserves_refusal_codes() {
    // #473: service refusals become `isError` tool results carrying the
    // original {code, message}; they never become successes, and adapter
    // protocol errors stay JSON-RPC errors, not tool results.
    let input = corpus_workshop("synthetic/control-flow");
    let dup = {
        let dir = std::env::temp_dir().join(format!("wright-mcp-dup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dup.ws");
        std::fs::write(
            &path,
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
        )
        .unwrap();
        path
    };
    let stale = {
        let dir = std::env::temp_dir().join(format!("wright-mcp-stale-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("edit.ws");
        std::fs::write(
            &path,
            "variables {\n    global:\n        0: score\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(score, 1);\n    }\n}\n",
        )
        .unwrap();
        path
    };

    let lines = [
        mcp_call(
            1,
            "wright_references",
            serde_json::json!({"symbol": "nope"}),
        ),
        mcp_call(2, "wright_usage", serde_json::json!({"symbol": "nope"})),
        mcp_call(
            3,
            "wright_references",
            serde_json::json!({"symbol": 42, "snapshot": 1}),
        ),
        mcp_call(4, "wright_not_a_tool", serde_json::json!({})),
        mcp_call(5, "wright_symbols", serde_json::json!("not-an-object")),
        mcp_call(6, "wright_references", serde_json::json!({"symbol": 0})),
    ];
    let responses = run_lines(
        "mcp",
        &input,
        &lines.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    assert_eq!(responses.len(), 6);
    let refused = |response: &serde_json::Value| -> serde_json::Value {
        assert_eq!(response["result"]["isError"], true, "{response}");
        mcp_payload(response)
    };
    assert_eq!(refused(&responses[0])["code"], "unknown-symbol");
    assert_eq!(refused(&responses[1])["code"], "unknown-symbol");
    assert_eq!(refused(&responses[2])["code"], "invalid-id");
    // A numeric address without its issuing `snapshot` refuses `stale-id` (#595).
    assert_eq!(refused(&responses[5])["code"], "stale-id");
    assert_eq!(responses[3]["error"]["code"], -32602);
    assert_eq!(responses[4]["error"]["code"], -32602);

    let dup_line = mcp_call(1, "wright_references", serde_json::json!({"symbol": "dup"}));
    let dup_responses = run_lines("mcp", &dup, &[dup_line.as_str()]);
    let payload = serde_json::json!(dup_responses);
    let refusal = serde_json::from_str::<serde_json::Value>(
        payload[0]["result"]["content"][0]["text"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(dup_responses[0]["result"]["isError"], true);
    assert_eq!(refusal["code"], "ambiguous-symbol");

    let stale_key = stale.to_string_lossy().into_owned();
    let stale_line = mcp_call(
        1,
        "wright_validate_edit_transaction",
        serde_json::json!({
            "sources": { stale_key.clone(): "changed text" },
            "transaction": {
                "edits": [{
                    "kind": "edit",
                    "source": stale_key,
                    "source_identity": wright_driver::input_identity(&std::fs::read_to_string(&stale).unwrap()),
                    "range": { "start_line": 1, "start_col": 1, "end_line": 1, "end_col": 2 },
                    "new_text": "x",
                }]
            }
        }),
    );
    let stale_responses = run_lines("mcp", &stale, &[stale_line.as_str()]);
    // Edit outcomes ride the result payload (`ok: false` + diagnostics), not
    // the error envelope — the adapter passes the service result through
    // unchanged either way.
    let refusal = mcp_payload(&stale_responses[0]);
    assert_eq!(refusal["ok"], false, "{refusal}");
    assert_eq!(
        refusal["diagnostics"][0]["code"], "edit-stale-source",
        "{refusal}"
    );
    let _ = std::fs::remove_dir_all(dup.parent().unwrap());
    let _ = std::fs::remove_dir_all(stale.parent().unwrap());
}

#[test]
fn mcp_transport_observes_disk_changes_and_enforces_fresh_ids() {
    // #473 + #471 + #595: an MCP session is long-lived like `serve` stdio —
    // a file edit on disk is reflected by the next tools/call, and numeric
    // ids are scoped to the `snapshot` generation the response reported:
    // absent or carried over from a dropped generation, a numeric address
    // refuses `stale-id`.
    use std::io::{BufRead, BufReader};
    let dir = std::env::temp_dir().join(format!("wright-serve-mcp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("session.ws");
    let v1 = "variables {\n    global:\n        0: score\n}\n\nrule (\"setup\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(score, 1);\n    }\n}\n";
    let v2 = "variables {\n    global:\n        0: points\n}\n\nrule (\"setup\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(points, 1);\n    }\n}\n";
    std::fs::write(&input, v1).unwrap();

    let mut child = Command::new(wright())
        .args(["serve", "--transport", "mcp"])
        .arg(&input)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wright serve spawns");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut id = 0u64;
    let mut exchange = |method: serde_json::Value| -> serde_json::Value {
        id += 1;
        let mut request = method;
        request["jsonrpc"] = serde_json::json!("2.0");
        request["id"] = serde_json::json!(id);
        writeln!(stdin, "{request}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        assert_ne!(
            stdout.read_line(&mut line).unwrap(),
            0,
            "no answer to {request}"
        );
        serde_json::from_str(&line).expect("JSON response")
    };

    let call = |name: &str, arguments: serde_json::Value| {
        serde_json::json!({
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments },
        })
    };
    let symbols = exchange(call("wright_symbols", serde_json::json!({})));
    let names: Vec<String> = mcp_payload(&symbols)["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|symbol| symbol["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["score", "setup"]);
    let snapshot = mcp_payload(&symbols)["snapshot"].as_u64().unwrap();

    // A numeric address without its issuing generation refuses `stale-id`;
    // echoing the generation resolves.
    let bare = exchange(call("wright_references", serde_json::json!({"symbol": 0})));
    assert_eq!(mcp_payload(&bare)["code"], "stale-id", "{bare}");
    let numeric = exchange(call(
        "wright_references",
        serde_json::json!({"symbol": 0, "snapshot": snapshot}),
    ));
    assert!(numeric["result"].get("isError").is_none(), "{numeric}");

    std::fs::write(&input, v2).unwrap();

    let stale = exchange(call(
        "wright_references",
        serde_json::json!({"symbol": 0, "snapshot": snapshot}),
    ));
    assert_eq!(mcp_payload(&stale)["code"], "stale-id", "{stale}");
    assert_eq!(stale["result"]["isError"], true);
    let by_name = exchange(call(
        "wright_references",
        serde_json::json!({"symbol": "points", "snapshot": snapshot}),
    ));
    assert!(
        by_name["result"].get("isError").is_none(),
        "a name ignores the dropped generation: {by_name}"
    );

    let symbols = exchange(call("wright_symbols", serde_json::json!({})));
    let names: Vec<String> = mcp_payload(&symbols)["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|symbol| symbol["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["points", "setup"]);
    let snapshot = mcp_payload(&symbols)["snapshot"].as_u64().unwrap();
    let numeric = exchange(call(
        "wright_references",
        serde_json::json!({"symbol": 0, "snapshot": snapshot}),
    ));
    assert!(numeric["result"].get("isError").is_none(), "{numeric}");

    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "serve exited: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mcp_transport_bootstraps_over_a_broken_project_and_recovers() {
    // #512: MCP `initialize`/`tools/list` are independent of the session
    // project — a missing input must not prevent bootstrap or hide the
    // stable tool set. `tools/call` maps the project-load refusal to the
    // `isError` tool result, and the same server adopts the repaired input.
    use std::io::{BufRead, BufReader};
    let dir = std::env::temp_dir().join(format!("wright-serve-mcp-512-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("session.ws");

    let mut child = Command::new(wright())
        .args(["serve", "--transport", "mcp"])
        .arg(&input)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wright serve spawns");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut exchange = |request: serde_json::Value| -> serde_json::Value {
        writeln!(stdin, "{request}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        assert_ne!(
            stdout.read_line(&mut line).unwrap(),
            0,
            "no answer to {request}"
        );
        serde_json::from_str(&line).expect("JSON response")
    };

    let initialize = exchange(serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}},
    }));
    assert_eq!(initialize["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(
        initialize["result"]["serverInfo"]["name"],
        "wright-tool-service"
    );

    // The broken project does not hide tools: the set is the same one the
    // server advertises once the project loads (compared below), and it
    // still contains a program-reading tool.
    let list = exchange(serde_json::json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/list",
    }));
    let broken_names: Vec<String> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        broken_names.iter().any(|name| name == "wright_project"),
        "{list}"
    );

    // A program-reading tool call is the service's structured refusal as an
    // `isError` tool result — not a protocol error.
    let refused = exchange(serde_json::json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "wright_project", "arguments": {}},
    }));
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    assert_eq!(mcp_payload(&refused)["code"], "input-io", "{refused}");

    std::fs::write(
        &input,
        "variables {\n    global:\n        0: score\n}\n\nrule (\"setup\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(score, 1);\n    }\n}\n",
    )
    .unwrap();
    let healed = exchange(serde_json::json!({
        "jsonrpc": "2.0", "id": 4, "method": "tools/call",
        "params": {"name": "wright_project", "arguments": {}},
    }));
    assert!(healed["result"].get("isError").is_none(), "{healed}");
    assert_eq!(mcp_payload(&healed)["rules"], 1, "{healed}");

    let healed_list = exchange(serde_json::json!({
        "jsonrpc": "2.0", "id": 5, "method": "tools/list",
    }));
    let healed_names: Vec<String> = healed_list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(healed_names, broken_names, "{healed_list}");

    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "serve exited: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}
