//! Analysis tests (#25): each shipped analysis has corpus-backed negative
//! cases and dedicated positive fixtures, and every finding links to a
//! source location and IR node.

use std::path::{Path, PathBuf};

use wright_analyzer::analysis::{self, Severity};
use wright_core::hir;
use wright_ir::lower;
use wright_ir::wir::Program as WirProgram;

fn fixture_path(fixture_id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../adapter/fixtures")
        .join(format!("{fixture_id}.json"))
}

fn local_fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.json"))
}

fn lower_program(path: &Path) -> WirProgram {
    let protocol = hir::parse_str(
        &std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("cannot read fixture {}: {error}", path.display())),
    )
    .expect("fixture parses");
    let model = protocol.to_ir().expect("fixture converts");
    lower::lower(&model).expect("fixture lowers")
}

fn corpus_program(fixture_id: &str) -> WirProgram {
    lower_program(&fixture_path(fixture_id))
}

fn findings_by_code(program: &WirProgram, code: &str) -> Vec<analysis::Finding> {
    analysis::analyze(program)
        .into_iter()
        .filter(|finding| finding.code == code)
        .collect()
}

#[test]
fn min_wait_loop_fires_on_hot_loops_in_the_corpus() {
    // The cake's `while true: ... wait(0.016)` and control-flow's
    // `while index < 3: ... wait()` both run at the maximum rate.
    let cake = corpus_program("real-world/overpy-cake");
    let findings = findings_by_code(&cake, "min-wait-loop");
    assert_eq!(findings.len(), 1, "one hot while loop in the cake");
    assert_eq!(findings[0].severity, Severity::Warning);
    assert!(findings[0].span.is_some(), "finding must carry its span");
    assert!(
        findings[0].action.is_some(),
        "finding must link to its action"
    );

    let control_flow = corpus_program("synthetic/control-flow");
    let findings = findings_by_code(&control_flow, "min-wait-loop");
    assert_eq!(
        findings.len(),
        1,
        "the bounded while waits at the minimum rate"
    );
}

#[test]
fn min_wait_loop_does_not_fire_without_loops_or_with_longer_waits() {
    let expressions = corpus_program("synthetic/expressions-values");
    assert!(findings_by_code(&expressions, "min-wait-loop").is_empty());

    // The dedicated fixture waits 0.1s per iteration: below the threshold.
    let slow = lower_program(&local_fixture_path("expensive-loop"));
    assert!(findings_by_code(&slow, "min-wait-loop").is_empty());
}

#[test]
fn duplicate_condition_fires_on_redundant_branches() {
    let program = lower_program(&local_fixture_path("duplicate-condition"));
    let findings = findings_by_code(&program, "duplicate-condition");
    assert_eq!(findings.len(), 1, "elif repeats the if condition");
    assert_eq!(findings[0].severity, Severity::Warning);
    assert!(findings[0].span.is_some());
    assert!(
        findings[0].value.is_some(),
        "finding must link to the condition value"
    );
}

#[test]
fn duplicate_condition_does_not_fire_on_distinct_conditions() {
    let control_flow = corpus_program("synthetic/control-flow");
    assert!(
        findings_by_code(&control_flow, "duplicate-condition").is_empty(),
        "index == 0 / index == 1 are distinct"
    );
    let cake = corpus_program("real-world/overpy-cake");
    assert!(findings_by_code(&cake, "duplicate-condition").is_empty());
}

#[test]
fn expensive_loop_check_fires_on_geometry_predicates_in_loops() {
    let program = lower_program(&local_fixture_path("expensive-loop"));
    let findings = findings_by_code(&program, "expensive-loop-check");
    assert_eq!(findings.len(), 1, "distance() inside the while body");
    assert_eq!(findings[0].severity, Severity::Info);
    assert!(findings[0].span.is_some());
    assert!(findings[0].value.is_some());
}

#[test]
fn expensive_loop_check_does_not_fire_on_the_corpus() {
    for fixture_id in [
        "synthetic/basic-rule",
        "synthetic/control-flow",
        "synthetic/declarations-rules",
        "synthetic/expressions-values",
        "synthetic/preprocessing",
        "real-world/overpy-cake",
    ] {
        let program = corpus_program(fixture_id);
        assert!(
            findings_by_code(&program, "expensive-loop-check").is_empty(),
            "{fixture_id} has no geometry predicate in a loop"
        );
    }
}

#[test]
fn analyze_aggregates_all_shipped_analyses() {
    let control_flow = corpus_program("synthetic/control-flow");
    let findings = analysis::analyze(&control_flow);
    let codes: std::collections::BTreeSet<&str> = findings.iter().map(|f| f.code).collect();
    assert!(
        codes.contains("min-wait-loop"),
        "aggregate run must include min-wait-loop: {codes:?}"
    );
    for finding in &findings {
        assert!(finding.span.is_some(), "{} must carry a span", finding.code);
    }
}
