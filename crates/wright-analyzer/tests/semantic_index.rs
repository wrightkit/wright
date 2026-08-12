//! Corpus-backed tests for the semantic index (#23): symbols, references,
//! find-references, and usage queries over every v0.1 bridge fixture.

use std::path::{Path, PathBuf};

use wright_analyzer::symbols::{ReferenceKind, SemanticIndex, SymbolKind};
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

fn build_index(fixture_id: &str) -> SemanticIndex {
    let protocol = hir::parse_str(&read_fixture(fixture_id))
        .unwrap_or_else(|error| panic!("{fixture_id} must parse: {error}"));
    let model = protocol
        .to_ir()
        .unwrap_or_else(|error| panic!("{fixture_id} must convert: {error}"));
    let program =
        lower::lower(&model).unwrap_or_else(|error| panic!("{fixture_id} must lower: {error}"));
    SemanticIndex::build(&program)
        .unwrap_or_else(|error| panic!("{fixture_id} must index: {error}"))
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
fn every_fixture_builds_a_symbol_table() {
    for fixture_id in ADAPTER_FIXTURES {
        let index = build_index(fixture_id);
        assert!(
            index.symbols().next().is_some(),
            "{fixture_id} must declare symbols"
        );
    }
}

#[test]
fn lookup_and_filter_queries_work() {
    let index = build_index("synthetic/control-flow");
    let by_name = index.find_by_name("index");
    assert_eq!(by_name.len(), 1, "index must be a single global symbol");
    let symbol = index.symbol(by_name[0]).expect("symbol exists");
    assert_eq!(symbol.kind, SymbolKind::GlobalVariable);
    assert_eq!(symbol.name, "index");
    assert!(symbol.span.is_some(), "declaration must carry its span");

    let rules = index.symbols_of(SymbolKind::Rule);
    assert_eq!(rules.len(), 2, "control-flow has two rules");
    let globals = index.symbols_of(SymbolKind::GlobalVariable);
    assert_eq!(globals.len(), 1);
}

#[test]
fn find_references_and_usage_for_loop_variable() {
    let index = build_index("synthetic/control-flow");
    let symbol = index.find_by_name("index")[0];
    let references = index.references(symbol);

    // Writes: the for-loop variable binding and the compound-assignment
    // modify. Reads: two if/elif conditions, three debug reads, and the
    // while condition. (The modify's operand is a literal, so it contributes
    // no read.)
    let writes = references
        .iter()
        .filter(|reference| reference.kind == ReferenceKind::Write)
        .count();
    let reads = references
        .iter()
        .filter(|reference| reference.kind == ReferenceKind::Read)
        .count();
    assert_eq!(writes, 2, "for-loop bind + modify write");
    assert_eq!(
        reads, 6,
        "two conditions + three debug reads + while condition"
    );
    assert!(
        references.iter().all(|reference| reference.span.is_some()),
        "references must preserve source locations"
    );

    let usage = index.usage(symbol);
    assert_eq!(usage.reads, 6);
    assert_eq!(usage.writes, 2);
    assert_eq!(usage.calls, 0);
    assert_eq!(usage.rules, 2, "index is used in both rules");
}

#[test]
fn find_references_for_subroutine() {
    let index = build_index("synthetic/declarations-rules");
    let symbol = index.find_by_name("showStatus")[0];
    let references = index.references(symbol);

    let kinds: Vec<ReferenceKind> = references.iter().map(|reference| reference.kind).collect();
    assert!(
        kinds.contains(&ReferenceKind::Declaration),
        "subroutine declaration must be indexed"
    );
    assert!(
        kinds.contains(&ReferenceKind::Call),
        "subroutine call in the rule must be indexed"
    );
    assert!(
        kinds.contains(&ReferenceKind::Definition),
        "the def body rule event must be indexed"
    );

    let call = references
        .iter()
        .find(|reference| reference.kind == ReferenceKind::Call)
        .expect("call reference");
    assert!(call.rule.is_some(), "call must be tied to its rule");
    assert!(call.action.is_some(), "call must be tied to its action");
    assert!(call.span.is_some(), "call must carry its span");

    let usage = index.usage(symbol);
    assert_eq!(usage.calls, 1);
    assert_eq!(usage.rules, 2, "call rule + def rule");
}

#[test]
fn player_variable_writes_are_indexed() {
    let index = build_index("synthetic/declarations-rules");
    let symbol = index.find_by_name("hasStarted")[0];
    assert_eq!(
        index.symbol(symbol).unwrap().kind,
        SymbolKind::PlayerVariable
    );
    let references = index.references(symbol);
    let writes = references
        .iter()
        .filter(|reference| reference.kind == ReferenceKind::Write)
        .count();
    assert_eq!(writes, 1, "eventPlayer.hasStarted = true");
}

#[test]
fn value_reads_are_tied_to_precise_spans() {
    // `points.append(result)` in expressions-values reads `points` (receiver)
    // and `result`; the reads must carry value-level spans, not just the
    // action span.
    let index = build_index("synthetic/expressions-values");
    let points = index.find_by_name("points")[0];
    let reads: Vec<_> = index
        .references(points)
        .into_iter()
        .filter(|reference| reference.kind == ReferenceKind::Read)
        .collect();
    assert!(
        reads.iter().all(|reference| reference.value.is_some()),
        "value reads must be tied to value nodes"
    );
    assert!(
        reads.iter().all(|reference| reference.span.is_some()),
        "value reads must carry precise spans"
    );
}
