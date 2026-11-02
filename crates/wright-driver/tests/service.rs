use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wright_driver::edit::{EditRange, EditTransaction, SourceEdit};
use wright_driver::essential;
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
    let default_finding = default_findings["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["code"] == "min-wait-loop")
        .expect("the fixture triggers min-wait-loop");
    assert_eq!(default_finding["severity"], "warning");
}

fn compile_request(name: &str) -> ToolRequest {
    ToolRequest::Compile {
        output: Some(std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)),
        force: true,
        to: None,
    }
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
fn inspect_results_resolve_span_paths() {
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
                if let Some(span) = object.get("span")
                    && span.is_object()
                {
                    spans.push(span.clone());
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
fn check_reports_the_program_summary_and_follow_ups() {
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let check = serde_json::to_value(session.check().result).unwrap();
    let analyze = serde_json::to_value(session.analyze().result).unwrap();
    let inspect = serde_json::to_value(session.inspect().result).unwrap();

    assert_eq!(check["program"], analyze["program"]);
    assert!(
        check["program"]["rules"].as_u64().unwrap() > 0,
        "check's summary counts the loaded program: {check}"
    );
    for (surface, result) in [
        ("check", &check),
        ("analyze", &analyze),
        ("inspect", &inspect),
    ] {
        let next = result["next"]
            .as_array()
            .unwrap_or_else(|| panic!("{surface} offers follow-up commands: {result}"));
        assert!(!next.is_empty(), "{surface} lists at least one follow-up");
        assert!(
            next.iter().all(|command| command
                .as_str()
                .is_some_and(|command| command.starts_with("wright "))),
            "{surface} follow-ups are complete `wright` command lines: {next:?}"
        );
    }
}

#[test]
fn references_carry_their_owning_rule_identity() {
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(declarations_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let refs = serde_json::to_value(session.refs("score", None, None, None, None).result).unwrap();
    let entries = refs["references"]
        .as_array()
        .expect("refs lists references");
    assert!(!entries.is_empty(), "the fixture names references");
    let mut owned = 0;
    for entry in entries {
        match (entry["rule"].as_u64(), entry["owningRule"].as_object()) {
            (Some(id), Some(owning)) => {
                owned += 1;
                assert_eq!(owning["id"].as_u64().unwrap(), id);
                assert!(
                    owning["name"].as_str().is_some_and(|name| !name.is_empty()),
                    "owningRule names the rule: {entry}"
                );
                assert!(
                    owning["span"]["path"]
                        .as_str()
                        .is_some_and(|path| !path.is_empty()),
                    "owningRule's span resolves a path: {entry}"
                );
            }
            (None, None) => {}
            _ => panic!("`rule` and `owningRule` disagree: {entry}"),
        }
    }
    assert!(owned > 0, "the fixture's references own a rule: {refs}");
}

#[test]
fn cost_estimate_file_selection_narrows_findings_like_lint() {
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

    let code = refusal_code(
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
    assert_eq!(code, "invalid-selection");
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
        serde_json::json!({"total": 1, "withheld": 0, "byCode": {"min-wait-loop": 1}})
    );
    assert_eq!(lint["counts"]["findings"]["total"], 1);
    assert_eq!(lint["findings"].as_array().unwrap().len(), 1);
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

    let generation = service.snapshot_generation().expect("loaded");
    let symbols = result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );
    assert_eq!(
        symbols["snapshot"], generation,
        "symbols report their generation"
    );
    let symbol_id = |name: &str| {
        symbols["symbols"]
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
            snapshot: None,
            name: None,
            file: None,
            max: None,
            rule: None,
            content: false,
            all: true,
        },
    );
    assert_eq!(
        rules["snapshot"], generation,
        "rules report their generation"
    );
    let rule_index = |name: &str| {
        rules["rules"]
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
                snapshot: None,
                symbol: "score".into(),
                kind: None,
                rule: None,
                file: None,
                max: None,
                all: true,
            },
            ToolRequest::References {
                snapshot: Some(generation),
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
                snapshot: None,
                symbol: "score".into(),
            },
            ToolRequest::Usage {
                snapshot: Some(generation),
                symbol: symbol_id("score").into(),
            },
        ),
        (
            ToolRequest::Cfg {
                snapshot: None,
                rule: "player starts".into(),
                kind: None,
                max: None,
                all: true,
            },
            ToolRequest::Cfg {
                snapshot: Some(generation),
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
            snapshot: None,
            symbol: "score".into(),
        },
    );
    assert_eq!(usage["id"], symbol_id("score"));
    assert_eq!(usage["kind"], "globalVariable");
    assert_eq!(usage["symbol"], "score");

    assert_eq!(symbol_id("player starts"), 4);
    assert_eq!(rule_index("player starts"), 1);
    match service.handle(&ToolRequest::Cfg {
        snapshot: Some(generation),
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
            snapshot: None,
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
    let spanned: Vec<_> = symbols["symbols"]
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
fn semantic_query_selection_wraps_filters_and_bounds() {
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
    assert_eq!(all["snapshot"], 1, "{all}");
    let total = all["symbols"].as_array().unwrap().len();

    let rules_only = result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: Some("rule".to_string()),
            file: None,
            max: None,
            all: true,
        },
    );
    let rules_only = rules_only["symbols"].as_array().unwrap();
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
    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::Symbols {
                kind: None,
                file: Some("elsewhere.ws".to_string()),
                max: None,
                all: true,
            }
        ),
        "invalid-selection"
    );

    let rules = result_of(
        &mut service,
        &ToolRequest::Rules {
            snapshot: None,
            name: None,
            file: None,
            max: None,
            rule: None,
            content: false,
            all: true,
        },
    );
    assert_eq!(rules["snapshot"], 1, "{rules}");
    let rule_total = rules["rules"].as_array().unwrap().len();
    for rule in rules["rules"].as_array().unwrap() {
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
            snapshot: None,
            name: None,
            file: None,
            max: None,
            rule: Some("player starts".into()),
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
            snapshot: None,
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
            snapshot: None,
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
            snapshot: None,
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
            snapshot: None,
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
    let generation = service.snapshot_generation();
    let outside = result_of(
        &mut service,
        &ToolRequest::References {
            snapshot: generation,
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
            snapshot: None,
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
            snapshot: None,
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
            snapshot: None,
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
        serde_json::json!([
            { "caller": "player starts", "callerAddress": "player starts", "callee": "showStatus" }
        ])
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
            "programRules": 2,
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
            "rules": 2,
            "conditions": 0,
            "actions": 0,
            "waits": 0,
        }),
        "analyze counts agree with costEstimate: {analyze:?}"
    );

    let references = result_of(ToolRequest::References {
        snapshot: None,
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
        &vec![
            serde_json::json!({ "caller": "caller", "callerAddress": "caller", "callee": "reset" })
        ],
        "Start Rule edges only: {graph:?}"
    );
}

