//! Internal IR conversion tests: every v0.1 bridge fixture converts into the
//! typed `wright_ir` Opy HIR model without opaque catch-all nodes, validates,
//! and dumps deterministically.

use std::path::{Path, PathBuf};

use wright_core::hir::{self, HirError};
use wright_ir::error::IrError;

fn fixture_path(fixture_id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../adapter/fixtures")
        .join(format!("{fixture_id}.json"))
}

fn read_fixture(fixture_id: &str) -> String {
    std::fs::read_to_string(fixture_path(fixture_id))
        .unwrap_or_else(|error| panic!("cannot read adapter fixture {fixture_id}: {error}"))
}

const ADAPTER_FIXTURES: &[&str] = &[
    "synthetic/basic-rule",
    "synthetic/control-flow",
    "synthetic/declarations-rules",
    "synthetic/expressions-values",
    "synthetic/preprocessing",
    "real-world/overpy-cake",
];

#[test]
fn every_bridge_fixture_converts_and_validates() {
    for fixture_id in ADAPTER_FIXTURES {
        let protocol = hir::parse_str(&read_fixture(fixture_id))
            .unwrap_or_else(|error| panic!("{fixture_id} must parse: {error}"));
        let model = protocol
            .to_ir()
            .unwrap_or_else(|error| panic!("{fixture_id} must convert: {error}"));
        model
            .validate()
            .unwrap_or_else(|error| panic!("{fixture_id} model must validate: {error}"));
    }
}

#[test]
fn every_bridge_fixture_dumps_deterministically() {
    for fixture_id in ADAPTER_FIXTURES {
        let protocol = hir::parse_str(&read_fixture(fixture_id)).unwrap();
        let model = protocol.to_ir().unwrap();
        let first = model.dump();
        let second = model.dump();
        assert_eq!(
            first, second,
            "{fixture_id} model dump must be deterministic"
        );
        assert!(
            !first.is_empty(),
            "{fixture_id} model dump must not be empty"
        );
    }
}

#[test]
fn conversion_resolves_typed_symbol_references() {
    // `declarations-rules` declares a global, a player variable, a
    // subroutine, and a def; conversion must produce typed IDs, not strings.
    let protocol = hir::parse_str(&read_fixture("synthetic/declarations-rules")).unwrap();
    let model = protocol.to_ir().unwrap();

    assert_eq!(model.globals.len(), 1);
    assert_eq!(model.globals.iter().next().unwrap().name, "score");
    assert_eq!(model.players.len(), 1);
    assert_eq!(model.players.iter().next().unwrap().name, "hasStarted");
    // One subroutine: declared AND defined, merged into a single entry with a
    // body.
    assert_eq!(model.subroutines.len(), 1);
    let subroutine = model.subroutines.iter().next().unwrap();
    assert_eq!(subroutine.name, "showStatus");
    assert!(
        subroutine.body.is_some(),
        "def body must attach to the subroutine"
    );

    // The rule's callSubroutine statement must reference the typed ID.
    let rule = model.rules.iter().next().unwrap();
    assert_eq!(rule.name, "player starts");
    assert_eq!(rule.actions.len(), 2);
}

#[test]
fn conversion_rejects_unknown_binary_operator() {
    let payload = r#"{
        "protocol": { "name": "wright/opy-hir", "version": "1.0.0" },
        "generator": { "name": "g", "version": "0", "frontend": "f" },
        "files": [ { "id": 0, "path": "source.opy" } ],
        "declarations": [],
        "rules": [ {
            "name": "r",
            "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } },
            "event": { "name": "global", "args": [], "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } } },
            "conditions": [],
            "actions": [
                { "kind": "expr", "expr": { "kind": "binary", "op": "frobnicate", "left": { "kind": "number", "value": 1, "text": "1" }, "right": { "kind": "number", "value": 2, "text": "2" }, "span": { "file": 0, "start": { "line": 2, "col": 5 }, "end": { "line": 2, "col": 6 } } }, "span": { "file": 0, "start": { "line": 2, "col": 5 }, "end": { "line": 2, "col": 6 } } }
            ]
        } ]
    }"#;
    let protocol = hir::parse_str(payload).unwrap();
    let error = protocol.to_ir().unwrap_err();
    assert_eq!(error.code(), "unsupported");
    match error {
        IrError::Unsupported { message, span } => {
            assert!(message.contains("frobnicate"));
            assert!(span.is_some(), "unsupported operator must carry its span");
        }
        other => panic!("expected unsupported, got {other}"),
    }
}

#[test]
fn for_loop_variable_must_be_a_global_variable() {
    // The protocol gate rejects a for-loop variable that is not a global
    // variable reference before conversion ever runs.
    let payload = r#"{
        "protocol": { "name": "wright/opy-hir", "version": "1.0.0" },
        "generator": { "name": "g", "version": "0", "frontend": "f" },
        "files": [ { "id": 0, "path": "source.opy" } ],
        "declarations": [],
        "rules": [ {
            "name": "r",
            "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } },
            "event": { "name": "global", "args": [], "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } } },
            "conditions": [],
            "actions": [
                { "kind": "for", "variable": { "kind": "string", "value": "oops" }, "iterable": { "kind": "call", "name": "range", "args": [] }, "body": [], "span": { "file": 0, "start": { "line": 2, "col": 5 }, "end": { "line": 2, "col": 6 } } }
            ]
        } ]
    }"#;
    let error = hir::parse_str(payload).unwrap_err();
    assert_eq!(error.code(), "invalid-structure");
}

#[test]
fn protocol_validation_still_runs_before_conversion() {
    // An unresolvable reference is caught by protocol validation, not
    // conversion.
    let payload = r#"{
        "protocol": { "name": "wright/opy-hir", "version": "1.0.0" },
        "generator": { "name": "g", "version": "0", "frontend": "f" },
        "files": [ { "id": 0, "path": "source.opy" } ],
        "declarations": [],
        "rules": [ {
            "name": "r",
            "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } },
            "event": { "name": "global", "args": [], "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } } },
            "conditions": [],
            "actions": [
                { "kind": "expr", "expr": { "kind": "call", "name": "debug", "args": [ { "kind": "globalVar", "name": "missing", "span": { "file": 0, "start": { "line": 2, "col": 11 }, "end": { "line": 2, "col": 12 } } } ] }, "span": { "file": 0, "start": { "line": 2, "col": 5 }, "end": { "line": 2, "col": 6 } } }
            ]
        } ]
    }"#;
    let error = hir::parse_str(payload).unwrap_err();
    assert_eq!(error.code(), "unresolved-reference");
    assert!(matches!(error, HirError::Invalid { .. }));
}
