//! ToolService preserves the machine contract over canonical Workshop input
//! and reports provider-owned OPY gaps as structured refusals.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wright_driver::edit::{EditRange, EditTransaction, RenameTarget, SourceEdit};
use wright_driver::service::{Address, ToolRequest, ToolResponse, ToolService};
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
        Create Effect(All Players(All Teams), Orb, Red, Vector(0, 0, 0), 1, None);
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

/// `facts` carries the #445 layers: canonical element cost, per-rule
/// attribution with condition trees, performance/stability risk indicators,
/// and persistent-object facts — all with resolved locations.
#[test]
fn analyze_reports_cost_complexity_and_risk_facts() {
    let source = r#"
variables {
    global:
        0: score
}

rule ("hot path") {
    event {
        Ongoing - Global;
    }
    conditions {
        Global.score == 1;
        Distance Between(Vector(0, 0, 0), Vector(1, 0, 0)) < 10;
    }
    actions {
        Create Effect(All Players(All Teams), Orb, Red, Vector(0, 0, 0), 1, None);
    }
}
"#;
    let path = temp_workshop("hot.ws", source);
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let envelope = session.analyze();
    assert!(envelope.ok, "{:?}", envelope.diagnostics);
    let facts = &envelope.result.facts;

    let cost = &facts["cost"];
    let total = cost["elementCount"]
        .as_u64()
        .expect("canonical element count (#445)");
    assert!(total > 0);
    assert_eq!(cost["counts"]["rules"], 1);
    assert_eq!(cost["counts"]["conditions"], 2);
    assert_eq!(cost["counts"]["actions"], 1);

    let rule = &facts["rules"][0];
    assert_eq!(rule["elements"].as_u64(), Some(total));
    let conditions = rule["conditions"].as_array().expect("condition trees");
    assert_eq!(conditions.len(), 2);
    let condition_elements: u64 = conditions
        .iter()
        .map(|condition| condition["elements"].as_u64().expect("condition cost"))
        .sum();
    assert_eq!(rule["conditionElements"].as_u64(), Some(condition_elements));
    // Resolved locations: authored path on the source-parsed input (#445).
    assert_eq!(rule["span"]["path"], "hot.ws");
    assert_eq!(conditions[1]["span"]["path"], "hot.ws");

    let risks = facts["risks"].as_array().expect("risk indicators (#445)");
    assert!(
        risks.iter().any(|finding| {
            finding["code"] == "ongoing-condition-hot-path"
                && finding["evidence"] == "heuristic"
                && finding["span"]["path"] == "hot.ws"
        }),
        "the ongoing-condition hot path is a heuristic risk fact: {risks:?}"
    );
    assert!(
        risks
            .iter()
            .all(|finding| finding["code"] != "duplicate-condition"),
        "correctness findings stay out of the risk frame"
    );

    let objects = facts["persistentObjects"].as_array().expect("object facts");
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0]["span"]["path"], "hot.ws");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// Correctness findings (e.g. `duplicate-condition`) are not performance or
/// stability risks, so they remain visible through `lint`/`findings` but do
/// not enter `facts.risks` (#445, #209).
#[test]
fn analyze_risks_exclude_correctness_findings() {
    let source = r#"
variables {
    global:
        0: index
}

rule ("dup") {
    event {
        Ongoing - Global;
    }
    actions {
        If(Compare(Global.index, ==, 0));
            Set Global Variable(index, 1);
        Else If(Compare(Global.index, ==, 0));
            Set Global Variable(index, 2);
        End;
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
    let lint = session.lint();
    assert!(
        lint.result
            .findings
            .as_array()
            .is_some_and(|findings| findings
                .iter()
                .any(|finding| finding["code"] == "duplicate-condition")),
        "the fixture triggers a correctness finding"
    );
    let analyze = session.analyze();
    assert!(analyze.ok);
    let risks = analyze.result.facts["risks"]
        .as_array()
        .expect("risks array");
    assert!(
        risks
            .iter()
            .all(|finding| finding["code"] != "duplicate-condition"),
        "correctness findings are excluded from risks: {risks:?}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

// ── Session freshness and stale-id refusal (#471) ───────────────────────────

/// `score` is symbol id 0 and `"setup"` rule index 0.
const FRESHNESS_V1: &str = r#"
variables {
    global:
        0: score
}
rule ("setup") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(score, 1);
    }
}
"#;

/// The same input after an edit: `score` was renamed to `points`, `"extra"`
/// was added as rule index 1, `sqrt` is an unknown value `check` reports,
/// and the `While`/`Wait(0.016)` loop triggers `min-wait-loop`.
const FRESHNESS_V2: &str = r#"
variables {
    global:
        0: points
}
rule ("setup") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(points, sqrt(4));
    }
}
rule ("extra") {
    event {
        Ongoing - Global;
    }
    actions {
        While(Compare(Global.points, <, 3));
            Modify Global Variable(points, Add, 1);
            Wait(0.016, Ignore Condition);
        End;
    }
}
"#;

/// A second variant with deliberately duplicated names for the
/// `ambiguous-*` recovery path.
const FRESHNESS_AMBIGUOUS: &str = r#"
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

