//! WIR → OSTW reconstruction suite (#125).
//!
//! For every committed reconstruction fixture under
//! `compatibility/ostw/reconstruction/`, the full loop
//!
//! ```text
//! Workshop text → shared parser → WIR → reconstruct → OSTW text
//!   → native wright-ostw frontend (generated ds.toml project root) → HIR
//!   → shared lowering → WIR → shared Workshop emitter → Workshop text
//! ```
//!
//! must hold: the reconstructed OSTW loads through the native frontend with
//! zero diagnostics, the reconstructed Workshop is semantically equivalent
//! to the original under the declared #119 normalization, and the
//! reconstructed Workshop text reparses and re-emits byte-identically
//! (round-trip fixed point). The normalization is applied identically to
//! both sides, exactly like the forward differential
//! (`crates/wright-ostw/tests/differential.rs`).
//!
//! The machine-readable report lands at
//! `target/wright-ostw-reconstruct-report.json` (the repo report pattern).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sha2::Digest as _;
use workshop_rs::wir::{self, Action, Event, Value};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// Parse Workshop text through the shared parser with the canonical
/// signature context (the same path the driver uses).
fn parse(catalog: &wright_workshop::catalog::Catalog, text: &str) -> wir::Program {
    let manifest =
        wright_opy::manifest::Manifest::builtin().expect("the OPY manifest is embedded and valid");
    let context = wright_core::signatures::ChainedExpectedDomain::new(manifest, catalog);
    let program = wright_workshop::parser::parse_with_context(
        text,
        catalog,
        &wright_workshop::catalog::Locale::new("en-US"),
        &context,
    )
    .unwrap_or_else(|error| panic!("fixture Workshop text must parse: {error}"));
    program
        .validate()
        .expect("parsed programs validate structurally");
    program
}

/// Load the reconstructed OSTW through the native frontend in a generated
/// project root (`ds.toml` + `main.ostw`). Returns the HIR.
fn compile_reconstructed_ostw(ostw_text: &str, test_name: &str) -> wright_ir::hir::Program {
    let root = std::env::temp_dir().join(format!("wright-ostw-reconstruct-{test_name}"));
    std::fs::create_dir_all(&root).expect("create project root");
    std::fs::write(root.join("ds.toml"), "entry_point=\"main.ostw\"\n").expect("write ds.toml");
    std::fs::write(root.join("main.ostw"), ostw_text).expect("write main.ostw");
    let (outcome, semantic) =
        wright_ostw::compile_with_semantics(ostw_text, Some("main.ostw"), &root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        outcome.error.is_none(),
        "reconstructed project must load: {:?}",
        outcome.error
    );
    assert!(
        outcome.diagnostics.is_empty(),
        "reconstructed project must parse cleanly: {:?}",
        outcome.diagnostics
    );
    assert!(
        semantic.diagnostics.is_empty(),
        "reconstructed OSTW must resolve cleanly: {:?}",
        semantic.diagnostics
    );
    semantic.hir.expect("HIR produced")
}

fn fold(program: &mut wir::Program) {
    use wright_transform::pipeline::Pass as _;
    wright_transform::fold_constants::FoldConstants.run(program);
}

/// Inline write-once per-call player variables (declared #119 contract; the
/// reference materializes void-function arguments this way). Applied
/// identically to both sides.
fn inline_write_once_player_vars(program: &mut wir::Program) {
    for rule_index in 0..program.rules.len() {
        let rule_id = wright_ir::ids::Id::from_index(rule_index);
        let actions: Vec<wir::ActionId> = program
            .rules
            .get(rule_id)
            .map(|rule| rule.actions.clone())
            .unwrap_or_default();
        let mut writers: HashMap<u32, (usize, wir::ValueId)> = HashMap::new();
        for (index, action) in actions.iter().enumerate() {
            let Some(wir::Action::SetPlayerVariable {
                player,
                variable,
                value,
                ..
            }) = program.actions.get(*action)
            else {
                continue;
            };
            let is_event_player = matches!(
                program.values.get(*player).map(|node| &node.value),
                Some(wir::Value::EventPlayer)
            );
            if !is_event_player {
                continue;
            }
            let var_index = variable.index() as u32;
            writers
                .entry(var_index)
                .and_modify(|slot| *slot = (usize::MAX, *value)) // written again
                .or_insert((index, *value));
        }
        let mut removed: Vec<wir::ActionId> = Vec::new();
        let mut substitutions: HashMap<u32, wir::ValueId> = HashMap::new();
        for action in &actions {
            let Some(Action::SetPlayerVariable { variable, .. }) = program.actions.get(*action)
            else {
                continue;
            };
            if let Some((index, value)) = writers.get(&(variable.index() as u32)).copied() {
                if index != usize::MAX {
                    removed.push(*action);
                    substitutions.insert(variable.index() as u32, value);
                }
            }
        }
        if substitutions.is_empty() {
            continue;
        }
        let values_len = program.values.len();
        for index in 0..values_len {
            let id = wright_ir::ids::Id::from_index(index);
            let node = program
                .values
                .get(id)
                .cloned()
                .unwrap_or_else(|| workshop_rs::wir::ValueNode::new(wir::Value::Null, None));
            let replacement = match &node.value {
                Value::PlayerVariable { player, variable }
                    if matches!(
                        program.values.get(*player).map(|n| &n.value),
                        Some(Value::EventPlayer)
                    ) =>
                {
                    substitutions.get(&(variable.index() as u32)).copied()
                }
                _ => None,
            };
            let Some(replacement) = replacement else {
                continue;
            };
            let replacement = program
                .values
                .get(replacement)
                .cloned()
                .expect("replacement in range");
            program.values.get_mut(id).expect("id in range").value = replacement.value;
        }
        let rule_id = wright_ir::ids::Id::from_index(rule_index);
        if let Some(rule) = program.rules.get_mut(rule_id) {
            rule.actions.retain(|action| !removed.contains(action));
        }
    }
}

/// The declared foreach divergence: `For Player Variable(Event Player, v, …)`
/// loops normalize to the global form. Applied identically to both sides.
fn foreach_globalize(program: &mut wir::Program) {
    let mut rewrites: HashMap<u32, wir::GlobalVarId> = HashMap::new();
    fn global_for(
        program: &mut wir::Program,
        name: &str,
        rewrites: &mut HashMap<u32, wir::GlobalVarId>,
        player_index: u32,
    ) -> wir::GlobalVarId {
        if let Some(global) = rewrites.get(&player_index) {
            return *global;
        }
        let index = program.global_variables.len() as u32;
        let id = program.global_variables.push(wir::WorkshopVariable {
            name: name.to_string(),
            index,
            span: None,
            name_span: None,
        });
        rewrites.insert(player_index, id);
        id
    }
    fn rewrite_value(
        program: &mut wir::Program,
        id: wir::ValueId,
        rewrites: &HashMap<u32, wir::GlobalVarId>,
    ) {
        let node = program
            .values
            .get(id)
            .cloned()
            .unwrap_or_else(|| workshop_rs::wir::ValueNode::new(wir::Value::Null, None));
        let children: Vec<wir::ValueId> = match &node.value {
            Value::Array(elements) => elements.clone(),
            Value::Vector { x, y, z } => vec![*x, *y, *z],
            Value::PlayerVariable { player, .. } => vec![*player],
            Value::Call { args, .. } => args.clone(),
            _ => Vec::new(),
        };
        for child in children {
            rewrite_value(program, child, rewrites);
        }
        if let Value::PlayerVariable { player, variable } = &node.value {
            if matches!(
                program.values.get(*player).map(|n| &n.value),
                Some(Value::EventPlayer)
            ) {
                if let Some(global) = rewrites.get(&(variable.index() as u32)) {
                    program.values.get_mut(id).expect("id in range").value =
                        Value::GlobalVariable(*global);
                }
            }
        }
    }
    fn rewrite_actions(
        program: &mut wir::Program,
        actions: &[wir::ActionId],
        rewrites: &HashMap<u32, wir::GlobalVarId>,
    ) {
        for action in actions {
            let Some(node) = program.actions.get(*action).cloned() else {
                continue;
            };
            let children: Vec<wir::ValueId> = match &node {
                Action::SetGlobalVariable { value, .. }
                | Action::ModifyGlobalVariable { value, .. }
                | Action::Debug { value, .. }
                | Action::Print { message: value, .. } => vec![*value],
                Action::SetPlayerVariable { player, value, .. }
                | Action::ModifyPlayerVariable { player, value, .. } => vec![*player, *value],
                Action::CallSubroutine { .. } => Vec::new(),
                Action::If {
                    branches,
                    else_body,
                    ..
                } => {
                    let mut out = Vec::new();
                    for branch in branches {
                        out.push(branch.condition);
                    }
                    if let Some(else_body) = else_body {
                        for action in else_body {
                            rewrite_actions(program, &[*action], rewrites);
                        }
                    }
                    for branch in branches {
                        rewrite_actions(program, &branch.body, rewrites);
                    }
                    out
                }
                Action::While {
                    condition, body, ..
                } => {
                    rewrite_actions(program, body, rewrites);
                    vec![*condition]
                }
                Action::ForGlobalVariable {
                    start,
                    stop,
                    step,
                    body,
                    ..
                }
                | Action::ForPlayerVariable {
                    start,
                    stop,
                    step,
                    body,
                    ..
                } => {
                    rewrite_actions(program, body, rewrites);
                    vec![*start, *stop, *step]
                }
                Action::Call { args, .. } => args.clone(),
            };
            for child in children {
                rewrite_value(program, child, rewrites);
            }
        }
    }
    for rule_index in 0..program.rules.len() {
        let rule_id = wright_ir::ids::Id::from_index(rule_index);
        let player_loops: Vec<(wir::ActionId, u32, String)> = program
            .rules
            .get(rule_id)
            .map(|rule| rule.actions.clone())
            .unwrap_or_default()
            .iter()
            .filter_map(|action| {
                let Some(Action::ForPlayerVariable { variable, .. }) = program.actions.get(*action)
                else {
                    return None;
                };
                let name = program
                    .player_variables
                    .get(*variable)
                    .map(|v| v.name.clone())
                    .unwrap_or_default();
                Some((*action, variable.index() as u32, name))
            })
            .collect();
        let mut local_rewrites = rewrites.clone();
        for (action, player_index, name) in player_loops {
            let global = global_for(program, &name, &mut local_rewrites, player_index);
            let loop_body = {
                let Some(Action::ForPlayerVariable { body, .. }) = program.actions.get(action)
                else {
                    continue;
                };
                body.clone()
            };
            rewrite_actions(program, &loop_body, &local_rewrites);
            let (start, stop, step) = {
                let Some(Action::ForPlayerVariable {
                    start, stop, step, ..
                }) = program.actions.get(action)
                else {
                    continue;
                };
                (*start, *stop, *step)
            };
            let span = program.actions.get(action).and_then(|action| action.span());
            *program.actions.get_mut(action).expect("action in range") =
                Action::ForGlobalVariable {
                    variable: global,
                    start,
                    stop,
                    step,
                    body: loop_body,
                    span,
                    target_span: None,
                };
        }
        for (player_index, global) in local_rewrites {
            rewrites.entry(player_index).or_insert(global);
        }
    }
}

