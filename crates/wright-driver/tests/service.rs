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
