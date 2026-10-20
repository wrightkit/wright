use crate::profile::Profile;

/// Statistics for one pass run.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PassStats {
    /// The stable pass name.
    pub pass: String,
    /// The number of nodes this pass rewrote.
    pub changed: usize,
    /// Total value/action nodes before the pass.
    pub nodes_before: usize,
    /// Total value/action nodes after the pass.
    pub nodes_after: usize,
}

/// One recorded pass run.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PassResult {
    pub stats: PassStats,
}

/// Run the semantics-preserving transform profile over the canonical public
/// Workshop program model.
#[hotpath::measure]
pub fn run(
    program: &mut workshop_rs::Program,
    profile: Profile,
) -> Result<Vec<PassResult>, workshop_rs::WorkshopError> {
    hotpath::measure_block!("transform::validate_pre", program.validate())?;
    run_validated(program, profile)
}

/// Run a transform profile on an already-validated canonical program.
///
/// The caller must successfully call [`workshop_rs::Program::validate`] after
/// the last mutation before invoking this function. Input validation is skipped;
/// non-off profiles still validate the program after the transform fixpoint.
#[hotpath::measure]
pub fn run_validated(
    program: &mut workshop_rs::Program,
    profile: Profile,
) -> Result<Vec<PassResult>, workshop_rs::WorkshopError> {
    if profile == Profile::Off {
        return Ok(Vec::new());
    }
    let mut changed = 0;
    let mut first_iteration = true;
    let mut nodes_before = 0;
    let nodes_after = hotpath::measure_block!("transform::fold_to_fixpoint", {
        loop {
            let mut nodes = 0;
            let iteration_changed = program
                .rules
                .iter_mut()
                .map(|rule| {
                    rule.conditions
                        .iter_mut()
                        .map(|condition| {
                            usize::from(fold_value_once(&mut condition.value, &mut nodes))
                        })
                        .sum::<usize>()
                        + rule
                            .actions
                            .iter_mut()
                            .map(|action| fold_action_once(action, &mut nodes))
                            .sum::<usize>()
                })
                .sum::<usize>();
            if first_iteration {
                nodes_before = nodes;
                first_iteration = false;
            }
            if iteration_changed == 0 {
                break nodes;
            }
            changed += iteration_changed;
        }
    });
    hotpath::measure_block!("transform::validate_post", program.validate())?;
    Ok(vec![PassResult {
        stats: PassStats {
            pass: "fold-constants".to_string(),
            changed,
            nodes_before,
            nodes_after,
        },
    }])
}

pub use self::run as run_canonical;

/// Whether a value contains nothing but literals — the condition under
/// which a short-circuit fold may discard it. `GlobalVariable`,
/// `PlayerVariable`, `Subroutine`, `EventPlayer`, and `Call` all carry
/// identifier occurrences (or nested operands that may), so dropping them
/// would remove occurrences semantic services index (#639).
fn literal_only(value: &workshop_rs::Value) -> bool {
    use workshop_rs::Value;
    match value {
        Value::Number(_)
        | Value::String(_)
        | Value::LocalizedString(_)
        | Value::Bool(_)
        | Value::Null
        | Value::Enum { .. } => true,
        Value::Array(values) => values.iter().all(literal_only),
        Value::Vector { x, y, z } => literal_only(x) && literal_only(y) && literal_only(z),
        Value::GlobalVariable(_)
        | Value::PlayerVariable { .. }
        | Value::Subroutine(_)
        | Value::EventPlayer
        | Value::Call { .. } => false,
    }
}