/// `Custom String` placeholder syntax is an output-form difference
/// (`<0>` ≡ `{0}`); normalize the text of format strings on both sides.
fn fold_placeholders(program: &mut wir::Program) {
    let mut texts: Vec<(usize, String)> = Vec::new();
    for index in 0..program.values.len() {
        let id = wright_ir::ids::Id::from_index(index);
        let Some(node) = program.values.get(id) else {
            continue;
        };
        if let Value::Call { name, args } = &node.value {
            if name == "customString" && !args.is_empty() {
                let text_id = args[0];
                if let Some(Value::String(text)) = program.values.get(text_id).map(|n| &n.value) {
                    let normalized: String = text
                        .chars()
                        .map(|c| match c {
                            '<' => '{',
                            '>' => '}',
                            other => other,
                        })
                        .collect();
                    if normalized != *text {
                        texts.push((text_id.index(), normalized));
                    }
                }
            }
        }
    }
    for (index, text) in texts {
        let id = wright_ir::ids::Id::from_index(index);
        program.values.get_mut(id).expect("id in range").value = Value::String(text);
    }
}

/// The reference's null/unit vector output idioms (P6 evidence):
/// `Vector(0,0,0)` ≡ `Subtract(Left, Left)` and `Vector(1,0,0)` ≡ `Left`.
/// Applied identically to both sides.
fn vector_idioms(program: &mut wir::Program) {
    let mut rewrites: Vec<(usize, wir::Value)> = Vec::new();
    for index in 0..program.values.len() {
        let id = wright_ir::ids::Id::from_index(index);
        let Some(node) = program.values.get(id) else {
            continue;
        };
        let Value::Call { name, args } = &node.value else {
            continue;
        };
        if name != "vector" || args.len() != 3 {
            continue;
        }
        let components: Option<(f64, f64, f64)> = (|| {
            Some((
                match &program.values.get(args[0])?.value {
                    Value::Number { value, .. } => *value,
                    _ => return None,
                },
                match &program.values.get(args[1])?.value {
                    Value::Number { value, .. } => *value,
                    _ => return None,
                },
                match &program.values.get(args[2])?.value {
                    Value::Number { value, .. } => *value,
                    _ => return None,
                },
            ))
        })();
        let Some((x, y, z)) = components else {
            continue;
        };
        let left = program.values.push(wir::ValueNode::new(
            Value::Enum {
                value_type: "HudPosition".to_string(),
                value: "LEFT".to_string(),
            },
            None,
        ));
        if x == 0.0 && y == 0.0 && z == 0.0 {
            rewrites.push((
                index,
                Value::Call {
                    name: "subtract".to_string(),
                    args: vec![left, left],
                },
            ));
        } else if x == 1.0 && y == 0.0 && z == 0.0 {
            rewrites.push((
                index,
                Value::Enum {
                    value_type: "HudPosition".to_string(),
                    value: "LEFT".to_string(),
                },
            ));
        }
    }
    for (index, value) in rewrites {
        let id = wright_ir::ids::Id::from_index(index);
        program.values.get_mut(id).expect("id in range").value = value;
    }
}

/// The declared #119 normalization, applied identically to both sides.
fn normalize(program: &mut wir::Program) {
    fold(program);
    inline_write_once_player_vars(program);
    fold(program);
    foreach_globalize(program);
    vector_idioms(program);
    fold_placeholders(program);
}

// -- structural comparison (the declared #119 contract) ---------------------

fn compare(actual: &wir::Program, expected: &wir::Program) -> Result<(), String> {
    let rules_a = &actual.rules;
    let rules_b = &expected.rules;
    if rules_a.len() != rules_b.len() {
        return Err(format!(
            "rule count differs: actual {} vs reference {}",
            rules_a.len(),
            rules_b.len()
        ));
    }
    for (index, (rule_a, rule_b)) in rules_a.iter().zip(rules_b.iter()).enumerate() {
        let name_a = match rule_a.name.as_str() {
            "Initialize global variables" => "Initial Global",
            "Initialize player variables" => "Initial Player",
            other => other,
        };
        let name_b = match rule_b.name.as_str() {
            "Initialize global variables" => "Initial Global",
            "Initialize player variables" => "Initial Player",
            other => other,
        };
        if name_a != name_b {
            return Err(format!(
                "rule {index} name differs: '{}' vs '{}'",
                rule_a.name, rule_b.name
            ));
        }
        match (&rule_a.event, &rule_b.event) {
            (Event::Global, Event::Global)
            | (Event::EachPlayer, Event::EachPlayer)
            | (
                Event::EachPlayer,
                Event::EachPlayerWithFilters {
                    team: workshop_rs::wir::EventTeam::All,
                    target: workshop_rs::wir::EventTarget::All,
                },
            )
            | (
                Event::EachPlayerWithFilters {
                    team: workshop_rs::wir::EventTeam::All,
                    target: workshop_rs::wir::EventTarget::All,
                },
                Event::EachPlayer,
            )
            | (
                Event::EachPlayerWithFilters {
                    team: workshop_rs::wir::EventTeam::All,
                    target: workshop_rs::wir::EventTarget::All,
                },
                Event::EachPlayerWithFilters {
                    team: workshop_rs::wir::EventTeam::All,
                    target: workshop_rs::wir::EventTarget::All,
                },
            ) => {}
            (Event::EachPlayerWithFilters { .. }, _)
            | (Event::Player { .. }, _)
            | (_, Event::EachPlayerWithFilters { .. })
            | (_, Event::Player { .. }) => {
                return Err(format!("rule {index} uses an unsupported event"));
            }
            (Event::Subroutine(a), Event::Subroutine(b)) => {
                let name_a = actual
                    .subroutines
                    .get(*a)
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                let name_b = expected
                    .subroutines
                    .get(*b)
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                if name_a != name_b {
                    return Err(format!(
                        "rule {index} subroutine differs: {name_a} vs {name_b}"
                    ));
                }
            }
            (a, b) => {
                return Err(format!("rule {index} event differs: {a:?} vs {b:?}"));
            }
        }
        if rule_a.conditions.len() != rule_b.conditions.len() {
            return Err(format!(
                "rule {index} condition count differs: {} vs {}",
                rule_a.conditions.len(),
                rule_b.conditions.len()
            ));
        }
        for (condition_a, condition_b) in rule_a.conditions.iter().zip(rule_b.conditions.iter()) {
            compare_value(actual, expected, *condition_a, *condition_b)
                .map_err(|message| format!("rule {index} condition: {message}"))?;
        }
        if rule_a.actions.len() != rule_b.actions.len() {
            return Err(format!(
                "rule {index} action count differs: {} vs {}",
                rule_a.actions.len(),
                rule_b.actions.len()
            ));
        }
        for (action_a, action_b) in rule_a.actions.iter().zip(rule_b.actions.iter()) {
            compare_action(actual, expected, *action_a, *action_b)
                .map_err(|message| format!("rule {index}: {message}"))?;
        }
    }
    Ok(())
}

