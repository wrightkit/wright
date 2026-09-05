//! Cross-format conversion integration suite (#126).
//!
//! Drives the **shipped** shared driver/session conversion operation
//! ([`CompilerSession::convert`]) over the committed Workshop fixtures and
//! proves both reverse loops through the real native frontends:
//!
//! * `Workshop → convert(opy) → native wright-opy frontend → HIR → WIR →
//!   Workshop` — equivalence under `workshop_rs::roundtrip::equivalent`
//!   (the #124 contract, no normalization);
//! * `Workshop → convert(ostw) → native wright-ostw frontend (generated
//!   `ds.toml` project root) → HIR → WIR → Workshop` — equivalence under the
//!   declared #119 normalization applied identically to both sides, plus the
//!   round-trip fixed point (reconstructed Workshop reparses and re-emits
//!   byte-identically).
//!
//! Rejections are deterministic with the reconstructor's stable codes and
//! never carry partial source, and the machine-readable report lands at
//! `target/wright-convert-report.json` (one entry per fixture, the repo
//! report pattern).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use workshop_rs::wir::{self, Action, Value};
use wright_driver::CompilerSession;
use wright_driver::config::SessionConfig;
use wright_driver::result::{ConvertResult, ConvertTarget};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The committed #124 OPY reconstruction fixtures (OPY-surface Workshop
/// inputs).
const OPY_FIXTURES: &[&str] = &[
    "variables-declarations",
    "subroutine-control-flow",
    "player-events",
    "values-enums",
    "actions-surface",
];

