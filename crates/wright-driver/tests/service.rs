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
        ToolRequest::Rules {
            name: None,
            file: None,
            max: None,
            rule: None,
            content: false,
            all: true,
        },
        ToolRequest::Findings {
            selection: FindingSelection::default(),
            all: true,
        },
        ToolRequest::CostEstimate {
            selection: FindingSelection::default(),
            rule: None,
            all: true,
        },
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

    let lint = match service.handle(&ToolRequest::Lint {
        selection: FindingSelection::default(),
        all: true,
    }) {
        ToolResponse::Ok { result } => result,
        ToolResponse::Error { error } => panic!("lint failed: {error:?}"),
    };
    assert!(lint.get("config").is_none());
    assert!(lint.get("rules").is_none());
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

    let default_findings = match service.handle(&ToolRequest::Findings {
        selection: FindingSelection::default(),
        all: true,
    }) {
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

/// A compile request whose program lands in the cargo-owned test directory,
/// so no test writes into the crate or the working directory.
fn compile_request(name: &str) -> ToolRequest {
    ToolRequest::Compile {
        output: Some(std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)),
        force: true,
        to: None,
    }
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
        (compile_request("routes.ow"), "compile"),
        (ToolRequest::Check { all: true }, "check"),
        (
            ToolRequest::Analyze {
                selection: Default::default(),
                all: true,
            },
            "analyze",
        ),
        (ToolRequest::Inspect { all: true }, "inspect"),
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

    let plain = result_of(ToolRequest::Findings {
        selection: FindingSelection::default(),
        all: true,
    });
    assert!(plain.is_array(), "the default result stays a bare array");
    assert_eq!(plain.as_array().unwrap().len(), 10);

    let errors = result_of(ToolRequest::Findings {
        selection: FindingSelection {
            severity: Some(Severity::Error),
            ..FindingSelection::default()
        },
        all: true,
    });
    assert!(errors["findings"].as_array().unwrap().is_empty());
    assert_eq!(
        errors["selection"],
        serde_json::json!({"total": 10, "withheld": 0})
    );

    let lint = result_of(ToolRequest::Lint {
        selection: FindingSelection {
            rule: Some("repeated-value".to_string()),
            max: Some(2),
            ..FindingSelection::default()
        },
        all: true,
    });
    assert_eq!(lint["findings"].as_array().unwrap().len(), 2);
    assert_eq!(
        lint["selection"],
        serde_json::json!({"total": 10, "withheld": 7})
    );

    let cost = result_of(ToolRequest::CostEstimate {
        selection: FindingSelection {
            max: Some(1),
            ..FindingSelection::default()
        },
        rule: None,
        all: true,
    });
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
        ToolRequest::Findings {
            selection: FindingSelection {
                rule: Some("not-a-rule".to_string()),
                ..FindingSelection::default()
            },
            all: true,
        },
        ToolRequest::Lint {
            selection: FindingSelection {
                rule: Some("not-a-rule".to_string()),
                ..FindingSelection::default()
            },
            all: true,
        },
        ToolRequest::CostEstimate {
            selection: FindingSelection {
                rule: Some("not-a-rule".to_string()),
                ..FindingSelection::default()
            },
            rule: None,
            all: true,
        },
        ToolRequest::Analyze {
            selection: FindingSelection {
                rule: Some("not-a-rule".to_string()),
                ..FindingSelection::default()
            },
            all: true,
        },
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
fn an_unknown_rule_id_in_the_lint_policy_is_a_usage_error() {
    // `--disable-rule`, `--rule-severity`, and lint-config `rules:` keys all
    // land in `config.lint.rules`; session construction validates every key
    // against the registry so a typo never silently leaves a rule enabled
    // (#640).
    for mut lint in [
        {
            let mut lint = wright_driver::config::LintConfig::default();
            lint.disable("not-a-rule");
            lint
        },
        {
            let mut lint = wright_driver::config::LintConfig::default();
            assert!(lint.set_severity_by_name("not-a-rule", "error"));
            lint
        },
        wright_driver::config::LintConfig::from_yaml_str(
            "rules:\n  not-a-rule:\n    enabled: false\n",
        )
        .unwrap(),
    ] {
        lint.disable("min-wait-loop");
        let error = CompilerSession::new(SessionConfig {
            input: InputSpec::Path(workshop_path()),
            kind: SourceKind::Workshop,
            lint,
            ..SessionConfig::default()
        })
        .err()
        .expect("an unknown policy id must not construct a session");
        assert_eq!(error.code, "invalid-selection");
        assert!(
            error.message.contains("not-a-rule"),
            "the message names the unknown id: {}",
            error.message
        );
    }

    // Known ids — including a declarative rule loaded through `--rule` —
    // behave as before.
    let dir = std::env::temp_dir().join(format!("wright-policy-640-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let rule_file = dir.join("smoke.yaml");
    std::fs::write(
        &rule_file,
        "id: community/policy-smoke\nmetadata:\n  summary: summary\n  rationale: rationale\n  documentation: documentation\n  known-limits: limits\n  tags: []\nmatcher: {}\n",
    )
    .unwrap();
    let mut lint = wright_driver::config::LintConfig::default();
    lint.disable("min-wait-loop");
    lint.disable("community/policy-smoke");
    CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        lint_rule_paths: vec![rule_file.clone()],
        lint,
        ..SessionConfig::default()
    })
    .expect("ids the registry knows construct a session");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn analyze_selection_narrows_risks_and_reports_the_outcome() {
    // #635: `severity`/`rule`/`file`/`max` narrow `facts.risks` like lint
    // findings, and `result.selection` reports the true total — never a
    // 0/0 beside non-empty risks. `real-world/overpy-cake.ws` produces 10
    // risk findings: 9 `repeated-value` warnings and 1 `min-wait-loop`
    // warning.
    let input = workspace_root().join("tests/fixtures/workshop/real-world/overpy-cake.ws");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    let mut analyze = |selection| {
        result_of(
            &mut service,
            &ToolRequest::Analyze {
                selection,
                all: true,
            },
        )
    };

    let plain = analyze(FindingSelection::default());
    assert_eq!(plain["result"]["risks"].as_array().unwrap().len(), 10);
    assert!(plain["result"].get("selection").is_none());

    let errors = analyze(FindingSelection {
        severity: Some(Severity::Error),
        ..FindingSelection::default()
    });
    assert!(errors["result"]["risks"].as_array().unwrap().is_empty());
    assert_eq!(
        errors["result"]["selection"],
        serde_json::json!({"total": 10, "withheld": 0})
    );

    let bounded = analyze(FindingSelection {
        rule: Some("repeated-value".to_string()),
        max: Some(2),
        ..FindingSelection::default()
    });
    let risks = bounded["result"]["risks"].as_array().unwrap();
    assert_eq!(risks.len(), 2);
    assert!(risks.iter().all(|risk| risk["code"] == "repeated-value"));
    assert_eq!(
        bounded["result"]["selection"],
        serde_json::json!({"total": 10, "withheld": 7})
    );

    // `file` selects by resolved `span.path` identity: the input path as
    // passed resolves to the reported root-relative spelling.
    let here = analyze(FindingSelection {
        file: Some(input.to_str().unwrap().to_string()),
        ..FindingSelection::default()
    });
    assert_eq!(here["result"]["risks"].as_array().unwrap().len(), 10);
    let elsewhere = analyze(FindingSelection {
        file: Some("elsewhere.ws".to_string()),
        ..FindingSelection::default()
    });
    assert!(elsewhere["result"]["risks"].as_array().unwrap().is_empty());
    assert_eq!(
        elsewhere["result"]["selection"],
        serde_json::json!({"total": 10, "withheld": 0})
    );

    let essential = result_of(
        &mut service,
        &ToolRequest::Analyze {
            selection: FindingSelection {
                rule: Some("repeated-value".to_string()),
                ..FindingSelection::default()
            },
            all: false,
        },
    );
    assert_eq!(essential["result"]["counts"]["risks"]["total"], 9);
    assert_eq!(
        essential["result"]["selection"],
        serde_json::json!({"total": 10, "withheld": 0})
    );

    let mut selected_session = CompilerSession::new(SessionConfig {
        selection: FindingSelection {
            severity: Some(Severity::Error),
            ..FindingSelection::default()
        },
        input: InputSpec::Path(input),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let envelope = selected_session.analyze();
    assert!(
        envelope.result.facts["risks"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let outcome = envelope.result.selection.expect("selection is recorded");
    assert_eq!(outcome.total, 10);
    assert_eq!(outcome.max_severity, Some(Severity::Warning));
}

#[test]
fn inspect_results_resolve_span_paths() {
    // #635: `inspect` spans resolve `path` like `analyze`/`lint` findings —
    // a bare `{file: <index>}` is unusable outside the session.
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(declarations_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let envelope = session.inspect();
    let result = serde_json::to_value(&envelope.result).unwrap();
    let mut spans = Vec::new();
    fn collect(value: &serde_json::Value, spans: &mut Vec<serde_json::Value>) {
        match value {
            serde_json::Value::Object(object) => {
                if let Some(span) = object.get("span") {
                    if span.is_object() {
                        spans.push(span.clone());
                    }
                }
                object.values().for_each(|value| collect(value, spans));
            }
            serde_json::Value::Array(items) => {
                items.iter().for_each(|value| collect(value, spans));
            }
            _ => {}
        }
    }
    collect(&result, &mut spans);
    assert!(!spans.is_empty(), "the fixture carries spans");
    assert!(
        spans
            .iter()
            .all(|span| span["path"].as_str().is_some_and(|path| !path.is_empty())),
        "every span resolves a path: {spans:?}"
    );
}

#[test]
fn cost_estimate_file_selection_narrows_findings_like_lint() {
    // #722: cost findings now carry resolved span paths, so `file`
    // applies exactly like `lint` — it narrows the reported set and the
    // selection outcome records the withheld count. A matching file
    // keeps its finding; a non-matching one reports an empty set.
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    let cost = result_of(
        &mut service,
        &ToolRequest::CostEstimate {
            selection: FindingSelection {
                file: Some("control-flow.ws".to_string()),
                ..FindingSelection::default()
            },
            rule: None,
            all: true,
        },
    );
    assert_eq!(cost["findings"].as_array().unwrap().len(), 1);
    assert_eq!(cost["selection"]["total"].as_u64().unwrap(), 1);
    assert_eq!(cost["selection"]["withheld"].as_u64().unwrap(), 0);

    let cost = result_of(
        &mut service,
        &ToolRequest::CostEstimate {
            selection: FindingSelection {
                file: Some("other-file.ws".to_string()),
                ..FindingSelection::default()
            },
            rule: None,
            all: true,
        },
    );
    assert!(cost["findings"].as_array().unwrap().is_empty());
    // `total` still counts the full finding set; the non-matching file
    // drops its findings without a `max`, so nothing is `withheld`.
    assert_eq!(cost["selection"]["total"].as_u64().unwrap(), 1);
    assert_eq!(cost["selection"]["withheld"].as_u64().unwrap(), 0);
    let cost = result_of(
        &mut service,
        &ToolRequest::CostEstimate {
            selection: FindingSelection {
                severity: Some(Severity::Error),
                ..FindingSelection::default()
            },
            rule: None,
            all: true,
        },
    );
    assert!(
        cost["findings"].as_array().unwrap().is_empty(),
        "the error threshold selects none of the warnings: {cost}"
    );
    assert!(cost["selection"]["total"].as_u64().unwrap() > 0);
}

/// #691: `costEstimate` reports the canonical element count — the
/// `workshop-rs`-owned computation — against the client element budget,
/// with each rule's share ranked by count, largest first. The expected
/// numbers come from reparsing the fixture through `workshop-rs` itself,
/// not from the service under test.
#[test]
fn cost_estimate_reports_the_canonical_element_count() {
    let source = std::fs::read_to_string(workshop_path()).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    let cost = result_of(
        &mut service,
        &ToolRequest::CostEstimate {
            selection: FindingSelection::default(),
            rule: None,
            all: true,
        },
    );

    let catalog = workshop_rs::catalog::Catalog::builtin_ref().expect("catalog");
    let locale = workshop_rs::catalog::Locale::new("en-US");
    let program = workshop_rs::parser::parse_with_context(&source, catalog, &locale, catalog)
        .expect("fixture parses");
    let report = program.element_count(catalog).expect("fixture counts");

    let elements = &cost["elementCount"];
    assert_eq!(
        elements["total"].as_u64().map(|total| total as usize),
        Some(report.total)
    );
    assert_eq!(elements["budget"].as_u64(), Some(32768));
    let rules = elements["rules"].as_array().unwrap();
    assert_eq!(rules.len(), program.rules.len());
    assert_eq!(
        rules
            .iter()
            .map(|rule| rule["count"].as_u64().unwrap() as usize)
            .sum::<usize>(),
        report.total,
        "rule shares sum to the total: {rules:?}"
    );
    let mut expected: Vec<usize> = (0..report.rules.len()).collect();
    expected.sort_by_key(|index| (std::cmp::Reverse(report.rules[*index].count), *index));
    for (entry, index) in rules.iter().zip(expected) {
        assert_eq!(entry["index"], index);
        assert_eq!(entry["name"], program.rules[index].name);
        assert_eq!(
            entry["count"].as_u64().map(|count| count as usize),
            Some(report.rules[index].count)
        );
    }
    assert!(
        cost["kind"]["elementCount"]
            .as_str()
            .is_some_and(|kind| kind.contains("not a statement of client importability")),
        "the kind map scopes the count: {cost}"
    );
}

#[test]
fn session_cost_keeps_the_selection_outcome_for_the_verdict() {
    // #635: the in-process `CostResult` keeps the full set's outcome so the
    // verdict reflects the unselected maximum severity.
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        selection: FindingSelection {
            severity: Some(Severity::Error),
            ..FindingSelection::default()
        },
        ..SessionConfig::default()
    })
    .unwrap();
    let envelope = session.cost(None);
    assert!(
        envelope.result.result["findings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let outcome = envelope
        .result
        .selection
        .expect("the outcome survives in-process");
    assert_eq!(outcome.max_severity, Some(Severity::Warning));
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
fn essential_forms_report_actions_without_fact_dumps() {
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    let lint = result_of(
        &mut service,
        &ToolRequest::Lint {
            selection: FindingSelection::default(),
            all: false,
        },
    );
    assert_eq!(lint["counts"]["findings"]["total"], 1);
    assert!(lint["findings"].as_array().unwrap().len() <= 5);
    assert!(lint["expand"].as_str().unwrap().contains("all"));
    assert!(
        lint.get("config").is_none(),
        "configuration is served by lintRules"
    );

    let analyze = result_of(
        &mut service,
        &ToolRequest::Analyze {
            selection: Default::default(),
            all: false,
        },
    );
    let analyze = &analyze["result"];
    assert_eq!(analyze["program"]["rules"], 2);
    assert_eq!(analyze["counts"]["risks"]["total"], 1);
    assert!(analyze["hotspots"].as_array().unwrap().len() <= 5);
    assert!(analyze["expand"].is_string());
    assert!(
        analyze["facts"].is_null(),
        "full payload withheld: {analyze}"
    );

    let inspect = result_of(&mut service, &ToolRequest::Inspect { all: false });
    let inspect = &inspect["result"];
    assert_eq!(inspect["counts"]["rules"], 2);
    assert_eq!(inspect["counts"]["references"], 8);
    assert!(inspect["expand"].is_string());
    assert!(
        inspect["symbols"].is_null(),
        "full payload withheld: {inspect}"
    );
}

#[test]
fn essential_lint_applies_selection_before_summarizing() {
    // The selected set and selection summary survive presentation.
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    let lint = result_of(
        &mut service,
        &ToolRequest::Lint {
            selection: FindingSelection {
                severity: Some(Severity::Warning),
                max: Some(2),
                ..FindingSelection::default()
            },
            all: false,
        },
    );
    assert_eq!(
        lint["selection"],
        serde_json::json!({"total": 1, "withheld": 0})
    );
    assert_eq!(lint["counts"]["findings"]["total"], 1);
    assert_eq!(lint["findings"].as_array().unwrap().len(), 1);
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

    let symbols = result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );
    let symbol_id = |name: &str| {
        symbols
            .as_array()
            .unwrap()
            .iter()
            .find(|symbol| symbol["name"] == name)
            .and_then(|symbol| symbol["id"].as_u64())
            .unwrap_or_else(|| panic!("no symbol named {name}")) as u32
    };
    let rules = result_of(
        &mut service,
        &ToolRequest::Rules {
            name: None,
            file: None,
            max: None,
            rule: None,
            content: false,
            all: true,
        },
    );
    let rule_index = |name: &str| {
        rules
            .as_array()
            .unwrap()
            .iter()
            .find(|rule| rule["name"] == name)
            .and_then(|rule| rule["id"].as_u64())
            .unwrap_or_else(|| panic!("no rule named {name}")) as u32
    };

    for (by_name, by_id) in [
        (
            ToolRequest::References {
                symbol: "score".into(),
                kind: None,
                rule: None,
                file: None,
                max: None,
                all: true,
            },
            ToolRequest::References {
                symbol: symbol_id("score").into(),
                kind: None,
                rule: None,
                file: None,
                max: None,
                all: true,
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
                kind: None,
                max: None,
                all: true,
            },
            ToolRequest::Cfg {
                rule: rule_index("player starts").into(),
                kind: None,
                max: None,
                all: true,
            },
        ),
    ] {
        assert_eq!(
            result_of(&mut service, &by_name),
            result_of(&mut service, &by_id)
        );
    }

    let usage = result_of(
        &mut service,
        &ToolRequest::Usage {
            symbol: "score".into(),
        },
    );
    assert_eq!(usage["id"], symbol_id("score"));
    assert_eq!(usage["kind"], "globalVariable");
    assert_eq!(usage["symbol"], "score");

    assert_eq!(symbol_id("player starts"), 4);
    assert_eq!(rule_index("player starts"), 1);
    match service.handle(&ToolRequest::Cfg {
        rule: 4.into(),
        kind: None,
        max: None,

        all: true,
    }) {
        ToolResponse::Error { error } => assert_eq!(error.code, "invalid-id"),
        ToolResponse::Ok { result } => panic!("rule index 4 must not exist: {result}"),
    }

    let references = result_of(
        &mut service,
        &ToolRequest::References {
            symbol: "score".into(),
            kind: None,
            rule: None,
            file: None,
            max: None,
            all: true,
        },
    );
    for reference in references["references"].as_array().unwrap() {
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
                kind: None,
                rule: None,
                file: None,
                max: None,
                all: true,
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
                kind: None,
                max: None,
                all: true,
            },
            "unknown-rule",
        ),
        (
            ToolRequest::References {
                symbol: "dup".into(),
                kind: None,
                rule: None,
                file: None,
                max: None,
                all: true,
            },
            "ambiguous-symbol",
        ),
        (
            ToolRequest::Cfg {
                rule: "dup".into(),
                kind: None,
                max: None,
                all: true,
            },
            "ambiguous-rule",
        ),
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

    // A near miss names the declared spelling once; a far miss names the
    // listing operation instead (#567).
    for (request, needle) in [
        (
            ToolRequest::Usage {
                symbol: "dupp".into(),
            },
            "unknown symbol 'dupp'; nearest declared: 'dup'",
        ),
        (
            ToolRequest::Cfg {
                rule: "Dup".into(),
                kind: None,
                max: None,
                all: true,
            },
            "unknown rule 'Dup'; nearest declared: 'dup'",
        ),
        (
            ToolRequest::Usage {
                symbol: "nope".into(),
            },
            "the 'symbols' operation lists them",
        ),
        (
            ToolRequest::Cfg {
                rule: "nope".into(),
                kind: None,
                max: None,
                all: true,
            },
            "the 'rules' operation lists them",
        ),
    ] {
        match service.handle(&request) {
            ToolResponse::Error { error } => {
                assert!(error.message.ends_with(needle), "{request:?}: {error:?}");
            }
            ToolResponse::Ok { result } => {
                panic!("{request:?} must not return a result: {result}")
            }
        }
    }
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn semantic_query_selection_wraps_filters_and_bounds() {
    // #531: a request with any selection field returns `{<key>: [...],
    // "selection": {total, withheld}}`; `total` counts the set before
    // selection and `withheld` only what `max` dropped. Requests without
    // selection fields keep the bare shapes.
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(declarations_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    let all = result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );
    assert!(all.is_array());
    let total = all.as_array().unwrap().len();

    // `kind` predates #531: alone it keeps the filtered bare array; combined
    // with `file`/`max` it joins the selection and wraps.
    let rules_only = result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: Some("rule".to_string()),
            file: None,
            max: None,
            all: true,
        },
    );
    assert!(rules_only.is_array(), "kind alone keeps the bare array");
    let rules_only = rules_only.as_array().unwrap();
    assert!(rules_only.iter().all(|symbol| symbol["kind"] == "rule"));
    assert!(!rules_only.is_empty());
    assert!(rules_only.len() < total);

    let bounded_rules = result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: Some("rule".to_string()),
            file: None,
            max: Some(1),

            all: true,
        },
    );
    assert_eq!(bounded_rules["symbols"].as_array().unwrap().len(), 1);
    assert_eq!(bounded_rules["selection"]["total"], total);

    let bounded = result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: Some(1),

            all: true,
        },
    );
    assert_eq!(bounded["symbols"].as_array().unwrap().len(), 1);
    assert_eq!(bounded["selection"]["total"], total);
    assert_eq!(bounded["selection"]["withheld"], total - 1);

    let in_file = result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: Some("declarations-rules.ws".to_string()),
            max: None,
            all: true,
        },
    );
    assert_eq!(in_file["symbols"].as_array().unwrap().len(), total);
    let elsewhere = result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: Some("elsewhere.ws".to_string()),
            max: None,
            all: true,
        },
    );
    assert!(elsewhere["symbols"].as_array().unwrap().is_empty());
    assert_eq!(elsewhere["selection"]["total"], total);

    let rules = result_of(
        &mut service,
        &ToolRequest::Rules {
            name: None,
            file: None,
            max: None,
            rule: None,
            content: false,
            all: true,
        },
    );
    assert!(rules.is_array());
    let rule_total = rules.as_array().unwrap().len();
    for rule in rules.as_array().unwrap() {
        if rule["span"].is_object() {
            assert!(
                rule["span"]["path"].is_string(),
                "rule span carries no path: {rule}"
            );
        }
    }
    let named = result_of(
        &mut service,
        &ToolRequest::Rules {
            name: Some("player starts".to_string()),
            file: None,
            max: None,
            rule: None,
            content: false,
            all: true,
        },
    );
    let kept = named["rules"].as_array().unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0]["name"], "player starts");
    assert_eq!(kept[0]["event"], "eachPlayer");
    assert_eq!(named["selection"]["total"], rule_total);
    let bounded_rules = result_of(
        &mut service,
        &ToolRequest::Rules {
            name: None,
            file: Some("declarations-rules.ws".to_string()),
            max: Some(1),
            rule: None,
            content: false,
            all: true,
        },
    );
    assert_eq!(bounded_rules["rules"].as_array().unwrap().len(), 1);
    assert_eq!(
        bounded_rules["selection"],
        serde_json::json!({"total": rule_total, "withheld": rule_total - 1})
    );

    let references = result_of(
        &mut service,
        &ToolRequest::References {
            symbol: "score".into(),
            kind: None,
            rule: None,
            file: None,
            max: None,
            all: true,
        },
    );
    assert!(references["references"].is_array());
    let writes = result_of(
        &mut service,
        &ToolRequest::References {
            symbol: "score".into(),
            kind: Some("write".to_string()),
            rule: None,
            file: None,
            max: None,
            all: true,
        },
    );
    let kept = writes["references"].as_array().unwrap();
    assert!(!kept.is_empty());
    assert!(kept.iter().all(|reference| reference["kind"] == "write"));
    assert_eq!(
        writes["selection"]["total"],
        references["references"].as_array().unwrap().len()
    );
    let inside = result_of(
        &mut service,
        &ToolRequest::References {
            symbol: "score".into(),
            kind: None,
            rule: Some("Subroutine showStatus".into()),
            file: None,
            max: None,
            all: true,
        },
    );
    let kept = inside["references"].as_array().unwrap();
    assert!(!kept.is_empty());
    assert!(kept.iter().all(|reference| reference["rule"] == 0));
    let outside = result_of(
        &mut service,
        &ToolRequest::References {
            symbol: "score".into(),
            kind: None,
            rule: Some(1_u32.into()),
            file: None,
            max: None,
            all: true,
        },
    );
    assert!(outside["references"].as_array().unwrap().is_empty());
    assert_eq!(
        outside["selection"]["total"],
        references["references"].as_array().unwrap().len()
    );

    let cfg = result_of(
        &mut service,
        &ToolRequest::Cfg {
            rule: "player starts".into(),
            kind: None,
            max: None,
            all: true,
        },
    );
    assert!(cfg["selection"].is_null());
    let block_total = cfg["blocks"].as_array().unwrap().len();
    let entries = result_of(
        &mut service,
        &ToolRequest::Cfg {
            rule: "player starts".into(),
            kind: Some("entry".to_string()),
            max: None,
            all: true,
        },
    );
    let kept = entries["blocks"].as_array().unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0]["kind"], "entry");
    assert_eq!(kept[0]["id"], entries["entry"]);
    assert_eq!(entries["selection"]["total"], block_total);
    let bounded_cfg = result_of(
        &mut service,
        &ToolRequest::Cfg {
            rule: "player starts".into(),
            kind: None,
            max: Some(1),

            all: true,
        },
    );
    assert_eq!(bounded_cfg["blocks"].as_array().unwrap().len(), 1);
    assert_eq!(bounded_cfg["selection"]["withheld"], block_total - 1);

    let graph = result_of(
        &mut service,
        &ToolRequest::CallGraph {
            caller: None,
            callee: None,
            max: None,
            all: true,
        },
    );
    assert!(graph.is_array());
    assert_eq!(graph.as_array().unwrap().len(), 1);
    let from_rule = result_of(
        &mut service,
        &ToolRequest::CallGraph {
            caller: Some("player starts".to_string()),
            callee: None,
            max: None,
            all: true,
        },
    );
    assert_eq!(
        from_rule["edges"],
        serde_json::json!([{ "caller": "player starts", "callee": "showStatus" }])
    );
    let quiet_rule = result_of(
        &mut service,
        &ToolRequest::CallGraph {
            caller: Some("Subroutine showStatus".to_string()),
            callee: None,
            max: None,
            all: true,
        },
    );
    assert!(quiet_rule["edges"].as_array().unwrap().is_empty());
    assert_eq!(quiet_rule["selection"]["total"], 1);
    let to_sub = result_of(
        &mut service,
        &ToolRequest::CallGraph {
            caller: None,
            callee: Some("showStatus".to_string()),
            max: Some(0),

            all: true,
        },
    );
    assert!(to_sub["edges"].as_array().unwrap().is_empty());
    assert_eq!(
        to_sub["selection"],
        serde_json::json!({"total": 1, "withheld": 1})
    );
}

