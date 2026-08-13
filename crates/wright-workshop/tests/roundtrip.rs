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
    for fixture_id in [
        "synthetic/basic-rule",
        "synthetic/control-flow",
        "synthetic/declarations-rules",
        "synthetic/expressions-values",
        "synthetic/preprocessing",
        "real-world/overpy-cake",
    ] {
        let record = roundtrip::round_trip(&corpus_text(fixture_id), &catalog(), &en());
        assert!(
            record.error.is_none(),
            "{fixture_id} must round-trip cleanly: {:?}",
            record.error
        );
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
    // blocks the gate.
    let failures: Vec<String> = [
        "synthetic/basic-rule",
        "synthetic/control-flow",
        "synthetic/declarations-rules",
        "synthetic/expressions-values",
        "synthetic/preprocessing",
        "real-world/overpy-cake",
    ]
    .iter()
    .map(|fixture_id| roundtrip::round_trip(&corpus_text(fixture_id), &catalog(), &en()))
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
    let a = wright_ir::wir::Program::default();
    let b = wright_ir::wir::Program::default();
    // Two empty programs are equivalent.
    assert!(roundtrip::equivalent(&a, &b));
    // Same semantics, different file ids in spans: still equivalent.
    let mut c = wright_ir::wir::Program::default();
    c.files
        .push(wright_ir::source::SourceFile::new("other.txt"));
    assert!(
        roundtrip::equivalent(&a, &c),
        "file paths are presentation-only"
    );
}

#[test]
fn equivalence_detects_semantic_differences() {
    let mut a = wright_ir::wir::Program::default();
    a.files
        .push(wright_ir::source::SourceFile::new("workshop.txt"));
    let mut b = a.clone();
    b.files
        .push(wright_ir::source::SourceFile::new("workshop.txt"));

    let value_a = a.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::Number(1.0),
        None,
    ));
    let value_b = b.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::Number(2.0),
        None,
    ));
    a.actions.push(wright_ir::wir::Action::Debug {
        value: value_a,
        span: None,
    });
    b.actions.push(wright_ir::wir::Action::Debug {
        value: value_b,
        span: None,
    });
    a.rules.push(wright_ir::wir::Rule {
        name: "r".into(),
        span: None,
        name_span: None,
        disabled: false,
        event: wright_ir::wir::Event::Global,
        conditions: vec![],
        actions: a
            .actions
            .iter()
            .map(|_| wright_ir::ids::Id::from_index(0))
            .collect(),
    });
    b.rules.push(wright_ir::wir::Rule {
        name: "r".into(),
        span: None,
        name_span: None,
        disabled: false,
        event: wright_ir::wir::Event::Global,
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
    let record = roundtrip::round_trip(
        "rule (\"broken\") { actions { If(True); } }",
        &catalog(),
        &en(),
    );
    assert!(!record.parse_ok);
    assert!(!record.equivalent);
    assert!(record.error.is_some(), "failure is recorded");
}

#[test]
fn unknown_builtin_fails_at_emit_stage() {
    // A program that parses but contains a non-catalog value fails emit.
    let mut program = wright_ir::wir::Program::default();
    program
        .files
        .push(wright_ir::source::SourceFile::new("workshop.txt"));
    let value = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::Call {
            name: "notACatalogId".into(),
            args: vec![],
        },
        None,
    ));
    let call = program.actions.push(wright_ir::wir::Action::Call {
        name: "wait".into(),
        args: vec![value],
        span: None,
    });
    program.rules.push(wright_ir::wir::Rule {
        name: "x".into(),
        span: None,
        name_span: None,
        disabled: false,
        event: wright_ir::wir::Event::Global,
        conditions: vec![],
        actions: vec![call],
    });
    // Equivalent to the round-trip emit stage: emission of unknown ids fails.
    let error =
        wright_workshop::emitter::emit(&program, &catalog(), &en()).expect_err("unknown id");
    assert!(error.to_string().contains("notACatalogId"));
}
