//! Native parser tests (#32): the en-US corpus Workshop text parses directly
//! into validated, locale-independent WIR, and diagnostics distinguish
//! malformed, unknown, and unsupported input.

use std::path::{Path, PathBuf};

use wright_workshop::catalog::{Catalog, Locale};
use wright_workshop::parser;
use wright_workshop::validate;
use wright_workshop::wir;

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
    // The corpus Workshop text parses directly against the catalog context,
    // which pins the expected enum domains from the canonical catalog
    // signatures (e.g. Create HUD Text's Reevaluation argument is `HudReeval`,
    // Create Beam Effect's is `EffectReeval`), resolving bare members that
    // are ambiguous across the catalog's enum domains (e.g.
    // `Visible To and String`).
    //
    // One documented exception: `real-world/overpy-cake`'s bare `Up` (OverPy
    // folds the vector-up constant into the bare member inside `Add(...)`) is
    // genuinely ambiguous between the `Vector` and `Rounding` enum domains and
    // no enclosing signature pins it, so the parser rejects it deterministically
    // rather than guessing (#111). The assertion below pins that this is the
    // ONLY way the fixture fails.
    let documented_ambiguities = [("real-world/overpy-cake", "ambiguous enum member 'Up'")];
    for fixture_id in CORPUS_FIXTURES {
        let text = corpus_workshop_text(fixture_id);
        let catalog = catalog();
        match parser::parse_with_context(&text, &catalog, &Locale::new("en-US"), &catalog) {
            Ok(program) => {
                program
                    .validate()
                    .unwrap_or_else(|error| panic!("{fixture_id} WIR must validate: {error}"));
                validate::validate_canonical_ids(&program, &catalog).unwrap_or_else(|error| {
                    panic!("{fixture_id} canonical ids must resolve: {error}")
                });
                assert!(!program.rules.is_empty(), "{fixture_id} must produce rules");
                assert!(
                    !program.dump().is_empty(),
                    "{fixture_id} dump must not be empty"
                );
            }
            Err(error) => {
                let Some((_, message)) = documented_ambiguities
                    .iter()
                    .find(|(id, _)| *id == *fixture_id)
                else {
                    panic!("{fixture_id} must parse:\n{error}");
                };
                assert!(
                    error.to_string().contains(message),
                    "{fixture_id} fails only with the documented ambiguity, got: {error}"
                );
            }
        }
    }
}

#[test]
fn parsing_is_deterministic() {
    let text = corpus_workshop_text("synthetic/control-flow");
    let catalog = catalog();
    let first =
        parser::parse_with_context(&text, &catalog, &Locale::new("en-US"), &catalog).unwrap();
    let second =
        parser::parse_with_context(&text, &catalog, &Locale::new("en-US"), &catalog).unwrap();
    assert_eq!(first.dump(), second.dump());
}

