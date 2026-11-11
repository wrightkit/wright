use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::{Action, Event, Program, Rule, Value, parser};
use wright_analyzer::analysis::{EvidenceClass, Severity};
use wright_analyzer::canonical::SemanticService;
use wright_analyzer::declarative::{DeclarativeRule, RuleDefinition};
use wright_analyzer::registry::{LintConfig, LintRegistry, RuleRegistryError};

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
    let catalog = workshop_rs::catalog::Catalog::builtin_ref().unwrap();
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
    let rule = DeclarativeRule::from_definition(definition, catalog)
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
    assert!(DeclarativeRule::from_definition(reserved, catalog).is_err());
}

#[test]
fn declarative_scopes_match_all_requested_public_control_flow_regions() {
    let catalog = workshop_rs::catalog::Catalog::builtin_ref().unwrap();
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
        scoped_call_rule(catalog, "community/while", "while", false).run(&program, 0, None, None);
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
        scoped_call_rule(catalog, "community/if", "if", true).run(&program, 0, None, None);
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
        scoped_call_rule(catalog, "community/rule", "rule", false).run(&program, 0, None, None);
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
fn lint_config_packages_round_trip_and_reject_unknown_fields() {
    let config = LintConfig::from_yaml_str(
        r#"
packages:
  - repo: community/example-rules
    sha: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  - repo: https://github.com/wrightkit/lint-rules
    sha: bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
rules:
  community/minimum-wait:
    severity: error
"#,
    )
    .expect("lint config YAML parses");

    assert_eq!(config.packages.len(), 2);
    assert_eq!(config.packages[0].repo, "community/example-rules");
    assert_eq!(config.packages[0].sha, "a".repeat(40));
    assert_eq!(
        config.packages[1].repo,
        "https://github.com/wrightkit/lint-rules"
    );

    let serialized = yaml_serde::to_string(&config).expect("lint config serializes");
    let round_tripped =
        LintConfig::from_yaml_str(&serialized).expect("serialized lint config parses");
    assert_eq!(round_tripped, config);

    assert!(
        LintConfig::from_yaml_str(
            "packages:\n  - repo: community/x\n    sha: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n    ref: main\n"
        )
        .is_err()
    );
    assert!(LintConfig::from_yaml_str("lock-file: yes\n").is_err());
}

#[test]
fn first_party_rule_loading_replaces_the_matching_native_id() {
    let mut registry = LintRegistry::default();
    registry
        .load_first_party_yaml_str(
            r#"
id: while-without-wait
metadata:
  summary: summary
  rationale: rationale
  documentation: documentation
  known-limits: limits
  tags: []
matcher:
  scope: while
  actions:
    - kind: call
"#,
        )
        .unwrap();

    let descriptors = registry.descriptors(&LintConfig::default());
    let entries: Vec<_> = descriptors
        .iter()
        .filter(|d| d.id == "while-without-wait")
        .collect();
    assert_eq!(entries.len(), 1, "the ID has one owner: {entries:?}");
    assert_eq!(entries[0].kind, "declarative");

    let mut program = Program::new();
    program.rule(
        Rule::new("valid", Event::Global)
            .action(Action::While {
                condition: Value::Bool(true),
            })
            .action(Action::call("wait", [Value::Number(0.016)]))
            .action(Action::End),
    );
    let findings = registry.run(&program, &LintConfig::default());
    let hits: Vec<_> = findings
        .iter()
        .filter(|finding| finding.code == "while-without-wait")
        .collect();
    assert_eq!(hits.len(), 1, "the ID reports once: {findings:?}");
    assert_eq!(hits[0].evidence, EvidenceClass::Exact);
    assert!(hits[0].message.contains("matched"));

    assert!(matches!(
        registry.load_first_party_yaml_str(
            "id: while-without-wait\nmetadata:\n  summary: s\n  rationale: r\n  documentation: d\n  known-limits: k\n  tags: []\nmatcher: {}\n"
        ),
        Err(RuleRegistryError::DuplicateId(id)) if id == "while-without-wait"
    ));

    registry
        .load_first_party_yaml_str(
            "id: wright/example\nmetadata:\n  summary: s\n  rationale: r\n  documentation: d\n  known-limits: k\n  tags: []\nmatcher: {}\n",
        )
        .unwrap();
    assert!(
        registry
            .load_first_party_yaml_str(
                "id: community/x\nmetadata:\n  summary: s\n  rationale: r\n  documentation: d\n  known-limits: k\n  tags: []\nmatcher: {}\n"
            )
            .is_err()
    );
    assert!(
        registry
            .load_yaml_str(
                "id: bare-id\nmetadata:\n  summary: s\n  rationale: r\n  documentation: d\n  known-limits: k\n  tags: []\nmatcher: {}\n"
            )
            .is_err()
    );
    registry
        .load_yaml_str(
            "id: community/dup\nmetadata:\n  summary: s\n  rationale: r\n  documentation: d\n  known-limits: k\n  tags: []\nmatcher: {}\n",
        )
        .unwrap();
    assert!(matches!(
        registry.load_yaml_str(
            "id: community/dup\nmetadata:\n  summary: s\n  rationale: r\n  documentation: d\n  known-limits: k\n  tags: []\nmatcher: {}\n"
        ),
        Err(RuleRegistryError::DuplicateId(id)) if id == "community/dup"
    ));
}

const METADATA: &str = "metadata:\n  summary: summary\n  rationale: r\n  documentation: d\n  known-limits: k\n  tags: []\n";

/// The three loop scopes of #970's table, as a `scope` list.
const LOOP_SCOPES: &str = "[while, for-global-variable, for-player-variable]";

#[test]
fn scope_list_matches_each_listed_scope() {
    let catalog = Catalog::builtin_ref().unwrap();
    let mut program = Program::new();
    program.rule(
        Rule::new("loops", Event::Global)
            .action(Action::While {
                condition: Value::Bool(true),
            })
            .action(Action::call("wait", [Value::Number(1.0)]))
            .action(Action::End)
            .action(Action::ForGlobalVariable {
                variable: "index".into(),
                start: Value::Number(0.0),
                stop: Value::Number(2.0),
                step: Value::Number(1.0),
            })
            .action(Action::call("wait", [Value::Number(1.0)]))
            .action(Action::End)
            .action(Action::ForPlayerVariable {
                player: Value::EventPlayer,
                variable: "index".into(),
                start: Value::Number(0.0),
                stop: Value::Number(2.0),
                step: Value::Number(1.0),
            })
            .action(Action::call("wait", [Value::Number(1.0)]))
            .action(Action::End)
            .action(Action::If {
                condition: Value::Bool(true),
            })
            .action(Action::call("wait", [Value::Number(1.0)]))
            .action(Action::End),
    );
    let yaml = format!(
        "id: community/loops\n{METADATA}matcher:\n  scope: {LOOP_SCOPES}\n  actions:\n    - kind: call\n"
    );
    let rule =
        DeclarativeRule::from_definition(RuleDefinition::from_yaml_str(&yaml).unwrap(), catalog)
            .unwrap();
    let findings = rule.run(&program, 0, None, None);
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.action)
            .collect::<Vec<_>>(),
        [Some(0), Some(3), Some(6)],
        "one finding per listed loop kind, none for `if`: {findings:?}"
    );
    // Two scopes mixed in one list.
    let yaml = format!(
        "id: community/mixed\n{METADATA}matcher:\n  scope: [if, while]\n  actions:\n    - kind: call\n"
    );
    let rule =
        DeclarativeRule::from_definition(RuleDefinition::from_yaml_str(&yaml).unwrap(), catalog)
            .unwrap();
    let findings = rule.run(&program, 0, None, None);
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.action)
            .collect::<Vec<_>>(),
        [Some(0), Some(9)]
    );
    // An empty list rejects the rule rather than silently matching nothing.
    let yaml = format!("id: community/empty\n{METADATA}matcher:\n  scope: []\n");
    let result =
        DeclarativeRule::from_definition(RuleDefinition::from_yaml_str(&yaml).unwrap(), catalog);
    assert!(result.is_err(), "an empty scope list is a load error");
}

