//! Canonical Workshop signature context (#111).
//!
//! These regressions protect the consumer path against ambiguous bare `None`
//! members. The expected-domain context is supplied by the canonical
//! `workshop-rs` catalog; the catalog data and context-free parser behavior
//! remain owned and tested by `workshop-rs`.

use std::path::Path;

use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::parser;
use workshop_rs::roundtrip;
use workshop_rs::signatures::ExpectedDomain;
use workshop_rs::wir;

fn catalog() -> Catalog {
    Catalog::builtin().unwrap()
}

fn en() -> Locale {
    Locale::new("en-US")
}

fn parse_with_catalog(text: &str) -> wir::Program {
    let catalog = catalog();
    parser::parse_with_context(text, &catalog, &en(), &catalog).expect("catalog context parses")
}

fn round_trip_with_catalog(text: &str) -> roundtrip::RoundTripRecord {
    let catalog = catalog();
    roundtrip::round_trip_with_context(text, &catalog, &en(), &catalog)
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
    let program = parse_with_catalog(text);
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
    // the catalog's setInvisibility is a member action, so Workshop text places
    // the receiver as argument 0 and the signature-pinned parameter at
    // argument 1.
    let text = "rule (\"x\") { event { Ongoing - Each Player; } actions { Set Invisible(Event Player, None); } }";
    let program = parse_with_catalog(text);
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
    let catalog = catalog();
    let error = parser::parse_with_context(text, &catalog, &en(), &catalog)
        .expect_err("a non-matching expected domain must keep the ambiguity");
    assert!(
        matches!(error, workshop_rs::WorkshopError::Unsupported { .. }),
        "expected a structured ambiguity: {error}"
    );
    assert!(error.to_string().contains("ambiguous enum member 'None'"));
}

#[test]
fn expected_domain_resolution_comes_from_the_canonical_catalog() {
    let catalog = catalog();
    // chaseOverTime: params [variable, destination, duration, reevaluation].
    assert_eq!(
        catalog.expected_domain("chaseOverTime", 3),
        Some("ChaseTimeReeval")
    );
    assert_eq!(catalog.expected_domain("chaseOverTime", 2), None);
    // setInvisibility: member action; Workshop arg 1 is the pinned param.
    assert_eq!(catalog.expected_domain("setInvisibility", 0), None);
    assert_eq!(catalog.expected_domain("setInvisibility", 1), Some("Invis"));
    // Unknown catalog ids and out-of-range indexes answer None.
    assert_eq!(catalog.expected_domain("noSuchAction", 0), None);
    assert_eq!(catalog.expected_domain("chaseOverTime", 4), None);
}

#[test]
fn emitter_chase_none_round_trips_through_the_shipped_path() {
    // #111: emitter-produced `Chase Global Variable Over Time(..., None)`
    // (bare `None` shared by ChaseTimeReeval/ChaseRateReeval/Invis) reparses
    // to ChaseTimeReeval.NONE through the shipped parse+emit path, and the
    // round-tripped WIR is equivalent to the input WIR.
    let text = "variables { global: 0: g }\nrule (\"chase\") { event { Ongoing - Global; } actions { Chase Global Variable Over Time(Global.g, 0, 30, None); } }";
    let record = round_trip_with_catalog(text);
    assert!(
        record.error.is_none(),
        "the pinned Chase None must round-trip: {:?}",
        record.error
    );
    assert!(record.parse_ok && record.emit_ok && record.reparse_ok && record.equivalent);
}

#[test]
fn emitter_set_invisible_none_round_trips_through_the_shipped_path() {
    // #111: `Set Invisible(Event Player, None)` reparses to Invis.NONE via
    // the member-function receiver offset and round-trips to equivalent WIR.
    let text = "rule (\"inv\") { event { Ongoing - Each Player; } actions { Set Invisible(Event Player, None); } }";
    let record = round_trip_with_catalog(text);
    assert!(
        record.error.is_none(),
        "the pinned Invis None must round-trip: {:?}",
        record.error
    );
    assert!(record.parse_ok && record.emit_ok && record.reparse_ok && record.equivalent);
}

