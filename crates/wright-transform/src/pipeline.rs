use workshop_rs::format::format_number;
use workshop_rs::ids::Id;
use workshop_rs::wir::{self, Value};

use crate::fold_constants::FoldConstants;
use crate::profile::Profile;

/// A transformation pass over validated WIR.
pub trait Pass {
    /// The stable pass name (reported in metrics and regression fixtures).
    fn name(&self) -> &'static str;

    /// Run the pass over the program and return its statistics.
    fn run(&self, program: &mut wir::Program) -> PassStats;
}

/// The `fold-constants` pass.
impl Pass for FoldConstants {
    fn name(&self) -> &'static str {
        "fold-constants"
    }

    fn run(&self, program: &mut wir::Program) -> PassStats {
        let nodes_before = program.values.len() + program.actions.len();
        let mut changed = 0usize;
        loop {
            let mut iteration_changed = 0usize;
            for index in 0..program.values.len() {
                let id = Id::from_index(index);
                let current = {
                    let Some(node) = program.values.get_mut(id) else {
                        continue;
                    };
                    std::mem::replace(&mut node.value, Value::Null)
                };
                match fold_wir_one(program, &current) {
                    Some(folded) => {
                        program.values.get_mut(id).expect("id in range").value = folded;
                        iteration_changed += 1;
                    }
                    None => {
                        program.values.get_mut(id).expect("id in range").value = current;
                    }
                }
            }
            if iteration_changed == 0 {
                break;
            }
            changed += iteration_changed;
        }
        PassStats {
            pass: self.name().to_string(),
            changed,
            nodes_before,
            nodes_after: program.values.len() + program.actions.len(),
        }
    }
}