fn compare_actions(
    actual: &wir::Program,
    expected: &wir::Program,
    actions_a: &[wir::ActionId],
    actions_b: &[wir::ActionId],
) -> Result<(), String> {
    if actions_a.len() != actions_b.len() {
        return Err(format!(
            "action list length differs: {} vs {}",
            actions_a.len(),
            actions_b.len()
        ));
    }
    for (action_a, action_b) in actions_a.iter().zip(actions_b.iter()) {
        compare_action(actual, expected, *action_a, *action_b)?;
    }
    Ok(())
}

fn compare_action(
    actual: &wir::Program,
    expected: &wir::Program,
    action_a: wir::ActionId,
    action_b: wir::ActionId,
) -> Result<(), String> {
    let (Some(a), Some(b)) = (actual.actions.get(action_a), expected.actions.get(action_b)) else {
        return Err("dangling action".to_string());
    };
    match (a, b) {
        (
            Action::SetGlobalVariable {
                variable: va,
                value: value_a,
                ..
            },
            Action::SetGlobalVariable {
                variable: vb,
                value: value_b,
                ..
            },
        ) => {
            if global_name(actual, *va) != global_name(expected, *vb) {
                return Err(format!(
                    "setGlobalVariable target differs: {} vs {}",
                    global_name(actual, *va),
                    global_name(expected, *vb)
                ));
            }
            compare_value(actual, expected, *value_a, *value_b)
        }
        (
            Action::ModifyGlobalVariable {
                variable: va,
                op: op_a,
                value: value_a,
                ..
            },
            Action::ModifyGlobalVariable {
                variable: vb,
                op: op_b,
                value: value_b,
                ..
            },
        ) => {
            if global_name(actual, *va) != global_name(expected, *vb) {
                return Err(format!(
                    "modifyGlobalVariable target differs: {} vs {}",
                    global_name(actual, *va),
                    global_name(expected, *vb)
                ));
            }
            if op_a != op_b {
                return Err(format!("modify operator differs: {op_a:?} vs {op_b:?}"));
            }
            compare_value(actual, expected, *value_a, *value_b)
        }
        (
            Action::SetPlayerVariable {
                player: player_a,
                variable: va,
                value: value_a,
                ..
            },
            Action::SetPlayerVariable {
                player: player_b,
                variable: vb,
                value: value_b,
                ..
            },
        ) => {
            compare_value(actual, expected, *player_a, *player_b)?;
            if player_name(actual, *va) != player_name(expected, *vb) {
                return Err(format!(
                    "setPlayerVariable target differs: {} vs {}",
                    player_name(actual, *va),
                    player_name(expected, *vb)
                ));
            }
            compare_value(actual, expected, *value_a, *value_b)
        }
        (
            Action::ModifyPlayerVariable {
                player: player_a,
                variable: va,
                op: op_a,
                value: value_a,
                ..
            },
            Action::ModifyPlayerVariable {
                player: player_b,
                variable: vb,
                op: op_b,
                value: value_b,
                ..
            },
        ) => {
            compare_value(actual, expected, *player_a, *player_b)?;
            if player_name(actual, *va) != player_name(expected, *vb) {
                return Err(format!(
                    "modifyPlayerVariable target differs: {} vs {}",
                    player_name(actual, *va),
                    player_name(expected, *vb)
                ));
            }
            if op_a != op_b {
                return Err(format!("modify operator differs: {op_a:?} vs {op_b:?}"));
            }
            compare_value(actual, expected, *value_a, *value_b)
        }
        (Action::CallSubroutine { .. }, Action::CallSubroutine { .. }) => Ok(()),
        (
            Action::If {
                branches: branches_a,
                else_body: else_a,
                ..
            },
            Action::If {
                branches: branches_b,
                else_body: else_b,
                ..
            },
        ) => {
            if branches_a.len() != branches_b.len() {
                return Err(format!(
                    "if branch count differs: {} vs {}",
                    branches_a.len(),
                    branches_b.len()
                ));
            }
            for (branch_a, branch_b) in branches_a.iter().zip(branches_b.iter()) {
                compare_value(actual, expected, branch_a.condition, branch_b.condition)?;
                compare_actions(actual, expected, &branch_a.body, &branch_b.body)?;
            }
            match (else_a, else_b) {
                (None, None) => Ok(()),
                (Some(body_a), Some(body_b)) => compare_actions(actual, expected, body_a, body_b),
                (Some(_), None) => Err("actual has an else body, reference does not".to_string()),
                (None, Some(_)) => Err("reference has an else body, actual does not".to_string()),
            }
        }
        (
            Action::While {
                condition: condition_a,
                body: body_a,
                ..
            },
            Action::While {
                condition: condition_b,
                body: body_b,
                ..
            },
        ) => {
            compare_value(actual, expected, *condition_a, *condition_b)?;
            compare_actions(actual, expected, body_a, body_b)
        }
        (
            Action::ForGlobalVariable {
                variable: va,
                start: start_a,
                stop: stop_a,
                step: step_a,
                body: body_a,
                ..
            },
            Action::ForGlobalVariable {
                variable: vb,
                start: start_b,
                stop: stop_b,
                step: step_b,
                body: body_b,
                ..
            },
        ) => {
            if global_name(actual, *va) != global_name(expected, *vb) {
                return Err(format!(
                    "for loop variable differs: {} vs {}",
                    global_name(actual, *va),
                    global_name(expected, *vb)
                ));
            }
            compare_value(actual, expected, *start_a, *start_b)?;
            compare_value(actual, expected, *stop_a, *stop_b)?;
            compare_value(actual, expected, *step_a, *step_b)?;
            compare_actions(actual, expected, body_a, body_b)
        }
        (Action::Debug { .. }, Action::Debug { .. }) => {
            Err("debug actions are rejected on the reconstruction surface".to_string())
        }
        (Action::Print { .. }, Action::Print { .. }) => {
            Err("print actions are rejected on the reconstruction surface".to_string())
        }
        (
            Action::Call {
                name: name_a,
                args: args_a,
                ..
            },
            Action::Call {
                name: name_b,
                args: args_b,
                ..
            },
        ) => {
            if name_a != name_b {
                return Err(format!("call name differs: '{name_a}' vs '{name_b}'"));
            }
            if args_a.len() != args_b.len() {
                return Err(format!(
                    "call '{name_a}' arity differs: {} vs {}",
                    args_a.len(),
                    args_b.len()
                ));
            }
            for (value_a, value_b) in args_a.iter().zip(args_b.iter()) {
                compare_value(actual, expected, *value_a, *value_b)?;
            }
            Ok(())
        }
        (a, b) => Err(format!(
            "action kind differs: {} vs {}",
            action_kind(a),
            action_kind(b)
        )),
    }
}

fn compare_value(
    actual: &wir::Program,
    expected: &wir::Program,
    value_a: wir::ValueId,
    value_b: wir::ValueId,
) -> Result<(), String> {
    let (Some(node_a), Some(node_b)) = (actual.values.get(value_a), expected.values.get(value_b))
    else {
        return Err("dangling value".to_string());
    };
    match (&node_a.value, &node_b.value) {
        (Value::Number { value: x, .. }, Value::Number { value: y, .. }) => {
            if x != y {
                return Err(format!("number differs: {x} vs {y}"));
            }
            Ok(())
        }
        (Value::String(x), Value::String(y)) => {
            if x != y {
                return Err(format!("string differs: '{x}' vs '{y}'"));
            }
            Ok(())
        }
        (Value::Bool(x), Value::Bool(y)) => {
            if x != y {
                return Err(format!("bool differs: {x} vs {y}"));
            }
            Ok(())
        }
        (Value::Null, Value::Null) => Ok(()),
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                return Err(format!("array length differs: {} vs {}", x.len(), y.len()));
            }
            for (a, b) in x.iter().zip(y.iter()) {
                compare_value(actual, expected, *a, *b)?;
            }
            Ok(())
        }
        (
            Value::Vector {
                x: x1,
                y: y1,
                z: z1,
            },
            Value::Vector {
                x: x2,
                y: y2,
                z: z2,
            },
        ) => {
            compare_value(actual, expected, *x1, *x2)?;
            compare_value(actual, expected, *y1, *y2)?;
            compare_value(actual, expected, *z1, *z2)
        }
        (
            Value::Enum {
                value_type: t1,
                value: v1,
            },
            Value::Enum {
                value_type: t2,
                value: v2,
            },
        ) => {
            let team_equivalent = matches!(
                (t1.as_str(), t2.as_str()),
                ("Color", "Team") | ("Team", "Color")
            ) && v1 == v2;
            if (t1 != t2 || v1 != v2) && !team_equivalent {
                return Err(format!("enum differs: {t1}.{v1} vs {t2}.{v2}"));
            }
            Ok(())
        }
        (Value::GlobalVariable(x), Value::GlobalVariable(y)) => {
            if global_name(actual, *x) != global_name(expected, *y) {
                return Err(format!(
                    "global differs: {} vs {}",
                    global_name(actual, *x),
                    global_name(expected, *y)
                ));
            }
            Ok(())
        }
        (
            Value::PlayerVariable {
                player: p1,
                variable: v1,
            },
            Value::PlayerVariable {
                player: p2,
                variable: v2,
            },
        ) => {
            compare_value(actual, expected, *p1, *p2)?;
            if player_name(actual, *v1) != player_name(expected, *v2) {
                return Err(format!(
                    "player variable differs: {} vs {}",
                    player_name(actual, *v1),
                    player_name(expected, *v2)
                ));
            }
            Ok(())
        }
        (Value::EventPlayer, Value::EventPlayer) => Ok(()),
        (
            Value::Call {
                name: name_a,
                args: args_a,
            },
            Value::Call {
                name: name_b,
                args: args_b,
            },
        ) => {
            if name_a != name_b {
                return Err(format!("value call differs: '{name_a}' vs '{name_b}'"));
            }
            if args_a.len() != args_b.len() {
                return Err(format!(
                    "value call '{name_a}' arity differs: {} vs {}",
                    args_a.len(),
                    args_b.len()
                ));
            }
            for (a, b) in args_a.iter().zip(args_b.iter()) {
                compare_value(actual, expected, *a, *b)?;
            }
            Ok(())
        }
        (a, b) => Err(format!(
            "value kind differs: {} vs {}",
            value_kind(a),
            value_kind(b)
        )),
    }
}

