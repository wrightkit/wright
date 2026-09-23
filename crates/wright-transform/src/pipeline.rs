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
pub fn run(
    program: &mut workshop_rs::Program,
    profile: Profile,
) -> Result<Vec<PassResult>, workshop_rs::WorkshopError> {
    run_canonical(program, profile)
}

/// Run the semantics-preserving transform profile over the canonical public
/// Workshop program model.
pub fn run_canonical(
    program: &mut workshop_rs::Program,
    profile: Profile,
) -> Result<Vec<PassResult>, workshop_rs::WorkshopError> {
    program.validate()?;
    if profile == Profile::Off {
        return Ok(Vec::new());
    }
    let nodes_before = program_node_count(program);
    let mut changed = 0;
    loop {
        let iteration_changed = program
            .rules
            .iter_mut()
            .map(|rule| {
                rule.conditions
                    .iter_mut()
                    .map(|condition| usize::from(fold_value_once(&mut condition.value)))
                    .sum::<usize>()
                    + rule.actions.iter_mut().map(fold_action_once).sum::<usize>()
            })
            .sum::<usize>();
        if iteration_changed == 0 {
            break;
        }
        changed += iteration_changed;
    }
    program.validate()?;
    Ok(vec![PassResult {
        stats: PassStats {
            pass: "fold-constants".to_string(),
            changed,
            nodes_before,
            nodes_after: program_node_count(program),
        },
    }])
}

fn fold_action_once(action: &mut workshop_rs::Action) -> usize {
    use workshop_rs::Action;
    match action {
        Action::SetGlobalVariable { value, .. }
        | Action::ModifyGlobalVariable { value, .. }
        | Action::If { condition: value }
        | Action::ElseIf { condition: value }
        | Action::While { condition: value } => usize::from(fold_value_once(value)),
        Action::SetPlayerVariable { player, value, .. }
        | Action::ModifyPlayerVariable { player, value, .. } => {
            usize::from(fold_value_once(player)) + usize::from(fold_value_once(value))
        }
        Action::AssignMember { target, value, .. } => {
            usize::from(fold_value_once(target)) + usize::from(fold_value_once(value))
        }
        Action::ForGlobalVariable {
            start, stop, step, ..
        } => {
            usize::from(fold_value_once(start))
                + usize::from(fold_value_once(stop))
                + usize::from(fold_value_once(step))
        }
        Action::ForPlayerVariable {
            player,
            start,
            stop,
            step,
            ..
        } => {
            usize::from(fold_value_once(player))
                + usize::from(fold_value_once(start))
                + usize::from(fold_value_once(stop))
                + usize::from(fold_value_once(step))
        }
        Action::Disabled { action } => fold_action_once(action),
        Action::Call { args, .. } => args
            .iter_mut()
            .map(|value| usize::from(fold_value_once(value)))
            .sum(),
        Action::CallSubroutine { .. } | Action::Else | Action::End => 0,
    }
}

