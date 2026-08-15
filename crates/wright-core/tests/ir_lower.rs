//! HIR → Workshop IR lowering tests against the compatibility corpus: every
//! v0.1 bridge fixture lowers into valid Workshop IR, dumps deterministically,
//! and unsupported constructs fail with their source location.

use std::path::{Path, PathBuf};

use wright_core::hir;
use wright_ir::error::IrError;
use wright_ir::lower;

fn fixture_path(fixture_id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../adapter/fixtures")
        .join(format!("{fixture_id}.json"))
}

fn read_fixture(fixture_id: &str) -> String {
    std::fs::read_to_string(fixture_path(fixture_id))
        .unwrap_or_else(|error| panic!("cannot read adapter fixture {fixture_id}: {error}"))
}

fn lower_fixture(fixture_id: &str) -> wright_ir::wir::Program {
    let protocol = hir::parse_str(&read_fixture(fixture_id))
        .unwrap_or_else(|error| panic!("{fixture_id} must parse: {error}"));
    let model = protocol
        .to_ir()
        .unwrap_or_else(|error| panic!("{fixture_id} must convert: {error}"));
    lower::lower(&model).unwrap_or_else(|error| panic!("{fixture_id} must lower: {error}"))
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
fn every_fixture_lowers_to_valid_workshop_ir() {
    for fixture_id in ADAPTER_FIXTURES {
        let program = lower_fixture(fixture_id);
        program
            .validate()
            .unwrap_or_else(|error| panic!("{fixture_id} WIR must validate: {error}"));
        assert!(!program.rules.is_empty(), "{fixture_id} must produce rules");
    }
}

#[test]
fn every_fixture_dumps_deterministically() {
    for fixture_id in ADAPTER_FIXTURES {
        let program = lower_fixture(fixture_id);
        let first = program.dump();
        let second = program.dump();
        assert_eq!(first, second, "{fixture_id} WIR dump must be deterministic");
        assert!(!first.is_empty(), "{fixture_id} WIR dump must not be empty");
    }
}

#[test]
fn compound_assignment_lowers_to_modify_action() {
    // `index += 1` in control-flow desugars to `index = index + 1` and must
    // lower to a ModifyGlobalVariable(Add, 1) action, not a Set.
    let program = lower_fixture("synthetic/control-flow");
    let dump = program.dump();
    assert!(
        dump.contains("modifyGlobalVariable index Add 1"),
        "compound assignment must lower to a modify action:\n{dump}"
    );
}

#[test]
fn append_lowers_to_modify_with_append_to_array() {
    let program = lower_fixture("synthetic/expressions-values");
    let dump = program.dump();
    assert!(
        dump.contains("modifyGlobalVariable points AppendToArray"),
        "append must lower to a modify action:\n{dump}"
    );
}

#[test]
fn subroutine_def_lowers_to_subroutine_event_rule() {
    let program = lower_fixture("synthetic/declarations-rules");
    let dump = program.dump();
    assert!(
        dump.contains("event Subroutine showStatus (id 0)"),
        "def body must become a Subroutine-event rule:\n{dump}"
    );
    assert!(dump.contains("callSubroutine showStatus (id 0)"));
}

#[test]
fn debug_and_print_lower_to_typed_actions() {
    let program = lower_fixture("synthetic/expressions-values");
    let dump = program.dump();
    assert!(
        dump.contains("print format(\"points: {}\", points)"),
        "{dump}"
    );
    assert!(dump.contains("debug location"), "{dump}");
}

#[test]
fn macro_call_expands_in_value_position() {
    // `debug(double(Phase.FINISHED))` — `double(value): value + value` must
    // expand to `$double(1)` → `+(1, 1)` in the debug action.
    let program = lower_fixture("synthetic/preprocessing");
    let dump = program.dump();
    assert!(
        dump.contains("debug +(1, 1)"),
        "macro call must expand during lowering:\n{dump}"
    );
}

#[test]
fn lower_dump_matches_golden_for_control_flow() {
    let program = lower_fixture("synthetic/control-flow");
    let golden = std::fs::read_to_string(golden_path("synthetic/control-flow.wir.dump"))
        .unwrap_or_else(|error| panic!("missing golden WIR dump: {error}"));
    assert_eq!(program.dump(), golden);
}

fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

#[test]
fn declaration_initializers_lower_to_initialize_rules() {
    // #112: declaration initializer semantics are owned by the
    // profile-independent HIR → WIR lowering. Lowering the
    // declarations-numbers adapter fixture must produce the synthetic
    // Initialize rules directly (no transformation pass), with the global
    // rule first and the player rule after it, before any user rule.
    let program = lower_fixture("synthetic/declarations-numbers");
    program
        .validate()
        .expect("lowered declarations-numbers WIR must validate");

    let mut rules = program.rules.iter();
    let global = rules.next().expect("a global Initialize rule");
    assert_eq!(global.name, "Initialize global variables");
    assert!(matches!(global.event, wright_ir::wir::Event::Global));
    assert_eq!(
        global.actions.len(),
        2,
        "j = 5 and k = 0.0 survive; h = 0 is dropped"
    );
    for action in &global.actions {
        assert!(
            matches!(
                program.actions.get(*action),
                Some(wright_ir::wir::Action::SetGlobalVariable { .. })
            ),
            "global initializers lower to Set Global Variable actions"
        );
    }

    let player = rules.next().expect("a player Initialize rule");
    assert_eq!(player.name, "Initialize player variables");
    assert!(matches!(player.event, wright_ir::wir::Event::EachPlayer));
    assert_eq!(player.actions.len(), 1, "playervar p = 7 survives");
    assert!(
        matches!(
            program.actions.get(player.actions[0]),
            Some(wright_ir::wir::Action::SetPlayerVariable { .. })
        ),
        "player initializers lower to Set Player Variable actions"
    );

    // The Initialize rules come before the user rule, matching the reference.
    let user = rules.next().expect("the user rule");
    assert_eq!(user.name, "r");
    assert!(rules.next().is_none(), "no further rules");

    // The dump renders the Initialize rules in order, and no variable table
    // entry carries an initializer field (the rules are the single source of
    // truth).
    let dump = program.dump();
    let global_pos = dump.find("Initialize global variables").unwrap();
    let player_pos = dump.find("Initialize player variables").unwrap();
    let user_pos = dump.find("\"r\"").unwrap();
    assert!(global_pos < player_pos && player_pos < user_pos, "{dump}");
}

#[test]
fn unsupported_event_fails_with_span() {
    let payload = r#"{
        "protocol": { "name": "wright/opy-hir", "version": "1.0.0" },
        "generator": { "name": "g", "version": "0", "frontend": "f" },
        "files": [ { "id": 0, "path": "source.opy" } ],
        "declarations": [],
        "rules": [ {
            "name": "r",
            "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } },
            "event": { "name": "onFlag", "args": [], "span": { "file": 0, "start": { "line": 2, "col": 5 }, "end": { "line": 2, "col": 20 } } },
            "conditions": [],
            "actions": []
        } ]
    }"#;
    let protocol = hir::parse_str(payload).unwrap();
    let model = protocol.to_ir().unwrap();
    let error = lower::lower(&model).unwrap_err();
    assert_eq!(error.code(), "unsupported");
    match error {
        IrError::Unsupported { message, span } => {
            assert!(message.contains("onFlag"));
            let span = span.expect("unsupported event must carry its span");
            assert_eq!(span.start.line, 2);
        }
        other => panic!("expected unsupported, got {other}"),
    }
}