const EACH_MATCH_SOURCE: &str = r#"
variables {
    global:
        0: index
        1: other
}
rule ("each match") {
    event {
        Ongoing - Global;
    }
    conditions {
        Count Of(Filtered Array(All Players(Team 1), Is True For All(Array(Is Alive(Current Array Element)), Current Array Element))) > 0;
        disabled Count Of(Filtered Array(All Players(Team 2), Current Array Element)) > 0;
    }
    actions {
        Set Global Variable(other, Filtered Array(All Players(Team 1), Is True For Any(Array(Is Alive(Current Array Element)), Current Array Element)));
        disabled Set Global Variable(other, Is True For All(Array(True), Current Array Element));
    }
}
"#;

#[test]
fn values_with_each_match_reports_one_finding_per_node() {
    let catalog = Catalog::builtin_ref().unwrap();
    let program = parse_workshop(EACH_MATCH_SOURCE);
    // Two matches live in one value tree (condition 0), a third in a live
    // action argument, a fourth inside the disabled action — which must not
    // report.
    let yaml = format!(
        "id: community/binders\n{METADATA}matcher:\n  scope: rule\n  report: each-match\n  values:\n    - call:\n        names: [filteredArray, isTrueForAll, isTrueForAny, mappedArray, sortedArray]\n"
    );
    let rule =
        DeclarativeRule::from_definition(RuleDefinition::from_yaml_str(&yaml).unwrap(), catalog)
            .unwrap();
    let findings = rule.run(&program, 0, None, None);
    assert_eq!(
        findings.len(),
        4,
        "one finding per matched node: {findings:?}"
    );
    assert!(
        findings
            .iter()
            .all(|finding| finding.message.contains("matched 4 nodes")),
        "the declarative message carries the scope's matched count"
    );
    for finding in &findings {
        assert!(
            finding.span.is_some(),
            "each finding anchors at the node's own span: {finding:?}"
        );
    }
    // The first two are inside condition 0; the rest are action arguments.
    assert_eq!(findings[0].action, None);
    assert_eq!(findings[1].action, None);
    assert_eq!(findings[2].action, Some(0));
    assert_eq!(findings[3].action, Some(0));
    // Anchored at the node, not at the root: the two condition findings
    // point at different nodes.
    assert_ne!(findings[0].span, findings[1].span);
    assert_eq!(
        findings[0].span,
        program.condition_value_span(0, 0, &[0, 0]),
        "the outer filteredArray call anchors the first match"
    );
}

