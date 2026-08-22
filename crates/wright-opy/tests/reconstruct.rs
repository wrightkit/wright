//! WIR → OPY reconstruction round-trip suite (issue #124).
//!
//! Proves `Workshop → WIR → reconstructed OPY → native frontend → HIR → WIR`
//! semantic equivalence for every deterministic reconstruction fixture:
//! the native OPY frontend accepts the reconstructed source, the recompiled
//! WIR is structurally equivalent to the parsed Workshop program under
//! `workshop_rs::roundtrip::equivalent`, and the recompiled WIR still
//! emits to Workshop text through the shipped emitter (the trailing
//! `→ Workshop` hop). Determinism, the machine-readable support boundary,
//! and the explicit rejection surface are all asserted here through the
//! shipped API.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::parser;
use workshop_rs::source::{Position, Span};
use workshop_rs::wir::{self, Action, Event, ModifyOp, Program, Value, ValueNode};

/// (fixture id, constructs covered) — the machine-readable coverage map that
/// the support boundary consistency test cross-checks.
const ROUND_TRIP_FIXTURES: &[(&str, &[&str])] = &[
    (
        "variables-declarations",
        &[
            "global-variable-declaration",
            "player-variable-declaration",
            "variable-index",
            "global-initializer-rule",
            "player-initializer-rule",
            "subroutine-declaration",
            "global-event",
            "each-player-event",
            "number-value",
            "bool-value",
            "global-variable-access",
            "player-variable-access",
            "binary-expression",
            "set-global-variable",
            "set-player-variable",
            "call-subroutine",
        ],
    ),
    (
        "subroutine-control-flow",
        &[
            "subroutine-body",
            "modify-global-variable",
            "append-modify",
            "modify-player-variable",
            "if-statement",
            "while-statement",
            "for-range-loop",
            "wait-action",
            "call-subroutine",
        ],
    ),
    (
        "player-events",
        &[
            "each-player-event",
            "condition",
            "null-value",
            "global-variable-access",
            "player-variable-access",
            "member-action",
            "member-value-call",
            "subroutine-body",
            "set-player-variable",
        ],
    ),
    (
        "values-enums",
        &[
            "condition",
            "string-value",
            "null-value",
            "enum-value",
            "global-variable-access",
            "player-variable-access",
            "event-player-value",
            "binary-expression",
            "unary-expression",
            "value-call",
            "member-value-call",
        ],
    ),
    (
        "actions-surface",
        &[
            "wait-action",
            "disable-inspector-action",
            "play-effect-action",
            "chase-over-time-action",
            "subroutine-body",
            "call-subroutine",
        ],
    ),
];

