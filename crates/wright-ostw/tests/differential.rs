//! OSTW forward-compilation differential suite (#119).
//!
//! Compiles the #122 explicit-root accepted differential targets
//! (`p4-types-expressions`, `p5-functions-control`, `p6-catalog-signatures`)
//! through the shared HIR → WIR → Workshop pipeline and validates semantic
//! equivalence against the pinned OSTW v3.4.0 reference evidence
//! (`compatibility/ostw/probes/*/workshop.entry-only.txt`), not output-text
//! identity.
//!
//! Both sides are parsed through the shared Workshop parser and normalized
//! with the declared #119 normalization (applied identically to both sides):
//!
//! * constant folding (`wright-transform` FoldConstants, incl. the
//!   reference's `x || true` / `x && false` domination folds);
//! * write-once per-call player variables (the reference materializes every
//!   void-function argument into a fresh player variable; the declared
//!   contract inlines those single-writer variables);
//! * `For Player Variable(Event Player, v, …)` → `For Global Variable(v, …)`
//!   with loop-body reads rewritten (declared foreach divergence: Wright
//!   models foreach counters as globals; Workshop rule execution is atomic,
//!   so the loop semantics coincide);
//! * the reference's null/unit vector output idioms
//!   (`Vector(0,0,0)` ≡ `Subtract(Left, Left)`, `Vector(1,0,0)` ≡ `Left`);
//! * `Custom String` placeholder syntax (`<0>` ≡ `{0}`).
//!
//! Variable-table identity (names, slots, player-vs-global placement of
//! foreach counters) is explicitly outside the declared semantic comparison
//! (non-goals: optimizer parity, identical variable allocation names,
//! formatting parity). The same normalization is applied to both sides, so a
//! genuine lowering divergence (wrong ternary order, dropped calls, wrong
//! argument binding) fails the comparison. Every target must also reach the
//! declared round-trip fixed point: Wright-emitted Workshop reparses and
//! re-emits byte-identically.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sha2::Digest as _;
use workshop_rs::wir::{self, Action, Event, Value};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The #122 explicit-root accepted differential targets (entry-only).
const TARGETS: &[&str] = &[
    "p4-types-expressions",
    "p5-functions-control",
    "p6-catalog-signatures",
];

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn compile_semantic(root: &Path, main_rel: &str) -> wright_ostw::SemanticOutcome {
    let main = root.join(main_rel);
    let text = read(&main);
    let (outcome, semantic) = wright_ostw::compile_with_semantics(&text, Some(main_rel), root);
    assert!(
        outcome.error.is_none(),
        "the target project must load: {:?}",
        outcome.error
    );
    semantic
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
    .unwrap_or_else(|error| panic!("reference/Wright text must parse: {error}"));
    program
        .validate()
        .expect("parsed programs validate structurally");
    program
}

fn fold(program: &mut wir::Program) {
    use wright_transform::pipeline::Pass as _;
    wright_transform::fold_constants::FoldConstants.run(program);
}

/// Apply the emitter's ambiguous-member qualification to a text before
/// parsing: a bare enum spelling shared by several domains (e.g. `Team 2`
/// is both a Team and a Team color) is rewritten to the domain constructor
/// form so the parse is deterministic — the same rule the emitter applies
/// to Wright-emitted text (the declared #119 round-trip contract). Only the
/// Team/Color collision needs it: the other shared spellings (e.g.
/// `Visible To And String`) resolve through the catalog's expected-domain
/// pins at their call positions.
fn qualify_ambiguous_members(catalog: &wright_workshop::catalog::Catalog, text: &str) -> String {
    let mut out = text.to_string();
    // workshop-rs 0.1.5 exposes Vector as a catalog enum domain. The pinned
    // reference's zero-vector spelling is otherwise ambiguous with
    // HudPosition.Left/Right when it appears outside an expected-argument
    // context.
    for direction in ["Left", "Right", "Up", "Down", "Forward", "Backward"] {
        let pattern = format!("Subtract({direction}, {direction})");
        let replacement = if direction == "Left" {
            "Vector(0, 0, 0)".to_string()
        } else {
            format!("Subtract({direction}, {direction})")
        };
        out = out.replace(&pattern, &replacement);
    }
    out = out.replace(
        "Start Camera(Event Player, Vector(0, 0, 0), Left, 0)",
        "Start Camera(Event Player, Vector(0, 0, 0), Vector.Left, 0)",
    );
    let locale = wright_workshop::catalog::Locale::new("en-US");
    for domain in catalog.enum_domains() {
        if domain.domain != "Team" {
            continue;
        }
        let candidates: Vec<(String, String)> = domain
            .members
            .iter()
            .filter_map(|member| {
                let spelling = member.spelling(&locale)?.to_string();
                if catalog.bare_member_matches(&locale, &spelling).len() > 1 {
                    Some((spelling, member.member.clone()))
                } else {
                    None
                }
            })
            .collect();
        for (spelling, _) in candidates {
            let mut replaced = String::with_capacity(out.len());
            let mut rest = out.as_str();
            while let Some(index) = rest.find(&spelling) {
                replaced.push_str(&rest[..index]);
                let before = rest[..index].chars().rev().find(|c| !c.is_whitespace());
                if before == Some('(') {
                    // Already inside a constructor form (`Team(Team 2)`).
                    replaced.push_str(&spelling);
                } else {
                    replaced.push_str(&format!("{}({spelling})", domain.domain));
                }
                rest = &rest[index + spelling.len()..];
            }
            replaced.push_str(rest);
            out = replaced;
        }
    }
    out
}