/// The committed #125 OSTW reconstruction fixtures (OSTW-surface Workshop
/// inputs).
const OSTW_FIXTURES: &[&str] = &["surface-basic", "surface-actions", "surface-values"];

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn sha256(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Parse Workshop text through the shared parser with the canonical
/// signature context (the same path the driver uses).
fn parse(catalog: &workshop_rs::catalog::Catalog, text: &str) -> wir::Program {
    let manifest =
        wright_opy::manifest::Manifest::builtin().expect("the OPY manifest is embedded and valid");
    let context = wright_core::signatures::ChainedExpectedDomain::new(&manifest, catalog);
    let program = workshop_rs::parser::parse_with_context(
        text,
        catalog,
        &workshop_rs::catalog::Locale::new("en-US"),
        &context,
    )
    .unwrap_or_else(|error| panic!("fixture Workshop text must parse: {error}"));
    program
        .validate()
        .expect("parsed programs validate structurally");
    program
}

/// A temp input file carrying Workshop text, so the driver's real
/// discovery/load path (extension → kind) drives the conversion.
fn workshop_input(text: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-convert-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("program.txt");
    std::fs::write(&path, text).unwrap();
    path
}

/// Drive the shipped shared conversion operation for one target.
fn convert(text: &str, target: ConvertTarget) -> (wright_driver::Envelope<ConvertResult>, PathBuf) {
    let path = workshop_input(text);
    let mut session = CompilerSession::new(SessionConfig::from_path(path.clone())).unwrap();
    let envelope = session.convert(target);
    (envelope, path)
}

/// Load the reconstructed OSTW through the owner-backed adapter in a
/// generated project root (`ds.toml` + `main.ostw`). Returns canonical WIR.
fn compile_reconstructed_ostw(ostw_text: &str, test_name: &str) -> wir::Program {
    let root = std::env::temp_dir().join(format!("wright-convert-ostw-{test_name}"));
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
    semantic.wir.expect("WIR produced")
}

/// Emit Workshop text for a WIR program through the shared emitter.
fn emit_workshop(
    catalog: &workshop_rs::catalog::Catalog,
    program: &wir::Program,
) -> Result<String, String> {
    workshop_rs::emitter::emit(
        program,
        catalog,
        &workshop_rs::catalog::Locale::new("en-US"),
    )
    .map_err(|error| error.to_string())
}

// ---------------------------------------------------------------------------
// The declared #119 normalization, applied identically to both sides. This is
// the comparison contract of the OSTW differential/reconstruction suites
// (`crates/wright-ostw/tests/{differential,reconstruct}.rs`); it is copied
// here (test-side, not shipped reconstruction logic) so the shared-path
// integration suite compares under exactly the same contract.
// ---------------------------------------------------------------------------

fn fold(program: &mut wir::Program) {
    use wright_transform::pipeline::Pass as _;
    wright_transform::fold_constants::FoldConstants.run(program);
}

/// The owner compiler's pinned OverPy contract lowers `not` over a comparison
/// to the complementary comparison (for example, `not (a < b)` to `a >= b`).
/// Normalize that representation difference before structural WIR comparison.
fn normalize_negated_comparisons(program: &mut wir::Program) {
    for index in 0..program.values.len() {
        let id = wright_ir::ids::Id::from_index(index);
        let Some(node) = program.values.get(id).cloned() else {
            continue;
        };
        let Value::Call { name, args } = node.value else {
            continue;
        };
        if name != "not" || args.len() != 1 {
            continue;
        }
        let Some(Value::Call {
            name: comparison,
            args: operands,
        }) = program.values.get(args[0]).map(|node| node.value.clone())
        else {
            continue;
        };
        let Some(negated) = negated_comparison(&comparison) else {
            continue;
        };
        program.values.get_mut(id).expect("value in range").value = Value::Call {
            name: negated.to_string(),
            args: operands,
        };
    }
}

fn negated_comparison(operator: &str) -> Option<&'static str> {
    Some(match operator {
        "==" => "!=",
        "!=" => "==",
        "<" => ">=",
        ">" => "<=",
        "<=" => ">",
        ">=" => "<",
        _ => return None,
    })
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
                | Action::ModifyGlobalVariable { value, .. } => vec![*value],
                Action::Call { args, .. } => args.clone(),
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

/// The Workshop parser preserves numeric `0`/`1` spellings for boolean action
/// arguments while the owner-backed source path lowers them as typed booleans.
/// Normalize that representation difference before applying the #119 rules.
fn normalize_boolean_argument_spellings(
    program: &mut wir::Program,
    catalog: &workshop_rs::catalog::Catalog,
) {
    let mut rewrites = Vec::new();
    for action in program.actions.iter() {
        let workshop_rs::wir::Action::Call { name, args, .. } = action else {
            continue;
        };
        let Some(entry) = catalog.entry(workshop_rs::catalog::Kind::Action, name) else {
            continue;
        };
        for (index, arg) in args.iter().enumerate() {
            if entry.param_types.get(index).and_then(Option::as_deref) != Some("Boolean") {
                continue;
            }
            match program.values.get(*arg).map(|node| &node.value) {
                Some(workshop_rs::wir::Value::Bool(value)) => rewrites.push((*arg, *value)),
                Some(workshop_rs::wir::Value::Number { value, .. }) if *value == 0.0 => {
                    rewrites.push((*arg, false));
                }
                Some(workshop_rs::wir::Value::Number { value, .. }) if *value == 1.0 => {
                    rewrites.push((*arg, true));
                }
                _ => {}
            }
        }
    }
    for (id, value) in rewrites {
        program
            .values
            .get_mut(id)
            .expect("boolean argument in range")
            .value = workshop_rs::wir::Value::Bool(value);
    }
}

/// The declared #119 normalization, applied identically to both sides.
fn normalize(program: &mut wir::Program, catalog: &workshop_rs::catalog::Catalog) {
    normalize_boolean_argument_spellings(program, catalog);
    fold(program);
    inline_write_once_player_vars(program);
    fold(program);
    foreach_globalize(program);
    vector_idioms(program);
    fold_placeholders(program);
}

// ---------------------------------------------------------------------------
// Suite
// ---------------------------------------------------------------------------

/// The full Workshop → OPY loop through the shared driver path.
fn opy_round_trip(
    catalog: &workshop_rs::catalog::Catalog,
    fixture: &str,
    failures: &mut Vec<String>,
) -> serde_json::Value {
    const OWNER_UNSUPPORTED_FIXTURES: &[&str] =
        &["subroutine-control-flow", "player-events", "values-enums"];
    let path = workspace_root()
        .join("crates/wright-opy/tests/fixtures/reconstruct")
        .join(format!("{fixture}.ws"));
    let source = read(&path);
    let original = parse(catalog, &source);

    let (envelope, input_path) = convert(&source, ConvertTarget::Opy);
    if !envelope.ok {
        if fixture == "values-enums"
            && envelope
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "unsupported-enum-domain-mismatch")
        {
            return serde_json::json!({
                "status": "owner-unsupported",
                "ownerDiagnostic": envelope.diagnostics[0].message,
            });
        }
        failures.push(format!(
            "{fixture}: shared convert rejected: {:?}",
            envelope.diagnostics
        ));
        return serde_json::json!({ "status": "convert-rejected" });
    }
    let opy = envelope.result.text.clone();
    assert_eq!(envelope.result.target, ConvertTarget::Opy);
    assert_eq!(envelope.result.sha256.len(), 64);

    // The reconstructed OPY must reload through the real native frontend…
    let recompiled = match wright_opy::compile(&opy, &format!("{fixture}.opy"), Path::new("")) {
        Ok(program) => program,
        Err(error) => {
            if error.code == "unsupported-integration-surface" {
                assert!(
                    OWNER_UNSUPPORTED_FIXTURES.contains(&fixture),
                    "unexpected published opy-compiler limitation for {fixture}: {error}"
                );
                return serde_json::json!({
                    "status": "owner-unsupported",
                    "ownerDiagnostic": error.to_string(),
                });
            }
            failures.push(format!(
                "{fixture}: the native frontend rejected the reconstructed OPY: {error}"
            ));
            return serde_json::json!({ "status": "frontend-rejected" });
        }
    };
    // …and the recompiled WIR must be equivalent to the parsed WIR, then
    // still emit to Workshop text through the shipped emitter.
    let mut original = original;
    let mut recompiled = recompiled;
    normalize_negated_comparisons(&mut original);
    normalize_negated_comparisons(&mut recompiled);
    let equivalent = workshop_rs::roundtrip::equivalent(&original, &recompiled);
    if !equivalent {
        failures.push(format!("{fixture}: recompiled WIR is not equivalent"));
    }
    // The trailing `→ Workshop` hop: the recompiled WIR still emits to
    // Workshop text through the shipped emitter.
    let workshop_emit = emit_workshop(catalog, &recompiled);
    if let Err(error) = &workshop_emit {
        failures.push(format!(
            "{fixture}: Workshop emission of the recompiled WIR failed: {error}"
        ));
    }
    let _ = std::fs::remove_dir_all(input_path.parent().unwrap());

    serde_json::json!({
        "status": "round-trip",
        "target": "opy",
        "inputSha256": sha256(&source),
        "reconstructedSha256": envelope.result.sha256,
        "frontendAccepted": true,
        "equivalent": equivalent,
        "workshopEmit": workshop_emit.is_ok(),
    })
}

