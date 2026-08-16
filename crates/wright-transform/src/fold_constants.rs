//! The `fold-constants` compat pass.
//!
//! Folds constant expressions in place: `2 * 3` → `6`, `-5` → the literal
//! `-5`, `sqrt(4)` → `2`, `1 < 2` → `True`, and boolean logic on literals.
//! This is exactly the arithmetic the pinned OverPy reference folds before
//! emission (evidence: the expressions-values oracle emits
//! `Add(Count Of(Global.points), 6)` for `len(points) + 2 * 3`, and the
//! overpy-cake oracle fully folds `sqrt`/arithmetic chains). The pass mutates
//! value nodes in place so callers keep their ids, and runs to a fixpoint so
//! nested constants fold transitively.

use workshop_rs::wir::{self, Value};

use crate::pipeline::{Pass, PassStats};

/// The `fold-constants` pass.
pub struct FoldConstants;

impl Pass for FoldConstants {
    fn name(&self) -> &'static str {
        "fold-constants"
    }

    fn run(&self, program: &mut wir::Program) -> PassStats {
        let nodes_before = program.values.len() + program.actions.len();
        let mut changed = 0usize;
        // Fold to a fixpoint so nested constant expressions collapse.
        loop {
            let mut iteration_changed = 0usize;
            for index in 0..program.values.len() {
                let id = wright_ir::ids::Id::from_index(index);
                let current = {
                    let Some(node) = program.values.get_mut(id) else {
                        continue;
                    };
                    std::mem::replace(&mut node.value, Value::Null)
                };
                match fold_one(program, &current) {
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
        // Dead (unreferenced) nodes remain in the arena; the emitted-resource
        // metric is measured on generated text, and the arena count is the
        // deterministic before/after evidence.
        PassStats {
            pass: self.name().to_string(),
            changed,
            nodes_before,
            nodes_after: program.values.len() + program.actions.len(),
        }
    }
}

/// Fold one value node; `None` when nothing folds. Read-only on the program.
fn fold_one(program: &wir::Program, value: &Value) -> Option<Value> {
    match &value {
        Value::Call { name, args } => {
            if args.len() == 2 {
                let left = number(program, args[0]);
                let right = number(program, args[1]);
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
                // Boolean logic on literal operands.
                if name == "and" || name == "or" {
                    if let (Some(left), Some(right)) =
                        (bool_value(program, args[0]), bool_value(program, args[1]))
                    {
                        return Some(Value::Bool(match name.as_str() {
                            "and" => left && right,
                            _ => left || right,
                        }));
                    }
                    // Domination rules the pinned OSTW reference folds
                    // (P4 evidence: `selector == 0 || ping()` inlines to
                    // `Or(Compare(...), True)` and folds to `True`):
                    // `x || true` → `True`, `x && false` → `False`.
                    if name == "or"
                        && (bool_value(program, args[0]) == Some(true)
                            || bool_value(program, args[1]) == Some(true))
                    {
                        return Some(Value::Bool(true));
                    }
                    if name == "and"
                        && (bool_value(program, args[0]) == Some(false)
                            || bool_value(program, args[1]) == Some(false))
                    {
                        return Some(Value::Bool(false));
                    }
                }
                // `cakePos[0]` folds to `First Of(cakePos)` (reference
                // emission evidence: overpy-cake oracle).
                if name == "valueInArray"
                    && args.len() == 2
                    && number(program, args[1]) == Some(0.0)
                {
                    return Some(Value::Call {
                        name: "firstOf".to_string(),
                        args: vec![args[0]],
                    });
                }
            }
            if args.len() == 1 {
                if let Some(operand) = number(program, args[0]) {
                    match name.as_str() {
                        "-" => return Some(folded_number(-operand)),
                        "sqrt" => return Some(folded_number(operand.sqrt())),
                        "abs" | "absoluteValue" => return Some(folded_number(operand.abs())),
                        _ => {}
                    }
                }
                if name == "not" {
                    if let Some(operand) = bool_value(program, args[0]) {
                        return Some(Value::Bool(!operand));
                    }
                }
            }
            None
        }
        // `vect(0, 1, 0)` folds to the canonical `Up` vector constant
        // (reference emission evidence: overpy-cake oracle).
        Value::Vector { x, y, z } => {
            let (Some(x), Some(y), Some(z)) = (
                number(program, *x),
                number(program, *y),
                number(program, *z),
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

/// A folded numeric literal carrying the formatted spelling (the reference
/// emits computed values with the same formatting as literal integers).
fn folded_number(value: f64) -> Value {
    Value::Number {
        value,
        text: wright_ir::format::format_number(value),
    }
}

/// The numeric value of a node, if it is a number literal.
fn number(program: &wir::Program, id: wright_ir::ids::Id<wir::ValueNode>) -> Option<f64> {
    match program.values.get(id)?.value {
        Value::Number { value, .. } => Some(value),
        _ => None,
    }
}

/// The boolean value of a node, if it is a boolean literal.
fn bool_value(program: &wir::Program, id: wright_ir::ids::Id<wir::ValueNode>) -> Option<bool> {
    match program.values.get(id)?.value {
        Value::Bool(value) => Some(value),
        _ => None,
    }
}