/// (test case, expected first diagnostic code, rejected constructs) — the
/// machine-readable rejection map for the support boundary.
const REJECTION_CASES: &[(&str, &str, &[&str])] = &[
    (
        "rejects_per_player_loop",
        "unsupported-per-player-loop",
        &["per-player-loop"],
    ),
    (
        "rejects_disabled_rule",
        "unsupported-disabled-rule",
        &["disabled-rule"],
    ),
    (
        "rejects_arbitrary_player_target",
        "unsupported-arbitrary-player-target",
        &["arbitrary-player-target"],
    ),
    (
        "rejects_unrepresentable_value_call",
        "unsupported-value-call",
        &["unrepresentable-value-call"],
    ),
    (
        "rejects_unrepresentable_action_call",
        "unsupported-action-call",
        &["unrepresentable-action-call"],
    ),
    (
        "rejects_dedicated_action_call",
        "unsupported-action-call",
        &["dedicated-action-call"],
    ),
    (
        "rejects_dedicated_value_call",
        "unsupported-value-call",
        &["dedicated-value-call"],
    ),
    (
        "rejects_invalid_identifier",
        "unsupported-name",
        &["invalid-identifier-name"],
    ),
    (
        "rejects_reserved_name",
        "unsupported-name",
        &["reserved-name"],
    ),
    (
        "rejects_duplicate_name",
        "unsupported-duplicate-name",
        &["duplicate-name"],
    ),
    (
        "rejects_negative_number",
        "unsupported-negative-number",
        &["negative-number"],
    ),
    (
        "rejects_non_finite_number",
        "unsupported-non-finite-number",
        &["non-finite-number"],
    ),
    (
        "rejects_unknown_enum_domain",
        "unsupported-enum-domain",
        &["unknown-enum-domain"],
    ),
    (
        "rejects_unknown_enum_member",
        "unsupported-enum-member",
        &["unknown-enum-member"],
    ),
    (
        "rejects_remove_from_array_modify",
        "unsupported-modify-op",
        &["remove-from-array-modify"],
    ),
    (
        "rejects_missing_argument",
        "unsupported-missing-argument",
        &["missing-argument"],
    ),
    (
        "rejects_invalid_arity",
        "unsupported-invalid-arity",
        &["invalid-arity"],
    ),
    (
        "rejects_enum_domain_mismatch",
        "unsupported-enum-domain-mismatch",
        &["enum-domain-mismatch"],
    ),
    (
        "rejects_invalid_argument",
        "unsupported-invalid-argument",
        &["invalid-argument"],
    ),
    (
        "rejects_set_with_same_variable_binary",
        "unsupported-set-binary",
        &["set-same-variable-binary"],
    ),
    (
        "rejects_rule_order",
        "unsupported-rule-order",
        &["rule-order"],
    ),
    (
        "rejects_non_canonical_init_rule",
        "unsupported-init-rule",
        &["non-canonical-init-rule"],
    ),
    (
        "rejects_subroutine_index_mismatch",
        "unsupported-subroutine-index",
        &["subroutine-index-mismatch"],
    ),
    (
        "rejects_unsorted_global_slots",
        "unsupported-global-order",
        &["global-index-order"],
    ),
    (
        "rejects_indexed_initializer",
        "unsupported-indexed-initializer",
        &["indexed-initializer"],
    ),
    ("rejects_settings", "unsupported-settings", &["settings"]),
];

// ---- support boundary ----

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BoundaryFile {
    schema_version: u32,
    supported: Vec<BoundaryEntry>,
    rejected: Vec<RejectedEntry>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BoundaryEntry {
    id: String,
    #[serde(default)]
    unit_only: bool,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RejectedEntry {
    id: String,
    code: String,
}

fn boundary() -> BoundaryFile {
    let path = fixtures_dir().join("boundary.json");
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap())
        .expect("boundary.json must parse")
}

// ---- helpers ----

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/reconstruct")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn catalog() -> Catalog {
    Catalog::builtin().unwrap()
}

fn en() -> Locale {
    Locale::new("en-US")
}

fn span() -> Span {
    Span::new(
        wright_ir::ids::Id::from_index(0),
        Position::new(1, 1),
        Position::new(1, 1),
    )
}

fn value(program: &mut Program, value: Value) -> wir::ValueId {
    program.values.push(ValueNode::new(value, Some(span())))
}

fn number(program: &mut Program, number: f64) -> wir::ValueId {
    value(
        program,
        Value::Number {
            value: number,
            text: wright_ir::format::format_number(number),
        },
    )
}

fn global(program: &mut Program, name: &str) -> (wir::GlobalVarId, wir::ValueId) {
    let id = program.global_variables.push(wir::WorkshopVariable {
        name: name.to_string(),
        index: program.global_variables.len() as u32,
        span: None,
        name_span: None,
    });
    let read = value(program, Value::GlobalVariable(id));
    (id, read)
}

fn event_player(program: &mut Program) -> wir::ValueId {
    value(program, Value::EventPlayer)
}

fn player(program: &mut Program, name: &str) -> (wir::PlayerVarId, wir::ValueId) {
    let id = program.player_variables.push(wir::WorkshopVariable {
        name: name.to_string(),
        index: program.player_variables.len() as u32,
        span: None,
        name_span: None,
    });
    let player = event_player(program);
    let read = value(
        program,
        Value::PlayerVariable {
            player,
            variable: id,
        },
    );
    (id, read)
}

fn enum_value(program: &mut Program, domain: &str, member: &str) -> wir::ValueId {
    value(
        program,
        Value::Enum {
            value_type: domain.to_string(),
            value: member.to_string(),
        },
    )
}

fn global_rule(program: &mut Program, name: &str, actions: Vec<wir::ActionId>) {
    program.rules.push(wir::Rule {
        name: name.to_string(),
        span: None,
        name_span: None,
        disabled: false,
        event: Event::Global,
        conditions: Vec::new(),
        actions,
    });
}

