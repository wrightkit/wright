//! Cross-language round-trip suite (#35): every supported-locale fixture
//! round-trips with recorded evidence, equivalence ignores presentation-only
//! differences, and negative fixtures fail at the right stage.

use std::path::{Path, PathBuf};

use wright_workshop::catalog::{Catalog, Locale};
use wright_workshop::roundtrip::{self, RoundTripRecord};

fn oracle_path(fixture_id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures")
        .join(fixture_id)
        .join("oracle.json")
}

fn corpus_text(fixture_id: &str) -> String {
    let oracle = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(oracle_path(fixture_id)).unwrap(),
    )
    .unwrap();
    oracle["compile"]["workshop"].as_str().unwrap().to_string()
}

fn catalog() -> Catalog {
    Catalog::builtin().unwrap()
}

fn en() -> Locale {
    Locale::new("en-US")
}

#[test]
fn every_corpus_fixture_round_trips_with_full_evidence() {
    // Corpus text round-trips against the catalog context, which pins the
    // expected enum domains from the canonical catalog signatures.
    // `real-world/overpy-cake` is the documented exception: its bare `Up`
    // (OverPy folds the vector-up constant inside `Add(...)`) is genuinely
    // ambiguous between the `Vector` and `Rounding` enum domains and no
    // enclosing signature pins it, so the parser rejects it (#111).
    let documented_ambiguities = [("real-world/overpy-cake", "ambiguous enum member 'Up'")];
    for fixture_id in [
        "synthetic/basic-rule",
        "synthetic/control-flow",
        "synthetic/declarations-rules",
        "synthetic/expressions-values",
        "synthetic/preprocessing",
        "synthetic/receiver-calls",
        "real-world/overpy-cake",
    ] {
        let catalog = catalog();
        let record =
            roundtrip::round_trip_with_context(&corpus_text(fixture_id), &catalog, &en(), &catalog);
        if let Some(error) = &record.error {
            let Some((_, message)) = documented_ambiguities
                .iter()
                .find(|(id, _)| **id == *fixture_id)
            else {
                panic!("{fixture_id} must round-trip cleanly: {error}");
            };
            assert!(
                error.contains(message),
                "{fixture_id} fails only with the documented ambiguity, got: {error}"
            );
            continue;
        }
        assert!(record.parse_ok, "{fixture_id}");
        assert!(record.emit_ok, "{fixture_id}");
        assert!(record.reparse_ok, "{fixture_id}");
        assert!(record.equivalent, "{fixture_id} must be WIR-equivalent");
        assert_eq!(record.locale, en());
        assert_eq!(record.catalog_version, 1);
        assert_eq!(record.input_identity.len(), 64, "identity is a sha256 hex");
    }
}

#[test]
fn same_locale_round_trip_is_a_release_gate() {
    // The suite fails closed: any fixture failing round-trip equivalence
    // blocks the gate. The `real-world/overpy-cake` bare-`Up` ambiguity is
    // the single documented exception (#118): the catalog now documents both
    // the `Vector` and `Rounding` "Up" members, and no enclosing signature
    // pins the fixture's folded vector-up constant.
    let failures: Vec<String> = [
        "synthetic/basic-rule",
        "synthetic/control-flow",
        "synthetic/declarations-rules",
        "synthetic/expressions-values",
        "synthetic/preprocessing",
        "synthetic/receiver-calls",
        "real-world/overpy-cake",
    ]
    .iter()
    .filter(|fixture_id| **fixture_id != "real-world/overpy-cake")
    .map(|fixture_id| {
        let catalog = catalog();
        roundtrip::round_trip_with_context(&corpus_text(fixture_id), &catalog, &en(), &catalog)
    })
    .filter(|record: &RoundTripRecord| !record.equivalent || record.error.is_some())
    .map(|record| record.locale.to_string())
    .collect();
    assert!(
        failures.is_empty(),
        "round-trip gate failures: {failures:?}"
    );
}

#[test]
fn equivalence_ignores_presentation_but_preserves_semantics() {
    let a = wright_workshop::wir::Program::default();
    let b = wright_workshop::wir::Program::default();
    // Two empty programs are equivalent.
    assert!(roundtrip::equivalent(&a, &b));
    // Same semantics, different file ids in spans: still equivalent.
    let mut c = wright_workshop::wir::Program::default();
    c.files
        .push(wright_workshop::source::SourceFile::new("other.txt"));
    assert!(
        roundtrip::equivalent(&a, &c),
        "file paths are presentation-only"
    );
}

#[test]
fn equivalence_detects_semantic_differences() {
    let mut a = wright_workshop::wir::Program::default();
    a.files
        .push(wright_workshop::source::SourceFile::new("workshop.txt"));
    let mut b = a.clone();
    b.files
        .push(wright_workshop::source::SourceFile::new("workshop.txt"));

    let value_a = a.values.push(wright_workshop::wir::ValueNode::new(
        wright_workshop::wir::Value::Number {
            value: 1.0,
            text: "1".to_string(),
        },
        None,
    ));
    let value_b = b.values.push(wright_workshop::wir::ValueNode::new(
        wright_workshop::wir::Value::Number {
            value: 2.0,
            text: "2".to_string(),
        },
        None,
    ));
    a.actions.push(wright_workshop::wir::Action::Debug {
        value: value_a,
        span: None,
    });
    b.actions.push(wright_workshop::wir::Action::Debug {
        value: value_b,
        span: None,
    });
    a.rules.push(wright_workshop::wir::Rule {
        name: "r".into(),
        span: None,
        name_span: None,
        disabled: false,
        event: wright_workshop::wir::Event::Global,
        conditions: vec![],
        actions: a
            .actions
            .iter()
            .map(|_| wright_ir::ids::Id::from_index(0))
            .collect(),
    });
    b.rules.push(wright_workshop::wir::Rule {
        name: "r".into(),
        span: None,
        name_span: None,
        disabled: false,
        event: wright_workshop::wir::Event::Global,
        conditions: vec![],
        actions: b
            .actions
            .iter()
            .map(|_| wright_ir::ids::Id::from_index(0))
            .collect(),
    });
    assert!(
        !roundtrip::equivalent(&a, &b),
        "different values must not be equivalent"
    );
}