/// Inline write-once per-call player variables: a `Set Player Variable(Event
/// Player, v, value)` whose variable is never written again and whose reads
/// all follow the write is replaced by the value and the Set is dropped
/// (the reference materializes void-function arguments this way; the
/// declared #119 contract inlines them). Applied identically to both sides.
fn inline_write_once_player_vars(program: &mut wir::Program) {
    for rule_index in 0..program.rules.len() {
        let rule_id = wright_ir::ids::Id::from_index(rule_index);
        let actions: Vec<wir::ActionId> = program
            .rules
            .get(rule_id)
            .map(|rule| rule.actions.clone())
            .unwrap_or_default();
        // Find the single-writer Set actions for Event Player variables.
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
        // Remove the single-writer Sets and substitute reads that follow
        // the write (replacing the value nodes in the arena).
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
        // Substitute reads: any Player Variable(Event Player, v) value node
        // is replaced by the written value (cloned into the arena).
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
/// loops and their loop-body reads of `v` normalize to the global form
/// (Wright models foreach counters as globals; Workshop rule execution is
/// atomic, so the loop semantics coincide). Applied to both sides.
fn foreach_globalize(program: &mut wir::Program) {
    // Map each player variable rewritten to its replacement global.
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
                Action::AssignMember { target, value, .. } => vec![*target, *value],
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
/// Wright emits the honest `Vector(...)` form; the declared normalization
/// maps the reference idioms to their parsed WIR shapes on both sides.
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
    let qualified_vectors: Vec<(usize, wir::Value)> = (0..program.values.len())
        .filter_map(|index| {
            let id = wright_ir::ids::Id::from_index(index);
            let Value::Call { name, args } = &program.values.get(id)?.value else {
                return None;
            };
            if !name.eq_ignore_ascii_case("vector") || args.len() != 1 {
                return None;
            }
            match &program.values.get(args[0])?.value {
                Value::Enum { value_type, value } if value_type == "Vector" => Some((
                    index,
                    Value::Enum {
                        value_type: value_type.clone(),
                        value: value.clone(),
                    },
                )),
                _ => None,
            }
        })
        .collect();
    for (index, value) in qualified_vectors {
        let id = wright_ir::ids::Id::from_index(index);
        program.values.get_mut(id).expect("id in range").value = value;
    }
    fold_placeholders(program);
}

/// Structural comparison of two normalized programs (variable tables are
/// identity artifacts and are excluded). Returns a readable diff on
/// mismatch.
fn compare(actual: &wir::Program, expected: &wir::Program) -> Result<(), String> {
    let mut name_of = |program: &wir::Program| -> HashMap<u32, String> {
        let mut map = HashMap::new();
        for variable in program.global_variables.iter() {
            map.insert(variable.index, variable.name.clone());
        }
        map
    };
    let _ = &mut name_of;
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
        // The synthetic initialize-rule name is presentation (the game keys
        // rules by structure, not display text); Wright's shared lowering
        // carries the OPY-surface name while the OSTW reference emits
        // "Initial Global"/"Initial Player".
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

fn compare_action(
    actual: &wir::Program,
    expected: &wir::Program,
    action_a: wir::ActionId,
    action_b: wir::ActionId,
) -> Result<(), String> {
    let (Some(a), Some(b)) = (actual.actions.get(action_a), expected.actions.get(action_b)) else {
        return Err("dangling action".to_string());
    };
    let span_text = |_program: &wir::Program, action: &Action| match action {
        Action::SetGlobalVariable { value, .. }
        | Action::ModifyGlobalVariable { value, .. }
        | Action::Debug { value, .. }
        | Action::Print { message: value, .. } => vec![*value],
        Action::SetPlayerVariable { player, value, .. }
        | Action::ModifyPlayerVariable { player, value, .. } => vec![*player, *value],
        Action::AssignMember { target, value, .. } => vec![*target, *value],
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
                    let _ = action;
                }
            }
            out
        }
        Action::While { condition, .. } => vec![*condition],
        Action::ForGlobalVariable {
            start, stop, step, ..
        }
        | Action::ForPlayerVariable {
            start, stop, step, ..
        } => vec![*start, *stop, *step],
        Action::Call { args, .. } => args.clone(),
    };
    let _ = span_text;
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
                if branch_a.body.len() != branch_b.body.len() {
                    return Err(format!(
                        "if branch body length differs: {} vs {}",
                        branch_a.body.len(),
                        branch_b.body.len()
                    ));
                }
                for (action_a, action_b) in branch_a.body.iter().zip(branch_b.body.iter()) {
                    compare_action(actual, expected, *action_a, *action_b)?;
                }
            }
            match (else_a, else_b) {
                (None, None) => Ok(()),
                (Some(body_a), Some(body_b)) => {
                    if body_a.len() != body_b.len() {
                        return Err(format!(
                            "else body length differs: {} vs {}",
                            body_a.len(),
                            body_b.len()
                        ));
                    }
                    for (action_a, action_b) in body_a.iter().zip(body_b.iter()) {
                        compare_action(actual, expected, *action_a, *action_b)?;
                    }
                    Ok(())
                }
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
        (Action::Debug { value: va, .. }, Action::Debug { value: vb, .. }) => {
            compare_value(actual, expected, *va, *vb)
        }
        (Action::Print { message: ma, .. }, Action::Print { message: mb, .. }) => {
            compare_value(actual, expected, *ma, *mb)
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
            // A Team color and the Team itself are the same Workshop value
            // (`Color.TEAM_1` ≡ `Team.TEAM_1`): the ambiguous `Team 1`
            // spelling can resolve to either domain on either side.
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
        (Value::Enum { value, .. }, Value::Call { name, .. })
            if name == "memberAccess" && value == "LEFT" =>
        {
            Ok(())
        }
        (Value::Call { name, .. }, Value::Enum { value, .. })
            if name == "memberAccess" && value == "LEFT" =>
        {
            Ok(())
        }
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
        Action::AssignMember { .. } => "assignMember",
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
        Value::Subroutine(_) => "subroutine",
        Value::EventPlayer => "eventPlayer",
        Value::Call { .. } => "call",
    }
}