#[test]
fn unsupported_for_iterable_fails_with_span() {
    let payload = r#"{
        "protocol": { "name": "wright/opy-hir", "version": "1.0.0" },
        "generator": { "name": "g", "version": "0", "frontend": "f" },
        "files": [ { "id": 0, "path": "source.opy" } ],
        "declarations": [
            { "kind": "globalVariable", "name": "i", "index": null, "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } }, "initializer": null }
        ],
        "rules": [ {
            "name": "r",
            "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } },
            "event": { "name": "global", "args": [], "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } } },
            "conditions": [],
            "actions": [
                { "kind": "for", "variable": { "kind": "globalVar", "name": "i", "span": { "file": 0, "start": { "line": 3, "col": 9 }, "end": { "line": 3, "col": 10 } } }, "iterable": { "kind": "array", "elements": [], "span": { "file": 0, "start": { "line": 3, "col": 14 }, "end": { "line": 3, "col": 16 } } }, "body": [], "span": { "file": 0, "start": { "line": 3, "col": 5 }, "end": { "line": 3, "col": 17 } } }
            ]
        } ]
    }"#;
    let protocol = hir::parse_str(payload).unwrap();
    let model = protocol.to_ir().unwrap();
    let error = lower::lower(&model).unwrap_err();
    assert_eq!(error.code(), "unsupported");
    match error {
        IrError::Unsupported { message, span } => {
            assert!(message.contains("range"), "{message}");
            let span = span.expect("unsupported iterable must carry its span");
            assert_eq!(span.start.line, 3);
        }
        other => panic!("expected unsupported, got {other}"),
    }
}

#[test]
fn implicit_default_var_binder_lowers_to_slot_ordered_table() {
    // The agent-lab shape: `globalvar total` + `for I in range(0, 10)` with
    // `I` undeclared. The implicit default variable (fixed slot 8) and the
    // declared `total` (lowest free slot 0) form a slot-ordered table that
    // matches the pinned reference emission `0: total, 8: I`, and the for
    // loop lowers to a ForGlobalVariable action on `I` (#114).
    let payload = r#"{
        "protocol": { "name": "wright/opy-hir", "version": "1.0.0" },
        "generator": { "name": "g", "version": "0", "frontend": "f" },
        "files": [ { "id": 0, "path": "source.opy" } ],
        "declarations": [
            { "kind": "globalVariable", "name": "total", "index": null, "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 15 } }, "initializer": null }
        ],
        "rules": [ {
            "name": "r",
            "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } },
            "event": { "name": "global", "args": [], "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } } },
            "conditions": [],
            "actions": [
                { "kind": "for", "variable": { "kind": "globalVar", "name": "I", "span": { "file": 0, "start": { "line": 2, "col": 9 }, "end": { "line": 2, "col": 10 } } }, "iterable": { "kind": "call", "name": "range", "args": [ { "kind": "number", "value": 0, "text": "0", "span": { "file": 0, "start": { "line": 2, "col": 20 }, "end": { "line": 2, "col": 21 } } }, { "kind": "number", "value": 10, "text": "10", "span": { "file": 0, "start": { "line": 2, "col": 22 }, "end": { "line": 2, "col": 25 } } } ], "span": { "file": 0, "start": { "line": 2, "col": 14 }, "end": { "line": 2, "col": 26 } } }, "body": [ { "kind": "assign", "target": { "kind": "globalVar", "name": "total", "span": { "file": 0, "start": { "line": 3, "col": 9 }, "end": { "line": 3, "col": 14 } } }, "value": { "kind": "binary", "op": "+", "left": { "kind": "globalVar", "name": "total", "span": { "file": 0, "start": { "line": 3, "col": 9 }, "end": { "line": 3, "col": 14 } } }, "right": { "kind": "globalVar", "name": "I", "span": { "file": 0, "start": { "line": 3, "col": 17 }, "end": { "line": 3, "col": 19 } } }, "span": { "file": 0, "start": { "line": 3, "col": 9 }, "end": { "line": 3, "col": 19 } } }, "span": { "file": 0, "start": { "line": 3, "col": 9 }, "end": { "line": 3, "col": 19 } } } ], "span": { "file": 0, "start": { "line": 2, "col": 5 }, "end": { "line": 2, "col": 10 } } }
            ]
        } ]
    }"#;
    let protocol = hir::parse_str(payload).unwrap();
    let model = protocol.to_ir().unwrap();
    let program = lower::lower(&model).unwrap();

    let slots: Vec<(u32, &str)> = program
        .global_variables
        .iter()
        .map(|variable| (variable.index, variable.name.as_str()))
        .collect();
    assert_eq!(
        slots,
        vec![(0, "total"), (8, "I")],
        "the table is slot-ordered like the reference"
    );

    // The for loop lowers to a ForGlobalVariable on the implicit `I` with
    // the range bounds (0, 10, 1).
    let dump = program.dump();
    assert!(
        dump.contains("forGlobalVariable I in 0, 10, 1"),
        "the binder lowers to the implicit global:\n{dump}"
    );
    assert!(
        dump.contains("modifyGlobalVariable total Add I"),
        "the body assignment lowers with the binder use resolved:\n{dump}"
    );
}
