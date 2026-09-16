use std::path::{Path, PathBuf};

use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::parser;
use wright_analyzer::analysis::Severity;
use wright_analyzer::declarative::{RuleDefinition, Scope};
use wright_analyzer::registry::{LintConfig, LintRegistry};

const RULE_YAML: &str = r#"
id: community/minimum-wait
metadata:
  summary: loop contains a minimum wait
  rationale: minimum waits can create high-frequency loops
  documentation: Finds a minimum wait inside a while scope.
  known-limits: This is structural and does not measure runtime cost.
  tags: [performance]
matcher:
  scope: while
  event: global
  actions:
    - kind: call
      name: Wait
      count:
        min: 1
"#;

fn workshop_path(fixture_id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures")
        .join(fixture_id)
        .join("workshop.ws")
}

fn workshop_program(fixture_id: &str) -> workshop_rs::wir::Program {
    let text = std::fs::read_to_string(workshop_path(fixture_id)).unwrap();
    let catalog = Catalog::builtin().unwrap();
    parser::parse_wir_with_context(&text, &catalog, &Locale::new("en-US"), &catalog).unwrap()
}

#[test]
fn rule_yaml_preserves_the_supported_definition_shape() {
    let definition = RuleDefinition::from_yaml_str(RULE_YAML).expect("rule YAML parses");

    assert_eq!(definition.id, "community/minimum-wait");
    assert_eq!(definition.locale, "en-US");
    assert!(matches!(definition.matcher.scope, Scope::While));
    assert_eq!(definition.matcher.event.as_deref(), Some("global"));
    assert_eq!(definition.matcher.actions.len(), 1);
    assert_eq!(definition.matcher.actions[0].name.as_deref(), Some("Wait"));
    assert_eq!(definition.matcher.actions[0].count.min, Some(1));
}

#[test]
fn rule_and_project_yaml_preserve_registry_behavior() {
    let mut registry = LintRegistry::default();
    registry
        .load_yaml_str(RULE_YAML)
        .expect("declarative rule loads");

    let config = LintConfig::from_yaml_str(
        r#"
rules:
  community/minimum-wait:
    enabled: true
    severity: error
    options:
      min-matches: 1
"#,
    )
    .expect("project lint YAML parses");

    assert!(config.is_enabled("community/minimum-wait"));
    assert_eq!(
        config.options("community/minimum-wait").min_matches,
        Some(1)
    );

    let finding = registry
        .run(&workshop_program("synthetic/control-flow"), &config)
        .into_iter()
        .find(|finding| finding.code == "community/minimum-wait")
        .expect("the external rule finds the nested Wait action");
    assert_eq!(finding.severity, Severity::Error);
    assert!(finding.span.is_some());
}

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
    .expect("project lint YAML parses");

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
fn invalid_yaml_remains_an_explicit_error() {
    let rule_error =
        RuleDefinition::from_yaml_str("id: community/broken\nmetadata: []\nmatcher: {}\n")
            .expect_err("invalid rule YAML must be rejected");
    assert!(rule_error.to_string().starts_with("invalid rule YAML:"));

    let config_error = LintConfig::from_yaml_str(
        "rules:\n  community/minimum-wait:\n    options:\n      unsupported: true\n",
    );
    assert!(
        config_error.is_err(),
        "unknown config options must be rejected"
    );
}