#[test]
fn disabled_statements_and_rules_reach_neither_cost_nor_call_graph() {
    // #633: a `disabled` statement and a `disabled rule` are not live — the
    // call graph reports no edges for them, and the cost counts agree with
    // each other instead of disagreeing on the disabled `Wait`.
    let text = r#"subroutines {
    0: reset
}

rule ("caller") {
    event {
        Ongoing - Global;
    }
    actions {
        disabled Call Subroutine(reset);
        disabled Wait(1, Ignore Condition);
    }
}

disabled rule ("disabled caller") {
    event {
        Ongoing - Global;
    }
    actions {
        Call Subroutine(reset);
        Wait(1, Ignore Condition);
    }
}
"#;
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Text {
            text: text.to_string(),
            path: None,
        },
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    let mut result_of = |request| match service.handle(&request) {
        ToolResponse::Ok { result } => result,
        ToolResponse::Error { error } => panic!("{error:?}"),
    };

    let graph = result_of(ToolRequest::CallGraph {
        caller: None,
        callee: None,
        max: None,

        all: true,
    });
    assert_eq!(graph.as_array().unwrap(), &Vec::<serde_json::Value>::new());

    let cost = result_of(ToolRequest::CostEstimate {
        selection: FindingSelection::default(),
        rule: None,
        all: true,
    });
    assert_eq!(
        cost["exact"],
        serde_json::json!({
            "emittedBytes": cost["exact"]["emittedBytes"],
            "programActions": 0,
            "programRules": 1,
            "waitActions": 0,
        })
    );

    let analyze = result_of(ToolRequest::Analyze {
        selection: FindingSelection::default(),
        all: true,
    });
    assert_eq!(
        analyze["result"]["cost"]["counts"],
        serde_json::json!({
            "rules": 1,
            "conditions": 0,
            "actions": 0,
            "waits": 0,
        }),
        "analyze counts agree with costEstimate: {analyze:?}"
    );

    // `references` stays an authored-occurrence index: it still reports
    // both call sites, so callGraph is exactly its live-call subset.
    let references = result_of(ToolRequest::References {
        symbol: "reset".into(),
        kind: Some("call".to_string()),
        rule: None,
        file: None,
        max: None,

        all: true,
    });
    assert_eq!(references["references"].as_array().unwrap().len(), 2);
}

