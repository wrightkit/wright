use workshop_rs::{Action, Event, Program, Rule, Value, Variable};
use wright_transform::profile::Profile;
use wright_transform::{run, run_validated};

fn arithmetic_program() -> Program {
    let mut program = Program::new();
    program.global_variable(Variable::with_index("points", 1));
    program.global_variable(Variable::with_index("result", 0));

    let len = Value::call("len", [Value::global_variable("points")]);
    let multiply = Value::call("*", [Value::number(2.0), Value::number(3.0)]);
    let add = Value::call("+", [len, multiply]);

    program.rule(
        Rule::new("compute", Event::Global).action(Action::SetGlobalVariable {
            variable: "result".to_string(),
            value: add,
        }),
    );
    program
}

#[test]
fn off_profile_performs_no_transformation() {
    let mut program = arithmetic_program();
    let before = program.dump();
    let results = run(&mut program, Profile::Off).unwrap();
    assert!(results.is_empty(), "off runs no passes");
    assert_eq!(program.dump(), before, "off leaves the program untouched");
    assert!(program.validate().is_ok());
}

#[test]
fn compat_profile_folds_constants_with_metrics() {
    let mut program = arithmetic_program();
    let results = run(&mut program, Profile::Compat).unwrap();
    assert_eq!(results.len(), 1, "one compat pass");

    let fold = results
        .iter()
        .find(|result| result.stats.pass == "fold-constants")
        .expect("fold-constants ran");
    assert!(fold.stats.changed >= 1, "changed: {}", fold.stats.changed);

    let rule = &program.rules[0];
    let Action::SetGlobalVariable { value, .. } = &rule.actions[0] else {
        panic!("expected Set Global Variable");
    };
    let Value::Call { name, args } = value else {
        panic!("expected Call");
    };
    assert_eq!(name, "+");
    assert_eq!(args.len(), 2);
    assert!(matches!(&args[1], Value::Number(num) if *num == 6.0));
    assert!(program.validate().is_ok());
}

#[test]
fn compat_profile_folds_canonical_square_root() {
    let mut program = arithmetic_program();
    let square_root = Value::call("squareRoot", [Value::number(2.0)]);
    program.rules[0].actions.push(Action::SetGlobalVariable {
        variable: "result".to_string(),
        value: square_root,
    });

    run(&mut program, Profile::Compat).unwrap();

    let Action::SetGlobalVariable { value, .. } = &program.rules[0].actions[1] else {
        panic!("expected Set Global Variable");
    };
    match value {
        Value::Number(value) => assert_eq!(*value, 2.0_f64.sqrt()),
        other => panic!("canonical squareRoot should fold to a number, got {other:?}"),
    }
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
fn validated_entry_preserves_output_and_pass_results() {
    let catalog = workshop_rs::catalog::Catalog::builtin().unwrap();
    let locale = workshop_rs::catalog::Locale::new("en-US");
    for profile in [Profile::Off, Profile::Compat] {
        let mut checked = arithmetic_program();
        let mut validated = checked.clone();
        validated.validate().unwrap();

        let expected = run(&mut checked, profile).unwrap();
        let actual = run_validated(&mut validated, profile).unwrap();

        assert_eq!(actual, expected, "{profile:?}");
        assert_eq!(
            workshop_rs::emitter::emit(&validated, &catalog, &locale).unwrap(),
            workshop_rs::emitter::emit(&checked, &catalog, &locale).unwrap(),
            "{profile:?}"
        );
        validated.validate().unwrap();
    }
}

#[test]
fn checked_entry_rejects_invalid_input_before_mutation() {
    for profile in [Profile::Off, Profile::Compat] {
        let mut program = arithmetic_program();
        program.global_variables.clear();
        let expected = program.validate().unwrap_err().to_string();
        let before = program.dump();

        let error = run(&mut program, profile).unwrap_err();

        assert_eq!(error.to_string(), expected, "{profile:?}");
        assert_eq!(program.dump(), before, "{profile:?} rejects before folding");
    }
}