#[test]
fn malformed_input_is_recorded_not_crashed() {
    let record =
        roundtrip::round_trip("rule (\"broken\") { actions { If(True);", &catalog(), &en());
    assert!(!record.parse_ok);
    assert!(!record.equivalent);
    assert!(record.error.is_some(), "failure is recorded");
}

#[test]
fn unknown_builtin_fails_at_emit_stage() {
    // A program that parses but contains a non-catalog value fails emit.
    let mut program = wright_workshop::wir::Program::default();
    program
        .files
        .push(wright_workshop::source::SourceFile::new("workshop.txt"));
    let value = program.values.push(wright_workshop::wir::ValueNode::new(
        wright_workshop::wir::Value::Call {
            name: "notACatalogId".into(),
            args: vec![],
        },
        None,
    ));
    let call = program.actions.push(wright_workshop::wir::Action::Call {
        name: "wait".into(),
        args: vec![value],
        span: None,
    });
    program.rules.push(wright_workshop::wir::Rule {
        name: "x".into(),
        span: None,
        name_span: None,
        disabled: false,
        event: wright_workshop::wir::Event::Global,
        conditions: vec![],
        actions: vec![call],
    });
    // Equivalent to the round-trip emit stage: emission of unknown ids fails.
    let error =
        wright_workshop::emitter::emit(&program, &catalog(), &en()).expect_err("unknown id");
    assert!(error.to_string().contains("notACatalogId"));
}

/// The canonical signature context from the #109 manifest, as the shipped
/// driver wires it into the Workshop parse path (#111).
fn manifest_context() -> &'static dyn wright_core::signatures::ExpectedDomain {
    wright_opy::manifest::Manifest::builtin().expect("builtin manifest")
}

#[test]
fn emitter_chase_none_round_trips_through_the_shipped_path() {
    // #111: emitter-produced `Chase Global Variable Over Time(..., None)`
    // (bare `None` shared by ChaseTimeReeval/ChaseRateReeval/Invis) reparses
    // to ChaseTimeReeval.NONE through the shipped parse+emit path, and the
    // round-tripped WIR is equivalent to the input WIR.
    let text = "variables { global: 0: g }\nrule (\"chase\") { event { Ongoing - Global; } actions { Chase Global Variable Over Time(Global.g, 0, 30, None); } }";
    let record = roundtrip::round_trip_with_context(text, &catalog(), &en(), manifest_context());
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
    let record = roundtrip::round_trip_with_context(text, &catalog(), &en(), manifest_context());
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
    // domain through the manifest's contextual-dispatch data, so the bare
    // `None` reparses and round-trips to equivalent WIR.
    let text = "variables { global: 0: g }\nrule (\"chase\") { event { Ongoing - Global; } actions { Chase Global Variable At Rate(Global.g, 10, 2, None); } }";
    let record = roundtrip::round_trip_with_context(text, &catalog(), &en(), manifest_context());
    assert!(
        record.error.is_none(),
        "the pinned ChaseRateReeval None must round-trip: {:?}",
        record.error
    );
    assert!(record.parse_ok && record.emit_ok && record.reparse_ok && record.equivalent);
    // The player form follows the same path through its own catalog id.
    let text = "variables { player: 0: P }\nrule (\"chase\") { event { Ongoing - Each Player; } actions { Chase Player Variable At Rate(Event Player, P, 0, 1, None); } }";
    let record = roundtrip::round_trip_with_context(text, &catalog(), &en(), manifest_context());
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
    // frontend, emits through the catalog, reparses with the manifest
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
    let emitted =
        wright_workshop::emitter::emit(&wir, &catalog(), &en()).expect("the fixture emits");
    // The emission includes Debug/Print HUD text (canonical catalog layout)
    // and chase `None` members, so both the manifest and the catalog supply
    // the expected enum domains.
    let catalog = catalog();
    let context = wright_core::signatures::ChainedExpectedDomain::new(manifest_context(), &catalog);
    let record = roundtrip::round_trip_with_context(&emitted, &catalog, &en(), &context);
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
    let first =
        wright_workshop::parser::parse_with_context(text, &catalog, &en(), manifest_context())
            .expect("pinned Chase None parses");
    let emitted = wright_workshop::emitter::emit(&first, &catalog, &en()).expect("emits");
    assert!(
        emitted.contains("Chase Global Variable Over Time(Global.g, 0, 30, None)"),
        "emission preserves the bare None spelling:\n{emitted}"
    );
    let reparsed =
        wright_workshop::parser::parse_with_context(&emitted, &catalog, &en(), manifest_context())
            .expect("emitted text reparses with context");
    let reemitted = wright_workshop::emitter::emit(&reparsed, &catalog, &en()).expect("re-emits");
    assert_eq!(emitted, reemitted, "emission must be a fixed point");
}
