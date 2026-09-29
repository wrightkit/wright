//! ToolService preserves the machine contract over canonical Workshop input
//! and reports provider-owned OPY gaps as structured refusals.

use std::path::{Path, PathBuf};

use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::{
    CompilerSession, FindingSelection, InputSpec, SessionConfig, Severity, SourceKind,
};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn workshop_path() -> PathBuf {
    workspace_root().join("tests/fixtures/workshop/synthetic/control-flow.ws")
}

#[test]
fn tool_service_queries_canonical_workshop() {
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    for request in [
        ToolRequest::Capabilities,
        ToolRequest::Project,
        ToolRequest::Rules,
        ToolRequest::Findings(FindingSelection::default()),
        ToolRequest::CostEstimate(FindingSelection::default()),
    ] {
        assert!(matches!(service.handle(&request), ToolResponse::Ok { .. }));
    }
}

#[test]
fn tool_service_inspect_matches_session_inspect() {
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let expected = session.inspect();
    let mut service = ToolService::new(&mut session).unwrap();
    let actual = service.inspect();

    assert_eq!(actual.ok, expected.ok);
    assert_eq!(
        serde_json::to_value(actual.result).unwrap(),
        serde_json::to_value(expected.result).unwrap()
    );
}

#[test]
fn tool_service_lint_queries_keep_the_session_configuration() {
    let mut lint = wright_driver::config::LintConfig::default();
    assert!(lint.set_severity_by_name("min-wait-loop", "error"));
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        lint,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    let lint_rules = match service.handle(&ToolRequest::LintRules) {
        ToolResponse::Ok { result } => result,
        ToolResponse::Error { error } => panic!("lintRules failed: {error:?}"),
    };
    assert_eq!(
        lint_rules["config"]["rules"]["min-wait-loop"]["severity"],
        "error"
    );

    let lint = match service.handle(&ToolRequest::Lint(FindingSelection::default())) {
        ToolResponse::Ok { result } => result,
        ToolResponse::Error { error } => panic!("lint failed: {error:?}"),
    };
    assert_eq!(lint["config"], lint_rules["config"]);
    // #431: `lint` inlines only the finding-interpretation fields;
    // `lintRules` remains the full-metadata surface.
    let lint_rule = lint["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["id"] == "min-wait-loop")
        .expect("lint lists every registered rule");
    assert_eq!(
        lint_rule,
        &serde_json::json!({ "id": "min-wait-loop", "effectiveSeverity": "error" })
    );
    let full_rule = lint_rules["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["id"] == "min-wait-loop")
        .expect("lintRules lists every registered rule");
    assert!(full_rule["summary"].is_string());
    let configured_finding = lint["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["code"] == "min-wait-loop")
        .expect("the fixture triggers min-wait-loop");
    assert_eq!(configured_finding["severity"], "error");

    let default_findings = match service.handle(&ToolRequest::Findings(FindingSelection::default()))
    {
        ToolResponse::Ok { result } => result,
        ToolResponse::Error { error } => panic!("findings failed: {error:?}"),
    };
    let default_finding = default_findings
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["code"] == "min-wait-loop")
        .expect("the fixture triggers min-wait-loop");
    assert_eq!(default_finding["severity"], "warning");
}

#[test]
fn tool_service_routes_workflows_through_the_agent_request_contract() {
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    let capabilities = service.capabilities();
    assert_eq!(capabilities.agent_contract, "wright-agent/v1");
    for (request, command) in [
        (ToolRequest::Compile, "compile"),
        (ToolRequest::Check, "check"),
        (ToolRequest::Analyze, "analyze"),
        (ToolRequest::Inspect, "inspect"),
    ] {
        let ToolResponse::Ok { result } = service.handle(&request) else {
            panic!("{command} returns its structured envelope");
        };
        assert_eq!(result["command"], command);
        assert_eq!(result["wright"]["contract"], "wright-result/v1");
    }
}

#[test]
fn agent_finding_selection_filters_and_truncates_without_touching_defaults() {
    // #430: `real-world/overpy-cake.ws` produces 10 findings — 9 identical
    // `repeated-value` warnings and one `min-wait-loop` warning.
    let input = workspace_root().join("tests/fixtures/workshop/real-world/overpy-cake.ws");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    let mut result_of = |request| match service.handle(&request) {
        ToolResponse::Ok { result } => result,
        ToolResponse::Error { error } => panic!("{error:?}"),
    };

    // No selection fields: the bare array result is unchanged.
    let plain = result_of(ToolRequest::Findings(FindingSelection::default()));
    assert!(plain.is_array(), "the default result stays a bare array");
    assert_eq!(plain.as_array().unwrap().len(), 10);

    // `severity` is a threshold; `error` selects none of the warning findings
    // while the summary still reports the true total.
    let errors = result_of(ToolRequest::Findings(FindingSelection {
        severity: Some(Severity::Error),
        ..FindingSelection::default()
    }));
    assert!(errors["findings"].as_array().unwrap().is_empty());
    assert_eq!(
        errors["selection"],
        serde_json::json!({"total": 10, "withheld": 0})
    );

    // `rule` + `max` on `lint` embed the same summary beside the findings.
    let lint = result_of(ToolRequest::Lint(FindingSelection {
        rule: Some("repeated-value".to_string()),
        max: Some(2),
        ..FindingSelection::default()
    }));
    assert_eq!(lint["findings"].as_array().unwrap().len(), 2);
    assert_eq!(
        lint["selection"],
        serde_json::json!({"total": 10, "withheld": 7})
    );

    // The same fields apply to `costEstimate` findings.
    let cost = result_of(ToolRequest::CostEstimate(FindingSelection {
        max: Some(1),
        ..FindingSelection::default()
    }));
    assert_eq!(cost["findings"].as_array().unwrap().len(), 1);
    assert!(cost["selection"]["withheld"].as_u64().unwrap() > 0);
}

#[test]
fn an_unknown_rule_id_is_a_service_error_not_an_empty_result() {
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    for request in [
        ToolRequest::Findings(FindingSelection {
            rule: Some("not-a-rule".to_string()),
            ..FindingSelection::default()
        }),
        ToolRequest::Lint(FindingSelection {
            rule: Some("not-a-rule".to_string()),
            ..FindingSelection::default()
        }),
        ToolRequest::CostEstimate(FindingSelection {
            rule: Some("not-a-rule".to_string()),
            ..FindingSelection::default()
        }),
    ] {
        match service.handle(&request) {
            ToolResponse::Error { error } => {
                assert_eq!(error.code, "invalid-selection");
                assert!(error.message.contains("not-a-rule"));
            }
            ToolResponse::Ok { result } => {
                panic!("an unknown rule id must not return a result: {result}")
            }
        }
    }
}

#[test]
fn an_empty_selection_serializes_identically_to_no_selection() {
    // Omitting every selection option reproduces the previous JSON output
    // byte-for-byte (#430).
    let config = SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    };
    let plain = CompilerSession::new(config.clone()).unwrap().lint();
    let selected = CompilerSession::new(SessionConfig {
        selection: FindingSelection::default(),
        ..config
    })
    .unwrap()
    .lint();
    assert_eq!(
        serde_json::to_vec(&plain).unwrap(),
        serde_json::to_vec(&selected).unwrap(),
    );
}

#[test]
fn tool_service_keeps_provider_refusals_structured() {
    let path = workspace_root().join("tests/fixtures/opy/basic-rule.opy");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path),
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    })
    .unwrap();
    let diagnostic = match session.load() {
        Ok(_) => panic!("static OPY fallback must not load"),
        Err(diagnostic) => diagnostic,
    };
    assert_eq!(diagnostic.code, "source-provider-unavailable");
}

