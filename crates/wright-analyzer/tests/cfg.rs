//! Corpus-backed tests for the control-flow graph (#24): representative
//! rules produce inspectable CFGs with explicit branches, loops, back-edges,
//! and timing flags.

use std::path::{Path, PathBuf};

use workshop_rs::wir::{Program as WirProgram, RuleId};
use wright_analyzer::cfg::{BlockKind, Cfg, EdgeKind};
use wright_core::hir;
use wright_ir::lower;

fn fixture_path(fixture_id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../adapter/fixtures")
        .join(format!("{fixture_id}.json"))
}

fn read_fixture(fixture_id: &str) -> String {
    std::fs::read_to_string(fixture_path(fixture_id))
        .unwrap_or_else(|error| panic!("cannot read adapter fixture {fixture_id}: {error}"))
}

fn lower_fixture(fixture_id: &str) -> WirProgram {
    let protocol = hir::parse_str(&read_fixture(fixture_id))
        .unwrap_or_else(|error| panic!("{fixture_id} must parse: {error}"));
    let model = protocol
        .to_ir()
        .unwrap_or_else(|error| panic!("{fixture_id} must convert: {error}"));
    lower::lower(&model).unwrap_or_else(|error| panic!("{fixture_id} must lower: {error}"))
}

fn cfg_for(program: &WirProgram, rule_name: &str) -> Cfg {
    let rule = program
        .rules
        .iter()
        .enumerate()
        .find(|(_, rule)| rule.name == rule_name)
        .unwrap_or_else(|| panic!("rule {rule_name} not found"))
        .0;
    Cfg::build(program, RuleId::from_index(rule)).expect("CFG builds")
}

const ADAPTER_FIXTURES: &[&str] = &[
    "synthetic/basic-rule",
    "synthetic/control-flow",
    "synthetic/declarations-rules",
    "synthetic/expressions-values",
    "synthetic/preprocessing",
    "real-world/overpy-cake",
];

#[test]
fn every_rule_builds_an_inspectable_cfg() {
    for fixture_id in ADAPTER_FIXTURES {
        let program = lower_fixture(fixture_id);
        for (index, _) in program.rules.iter().enumerate() {
            let cfg = Cfg::build(&program, RuleId::from_index(index)).unwrap_or_else(|error| {
                panic!("{fixture_id} rule {index} must build a CFG: {error}")
            });
            // Entry is always reachable and the exit is reachable from it.
            assert!(cfg.reachable(cfg.entry()));
            assert!(
                cfg.reachable(cfg.exit()),
                "{fixture_id} rule {index} must reach exit"
            );
            let dump = cfg.dump(&program);
            assert!(
                !dump.is_empty(),
                "{fixture_id} rule {index} dump must not be empty"
            );
        }
    }
}

#[test]
fn while_loop_has_back_edge_and_exit_edge() {
    let program = lower_fixture("synthetic/control-flow");
    let cfg = cfg_for(&program, "bounded while");

    let headers = cfg.loop_headers();
    assert_eq!(headers.len(), 1, "bounded while has one loop header");
    let header = headers[0];
    assert!(matches!(
        cfg.block(header).unwrap().kind,
        BlockKind::While { .. }
    ));

    let edges = cfg.successors(header);
    assert!(
        edges.iter().any(|(_, kind)| *kind == EdgeKind::LoopExit),
        "loop header must have an exit edge"
    );
    // The back-edge is the body's edge back into the header.
    assert!(
        cfg.predecessors(header)
            .iter()
            .any(|(_, kind)| *kind == EdgeKind::BackEdge),
        "the loop body must have a back-edge into the header"
    );

    // The wait lives in the loop body, so a body block is flagged.
    let wait_blocks = cfg.blocks_with_waits();
    assert_eq!(wait_blocks.len(), 1, "the wait action must flag its block");
    let wait_block = wait_blocks[0];
    assert_ne!(
        wait_block, header,
        "the wait is in the body, not the header"
    );
}

#[test]
fn if_statement_produces_branch_edges() {
    let program = lower_fixture("synthetic/control-flow");
    let cfg = cfg_for(&program, "control flow");

    let if_blocks: Vec<_> = cfg
        .blocks()
        .filter(|block| {
            matches!(
                cfg.block(*block).map(|b| &b.kind),
                Some(BlockKind::If { .. })
            )
        })
        .collect();
    assert_eq!(if_blocks.len(), 2, "if and elif each get an If block");
    for block in &if_blocks {
        let edges = cfg.successors(*block);
        assert!(
            edges.iter().any(|(_, kind)| *kind == EdgeKind::BranchTrue),
            "If block must branch true"
        );
        assert!(
            edges.iter().any(|(_, kind)| *kind == EdgeKind::BranchFalse),
            "If block must branch false"
        );
    }
}

#[test]
fn for_loop_header_is_a_loop() {
    let program = lower_fixture("synthetic/control-flow");
    let cfg = cfg_for(&program, "control flow");
    let headers = cfg.loop_headers();
    assert!(
        headers.iter().any(|header| matches!(
            cfg.block(*header).unwrap().kind,
            BlockKind::ForHeader { .. }
        )),
        "the for loop must be a loop header"
    );
}

#[test]
fn cfg_dump_is_deterministic() {
    let program = lower_fixture("synthetic/control-flow");
    let cfg = cfg_for(&program, "control flow");
    let first = cfg.dump(&program);
    let second = cfg.dump(&program);
    assert_eq!(first, second);
}

#[test]
fn blocks_without_waits_are_not_flagged() {
    let program = lower_fixture("synthetic/expressions-values");
    let cfg = cfg_for(&program, "expressions and values");
    assert!(
        cfg.blocks_with_waits().is_empty(),
        "no waits in expressions-values"
    );
}

#[test]
fn action_to_block_mapping_is_precise() {
    let program = lower_fixture("synthetic/control-flow");
    let cfg = cfg_for(&program, "bounded while");
    let rule = program
        .rules
        .iter()
        .find(|rule| rule.name == "bounded while")
        .expect("rule");
    for action in &rule.actions {
        assert!(
            cfg.action_block(*action).is_some(),
            "every action must map to a block"
        );
    }
}