#[test]
fn within_in_args_limits_the_ancestor_argument_positions() {
    let catalog = Catalog::builtin_ref().unwrap();
    let source = r#"
variables {
    global:
        0: pool
}
rule ("within") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(pool, Filtered Array(All Players(Team 1), Is True For Any(Array(True), Current Array Element)));
        Set Global Variable(pool, Filtered Array(Sorted Array(All Players(Team 1), Current Array Element), Is Alive(Current Array Element)));
    }
}
"#;
    let program = parse_workshop(source);
    let binders = "names: [filteredArray, isTrueForAll, isTrueForAny, mappedArray, sortedArray]";
    let yaml = format!(
        "id: community/nested\n{METADATA}matcher:\n  scope: rule\n  report: each-match\n  values:\n    - call:\n        {binders}\n      within:\n        call:\n          {binders}\n        in-args:\n          from: 1\n"
    );
    let rule =
        DeclarativeRule::from_definition(RuleDefinition::from_yaml_str(&yaml).unwrap(), catalog)
            .unwrap();
    let findings = rule.run(&program, 0, None, None);
    assert_eq!(
        findings.len(),
        1,
        "only the binder inside a per-element argument reports: {findings:?}"
    );
    assert_eq!(findings[0].action, Some(0));
    let covered = span_text(source, &findings[0].span.unwrap());
    assert!(
        covered.contains("Is True For Any"),
        "the finding anchors at the inner binder: {covered}"
    );
}

