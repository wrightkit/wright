//! Native parser tests (#32): the en-US corpus Workshop text parses directly
//! into validated, locale-independent WIR, and diagnostics distinguish
//! malformed, unknown, and unsupported input.

use std::path::{Path, PathBuf};

use wright_ir::wir;
use wright_workshop::catalog::{Catalog, Locale};
use wright_workshop::parser;
use wright_workshop::validate;

fn fixture_oracle_path(fixture_id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures")
        .join(fixture_id)
        .join("oracle.json")
}

fn corpus_workshop_text(fixture_id: &str) -> String {
    let oracle = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(fixture_oracle_path(fixture_id))
            .unwrap_or_else(|error| panic!("cannot read oracle for {fixture_id}: {error}")),
    )
    .expect("oracle json");
    oracle["compile"]["workshop"]
        .as_str()
        .expect("workshop text")
        .to_string()
}

fn catalog() -> Catalog {
    Catalog::builtin().expect("built-in catalog")
}

const CORPUS_FIXTURES: &[&str] = &[
    "synthetic/basic-rule",
    "synthetic/control-flow",
    "synthetic/declarations-rules",
    "synthetic/expressions-values",
    "synthetic/preprocessing",
    "real-world/overpy-cake",
];

#[test]
fn every_corpus_workshop_text_parses_to_valid_wir() {
    for fixture_id in CORPUS_FIXTURES {
        let text = corpus_workshop_text(fixture_id);
        let program = parser::parse(&text, &catalog(), &Locale::new("en-US"))
            .unwrap_or_else(|error| panic!("{fixture_id} must parse:\n{error}"));
        program
            .validate()
            .unwrap_or_else(|error| panic!("{fixture_id} WIR must validate: {error}"));
        validate::validate_canonical_ids(&program, &catalog())
            .unwrap_or_else(|error| panic!("{fixture_id} canonical ids must resolve: {error}"));
        assert!(!program.rules.is_empty(), "{fixture_id} must produce rules");
        assert!(
            !program.dump().is_empty(),
            "{fixture_id} dump must not be empty"
        );
    }
}

#[test]
fn parsing_is_deterministic() {
    let text = corpus_workshop_text("synthetic/control-flow");
    let first = parser::parse(&text, &catalog(), &Locale::new("en-US")).unwrap();
    let second = parser::parse(&text, &catalog(), &Locale::new("en-US")).unwrap();
    assert_eq!(first.dump(), second.dump());
}

#[test]
fn parsed_variables_and_subroutines_carry_indexes() {
    let program = parser::parse(
        &corpus_workshop_text("synthetic/declarations-rules"),
        &catalog(),
        &Locale::new("en-US"),
    )
    .unwrap();
    let globals: Vec<_> = program
        .global_variables
        .iter()
        .map(|variable| (variable.name.as_str(), variable.index))
        .collect();
    assert_eq!(globals, vec![("score", 0)]);
    let players: Vec<_> = program
        .player_variables
        .iter()
        .map(|variable| (variable.name.as_str(), variable.index))
        .collect();
    assert_eq!(players, vec![("hasStarted", 0)]);
    let subroutines: Vec<_> = program
        .subroutines
        .iter()
        .map(|subroutine| (subroutine.name.as_str(), subroutine.index))
        .collect();
    assert_eq!(subroutines, vec![("showStatus", 0)]);
}

#[test]
fn parsed_events_are_canonical() {
    let program = parser::parse(
        &corpus_workshop_text("synthetic/declarations-rules"),
        &catalog(),
        &Locale::new("en-US"),
    )
    .unwrap();
    let events: Vec<String> = program
        .rules
        .iter()
        .map(|rule| match &rule.event {
            wir::Event::Global => "global".to_string(),
            wir::Event::EachPlayer => "eachPlayer".to_string(),
            wir::Event::Subroutine(subroutine) => format!(
                "subroutine:{}",
                program.subroutines.get(*subroutine).unwrap().name
            ),
        })
        .collect();
    assert_eq!(
        events,
        vec![
            "subroutine:showStatus".to_string(),
            "eachPlayer".to_string()
        ]
    );
}

#[test]
fn parsed_conditions_resolve_infix_operators() {
    let program = parser::parse(
        &corpus_workshop_text("synthetic/declarations-rules"),
        &catalog(),
        &Locale::new("en-US"),
    )
    .unwrap();
    let rule = program
        .rules
        .iter()
        .find(|rule| rule.name == "player starts")
        .expect("rule");
    assert_eq!(rule.conditions.len(), 1, "one condition");
    // The condition is `==(hasSpawned(eventPlayer), true)`.
    let condition = program.values.get(rule.conditions[0]).unwrap();
    match &condition.value {
        wir::Value::Call { name, args } => {
            assert_eq!(name, "==");
            assert_eq!(args.len(), 2);
        }
        other => panic!("expected a comparison call, got {other:?}"),
    }
}

#[test]
fn spans_are_preserved() {
    let text = corpus_workshop_text("synthetic/basic-rule");
    let program = parser::parse(&text, &catalog(), &Locale::new("en-US")).unwrap();
    let rule = program.rules.iter().next().unwrap();
    let rule_span = rule.span.expect("rule span");
    assert_eq!(rule.name, "setup");
    assert!(rule_span.start.line >= 1);
    // The disable-inspector action carries its own span.
    let action = program.actions.get(rule.actions[0]).expect("action");
    assert!(action.span().is_some());
}

#[test]
fn malformed_input_is_reported_as_malformed() {
    let text = "rule (\"broken\") { actions { If(True); } }";
    let error = parser::parse(text, &catalog(), &Locale::new("en-US")).unwrap_err();
    assert!(
        matches!(error, wright_workshop::WorkshopError::Malformed { .. }),
        "If without End is malformed: {error}"
    );
    assert!(error.to_string().contains("malformed"));
}

#[test]
fn unknown_spelling_is_reported_as_unknown() {
    let text = "rule (\"x\") { event { Ongoing - Global; } actions { Totally Unknown Thing(1); } }";
    let error = parser::parse(text, &catalog(), &Locale::new("en-US")).unwrap_err();
    assert!(
        matches!(error, wright_workshop::WorkshopError::Unknown { .. }),
        "unknown action must be Unknown: {error}"
    );
    assert!(error.to_string().contains("Totally Unknown Thing"));
}

#[test]
fn unsupported_construct_is_distinct_from_malformed() {
    // A non-default eachPlayer sub-parameter is recognized but unsupported.
    let text = "rule (\"x\") { event { Ongoing - Each Player; Team 1; } actions { } }";
    let error = parser::parse(text, &catalog(), &Locale::new("en-US")).unwrap_err();
    assert!(
        matches!(error, wright_workshop::WorkshopError::Unsupported { .. }),
        "non-default event parameter must be Unsupported: {error}"
    );
}

#[test]
fn explicit_locale_is_honored() {
    // en-US parsing is deterministic; the parser never guesses a locale.
    let text = corpus_workshop_text("synthetic/basic-rule");
    let program = parser::parse(&text, &catalog(), &Locale::new("en-US")).unwrap();
    let dump = program.dump();
    assert!(!dump.is_empty());
}