#[test]
fn call_graph_edges_come_from_subroutine_calls_not_builtin_names() {
    // #634: `Start Rule(sub)` is a real call and produces an edge; a
    // builtin `Call` that merely shares a subroutine's name produces none.
    let text = r#"subroutines {
    0: reset
    1: bigMessage
}

rule ("caller") {
    event {
        Ongoing - Global;
    }
    actions {
        Start Rule(reset, Restart Rule);
        Big Message(All Players(All Teams), Custom String("x"));
    }
}
"#;
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Text {
            text: text.to_string(),
            path: None,
        },
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    let graph = match service.handle(&ToolRequest::CallGraph {
        caller: None,
        callee: None,
        max: None,

        all: true,
    }) {
        ToolResponse::Ok { result } => result,
        ToolResponse::Error { error } => panic!("{error:?}"),
    };
    assert_eq!(
        graph.as_array().unwrap(),
        &vec![serde_json::json!({ "caller": "caller", "callee": "reset" })],
        "Start Rule edges only: {graph:?}"
    );
}

#[test]
fn semantic_query_selection_refuses_unknown_filter_values() {
    // #531: an unknown filter value is a structured `invalid-selection`
    // error, never a silently empty result.
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(declarations_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    for request in [
        ToolRequest::Rules {
            name: Some("nope".to_string()),
            file: None,
            max: None,
            rule: None,
            content: false,
            all: true,
        },
        ToolRequest::Symbols {
            kind: Some("nope".to_string()),
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::References {
            symbol: "score".into(),
            kind: Some("nope".to_string()),
            rule: None,
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::References {
            symbol: "score".into(),
            kind: None,
            rule: Some("nope".into()),
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::References {
            symbol: "score".into(),
            kind: None,
            rule: Some(99_u32.into()),
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::Cfg {
            rule: "player starts".into(),
            kind: Some("nope".to_string()),
            max: None,
            all: true,
        },
        ToolRequest::CallGraph {
            caller: Some("nope".to_string()),
            callee: None,
            max: None,
            all: true,
        },
        ToolRequest::CallGraph {
            caller: None,
            callee: Some("nope".to_string()),
            max: None,
            all: true,
        },
    ] {
        assert_eq!(
            refusal_code(&mut service, &request),
            "invalid-selection",
            "{request:?}"
        );
    }

    for (request, expected) in [
        (
            ToolRequest::Symbols {
                kind: Some("nope".to_string()),
                file: None,
                max: None,
                all: true,
            },
            "globalVariable, playerVariable, subroutine, rule",
        ),
        (
            ToolRequest::References {
                symbol: "score".into(),
                kind: Some("nope".to_string()),
                rule: None,
                file: None,
                max: None,
                all: true,
            },
            "declaration, definition, read, write, call",
        ),
        (
            ToolRequest::Cfg {
                rule: "player starts".into(),
                kind: Some("nope".to_string()),
                max: None,
                all: true,
            },
            "entry, exit, block, if, while, for",
        ),
    ] {
        let ToolResponse::Error { error } = service.handle(&request) else {
            panic!("{request:?} must refuse")
        };
        assert!(
            error.message.contains(expected),
            "{request:?} names its accepted values: {}",
            error.message
        );
    }
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
    let agent_symbols = result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );
    let agent_references = result_of(
        &mut service,
        &ToolRequest::References {
            symbol: "score".into(),
            kind: None,
            rule: None,
            file: None,
            max: None,
            all: true,
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
            kind: None,
            max: None,
            all: true,
        },
    );
    let agent_callgraph = result_of(
        &mut service,
        &ToolRequest::CallGraph {
            caller: None,
            callee: None,
            max: None,
            all: true,
        },
    );
    let agent_cost = result_of(
        &mut service,
        &ToolRequest::CostEstimate {
            selection: FindingSelection::default(),
            rule: None,
            all: true,
        },
    );
    drop(service);

    let mut cli = CompilerSession::new(input()).unwrap();
    assert_eq!(
        serde_json::to_value(cli.symbols(None, None, None).result).unwrap(),
        agent_symbols
    );

    let refs = serde_json::to_value(cli.refs("score", None, None, None, None).result).unwrap();
    assert_eq!(refs, agent_references);
    for (key, value) in agent_usage.as_object().unwrap() {
        assert_eq!(&refs[key], value);
    }

    assert_eq!(
        serde_json::to_value(cli.cfg("player starts", None, None).result).unwrap(),
        agent_cfg
    );
    assert_eq!(
        serde_json::to_value(cli.callgraph(None, None, None).result).unwrap(),
        agent_callgraph
    );
    assert_eq!(
        serde_json::to_value(cli.cost(None).result).unwrap(),
        agent_cost
    );
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
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    ));
    assert_eq!(before, ["score", "setup"]);

    for request in [
        ToolRequest::References {
            symbol: 0.into(),
            kind: None,
            rule: None,
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::Findings {
            selection: FindingSelection::default(),
            all: true,
        },
        ToolRequest::LintRules,
        ToolRequest::Inspect { all: true },
        ToolRequest::Analyze {
            selection: Default::default(),
            all: true,
        },
    ] {
        let before = serde_json::to_string(&service.handle(&request)).unwrap();
        assert_eq!(
            before,
            serde_json::to_string(&service.handle(&request)).unwrap()
        );
    }
    assert_eq!(
        {
            let mut expected = serde_json::to_value(service.analyze()).unwrap();
            expected["result"] = wright_driver::essential::analyze(&expected["result"], true);
            expected
        },
        result_of(
            &mut service,
            &ToolRequest::Analyze {
                selection: Default::default(),
                all: true,
            }
        )
    );

    std::fs::write(&input, FRESHNESS_V2).unwrap();

    let after = names(result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    ));
    assert_eq!(after, ["points", "setup", "extra"]);
    assert_eq!(service.reload_count(), 1);

    let project = result_of(&mut service, &ToolRequest::Project);
    assert_eq!(project["rules"], 2);
    assert_eq!(project["symbols"], 3);

    let check = result_of(&mut service, &ToolRequest::Check { all: true });
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
        &ToolRequest::Lint {
            selection: FindingSelection::default(),
            all: true,
        },
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
    result_of(
        &mut service,
        &ToolRequest::Rules {
            name: None,
            file: None,
            max: None,
            rule: None,
            content: false,
            all: true,
        },
    );
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
        ToolRequest::Rules {
            name: None,
            file: None,
            max: None,
            rule: None,
            content: false,
            all: true,
        },
        ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::Findings {
            selection: FindingSelection::default(),
            all: true,
        },
    ] {
        result_of(&mut service, &request);
    }
    assert_eq!(service.reload_count(), 0);

    // Rewriting identical bytes is still the same input.
    std::fs::write(&input, FRESHNESS_V1).unwrap();
    result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );
    assert_eq!(service.reload_count(), 0);

    std::fs::write(&input, FRESHNESS_V2).unwrap();
    result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );
    assert_eq!(service.reload_count(), 1);
    result_of(&mut service, &ToolRequest::Project);
    result_of(
        &mut service,
        &ToolRequest::Rules {
            name: None,
            file: None,
            max: None,
            rule: None,
            content: false,
            all: true,
        },
    );
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
        result_of(
            service,
            &ToolRequest::Rules {
                name: None,
                file: None,
                max: None,
                rule: None,
                content: false,

                all: true,
            },
        )
        .as_array()
        .unwrap()
        .len()
    };
    assert_eq!(rules(&mut service), 1);

    // Editing a member file is observed.
    std::fs::write(dir.join("program.ws"), FRESHNESS_V2).unwrap();
    assert_eq!(rules(&mut service), 2);
    assert_eq!(service.reload_count(), 1);

    std::fs::write(dir.join("other.ws"), FRESHNESS_V1).unwrap();
    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::Rules {
                name: None,
                file: None,
                max: None,
                rule: None,
                content: false,

                all: true,
            }
        ),
        "input-kind-ambiguous"
    );
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
        refusal_code(
            &mut service,
            &ToolRequest::Cfg {
                rule: 0.into(),
                kind: None,
                max: None,
                all: true,
            }
        ),
        "stale-id"
    );
    assert_eq!(rules(&mut service), 2);
    assert_eq!(service.reload_count(), 2);
    result_of(
        &mut service,
        &ToolRequest::Cfg {
            rule: 0.into(),
            kind: None,
            max: None,
            all: true,
        },
    );

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

    std::fs::write(
        &input,
        "variables {\n    global:\n        0: score\n}\nrule (\"setup\") {\n",
    )
    .unwrap();
    for request in [
        ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::Rules {
            name: None,
            file: None,
            max: None,
            rule: None,
            content: false,
            all: true,
        },
        ToolRequest::Project,
    ] {
        let code = refusal_code(&mut service, &request);
        assert_eq!(code, "parse-error", "{request:?}");
    }
    // `check` is a workflow (#591): its own load surfaces the same
    // `parse-error` inside the envelope, not as a `ToolResponse` refusal.
    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Check { all: true }) else {
        panic!("check returns the workflow envelope")
    };
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(result["diagnostics"][0]["code"], "parse-error");
    assert!(matches!(
        service.handle(&ToolRequest::Capabilities),
        ToolResponse::Ok { .. }
    ));

    std::fs::write(&input, FRESHNESS_V2).unwrap();
    let symbols = result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );
    assert!(
        symbols
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["name"] == "points"),
        "the healed input serves the new program: {symbols}"
    );
    assert_eq!(service.reload_count(), 1);

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

    result_of(
        &mut service,
        &ToolRequest::References {
            symbol: 0.into(),
            kind: None,
            rule: None,
            file: None,
            max: None,
            all: true,
        },
    );
    result_of(
        &mut service,
        &ToolRequest::Cfg {
            rule: 0.into(),
            kind: None,
            max: None,
            all: true,
        },
    );

    std::fs::write(&input, FRESHNESS_V2).unwrap();

    for request in [
        ToolRequest::References {
            symbol: 0.into(),
            kind: None,
            rule: None,
            file: None,
            max: None,
            all: true,
        },
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
        ToolRequest::Cfg {
            rule: 0.into(),
            kind: None,
            max: None,
            all: true,
        },
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
            kind: None,
            rule: None,
            file: None,
            max: None,
            all: true,
        },
    );
    assert!(
        references["references"].as_array().unwrap().len() >= 2,
        "{references}"
    );
    result_of(
        &mut service,
        &ToolRequest::Cfg {
            rule: "extra".into(),
            kind: None,
            max: None,
            all: true,
        },
    );

    result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );
    let usage = result_of(&mut service, &ToolRequest::Usage { symbol: 0.into() });
    assert_eq!(usage["id"], 0);
    assert_eq!(usage["symbol"], "points");
    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::Cfg {
                rule: 1.into(),
                kind: None,
                max: None,
                all: true,
            }
        ),
        "stale-id"
    );
    result_of(
        &mut service,
        &ToolRequest::Rules {
            name: None,
            file: None,
            max: None,
            rule: None,
            content: false,
            all: true,
        },
    );
    result_of(
        &mut service,
        &ToolRequest::Cfg {
            rule: 1.into(),
            kind: None,
            max: None,
            all: true,
        },
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// #592: a numeric `references` `rule` filter is a rule-space address under
/// the same freshness contract as `cfg` — an index issued before a reload
/// refuses `stale-id` even when it still names a (different) valid rule.
const REORDER_V1: &str = r#"
variables {
    global:
        0: score
}
rule ("first") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(score, 1);
    }
}
rule ("second") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(score, 2);
    }
}
"#;