fn fold_action_once(action: &mut workshop_rs::Action, nodes: &mut usize) -> usize {
    *nodes += 1;
    use workshop_rs::Action;
    match action {
        Action::SetGlobalVariable { value, .. }
        | Action::ModifyGlobalVariable { value, .. }
        | Action::If { condition: value }
        | Action::ElseIf { condition: value }
        | Action::While { condition: value } => usize::from(fold_value_once(value, nodes)),
        Action::SetPlayerVariable { player, value, .. }
        | Action::ModifyPlayerVariable { player, value, .. } => {
            usize::from(fold_value_once(player, nodes)) + usize::from(fold_value_once(value, nodes))
        }
        Action::AssignMember { target, value, .. } => {
            usize::from(fold_value_once(target, nodes)) + usize::from(fold_value_once(value, nodes))
        }
        Action::ForGlobalVariable {
            start, stop, step, ..
        } => {
            usize::from(fold_value_once(start, nodes))
                + usize::from(fold_value_once(stop, nodes))
                + usize::from(fold_value_once(step, nodes))
        }
        Action::ForPlayerVariable {
            player,
            start,
            stop,
            step,
            ..
        } => {
            usize::from(fold_value_once(player, nodes))
                + usize::from(fold_value_once(start, nodes))
                + usize::from(fold_value_once(stop, nodes))
                + usize::from(fold_value_once(step, nodes))
        }
        Action::Disabled { action } => fold_action_once(action, nodes),
        Action::Call { args, .. } => args
            .iter_mut()
            .map(|value| usize::from(fold_value_once(value, nodes)))
            .sum(),
        Action::CallSubroutine { .. } | Action::Else | Action::End => 0,
    }
}