// ── Semantic query commands and name addressing (#429) ───────────────────────

fn declarations_path() -> PathBuf {
    workspace_root().join("tests/fixtures/workshop/synthetic/declarations-rules.ws")
}

fn result_of(service: &mut ToolService<'_>, request: &ToolRequest) -> serde_json::Value {
    match service.handle(request) {
        ToolResponse::Ok { result } => result,
        ToolResponse::Error { error } => panic!("{request:?} failed: {error:?}"),
    }
}

fn temp_workshop(name: &str, text: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-driver-429-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn name_addressing_matches_numeric_addressing_and_resolves_span_paths() {
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(declarations_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    // Discover the numeric ids through the shared semantic index rather than
    // hardcoding them.
    let symbols = result_of(&mut service, &ToolRequest::Symbols { kind: None });
    let symbol_id = |name: &str| {
        symbols
            .as_array()
            .unwrap()
            .iter()
            .find(|symbol| symbol["name"] == name)
            .and_then(|symbol| symbol["id"].as_u64())
            .unwrap_or_else(|| panic!("no symbol named {name}")) as u32
    };
    let rules = result_of(&mut service, &ToolRequest::Rules);
    let rule_index = |name: &str| {
        rules
            .as_array()
            .unwrap()
            .iter()
            .find(|rule| rule["name"] == name)
            .and_then(|rule| rule["id"].as_u64())
            .unwrap_or_else(|| panic!("no rule named {name}")) as u32
    };

    // Names and ids produce the same payloads.
    for (by_name, by_id) in [
        (
            ToolRequest::References {
                symbol: "score".into(),
            },
            ToolRequest::References {
                symbol: symbol_id("score").into(),
            },
        ),
        (
            ToolRequest::Usage {
                symbol: "score".into(),
            },
            ToolRequest::Usage {
                symbol: symbol_id("score").into(),
            },
        ),
        (
            ToolRequest::Cfg {
                rule: "player starts".into(),
            },
            ToolRequest::Cfg {
                rule: rule_index("player starts").into(),
            },
        ),
    ] {
        assert_eq!(
            result_of(&mut service, &by_name),
            result_of(&mut service, &by_id)
        );
    }

    // `usage` echoes the resolved identity so a name-addressed caller can
    // correlate with `symbols`.
    let usage = result_of(
        &mut service,
        &ToolRequest::Usage {
            symbol: "score".into(),
        },
    );
    assert_eq!(usage["id"], symbol_id("score"));
    assert_eq!(usage["kind"], "globalVariable");
    assert_eq!(usage["symbol"], "score");

    // The split numbering spaces are why names exist: "player starts" is
    // symbol id 4 but rule index 1; its symbol id is an invalid `cfg` target.
    assert_eq!(symbol_id("player starts"), 4);
    assert_eq!(rule_index("player starts"), 1);
    match service.handle(&ToolRequest::Cfg { rule: 4.into() }) {
        ToolResponse::Error { error } => assert_eq!(error.code, "invalid-id"),
        ToolResponse::Ok { result } => panic!("rule index 4 must not exist: {result}"),
    }

    // References and symbols carry resolved source paths, not bare file ids.
    let references = result_of(
        &mut service,
        &ToolRequest::References {
            symbol: "score".into(),
        },
    );
    for reference in references.as_array().unwrap() {
        assert_eq!(
            reference["span"]["path"].as_str().unwrap(),
            "declarations-rules.ws",
            "reference spans resolve to the root-relative path: {reference}"
        );
    }
    // Symbols whose model carries a span (rules, subroutine symbols) resolve
    // the same path; declaration-less spans stay `null`.
    let spanned: Vec<_> = symbols
        .as_array()
        .unwrap()
        .iter()
        .filter(|symbol| symbol["span"].is_object())
        .collect();
    assert!(!spanned.is_empty());
    for symbol in spanned {
        assert_eq!(symbol["span"]["path"], "declarations-rules.ws");
    }
}

#[test]
fn name_addressing_reports_unknown_and_ambiguous_names_as_structured_errors() {
    let source = r#"
variables {
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
"#;
    let path = temp_workshop("dup.ws", source);
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    for (request, code) in [
        (
            ToolRequest::References {
                symbol: "nope".into(),
            },
            "unknown-symbol",
        ),
        (
            ToolRequest::Usage {
                symbol: "nope".into(),
            },
            "unknown-symbol",
        ),
        (
            ToolRequest::Cfg {
                rule: "nope".into(),
            },
            "unknown-rule",
        ),
        (
            ToolRequest::References {
                symbol: "dup".into(),
            },
            "ambiguous-symbol",
        ),
        (ToolRequest::Cfg { rule: "dup".into() }, "ambiguous-rule"),
    ] {
        match service.handle(&request) {
            ToolResponse::Error { error } => {
                assert_eq!(error.code, code, "{request:?}");
                assert!(error.message.contains("dup") || error.message.contains("nope"));
            }
            ToolResponse::Ok { result } => {
                panic!("{request:?} must not return a result: {result}")
            }
        }
    }
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn session_query_workflows_return_the_agent_operation_payloads() {
    // The CLI's `symbols`/`refs`/`cfg`/`callgraph`/`cost` results are the
    // same payloads the agent operations serve, by construction (#429).
    let input = || SessionConfig {
        input: InputSpec::Path(declarations_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    };

    let mut agent = CompilerSession::new(input()).unwrap();
    let mut service = ToolService::new(&mut agent).unwrap();
    let agent_symbols = result_of(&mut service, &ToolRequest::Symbols { kind: None });
    let agent_references = result_of(
        &mut service,
        &ToolRequest::References {
            symbol: "score".into(),
        },
    );
    let agent_usage = result_of(
        &mut service,
        &ToolRequest::Usage {
            symbol: "score".into(),
        },
    );
    let agent_cfg = result_of(
        &mut service,
        &ToolRequest::Cfg {
            rule: "player starts".into(),
        },
    );
    let agent_callgraph = result_of(&mut service, &ToolRequest::CallGraph);
    let agent_cost = result_of(
        &mut service,
        &ToolRequest::CostEstimate(FindingSelection::default()),
    );
    drop(service);

    let mut cli = CompilerSession::new(input()).unwrap();
    assert_eq!(
        serde_json::to_value(cli.symbols(None).result).unwrap(),
        agent_symbols
    );

    // `refs` is the usage payload plus the references list under `references`.
    let refs = serde_json::to_value(cli.refs("score").result).unwrap();
    let mut expected_refs = agent_usage.as_object().unwrap().clone();
    expected_refs.insert("references".to_string(), agent_references);
    assert_eq!(refs, serde_json::Value::Object(expected_refs));

    assert_eq!(
        serde_json::to_value(cli.cfg("player starts").result).unwrap(),
        agent_cfg
    );
    assert_eq!(
        serde_json::to_value(cli.callgraph().result).unwrap(),
        agent_callgraph
    );
    assert_eq!(serde_json::to_value(cli.cost().result).unwrap(), agent_cost);
}

#[test]
fn analyze_reports_persistent_object_facts() {
    let source = r#"
rule ("effect") {
    event {
        Ongoing - Global;
    }
    actions {
        Create Effect(All Players(All), Orb, Red, Vector(0, 0, 0), 1, None);
    }
}
"#;
    let path = temp_workshop("effect.ws", source);
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let envelope = session.analyze();
    assert!(envelope.ok);
    let objects = envelope.result.facts["persistentObjects"]
        .as_array()
        .expect("analyze reports facts.persistentObjects");
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0]["kind"], "effect");
    assert_eq!(objects[0]["executionScope"], "global");
    assert_eq!(objects[0]["visibility"], "all-players");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