fn reconstruct(program: &Program) -> Result<String, wright_opy::reconstruct::ReconstructError> {
    wright_opy::reconstruct::reconstruct(program)
}

// ---- round-trip suite ----

#[test]
fn every_fixture_round_trips_through_the_shipped_path() {
    let catalog = catalog();
    let locale = en();
    let mut report: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    let mut failures = Vec::new();

    for (fixture, _constructs) in ROUND_TRIP_FIXTURES {
        let source = std::fs::read_to_string(fixtures_dir().join(format!("{fixture}.ws"))).unwrap();
        let parsed = match parser::parse(&source, &catalog, &locale) {
            Ok(program) => program,
            Err(error) => {
                failures.push(format!("{fixture}: Workshop parse failed: {error}"));
                continue;
            }
        };
        let opy = match wright_opy::reconstruct::reconstruct(&parsed) {
            Ok(opy) => opy,
            Err(error) => {
                failures.push(format!("{fixture}: reconstruction failed: {error}"));
                continue;
            }
        };
        // The native frontend must accept the reconstructed OPY…
        let hir = match wright_opy::compile(&opy, &format!("{fixture}.opy"), Path::new("")) {
            Ok(hir) => hir,
            Err(error) => {
                failures.push(format!(
                    "{fixture}: the native frontend rejected the reconstructed OPY: {error}"
                ));
                continue;
            }
        };
        // …and the recompiled WIR must be equivalent to the parsed WIR.
        let recompiled = match wright_ir::lower::lower(&hir.to_ir().unwrap()) {
            Ok(program) => program,
            Err(error) => {
                failures.push(format!("{fixture}: re-lowering failed: {error}"));
                continue;
            }
        };
        let equivalent = workshop_rs::roundtrip::equivalent(&parsed, &recompiled);
        // The trailing `→ Workshop` hop: the recompiled WIR still emits to
        // Workshop text through the shipped emitter.
        let workshop_emit = workshop_rs::emitter::emit(&recompiled, &catalog, &locale)
            .map(|_| ())
            .map_err(|error| error.to_string());
        if !equivalent {
            failures.push(format!("{fixture}: recompiled WIR is not equivalent"));
        }
        if let Err(error) = &workshop_emit {
            failures.push(format!(
                "{fixture}: Workshop emission of the recompiled WIR failed: {error}"
            ));
        }

        let report_entry = serde_json::json!({
            "input": source,
            "inputSha256": sha256(&source),
            "reconstructedOpy": opy,
            "frontendAccepted": true,
            "equivalent": equivalent,
            "workshopEmit": workshop_emit.is_ok(),
        });
        report.insert(fixture.to_string(), report_entry);

        // Evidence: one reconstructed OPY source per fixture.
        let out_dir = workspace_root().join("target/wright-reconstruction");
        std::fs::create_dir_all(&out_dir).unwrap();
        std::fs::write(out_dir.join(format!("{fixture}.opy")), &opy).unwrap();
    }

    let report_path = workspace_root().join("target/wright-reconstruction-report.json");
    std::fs::create_dir_all(report_path.parent().unwrap()).unwrap();
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&serde_json::Value::Object(report.into_iter().collect()))
            .unwrap(),
    )
    .unwrap();

    assert!(
        failures.is_empty(),
        "every reconstruction fixture must round-trip:\n{}",
        failures.join("\n")
    );
}

#[test]
fn reconstruction_is_deterministic() {
    let catalog = catalog();
    let locale = en();
    for (fixture, _constructs) in ROUND_TRIP_FIXTURES {
        let source = std::fs::read_to_string(fixtures_dir().join(format!("{fixture}.ws"))).unwrap();
        let parsed = parser::parse(&source, &catalog, &locale).unwrap();
        let first = wright_opy::reconstruct::reconstruct(&parsed).unwrap();
        let second = wright_opy::reconstruct::reconstruct(&parsed).unwrap();
        assert_eq!(
            first, second,
            "{fixture}: reconstruction must be byte-stable"
        );
    }
}

// ---- support boundary ----