#[test]
fn parsed_variables_and_subroutines_carry_indexes() {
    let catalog = catalog();
    let program = parser::parse_with_context(
        &corpus_workshop_text("synthetic/declarations-rules"),
        &catalog,
        &Locale::new("en-US"),
        &catalog,
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
    let catalog = catalog();
    let program = parser::parse_with_context(
        &corpus_workshop_text("synthetic/declarations-rules"),
        &catalog,
        &Locale::new("en-US"),
        &catalog,
    )
    .unwrap();
    let events: Vec<String> = program
        .rules
        .iter()
        .map(|rule| match &rule.event {
            wir::Event::Global => "global".to_string(),
            wir::Event::EachPlayer | wir::Event::EachPlayerWithFilters { .. } => {
                "eachPlayer".to_string()
            }
            wir::Event::Player { kind, .. } => kind.catalog_id().to_string(),
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
    let catalog = catalog();
    let program = parser::parse_with_context(
        &corpus_workshop_text("synthetic/declarations-rules"),
        &catalog,
        &Locale::new("en-US"),
        &catalog,
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
    // A rule-final If without `End;` is the oracle's valid spelling (#87);
    // an If whose body never closes at all stays malformed.
    let text = "rule (\"broken\") { actions { If(True);";
    let error = parser::parse(text, &catalog(), &Locale::new("en-US")).unwrap_err();
    assert!(
        matches!(error, wright_workshop::WorkshopError::Malformed { .. }),
        "an unclosed If body is malformed: {error}"
    );
    assert!(error.to_string().contains("malformed"));
}

#[test]
fn rule_final_if_without_end_is_the_oracle_spelling() {
    let text = "rule (\"ok\") { actions { If(True); } }";
    let program = parser::parse(text, &catalog(), &Locale::new("en-US"))
        .expect("a rule-final If without End; is valid (oracle spelling, #87)");
    assert_eq!(program.rules.len(), 1);
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
fn bare_chase_reevaluation_none_is_ambiguous_across_domains() {
    // #105: both reference reevaluation domains spell their NONE member
    // "None". The flat Workshop parser cannot disambiguate the bare spelling,
    // so it fails with a structured Unsupported diagnostic (documented
    // round-trip boundary in docs/workshop/support-matrix.md; the emitted
    // semantic value is reference-equivalent).
    let text = "variables { global: 0: g }\nrule (\"x\") { event { Ongoing - Global; } actions { Set Global Variable(g, None); } }";
    let error = parser::parse(text, &catalog(), &Locale::new("en-US")).unwrap_err();
    assert!(
        matches!(error, wright_workshop::WorkshopError::Unsupported { .. }),
        "the shared None member spelling must be a structured ambiguity: {error}"
    );
    assert!(error.to_string().contains("ambiguous enum member 'None'"));
}

#[test]
fn explicit_locale_is_honored() {
    // en-US parsing is deterministic; the parser never guesses a locale.
    let text = corpus_workshop_text("synthetic/basic-rule");
    let program = parser::parse(&text, &catalog(), &Locale::new("en-US")).unwrap();
    let dump = program.dump();
    assert!(!dump.is_empty());
}

/// The canonical signature context from the #109 manifest, as the shipped
/// driver wires it into the Workshop parse path (#111).
fn manifest_context() -> &'static dyn wright_core::signatures::ExpectedDomain {
    wright_opy::manifest::Manifest::builtin().expect("builtin manifest")
}

/// The last argument value of the first call action of a parsed program.
fn enum_value_of_first_action(program: &wir::Program, action_index: usize) -> &wir::Value {
    let action = program.actions.iter().nth(action_index).expect("action");
    let wir::Action::Call { args, .. } = action else {
        panic!("expected a call action, got {action:?}");
    };
    let last = args.last().expect("call has an argument");
    let wir::ValueNode { value, .. } = program.values.get(*last).expect("value");
    value
}

#[test]
fn context_pinned_ambiguous_none_resolves_via_canonical_signature() {
    // #111: emitter-produced `Chase Global Variable Over Time(..., None)`
    // reparses to ChaseTimeReeval.NONE because the canonical chaseOverTime
    // signature pins argument 3 to the ChaseTimeReeval domain.
    let text = "variables { global: 0: g }\nrule (\"x\") { event { Ongoing - Global; } actions { Chase Global Variable Over Time(Global.g, 0, 30, None); } }";
    let program =
        parser::parse_with_context(text, &catalog(), &Locale::new("en-US"), manifest_context())
            .expect("the pinned Chase None must resolve");
    let value = enum_value_of_first_action(&program, 0);
    assert!(
        matches!(value, wir::Value::Enum { value_type, value }
            if value_type == "ChaseTimeReeval" && value == "NONE"),
        "the bare None resolves to ChaseTimeReeval.NONE, got {value:?}"
    );
}

#[test]
fn context_pinned_ambiguous_none_resolves_for_set_invisible() {
    // #111: `Set Invisible(Event Player, None)` reparses to Invis.NONE. The
    // manifest's setInvisibility is a member action, so Workshop text places
    // the receiver as argument 0 and the signature-pinned parameter at
    // argument 1.
    let text = "rule (\"x\") { event { Ongoing - Each Player; } actions { Set Invisible(Event Player, None); } }";
    let program =
        parser::parse_with_context(text, &catalog(), &Locale::new("en-US"), manifest_context())
            .expect("the pinned Invis None must resolve");
    let value = enum_value_of_first_action(&program, 0);
    assert!(
        matches!(value, wir::Value::Enum { value_type, value }
            if value_type == "Invis" && value == "NONE"),
        "the bare None resolves to Invis.NONE, got {value:?}"
    );
}

#[test]
fn wrong_domain_context_keeps_the_ambiguity_rejected() {
    // A signature pinning a *different* domain than the ambiguous member's
    // candidates must not resolve it: `Wait(...)` expects `Wait` (which has
    // no `None` member), so the bare `None` stays ambiguous — no guessing,
    // no arbitrary precedence.
    let text = "rule (\"x\") { event { Ongoing - Global; } actions { Wait(0.016, None); } }";
    let error =
        parser::parse_with_context(text, &catalog(), &Locale::new("en-US"), manifest_context())
            .expect_err("a non-matching expected domain must keep the ambiguity");
    assert!(
        matches!(error, wright_workshop::WorkshopError::Unsupported { .. }),
        "expected a structured ambiguity: {error}"
    );
    assert!(error.to_string().contains("ambiguous enum member 'None'"));
}

#[test]
fn expected_domain_resolution_tracks_the_manifest_declared_domains() {
    // Behavioral check that resolution consumes the #109 manifest as the
    // single source of expected domains: the adapter answers exactly the
    // manifest's declared parameter domains, including the receiver-offset
    // rule for member-kind functions.
    let manifest = wright_opy::manifest::Manifest::builtin().expect("builtin manifest");
    use wright_core::signatures::ExpectedDomain;
    // chaseOverTime: params [variable, destination, duration, reevaluation].
    assert_eq!(
        manifest.expected_domain("chaseOverTime", 3),
        Some("ChaseTimeReeval")
    );
    assert_eq!(manifest.expected_domain("chaseOverTime", 2), None);
    // setInvisibility: member action; Workshop arg 1 is the pinned param.
    assert_eq!(manifest.expected_domain("setInvisibility", 0), None);
    assert_eq!(
        manifest.expected_domain("setInvisibility", 1),
        Some("Invis")
    );
    // Unknown catalog ids and out-of-range indexes answer None.
    assert_eq!(manifest.expected_domain("noSuchAction", 0), None);
    assert_eq!(manifest.expected_domain("chaseOverTime", 4), None);
}

#[test]
fn cross_domain_member_spelling_collisions_are_the_documented_inventory() {
    // Systematic collision check (#111): scan the declared catalog for member
    // spellings shared by more than one enum domain (en-US) and assert the
    // inventory is exactly the documented one. A new collision fails this
    // test, forcing an explicit resolution decision for it.
    use std::collections::BTreeMap;
    let mut spelling_to_domains: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for domain in catalog().enum_domains() {
        for member in &domain.members {
            let spelling = member
                .spelling(&Locale::new("en-US"))
                .expect("en-US member spelling")
                .to_string();
            spelling_to_domains
                .entry(spelling)
                .or_default()
                .push(domain.domain.clone());
        }
    }
    let collisions: Vec<(String, Vec<String>)> = spelling_to_domains
        .into_iter()
        .filter(|(_, domains)| domains.len() > 1)
        .collect();
    assert_eq!(
        collisions,
        vec![
            (
                "All".to_string(),
                vec![
                    "EventTeam".to_string(),
                    "EventPlayer".to_string(),
                    "Invis".to_string()
                ]
            ),
            (
                "None".to_string(),
                vec![
                    "FacingReeval".to_string(),
                    "ChaseTimeReeval".to_string(),
                    "ChaseRateReeval".to_string(),
                    "Invis".to_string(),
                    "ThrottleReeval".to_string(),
                    "EffectReeval".to_string()
                ]
            ),
            (
                "Team 1".to_string(),
                vec![
                    "Color".to_string(),
                    "Team".to_string(),
                    "EventTeam".to_string()
                ]
            ),
            (
                "Team 2".to_string(),
                vec![
                    "Color".to_string(),
                    "Team".to_string(),
                    "EventTeam".to_string()
                ]
            ),
            (
                "Up".to_string(),
                vec!["Vector".to_string(), "Rounding".to_string()]
            ),
            (
                "Visible To".to_string(),
                vec![
                    "HudReeval".to_string(),
                    "EffectReeval".to_string(),
                    "InworldTextReeval".to_string()
                ]
            ),
            (
                "Visible To String and Color".to_string(),
                vec!["HudReeval".to_string(), "InworldTextReeval".to_string()]
            ),
            (
                "Visible To and Color".to_string(),
                vec![
                    "HudReeval".to_string(),
                    "EffectReeval".to_string(),
                    "InworldTextReeval".to_string()
                ]
            ),
            (
                "Visible To and String".to_string(),
                vec!["HudReeval".to_string(), "InworldTextReeval".to_string()]
            ),
        ],
        "the declared catalog's cross-domain member-spelling collisions"
    );
}