#[test]
fn emitter_chase_at_rate_none_round_trips_through_the_shipped_path() {
    // #110: the chase rate form emits `Chase Global Variable At Rate(...,
    // None)`; the catalog id `chaseAtRate` selects the `ChaseRateReeval`
    // domain through the catalog's contextual-dispatch data, so the bare
    // `None` reparses and round-trips to equivalent WIR.
    let text = "variables { global: 0: g }\nrule (\"chase\") { event { Ongoing - Global; } actions { Chase Global Variable At Rate(Global.g, 10, 2, None); } }";
    let record = round_trip_with_catalog(text);
    assert!(
        record.error.is_none(),
        "the pinned ChaseRateReeval None must round-trip: {:?}",
        record.error
    );
    assert!(record.parse_ok && record.emit_ok && record.reparse_ok && record.equivalent);
    // The player form follows the same path through its own catalog id.
    let text = "variables { player: 0: P }\nrule (\"chase\") { event { Ongoing - Each Player; } actions { Chase Player Variable At Rate(Event Player, P, 0, 1, None); } }";
    let record = round_trip_with_catalog(text);
    assert!(
        record.error.is_none(),
        "the pinned player ChaseRateReeval None must round-trip: {:?}",
        record.error
    );
    assert!(record.parse_ok && record.emit_ok && record.reparse_ok && record.equivalent);
}

#[test]
fn chase_keyword_fixture_round_trips_through_the_shipped_path() {
    // The `synthetic/chase-keywords` surface (rate/duration forms, global
    // and player variables, keyword-bound wait/vect/len/print/
    // getPlayersInRadius/setStatusEffect) compiles through the native OPY
    // frontend, emits through the catalog, reparses with the catalog
    // signature context, and re-emits to a fixed point (#110). The oracle
    // text itself is not the input: the reference emits bare variable names
    // where the native Workshop parser's canonical spelling is `Global.g`
    // (documented N-level presentation difference), so the round-trip uses
    // the native emission.
    let source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../compatibility/fixtures/synthetic/chase-keywords/source.opy"),
    )
    .unwrap();
    let hir = wright_opy::compile(&source, "source.opy", Path::new(""))
        .expect("the fixture compiles natively");
    let model = wright_core::hir::convert::convert(&hir).expect("the HIR converts");
    let wir = wright_ir::lower::lower(&model).expect("the fixture lowers to WIR");
    let emitted = workshop_rs::emitter::emit(&wir, &catalog(), &en()).expect("the fixture emits");
    // The emission includes Debug/Print HUD text (canonical catalog layout)
    // and chase `None` members, so the catalog supplies
    // the expected enum domains.
    let record = round_trip_with_catalog(&emitted);
    assert!(
        record.error.is_none(),
        "the chase-keywords emission must round-trip: {:?}",
        record.error
    );
    assert!(record.parse_ok && record.emit_ok && record.reparse_ok && record.equivalent);
}

#[test]
fn context_free_chase_none_stays_a_documented_exception() {
    // Without a signature pin the ambiguity stays rejected: the same input
    // through the plain (context-free) round-trip fails at parse, keeping the
    // pre-#111 boundary deterministic.
    let text = "variables { global: 0: g }\nrule (\"chase\") { event { Ongoing - Global; } actions { Chase Global Variable Over Time(Global.g, 0, 30, None); } }";
    let record = roundtrip::round_trip(text, &catalog(), &en());
    assert!(!record.parse_ok, "context-free None must stay rejected");
    let error = record.error.expect("a parse failure is recorded");
    assert!(error.contains("ambiguous enum member 'None'"), "{error}");
}

#[test]
fn context_chase_none_emission_is_a_fixed_point() {
    // Parse the emitted form with context, emit, reparse with context, and
    // emit again: the text is a fixed point.
    let text = "variables { global: 0: g }\nrule (\"chase\") { event { Ongoing - Global; } actions { Chase Global Variable Over Time(Global.g, 0, 30, None); } }";
    let catalog = catalog();
    let first = parser::parse_with_context(text, &catalog, &en(), &catalog)
        .expect("pinned Chase None parses");
    let emitted = workshop_rs::emitter::emit(&first, &catalog, &en()).expect("emits");
    assert!(
        emitted.contains("Chase Global Variable Over Time(Global.g, 0, 30, None)"),
        "emission preserves the bare None spelling:\n{emitted}"
    );
    let reparsed = parser::parse_with_context(&emitted, &catalog, &en(), &catalog)
        .expect("emitted text reparses with context");
    let reemitted = workshop_rs::emitter::emit(&reparsed, &catalog, &en()).expect("re-emits");
    assert_eq!(emitted, reemitted, "emission must be a fixed point");
}