#[test]
fn support_boundary_is_consistent_with_the_tests() {
    let boundary = boundary();
    assert_eq!(boundary.schema_version, 1);

    let supported: std::collections::HashSet<&str> = boundary
        .supported
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    let _rejected: std::collections::HashSet<&str> = boundary
        .rejected
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();

    let mut fixture_constructs: Vec<&str> = Vec::new();
    for (_fixture, constructs) in ROUND_TRIP_FIXTURES {
        for construct in *constructs {
            assert!(
                supported.contains(*construct),
                "fixture construct '{construct}' is missing from the support boundary"
            );
            fixture_constructs.push(construct);
        }
    }
    let mut rejection_constructs: Vec<&str> = Vec::new();
    for (_case, code, constructs) in REJECTION_CASES {
        for construct in *constructs {
            let entry = boundary
                .rejected
                .iter()
                .find(|entry| entry.id == *construct)
                .unwrap_or_else(|| {
                    panic!("rejection construct '{construct}' is missing from the boundary")
                });
            assert_eq!(
                entry.code, *code,
                "rejection construct '{construct}' records code '{}' but the test expects '{code}'",
                entry.code
            );
            rejection_constructs.push(construct);
        }
    }

    // No silent coverage drift: every fixture-covered supported construct is
    // exercised by a fixture, and every rejected construct is exercised by a
    // rejection case.
    for entry in &boundary.supported {
        if entry.unit_only {
            continue;
        }
        assert!(
            fixture_constructs.contains(&entry.id.as_str()),
            "supported construct '{}' is covered by no fixture",
            entry.id
        );
    }
    for entry in &boundary.rejected {
        assert!(
            rejection_constructs.contains(&entry.id.as_str()),
            "rejected construct '{}' is covered by no rejection case",
            entry.id
        );
    }
}

// ---- explicit rejection ----

#[test]
fn non_representable_constructs_fail_deterministically() {
    for (case, expected_code, _constructs) in REJECTION_CASES {
        let program = rejection_program(case);
        let error = match reconstruct(&program) {
            Ok(_) => panic!("{case}: the program must be rejected"),
            Err(error) => error,
        };
        let first = &error.issues[0];
        assert_eq!(
            first.code, *expected_code,
            "{case}: expected code '{}' but got '{}' with message '{}'",
            expected_code, first.code, first.message
        );
        assert!(
            !first.message.is_empty(),
            "{case}: the diagnostic must name the construct"
        );
    }
}

