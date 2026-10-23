use std::sync::Arc;

use serde_json::Value;
use workshop_rs::catalog::Locale;
use workshop_rs::parser;
use workshop_rs::{Event, Program, Rule};
use wright_analyzer::registry::{LintConfig, LintRegistry};
use wright_analyzer::service::{Origin, SemanticService};

fn service_for(program: Program) -> SemanticService<'static> {
    SemanticService::with_shared_program(
        Arc::new(program),
        Origin {
            kind: "unknown".to_string(),
            locale: None,
        },
        LintConfig::default(),
        Arc::new(LintRegistry::default()),
    )
}

fn workshop_service_from_text(text: &str) -> SemanticService<'static> {
    let catalog = workshop_rs::catalog::Catalog::builtin_ref().unwrap();
    let program = parser::parse_with_context(text, catalog, &Locale::new("en-US"), catalog)
        .unwrap_or_else(|error| panic!("Workshop source must parse: {error}"));
    SemanticService::with_shared_program(
        Arc::new(program),
        Origin {
            kind: "workshop".to_string(),
            locale: Some(Locale::new("en-US").to_string()),
        },
        LintConfig::default(),
        Arc::new(LintRegistry::default()),
    )
}

fn rules_service(names: &[&str]) -> SemanticService<'static> {
    let mut program = Program::new();
    for name in names {
        program.rule(Rule::new(*name, Event::Global));
    }
    service_for(program)
}

#[test]
fn rule_addresses_resolve_names_ordinals_and_literals() {
    let service = rules_service(&["a", "b", "a"]);
    assert_eq!(service.resolve_rule("a#1").unwrap(), 0);
    assert_eq!(service.resolve_rule("a#2").unwrap(), 2);
    assert_eq!(service.resolve_rule("b").unwrap(), 1);
    assert_eq!(
        service.resolve_rule("a#3").unwrap_err().code,
        "unknown-rule"
    );
    assert_eq!(
        service.resolve_rule("a").unwrap_err().code,
        "ambiguous-rule"
    );

    let service = rules_service(&["a#2"]);
    assert_eq!(service.resolve_rule("a#2").unwrap(), 0);

    let service = rules_service(&["a", "a", "a#2"]);
    let error = service.resolve_rule("a#2").unwrap_err();
    assert_eq!(error.code, "ambiguous-rule");
    let addresses: Vec<&str> = error.fields["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|candidate| candidate["address"].as_str().unwrap())
        .collect();
    assert!(
        addresses.contains(&"a#2") && addresses.contains(&"a#2#1"),
        "{error:?}"
    );
    assert_eq!(service.resolve_rule("a#2#1").unwrap(), 2);
    assert_eq!(service.resolve_rule("a#1").unwrap(), 0);

    let service = rules_service(&["", "x", ""]);
    assert_eq!(service.resolve_rule("#1").unwrap(), 0);
    assert_eq!(service.resolve_rule("#2").unwrap(), 2);
    assert_eq!(service.resolve_rule("x").unwrap(), 1);
}

#[test]
fn canonical_rule_addresses_round_trip_through_the_grammar() {
    let service = rules_service(&["a", "a", "a#2", "", "solo"]);
    let rules: Value = serde_json::from_str(&service.handle_json(r#"{"op":"listRules"}"#)).unwrap();
    let rules = rules["result"].as_array().unwrap();
    let addresses: Vec<&str> = rules
        .iter()
        .map(|rule| rule["address"].as_str().unwrap())
        .collect();
    assert_eq!(addresses, ["a#1", "a#2", "a#2#1", "#1", "solo"]);
    for rule in rules {
        let address = rule["address"].as_str().unwrap();
        match service.resolve_rule(address) {
            Ok(resolved) => {
                assert_eq!(resolved, rule["id"].as_u64().unwrap() as usize, "{address}")
            }
            Err(error) => assert_eq!(error.code, "ambiguous-rule", "{address}: {error:?}"),
        }
    }
}

#[test]
fn symbol_addresses_exclude_rules_and_take_kinds() {
    let service = workshop_service_from_text(
        r#"
variables {
    global:
        0: score
}
rule ("player starts") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
}
"#,
    );
    let score = service.resolve_symbol("score").unwrap();
    assert_eq!(
        service.resolve_symbol("globalVariable:score").unwrap().id,
        score.id
    );
    assert_eq!(
        service.resolve_symbol("player starts").unwrap_err().code,
        "unknown-symbol"
    );
    let service = workshop_service_from_text(
        r#"
variables {
    global:
        0: dup
}
rule ("dup") {
    event {
        Ongoing - Global;
    }
}
rule ("dup") {
    event {
        Ongoing - Global;
    }
}
"#,
    );
    assert_eq!(service.resolve_symbol("dup").unwrap().name, "dup");
    assert_eq!(
        service.resolve_symbol("globalVariable:dup").unwrap().id,
        service.resolve_symbol("dup").unwrap().id
    );
}

#[test]
fn symbol_addresses_disambiguate_kinds_across_collisions() {
    let service = workshop_service_from_text(
        r#"
variables {
    global:
        0: x
}
subroutines {
    0: x
}
rule ("uses x") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(x, 1);
    }
}
"#,
    );
    let error = service.resolve_symbol("x").unwrap_err();
    assert_eq!(error.code, "ambiguous-symbol");
    let addresses: Vec<&str> = error.fields["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|candidate| candidate["address"].as_str().unwrap())
        .collect();
    assert_eq!(addresses, ["globalVariable:x", "subroutine:x"], "{error:?}");
    assert_eq!(
        service.resolve_symbol("globalVariable:x").unwrap().name,
        "x"
    );
    assert_eq!(service.resolve_symbol("subroutine:x").unwrap().name, "x");
}
