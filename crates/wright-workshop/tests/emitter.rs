//! Emitter tests (#34): byte-stable localized emission, round-trip
//! equivalence, and structured failure for unsupported/unknown output.

use std::path::{Path, PathBuf};

use wright_ir::wir;
use wright_workshop::catalog::{Catalog, Locale};
use wright_workshop::emitter;
use wright_workshop::parser;

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

/// Remove span suffixes so dumps can be compared modulo source locations.
fn without_spans(dump: &str) -> String {
    let re = regex::Regex::new(r" @\d+:\d+:\d+-\d+:\d+").unwrap();
    re.replace_all(dump, "").into_owned()
}

#[test]
fn emission_is_byte_stable_and_a_fixed_point() {
    let program = parser::parse(&corpus_text("synthetic/control-flow"), &catalog(), &en()).unwrap();
    let first = emitter::emit(&program, &catalog(), &en()).expect("emits");
    let second = emitter::emit(&program, &catalog(), &en()).expect("emits");
    assert_eq!(first, second, "emission must be byte-stable");

    // The emitted text is a fixed point: re-emitting the reparsed program
    // produces identical text.
    let reparsed = parser::parse(&first, &catalog(), &en()).expect("emitted text reparses");
    let reemitted = emitter::emit(&reparsed, &catalog(), &en()).expect("re-emits");
    assert_eq!(first, reemitted, "emission must be a fixed point");
}

#[test]
fn every_corpus_program_round_trips_to_equivalent_wir() {
    for fixture_id in [
        "synthetic/basic-rule",
        "synthetic/control-flow",
        "synthetic/declarations-rules",
        "synthetic/expressions-values",
        "synthetic/preprocessing",
        "real-world/overpy-cake",
    ] {
        let program = parser::parse(&corpus_text(fixture_id), &catalog(), &en())
            .unwrap_or_else(|error| panic!("{fixture_id} must parse: {error}"));
        let emitted = emitter::emit(&program, &catalog(), &en())
            .unwrap_or_else(|error| panic!("{fixture_id} must emit: {error}"));
        let reparsed = parser::parse(&emitted, &catalog(), &en()).unwrap_or_else(|error| {
            panic!("{fixture_id} emitted text must reparse:\n{error}\n{emitted}")
        });
        let original = without_spans(&program.dump());
        let round_tripped = without_spans(&reparsed.dump());
        assert_eq!(
            original, round_tripped,
            "{fixture_id} round trip must preserve semantics"
        );
    }
}

#[test]
fn emitted_text_is_recognizably_workshop() {
    let program = parser::parse(&corpus_text("synthetic/basic-rule"), &catalog(), &en()).unwrap();
    let emitted = emitter::emit(&program, &catalog(), &en()).unwrap();
    assert!(emitted.contains("rule (\"setup\") {"));
    assert!(emitted.contains("event {"));
    assert!(emitted.contains("Ongoing - Global;"));
    assert!(emitted.contains("actions {"));
    assert!(emitted.contains("Disable Inspector Recording;"));
}

#[test]
fn emitted_condition_matches_reference_infix_form() {
    let program = parser::parse(
        &corpus_text("synthetic/declarations-rules"),
        &catalog(),
        &en(),
    )
    .unwrap();
    let emitted = emitter::emit(&program, &catalog(), &en()).unwrap();
    assert!(
        emitted.contains("Has Spawned(Event Player) == True"),
        "non-comparison conditions emit in reference infix form:\n{emitted}"
    );
}

#[test]
fn debug_actions_emit_hud_text() {
    // Since M8, Debug/Print emit a semantically equivalent Create HUD Text
    // effect (documented intentional difference from the reference's
    // type-aware formatting).
    let mut program = wir::Program::default();
    let file = program
        .files
        .push(wright_ir::source::SourceFile::new("workshop.txt"));
    let value = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::Number(1.0),
        None,
    ));
    let debug = program
        .actions
        .push(wir::Action::Debug { value, span: None });
    program.rules.push(wir::Rule {
        name: "x".into(),
        span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: vec![],
        actions: vec![debug],
    });
    let _ = file;
    let emitted = emitter::emit(&program, &catalog(), &en()).expect("Debug emits");
    assert!(
        emitted.contains("Create HUD Text(All Players(All Teams), Null, Custom String(\"{0}\", 1)"),
        "debug emits the value as HUD text:\n{emitted}"
    );
    // The emitted text reparses to a createHudText action call.
    let reparsed = wright_workshop::parser::parse(&emitted, &catalog(), &en()).unwrap();
    assert_eq!(reparsed.rules.len(), 1);
}

#[test]
fn unknown_value_id_fails_explicitly() {
    let mut program = wir::Program::default();
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
    let call = program.actions.push(wir::Action::Call {
        name: "wait".into(),
        args: vec![value],
        span: None,
    });
    program.rules.push(wir::Rule {
        name: "x".into(),
        span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: vec![],
        actions: vec![call],
    });
    let error = emitter::emit(&program, &catalog(), &en()).expect_err("unknown id must fail");
    assert!(error.to_string().contains("notACatalogId"), "{error}");
}
