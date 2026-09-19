//! Canonical transformation pipeline tests: `off` is a no-op, the compat
//! pass is evidence-backed with before/after metrics, and every pass leaves
//! the Program validated.

use workshop_rs::{Action, Event, Program, Rule, Value, Variable};
use wright_transform::profile::Profile;
use wright_transform::run;

fn single_action_program(value: Value) -> Program {
    let mut program = Program::new();
    program.global_variables.push(Variable::new("result"));
    program.rule(Rule {
        name: "test".to_string(),
        disabled: false,
        event: Event::Global,
        conditions: vec![],
        actions: vec![Action::SetGlobalVariable {
            variable: "result".to_string(),
            value,
        }],
    });
    program
}

fn arithmetic_program() -> Program {
    let mut program = single_action_program(Value::call(
        "+",
        [
            Value::call("len", [Value::global_variable("points")]),
            Value::call("*", [Value::Number(2.0), Value::Number(3.0)]),
        ],
    ));
    program.global_variables.push(Variable::new("points"));
    program
}

#[test]
fn off_profile_performs_no_transformation() {
    let mut program = arithmetic_program();
    let before = format!("{program:?}");
    let results = run(&mut program, Profile::Off).unwrap();
    assert!(results.is_empty(), "off runs no passes");
    assert_eq!(format!("{program:?}"), before);
    assert!(program.validate().is_ok());
}

#[test]
fn compat_profile_folds_constants_with_metrics() {
    let mut program = arithmetic_program();
    let results = run(&mut program, Profile::Compat).unwrap();
    assert_eq!(results.len(), 1, "one compat pass");

    let fold = results
        .iter()
        .find(|r| r.stats.pass == "fold-constants")
        .expect("fold-constants ran");
    assert!(fold.stats.changed >= 1);

    let Action::SetGlobalVariable { value, .. } = &program.rules[0].actions[0] else {
        panic!("expected Set Global Variable");
    };
    let Value::Call { name, args } = value else {
        panic!("expected an add call");
    };
    assert_eq!(name, "+");
    assert!(matches!(&args[1], Value::Number(n) if *n == 6.0));
    assert!(program.validate().is_ok());
}

#[test]
fn compat_profile_folds_canonical_square_root() {
    let mut program = single_action_program(Value::call("squareRoot", [Value::Number(2.0)]));
    run(&mut program, Profile::Compat).unwrap();
    let Action::SetGlobalVariable { value, .. } = &program.rules[0].actions[0] else {
        panic!("expected SetGlobalVariable");
    };
    assert!(matches!(value, Value::Number(n) if *n == 2.0_f64.sqrt()));
}

#[test]
fn folding_determinism_and_aggressive_profile() {
    let (mut first, mut second) = (arithmetic_program(), arithmetic_program());
    let r1 = run(&mut first, Profile::Compat).unwrap();
    let r2 = run(&mut second, Profile::Compat).unwrap();
    assert_eq!(format!("{first:?}"), format!("{second:?}"));
    assert_eq!(r1, r2);

    let mut agg = arithmetic_program();
    let results = run(&mut agg, Profile::Aggressive).unwrap();
    assert_eq!(results.len(), 1, "aggressive = compat passes in v1");
    assert!(agg.validate().is_ok());
}
