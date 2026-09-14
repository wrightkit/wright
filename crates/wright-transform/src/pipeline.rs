use workshop_rs::wir;

use crate::fold_constants::FoldConstants;
use crate::profile::Profile;

/// A transformation pass over validated WIR.
pub trait Pass {
    /// The stable pass name (reported in metrics and regression fixtures).
    fn name(&self) -> &'static str;

    /// Run the pass over the program and return its statistics.
    fn run(&self, program: &mut wir::Program) -> PassStats;
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
) -> Result<Vec<PassResult>, wright_ir::error::IrError> {
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
    let mut changed = 0;
    for rule in &mut program.rules {
        for condition in &mut rule.conditions {
            changed += changed_value(&mut condition.value);
        }
        for action in &mut rule.actions {
            changed += fold_action(action);
        }
    }
    program.validate()?;
    Ok(vec![PassResult {
        stats: PassStats {
            pass: "fold-constants".to_string(),
            changed,
            nodes_before: 0,
            nodes_after: 0,
        },
    }])
}

fn fold_action(action: &mut workshop_rs::Action) -> usize {
    use workshop_rs::Action;
    match action {
        Action::SetGlobalVariable { value, .. }
        | Action::ModifyGlobalVariable { value, .. }
        | Action::If { condition: value }
        | Action::ElseIf { condition: value }
        | Action::While { condition: value } => changed_value(value),
        Action::SetPlayerVariable { player, value, .. }
        | Action::ModifyPlayerVariable { player, value, .. } => {
            changed_value(player) + changed_value(value)
        }
        Action::AssignMember { target, value, .. } => changed_value(target) + changed_value(value),
        Action::ForGlobalVariable {
            start, stop, step, ..
        }
        | Action::ForPlayerVariable {
            start, stop, step, ..
        } => changed_value(start) + changed_value(stop) + changed_value(step),
        Action::Disabled { action } => fold_action(action),
        Action::Call { args, .. } => args.iter_mut().map(changed_value).sum(),
        Action::CallSubroutine { .. } | Action::Else | Action::End => 0,
    }
}

fn changed_value(value: &mut workshop_rs::Value) -> usize {
    usize::from(fold_value(value))
}

fn fold_value(value: &mut workshop_rs::Value) -> bool {
    use workshop_rs::Value;
    match value {
        Value::Array(values) => values.iter_mut().any(fold_value),
        Value::Vector { x, y, z } => fold_value(x) || fold_value(y) || fold_value(z),
        Value::PlayerVariable { player, .. } => fold_value(player),
        Value::Call { name, args } => {
            let mut changed = args.iter_mut().any(fold_value);
            if args.len() == 2 {
                if let (Value::Number(left), Value::Number(right)) = (&args[0], &args[1]) {
                    let result = match name.as_str() {
                        "+" | "add" => Some(left + right),
                        "-" | "subtract" => Some(left - right),
                        "*" | "multiply" => Some(left * right),
                        "/" | "divide" => Some(left / right),
                        _ => None,
                    };
                    if let Some(result) = result {
                        *value = Value::Number(result);
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
}