fn fold_wir_one(program: &wir::Program, value: &Value) -> Option<Value> {
    match value {
        Value::Call { name, args } => {
            if args.len() == 2 {
                let left = wir_number(program, args[0]);
                let right = wir_number(program, args[1]);
                if let (Some(left), Some(right)) = (left, right) {
                    let folded = match name.as_str() {
                        "+" | "add" => Some(folded_number(left + right)),
                        "-" | "subtract" => Some(folded_number(left - right)),
                        "*" | "multiply" => Some(folded_number(left * right)),
                        "/" | "divide" => Some(folded_number(left / right)),
                        "==" => Some(Value::Bool(left == right)),
                        "!=" => Some(Value::Bool(left != right)),
                        "<" => Some(Value::Bool(left < right)),
                        "<=" => Some(Value::Bool(left <= right)),
                        ">" => Some(Value::Bool(left > right)),
                        ">=" => Some(Value::Bool(left >= right)),
                        _ => None,
                    };
                    if folded.is_some() {
                        return folded;
                    }
                }
                if name == "and" || name == "or" {
                    if let (Some(left), Some(right)) =
                        (wir_bool(program, args[0]), wir_bool(program, args[1]))
                    {
                        return Some(Value::Bool(if name == "and" {
                            left && right
                        } else {
                            left || right
                        }));
                    }
                    if name == "or"
                        && (wir_bool(program, args[0]) == Some(true)
                            || wir_bool(program, args[1]) == Some(true))
                    {
                        return Some(Value::Bool(true));
                    }
                    if name == "and"
                        && (wir_bool(program, args[0]) == Some(false)
                            || wir_bool(program, args[1]) == Some(false))
                    {
                        return Some(Value::Bool(false));
                    }
                }
                if name == "valueInArray"
                    && args.len() == 2
                    && wir_number(program, args[1]) == Some(0.0)
                {
                    return Some(Value::Call {
                        name: "firstOf".to_string(),
                        args: vec![args[0]],
                    });
                }
            }
            if args.len() == 1 {
                if let Some(operand) = wir_number(program, args[0]) {
                    match name.as_str() {
                        "-" => return Some(folded_number(-operand)),
                        "sqrt" | "squareRoot" => return Some(folded_number(operand.sqrt())),
                        "abs" | "absoluteValue" => return Some(folded_number(operand.abs())),
                        _ => {}
                    }
                }
                if name == "not" {
                    if let Some(operand) = wir_bool(program, args[0]) {
                        return Some(Value::Bool(!operand));
                    }
                }
            }
            None
        }
        Value::Vector { x, y, z } => {
            let (Some(x), Some(y), Some(z)) = (
                wir_number(program, *x),
                wir_number(program, *y),
                wir_number(program, *z),
            ) else {
                return None;
            };
            if x == 0.0 && y == 1.0 && z == 0.0 {
                Some(Value::Enum {
                    value_type: "Vector".to_string(),
                    value: "UP".to_string(),
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

fn folded_number(value: f64) -> Value {
    Value::Number {
        value,
        text: format_number(value),
    }
}

fn wir_number(program: &wir::Program, id: Id<wir::ValueNode>) -> Option<f64> {
    match program.values.get(id)?.value {
        Value::Number { value, .. } => Some(value),
        _ => None,
    }
}

fn wir_bool(program: &wir::Program, id: Id<wir::ValueNode>) -> Option<bool> {
    match program.values.get(id)?.value {
        Value::Bool(value) => Some(value),
        _ => None,
    }
}

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

/// Run the pipeline for a profile over a validated program.
///
/// The program is validated before and after the pipeline; `Err` is returned
/// if the input was invalid or a pass left it invalid (a pass bug).
///
/// Source-semantic behavior (declaration initializers) is owned by the
/// profile-independent HIR → WIR lowering and never appears in this pass
/// pipeline (#112): profiles may only change semantics-preserving
/// representation/resource behavior.
pub fn run(
    program: &mut wir::Program,
    profile: Profile,
) -> Result<Vec<PassResult>, workshop_rs::wir::error::IrError> {
    program.validate()?;
    let passes: Vec<Box<dyn Pass>> = match profile {
        Profile::Off => Vec::new(),
        Profile::Compat | Profile::Aggressive => vec![Box::new(FoldConstants)],
    };
    let mut results = Vec::new();
    for pass in passes {
        let stats = pass.run(program);
        program.validate()?;
        results.push(PassResult { stats });
    }
    Ok(results)
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
    use workshop_rs::Action::*;
    let mut changed = 0;
    match action {
        SetGlobalVariable { value, .. }
        | ModifyGlobalVariable { value, .. }
        | If { condition: value }
        | ElseIf { condition: value }
        | While { condition: value } => changed += usize::from(fold_value_once(value)),
        SetPlayerVariable { player, value, .. }
        | ModifyPlayerVariable { player, value, .. }
        | AssignMember {
            target: player,
            value,
            ..
        } => {
            changed += usize::from(fold_value_once(player)) + usize::from(fold_value_once(value));
        }
        ForGlobalVariable {
            start, stop, step, ..
        } => {
            changed += usize::from(fold_value_once(start))
                + usize::from(fold_value_once(stop))
                + usize::from(fold_value_once(step));
        }
        ForPlayerVariable {
            player,
            start,
            stop,
            step,
            ..
        } => {
            changed += usize::from(fold_value_once(player))
                + usize::from(fold_value_once(start))
                + usize::from(fold_value_once(stop))
                + usize::from(fold_value_once(step));
        }
        Disabled { action } => changed += fold_action_once(action),
        Call { args, .. } => {
            changed += args
                .iter_mut()
                .map(|v| usize::from(fold_value_once(v)))
                .sum::<usize>()
        }
        CallSubroutine { .. } | Else | End => {}
    }
    changed
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

    fn program_with_settings() -> wir::Program {
        wir::Program {
            settings: Some(Settings {
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
            }),
            ..wir::Program::default()
        }
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

    #[test]
    fn canonical_pipeline_preserves_action_node_counts() {
        use workshop_rs::{Action, Event, Program, Rule, Value};

        let mut program = Program::new();
        program.rule(Rule {
            name: "counts".to_string(),
            disabled: false,
            event: Event::Global,
            conditions: vec![],
            actions: vec![
                Action::CallSubroutine {
                    subroutine: "sub".to_string(),
                },
                Action::Else,
                Action::End,
                Action::disabled(Action::SetGlobalVariable {
                    variable: "A".to_string(),
                    value: Value::Number(1.0),
                }),
                Action::ForPlayerVariable {
                    player: Value::Number(1.0),
                    variable: "A".to_string(),
                    start: Value::Number(1.0),
                    stop: Value::Number(2.0),
                    step: Value::Number(1.0),
                },
            ],
        });

        assert_eq!(program_node_count(&program), 11);
    }
}