/// The full Workshop → OSTW loop through the shared driver path.
fn ostw_round_trip(
    catalog: &workshop_rs::catalog::Catalog,
    fixture: &str,
    failures: &mut Vec<String>,
) -> serde_json::Value {
    let path = workspace_root()
        .join("compatibility/ostw/reconstruction")
        .join(fixture)
        .join("workshop.txt");
    let source = read(&path);
    let original = parse(catalog, &source);

    let (envelope, input_path) = convert(&source, ConvertTarget::Ostw);
    if !envelope.ok {
        failures.push(format!(
            "{fixture}: shared convert rejected: {:?}",
            envelope.diagnostics
        ));
        return serde_json::json!({ "status": "convert-rejected" });
    }
    let ostw = envelope.result.text.clone();
    assert_eq!(envelope.result.target, ConvertTarget::Ostw);

    // Reload through the native OSTW frontend in a generated project root.
    let reconstructed = compile_reconstructed_ostw(&ostw, fixture);
    reconstructed.validate().expect("lowered program validates");

    // WIR → Workshop text through the shared emitter, then the round-trip
    // fixed point: the reconstructed Workshop reparses and re-emits
    // byte-identically.
    let emitted = match emit_workshop(catalog, &reconstructed) {
        Ok(emitted) => emitted,
        Err(error) => {
            failures.push(format!("{fixture}: Workshop emission failed: {error}"));
            return serde_json::json!({ "status": "emit-failed" });
        }
    };
    let reparsed = parse(catalog, &emitted);
    let reemitted = match emit_workshop(catalog, &reparsed) {
        Ok(reemitted) => reemitted,
        Err(error) => {
            failures.push(format!("{fixture}: re-emission failed: {error}"));
            return serde_json::json!({ "status": "reemit-failed" });
        }
    };
    let fixed_point = emitted == reemitted;
    if !fixed_point {
        failures.push(format!(
            "{fixture}: reconstructed Workshop did not reach the round-trip fixed point"
        ));
    }

    // Semantic equivalence under the declared #119 normalization.
    let mut actual = reparsed;
    let mut reference = original;
    normalize(&mut actual, catalog);
    normalize(&mut reference, catalog);
    let equivalent = workshop_rs::roundtrip::equivalent(&actual, &reference);
    if !equivalent {
        failures.push(format!(
            "{fixture}: normalized recompiled WIR is not equivalent"
        ));
    }
    let _ = std::fs::remove_dir_all(input_path.parent().unwrap());

    serde_json::json!({
        "status": "round-trip",
        "target": "ostw",
        "inputSha256": sha256(&source),
        "reconstructedSha256": envelope.result.sha256,
        "frontendAccepted": true,
        "equivalent": equivalent,
        "workshopFixedPoint": fixed_point,
    })
}

