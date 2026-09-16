use wright_analyzer::declarative::RuleDefinition;
use wright_analyzer::registry::LintConfig;

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
fn invalid_rule_yaml_remains_an_explicit_error() {
    let error = RuleDefinition::from_yaml_str("id: community/broken\nmetadata: []\nmatcher: {}\n")
        .expect_err("invalid rule YAML must be rejected");

    assert!(error.to_string().starts_with("invalid rule YAML:"));
}