/// Fold one tree level. The caller repeats this pass to a fixpoint so a
/// parent sees values produced by a previous pass, matching the WIR pass.
fn fold_value_once(value: &mut workshop_rs::Value, nodes: &mut usize) -> bool {
    *nodes += 1;
    use workshop_rs::Value;
    match value {
        Value::Array(values) => {
            values
                .iter_mut()
                .map(|value| usize::from(fold_value_once(value, nodes)))
                .sum::<usize>()
                > 0
        }
        Value::Vector { x, y, z } => {
            let child_changed = usize::from(fold_value_once(x, nodes))
                + usize::from(fold_value_once(y, nodes))
                + usize::from(fold_value_once(z, nodes));
            let is_up = matches!((&**x, &**y, &**z), (
                Value::Number(x),
                Value::Number(y),
                Value::Number(z)
            ) if *x == 0.0 && *y == 1.0 && *z == 0.0);
            if is_up {
                *value = Value::Enum {
                    value_type: "Vector".to_string(),
                    value: "UP".to_string(),
                };
                true
            } else {
                child_changed > 0
            }
        }
        Value::PlayerVariable { player, .. } => fold_value_once(player, nodes),
        Value::Call { name, args } => {
            let mut changed = args
                .iter_mut()
                .map(|value| usize::from(fold_value_once(value, nodes)))
                .sum::<usize>()
                > 0;
            if args.len() == 2 {
                if let (Value::Number(left), Value::Number(right)) = (&args[0], &args[1]) {
                    // Folding must not turn a valid program into an invalid
                    // one: `Divide(1, 0)` and friends produce NaN/infinity,
                    // which post-validation rejects as non-canonical — leave
                    // the call unfolded so the profile keeps the input's
                    // pass/fail result (#639).
                    let result = match name.as_str() {
                        "+" | "add" => Some(left + right),
                        "-" | "subtract" => Some(left - right),
                        "*" | "multiply" => Some(left * right),
                        "/" | "divide" => Some(left / right),
                        "==" => Some(if left == right { 1.0 } else { 0.0 }),
                        "!=" => Some(if left != right { 1.0 } else { 0.0 }),
                        "<" => Some(if left < right { 1.0 } else { 0.0 }),
                        "<=" => Some(if left <= right { 1.0 } else { 0.0 }),
                        ">" => Some(if left > right { 1.0 } else { 0.0 }),
                        ">=" => Some(if left >= right { 1.0 } else { 0.0 }),
                        _ => None,
                    }
                    .filter(|result| result.is_finite());
                    if let Some(result) = result {
                        *value = if matches!(name.as_str(), "==" | "!=" | "<" | "<=" | ">" | ">=") {
                            Value::Bool(result != 0.0)
                        } else {
                            Value::Number(result)
                        };
                        changed = true;
                    }
                } else {
                    // The short-circuit folds discard the other operand
                    // entirely — apply them only when that operand is
                    // literal-only, so a folded `Or(True, Global.g)` cannot
                    // drop the identifier occurrence `references` indexes
                    // (#639).
                    let boolean_result = match (&args[0], &args[1]) {
                        (Value::Bool(left), Value::Bool(right)) if name == "and" => {
                            Some(*left && *right)
                        }
                        (Value::Bool(left), Value::Bool(right)) if name == "or" => {
                            Some(*left || *right)
                        }
                        (Value::Bool(true), other) | (other, Value::Bool(true))
                            if name == "or" && literal_only(other) =>
                        {
                            Some(true)
                        }
                        (Value::Bool(false), other) | (other, Value::Bool(false))
                            if name == "and" && literal_only(other) =>
                        {
                            Some(false)
                        }
                        _ => None,
                    };
                    if let Some(result) = boolean_result {
                        *value = Value::Bool(result);
                        changed = true;
                    } else if name == "valueInArray"
                        && matches!(&args[1], Value::Number(index) if *index == 0.0)
                    {
                        *value = Value::Call {
                            name: "firstOf".to_string(),
                            args: vec![args[0].clone()],
                        };
                        changed = true;
                    }
                }
            } else if args.len() == 1 {
                if let Value::Number(number) = args[0] {
                    let result = match name.as_str() {
                        "-" => Some(-number),
                        "sqrt" | "squareRoot" => Some(number.sqrt()),
                        "abs" | "absoluteValue" => Some(number.abs()),
                        _ => None,
                    }
                    .filter(|result| result.is_finite());
                    if let Some(result) = result {
                        *value = Value::Number(result);
                        changed = true;
                    }
                } else if name == "not"
                    && let Value::Bool(operand) = args[0]
                {
                    *value = Value::Bool(!operand);
                    changed = true;
                }
            }
            changed
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workshop_rs::settings::{Settings, SettingsNode};

    fn program_with_settings() -> workshop_rs::Program {
        let mut program = workshop_rs::Program::new();
        program.settings = Some(Settings {
            span: None,
            children: vec![SettingsNode::Group {
                name: "gamemodes".to_string(),
                children: vec![SettingsNode::Group {
                    name: "skirmish".to_string(),
                    children: vec![SettingsNode::List {
                        name: "enabledMaps".to_string(),
                        elements: vec![],
                        span: None,
                    }],
                    span: None,
                }],
                span: None,
            }],
        });
        program
    }

    #[test]
    fn settings_carrier_survives_every_profile() {
        for profile in [Profile::Off, Profile::Compat] {
            let mut program = program_with_settings();
            run(&mut program, profile).expect("pipeline runs");
            assert!(
                program.settings.is_some(),
                "{profile:?} must preserve the settings carrier"
            );
        }
    }

    /// #639: folding must not turn a valid program into an invalid one —
    /// `Divide(1, 0)` produces infinity and `sqrt(-1)` produces NaN, both
    /// rejected by post-validation, so non-finite fold results stay
    /// unfolded and the profile preserves the input's pass/fail result.
    #[test]
    fn non_finite_fold_results_stay_unfolded() {
        use workshop_rs::{Action, Event, Program, Rule, Value, Variable};

        let mut program = Program::new();
        program.global_variable(Variable::with_index("a", 0));
        program.rule(
            Rule::new("r", Event::Global)
                .action(Action::SetGlobalVariable {
                    variable: "a".to_string(),
                    value: Value::call("divide", [1.0.into(), 0.0.into()]),
                })
                .action(Action::SetGlobalVariable {
                    variable: "a".to_string(),
                    value: Value::call("sqrt", [(-1.0).into()]),
                }),
        );

        run_canonical(&mut program, Profile::Compat).expect("pipeline runs");
        for action in &program.rules[0].actions {
            let Action::SetGlobalVariable { value, .. } = action else {
                panic!("set-global action")
            };
            assert!(
                matches!(value, Value::Call { .. }),
                "a non-finite fold result must not be emitted: {value:?}"
            );
        }
    }

    /// #639: the short-circuit folds discard the other operand — they apply
    /// only when that operand is literal-only, so a folded `Or(True,
    /// Global.g)` cannot drop the identifier occurrence semantic services
    /// index, while a literal operand still folds.
    #[test]
    fn short_circuit_folds_drop_only_literal_operands() {
        use workshop_rs::{Action, Condition, Event, Program, Rule, Value, Variable};

        let fold_one = |value: Value| {
            let mut program = Program::new();
            program.global_variable(Variable::with_index("g", 0));
            program.rule(
                Rule::new("r", Event::Global)
                    .condition(Condition::new(value))
                    .action(Action::Call {
                        name: "probe".to_string(),
                        args: Vec::new(),
                    }),
            );
            run_canonical(&mut program, Profile::Compat).expect("pipeline runs");
            program.rules[0].conditions[0].value.clone()
        };
        let global_g = || Value::GlobalVariable("g".to_string());

        for value in [
            Value::call("or", [true.into(), global_g()]),
            Value::call("or", [global_g(), true.into()]),
            Value::call("and", [false.into(), global_g()]),
            Value::call("and", [global_g(), false.into()]),
            Value::call("or", [true.into(), Value::call("firstOf", [global_g()])]),
        ] {
            assert!(
                matches!(fold_one(value), Value::Call { .. }),
                "an occurrence-carrying operand must not be folded away"
            );
        }

        for value in [
            Value::call("or", [true.into(), 0.0.into()]),
            Value::call(
                "or",
                [Value::Array(vec![1.0.into(), "x".into()]), true.into()],
            ),
            Value::call("and", [false.into(), 0.0.into()]),
            Value::call("and", [false.into(), false.into()]),
            Value::call("or", [false.into(), true.into()]),
        ] {
            assert!(
                matches!(fold_one(value), Value::Bool(_)),
                "a literal-only operand still folds"
            );
        }
    }

    #[test]
    fn canonical_pipeline_folds_every_value_sibling() {
        use workshop_rs::{Action, Condition, Event, Program, Rule, Value};

        let mut program = Program::new();
        program.rule(
            Rule::new("r", Event::Global)
                .condition(Condition::new(Value::call(
                    "+",
                    [Value::call("*", [2.0.into(), 3.0.into()]), 4.0.into()],
                )))
                .action(Action::Call {
                    name: "probe".to_string(),
                    args: vec![
                        Value::Array(vec![
                            Value::call("+", [1.0.into(), 2.0.into()]),
                            Value::call("==", [4.0.into(), 4.0.into()]),
                        ]),
                        Value::call("and", [true.into(), false.into()]),
                        Value::call(
                            "valueInArray",
                            [
                                Value::Array(vec![1.0.into(), 2.0.into()]),
                                Value::call("+", [0.0.into(), 0.0.into()]),
                            ],
                        ),
                        Value::Vector {
                            x: Box::new(0.0.into()),
                            y: Box::new(1.0.into()),
                            z: Box::new(0.0.into()),
                        },
                    ],
                }),
        );

        let results = run_canonical(&mut program, Profile::Compat).expect("canonical pipeline");
        assert!(results[0].stats.changed > 0);
        assert!(matches!(
            program.rules[0].conditions[0].value,
            Value::Number(value) if value == 10.0
        ));
        let Action::Call { args, .. } = &program.rules[0].actions[0] else {
            panic!("probe action")
        };
        assert!(matches!(
            &args[0],
            Value::Array(values) if matches!(&values[0], Value::Number(value) if *value == 3.0)
                && matches!(&values[1], Value::Bool(true))
        ));
        assert!(matches!(&args[1], Value::Bool(false)));
        assert!(matches!(
            &args[2],
            Value::Call { name, args } if name == "firstOf" && args.len() == 1
        ));
        assert!(matches!(
            &args[3],
            Value::Enum { value_type, value } if value_type == "Vector" && value == "UP"
        ));
    }
}