fn freshness_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-driver-471-{tag}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn refusal_code(service: &mut ToolService<'_>, request: &ToolRequest) -> String {
    match service.handle(request) {
        ToolResponse::Error { error } => error.code,
        ToolResponse::Ok { result } => panic!("{request:?} must refuse: {result}"),
    }
}

#[test]
fn a_changed_input_is_reloaded_and_serves_the_new_program() {
    let dir = freshness_dir("edit");
    let input = dir.join("program.ws");
    std::fs::write(&input, FRESHNESS_V1).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    let names = |result: serde_json::Value| {
        result
            .as_array()
            .unwrap()
            .iter()
            .map(|symbol| symbol["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let before = names(result_of(
        &mut service,
        &ToolRequest::Symbols { kind: None },
    ));
    assert_eq!(before, ["score", "setup"]);

    for request in [
        ToolRequest::References { symbol: 0.into() },
        ToolRequest::Findings(FindingSelection::default()),
        ToolRequest::LintRules,
        ToolRequest::Inspect,
        ToolRequest::Analyze,
    ] {
        let before = serde_json::to_string(&service.handle(&request)).unwrap();
        assert_eq!(
            before,
            serde_json::to_string(&service.handle(&request)).unwrap()
        );
    }
    assert_eq!(
        serde_json::to_value(service.analyze()).unwrap(),
        result_of(&mut service, &ToolRequest::Analyze)
    );

    std::fs::write(&input, FRESHNESS_V2).unwrap();

    let after = names(result_of(
        &mut service,
        &ToolRequest::Symbols { kind: None },
    ));
    assert_eq!(after, ["points", "setup", "extra"]);
    assert_eq!(service.reload_count(), 1);

    let project = result_of(&mut service, &ToolRequest::Project);
    assert_eq!(project["rules"], 2);
    assert_eq!(project["symbols"], 3);

    let check = result_of(&mut service, &ToolRequest::Check);
    assert_eq!(check["ok"], false);
    assert!(
        check["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"].as_str().unwrap().contains("sqrt")),
        "check reports the new program's residual: {check}"
    );
    let lint = result_of(
        &mut service,
        &ToolRequest::Lint(FindingSelection::default()),
    );
    assert!(
        lint["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["code"] == "min-wait-loop"),
        "lint reports the new program's findings: {lint}"
    );

    // The reloaded input is stable: repeat requests never reparse.
    result_of(&mut service, &ToolRequest::Project);
    result_of(&mut service, &ToolRequest::Rules);
    assert_eq!(service.reload_count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unchanged_input_is_never_reloaded() {
    let dir = freshness_dir("stable");
    let input = dir.join("program.ws");
    std::fs::write(&input, FRESHNESS_V1).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    for request in [
        ToolRequest::Project,
        ToolRequest::Rules,
        ToolRequest::Symbols { kind: None },
        ToolRequest::Findings(FindingSelection::default()),
    ] {
        result_of(&mut service, &request);
    }
    assert_eq!(service.reload_count(), 0);

    // Rewriting identical bytes is still the same input.
    std::fs::write(&input, FRESHNESS_V1).unwrap();
    result_of(&mut service, &ToolRequest::Symbols { kind: None });
    assert_eq!(service.reload_count(), 0);

    std::fs::write(&input, FRESHNESS_V2).unwrap();
    result_of(&mut service, &ToolRequest::Symbols { kind: None });
    assert_eq!(service.reload_count(), 1);
    result_of(&mut service, &ToolRequest::Project);
    result_of(&mut service, &ToolRequest::Rules);
    assert_eq!(service.reload_count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn directory_inputs_observe_added_and_removed_sources() {
    let dir = freshness_dir("dir");
    std::fs::write(dir.join("program.ws"), FRESHNESS_V1).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(dir.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    let rules = |service: &mut ToolService<'_>| {
        result_of(service, &ToolRequest::Rules)
            .as_array()
            .unwrap()
            .len()
    };
    assert_eq!(rules(&mut service), 1);

    // Editing a member file is observed.
    std::fs::write(dir.join("program.ws"), FRESHNESS_V2).unwrap();
    assert_eq!(rules(&mut service), 2);
    assert_eq!(service.reload_count(), 1);

    // An added member changes the fingerprint; the reload surfaces the
    // resolver's ambiguity refusal rather than serving either file's program.
    std::fs::write(dir.join("other.ws"), FRESHNESS_V1).unwrap();
    assert_eq!(
        refusal_code(&mut service, &ToolRequest::Rules),
        "input-kind-ambiguous"
    );
    // The broken state refuses consistently until the directory heals.
    assert_eq!(
        refusal_code(&mut service, &ToolRequest::Project),
        "input-kind-ambiguous"
    );

    std::fs::remove_file(dir.join("other.ws")).unwrap();
    // The failed refresh invalidated the served snapshot (#512): restoring
    // the exact previous disk state still reloads a fresh program — the
    // fingerprint-identical bytes do not resurrect it — and ids issued by
    // the dropped program refuse `stale-id` until the space is re-observed.
    assert_eq!(
        refusal_code(&mut service, &ToolRequest::Cfg { rule: 0.into() }),
        "stale-id"
    );
    assert_eq!(rules(&mut service), 2);
    assert_eq!(service.reload_count(), 2);
    result_of(&mut service, &ToolRequest::Cfg { rule: 0.into() });

    // Removing the sole member is equally observable.
    std::fs::remove_file(dir.join("program.ws")).unwrap();
    assert_eq!(
        refusal_code(&mut service, &ToolRequest::Project),
        "input-kind-ambiguous"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_reload_refuses_requests_until_the_input_heals() {
    let dir = freshness_dir("broken");
    let input = dir.join("program.ws");
    std::fs::write(&input, FRESHNESS_V1).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    assert_eq!(service.reload_count(), 0);

    // The locale still resolves but the program is malformed: the reload
    // failure is the parser's own `parse-error` diagnostic.
    std::fs::write(
        &input,
        "variables {\n    global:\n        0: score\n}\nrule (\"setup\") {\n",
    )
    .unwrap();
    // Every program-reading request refuses with the loader's diagnostic;
    // the stale program is never served. `capabilities` answers regardless.
    for request in [
        ToolRequest::Symbols { kind: None },
        ToolRequest::Rules,
        ToolRequest::Project,
        ToolRequest::Check,
    ] {
        let code = refusal_code(&mut service, &request);
        assert_eq!(code, "parse-error", "{request:?}");
    }
    assert!(matches!(
        service.handle(&ToolRequest::Capabilities),
        ToolResponse::Ok { .. }
    ));

    std::fs::write(&input, FRESHNESS_V2).unwrap();
    let symbols = result_of(&mut service, &ToolRequest::Symbols { kind: None });
    assert!(
        symbols
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["name"] == "points"),
        "the healed input serves the new program: {symbols}"
    );
    assert_eq!(service.reload_count(), 1);

    // Deleting the input is a reload failure, not a snapshot to serve.
    std::fs::remove_file(&input).unwrap();
    let code = refusal_code(&mut service, &ToolRequest::Project);
    assert_eq!(code, "input-io");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stale_numeric_ids_refuse_until_the_new_space_is_observed() {
    let dir = freshness_dir("stale");
    let input = dir.join("program.ws");
    std::fs::write(&input, FRESHNESS_V1).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    // Both spaces are current on a fresh service: `score` is symbol id 0.
    result_of(&mut service, &ToolRequest::References { symbol: 0.into() });
    result_of(&mut service, &ToolRequest::Cfg { rule: 0.into() });

    std::fs::write(&input, FRESHNESS_V2).unwrap();

    // Every operation that consumes a numeric program address refuses
    // `stale-id` after the reload; name addressing stays usable because it
    // resolves against the new program.
    for request in [
        ToolRequest::References { symbol: 0.into() },
        ToolRequest::Usage { symbol: 0.into() },
        ToolRequest::SemanticRename {
            sources: Some(std::collections::BTreeMap::new()),
            target: wright_driver::edit::RenameTarget {
                symbol: Some(0.into()),
                source: None,
                line: None,
                col: None,
                to: "renamed".to_string(),
            },
        },
        ToolRequest::Cfg { rule: 0.into() },
    ] {
        assert_eq!(
            refusal_code(&mut service, &request),
            "stale-id",
            "{request:?}"
        );
    }
    let references = result_of(
        &mut service,
        &ToolRequest::References {
            symbol: "points".into(),
        },
    );
    assert!(references.as_array().unwrap().len() >= 2, "{references}");
    result_of(
        &mut service,
        &ToolRequest::Cfg {
            rule: "extra".into(),
        },
    );

    // `symbols` re-establishes only the symbol space; `rules` the rule space.
    result_of(&mut service, &ToolRequest::Symbols { kind: None });
    let usage = result_of(&mut service, &ToolRequest::Usage { symbol: 0.into() });
    assert_eq!(usage["id"], 0);
    assert_eq!(usage["symbol"], "points");
    assert_eq!(
        refusal_code(&mut service, &ToolRequest::Cfg { rule: 1.into() }),
        "stale-id"
    );
    result_of(&mut service, &ToolRequest::Rules);
    result_of(&mut service, &ToolRequest::Cfg { rule: 1.into() });
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ambiguous_refusals_re_establish_the_current_id_space() {
    let dir = freshness_dir("ambiguous");
    let input = dir.join("program.ws");
    std::fs::write(&input, FRESHNESS_V1).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    result_of(&mut service, &ToolRequest::Symbols { kind: None });

    std::fs::write(&input, FRESHNESS_AMBIGUOUS).unwrap();

    // An ambiguous refusal already names the current candidates, so the
    // space it resolved against is observed even though it is an error.
    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::References {
                symbol: "dup".into()
            }
        ),
        "ambiguous-symbol"
    );
    result_of(&mut service, &ToolRequest::Usage { symbol: 0.into() });
    assert_eq!(
        refusal_code(&mut service, &ToolRequest::Cfg { rule: 0.into() }),
        "stale-id",
        "the rule space is untouched by a symbol-space observation"
    );
    assert_eq!(
        refusal_code(&mut service, &ToolRequest::Cfg { rule: "dup".into() }),
        "ambiguous-rule"
    );
    result_of(&mut service, &ToolRequest::Cfg { rule: 0.into() });
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_operation_is_fresh_after_a_reload_or_refuses_stale_ids() {
    // #471 enumerates the whole request surface: operations consuming a
    // numeric address refuse `stale-id`; every other program-reading
    // operation reports the reloaded program, and metadata/provider
    // operations are unaffected either way.
    let dir = freshness_dir("enumerate");
    let input = dir.join("program.ws");
    std::fs::write(&input, FRESHNESS_V1).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    result_of(&mut service, &ToolRequest::Project);
    std::fs::write(&input, FRESHNESS_V2).unwrap();

    // Numeric addresses into the pre-reload spaces refuse `stale-id`.
    for request in [
        ToolRequest::References { symbol: 0.into() },
        ToolRequest::Usage { symbol: 0.into() },
        ToolRequest::Cfg { rule: 0.into() },
    ] {
        assert_eq!(
            refusal_code(&mut service, &request),
            "stale-id",
            "{request:?}"
        );
    }

    // Metadata and provider operations answer without consulting the loaded
    // program; every program-reading operation reports the reloaded program.
    result_of(&mut service, &ToolRequest::Capabilities);
    result_of(&mut service, &ToolRequest::TargetMetadata);
    for (request, command) in [
        (ToolRequest::Compile, "compile"),
        (ToolRequest::Check, "check"),
        (ToolRequest::Analyze, "analyze"),
        (ToolRequest::Inspect, "inspect"),
    ] {
        assert_eq!(result_of(&mut service, &request)["command"], command);
    }
    let project = result_of(&mut service, &ToolRequest::Project);
    assert_eq!(project["rules"], 2);
    assert_eq!(project["symbols"], 3);
    let symbols = result_of(&mut service, &ToolRequest::Symbols { kind: None });
    assert!(
        symbols
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["name"] == "points")
    );
    // Id-issuing responses carry the new program's numbering: `usage`
    // resolves the reloaded index, `references`/`findings` point at the new
    // rule indexes, and `inspect`/`analyze` expose the fresh facts.
    let usage = result_of(
        &mut service,
        &ToolRequest::Usage {
            symbol: "points".into(),
        },
    );
    assert_eq!(usage["id"], 0);
    assert_eq!(usage["symbol"], "points");
    assert_eq!(
        result_of(&mut service, &ToolRequest::Rules)
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let references = result_of(
        &mut service,
        &ToolRequest::References {
            symbol: "points".into(),
        },
    );
    // Rule-internal references carry the new program's rule indexes; the
    // declaration itself is rule-less (`rule: null`).
    assert!(
        references
            .as_array()
            .unwrap()
            .iter()
            .any(|reference| reference["rule"] == 1),
        "{references}"
    );
    let findings = result_of(
        &mut service,
        &ToolRequest::Findings(FindingSelection::default()),
    );
    assert!(
        findings
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["code"] == "min-wait-loop" && finding["rule"].is_number())
    );
    result_of(
        &mut service,
        &ToolRequest::Lint(FindingSelection::default()),
    );
    result_of(&mut service, &ToolRequest::LintRules);
    result_of(&mut service, &ToolRequest::PersistentObjects);
    result_of(&mut service, &ToolRequest::CallGraph);
    let cost = result_of(
        &mut service,
        &ToolRequest::CostEstimate(FindingSelection::default()),
    );
    assert_eq!(cost["exact"]["programRules"], 2);
    let inspect = result_of(&mut service, &ToolRequest::Inspect);
    assert_eq!(inspect["result"]["rules"].as_array().unwrap().len(), 2);
    let analyze = result_of(&mut service, &ToolRequest::Analyze);
    assert!(
        analyze["result"]["facts"]["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["name"] == "points" && symbol["id"].is_number())
    );

    // The edit surfaces serve the caller's sources against the reloaded
    // program; name addressing resolves "points" in the new program.
    let source_key = input.to_string_lossy().into_owned();
    let sources =
        std::collections::BTreeMap::from([(source_key.clone(), FRESHNESS_V2.to_string())]);
    let validate = result_of(
        &mut service,
        &ToolRequest::ValidateEdit {
            sources: Some(sources.clone()),
            transaction: wright_driver::edit::EditTransaction::new(vec![
                serde_json::from_value(serde_json::json!({
                    "kind": "edit",
                    "source": source_key,
                    "source_identity": wright_driver::input_identity(FRESHNESS_V2),
                    "range": {
                        "start_line": 6, "start_col": 8,
                        "end_line": 6, "end_col": 13,
                    },
                    "new_text": "setup2",
                }))
                .unwrap(),
            ])
            .unwrap(),
        },
    );
    assert_eq!(validate["ok"], true, "{validate}");
    let rename = result_of(
        &mut service,
        &ToolRequest::SemanticRename {
            sources: Some(sources),
            target: wright_driver::edit::RenameTarget {
                symbol: Some("points".into()),
                source: None,
                line: None,
                col: None,
                to: "score".to_string(),
            },
        },
    );
    assert_eq!(rename["ok"], true, "{rename}");

    // Provider operations carry their own documents; an unconfigured
    // language id is the usual structured provider refusal, not `stale-id`.
    for request in [
        ToolRequest::ProviderSemanticRename {
            language_id: "not-a-language".to_string(),
            documents: Default::default(),
            position_document_uri: String::new(),
            position: wright_lpp::Position {
                line: 0,
                character: 0,
            },
            new_name: String::new(),
            project_root: None,
            sources: Default::default(),
        },
        ToolRequest::ProviderValidateEdit {
            language_id: "not-a-language".to_string(),
            documents: Default::default(),
            transaction: wright_driver::edit::EditTransaction { edits: vec![] },
            sources: Default::default(),
            project_root: None,
        },
    ] {
        let result = result_of(&mut service, &request);
        assert_eq!(result["ok"], false, "{request:?}: {result}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ── Deferred project load and project-independent operations (#512) ────────

/// An unreadable configured input does not fail service construction:
/// project-independent operations answer without consulting the load, and
/// program-reading requests refuse with the loader's diagnostic until the
/// input heals — all within the same running service.
#[test]
fn an_unloadable_input_defers_the_load_to_program_reading_requests() {
    let dir = freshness_dir("deferred");
    let input = dir.join("missing.ws");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    assert!(
        service.loaded().is_none(),
        "no placeholder snapshot is adopted"
    );

    // Project-independent operations answer without triggering a load.
    result_of(&mut service, &ToolRequest::Capabilities);
    result_of(&mut service, &ToolRequest::TargetMetadata);

    // Every program-reading request refuses with the loader's diagnostic;
    // no snapshot — stale or empty — is ever served. A numeric id is the
    // same load refusal, not a `stale-id` against a nonexistent space.
    for request in [
        ToolRequest::Project,
        ToolRequest::Rules,
        ToolRequest::Symbols { kind: None },
        ToolRequest::Check,
        ToolRequest::References { symbol: 0.into() },
    ] {
        assert_eq!(
            refusal_code(&mut service, &request),
            "input-io",
            "{request:?}"
        );
    }

    // A provider mutation carries its own documents: an unconfigured
    // language is the provider refusal result, not the load failure.
    let mutation = result_of(
        &mut service,
        &ToolRequest::ProviderValidateEdit {
            language_id: "not-a-language".to_string(),
            documents: Default::default(),
            transaction: wright_driver::edit::EditTransaction { edits: vec![] },
            sources: Default::default(),
            project_root: None,
        },
    );
    assert_eq!(mutation["ok"], false, "{mutation}");
    // None of the above triggered the project load.
    assert!(service.loaded().is_none());

    // The embedding `check` workflow surfaces the same loader diagnostic.
    let check = service.check();
    assert!(!check.ok);
    assert!(
        check.diagnostics.iter().any(|d| d.code == "input-io"),
        "{:?}",
        check.diagnostics
    );

    // Repairing the input lets the same service answer program-reading
    // requests — no restart, no explicit reload.
    std::fs::write(&input, FRESHNESS_V1).unwrap();
    let symbols = result_of(&mut service, &ToolRequest::Symbols { kind: None });
    assert!(
        symbols
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["name"] == "score"),
        "{symbols}"
    );
    assert!(service.loaded().is_some());
    // The deferred initial load is not a disk-change reload.
    assert_eq!(service.reload_count(), 0);

    // Breaking it again refuses program reads until it heals once more.
    std::fs::remove_file(&input).unwrap();
    for request in [ToolRequest::Project, ToolRequest::Symbols { kind: None }] {
        assert_eq!(
            refusal_code(&mut service, &request),
            "input-io",
            "{request:?}"
        );
    }
    result_of(&mut service, &ToolRequest::Capabilities);
    std::fs::write(&input, FRESHNESS_V1).unwrap();
    result_of(&mut service, &ToolRequest::Project);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A malformed configured input behaves the same way: construction
/// succeeds, program reads refuse with the parse diagnostic, and repair in
/// place resumes normal service.
#[test]
fn a_malformed_input_defers_the_load_and_recovers_in_place() {
    let dir = freshness_dir("malformed");
    let input = dir.join("program.ws");
    std::fs::write(
        &input,
        "variables {\n    global:\n        0: score\n}\nrule (\"setup\") {\n",
    )
    .unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    for request in [
        ToolRequest::Project,
        ToolRequest::Compile,
        ToolRequest::Symbols { kind: None },
    ] {
        assert_eq!(
            refusal_code(&mut service, &request),
            "parse-error",
            "{request:?}"
        );
    }

    std::fs::write(&input, FRESHNESS_V1).unwrap();
    let project = result_of(&mut service, &ToolRequest::Project);
    assert_eq!(project["rules"], 1, "{project}");
    assert!(service.check().ok, "check heals on the same session");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Ids issued before a failed refresh must not silently validate against
/// the repaired program (#512 + #471): a failed load invalidates the id
/// space, so numeric addresses issued by the previous snapshot refuse
/// `stale-id` until the new space is observed.
#[test]
fn ids_issued_before_a_failed_load_stay_stale_after_recovery() {
    let dir = freshness_dir("stale-recovery");
    let input = dir.join("program.ws");
    std::fs::write(&input, FRESHNESS_V1).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    result_of(&mut service, &ToolRequest::Symbols { kind: None });
    result_of(&mut service, &ToolRequest::Rules);
    result_of(&mut service, &ToolRequest::References { symbol: 0.into() });

    std::fs::write(
        &input,
        "variables {\n    global:\n        0: score\n}\nrule (\"setup\") {\n",
    )
    .unwrap();
    assert_eq!(
        refusal_code(&mut service, &ToolRequest::Project),
        "parse-error"
    );

    // The repaired program is adopted, but the previous snapshot's numeric
    // ids refuse `stale-id` until the client re-observes each space.
    std::fs::write(&input, FRESHNESS_V2).unwrap();
    assert_eq!(
        refusal_code(&mut service, &ToolRequest::References { symbol: 0.into() }),
        "stale-id"
    );
    assert_eq!(
        refusal_code(&mut service, &ToolRequest::Cfg { rule: 1.into() }),
        "stale-id"
    );
    result_of(&mut service, &ToolRequest::Symbols { kind: None });
    let usage = result_of(&mut service, &ToolRequest::Usage { symbol: 0.into() });
    assert_eq!(usage["symbol"], "points");
    result_of(&mut service, &ToolRequest::Rules);
    result_of(&mut service, &ToolRequest::Cfg { rule: 1.into() });
    let _ = std::fs::remove_dir_all(&dir);
}

/// A failed refresh invalidates the served snapshot even when the input is
/// restored to byte-identical content (#512): recovery reloads a fresh
/// program instead of fingerprint-matching the pre-failure snapshot back to
/// life, and ids it issued refuse `stale-id` until the new space is
/// observed.
#[test]
fn a_failed_refresh_invalidates_the_snapshot_even_when_bytes_return() {
    let dir = freshness_dir("invalidate");
    let input = dir.join("program.ws");
    std::fs::write(&input, FRESHNESS_V1).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    result_of(&mut service, &ToolRequest::Symbols { kind: None });
    result_of(&mut service, &ToolRequest::References { symbol: 0.into() });
    let builds = service.semantic_build_count();

    // Break the input, observe the refusal, then restore the exact previous
    // bytes.
    std::fs::write(
        &input,
        "variables {\n    global:\n        0: score\n}\nrule (\"setup\") {\n",
    )
    .unwrap();
    assert_eq!(
        refusal_code(&mut service, &ToolRequest::Project),
        "parse-error"
    );
    assert!(
        service.loaded().is_none(),
        "the failed refresh drops the served snapshot"
    );
    std::fs::write(&input, FRESHNESS_V1).unwrap();

    // The next program read reloads: the fingerprint-identical restored
    // bytes still produce a fresh snapshot, not the pre-failure one.
    result_of(&mut service, &ToolRequest::Project);
    assert_eq!(service.semantic_build_count(), builds + 1);

    // The pre-failure numeric ids are stale until the space is re-observed.
    assert_eq!(
        refusal_code(&mut service, &ToolRequest::References { symbol: 0.into() }),
        "stale-id"
    );
    result_of(&mut service, &ToolRequest::Symbols { kind: None });
    result_of(&mut service, &ToolRequest::References { symbol: 0.into() });
    let _ = std::fs::remove_dir_all(&dir);
}

// ── #472: `sources` is optional on the raw Workshop edit operations ──────

const RENAMEABLE: &str = "variables {\n    global:\n        0: score\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(score, Add(Global.score, 1));\n    }\n}\n";

fn rename_target() -> RenameTarget {
    RenameTarget {
        symbol: Some(Address::Name("score".to_string())),
        source: None,
        line: None,
        col: None,
        to: "total".to_string(),
    }
}

fn rename_edit_transaction(key: &str, source_identity: String) -> EditTransaction {
    EditTransaction::new(vec![SourceEdit {
        edit_kind: "edit".to_string(),
        source: key.to_string(),
        source_identity,
        range: EditRange {
            start_line: 3,
            start_col: 12,
            end_line: 3,
            end_col: 17,
        },
        new_text: "total".to_string(),
    }])
    .expect("the transaction is structurally valid")
}

#[test]
fn edit_operations_default_sources_to_the_on_disk_text() {
    let path = temp_workshop("score.ws", RENAMEABLE);
    let key = path.to_string_lossy().into_owned();
    let on_disk = std::fs::read_to_string(&path).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    // `semanticRename` by symbol name: omitting `sources` returns the same
    // validated transaction as supplying the on-disk text.
    let rename = |sources| ToolRequest::SemanticRename {
        sources,
        target: rename_target(),
    };
    let defaulted = result_of(&mut service, &rename(None));
    let explicit = result_of(
        &mut service,
        &rename(Some(BTreeMap::from([(key.clone(), on_disk.clone())]))),
    );
    assert_eq!(defaulted["ok"], true, "{defaulted:?}");
    assert_eq!(defaulted, explicit);
    assert!(
        defaulted["preview"][0]["new_text"]
            .as_str()
            .unwrap()
            .contains("0: total"),
        "{defaulted:?}"
    );

    // `validateEditTransaction`: omitting `sources` validates against the
    // on-disk text and returns the same result as explicit `sources`.
    let validate = |sources| ToolRequest::ValidateEdit {
        sources,
        transaction: rename_edit_transaction(&key, wright_driver::input_identity(&on_disk)),
    };
    let defaulted = result_of(&mut service, &validate(None));
    let explicit = result_of(
        &mut service,
        &validate(Some(BTreeMap::from([(key.clone(), on_disk.clone())]))),
    );
    assert_eq!(defaulted["ok"], true, "{defaulted:?}");
    assert_eq!(defaulted, explicit);
    assert!(
        defaulted["preview"][0]["new_text"]
            .as_str()
            .unwrap()
            .contains("0: total"),
        "{defaulted:?}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn edit_operations_keep_supplied_source_and_project_boundaries() {
    let path = temp_workshop("score.ws", RENAMEABLE);
    let key = path.to_string_lossy().into_owned();
    let on_disk = std::fs::read_to_string(&path).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    // A supplied `sources` text that differs from the loaded program still
    // refuses with `edit-stale-source`.
    for (op, result) in [
        (
            "semanticRename",
            result_of(
                &mut service,
                &ToolRequest::SemanticRename {
                    sources: Some(BTreeMap::from([(
                        key.clone(),
                        on_disk.replacen("score", "other", 1),
                    )])),
                    target: rename_target(),
                },
            ),
        ),
        (
            "validateEditTransaction",
            result_of(
                &mut service,
                &ToolRequest::ValidateEdit {
                    sources: Some(BTreeMap::from([(key.clone(), "changed".to_string())])),
                    transaction: rename_edit_transaction(
                        &key,
                        wright_driver::input_identity(&on_disk),
                    ),
                },
            ),
        ),
    ] {
        assert_eq!(result["ok"], false, "{op}: {result:?}");
        assert_eq!(
            result["diagnostics"][0]["code"], "edit-stale-source",
            "{op}: {result:?}"
        );
        assert!(result["preview"].is_null(), "{op}: no partial preview");
    }

    // A transaction naming a file that is not part of the loaded project
    // refuses with a structured error — with or without `sources`.
    let foreign = |sources| ToolRequest::ValidateEdit {
        sources,
        transaction: rename_edit_transaction("other.ws", wright_driver::input_identity(&on_disk)),
    };
    for (op, result) in [
        ("absent sources", result_of(&mut service, &foreign(None))),
        (
            "supplied sources",
            result_of(
                &mut service,
                &foreign(Some(BTreeMap::from([(
                    "other.ws".to_string(),
                    on_disk.clone(),
                )]))),
            ),
        ),
    ] {
        assert_eq!(result["ok"], false, "{op}: {result:?}");
        assert_eq!(
            result["diagnostics"][0]["code"], "edit-unknown-source",
            "{op}: {result:?}"
        );
    }

    // The default reads disk, not a stale snapshot: rewriting the input is
    // observed by the service's freshness check (#471), so the unsourced
    // rename resolves against the reloaded program — the new `other`
    // symbol renames cleanly while the old `score` name is gone.
    std::fs::write(&path, on_disk.replace("score", "other")).unwrap();
    let renamed = result_of(
        &mut service,
        &ToolRequest::SemanticRename {
            sources: None,
            target: RenameTarget {
                symbol: Some(Address::Name("other".to_string())),
                ..rename_target()
            },
        },
    );
    assert_eq!(renamed["ok"], true, "{renamed:?}");
    let stale = result_of(
        &mut service,
        &ToolRequest::SemanticRename {
            sources: None,
            target: rename_target(),
        },
    );
    assert_eq!(stale["ok"], false, "{stale:?}");
    assert_eq!(stale["diagnostics"][0]["code"], "unknown-symbol");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn unsourced_transaction_unifies_spelling_variants_of_the_input() {
    let path = temp_workshop("score.ws", RENAMEABLE);
    let key = path.to_string_lossy().into_owned();
    let on_disk = std::fs::read_to_string(&path).unwrap();
    let identity = wright_driver::input_identity(&on_disk);
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    let edit = |source: &str, range: EditRange, new_text: &str| SourceEdit {
        edit_kind: "edit".to_string(),
        source: source.to_string(),
        source_identity: identity.clone(),
        range,
        new_text: new_text.to_string(),
    };
    let declaration = EditRange {
        start_line: 3,
        start_col: 12,
        end_line: 3,
        end_col: 17,
    };
    let rule_name = EditRange {
        start_line: 6,
        start_col: 8,
        end_line: 6,
        end_col: 9,
    };

    // Two spellings of the same loaded input — a `file://` URI and the
    // plain path — are one file, not two sources: without `sources` the
    // transaction still produces a single atomic preview carrying both
    // edits, identical to the canonical explicit request.
    let uri = format!("file://{key}");
    let transaction = |first: &str, second: &str| {
        EditTransaction::new(vec![
            edit(first, declaration.clone(), "total"),
            edit(second, rule_name.clone(), "x"),
        ])
        .unwrap()
    };
    let defaulted = result_of(
        &mut service,
        &ToolRequest::ValidateEdit {
            sources: None,
            transaction: transaction(&uri, &key),
        },
    );
    let explicit = result_of(
        &mut service,
        &ToolRequest::ValidateEdit {
            sources: Some(BTreeMap::from([(key.clone(), on_disk.clone())])),
            transaction: transaction(&key, &key),
        },
    );
    assert_eq!(defaulted["ok"], true, "{defaulted:?}");
    assert_eq!(defaulted, explicit);
    assert_eq!(defaulted["preview"].as_array().unwrap().len(), 1);
    let new_text = defaulted["preview"][0]["new_text"].as_str().unwrap();
    assert!(
        new_text.contains("0: total") && new_text.contains("rule (\"x\")"),
        "{new_text}"
    );

    // Cross-spelling overlaps refuse the same way same-spelling ones do —
    // they must not slip through as two divergent previews of one file.
    let refused = result_of(
        &mut service,
        &ToolRequest::ValidateEdit {
            sources: None,
            transaction: EditTransaction::new(vec![
                edit(&uri, declaration.clone(), "total"),
                edit(&key, declaration.clone(), "other"),
            ])
            .unwrap(),
        },
    );
    assert_eq!(refused["ok"], false, "{refused:?}");
    assert_eq!(refused["diagnostics"][0]["code"], "edit-overlap");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

// ── Shared semantic state over one loaded snapshot (#513) ──────────────────

/// `analyze` renders through the service the loaded snapshot already
/// holds: repeated transport requests never rebuild the index, and the
/// embedding `analyze` shares the same build.
#[test]
fn repeated_analyze_requests_do_not_rebuild_semantic_state() {
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(declarations_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    assert_eq!(service.semantic_build_count(), 1);

    for _ in 0..3 {
        let analyze = result_of(&mut service, &ToolRequest::Analyze);
        assert_eq!(analyze["command"], "analyze");
        assert!(analyze["ok"].as_bool().unwrap_or(false), "{analyze}");
    }
    assert_eq!(service.semantic_build_count(), 1);

    // The embedding method renders through the same held service.
    assert!(service.analyze().ok);
    assert_eq!(service.semantic_build_count(), 1);
}

/// The session's own embedding workflows share the loaded snapshot's
/// semantic service: one construction serves analyze/inspect/lint and
/// every service_query command.
#[test]
fn repeated_embedding_workflows_share_one_semantic_build() {
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(declarations_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();

    assert!(session.analyze().ok);
    assert!(session.inspect().ok);
    assert!(session.lint().ok);
    assert!(session.symbols(None).ok);
    assert!(session.refs("score").ok);
    assert!(session.cfg("player starts").ok);
    assert!(session.callgraph().ok);
    assert!(session.cost().ok);
    assert_eq!(session.semantic_build_count(), 1);

    // Repetition on the unchanged snapshot is still the same build.
    assert!(session.analyze().ok);
    assert!(session.inspect().ok);
    assert!(session.lint().ok);
    assert!(session.symbols(None).ok);
    assert_eq!(session.semantic_build_count(), 1);
}

/// An effective lint configuration still configures the lint run over the
/// shared index: the custom severity reaches `lint`, and repeats keep
/// sharing the one build.
#[test]
fn session_lint_keeps_the_effective_configuration_over_the_shared_index() {
    let mut lint = wright_driver::config::LintConfig::default();
    assert!(lint.set_severity_by_name("min-wait-loop", "error"));
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        lint,
        ..SessionConfig::default()
    })
    .unwrap();

    let lint = session.lint();
    let finding = lint
        .result
        .findings
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["code"] == "min-wait-loop")
        .expect("the fixture triggers min-wait-loop");
    assert_eq!(finding["severity"], "error");

    assert!(session.lint().ok);
    assert!(session.analyze().ok);
    assert_eq!(session.semantic_build_count(), 1);
}

/// The tool-service `analyze` result equals the session workflow's — one
/// report rendered through the shared service.
#[test]
fn tool_service_analyze_matches_session_analyze() {
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let expected = session.analyze();
    let mut service = ToolService::new(&mut session).unwrap();
    let actual = service.analyze();

    assert_eq!(actual.ok, expected.ok);
    assert_eq!(
        serde_json::to_value(actual.result).unwrap(),
        serde_json::to_value(expected.result).unwrap()
    );
    assert_eq!(service.semantic_build_count(), 1);
}

/// A reload produces a fresh program and therefore a fresh service: the
/// count grows by exactly one and the new snapshot's facts are served.
#[test]
fn a_reload_rebuilds_the_semantic_service_once() {
    let dir = freshness_dir("semantic");
    let input = dir.join("program.ws");
    std::fs::write(&input, FRESHNESS_V1).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    result_of(&mut service, &ToolRequest::Analyze);
    assert_eq!(service.semantic_build_count(), 1);

    std::fs::write(&input, FRESHNESS_V2).unwrap();
    let analyze = result_of(&mut service, &ToolRequest::Analyze);
    assert!(
        analyze["result"]["facts"]["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["name"] == "points"),
        "the reloaded program's facts are served: {analyze}"
    );
    assert_eq!(service.semantic_build_count(), 2);

    // The new snapshot is stable again: further semantic requests share it.
    result_of(&mut service, &ToolRequest::Analyze);
    result_of(&mut service, &ToolRequest::Inspect);
    assert_eq!(service.semantic_build_count(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}