#[test]
fn semantic_query_selection_refuses_unknown_filter_values() {
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(declarations_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::Rules {
                snapshot: None,
                name: None,
                file: None,
                max: None,
                rule: Some("nope".into()),
                content: false,
                all: true,
            }
        ),
        "unknown-rule"
    );

    for request in [
        ToolRequest::Symbols {
            kind: Some("nope".to_string()),
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::References {
            snapshot: None,
            symbol: "score".into(),
            kind: Some("nope".to_string()),
            rule: None,
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::References {
            snapshot: None,
            symbol: "score".into(),
            kind: None,
            rule: Some("nope".into()),
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::References {
            snapshot: service.snapshot_generation(),
            symbol: "score".into(),
            kind: None,
            rule: Some(99_u32.into()),
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::Cfg {
            snapshot: None,
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
                snapshot: None,
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
                snapshot: None,
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

const FRESHNESS_AMBIGUOUS: &str = r#"
variables {
    global:
        0: dup
}
subroutines {
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

    let symbols = |service: &mut ToolService<'_>| {
        result_of(
            service,
            &ToolRequest::Symbols {
                kind: None,
                file: None,
                max: None,
                all: true,
            },
        )["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .map(|symbol| symbol["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(symbols(&mut service), ["score", "setup"]);

    for request in [
        ToolRequest::References {
            snapshot: service.snapshot_generation(),
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
            let envelope = service.analyze();
            let mut expected = serde_json::to_value(&envelope).unwrap();
            let meta = &envelope.result.risks_meta;
            expected["result"] = essential::analyze(&expected["result"].take(), meta, true).0;
            expected["snapshot"] = serde_json::json!(service.snapshot_generation().unwrap());
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

    assert_eq!(symbols(&mut service), ["points", "setup", "extra"]);
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

    result_of(&mut service, &ToolRequest::Project);
    result_of(
        &mut service,
        &ToolRequest::Rules {
            snapshot: None,
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
            snapshot: None,
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
            snapshot: None,
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
                snapshot: None,
                name: None,
                file: None,
                max: None,
                rule: None,
                content: false,

                all: true,
            },
        )["rules"]
            .as_array()
            .unwrap()
            .len()
    };
    assert_eq!(rules(&mut service), 1);

    std::fs::write(dir.join("program.ws"), FRESHNESS_V2).unwrap();
    assert_eq!(rules(&mut service), 2);
    assert_eq!(service.reload_count(), 1);
    let dropped = service.snapshot_generation().expect("loaded");

    std::fs::write(dir.join("other.ws"), FRESHNESS_V1).unwrap();
    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::Rules {
                snapshot: None,
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
    assert_eq!(
        service.snapshot_generation(),
        None,
        "the failed refresh dropped the snapshot"
    );
    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::Cfg {
                snapshot: Some(dropped),
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
    let v2 = service.snapshot_generation();
    assert_ne!(v2, Some(dropped), "recovery adopts a new generation");
    result_of(
        &mut service,
        &ToolRequest::Cfg {
            snapshot: v2,
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
            snapshot: None,
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
        symbols["symbols"]
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
fn numeric_ids_are_scoped_to_the_issuing_snapshot() {
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
    let v1 = service
        .snapshot_generation()
        .expect("the initial load adopts");

    result_of(
        &mut service,
        &ToolRequest::References {
            snapshot: Some(v1),
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
            snapshot: Some(v1),
            rule: 0.into(),
            kind: None,
            max: None,
            all: true,
        },
    );

    let capped = result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: Some(0),
            all: true,
        },
    );
    assert_eq!(capped["symbols"].as_array().unwrap().len(), 0);
    assert_eq!(capped["snapshot"], v1, "the listing reports its generation");
    for request in [
        ToolRequest::References {
            snapshot: None,
            symbol: 0.into(),
            kind: None,
            rule: None,
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::Usage {
            symbol: 0.into(),
            snapshot: None,
        },
        ToolRequest::Cfg {
            snapshot: None,
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

    std::fs::write(&input, FRESHNESS_V2).unwrap();

    for request in [
        ToolRequest::References {
            snapshot: Some(v1),
            symbol: 0.into(),
            kind: None,
            rule: None,
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::Usage {
            symbol: 0.into(),
            snapshot: Some(v1),
        },
        ToolRequest::Cfg {
            snapshot: Some(v1),
            rule: 0.into(),
            kind: None,
            max: None,
            all: true,
        },
        ToolRequest::Rules {
            snapshot: Some(v1),
            name: None,
            rule: Some(0.into()),
            file: None,
            max: None,
            content: false,
            all: true,
        },
    ] {
        assert_eq!(
            refusal_code(&mut service, &request),
            "stale-id",
            "{request:?}"
        );
    }

    let symbols = result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );
    let v2 = symbols["snapshot"]
        .as_u64()
        .expect("the listing reports it");
    assert_ne!(v2, v1, "a new snapshot was adopted");
    assert_eq!(service.snapshot_generation(), Some(v2));
    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::Usage {
                symbol: 0.into(),
                snapshot: Some(v1),
            }
        ),
        "stale-id",
        "re-observing the listing does not make generation {v1} current"
    );

    let usage = result_of(
        &mut service,
        &ToolRequest::Usage {
            symbol: 0.into(),
            snapshot: Some(v2),
        },
    );
    assert_eq!(usage["id"], 0);
    assert_eq!(usage["symbol"], "points");
    assert_eq!(usage["snapshot"], v2);
    let references = result_of(
        &mut service,
        &ToolRequest::References {
            snapshot: Some(v1),
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
        "a name address ignores the stale snapshot: {references}"
    );
    result_of(
        &mut service,
        &ToolRequest::Cfg {
            snapshot: Some(999),
            rule: "extra".into(),
            kind: None,
            max: None,
            all: true,
        },
    );
    result_of(
        &mut service,
        &ToolRequest::Cfg {
            snapshot: Some(v2),
            rule: 1.into(),
            kind: None,
            max: None,
            all: true,
        },
    );
    let _ = std::fs::remove_dir_all(&dir);
}

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
    let v1 = service
        .snapshot_generation()
        .expect("the initial load adopts");

    std::fs::write(&input, REORDER_V2).unwrap();

    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::References {
                snapshot: Some(v1),
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

    let filtered = result_of(
        &mut service,
        &ToolRequest::References {
            snapshot: None,
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
            snapshot: Some(v1),
            symbol: 0.into(),
            kind: None,
            rule: None,
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::References {
            snapshot: Some(v1),
            symbol: 0.into(),
            kind: None,
            rule: Some(0.into()),
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::Rules {
            snapshot: Some(v1),
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

    let symbols = result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );
    let v2 = symbols["snapshot"]
        .as_u64()
        .expect("the listing reports it");
    assert_ne!(v2, v1);
    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::References {
                snapshot: Some(v1),
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
            snapshot: None,
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
            snapshot: Some(v2),
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
fn ambiguous_refusals_do_not_unlock_numeric_ids() {
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
    let v1 = service
        .snapshot_generation()
        .expect("the initial load adopts");
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
                snapshot: None,
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
    let v2 = service
        .snapshot_generation()
        .expect("a snapshot is adopted");
    assert_ne!(v2, v1);
    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::Usage {
                symbol: 0.into(),
                snapshot: Some(v1),
            }
        ),
        "stale-id",
        "the ambiguous-symbol refusal unlocked nothing"
    );
    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::Cfg {
                snapshot: Some(v1),
                rule: 0.into(),
                kind: None,
                max: None,
                all: true,
            }
        ),
        "stale-id",
        "the ambiguous-symbol refusal unlocked nothing"
    );
    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::Cfg {
                snapshot: None,
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
            snapshot: Some(v2),
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
    let v1 = service
        .snapshot_generation()
        .expect("the initial load adopts");
    std::fs::write(&input, FRESHNESS_V2).unwrap();

    for request in [
        ToolRequest::References {
            snapshot: Some(v1),
            symbol: 0.into(),
            kind: None,
            rule: None,
            file: None,
            max: None,
            all: true,
        },
        ToolRequest::Usage {
            symbol: 0.into(),
            snapshot: Some(v1),
        },
        ToolRequest::Cfg {
            snapshot: Some(v1),
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
    let v2 = service.snapshot_generation().expect("the reload adopted");
    assert_ne!(v2, v1);
    assert_eq!(project["snapshot"], v2, "{project}");
    let symbols = result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );
    assert_eq!(symbols["snapshot"], v2);
    assert!(
        symbols["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["name"] == "points")
    );
    let usage = result_of(
        &mut service,
        &ToolRequest::Usage {
            snapshot: None,
            symbol: "points".into(),
        },
    );
    assert_eq!(usage["id"], 0);
    assert_eq!(usage["symbol"], "points");
    assert_eq!(usage["snapshot"], v2);
    let rules = result_of(
        &mut service,
        &ToolRequest::Rules {
            snapshot: None,
            name: None,
            file: None,
            max: None,
            rule: None,
            content: false,

            all: true,
        },
    );
    assert_eq!(rules["snapshot"], v2);
    assert_eq!(rules["rules"].as_array().unwrap().len(), 2);
    let references = result_of(
        &mut service,
        &ToolRequest::References {
            snapshot: None,
            symbol: "points".into(),
            kind: None,
            rule: None,
            file: None,
            max: None,
            all: true,
        },
    );
    assert_eq!(references["snapshot"], v2);
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
    assert_eq!(findings["snapshot"], v2);
    assert!(
        findings["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["code"] == "min-wait-loop" && finding["rule"].is_number())
    );
    let lint = result_of(
        &mut service,
        &ToolRequest::Lint {
            selection: FindingSelection::default(),
            all: true,
        },
    );
    assert_eq!(lint["snapshot"], v2);
    result_of(&mut service, &ToolRequest::LintRules);
    let objects = result_of(&mut service, &ToolRequest::PersistentObjects { all: true });
    assert_eq!(objects["snapshot"], v2, "{objects}");
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
    assert_eq!(inspect["snapshot"], v2, "{inspect}");
    let analyze = result_of(
        &mut service,
        &ToolRequest::Analyze {
            selection: Default::default(),
            all: true,
        },
    );
    assert_eq!(analyze["snapshot"], v2, "{analyze}");
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

    let request = ToolRequest::ProviderValidateEdit {
        language_id: "not-a-language".to_string(),
        documents: Some(Default::default()),
        transaction: wright_driver::edit::EditTransaction { edits: vec![] },
        sources: None,
        project_root: None,
    };
    let result = result_of(&mut service, &request);
    assert_eq!(result["ok"], false, "{request:?}: {result}");
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
            snapshot: None,
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
            snapshot: Some(0),
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

    let ToolResponse::Ok { result } = service.handle(&ToolRequest::Check { all: true }) else {
        panic!("check returns the workflow envelope")
    };
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(result["diagnostics"][0]["code"], "input-io");

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
        symbols["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["name"] == "score"),
        "{symbols}"
    );
    assert!(service.loaded().is_some());
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
    let v1 = service
        .snapshot_generation()
        .expect("the initial load adopts");
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
            snapshot: None,
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
            snapshot: Some(v1),
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
                snapshot: Some(v1),
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
                snapshot: Some(v1),
                rule: 1.into(),
                kind: None,
                max: None,
                all: true,
            }
        ),
        "stale-id"
    );
    let symbols = result_of(
        &mut service,
        &ToolRequest::Symbols {
            kind: None,
            file: None,
            max: None,
            all: true,
        },
    );
    let v2 = symbols["snapshot"]
        .as_u64()
        .expect("the listing reports it");
    assert_ne!(v2, v1, "recovery adopts a new generation");
    let usage = result_of(
        &mut service,
        &ToolRequest::Usage {
            symbol: 0.into(),
            snapshot: Some(v2),
        },
    );
    assert_eq!(usage["symbol"], "points");
    result_of(
        &mut service,
        &ToolRequest::Cfg {
            snapshot: Some(v2),
            rule: 1.into(),
            kind: None,
            max: None,
            all: true,
        },
    );
    let _ = std::fs::remove_dir_all(&dir);
}

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
    let v1 = service
        .snapshot_generation()
        .expect("the initial load adopts");
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
            snapshot: Some(v1),
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
    let v2 = service.snapshot_generation().expect("recovery adopts");
    assert_ne!(v2, v1, "byte-identical recovery is still a new generation");

    assert_eq!(
        refusal_code(
            &mut service,
            &ToolRequest::References {
                snapshot: Some(v1),
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
        &ToolRequest::References {
            snapshot: Some(v2),
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

fn score_edit_transaction(key: &str, source_identity: String) -> EditTransaction {
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

    let validate = |sources| ToolRequest::ValidateEdit {
        sources,
        transaction: score_edit_transaction(&key, wright_driver::input_identity(&on_disk)),
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
fn edits_accept_percent_encoded_file_uri_sources() {
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
    let identity = wright_driver::input_identity(&on_disk);
    let encoded = result_of(
        &mut service,
        &ToolRequest::ValidateEdit {
            sources: Some(BTreeMap::from([(uri.clone(), on_disk.clone())])),
            transaction: score_edit_transaction(&uri, identity.clone()),
        },
    );
    assert_eq!(encoded["ok"], true, "{encoded:?}");
    let plain = result_of(
        &mut service,
        &ToolRequest::ValidateEdit {
            sources: Some(BTreeMap::from([(
                path.to_string_lossy().into_owned(),
                on_disk,
            )])),
            transaction: score_edit_transaction(&path.to_string_lossy(), identity),
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

    let result = result_of(
        &mut service,
        &ToolRequest::ValidateEdit {
            sources: Some(BTreeMap::from([(key.clone(), "changed".to_string())])),
            transaction: score_edit_transaction(&key, wright_driver::input_identity(&on_disk)),
        },
    );
    assert_eq!(result["ok"], false, "{result:?}");
    assert_eq!(
        result["diagnostics"][0]["code"], "edit-stale-source",
        "{result:?}"
    );
    assert!(result["preview"].is_null(), "no partial preview");

    let foreign = |sources| ToolRequest::ValidateEdit {
        sources,
        transaction: score_edit_transaction("other.ws", wright_driver::input_identity(&on_disk)),
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

    std::fs::write(&path, on_disk.replace("score", "other")).unwrap();
    let reloaded = std::fs::read_to_string(&path).unwrap();
    let validation = result_of(
        &mut service,
        &ToolRequest::ValidateEdit {
            sources: None,
            transaction: score_edit_transaction(&key, wright_driver::input_identity(&reloaded)),
        },
    );
    assert_eq!(validation["ok"], true, "{validation:?}");
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
    let generation = service.snapshot_generation().expect("loaded");
    let request = |rule: Option<Address>, max: Option<usize>| ToolRequest::Rules {
        snapshot: Some(generation),
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
        "unknown-rule"
    );
    assert_eq!(
        refusal_code(&mut service, &request(Some(Address::Id(99)), None)),
        "invalid-selection"
    );

    let plain = result_of(
        &mut service,
        &ToolRequest::Rules {
            snapshot: None,
            name: None,
            rule: None,
            file: None,
            max: None,
            content: false,
            all: true,
        },
    );
    assert!(plain["rules"][0].get("content").is_none());
    assert_eq!(plain["snapshot"], generation);
}

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
        snapshot: None,
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

#[test]
fn short_circuit_folds_preserve_occurrences_for_references() {
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
        result_of(
            &mut service,
            &ToolRequest::References {
                snapshot: None,
                symbol: "g".into(),
                kind: None,
                rule: None,
                file: None,
                max: None,
                all: true,
            },
        )
    };

    let off_references = query(wright_driver::Profile::Off);
    let compat_references = query(wright_driver::Profile::Compat);
    assert_eq!(
        compat_references, off_references,
        "references see the same occurrences under both profiles"
    );
    assert!(
        off_references["references"]
            .as_array()
            .expect("reference list")
            .iter()
            .any(|reference| reference["kind"] == "read"),
        "the Or-folded operand keeps its authored read: {off_references:?}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[path = "service/compile.rs"]
mod compile;