fn global_name(program: &wir::Program, id: wir::GlobalVarId) -> String {
    program
        .global_variables
        .get(id)
        .map(|variable| variable.name.clone())
        .unwrap_or_default()
}

fn player_name(program: &wir::Program, id: wir::PlayerVarId) -> String {
    program
        .player_variables
        .get(id)
        .map(|variable| variable.name.clone())
        .unwrap_or_default()
}

fn action_kind(action: &Action) -> &'static str {
    match action {
        Action::SetGlobalVariable { .. } => "setGlobalVariable",
        Action::ModifyGlobalVariable { .. } => "modifyGlobalVariable",
        Action::SetPlayerVariable { .. } => "setPlayerVariable",
        Action::ModifyPlayerVariable { .. } => "modifyPlayerVariable",
        Action::CallSubroutine { .. } => "callSubroutine",
        Action::If { .. } => "if",
        Action::While { .. } => "while",
        Action::ForGlobalVariable { .. } => "forGlobalVariable",
        Action::ForPlayerVariable { .. } => "forPlayerVariable",
        Action::Debug { .. } => "debug",
        Action::Print { .. } => "print",
        Action::Call { .. } => "call",
    }
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Number { .. } => "number",
        Value::String(_) => "string",
        Value::Bool(_) => "bool",
        Value::Null => "null",
        Value::Array(_) => "array",
        Value::Vector { .. } => "vector",
        Value::Enum { .. } => "enum",
        Value::GlobalVariable(_) => "global",
        Value::PlayerVariable { .. } => "playerVariable",
        Value::EventPlayer => "eventPlayer",
        Value::Call { .. } => "call",
    }
}

// -- fixtures ---------------------------------------------------------------

const RECONSTRUCTION_DIR: &str = "compatibility/ostw/reconstruction";

const POSITIVE_FIXTURES: &[&str] = &["surface-basic", "surface-actions", "surface-values"];

const SYNTAX_VALUE_IDS: &[&str] = &[
    "==",
    "!=",
    "<",
    "<=",
    ">",
    ">=",
    "add",
    "and",
    "array",
    "customString",
    "divide",
    "eventPlayer",
    "ifThenElse",
    "multiply",
    "not",
    "or",
    "subtract",
    "valueInArray",
    "vector",
];

fn fixture_dir(name: &str) -> PathBuf {
    workspace_root().join(RECONSTRUCTION_DIR).join(name)
}

/// The full loop for one positive fixture.
fn run_full_loop(catalog: &wright_workshop::catalog::Catalog, name: &str) -> serde_json::Value {
    let dir = fixture_dir(name);
    let fixture_text = read(&dir.join("workshop.txt"));

    // Workshop → WIR (the shared parser, the driver path).
    let original = parse(catalog, &fixture_text);

    // WIR → OSTW (the shipped reconstruction API).
    let ostw = wright_ostw::reconstruct::reconstruct(&original, catalog).unwrap_or_else(|errors| {
        panic!(
            "{name}: fixture WIR must reconstruct: {}",
            errors
                .iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        )
    });

    // OSTW → native frontend → HIR → WIR (zero diagnostics).
    let hir = compile_reconstructed_ostw(&ostw, name);
    let reconstructed = wright_ir::lower::lower(&hir).expect("lowering succeeds");
    reconstructed.validate().expect("lowered program validates");

    // WIR → Workshop text (the shared emitter).
    let emitted = wright_workshop::emitter::emit(
        &reconstructed,
        catalog,
        &wright_workshop::catalog::Locale::new("en-US"),
    )
    .expect("reconstructed Workshop emission succeeds");

    // The reconstructed Workshop text reparses and re-emits byte-identically
    // (round-trip fixed point).
    let reparsed = parse(catalog, &emitted);
    let reemitted = wright_workshop::emitter::emit(
        &reparsed,
        catalog,
        &wright_workshop::catalog::Locale::new("en-US"),
    )
    .expect("re-emission succeeds");
    assert_eq!(
        emitted, reemitted,
        "{name}: reconstructed Workshop must reach the round-trip fixed point"
    );

    // Semantic equivalence under the declared #119 normalization.
    let mut actual = reparsed;
    let mut reference = original;
    normalize(&mut actual);
    normalize(&mut reference);
    compare(&actual, &reference)
        .unwrap_or_else(|message| panic!("{name}: semantic divergence: {message}"));

    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut hasher, ostw.as_bytes());
    let ostw_sha256 = format!("{:x}", hasher.finalize());
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut hasher, emitted.as_bytes());
    let workshop_sha256 = format!("{:x}", hasher.finalize());

    serde_json::json!({
        "status": "round-trip",
        "ostwSha256": ostw_sha256,
        "workshopSha256": workshop_sha256,
        "roundTrip": "fixed-point",
        "frontend": "wright/ostw-native zero diagnostics",
    })
}

#[test]
fn reconstruction_full_loop_holds_for_positive_fixtures() {
    let catalog = wright_workshop::catalog::Catalog::builtin().expect("catalog loads");
    let mut report = serde_json::Map::new();
    for name in POSITIVE_FIXTURES {
        report.insert(name.to_string(), run_full_loop(&catalog, name));
    }
    report.insert(
        "suite".to_string(),
        serde_json::json!({
            "name": "wright-ostw-reconstruct",
            "fixtures": POSITIVE_FIXTURES.len(),
        }),
    );
    let report_path = workspace_root().join("target/wright-ostw-reconstruct-report.json");
    let parent = report_path.parent().expect("target dir");
    std::fs::create_dir_all(parent).expect("create target dir");
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&serde_json::Value::Object(report))
            .expect("report serializes"),
    )
    .expect("write reconstruction report");
}

// -- deterministic emission (unit-level, drives the shipped API) ------------