/// Fold one tree level. The caller repeats this pass to a fixpoint so a
/// parent sees values produced by a previous pass, matching the WIR pass.
fn fold_value_once(value: &mut workshop_rs::Value) -> bool {
    use workshop_rs::Value;
    match value {
        Value::Array(values) => {
            values
                .iter_mut()
                .map(|value| usize::from(fold_value_once(value)))
                .sum::<usize>()
                > 0
        }
        Value::Vector { x, y, z } => {
            let child_changed = usize::from(fold_value_once(x))
                + usize::from(fold_value_once(y))
                + usize::from(fold_value_once(z));
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
        Value::PlayerVariable { player, .. } => fold_value_once(player),
        Value::Call { name, args } => {
            let mut changed = args
                .iter_mut()
                .map(|value| usize::from(fold_value_once(value)))
                .sum::<usize>()
                > 0;
            if args.len() == 2 {
                if let (Value::Number(left), Value::Number(right)) = (&args[0], &args[1]) {
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
                    };
                    if let Some(result) = result {
                        *value = if matches!(name.as_str(), "==" | "!=" | "<" | "<=" | ">" | ">=") {
                            Value::Bool(result != 0.0)
                        } else {
                            Value::Number(result)
                        };
                        changed = true;
                    }
                } else {
                    let boolean_result = match (&args[0], &args[1]) {
                        (Value::Bool(left), Value::Bool(right)) if name == "and" => {
                            Some(*left && *right)
                        }
                        (Value::Bool(left), Value::Bool(right)) if name == "or" => {
                            Some(*left || *right)
                        }
                        (Value::Bool(true), _) if name == "or" => Some(true),
                        (_, Value::Bool(true)) if name == "or" => Some(true),
                        (Value::Bool(false), _) if name == "and" => Some(false),
                        (_, Value::Bool(false)) if name == "and" => Some(false),
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
                    };
                    if let Some(result) = result {
                        *value = Value::Number(result);
                        changed = true;
                    }
                } else if name == "not" {
                    if let Value::Bool(operand) = args[0] {
                        *value = Value::Bool(!operand);
                        changed = true;
                    }
                }
            }
            changed
        }
        _ => false,
    }
}

fn program_node_count(program: &workshop_rs::Program) -> usize {
    program
        .rules
        .iter()
        .map(|rule| {
            rule.conditions
                .iter()
                .map(|condition| value_node_count(&condition.value))
                .sum::<usize>()
                + rule.actions.iter().map(action_node_count).sum::<usize>()
        })
        .sum()
}

fn action_node_count(action: &workshop_rs::Action) -> usize {
    use workshop_rs::Action;
    1 + match action {
        Action::SetGlobalVariable { value, .. }
        | Action::ModifyGlobalVariable { value, .. }
        | Action::If { condition: value }
        | Action::ElseIf { condition: value }
        | Action::While { condition: value } => value_node_count(value),
        Action::SetPlayerVariable { player, value, .. }
        | Action::ModifyPlayerVariable { player, value, .. } => {
            value_node_count(player) + value_node_count(value)
        }
        Action::AssignMember { target, value, .. } => {
            value_node_count(target) + value_node_count(value)
        }
        Action::ForGlobalVariable {
            start, stop, step, ..
        } => value_node_count(start) + value_node_count(stop) + value_node_count(step),
        Action::ForPlayerVariable {
            player,
            start,
            stop,
            step,
            ..
        } => {
            value_node_count(player)
                + value_node_count(start)
                + value_node_count(stop)
                + value_node_count(step)
        }
        Action::Disabled { action } => action_node_count(action),
        Action::Call { args, .. } => args.iter().map(value_node_count).sum(),
        Action::CallSubroutine { .. } | Action::Else | Action::End => 0,
    }
}

fn value_node_count(value: &workshop_rs::Value) -> usize {
    use workshop_rs::Value;
    1 + match value {
        Value::Array(values) => values.iter().map(value_node_count).sum(),
        Value::Vector { x, y, z } => {
            value_node_count(x) + value_node_count(y) + value_node_count(z)
        }
        Value::PlayerVariable { player, .. } => value_node_count(player),
        Value::Call { args, .. } => args.iter().map(value_node_count).sum(),
        _ => 0,
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
        for profile in [Profile::Off, Profile::Compat, Profile::Aggressive] {
            let mut program = program_with_settings();
            run(&mut program, profile).expect("pipeline runs");
            assert!(
                program.settings.is_some(),
                "{profile:?} must preserve the settings carrier"
            );
        }
    }

    #[test]
    fn canonical_pipeline_folds_every_value_sibling() {
        use workshop_rs::{Action, Condition, Event, Program, Rule, Value};

        let mut program = Program::new();
        program.rule(Rule {
            name: "r".to_string(),
            disabled: false,
            event: Event::Global,
            conditions: vec![Condition::new(Value::call(
                "+",
                [Value::call("*", [2.0.into(), 3.0.into()]), 4.0.into()],
            ))],
            actions: vec![Action::Call {
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
            }],
        });

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
