//! The `synthesize-initializers` compat pass.
//!
//! Moves variable initializers into synthetic
//! "Initialize global variables" (event `global`) and "Initialize player
//! variables" (event `eachPlayer`) rules so compiled output matches the
//! reference frontend, which emits `Set Global Variable(name, initializer)`
//! and `Set Player Variable(Event Player, name, initializer)` for
//! array/vector/numeric initializers (evidence: the expressions-values
//! oracle emits the Initialize rule before the user rules, and the
//! declarations-numbers oracle emits the player Initialize rule after the
//! global one). The initializers are cleared on the variable tables, so
//! downstream consumers see one source of truth.

use wright_ir::ids::Id;
use wright_ir::wir::{self, Action, Event, Rule};

use crate::pipeline::{Pass, PassStats};

/// The `synthesize-initializers` pass.
pub struct SynthesizeInitializers;

impl Pass for SynthesizeInitializers {
    fn name(&self) -> &'static str {
        "synthesize-initializers"
    }

    fn run(&self, program: &mut wir::Program) -> PassStats {
        let nodes_before = program.values.len() + program.actions.len();
        let mut global_actions = Vec::new();
        let mut player_actions = Vec::new();

        for (position, variable) in program.global_variables.iter().enumerate() {
            if let Some(initializer) = variable.initializer {
                global_actions.push(program.actions.push(Action::SetGlobalVariable {
                    variable: Id::from_index(position),
                    value: initializer,
                    span: None,
                    target_span: None,
                }));
            }
        }
        for (position, variable) in program.player_variables.iter().enumerate() {
            if let Some(initializer) = variable.initializer {
                let player = program
                    .values
                    .push(wir::ValueNode::new(wir::Value::EventPlayer, None));
                player_actions.push(program.actions.push(Action::SetPlayerVariable {
                    player,
                    variable: Id::from_index(position),
                    value: initializer,
                    span: None,
                    target_span: None,
                }));
            }
        }

        let changed = global_actions.len() + player_actions.len();
        if !global_actions.is_empty() || !player_actions.is_empty() {
            // The initialize rules come first, matching the reference
            // emission. Rules do not reference each other, so rebuilding the
            // arena is safe (ids shift; no persistent ids exist here).
            let mut new_rules = wright_ir::arena::Arena::new();
            if !global_actions.is_empty() {
                new_rules.push(Rule {
                    name: "Initialize global variables".to_string(),
                    span: None,
                    name_span: None,
                    disabled: false,
                    event: Event::Global,
                    conditions: Vec::new(),
                    actions: global_actions,
                });
            }
            if !player_actions.is_empty() {
                new_rules.push(Rule {
                    name: "Initialize player variables".to_string(),
                    span: None,
                    name_span: None,
                    disabled: false,
                    event: Event::EachPlayer,
                    conditions: Vec::new(),
                    actions: player_actions,
                });
            }
            for rule in program.rules.iter().cloned() {
                new_rules.push(rule);
            }
            program.rules = new_rules;
        }

        for position in 0..program.global_variables.len() {
            let id = Id::from_index(position);
            if let Some(variable) = program.global_variables.get_mut(id) {
                variable.initializer = None;
            }
        }
        for position in 0..program.player_variables.len() {
            let id = Id::from_index(position);
            if let Some(variable) = program.player_variables.get_mut(id) {
                variable.initializer = None;
            }
        }

        PassStats {
            pass: self.name().to_string(),
            changed,
            nodes_before,
            nodes_after: program.values.len() + program.actions.len(),
        }
    }
}