/// Build a minimal WIR program exercising every declared-surface construct,
/// used by the per-construct emission assertions.
fn surface_program(catalog: &wright_workshop::catalog::Catalog) -> wir::Program {
    let mut program = wir::Program::default();
    let g = program.global_variables.push(wir::WorkshopVariable {
        name: "g".to_string(),
        index: 0,
        span: None,
        name_span: None,
    });
    let counter = program.global_variables.push(wir::WorkshopVariable {
        name: "counter".to_string(),
        index: 1,
        span: None,
        name_span: None,
    });
    let arr = program.global_variables.push(wir::WorkshopVariable {
        name: "arr".to_string(),
        index: 2,
        span: None,
        name_span: None,
    });
    let health = program.player_variables.push(wir::WorkshopVariable {
        name: "health".to_string(),
        index: 0,
        span: None,
        name_span: None,
    });
    let sub = program.subroutines.push(wir::WorkshopSubroutine {
        name: "bump".to_string(),
        index: 0,
        span: None,
        name_span: None,
    });

    let number = |program: &mut wir::Program, value: f64| -> wir::ValueId {
        program.values.push(wir::ValueNode::new(
            wir::Value::Number {
                value,
                text: value.to_string(),
            },
            None,
        ))
    };
    let string = |program: &mut wir::Program, text: &str| -> wir::ValueId {
        program.values.push(wir::ValueNode::new(
            wir::Value::String(text.to_string()),
            None,
        ))
    };
    let global = |program: &mut wir::Program, id: wir::GlobalVarId| -> wir::ValueId {
        program
            .values
            .push(wir::ValueNode::new(wir::Value::GlobalVariable(id), None))
    };
    let call = |program: &mut wir::Program, name: &str, args: Vec<wir::ValueId>| -> wir::ValueId {
        program.values.push(wir::ValueNode::new(
            wir::Value::Call {
                name: name.to_string(),
                args,
            },
            None,
        ))
    };

    // A subroutine rule: counter += 1; if (counter == 10) { counter = 0; }
    let one = number(&mut program, 1.0);
    let counter_read = global(&mut program, counter);
    let add = call(&mut program, "add", vec![counter_read, one]);
    let mut sub_actions = vec![program.actions.push(wir::Action::ModifyGlobalVariable {
        variable: counter,
        op: wir::ModifyOp::Add,
        value: add,
        span: None,
        target_span: None,
    })];
    // if (counter == 10) { counter = 0; } inside the subroutine body.
    let ten = number(&mut program, 10.0);
    let counter_read = global(&mut program, counter);
    let condition = call(&mut program, "==", vec![counter_read, ten]);
    let zero = number(&mut program, 0.0);
    let set_zero = program.actions.push(wir::Action::SetGlobalVariable {
        variable: counter,
        value: zero,
        span: None,
        target_span: None,
    });
    let if_action = program.actions.push(wir::Action::If {
        branches: vec![wir::IfBranch {
            condition,
            body: vec![set_zero],
        }],
        else_body: None,
        span: None,
    });
    sub_actions.push(if_action);
    program.rules.push(wir::Rule {
        name: "Subroutine bump".to_string(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Subroutine(sub),
        conditions: Vec::new(),
        actions: sub_actions,
    });

    // The main rule: set/modify/if/while/for + calls + values.
    let mut actions = Vec::new();
    // g = Add(counter, 5); g += 1; arr.append(7); health = 1; health += 1;
    let five = number(&mut program, 5.0);
    let counter_read = global(&mut program, counter);
    let add = call(&mut program, "add", vec![counter_read, five]);
    actions.push(program.actions.push(wir::Action::SetGlobalVariable {
        variable: g,
        value: add,
        span: None,
        target_span: None,
    }));
    let one = number(&mut program, 1.0);
    actions.push(program.actions.push(wir::Action::ModifyGlobalVariable {
        variable: g,
        op: wir::ModifyOp::Add,
        value: one,
        span: None,
        target_span: None,
    }));
    let seven = number(&mut program, 7.0);
    actions.push(program.actions.push(wir::Action::ModifyGlobalVariable {
        variable: arr,
        op: wir::ModifyOp::AppendToArray,
        value: seven,
        span: None,
        target_span: None,
    }));
    let event_player = program
        .values
        .push(wir::ValueNode::new(wir::Value::EventPlayer, None));
    let one = number(&mut program, 1.0);
    actions.push(program.actions.push(wir::Action::SetPlayerVariable {
        player: event_player,
        variable: health,
        value: one,
        span: None,
        target_span: None,
    }));
    let event_player = program
        .values
        .push(wir::ValueNode::new(wir::Value::EventPlayer, None));
    let one = number(&mut program, 1.0);
    actions.push(program.actions.push(wir::Action::ModifyPlayerVariable {
        player: event_player,
        variable: health,
        op: wir::ModifyOp::Add,
        value: one,
        span: None,
        target_span: None,
    }));
    // bump();
    actions.push(program.actions.push(wir::Action::CallSubroutine {
        subroutine: sub,
        span: None,
        callee_span: None,
    }));
    // BigMessage(AllPlayers(Team.ALL), <"<0>", g>);
    let team_all = program.values.push(wir::ValueNode::new(
        wir::Value::Enum {
            value_type: "Team".to_string(),
            value: "ALL".to_string(),
        },
        None,
    ));
    let all_players = call(&mut program, "allPlayers", vec![team_all]);
    let text = string(&mut program, "<0>");
    let g_read = global(&mut program, g);
    let format = call(&mut program, "customString", vec![text, g_read]);
    actions.push(program.actions.push(wir::Action::Call {
        name: "bigMessage".to_string(),
        args: vec![all_players, format],
        span: None,
    }));
    // if (g > 10) { g = 0; } else if (g > 5) { g = 1; } else { g = 2; }
    let ten = number(&mut program, 10.0);
    let g_read = global(&mut program, g);
    let cond_a = call(&mut program, ">", vec![g_read, ten]);
    let zero = number(&mut program, 0.0);
    let set_a = program.actions.push(wir::Action::SetGlobalVariable {
        variable: g,
        value: zero,
        span: None,
        target_span: None,
    });
    let five = number(&mut program, 5.0);
    let g_read = global(&mut program, g);
    let cond_b = call(&mut program, ">", vec![g_read, five]);
    let one = number(&mut program, 1.0);
    let set_b = program.actions.push(wir::Action::SetGlobalVariable {
        variable: g,
        value: one,
        span: None,
        target_span: None,
    });
    let two = number(&mut program, 2.0);
    let set_c = program.actions.push(wir::Action::SetGlobalVariable {
        variable: g,
        value: two,
        span: None,
        target_span: None,
    });
    actions.push(program.actions.push(wir::Action::If {
        branches: vec![
            wir::IfBranch {
                condition: cond_a,
                body: vec![set_a],
            },
            wir::IfBranch {
                condition: cond_b,
                body: vec![set_b],
            },
        ],
        else_body: Some(vec![set_c]),
        span: None,
    }));
    // while (counter < 3) { counter += 1; }
    let three = number(&mut program, 3.0);
    let counter_read = global(&mut program, counter);
    let while_cond = call(&mut program, "<", vec![counter_read, three]);
    let one = number(&mut program, 1.0);
    let bump_counter = program.actions.push(wir::Action::ModifyGlobalVariable {
        variable: counter,
        op: wir::ModifyOp::Add,
        value: one,
        span: None,
        target_span: None,
    });
    actions.push(program.actions.push(wir::Action::While {
        condition: while_cond,
        body: vec![bump_counter],
        span: None,
    }));
    // for (counter = 0; counter < 3; 1) { g += 1; }
    let zero = number(&mut program, 0.0);
    let three = number(&mut program, 3.0);
    let counter_read = global(&mut program, counter);
    let stop = call(&mut program, "<", vec![counter_read, three]);
    let one = number(&mut program, 1.0);
    let one_more = number(&mut program, 1.0);
    let bump_g = program.actions.push(wir::Action::ModifyGlobalVariable {
        variable: g,
        op: wir::ModifyOp::Add,
        value: one_more,
        span: None,
        target_span: None,
    });
    actions.push(program.actions.push(wir::Action::ForGlobalVariable {
        variable: counter,
        start: zero,
        stop,
        step: one,
        body: vec![bump_g],
        span: None,
        target_span: None,
    }));
    // return;
    actions.push(program.actions.push(wir::Action::Call {
        name: "abort".to_string(),
        args: Vec::new(),
        span: None,
    }));

    program.rules.push(wir::Rule {
        name: "Main".to_string(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: Vec::new(),
        actions,
    });
    let _ = catalog;
    program
}

#[test]
fn emission_covers_every_declared_construct() {
    let catalog = wright_workshop::catalog::Catalog::builtin().expect("catalog loads");
    let program = surface_program(&catalog);
    program.validate().expect("synthetic program validates");
    let text = wright_ostw::reconstruct::reconstruct(&program, &catalog)
        .expect("the declared surface must reconstruct");
    for expected in [
        "globalvar Any g;",
        "globalvar Any counter;",
        "globalvar Any arr;",
        "playervar Any health;",
        "void bump() \"Subroutine bump\" {",
        "counter += 1;",
        "if (counter == 10) {",
        "counter = 0;",
        "g = counter + 5;",
        "g += 1;",
        "arr.append(7);",
        "health = 1;",
        "health += 1;",
        "bump();",
        "BigMessage(AllPlayers(Team.All), <\"<0>\", g>);",
        "if (g > 10) {",
        "else if (g > 5) {",
        "else {",
        "g = 2;",
        "while (counter < 3) {",
        "for (counter = 0; counter < 3; 1) {",
        "return;",
        "rule: \"Main\" {",
    ] {
        assert!(
            text.contains(expected),
            "reconstructed OSTW must contain {expected:?}:\n{text}"
        );
    }
}

#[test]
fn emission_is_byte_identical_across_runs() {
    let catalog = wright_workshop::catalog::Catalog::builtin().expect("catalog loads");
    let program = surface_program(&catalog);
    program.validate().expect("synthetic program validates");
    let first = wright_ostw::reconstruct::reconstruct(&program, &catalog).expect("reconstructs");
    let second = wright_ostw::reconstruct::reconstruct(&program, &catalog).expect("reconstructs");
    assert_eq!(first, second, "identical WIR must emit byte-identical OSTW");
    // Re-running on the fixture WIRs is also byte-identical (determinism over
    // the committed fixtures).
    for name in POSITIVE_FIXTURES {
        let dir = fixture_dir(name);
        let fixture = parse(&catalog, &read(&dir.join("workshop.txt")));
        let a = wright_ostw::reconstruct::reconstruct(&fixture, &catalog).expect("reconstructs");
        let b = wright_ostw::reconstruct::reconstruct(&fixture, &catalog).expect("reconstructs");
        assert_eq!(a, b, "{name}: reconstruction must be deterministic");
    }
}

// -- structured rejections (synthetic WIR per declared boundary) ------------

type ProgramBuilder = fn(&wright_workshop::catalog::Catalog) -> wir::Program;

fn program_with_for_player_variable(_: &wright_workshop::catalog::Catalog) -> wir::Program {
    let mut program = wir::Program::default();
    let p = program.player_variables.push(wir::WorkshopVariable {
        name: "p".to_string(),
        index: 0,
        span: None,
        name_span: None,
    });
    let event_player = program
        .values
        .push(wir::ValueNode::new(wir::Value::EventPlayer, None));
    let zero = program.values.push(wir::ValueNode::new(
        wir::Value::Number {
            value: 0.0,
            text: "0".to_string(),
        },
        None,
    ));
    let five = program.values.push(wir::ValueNode::new(
        wir::Value::Number {
            value: 5.0,
            text: "5".to_string(),
        },
        None,
    ));
    let one = program.values.push(wir::ValueNode::new(
        wir::Value::Number {
            value: 1.0,
            text: "1".to_string(),
        },
        None,
    ));
    let action = program.actions.push(wir::Action::ForPlayerVariable {
        player: event_player,
        variable: p,
        start: zero,
        stop: five,
        step: one,
        body: Vec::new(),
        span: None,
    });
    program.rules.push(wir::Rule {
        name: "R".to_string(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: Vec::new(),
        actions: vec![action],
    });
    program
}

fn program_with_debug(_: &wright_workshop::catalog::Catalog) -> wir::Program {
    let mut program = wir::Program::default();
    let value = program.values.push(wir::ValueNode::new(
        wir::Value::Number {
            value: 1.0,
            text: "1".to_string(),
        },
        None,
    ));
    let action = program
        .actions
        .push(wir::Action::Debug { value, span: None });
    program.rules.push(wir::Rule {
        name: "R".to_string(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: Vec::new(),
        actions: vec![action],
    });
    program
}

fn program_with_print(_: &wright_workshop::catalog::Catalog) -> wir::Program {
    let mut program = wir::Program::default();
    let message = program.values.push(wir::ValueNode::new(
        wir::Value::String("x".to_string()),
        None,
    ));
    let action = program.actions.push(wir::Action::Print {
        message,
        span: None,
    });
    program.rules.push(wir::Rule {
        name: "R".to_string(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: Vec::new(),
        actions: vec![action],
    });
    program
}

fn program_with_settings(_: &wright_workshop::catalog::Catalog) -> wir::Program {
    wir::Program {
        settings: Some(workshop_rs::settings::Settings {
            span: None,
            children: Vec::new(),
        }),
        ..wir::Program::default()
    }
}

fn program_with_unbound_action(_: &wright_workshop::catalog::Catalog) -> wir::Program {
    let mut program = wir::Program::default();
    let action = program.actions.push(wir::Action::Call {
        name: "createBeamEffect".to_string(),
        args: Vec::new(),
        span: None,
    });
    program.rules.push(wir::Rule {
        name: "R".to_string(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: Vec::new(),
        actions: vec![action],
    });
    program
}

fn program_with_unbound_value(_: &wright_workshop::catalog::Catalog) -> wir::Program {
    let mut program = wir::Program::default();
    let g = program.global_variables.push(wir::WorkshopVariable {
        name: "g".to_string(),
        index: 0,
        span: None,
        name_span: None,
    });
    let value = program.values.push(wir::ValueNode::new(
        wir::Value::Call {
            name: "getHealth".to_string(),
            args: Vec::new(),
        },
        None,
    ));
    let action = program.actions.push(wir::Action::SetGlobalVariable {
        variable: g,
        value,
        span: None,
        target_span: None,
    });
    program.rules.push(wir::Rule {
        name: "R".to_string(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: Vec::new(),
        actions: vec![action],
    });
    program
}

fn program_with_unbound_enum(_: &wright_workshop::catalog::Catalog) -> wir::Program {
    let mut program = wir::Program::default();
    let g = program.global_variables.push(wir::WorkshopVariable {
        name: "g".to_string(),
        index: 0,
        span: None,
        name_span: None,
    });
    let value = program.values.push(wir::ValueNode::new(
        wir::Value::Enum {
            value_type: "SomeDomain".to_string(),
            value: "X".to_string(),
        },
        None,
    ));
    let action = program.actions.push(wir::Action::SetGlobalVariable {
        variable: g,
        value,
        span: None,
        target_span: None,
    });
    program.rules.push(wir::Rule {
        name: "R".to_string(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: Vec::new(),
        actions: vec![action],
    });
    program
}

fn program_with_raise_to_power(_: &wright_workshop::catalog::Catalog) -> wir::Program {
    let mut program = wir::Program::default();
    let g = program.global_variables.push(wir::WorkshopVariable {
        name: "g".to_string(),
        index: 0,
        span: None,
        name_span: None,
    });
    let one = program.values.push(wir::ValueNode::new(
        wir::Value::Number {
            value: 1.0,
            text: "1".to_string(),
        },
        None,
    ));
    let action = program.actions.push(wir::Action::ModifyGlobalVariable {
        variable: g,
        op: wir::ModifyOp::RaiseToPower,
        value: one,
        span: None,
        target_span: None,
    });
    program.rules.push(wir::Rule {
        name: "R".to_string(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: Vec::new(),
        actions: vec![action],
    });
    program
}

fn program_with_remove_from_array(_: &wright_workshop::catalog::Catalog) -> wir::Program {
    let mut program = wir::Program::default();
    let g = program.global_variables.push(wir::WorkshopVariable {
        name: "g".to_string(),
        index: 0,
        span: None,
        name_span: None,
    });
    let one = program.values.push(wir::ValueNode::new(
        wir::Value::Number {
            value: 1.0,
            text: "1".to_string(),
        },
        None,
    ));
    let action = program.actions.push(wir::Action::ModifyGlobalVariable {
        variable: g,
        op: wir::ModifyOp::RemoveFromArray,
        value: one,
        span: None,
        target_span: None,
    });
    program.rules.push(wir::Rule {
        name: "R".to_string(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: Vec::new(),
        actions: vec![action],
    });
    program
}

fn program_with_non_comparison_condition(_: &wright_workshop::catalog::Catalog) -> wir::Program {
    let mut program = wir::Program::default();
    let condition = program
        .values
        .push(wir::ValueNode::new(wir::Value::Bool(true), None));
    program.rules.push(wir::Rule {
        name: "R".to_string(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: vec![condition],
        actions: Vec::new(),
    });
    program
}

fn program_with_partial_arity(_: &wright_workshop::catalog::Catalog) -> wir::Program {
    let mut program = wir::Program::default();
    let event_player = program
        .values
        .push(wir::ValueNode::new(wir::Value::EventPlayer, None));
    let action = program.actions.push(wir::Action::Call {
        name: "bigMessage".to_string(),
        args: vec![event_player], // 1 of 2 canonical arguments
        span: None,
    });
    program.rules.push(wir::Rule {
        name: "R".to_string(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: Vec::new(),
        actions: vec![action],
    });
    program
}

fn program_with_name_collision(_: &wright_workshop::catalog::Catalog) -> wir::Program {
    let mut program = wir::Program::default();
    program.global_variables.push(wir::WorkshopVariable {
        name: "g".to_string(),
        index: 0,
        span: None,
        name_span: None,
    });
    program.player_variables.push(wir::WorkshopVariable {
        name: "g".to_string(),
        index: 0,
        span: None,
        name_span: None,
    });
    program
}

fn program_with_empty_name(_: &wright_workshop::catalog::Catalog) -> wir::Program {
    let mut program = wir::Program::default();
    program.global_variables.push(wir::WorkshopVariable {
        name: String::new(),
        index: 0,
        span: None,
        name_span: None,
    });
    program
}

fn program_with_bodiless_subroutine(_: &wright_workshop::catalog::Catalog) -> wir::Program {
    let mut program = wir::Program::default();
    program.subroutines.push(wir::WorkshopSubroutine {
        name: "sub".to_string(),
        index: 0,
        span: None,
        name_span: None,
    });
    program
}

fn program_with_player_modify_receiver(_: &wright_workshop::catalog::Catalog) -> wir::Program {
    let mut program = wir::Program::default();
    let p = program.player_variables.push(wir::WorkshopVariable {
        name: "p".to_string(),
        index: 0,
        span: None,
        name_span: None,
    });
    let team_all = program.values.push(wir::ValueNode::new(
        wir::Value::Enum {
            value_type: "Team".to_string(),
            value: "ALL".to_string(),
        },
        None,
    ));
    let all_players = program.values.push(wir::ValueNode::new(
        wir::Value::Call {
            name: "allPlayers".to_string(),
            args: vec![team_all],
        },
        None,
    ));
    let one = program.values.push(wir::ValueNode::new(
        wir::Value::Number {
            value: 1.0,
            text: "1".to_string(),
        },
        None,
    ));
    let action = program.actions.push(wir::Action::ModifyPlayerVariable {
        player: all_players,
        variable: p,
        op: wir::ModifyOp::Add,
        value: one,
        span: None,
        target_span: None,
    });
    program.rules.push(wir::Rule {
        name: "R".to_string(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: Vec::new(),
        actions: vec![action],
    });
    program
}

fn program_with_non_literal_format_text(_: &wright_workshop::catalog::Catalog) -> wir::Program {
    let mut program = wir::Program::default();
    let g = program.global_variables.push(wir::WorkshopVariable {
        name: "g".to_string(),
        index: 0,
        span: None,
        name_span: None,
    });
    let one = program.values.push(wir::ValueNode::new(
        wir::Value::Number {
            value: 1.0,
            text: "1".to_string(),
        },
        None,
    ));
    let value = program.values.push(wir::ValueNode::new(
        wir::Value::Call {
            name: "customString".to_string(),
            args: vec![one],
        },
        None,
    ));
    let action = program.actions.push(wir::Action::SetGlobalVariable {
        variable: g,
        value,
        span: None,
        target_span: None,
    });
    program.rules.push(wir::Rule {
        name: "R".to_string(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: Vec::new(),
        actions: vec![action],
    });
    program
}

fn program_with_strict_greater_in_format(_: &wright_workshop::catalog::Catalog) -> wir::Program {
    let mut program = wir::Program::default();
    let g = program.global_variables.push(wir::WorkshopVariable {
        name: "g".to_string(),
        index: 0,
        span: None,
        name_span: None,
    });
    let text = program.values.push(wir::ValueNode::new(
        wir::Value::String("<0>".to_string()),
        None,
    ));
    let one = program.values.push(wir::ValueNode::new(
        wir::Value::Number {
            value: 1.0,
            text: "1".to_string(),
        },
        None,
    ));
    let two = program.values.push(wir::ValueNode::new(
        wir::Value::Number {
            value: 2.0,
            text: "2".to_string(),
        },
        None,
    ));
    let g_read = program
        .values
        .push(wir::ValueNode::new(wir::Value::GlobalVariable(g), None));
    let greater = program.values.push(wir::ValueNode::new(
        wir::Value::Call {
            name: ">".to_string(),
            args: vec![one, two],
        },
        None,
    ));
    let value = program.values.push(wir::ValueNode::new(
        wir::Value::Call {
            name: "customString".to_string(),
            args: vec![text, g_read, greater],
        },
        None,
    ));
    let action = program.actions.push(wir::Action::SetGlobalVariable {
        variable: g,
        value,
        span: None,
        target_span: None,
    });
    program.rules.push(wir::Rule {
        name: "R".to_string(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: Vec::new(),
        actions: vec![action],
    });
    program
}

fn program_with_invalid_number(_: &wright_workshop::catalog::Catalog) -> wir::Program {
    let mut program = wir::Program::default();
    let g = program.global_variables.push(wir::WorkshopVariable {
        name: "g".to_string(),
        index: 0,
        span: None,
        name_span: None,
    });
    let value = program.values.push(wir::ValueNode::new(
        wir::Value::Number {
            value: -5.0,
            text: "-5".to_string(),
        },
        None,
    ));
    let action = program.actions.push(wir::Action::SetGlobalVariable {
        variable: g,
        value,
        span: None,
        target_span: None,
    });
    program.rules.push(wir::Rule {
        name: "R".to_string(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: Vec::new(),
        actions: vec![action],
    });
    program
}

/// Every rejection case the declared boundary names: (manifest kind, expected
/// code, program builder). The boundary-manifest conformance test requires
/// the manifest's rejected set to be exactly this table.
fn rejection_cases() -> Vec<(&'static str, &'static str, ProgramBuilder)> {
    vec![
        (
            "forPlayerVariable",
            "reconstruct-unsupported-action",
            program_with_for_player_variable,
        ),
        (
            "debug",
            "reconstruct-unsupported-action",
            program_with_debug,
        ),
        (
            "print",
            "reconstruct-unsupported-action",
            program_with_print,
        ),
        (
            "settings",
            "reconstruct-unsupported-program-settings",
            program_with_settings,
        ),
        (
            "unboundAction",
            "reconstruct-unbound-call",
            program_with_unbound_action,
        ),
        (
            "unboundValue",
            "reconstruct-unbound-call",
            program_with_unbound_value,
        ),
        (
            "unboundEnum",
            "reconstruct-unbound-enum",
            program_with_unbound_enum,
        ),
        (
            "modifyOp:RaiseToPower",
            "reconstruct-unsupported-modify-op",
            program_with_raise_to_power,
        ),
        (
            "modifyOp:RemoveFromArray",
            "reconstruct-unsupported-modify-op",
            program_with_remove_from_array,
        ),
        (
            "condition",
            "reconstruct-unsupported-condition",
            program_with_non_comparison_condition,
        ),
        ("arity", "reconstruct-arity", program_with_partial_arity),
        (
            "nameCollision",
            "reconstruct-name-collision",
            program_with_name_collision,
        ),
        (
            "emptyName",
            "reconstruct-name-collision",
            program_with_empty_name,
        ),
        (
            "subroutine",
            "reconstruct-unsupported-subroutine",
            program_with_bodiless_subroutine,
        ),
        (
            "playerModifyReceiver",
            "reconstruct-unsupported-player-receiver",
            program_with_player_modify_receiver,
        ),
        (
            "formatText",
            "reconstruct-unsupported-format-text",
            program_with_non_literal_format_text,
        ),
        (
            "formatArg",
            "reconstruct-unsupported-format-arg",
            program_with_strict_greater_in_format,
        ),
        (
            "number",
            "reconstruct-unsupported-number",
            program_with_invalid_number,
        ),
    ]
}

#[test]
fn every_declared_rejection_is_structured_and_total() {
    let catalog = wright_workshop::catalog::Catalog::builtin().expect("catalog loads");
    for (kind, code, builder) in rejection_cases() {
        let program = builder(&catalog);
        program
            .validate()
            .expect("synthetic rejection program validates");
        let result = wright_ostw::reconstruct::reconstruct(&program, &catalog);
        let errors = result.expect_err(&format!("{kind} must be rejected"));
        assert!(
            errors.iter().any(|error| error.code == code),
            "{kind}: expected code {code}, got: {:?}",
            errors
        );
        for error in &errors {
            assert!(
                !error.code.is_empty() && !error.kind.is_empty(),
                "{kind}: every rejection must carry a stable code and kind"
            );
        }
        // Total rejection: no partial output is ever produced.
        assert!(
            !errors.is_empty(),
            "{kind}: a rejection must produce at least one diagnostic"
        );
    }
}

#[test]
fn rejection_never_produces_partial_output() {
    let catalog = wright_workshop::catalog::Catalog::builtin().expect("catalog loads");
    // The committed for-player-variable rejection fixture.
    let dir = fixture_dir("reject/for-player-variable");
    let fixture = parse(&catalog, &read(&dir.join("workshop.txt")));
    let result = wright_ostw::reconstruct::reconstruct(&fixture, &catalog);
    let errors = result.expect_err("the fixture must be rejected");
    assert!(
        errors
            .iter()
            .any(|error| error.code == "reconstruct-unsupported-action"
                && error.kind == "forPlayerVariable"),
        "for-player-variable fixture: {:?}",
        errors
    );
}

// -- machine-readable boundary manifest conformance --------------------------

/// Walk one program and collect the exercised construct kinds: catalog call
/// ids (action/value, excluding the special-syntax ids), enum members,
/// modify ops, and structural kinds.
fn collect_coverage(program: &wir::Program) -> serde_json::Value {
    fn walk_value(
        program: &wir::Program,
        id: wir::ValueId,
        values: &mut Vec<String>,
        enums: &mut Vec<String>,
    ) {
        let Some(node) = program.values.get(id) else {
            return;
        };
        match &node.value {
            Value::Call { name, args } => {
                if !SYNTAX_VALUE_IDS.contains(&name.as_str()) {
                    values.push(name.clone());
                }
                for arg in args {
                    walk_value(program, *arg, values, enums);
                }
            }
            Value::Array(elements) => {
                for element in elements {
                    walk_value(program, *element, values, enums);
                }
            }
            Value::Vector { x, y, z } => {
                walk_value(program, *x, values, enums);
                walk_value(program, *y, values, enums);
                walk_value(program, *z, values, enums);
            }
            Value::PlayerVariable { player, .. } => walk_value(program, *player, values, enums),
            Value::Enum { value_type, value } => {
                enums.push(format!("{value_type}.{value}"));
            }
            _ => {}
        }
    }
    let mut values = Vec::new();
    let mut actions = Vec::new();
    let mut enums = Vec::new();
    let mut modify_ops = Vec::new();
    let mut action_kinds = Vec::new();
    for rule in program.rules.iter() {
        for action in &rule.actions {
            let Some(node) = program.actions.get(*action) else {
                continue;
            };
            match node {
                Action::Call { name, args, .. } => {
                    if name == "abort" {
                        action_kinds.push("return".to_string());
                    } else {
                        action_kinds.push("call".to_string());
                        if !SYNTAX_VALUE_IDS.contains(&name.as_str()) {
                            actions.push(name.clone());
                        }
                    }
                    for arg in args {
                        walk_value(program, *arg, &mut values, &mut enums);
                    }
                }
                Action::SetGlobalVariable { value, .. } => {
                    action_kinds.push("setGlobalVariable".to_string());
                    walk_value(program, *value, &mut values, &mut enums);
                }
                Action::ModifyGlobalVariable { op, value, .. } => {
                    action_kinds.push("modifyGlobalVariable".to_string());
                    modify_ops.push(op.as_str().to_string());
                    walk_value(program, *value, &mut values, &mut enums);
                }
                Action::SetPlayerVariable { player, value, .. } => {
                    action_kinds.push("setPlayerVariable".to_string());
                    walk_value(program, *player, &mut values, &mut enums);
                    walk_value(program, *value, &mut values, &mut enums);
                }
                Action::ModifyPlayerVariable {
                    player, op, value, ..
                } => {
                    action_kinds.push("modifyPlayerVariable".to_string());
                    modify_ops.push(op.as_str().to_string());
                    walk_value(program, *player, &mut values, &mut enums);
                    walk_value(program, *value, &mut values, &mut enums);
                }
                Action::CallSubroutine { .. } => {
                    action_kinds.push("callSubroutine".to_string());
                }
                Action::If { branches, .. } => {
                    action_kinds.push("if".to_string());
                    for branch in branches {
                        walk_value(program, branch.condition, &mut values, &mut enums);
                    }
                }
                Action::While { condition, .. } => {
                    action_kinds.push("while".to_string());
                    walk_value(program, *condition, &mut values, &mut enums);
                }
                Action::ForGlobalVariable {
                    start, stop, step, ..
                } => {
                    action_kinds.push("forGlobalVariable".to_string());
                    walk_value(program, *start, &mut values, &mut enums);
                    walk_value(program, *stop, &mut values, &mut enums);
                    walk_value(program, *step, &mut values, &mut enums);
                }
                _ => {}
            }
        }
    }
    fn sorted_unique(mut items: Vec<String>) -> Vec<String> {
        items.sort();
        items.dedup();
        items
    }
    serde_json::json!({
        "values": sorted_unique(values),
        "actions": sorted_unique(actions),
        "enums": sorted_unique(enums),
        "modifyOps": sorted_unique(modify_ops),
        "actionKinds": sorted_unique(action_kinds),
    })
}

fn manifest() -> serde_json::Value {
    let path = workspace_root()
        .join(RECONSTRUCTION_DIR)
        .join("support-boundary.json");
    serde_json::from_str(&read(&path)).expect("manifest parses")
}

#[test]
fn boundary_manifest_matches_classification_and_fixture_coverage() {
    let catalog = wright_workshop::catalog::Catalog::builtin().expect("catalog loads");
    let manifest = manifest();

    // The rejected set in the manifest is exactly the tested rejection table.
    let mut manifest_kinds: Vec<String> = manifest["rejected"]
        .as_array()
        .expect("rejected list")
        .iter()
        .map(|entry| entry["kind"].as_str().expect("kind").to_string())
        .collect();
    manifest_kinds.sort();
    let mut tested_kinds: Vec<String> = rejection_cases()
        .iter()
        .map(|(kind, _, _)| kind.to_string())
        .collect();
    tested_kinds.sort();
    assert_eq!(
        manifest_kinds, tested_kinds,
        "the manifest's rejected set must exactly match the tested rejection table"
    );

    // Every manifest-supported bound id resolves through the shipped reverse
    // bindings (the classifier treats it as supported).
    for id in manifest["supported"]["boundValueIds"]
        .as_array()
        .expect("boundValueIds")
    {
        let id = id.as_str().expect("id");
        assert!(
            wright_ostw::reconstruct::value_ostw_name(id).is_some(),
            "manifest value id '{id}' must have a reverse binding"
        );
    }
    for id in manifest["supported"]["boundActionIds"]
        .as_array()
        .expect("boundActionIds")
    {
        let id = id.as_str().expect("id");
        assert!(
            wright_ostw::reconstruct::action_ostw_name(id).is_some(),
            "manifest action id '{id}' must have a reverse binding"
        );
    }
    for domain in manifest["supported"]["enumDomains"]
        .as_array()
        .expect("enumDomains")
    {
        let domain = domain.as_str().expect("domain");
        assert!(
            wright_ostw::reconstruct::bound_enum_domains()
                .iter()
                .any(|binding| binding.domain == domain),
            "manifest enum domain '{domain}' must have a reverse binding"
        );
    }
    for member in manifest["supported"]["enumMembers"]
        .as_array()
        .expect("enumMembers")
    {
        let member = member.as_str().expect("member");
        let (domain, value) = member.split_once('.').expect("domain.member");
        assert!(
            wright_ostw::reconstruct::enum_ostw(domain, value).is_some(),
            "manifest enum member '{member}' must have a reverse binding"
        );
    }
    for id in manifest["supported"]["syntaxValueIds"]
        .as_array()
        .expect("syntaxValueIds")
    {
        let id = id.as_str().expect("id");
        assert!(
            SYNTAX_VALUE_IDS.contains(&id),
            "syntax value id '{id}' must be in the known special-syntax set"
        );
    }

    // Fixture coverage must exactly match the manifest's supported sets.
    let mut coverage = serde_json::json!({
        "values": Vec::<String>::new(),
        "actions": Vec::<String>::new(),
        "enums": Vec::<String>::new(),
        "modifyOps": Vec::<String>::new(),
        "actionKinds": Vec::<String>::new(),
    });
    for name in POSITIVE_FIXTURES {
        let dir = fixture_dir(name);
        let fixture = parse(&catalog, &read(&dir.join("workshop.txt")));
        let fixture_coverage = collect_coverage(&fixture);
        for key in ["values", "actions", "enums", "modifyOps", "actionKinds"] {
            let mut merged: Vec<String> = coverage[key]
                .as_array()
                .expect("array")
                .iter()
                .map(|v| v.as_str().expect("str").to_string())
                .collect();
            merged.extend(
                fixture_coverage[key]
                    .as_array()
                    .expect("array")
                    .iter()
                    .map(|v| v.as_str().expect("str").to_string()),
            );
            merged.sort();
            merged.dedup();
            coverage[key] = serde_json::json!(merged);
        }
    }
    for key in ["boundValueIds", "boundActionIds", "enumMembers"] {
        let manifest_key = match key {
            "boundValueIds" => "values",
            "boundActionIds" => "actions",
            _ => "enums",
        };
        let mut manifest_list: Vec<String> = manifest["supported"][key]
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str").to_string())
            .collect();
        manifest_list.sort();
        let mut covered: Vec<String> = coverage[manifest_key]
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str").to_string())
            .collect();
        covered.sort();
        assert_eq!(
            manifest_list, covered,
            "manifest {key} must exactly match fixture coverage"
        );
    }
    // The structural kinds and modify ops exercised by the fixtures are the
    // declared ones.
    for kind in [
        "setGlobalVariable",
        "modifyGlobalVariable",
        "setPlayerVariable",
        "modifyPlayerVariable",
        "callSubroutine",
        "if",
        "while",
        "forGlobalVariable",
        "call",
        "return",
    ] {
        let covered: Vec<String> = coverage["actionKinds"]
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str").to_string())
            .collect();
        assert!(
            covered.contains(&kind.to_string()),
            "fixture coverage must exercise action kind {kind}"
        );
    }
    for op in [
        "Add",
        "Subtract",
        "Multiply",
        "Divide",
        "Modulo",
        "AppendToArray",
    ] {
        let covered: Vec<String> = coverage["modifyOps"]
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str").to_string())
            .collect();
        assert!(
            covered.contains(&op.to_string()),
            "fixture coverage must exercise modify op {op}"
        );
    }
}

#[test]
fn reconstruct_api_exposes_the_reverse_binding_tables() {
    let catalog = wright_workshop::catalog::Catalog::builtin().expect("catalog loads");
    // Every bound id maps to a catalog entry of the right kind, and the OSTW
    // name resolves back through signature::builtin to the same id.
    for (id, source) in wright_ostw::reconstruct::bound_action_ids() {
        assert!(
            catalog
                .entry(wright_workshop::catalog::Kind::Action, id)
                .is_some(),
            "bound action id '{id}' must exist in the canonical catalog"
        );
        assert_eq!(
            crate_signature_builtin(source),
            Some((wright_workshop::catalog::Kind::Action, id)),
            "OSTW action name '{source}' must resolve back to catalog id '{id}'"
        );
    }
    for (id, source) in wright_ostw::reconstruct::bound_value_ids() {
        assert!(
            catalog
                .entry(wright_workshop::catalog::Kind::Value, id)
                .is_some(),
            "bound value id '{id}' must exist in the canonical catalog"
        );
        assert_eq!(
            crate_signature_builtin(source),
            Some((wright_workshop::catalog::Kind::Value, id)),
            "OSTW value name '{source}' must resolve back to catalog id '{id}'"
        );
    }
    for binding in wright_ostw::reconstruct::bound_enum_domains() {
        assert!(
            catalog.enum_domain(binding.domain).is_some(),
            "bound enum domain '{}' must exist in the canonical catalog",
            binding.domain
        );
        for (member, source_member) in &binding.members {
            assert_eq!(
                wright_ostw::reconstruct::enum_ostw(binding.domain, member),
                Some((binding.source, *source_member)),
                "enum member '{}' must resolve back to '{}'",
                member,
                source_member
            );
        }
    }
}

/// The same lookup the OSTW semantic phase uses (signature::builtin), kept
/// local so the test asserts the round trip through the shipped frontend
/// binding table without importing the private module path.
fn crate_signature_builtin(name: &str) -> Option<(wright_workshop::catalog::Kind, &'static str)> {
    wright_ostw::signature::builtin(name)
}