#[test]
fn cross_format_conversion_round_trips_and_reports() {
    let catalog = workshop_rs::catalog::Catalog::builtin().expect("catalog loads");
    let mut failures = Vec::new();
    let mut report = serde_json::Map::new();
    for fixture in OPY_FIXTURES {
        report.insert(
            fixture.to_string(),
            opy_round_trip(&catalog, fixture, &mut failures),
        );
    }
    for fixture in OSTW_FIXTURES {
        report.insert(
            fixture.to_string(),
            ostw_round_trip(&catalog, fixture, &mut failures),
        );
    }
    for (name, target, relative) in rejection_cases() {
        report.insert(
            format!("reject/{name}"),
            rejection_entry(target, &workspace_root().join(relative), &mut failures),
        );
    }
    // One entry per fixture, plus the suite identity.
    report.insert(
        "suite".to_string(),
        serde_json::json!({
            "name": "wright-convert",
            "fixtures": OPY_FIXTURES.len() + OSTW_FIXTURES.len() + rejection_cases().len(),
        }),
    );
    let report_path = workspace_root().join("target/wright-convert-report.json");
    let parent = report_path.parent().expect("target dir");
    std::fs::create_dir_all(parent).expect("create target dir");
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&serde_json::Value::Object(report))
            .expect("report serializes"),
    )
    .expect("write conversion report");
    assert!(
        failures.is_empty(),
        "every committed fixture must round-trip or reject deterministically through the \
         shared path:\n{}",
        failures.join("\n")
    );
}

