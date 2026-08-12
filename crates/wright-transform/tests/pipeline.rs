//! WIR transformation pipeline tests (#51/#52): `off` is a no-op, compat
//! passes are evidence-backed with before/after metrics, and every pass
//! leaves the WIR validated.

use wright_ir::wir::{self, Action, Value, ValueNode};
use wright_transform::profile::Profile;
use wright_transform::run;

/// Build a program with `x = len(a) + 2 * 3` and an array initializer.
fn arithmetic_program() -> wir::Program {
    let mut program = wir::Program::default();
    program
        .files
        .push(wright_ir::source::SourceFile::new("test.opy"));

    let one = program
        .values
        .push(ValueNode::new(Value::Number(1.0), None));
    let two_literal = program
        .values
        .push(ValueNode::new(Value::Number(2.0), None));
    let array = program
        .values
        .push(ValueNode::new(Value::Array(vec![one, two_literal]), None));
    let points = program.global_variables.push(wir::WorkshopVariable {
        name: "points".to_string(),
        index: 1,
        span: None,
        initializer: Some(array),
    });
    let variable = program.global_variables.push(wir::WorkshopVariable {
        name: "result".to_string(),
        index: 0,
        span: None,
        initializer: None,
    });

    let two = program
        .values
        .push(ValueNode::new(Value::Number(2.0), None));
    let three = program
        .values
        .push(ValueNode::new(Value::Number(3.0), None));
    let multiply = program.values.push(ValueNode::new(
        Value::Call {
            name: "*".to_string(),
            args: vec![two, three],
        },
        None,
    ));
    let points_ref = program
        .values
        .push(ValueNode::new(Value::GlobalVariable(points), None));
    let len = program.values.push(ValueNode::new(
        Value::Call {
            name: "len".to_string(),
            args: vec![points_ref],
        },
        None,
    ));
    let add = program.values.push(ValueNode::new(
        Value::Call {
            name: "+".to_string(),
            args: vec![len, multiply],
        },
        None,
    ));
    let action = program.actions.push(Action::SetGlobalVariable {
        variable,
        value: add,
        span: None,
    });
    program.rules.push(wir::Rule {
        name: "compute".to_string(),
        span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: vec![],
        actions: vec![action],
    });
    program
}

#[test]
fn off_profile_performs_no_transformation() {
    let mut program = arithmetic_program();
    let before = program.dump();
    let results = run(&mut program, Profile::Off).unwrap();
    assert!(results.is_empty(), "off runs no passes");
    assert_eq!(program.dump(), before, "off leaves the WIR untouched");
    assert!(program.validate().is_ok());
}

#[test]
fn compat_profile_folds_constants_with_metrics() {
    let mut program = arithmetic_program();
    let results = run(&mut program, Profile::Compat).unwrap();
    assert_eq!(results.len(), 2, "two compat passes");

    // fold-constants: `2 * 3` collapsed to `6`.
    let fold = results
        .iter()
        .find(|result| result.stats.pass == "fold-constants")
        .expect("fold-constants ran");
    assert!(fold.stats.changed >= 1, "changed: {}", fold.stats.changed);

    // The folded expression is now the literal 6 in the action.
    let rule = program
        .rules
        .get(wright_ir::ids::Id::from_index(1))
        .expect("rule");
    let Action::SetGlobalVariable { value, .. } = program.actions.get(rule.actions[0]).unwrap()
    else {
        panic!("expected Set Global Variable");
    };
    let node = program.values.get(*value).unwrap();
    match &node.value {
        Value::Call { name, args } => {
            assert_eq!(
                name, "+",
                "folding keeps source-level names (emission maps them)"
            );
            let right = program.values.get(args[1]).unwrap();
            match right.value {
                Value::Number(n) => assert_eq!(n, 6.0, "2 * 3 folds to 6"),
                ref other => panic!("right side should be the literal 6, got {other:?}"),
            }
        }
        other => panic!("expected an add call, got {other:?}"),
    }
    assert!(program.validate().is_ok());
}

#[test]
fn compat_profile_synthesizes_initializer_rule() {
    let mut program = arithmetic_program();
    run(&mut program, Profile::Compat).unwrap();

    // The initialize rule is first and clears the variable initializer.
    assert_eq!(
        program
            .rules
            .get(wright_ir::ids::Id::from_index(0))
            .unwrap()
            .name,
        "Initialize global variables"
    );
    let initialize = program
        .rules
        .get(wright_ir::ids::Id::from_index(0))
        .unwrap();
    assert_eq!(initialize.actions.len(), 1);
    assert!(matches!(
        program.actions.get(initialize.actions[0]),
        Some(Action::SetGlobalVariable { .. })
    ));
    assert!(
        program
            .global_variables
            .iter()
            .all(|variable| variable.initializer.is_none()),
        "initializers move into the synthetic rule"
    );
    assert!(program.validate().is_ok());
}

#[test]
fn folding_is_deterministic() {
    let mut first = arithmetic_program();
    let mut second = arithmetic_program();
    let first_results = run(&mut first, Profile::Compat).unwrap();
    let second_results = run(&mut second, Profile::Compat).unwrap();
    assert_eq!(first.dump(), second.dump());
    assert_eq!(first_results, second_results);
}

#[test]
fn aggressive_profile_uses_evidence_backed_passes_only() {
    let mut program = arithmetic_program();
    let results = run(&mut program, Profile::Aggressive).unwrap();
    assert_eq!(results.len(), 2, "aggressive = compat passes in v1");
    assert!(program.validate().is_ok());
}