/// The same program with the rules swapped: index 0 is now "second".
const REORDER_V2: &str = r#"
variables {
    global:
        0: score
}
rule ("second") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(score, 2);
    }
}
rule ("first") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(score, 1);
    }
}
"#;

#[test]
fn stale_numeric_rule_filters_refuse_in_references_after_a_reload() {
    let dir = freshness_dir("stale-rule-filter");
    let input = dir.join("program.ws");
    std::fs::write(&input, REORDER_V1).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    std::fs::write(&input, REORDER_V2).unwrap();

    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::References {
                symbol: "score".into(),
                kind: None,
                rule: Some(0.into()),
                file: None,
                max: None,
                all: true,
            }
        ),
        "stale-id",
        "a numeric rule filter issued before the reload is stale"
    );

    // Name-addressed rule filters resolve against the current snapshot —
    // no observation is owed. Any filter field wraps the result in a
    // `{"references": [...], "selection": {...}}` object (#531).
    let filtered = result_of(
        &mut service,
        &ToolRequest::References {
            symbol: "score".into(),
            kind: None,
            rule: Some("first".into()),
            file: None,
            max: None,
            all: true,
        },
    );
    assert_eq!(
        filtered["references"]
            .as_array()
            .expect("filtered references")
            .len(),
        1,
        "only the write inside 'first' matches: {filtered}"
    );

    for request in [
        ToolRequest::References {
            symbol: 0.into(),
            kind: None,
            rule: None,
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::References {
            symbol: 0.into(),
            kind: None,
            rule: Some(0.into()),
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::Rules {
            name: None,
            rule: Some(0.into()),
            file: None,
            max: None,
            content: true,

            all: true,
        },
    ] {
        assert_eq!(
            refusal_code(&mut service, &request),
            "stale-id",
            "{request:?}"
        );
    }

    result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );
    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::References {
                symbol: "score".into(),
                kind: None,
                rule: Some(0.into()),
                file: None,
                max: None,
                all: true,
            }
        ),
        "stale-id"
    );

    result_of(
        &mut service,
        &ToolRequest::Rules {
            name: None,
            file: None,
            max: None,
            rule: None,
            content: false,
            all: true,
        },
    );
    let filtered = result_of(
        &mut service,
        &ToolRequest::References {
            symbol: "score".into(),
            kind: None,
            rule: Some(0.into()),
            file: None,
            max: None,
            all: true,
        },
    );
    let references = filtered["references"]
        .as_array()
        .expect("filtered references");
    assert_eq!(
        references.len(),
        1,
        "rule 0 is now 'second': {references:?}"
    );
    assert_eq!(references[0]["rule"], 0);
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
    result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );

    std::fs::write(&input, FRESHNESS_AMBIGUOUS).unwrap();

    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::References {
                symbol: "dup".into(),
                kind: None,
                rule: None,
                file: None,
                max: None,
                all: true,
            }
        ),
        "ambiguous-symbol"
    );
    result_of(&mut service, &ToolRequest::Usage { symbol: 0.into() });
    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::Cfg {
                rule: 0.into(),
                kind: None,
                max: None,
                all: true,
            }
        ),
        "stale-id",
        "the rule space is untouched by a symbol-space observation"
    );
    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::Cfg {
                rule: "dup".into(),
                kind: None,
                max: None,
                all: true,
            }
        ),
        "ambiguous-rule"
    );
    result_of(
        &mut service,
        &ToolRequest::Cfg {
            rule: 0.into(),
            kind: None,
            max: None,
            all: true,
        },
    );
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

    for request in [
        ToolRequest::References {
            symbol: 0.into(),
            kind: None,
            rule: None,
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::Usage { symbol: 0.into() },
        ToolRequest::Cfg {
            rule: 0.into(),
            kind: None,
            max: None,
            all: true,
        },
    ] {
        assert_eq!(
            refusal_code(&mut service, &request),
            "stale-id",
            "{request:?}"
        );
    }

    result_of(&mut service, &ToolRequest::Capabilities);
    result_of(&mut service, &ToolRequest::TargetMetadata);
    for (request, command) in [
        (compile_request("stale.ow"), "compile"),
        (ToolRequest::Check { all: true }, "check"),
        (
            ToolRequest::Analyze {
                selection: Default::default(),
                all: true,
            },
            "analyze",
        ),
        (ToolRequest::Inspect { all: true }, "inspect"),
    ] {
        assert_eq!(result_of(&mut service, &request)["command"], command);
    }
    let project = result_of(&mut service, &ToolRequest::Project);
    assert_eq!(project["rules"], 2);
    assert_eq!(project["symbols"], 3);
    let symbols = result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );
    assert!(
        symbols
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["name"] == "points")
    );
    let usage = result_of(
        &mut service,
        &ToolRequest::Usage {
            symbol: "points".into(),
        },
    );
    assert_eq!(usage["id"], 0);
    assert_eq!(usage["symbol"], "points");
    assert_eq!(
        result_of(
            &mut service,
            &ToolRequest::Rules {
                name: None,
                file: None,
                max: None,
                rule: None,
                content: false,

                all: true,
            }
        )
        .as_array()
        .unwrap()
        .len(),
        2
    );
    let references = result_of(
        &mut service,
        &ToolRequest::References {
            symbol: "points".into(),
            kind: None,
            rule: None,
            file: None,
            max: None,
            all: true,
        },
    );
    assert!(
        references["references"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reference| reference["rule"] == 1),
        "{references}"
    );
    let findings = result_of(
        &mut service,
        &ToolRequest::Findings {
            selection: FindingSelection::default(),
            all: true,
        },
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
        &ToolRequest::Lint {
            selection: FindingSelection::default(),
            all: true,
        },
    );
    result_of(&mut service, &ToolRequest::LintRules);
    result_of(&mut service, &ToolRequest::PersistentObjects { all: true });
    result_of(
        &mut service,
        &ToolRequest::CallGraph {
            caller: None,
            callee: None,
            max: None,
            all: true,
        },
    );
    let cost = result_of(
        &mut service,
        &ToolRequest::CostEstimate {
            selection: FindingSelection::default(),
            rule: None,
            all: true,
        },
    );
    assert_eq!(cost["exact"]["programRules"], 2);
    let inspect = result_of(&mut service, &ToolRequest::Inspect { all: true });
    assert_eq!(inspect["result"]["rules"].as_array().unwrap().len(), 2);
    let analyze = result_of(
        &mut service,
        &ToolRequest::Analyze {
            selection: Default::default(),
            all: true,
        },
    );
    assert!(
        analyze["result"]["variables"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["name"] == "points")
    );

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

    for request in [
        ToolRequest::ProviderSemanticRename {
            language_id: "not-a-language".to_string(),
            documents: Some(Default::default()),
            position_document_uri: String::new(),
            position: wright_lpp::Position {
                line: 0,
                character: 0,
            },
            new_name: String::new(),
            project_root: None,
            sources: None,
        },
        ToolRequest::ProviderValidateEdit {
            language_id: "not-a-language".to_string(),
            documents: Some(Default::default()),
            transaction: wright_driver::edit::EditTransaction { edits: vec![] },
            sources: None,
            project_root: None,
        },
    ] {
        let result = result_of(&mut service, &request);
        assert_eq!(result["ok"], false, "{request:?}: {result}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

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

    result_of(&mut service, &ToolRequest::Capabilities);
    result_of(&mut service, &ToolRequest::TargetMetadata);

    for request in [
        ToolRequest::Project,
        ToolRequest::Rules {
            name: None,
            file: None,
            max: None,
            rule: None,
            content: false,
            all: true,
        },
        ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::References {
            symbol: 0.into(),
            kind: None,
            rule: None,
            file: None,
            max: None,
            all: true,
        },
    ] {
        assert_eq!(
            refusal_code(&mut service, &request),
            "input-io",
            "{request:?}"
        );
    }

    // `check`/`compile` are workflows (#591): the same loader diagnostic
    // rides the `wright-result/v1` envelope instead of a `ToolResponse`
    // refusal, matching the embedding `check`/`compile` methods exactly.
    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Check { all: true }) else {
        panic!("check returns the workflow envelope")
    };
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(result["diagnostics"][0]["code"], "input-io");

    // A provider mutation carrying its own documents does not trigger the
    // load: an unconfigured language is the provider refusal result, not the
    // load failure. One that omits `documents` derives the set from the
    // loaded project (#548) and so surfaces the same `input-io` refusal the
    // program-reading requests do.
    let mutation = result_of(
        &mut service,
        &ToolRequest::ProviderValidateEdit {
            language_id: "not-a-language".to_string(),
            documents: Some(Default::default()),
            transaction: wright_driver::edit::EditTransaction { edits: vec![] },
            sources: None,
            project_root: None,
        },
    );
    assert_eq!(mutation["ok"], false, "{mutation}");
    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::ProviderValidateEdit {
                language_id: "not-a-language".to_string(),
                documents: None,
                transaction: wright_driver::edit::EditTransaction { edits: vec![] },
                sources: None,
                project_root: None,
            },
        ),
        "input-io"
    );
    assert!(service.loaded().is_none());

    let check = service.check();
    assert!(!check.ok);
    assert!(
        check.diagnostics.iter().any(|d| d.code == "input-io"),
        "{:?}",
        check.diagnostics
    );

    std::fs::write(&input, FRESHNESS_V1).unwrap();
    let symbols = result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );
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

    std::fs::remove_file(&input).unwrap();
    for request in [
        ToolRequest::Project,
        ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    ] {
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
        ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    ] {
        assert_eq!(
            refusal_code(&mut service, &request),
            "parse-error",
            "{request:?}"
        );
    }
    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Compile {
        output: None,
        force: false,
        to: None,
    }) else {
        panic!("compile returns the workflow envelope")
    };
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(result["diagnostics"][0]["code"], "parse-error");

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
    result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );
    result_of(
        &mut service,
        &ToolRequest::Rules {
            name: None,
            file: None,
            max: None,
            rule: None,
            content: false,
            all: true,
        },
    );
    result_of(
        &mut service,
        &ToolRequest::References {
            symbol: 0.into(),
            kind: None,
            rule: None,
            file: None,
            max: None,
            all: true,
        },
    );

    std::fs::write(
        &input,
        "variables {\n    global:\n        0: score\n}\nrule (\"setup\") {\n",
    )
    .unwrap();
    assert_eq!(
        refusal_code(&mut service, &ToolRequest::Project),
        "parse-error"
    );

    std::fs::write(&input, FRESHNESS_V2).unwrap();
    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::References {
                symbol: 0.into(),
                kind: None,
                rule: None,
                file: None,
                max: None,
                all: true,
            }
        ),
        "stale-id"
    );
    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::Cfg {
                rule: 1.into(),
                kind: None,
                max: None,
                all: true,
            }
        ),
        "stale-id"
    );
    result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );
    let usage = result_of(&mut service, &ToolRequest::Usage { symbol: 0.into() });
    assert_eq!(usage["symbol"], "points");
    result_of(
        &mut service,
        &ToolRequest::Rules {
            name: None,
            file: None,
            max: None,
            rule: None,
            content: false,
            all: true,
        },
    );
    result_of(
        &mut service,
        &ToolRequest::Cfg {
            rule: 1.into(),
            kind: None,
            max: None,
            all: true,
        },
    );
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
    result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );
    result_of(
        &mut service,
        &ToolRequest::References {
            symbol: 0.into(),
            kind: None,
            rule: None,
            file: None,
            max: None,
            all: true,
        },
    );
    let builds = service.semantic_build_count();

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

    result_of(&mut service, &ToolRequest::Project);
    assert_eq!(service.semantic_build_count(), builds + 1);

    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::References {
                symbol: 0.into(),
                kind: None,
                rule: None,
                file: None,
                max: None,
                all: true,
            }
        ),
        "stale-id"
    );
    result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );
    result_of(
        &mut service,
        &ToolRequest::References {
            symbol: 0.into(),
            kind: None,
            rule: None,
            file: None,
            max: None,
            all: true,
        },
    );
    let _ = std::fs::remove_dir_all(&dir);
}

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
fn rename_accepts_percent_encoded_file_uri_sources() {
    // #636: a `sources` key spelled `file:///…/my%20dir/main.ws` binds the
    // member whose path contains a space — the rename succeeds exactly as
    // with the plain spelling.
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-driver-636-uri-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let member = dir.join("my dir");
    std::fs::create_dir_all(&member).unwrap();
    let path = member.join("main.ws");
    std::fs::write(&path, RENAMEABLE).unwrap();
    let on_disk = std::fs::read_to_string(&path).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    let uri = format!("file://{}", path.display().to_string().replace(' ', "%20"));
    let encoded = result_of(
        &mut service,
        &ToolRequest::SemanticRename {
            sources: Some(BTreeMap::from([(uri, on_disk.clone())])),
            target: rename_target(),
        },
    );
    assert_eq!(encoded["ok"], true, "{encoded:?}");
    let plain = result_of(
        &mut service,
        &ToolRequest::SemanticRename {
            sources: Some(BTreeMap::from([(
                path.to_string_lossy().into_owned(),
                on_disk,
            )])),
            target: rename_target(),
        },
    );
    assert_eq!(plain["ok"], true, "{plain:?}");

    let _ = std::fs::remove_dir_all(dir);
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
        let analyze = result_of(
            &mut service,
            &ToolRequest::Analyze {
                selection: Default::default(),
                all: true,
            },
        );
        assert_eq!(analyze["command"], "analyze");
        assert!(analyze["ok"].as_bool().unwrap_or(false), "{analyze}");
    }
    assert_eq!(service.semantic_build_count(), 1);

    assert!(service.analyze().ok);
    assert_eq!(service.semantic_build_count(), 1);
}

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
    assert!(session.symbols(None, None, None).ok);
    assert!(session.refs("score", None, None, None, None).ok);
    assert!(session.cfg("player starts", None, None).ok);
    assert!(session.callgraph(None, None, None).ok);
    assert!(session.cost(None).ok);
    assert_eq!(session.semantic_build_count(), 1);

    assert!(session.analyze().ok);
    assert!(session.inspect().ok);
    assert!(session.lint().ok);
    assert!(session.symbols(None, None, None).ok);
    assert_eq!(session.semantic_build_count(), 1);
}

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
    result_of(
        &mut service,
        &ToolRequest::Analyze {
            selection: Default::default(),
            all: true,
        },
    );
    assert_eq!(service.semantic_build_count(), 1);

    std::fs::write(&input, FRESHNESS_V2).unwrap();
    let analyze = result_of(
        &mut service,
        &ToolRequest::Analyze {
            selection: Default::default(),
            all: true,
        },
    );
    assert!(
        analyze["result"]["variables"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["name"] == "points"),
        "the reloaded program's facts are served: {analyze}"
    );
    assert_eq!(service.semantic_build_count(), 2);

    result_of(
        &mut service,
        &ToolRequest::Analyze {
            selection: Default::default(),
            all: true,
        },
    );
    result_of(&mut service, &ToolRequest::Inspect { all: true });
    assert_eq!(service.semantic_build_count(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rules_content_serves_bounded_canonical_call_trees() {
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(declarations_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    let request = |rule: Option<Address>, max: Option<usize>| ToolRequest::Rules {
        name: None,
        rule,
        file: None,
        max,
        content: true,

        all: true,
    };

    let all = result_of(&mut service, &request(None, None));
    let rules = all["rules"].as_array().unwrap();
    assert_eq!(all["selection"]["withheld"], 0);
    assert!(
        rules
            .iter()
            .all(|rule| rule["content"]["format"] == "workshop-rs/rule-content-v1")
    );
    // A raw Workshop result carries neither `names` nor `unavailable`.
    assert!(all.get("names").is_none() && all.get("unavailable").is_none());

    let one = result_of(
        &mut service,
        &request(Some(Address::Name("player starts".to_string())), None),
    );
    let rule = &one["rules"][0];
    assert_eq!(one["rules"].as_array().unwrap().len(), 1);
    assert_eq!(rule["name"], "player starts");
    let condition = &rule["content"]["conditions"][0];
    assert_eq!(condition["value"]["call"], "==");
    assert_eq!(condition["value"]["args"][0]["call"], "hasSpawned");
    assert_eq!(condition["value"]["args"][1], true);
    assert!(condition["span"]["path"].is_string());
    let actions = rule["content"]["actions"].as_array().unwrap();
    assert_eq!(actions[0]["call"], "setPlayerVariable");
    assert_eq!(
        actions[0]["args"][1],
        serde_json::json!({"variable": "hasStarted"})
    );
    assert_eq!(
        actions[1],
        serde_json::json!({
            "call": "callSubroutine",
            "args": [{"subroutine": "showStatus"}],
            "span": actions[1]["span"],
        })
    );
    let by_id = result_of(&mut service, &request(Some(Address::Id(1)), None));
    assert_eq!(by_id["rules"][0]["content"], rule["content"]);

    for statement in rule["content"]["conditions"]
        .as_array()
        .unwrap()
        .iter()
        .chain(rule["content"]["actions"].as_array().unwrap())
    {
        assert_eq!(statement["span"]["path"], "declarations-rules.ws");
    }

    let bounded = result_of(&mut service, &request(None, Some(1)));
    assert_eq!(bounded["rules"].as_array().unwrap().len(), 1);
    assert_eq!(bounded["selection"]["withheld"], rules.len() - 1);

    assert_eq!(
        refusal_code(
            &mut service,
            &request(Some(Address::Name("nope".to_string())), None)
        ),
        "invalid-selection"
    );
    assert_eq!(
        refusal_code(&mut service, &request(Some(Address::Id(99)), None)),
        "invalid-selection"
    );

    let plain = result_of(
        &mut service,
        &ToolRequest::Rules {
            name: None,
            rule: None,
            file: None,
            max: None,
            content: false,
            all: true,
        },
    );
    assert!(plain.is_array());
    assert!(plain[0].get("content").is_none());
}

/// A `content` request without `max` applies the default bound of 20 rules
/// (ADR-0024): the selection reports every withheld rule. A `rule` name
/// shared by two rules refuses `ambiguous-rule`, the refusal `cfg` gives.
#[test]
fn rules_content_defaults_to_twenty_rules_and_names_ambiguity() {
    let dir = freshness_dir("rules-content-bound");
    let input = dir.join("program.ws");
    let mut text = "variables {\n    global:\n        0: score\n}\n".to_string();
    for index in 0..25 {
        let name = if index < 2 {
            "dup".to_string()
        } else {
            format!("r{index}")
        };
        text.push_str(&format!(
            "rule (\"{name}\") {{\n    event {{\n        Ongoing - Global;\n    }}\n    actions {{\n        Set Global Variable(score, {index});\n    }}\n}}\n"
        ));
    }
    std::fs::write(&input, text).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    let request = |rule: Option<Address>, max: Option<usize>| ToolRequest::Rules {
        name: None,
        rule,
        file: None,
        max,
        content: true,

        all: false,
    };

    let bounded = result_of(&mut service, &request(None, None));
    let mut full_request = request(None, None);
    if let ToolRequest::Rules { all, .. } = &mut full_request {
        *all = true;
    }
    assert_eq!(
        result_of(&mut service, &full_request)["rules"]
            .as_array()
            .unwrap()
            .len(),
        25
    );

    assert_eq!(bounded["rules"].as_array().unwrap().len(), 20);
    assert_eq!(
        bounded["selection"],
        serde_json::json!({"total": 25, "withheld": 5})
    );

    let one = result_of(
        &mut service,
        &request(Some(Address::Name("r3".to_string())), None),
    );
    assert_eq!(one["rules"].as_array().unwrap().len(), 1);
    assert_eq!(one["rules"][0]["name"], "r3");

    assert_eq!(
        refusal_code(
            &mut service,
            &request(Some(Address::Name("dup".to_string())), None)
        ),
        "ambiguous-rule"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// #639: a fold that would produce a non-finite number must not turn a
/// valid program invalid — `Divide(1, 0)` keeps the same pass/fail result
/// under `compat` as under `off`.
#[test]
fn a_fold_that_would_be_non_finite_keeps_the_off_profile_verdict() {
    let path = temp_workshop(
        "divide-by-zero.ws",
        "rule (\"r\")\n{\n    actions\n    {\n        Set Global Variable(A, Divide(1, 0));\n    }\n}\n",
    );
    let verdict = |profile: wright_driver::Profile| {
        let mut session = CompilerSession::new(SessionConfig {
            input: InputSpec::Path(path.clone()),
            kind: SourceKind::Workshop,
            profile,
            ..SessionConfig::default()
        })
        .expect("session creates");
        session.check().ok
    };
    let off = verdict(wright_driver::Profile::Off);
    let compat = verdict(wright_driver::Profile::Compat);
    assert!(off, "the input is valid without transforms");
    assert_eq!(
        off, compat,
        "compat must not turn a valid program into an invalid one"
    );
}

/// #639: the `Or(True, X)`/`And(False, X)` short-circuit folds discard X —
/// an identifier-bearing operand must survive, so `references` and
/// `semanticRename` for `g` return the same result under `compat` as under
/// `off`.
#[test]
fn short_circuit_folds_preserve_occurrences_for_references_and_rename() {
    let path = temp_workshop(
        "or-fold.ws",
        "variables\n{\n    global:\n        0: g\n}\n\nrule (\"r\")\n{\n    conditions\n    {\n        Or(True, Global.g) == True;\n    }\n    actions\n    {\n        Wait(1, Abort When False);\n    }\n}\n",
    );
    let query = |profile: wright_driver::Profile| {
        let mut session = CompilerSession::new(SessionConfig {
            input: InputSpec::Path(path.clone()),
            kind: SourceKind::Workshop,
            profile,
            ..SessionConfig::default()
        })
        .expect("session creates");
        let mut service = ToolService::new(&mut session).expect("service");
        let references = result_of(
            &mut service,
            &ToolRequest::References {
                symbol: "g".into(),
                kind: None,
                rule: None,
                file: None,
                max: None,
                all: true,
            },
        );
        let rename = result_of(
            &mut service,
            &ToolRequest::SemanticRename {
                sources: None,
                target: RenameTarget {
                    symbol: Some(Address::Name("g".to_string())),
                    source: None,
                    line: None,
                    col: None,
                    to: "renamed".to_string(),
                },
            },
        );
        (references, rename)
    };

    let (off_references, off_rename) = query(wright_driver::Profile::Off);
    let (compat_references, compat_rename) = query(wright_driver::Profile::Compat);
    assert_eq!(
        compat_references, off_references,
        "references see the same occurrences under both profiles"
    );
    assert_eq!(
        compat_rename, off_rename,
        "semanticRename resolves the same occurrences under both profiles"
    );
    assert!(
        off_references["references"]
            .as_array()
            .expect("reference list")
            .iter()
            .any(|reference| reference["kind"] == "read"),
        "the Or-folded operand keeps its authored read: {off_references:?}"
    );
    assert_eq!(
        off_rename["ok"],
        serde_json::Value::Bool(true),
        "rename validates: {off_rename:?}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// ADR-0025: the agent `compile` operation writes the program to a file and
/// answers with status, path and digest only; an existing file needs `force`.
#[test]
fn agent_compile_writes_a_file_and_returns_no_program_text() {
    let out = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("agent-compile.ow");
    let _ = std::fs::remove_file(&out);
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    let request = |force| ToolRequest::Compile {
        output: Some(out.clone()),
        force,
        to: None,
    };

    let first = result_of(&mut service, &request(false));
    assert_eq!(first["ok"], true, "{first}");
    let output = &first["result"]["output"];
    assert_eq!(output["written_to"], out.to_str().unwrap());
    assert!(output.get("text").is_none(), "{output}");
    let written = std::fs::read_to_string(&out).unwrap();
    assert_eq!(
        output["sha256"],
        wright_driver::sha256_hex(written.as_bytes())
    );

    let again = result_of(&mut service, &request(false));
    assert_eq!(again["exit"], 2, "{again}");
    assert_eq!(again["diagnostics"][0]["code"], "output-exists");
    assert!(result_of(&mut service, &request(true))["ok"] == true);
    let _ = std::fs::remove_file(&out);
}