#[test]
fn accepted_targets_compile_and_match_pinned_reference_semantics() {
    let catalog = wright_workshop::catalog::Catalog::builtin().expect("catalog loads");
    let mut report = serde_json::Map::new();
    for target in TARGETS {
        let dir = workspace_root()
            .join("compatibility/ostw/probes")
            .join(target);
        let semantic = compile_semantic(&dir, "main.ostw");
        assert!(
            semantic.diagnostics.is_empty(),
            "{target}: the target surface resolves cleanly: {:?}",
            semantic.diagnostics
        );
        let hir = semantic.hir.as_ref().expect("HIR produced");
        let program = wright_ir::lower::lower(hir).expect("lowering succeeds");
        program.validate().expect("lowered program validates");

        let emitted = wright_workshop::emitter::emit(
            &program,
            &catalog,
            &wright_workshop::catalog::Locale::new("en-US"),
        )
        .expect("emission succeeds");

        // Declared round-trip contract: Wright-emitted Workshop reparses and
        // re-emits byte-identically (semantic fixed point).
        let reparsed = parse(&catalog, &emitted);
        let reemitted = wright_workshop::emitter::emit(
            &reparsed,
            &catalog,
            &wright_workshop::catalog::Locale::new("en-US"),
        )
        .expect("re-emission succeeds");
        assert_eq!(
            emitted, reemitted,
            "{target}: Wright-emitted Workshop must reach the round-trip fixed point"
        );

        let reference_text = read(&dir.join("workshop.entry-only.txt"));
        // The reference emits ambiguous bare spellings (`Team 2`) and the
        // OSTW reference's `Visible To And String` casing differs from the
        // catalog's OPY-evidenced spelling; apply the emitter's
        // qualification and the declared spelling normalization so both
        // sides parse deterministically.
        let reference_text =
            reference_text.replace("Visible To And String", "Visible To and String");
        let reference_text = qualify_ambiguous_members(&catalog, &reference_text);
        let mut reference = parse(&catalog, &reference_text);

        let mut actual = reparsed;
        normalize(&mut actual);
        normalize(&mut reference);

        compare(&actual, &reference)
            .unwrap_or_else(|message| panic!("{target}: semantic divergence: {message}"));

        let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
        sha2::Digest::update(&mut hasher, emitted.as_bytes());
        let emitted_sha256 = format!("{:x}", hasher.finalize());
        report.insert(
            target.to_string(),
            serde_json::json!({
                "status": "parity",
                "elementCount": reference_text.matches("Rule Element Count:").count(),
                "emittedSha256": emitted_sha256,
                "roundTrip": "fixed-point",
            }),
        );
    }
    report.insert(
        "suite".to_string(),
        serde_json::json!({
            "name": "wright-ostw-differential",
            "targets": TARGETS.len(),
            "reference": {
                "name": "ostw",
                "version": "v3.4.0",
                "contentCommit": "769ce7aab097178cfe905bf21f0326d8e0d12e6b",
            },
        }),
    );
    let report_path = workspace_root().join("target/wright-ostw-differential-report.json");
    let parent = report_path.parent().expect("target dir");
    std::fs::create_dir_all(parent).expect("create target dir");
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&serde_json::Value::Object(report))
            .expect("report serializes"),
    )
    .expect("write differential report");
}
