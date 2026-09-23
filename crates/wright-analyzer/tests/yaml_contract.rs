use workshop_rs::catalog::Catalog;
use workshop_rs::{Action, Event, Program, Rule, Value};
use wright_analyzer::canonical::SemanticService;
use wright_analyzer::declarative::{DeclarativeRule, RuleDefinition};
use wright_analyzer::registry::{LintConfig, LintRegistry};

#[test]
fn lint_config_yaml_round_trips_through_yaml_serialization() {
    let config = LintConfig::from_yaml_str(
        r#"
rules:
  community/minimum-wait:
    enabled: false
    severity: warn
    options:
      max-matches: 2
"#,
    )
    .expect("lint config YAML parses");

    let serialized = yaml_serde::to_string(&config).expect("lint config serializes");
    let round_tripped =
        LintConfig::from_yaml_str(&serialized).expect("serialized lint config parses");

    assert!(!round_tripped.is_enabled("community/minimum-wait"));
    assert_eq!(
        round_tripped.options("community/minimum-wait").max_matches,
        Some(2)
    );
}

#[test]
fn declarative_rule_ids_keep_the_namespace_slash_contract() {
    let catalog = Catalog::builtin().unwrap();
    let definition = RuleDefinition::from_yaml_str(
        r#"
id: Community_Name/Rule.Name
metadata:
  summary: summary
  rationale: rationale
  documentation: documentation
  known-limits: limits
  tags: []
matcher: {}
"#,
    )
    .unwrap();
    let rule = DeclarativeRule::from_definition(definition, &catalog)
        .expect("the existing namespace/id contract accepts non-kebab-case components");
    assert_eq!(rule.id(), "Community_Name/Rule.Name");

    let reserved = RuleDefinition::from_yaml_str(
        r#"
id: wright/example
metadata:
  summary: summary
  rationale: rationale
  documentation: documentation
  known-limits: limits
  tags: []
matcher: {}
"#,
    )
    .unwrap();
    assert!(DeclarativeRule::from_definition(reserved, &catalog).is_err());
}

#[test]
fn declarative_scopes_match_all_requested_public_control_flow_regions() {
    let catalog = Catalog::builtin().unwrap();
    let mut program = Program::new();
    program.rule(
        Rule::new("scopes", Event::Global)
            .action(Action::call("wait", []))
            .action(Action::ForGlobalVariable {
                variable: "index".into(),
                start: Value::Number(0.0),
                stop: Value::Number(2.0),
                step: Value::Number(1.0),
            })
            .action(Action::call("wait", []))
            .action(Action::End)
            .action(Action::While {
                condition: Value::Bool(true),
            })
            .action(Action::call("wait", []))
            .action(Action::If {
                condition: Value::Bool(true),
            })
            .action(Action::call("wait", []))
            .action(Action::End)
            .action(Action::End)
            .action(Action::While {
                condition: Value::Bool(false),
            })
            .action(Action::call("wait", []))
            .action(Action::End)
            .action(Action::If {
                condition: Value::Bool(true),
            })
            .action(Action::call("wait", []))
            .action(Action::ElseIf {
                condition: Value::Bool(false),
            })
            .action(Action::call("wait", []))
            .action(Action::Else)
            .action(Action::call("wait", []))
            .action(Action::End)
            .action(Action::call("wait", [])),
    );

    let while_findings =
        scoped_call_rule(&catalog, "community/while", "while", false).run(&program, 0, None, None);
    assert_eq!(
        while_findings
            .iter()
            .map(|finding| finding.action)
            .collect::<Vec<_>>(),
        [Some(4), Some(10)]
    );
    assert!(while_findings[0].message.contains("matched 2 nodes"));
    assert!(while_findings[1].message.contains("matched 1 node"));

    let if_findings =
        scoped_call_rule(&catalog, "community/if", "if", true).run(&program, 0, None, None);
    assert_eq!(
        if_findings
            .iter()
            .map(|finding| finding.action)
            .collect::<Vec<_>>(),
        [Some(6), Some(13)]
    );
    assert!(if_findings[0].message.contains("matched 1 node"));
    assert!(if_findings[1].message.contains("matched 3 nodes"));

    let rule_findings =
        scoped_call_rule(&catalog, "community/rule", "rule", false).run(&program, 0, None, None);
    assert_eq!(rule_findings.len(), 1);
    assert_eq!(rule_findings[0].action, None);
    assert!(rule_findings[0].message.contains("matched 9 nodes"));
}

fn scoped_call_rule(
    catalog: &Catalog,
    id: &str,
    scope: &str,
    match_true_condition: bool,
) -> DeclarativeRule {
    let conditions = if match_true_condition {
        "  conditions:\n    boolean: true\n    count:\n      min: 1\n"
    } else {
        ""
    };
    let yaml = format!(
        "id: {id}\nmetadata:\n  summary: summary\n  rationale: rationale\n  documentation: documentation\n  known-limits: limits\n  tags: []\nmatcher:\n  scope: {scope}\n{conditions}  actions:\n    - kind: call\n"
    );
    let definition = RuleDefinition::from_yaml_str(&yaml).unwrap();
    DeclarativeRule::from_definition(definition, catalog).unwrap()
}

#[test]
fn lint_registry_keeps_yaml_loading_and_skipped_rule_reports() {
    let mut registry = LintRegistry::default();
    registry
        .load_yaml_str(
            r#"
id: community/has-wait
metadata:
  summary: summary
  rationale: rationale
  documentation: documentation
  known-limits: limits
  tags: []
matcher:
  scope: rule
  actions:
    - kind: call
"#,
        )
        .unwrap();

    let mut program = Program::new();
    program.rule(
        Rule::new("valid", Event::Global)
            .action(Action::While {
                condition: Value::Bool(true),
            })
            .action(Action::call("wait", [Value::Number(0.016)]))
            .action(Action::End),
    );
    program.rule(
        Rule::new("unbalanced", Event::Global)
            .action(Action::While {
                condition: Value::Bool(true),
            })
            .action(Action::call("wait", [Value::Number(0.016)])),
    );

    let report = registry.run_report(&program, &LintConfig::default());
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == "community/has-wait" && finding.rule == 0)
    );
    assert!(
        report
            .skipped
            .iter()
            .any(|rule| rule.id == "community/has-wait" && rule.rule == 1)
    );
    assert!(
        report
            .skipped
            .iter()
            .any(|rule| rule.id == "min-wait-loop" && rule.rule == 1)
    );

    let service = SemanticService::new(&program);
    let response: serde_json::Value =
        serde_json::from_str(&service.handle_json(r#"{"op":"lintRules"}"#)).unwrap();
    assert!(
        response["result"]["skipped"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| { rule["id"] == "min-wait-loop" && rule["rule"] == 1 })
    );
}

#[test]
fn invalid_rule_yaml_remains_an_explicit_error() {
    let error = RuleDefinition::from_yaml_str("id: community/broken\nmetadata: []\nmatcher: {}\n")
        .expect_err("invalid rule YAML must be rejected");

    assert!(error.to_string().starts_with("invalid rule YAML:"));
}
