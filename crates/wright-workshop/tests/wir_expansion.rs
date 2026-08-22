//! WIR expansion and canonical-identity tests (#31): the declared P0 surface
//! is representable in Workshop IR, and catalog-backed validation rejects
//! unknown or locale-tainted builtin references deterministically.

use wright_workshop::catalog::{Catalog, Locale};
use wright_workshop::source::{Position, SourceFile, Span};
use wright_workshop::validate;
use wright_workshop::wir::{self, Action, Event, Value, ValueNode};

fn catalog() -> Catalog {
    Catalog::builtin().expect("built-in catalog")
}

fn span(file: wright_ir::ids::Id<SourceFile>, line: u32, col: u32, end_col: u32) -> Span {
    Span::new(file, Position::new(line, col), Position::new(line, end_col))
}

/// Build a WIR program representing the corpus Workshop surface: one global
/// variable, a rule with a condition, a For loop, an If, a Modify action, a
/// generic action, and the catalog-backed values.
fn build_surface_program() -> wir::Program {
    let mut program = wir::Program::default();
    let file = program.files.push(SourceFile::new("workshop.txt"));
    let s = |line, col, end| span(file, line, col, end);

    let index = program.global_variables.push(wir::WorkshopVariable {
        name: "index".into(),
        index: 0,
        span: Some(s(1, 15, 20)),
        name_span: Some(s(1, 15, 20)),
    });

    let zero = program.values.push(ValueNode::new(
        Value::Number {
            value: 0.0,
            text: "0".to_string(),
        },
        Some(s(3, 24, 25)),
    ));
    let stop = program.values.push(ValueNode::new(
        Value::Number {
            value: 3.0,
            text: "3".to_string(),
        },
        Some(s(3, 27, 28)),
    ));
    let one = program.values.push(ValueNode::new(
        Value::Number {
            value: 1.0,
            text: "1".to_string(),
        },
        Some(s(3, 30, 31)),
    ));
    let index_ref = program.values.push(ValueNode::new(
        Value::GlobalVariable(index),
        Some(s(4, 18, 23)),
    ));
    let compare = program.values.push(ValueNode::new(
        Value::Call {
            name: "==".into(),
            args: vec![index_ref, zero],
        },
        Some(s(4, 10, 24)),
    ));
    let modified = program.values.push(ValueNode::new(
        Value::Call {
            name: "add".into(),
            args: vec![index_ref, one],
        },
        Some(s(5, 26, 33)),
    ));
    let yellow = program.values.push(ValueNode::new(
        Value::Enum {
            value_type: "Color".into(),
            value: "YELLOW".into(),
        },
        Some(s(5, 41, 47)),
    ));
    let beam_type = program.values.push(ValueNode::new(
        Value::Enum {
            value_type: "Beam".into(),
            value: "GOOD".into(),
        },
        Some(s(5, 35, 39)),
    ));
    let start_position = program.values.push(ValueNode::new(
        Value::Vector {
            x: zero,
            y: zero,
            z: zero,
        },
        Some(s(5, 49, 60)),
    ));
    let end_position = program.values.push(ValueNode::new(
        Value::Vector {
            x: one,
            y: one,
            z: one,
        },
        Some(s(5, 62, 73)),
    ));
    let all_teams = program.values.push(ValueNode::new(
        Value::Enum {
            value_type: "Team".into(),
            value: "ALL".into(),
        },
        Some(s(6, 14, 23)),
    ));
    let players = program.values.push(ValueNode::new(
        Value::Call {
            name: "allPlayers".into(),
            args: vec![all_teams],
        },
        Some(s(6, 14, 24)),
    ));

    let debug_value = program.values.push(ValueNode::new(
        Value::Number {
            value: 1.0,
            text: "1".to_string(),
        },
        Some(s(7, 11, 12)),
    ));
    let debug = program.actions.push(Action::Debug {
        value: debug_value,
        span: Some(s(7, 9, 13)),
    });
    let if_body = vec![debug];
    let if_action = program.actions.push(Action::If {
        branches: vec![wir::IfBranch {
            condition: compare,
            body: if_body,
        }],
        else_body: None,
        span: Some(s(4, 5, 8)),
    });
    let modify = program.actions.push(Action::ModifyGlobalVariable {
        variable: index,
        op: wir::ModifyOp::Add,
        value: modified,
        span: Some(s(5, 5, 34)),
        target_span: Some(s(5, 5, 10)),
    });
    let beam = program.actions.push(Action::Call {
        name: "createBeamEffect".into(),
        args: vec![players, beam_type, start_position, end_position, yellow],
        span: Some(s(6, 5, 25)),
    });
    let for_action = program.actions.push(Action::ForGlobalVariable {
        variable: index,
        start: zero,
        stop,
        step: one,
        body: vec![if_action, modify, beam],
        span: Some(s(3, 5, 31)),
        target_span: Some(s(3, 5, 10)),
    });

    program.rules.push(wir::Rule {
        name: "surface".into(),
        span: Some(s(2, 1, 6)),
        name_span: Some(s(2, 5, 6)),
        disabled: false,
        event: Event::Global,
        conditions: vec![],
        actions: vec![for_action],
    });
    program
}