/// Build the minimal WIR program for one rejection case.
fn rejection_program(case: &str) -> Program {
    let mut program = Program::default();
    match case {
        "rejects_per_player_loop" => {
            let (has_started, _) = player(&mut program, "hasStarted");
            let loop_player = event_player(&mut program);
            let loop_start = number(&mut program, 0.0);
            let loop_stop = number(&mut program, 3.0);
            let loop_step = number(&mut program, 1.0);
            let action = program.actions.push(Action::ForPlayerVariable {
                player: loop_player,
                variable: has_started,
                start: loop_start,
                stop: loop_stop,
                step: loop_step,
                body: Vec::new(),
                span: None,
            });
            global_rule(&mut program, "loop", vec![action]);
        }
        "rejects_disabled_rule" => {
            program.rules.push(wir::Rule {
                name: "off".to_string(),
                span: None,
                name_span: None,
                disabled: true,
                event: Event::Global,
                conditions: Vec::new(),
                actions: Vec::new(),
            });
        }
        "rejects_arbitrary_player_target" => {
            let (result, _) = global(&mut program, "result");
            let (has_started, _) = player(&mut program, "hasStarted");
            let player_expr = value(&mut program, Value::GlobalVariable(result));
            let one = number(&mut program, 1.0);
            let set = program.actions.push(Action::SetPlayerVariable {
                player: player_expr,
                variable: has_started,
                value: one,
                span: None,
                target_span: None,
            });
            global_rule(&mut program, "main", vec![set]);
        }
        "rejects_unrepresentable_value_call" => {
            let (result, _) = global(&mut program, "result");
            let (_, has_started) = player(&mut program, "hasStarted");
            let call = value(
                &mut program,
                Value::Call {
                    name: "countOf".to_string(),
                    args: vec![has_started],
                },
            );
            let set = program.actions.push(Action::SetGlobalVariable {
                variable: result,
                value: call,
                span: None,
                target_span: None,
            });
            global_rule(&mut program, "main", vec![set]);
        }
        "rejects_unrepresentable_action_call" => {
            let action = program.actions.push(Action::Call {
                name: "createBeamEffect".to_string(),
                args: Vec::new(),
                span: None,
            });
            global_rule(&mut program, "main", vec![action]);
        }
        "rejects_dedicated_action_call" => {
            let one = number(&mut program, 1.0);
            let action = program.actions.push(Action::Call {
                name: "debug".to_string(),
                args: vec![one],
                span: None,
            });
            global_rule(&mut program, "main", vec![action]);
        }
        "rejects_dedicated_value_call" => {
            let (result, _) = global(&mut program, "result");
            let one = number(&mut program, 1.0);
            let call = value(
                &mut program,
                Value::Call {
                    name: "vect".to_string(),
                    args: vec![one],
                },
            );
            let set = program.actions.push(Action::SetGlobalVariable {
                variable: result,
                value: call,
                span: None,
                target_span: None,
            });
            global_rule(&mut program, "main", vec![set]);
        }
        "rejects_invalid_identifier" => {
            program.global_variables.push(wir::WorkshopVariable {
                name: "two words".to_string(),
                index: 0,
                span: None,
                name_span: None,
            });
        }
        "rejects_reserved_name" => {
            program.global_variables.push(wir::WorkshopVariable {
                name: "if".to_string(),
                index: 0,
                span: None,
                name_span: None,
            });
        }
        "rejects_duplicate_name" => {
            for index in 0..2 {
                program.global_variables.push(wir::WorkshopVariable {
                    name: "dup".to_string(),
                    index,
                    span: None,
                    name_span: None,
                });
            }
        }
        "rejects_negative_number" => {
            let (result, _) = global(&mut program, "result");
            let negative = number(&mut program, -1.0);
            let set = program.actions.push(Action::SetGlobalVariable {
                variable: result,
                value: negative,
                span: None,
                target_span: None,
            });
            global_rule(&mut program, "main", vec![set]);
        }
        "rejects_non_finite_number" => {
            let (result, _) = global(&mut program, "result");
            let nan = value(
                &mut program,
                Value::Number {
                    value: f64::NAN,
                    text: "NaN".to_string(),
                },
            );
            let set = program.actions.push(Action::SetGlobalVariable {
                variable: result,
                value: nan,
                span: None,
                target_span: None,
            });
            global_rule(&mut program, "main", vec![set]);
        }
        "rejects_unknown_enum_domain" => {
            let (result, _) = global(&mut program, "result");
            let enum_value = enum_value(&mut program, "NotADomain", "X");
            let set = program.actions.push(Action::SetGlobalVariable {
                variable: result,
                value: enum_value,
                span: None,
                target_span: None,
            });
            global_rule(&mut program, "main", vec![set]);
        }
        "rejects_unknown_enum_member" => {
            let (result, _) = global(&mut program, "result");
            let enum_value = enum_value(&mut program, "Color", "CYAN");
            let set = program.actions.push(Action::SetGlobalVariable {
                variable: result,
                value: enum_value,
                span: None,
                target_span: None,
            });
            global_rule(&mut program, "main", vec![set]);
        }
        "rejects_remove_from_array_modify" => {
            let (score, _) = global(&mut program, "score");
            let one = number(&mut program, 1.0);
            let modify = program.actions.push(Action::ModifyGlobalVariable {
                variable: score,
                op: ModifyOp::RemoveFromArray,
                value: one,
                span: None,
                target_span: None,
            });
            global_rule(&mut program, "main", vec![modify]);
        }
        "rejects_missing_argument" => {
            let one = number(&mut program, 1.0);
            let wait = program.actions.push(Action::Call {
                name: "wait".to_string(),
                args: vec![one],
                span: None,
            });
            global_rule(&mut program, "main", vec![wait]);
        }
        "rejects_invalid_arity" => {
            let one = number(&mut program, 1.0);
            let ignore = enum_value(&mut program, "Wait", "IGNORE_CONDITION");
            let two = number(&mut program, 2.0);
            let wait = program.actions.push(Action::Call {
                name: "wait".to_string(),
                args: vec![one, ignore, two],
                span: None,
            });
            global_rule(&mut program, "main", vec![wait]);
        }
        "rejects_enum_domain_mismatch" => {
            let one = number(&mut program, 1.0);
            let yellow = enum_value(&mut program, "Color", "YELLOW");
            let wait = program.actions.push(Action::Call {
                name: "wait".to_string(),
                args: vec![one, yellow],
                span: None,
            });
            global_rule(&mut program, "main", vec![wait]);
        }
        "rejects_invalid_argument" => {
            let ten = number(&mut program, 10.0);
            let three = number(&mut program, 3.0);
            let none = enum_value(&mut program, "ChaseTimeReeval", "NONE");
            let chase = program.actions.push(Action::Call {
                name: "chaseOverTime".to_string(),
                args: vec![ten, ten, three, none],
                span: None,
            });
            global_rule(&mut program, "main", vec![chase]);
        }
        "rejects_set_with_same_variable_binary" => {
            let (score, score_read) = global(&mut program, "score");
            let one = number(&mut program, 1.0);
            let sum = value(
                &mut program,
                Value::Call {
                    name: "+".to_string(),
                    args: vec![score_read, one],
                },
            );
            let set = program.actions.push(Action::SetGlobalVariable {
                variable: score,
                value: sum,
                span: None,
                target_span: None,
            });
            global_rule(&mut program, "main", vec![set]);
        }
        "rejects_rule_order" => {
            let sub_id = program.subroutines.push(wir::WorkshopSubroutine {
                name: "tick".to_string(),
                index: 0,
                span: None,
                name_span: None,
            });
            global_rule(&mut program, "normal", Vec::new());
            program.rules.push(wir::Rule {
                name: "Subroutine tick".to_string(),
                span: None,
                name_span: None,
                disabled: false,
                event: Event::Subroutine(sub_id),
                conditions: Vec::new(),
                actions: Vec::new(),
            });
        }
        "rejects_non_canonical_init_rule" => {
            let (score, _) = global(&mut program, "score");
            let one = number(&mut program, 1.0);
            let set = program.actions.push(Action::SetGlobalVariable {
                variable: score,
                value: one,
                span: None,
                target_span: None,
            });
            let call = program.actions.push(Action::Call {
                name: "disableInspector".to_string(),
                args: Vec::new(),
                span: None,
            });
            program.rules.push(wir::Rule {
                name: "Initialize global variables".to_string(),
                span: None,
                name_span: None,
                disabled: false,
                event: Event::Global,
                conditions: Vec::new(),
                actions: vec![set, call],
            });
        }
        "rejects_subroutine_index_mismatch" => {
            program.subroutines.push(wir::WorkshopSubroutine {
                name: "tick".to_string(),
                index: 5,
                span: None,
                name_span: None,
            });
        }
        "rejects_unsorted_global_slots" => {
            program.global_variables.push(wir::WorkshopVariable {
                name: "a".to_string(),
                index: 5,
                span: None,
                name_span: None,
            });
            program.global_variables.push(wir::WorkshopVariable {
                name: "b".to_string(),
                index: 0,
                span: None,
                name_span: None,
            });
        }
        "rejects_indexed_initializer" => {
            // `a` claims the lowest free slot 0 without an initializer;
            // initializer-bearing `b` at slot 5 cannot be spelled in OPY
            // (the `globalvar name = value` form drops the index and would
            // re-lower `b` to slot 1).
            program.global_variables.push(wir::WorkshopVariable {
                name: "a".to_string(),
                index: 0,
                span: None,
                name_span: None,
            });
            let b = program.global_variables.push(wir::WorkshopVariable {
                name: "b".to_string(),
                index: 5,
                span: None,
                name_span: None,
            });
            let init_value = number(&mut program, 5.0);
            let set = program.actions.push(Action::SetGlobalVariable {
                variable: b,
                value: init_value,
                span: None,
                target_span: None,
            });
            program.rules.push(wir::Rule {
                name: "Initialize global variables".to_string(),
                span: None,
                name_span: None,
                disabled: false,
                event: Event::Global,
                conditions: Vec::new(),
                actions: vec![set],
            });
        }
        "rejects_settings" => {
            program.settings = Some(workshop_rs::settings::Settings {
                span: None,
                children: Vec::new(),
            });
        }
        other => panic!("unknown rejection case '{other}'"),
    }
    program
}

fn sha256(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}