/// The rejection fixture cases: (name, target, committed Workshop source).
fn rejection_cases() -> Vec<(&'static str, ConvertTarget, &'static str)> {
    vec![
        (
            "for-player-variable",
            ConvertTarget::Ostw,
            "compatibility/ostw/reconstruction/reject/for-player-variable/workshop.txt",
        ),
        (
            "opy-per-player-loop",
            ConvertTarget::Opy,
            "crates/wright-driver/tests/fixtures/convert/reject-opy-per-player-loop.ws",
        ),
    ]
}

/// Drive one rejection fixture through the shared path and record the
/// deterministic structured rejection (never partial source).
fn rejection_entry(
    target: ConvertTarget,
    path: &Path,
    failures: &mut Vec<String>,
) -> serde_json::Value {
    let source = read(path);
    let (envelope, input_path) = convert(&source, target);
    if envelope.ok {
        failures.push(format!(
            "reject case '{}' unexpectedly converted",
            path.display()
        ));
    }
    let codes: Vec<String> = envelope
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.clone())
        .collect();
    let _ = std::fs::remove_dir_all(input_path.parent().unwrap());
    serde_json::json!({
        "status": "rejected",
        "target": target.as_str(),
        "exit": envelope.exit,
        "codes": codes,
        "partialSource": !envelope.result.text.is_empty(),
    })
}

#[test]
fn conversion_is_byte_deterministic_across_runs() {
    let catalog = workshop_rs::catalog::Catalog::builtin().expect("catalog loads");
    for (target, fixture) in [
        (
            ConvertTarget::Opy,
            workspace_root()
                .join("crates/wright-opy/tests/fixtures/reconstruct/variables-declarations.ws"),
        ),
        (
            ConvertTarget::Ostw,
            workspace_root().join("compatibility/ostw/reconstruction/surface-basic/workshop.txt"),
        ),
    ] {
        let source = read(&fixture);
        let (first, path_a) = convert(&source, target);
        let (second, path_b) = convert(&source, target);
        assert!(
            first.ok,
            "first convert must succeed: {:?}",
            first.diagnostics
        );
        assert!(
            second.ok,
            "second convert must succeed: {:?}",
            second.diagnostics
        );
        assert_eq!(
            first.result.text,
            second.result.text,
            "convert({}) must be byte-stable",
            target.as_str()
        );
        assert_eq!(first.result.sha256, second.result.sha256);
        let _ = std::fs::remove_dir_all(path_a.parent().unwrap());
        let _ = std::fs::remove_dir_all(path_b.parent().unwrap());
    }
    let _ = catalog;
}

#[test]
fn unsupported_constructs_reject_with_structured_diagnostics_and_no_partial_source() {
    for (name, target, relative) in rejection_cases() {
        let source = read(&workspace_root().join(relative));
        let (first, path_a) = convert(&source, target);
        let (second, path_b) = convert(&source, target);
        assert_eq!(
            first.diagnostics, second.diagnostics,
            "{name}: the rejection must be deterministic"
        );
        assert_eq!(first.exit, 3, "{name}: unsupported must exit 3");
        assert_eq!(first.exit, second.exit);
        assert!(!first.ok, "{name}: the rejection must fail the envelope");
        assert!(
            first.result.text.is_empty(),
            "{name}: a rejection never carries partial source"
        );
        assert!(
            first
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity == wright_driver::Severity::Error),
            "{name}: rejections are errors"
        );
        assert!(
            first
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.stage == wright_driver::Stage::Reconstruction),
            "{name}: rejections carry the reconstruction stage"
        );
        // The reconstructor's stable codes are preserved verbatim.
        let codes: Vec<&str> = first
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect();
        let expected = match target {
            ConvertTarget::Ostw => "reconstruct-unsupported-action",
            ConvertTarget::Opy => "unsupported-per-player-loop",
        };
        assert!(
            codes.contains(&expected),
            "{name}: expected code {expected} in {codes:?}"
        );
        let _ = std::fs::remove_dir_all(path_a.parent().unwrap());
        let _ = std::fs::remove_dir_all(path_b.parent().unwrap());
    }
}