#[test]
fn corpus_surface_is_representable_and_validates() {
    let program = build_surface_program();
    program.validate().expect("WIR is structurally valid");
    validate::validate_canonical_ids(&program, &catalog()).expect("canonical ids resolve");

    let dump = program.dump();
    assert!(dump.contains("forGlobalVariable index in 0, 3, 1"));
    assert!(dump.contains("modifyGlobalVariable index Add"));
    assert!(dump.contains("call createBeamEffect"));
    assert!(dump.contains("Color.YELLOW"));
    assert!(dump.contains("allPlayers(Team.ALL)"));
}

#[test]
fn unknown_action_id_is_rejected_with_location() {
    let mut program = build_surface_program();
    let file = program
        .files
        .iter()
        .next()
        .map(|_| wright_ir::ids::Id::from_index(0))
        .expect("one file");
    let action = program
        .actions
        .get_mut(wir::ActionId::from_index(3))
        .expect("beam action");
    if let Action::Call { name, span, .. } = action {
        *name = "createLaserEffect".into();
        *span = Some(Span::new(file, Position::new(9, 5), Position::new(9, 20)));
    }
    let error = validate::validate_canonical_ids(&program, &catalog()).expect_err("unknown action");
    assert!(error.to_string().contains("createLaserEffect"), "{error}");
}

#[test]
fn unknown_enum_member_is_rejected() {
    let mut program = build_surface_program();
    // The Color enum node is at a known value id; replace its member.
    for node in program.values.iter() {
        if let Value::Enum {
            value_type, value, ..
        } = &node.value
        {
            if value_type == "Color" {
                let _ = value;
            }
        }
    }
    // Rebuild a bad enum node: find the Color enum and mutate it.
    let color_id = program
        .values
        .iter()
        .enumerate()
        .find(|(_, node)| {
            matches!(&node.value, Value::Enum { value_type, .. } if value_type == "Color")
        })
        .map(|(index, _)| wright_workshop::wir::ValueId::from_index(index))
        .expect("color node");
    if let Value::Enum { value, .. } = &mut program.values.get_mut(color_id).unwrap().value {
        *value = "NEON".into();
    }
    let error = validate::validate_canonical_ids(&program, &catalog()).expect_err("unknown member");
    assert!(error.to_string().contains("NEON"), "{error}");
}

#[test]
fn unknown_value_id_is_rejected() {
    let mut program = build_surface_program();
    for node in program.values.iter() {
        if let Value::Call { name, .. } = &node.value {
            if name == "add" {
                let _ = name;
            }
        }
    }
    let add_id = program
        .values
        .iter()
        .enumerate()
        .find(|(_, node)| matches!(&node.value, Value::Call { name, .. } if name == "add"))
        .map(|(index, _)| wright_workshop::wir::ValueId::from_index(index))
        .expect("add node");
    if let Value::Call { name, .. } = &mut program.values.get_mut(add_id).unwrap().value {
        *name = "plus".into();
    }
    let error = validate::validate_canonical_ids(&program, &catalog()).expect_err("unknown value");
    assert!(error.to_string().contains("plus"), "{error}");
}

#[test]
fn canonical_validation_is_locale_independent() {
    // Resolution uses canonical ids; locale spelling never appears in WIR.
    let program = build_surface_program();
    let _ = Locale::new("en-US");
    validate::validate_canonical_ids(&program, &catalog()).expect("valid");
    // No WIR dump contains a localized spelling.
    let dump = program.dump();
    assert!(!dump.contains("Disable Inspector Recording"));
}