#[test]
fn call_names_resolve_each_spelling_and_reject_unknown() {
    let catalog = Catalog::builtin_ref().unwrap();
    // Canonical ids and localized spellings resolve through the catalog.
    let yaml = format!(
        "id: community/names\n{METADATA}matcher:\n  scope: rule\n  report: each-match\n  values:\n    - call:\n        names: [filteredArray, Is True For All]\n"
    );
    let rule =
        DeclarativeRule::from_definition(RuleDefinition::from_yaml_str(&yaml).unwrap(), catalog)
            .unwrap();
    let program = parse_workshop(EACH_MATCH_SOURCE);
    let findings = rule.run(&program, 0, None, None);
    assert_eq!(
        findings.len(),
        3,
        "filteredArray + Is True For All match; isTrueForAny does not: {findings:?}"
    );
    // `name` and `names` are alternatives, not combinable.
    let both = format!(
        "id: community/both\n{METADATA}matcher:\n  values:\n    - call:\n        name: filteredArray\n        names: [sortedArray]\n"
    );
    assert!(
        DeclarativeRule::from_definition(RuleDefinition::from_yaml_str(&both).unwrap(), catalog)
            .is_err()
    );
    // An unknown or ambiguous spelling rejects the rule at load time.
    for names in ["[noSuchValueSpelling]", "[filteredArray, noSuchSpelling]"] {
        let yaml = format!(
            "id: community/unknown\n{METADATA}matcher:\n  values:\n    - call:\n        names: {names}\n"
        );
        let result = DeclarativeRule::from_definition(
            RuleDefinition::from_yaml_str(&yaml).unwrap(),
            catalog,
        );
        assert!(
            result.is_err(),
            "unknown spelling in `names` is a load error: {names}"
        );
    }
}

#[test]
fn first_party_metadata_sets_evidence_and_severity_external_rules_reject_it() {
    let catalog = Catalog::builtin_ref().unwrap();
    let mut registry = LintRegistry::default();
    registry
        .load_first_party_yaml_str(&format!(
            "id: wright/expensive\n{METADATA}  evidence: heuristic\n  severity: info\nmatcher:\n  scope: {LOOP_SCOPES}\n  report: each-match\n  values:\n    - call:\n        names: [distance, isInLoS]\n"
        ))
        .unwrap();
    let descriptors = registry.descriptors(&LintConfig::default());
    let descriptor = descriptors
        .iter()
        .find(|d| d.id == "wright/expensive")
        .unwrap();
    assert_eq!(descriptor.evidence, EvidenceClass::Heuristic);
    assert_eq!(descriptor.default_severity, Severity::Info);
    // A community package and a `--rule-file` rule may not declare either.
    for field in ["evidence: heuristic", "severity: info"] {
        let external = format!(
            "id: community/policy\nmetadata:\n  summary: s\n  rationale: r\n  documentation: d\n  known-limits: k\n  tags: []\n  {field}\nmatcher: {{}}\n"
        );
        assert!(
            registry.load_yaml_str(&external).is_err(),
            "community package must not declare {field}"
        );
        let result = DeclarativeRule::from_definition(
            RuleDefinition::from_yaml_str(&external).unwrap(),
            catalog,
        );
        assert!(result.is_err(), "rule-file metadata {field} rejected");
    }
}

fn parse_workshop(source: &str) -> Program {
    let catalog = Catalog::builtin_ref().unwrap();
    parser::parse_with_context(source, catalog, &Locale::new("en-US"), catalog)
        .unwrap_or_else(|error| panic!("Workshop source must parse: {error}"))
}

fn span_text(source: &str, span: &workshop_rs::source::Span) -> String {
    let (start, end) = (span.start, span.end);
    assert_eq!(
        start.line, end.line,
        "test spans stay on one line: {span:?}"
    );
    let line = source.lines().nth(start.line as usize - 1).unwrap();
    line.chars()
        .skip(start.col as usize - 1)
        .take((end.col - start.col) as usize)
        .collect()
}

#[test]
fn invalid_rule_yaml_remains_an_explicit_error() {
    let error = RuleDefinition::from_yaml_str("id: community/broken\nmetadata: []\nmatcher: {}\n")
        .expect_err("invalid rule YAML must be rejected");

    assert!(error.to_string().starts_with("invalid rule YAML:"));
}
