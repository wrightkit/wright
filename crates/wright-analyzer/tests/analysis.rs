//! Analysis tests (#25): each shipped analysis has corpus-backed negative
//! cases and dedicated positive fixtures, and every finding links to a
//! source location and IR node.

use std::path::{Path, PathBuf};

use workshop_rs::wir::Program as WirProgram;
use wright_analyzer::analysis::{self, Boundedness, EvidenceClass, Severity};
use wright_core::hir;
use wright_ir::lower;

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

#[test]
fn repeated_value_fires_on_duplicates_within_one_action() {
    // Parabola shape: two duplicated shape families inside one action of a
    // `For Global Variable` loop — the `distance(...)` family (2
    // occurrences) and the `time - offsets[I]` family (3 occurrences) — each
    // reporting exactly one finding at its first occurrence with the
    // statically known occurrence count in the message.
    let program = lower_program(&local_fixture_path("repeated-value-parabola"));
    let findings = findings_by_code(&program, "repeated-value");
    assert_eq!(findings.len(), 2, "one finding per duplicated shape family");
    assert_eq!(
        findings[0].message,
        "this value expression is evaluated 2 times within the same loop scope",
        "the distance family occurs twice"
    );
    assert_eq!(
        findings[1].message,
        "this value expression is evaluated 3 times within the same loop scope",
        "the subtract family occurs three times"
    );
    for finding in &findings {
        assert_eq!(finding.severity, Severity::Warning);
        assert_eq!(finding.evidence, EvidenceClass::Exact);
        assert!(finding.span.is_some(), "finding must carry its span");
        assert!(
            finding.action.is_some(),
            "finding must link to the loop action"
        );
        assert!(
            finding.value.is_some(),
            "finding must link to the offending value"
        );
    }
}

#[test]
fn repeated_value_fires_on_cross_action_duplicates() {
    // Santa shape: two families of an identical `vectorTowards(...)`
    // sub-expression (AB and AD) evaluated across sibling modify actions,
    // three occurrences per family. Under the amended per-shape reporting
    // rule each family fires exactly one finding at its first occurrence,
    // with the occurrence count in the message (3 each): 2 findings total.
    let program = lower_program(&local_fixture_path("repeated-value-santa"));
    let findings = findings_by_code(&program, "repeated-value");
    assert_eq!(findings.len(), 2, "one finding per family (AB, AD)");
    assert_ne!(
        findings[0].span, findings[1].span,
        "each family fires at its own first occurrence"
    );
    for finding in &findings {
        assert_eq!(finding.severity, Severity::Warning);
        assert_eq!(finding.evidence, EvidenceClass::Exact);
        assert_eq!(
            finding.message,
            "this value expression is evaluated 3 times within the same loop scope",
            "each family occurs three times"
        );
        assert!(finding.span.is_some(), "finding must carry its span");
        assert!(
            finding.action.is_some(),
            "finding must link to the loop action"
        );
        assert!(
            finding.value.is_some(),
            "finding must link to the offending value"
        );
    }
}

#[test]
fn repeated_value_reports_maximal_shapes_and_per_scope() {
    // The shapes fixture pins two amended semantics: (a) a duplicated inner
    // shape nested inside a duplicated larger shape is subsumed (only the
    // maximal shape fires); (b) the same duplicated shape in two separate
    // loop scopes of one rule fires once per scope. Rule 1 yields one
    // finding; rule 2 yields two (one per loop scope): three total.
    let program = lower_program(&local_fixture_path("repeated-value-shapes"));
    let findings = findings_by_code(&program, "repeated-value");
    assert_eq!(
        findings.len(),
        3,
        "one maximal finding in rule 1, one per scope in rule 2"
    );
    for finding in &findings {
        assert_eq!(finding.severity, Severity::Warning);
        assert_eq!(finding.evidence, EvidenceClass::Exact);
        assert_eq!(
            finding.message,
            "this value expression is evaluated 2 times within the same loop scope",
            "the maximal shape occurs twice per scope"
        );
        assert!(finding.span.is_some(), "finding must carry its span");
        assert!(finding.action.is_some());
        assert!(finding.value.is_some());
    }
}

#[test]
fn repeated_value_does_not_fire_on_negative_cases() {
    let program = lower_program(&local_fixture_path("repeated-value-negative"));
    assert!(
        findings_by_code(&program, "repeated-value").is_empty(),
        "no repeated-value finding on any REQ-001 negative case"
    );
}

#[test]
fn repeated_value_does_not_fire_on_the_corpus() {
    // The synthetic corpus fixtures contain no duplicated non-trivial value
    // in a loop scope. overpy-cake is excluded: its geometry-building loops
    // genuinely duplicate multi-call shapes (e.g. `cakePos[N]+vect(...)` and
    // the `CAKE_LONG-(abs(i2)-...)` shape), so it is a real positive
    // observation, not a negative case (see REQ-001 smoke-check evidence).
    for fixture_id in [
        "synthetic/basic-rule",
        "synthetic/control-flow",
        "synthetic/declarations-rules",
        "synthetic/expressions-values",
        "synthetic/preprocessing",
    ] {
        let program = corpus_program(fixture_id);
        assert!(
            findings_by_code(&program, "repeated-value").is_empty(),
            "{fixture_id} must produce no repeated-value findings"
        );
    }
}

#[test]
fn repeated_value_findings_are_deterministic() {
    let program = lower_program(&local_fixture_path("repeated-value-santa"));
    let first = findings_by_code(&program, "repeated-value");
    let second = findings_by_code(&program, "repeated-value");
    assert_eq!(
        first.len(),
        second.len(),
        "finding count must be identical across runs"
    );
    for (a, b) in first.iter().zip(second.iter()) {
        assert_eq!(a.code, b.code, "finding codes must be stable");
        assert_eq!(a.severity, b.severity, "finding severities must be stable");
        assert_eq!(a.span, b.span, "finding spans must be stable");
        assert_eq!(a.action, b.action, "finding actions must be stable");
        assert_eq!(a.value, b.value, "finding values must be stable");
    }
}

#[test]
fn while_without_wait_fires_on_waardless_while() {
    let program = lower_program(&local_fixture_path("while-without-wait-positive"));
    let findings = findings_by_code(&program, "while-without-wait");
    assert_eq!(findings.len(), 1, "one waardless while loop");
    assert_eq!(findings[0].severity, Severity::Warning);
    assert_eq!(findings[0].evidence, EvidenceClass::StaticIndicator);
    assert_eq!(
        findings[0].boundedness,
        Some(Boundedness::ObviouslyUnbounded),
        "the waardless `while true:` is obviously unbounded"
    );
    assert!(findings[0].span.is_some(), "finding must carry its span");
    assert!(
        findings[0].action.is_some(),
        "finding must link to the while action"
    );
    assert!(
        findings[0].value.is_none(),
        "the finding targets the loop action, not a value"
    );
}

#[test]
fn while_without_wait_classifies_bounded_loop() {
    // Rule 1 is the agent-lab#68 repro shape (loopCount < 10, loopCount += 1);
    // rule 2 pins the subtract direction (n > 0, n -= 1); rules 3-4 pin sound
    // nested loops (a nested counter `while` and a nested `For Global
    // Variable` with a non-zero literal step whose body does not write the
    // loop variable) whose own finite termination keeps the outer loops
    // bounded. All five findings are statically bounded counter loops,
    // reported at info severity, and none is overstated as an unbounded
    // hazard.
    let program = lower_program(&local_fixture_path("no-yield-bounded"));
    let findings = findings_by_code(&program, "while-without-wait");
    assert_eq!(
        findings.len(),
        5,
        "two counter loops plus the nested counter-loop pair and the nested-for outer loop"
    );
    for finding in &findings {
        assert_eq!(
            finding.boundedness,
            Some(Boundedness::StaticallyBounded),
            "{}: bounded counter loop must be classified as statically bounded",
            finding.message
        );
        assert_eq!(
            finding.severity,
            Severity::Info,
            "a statically bounded no-yield loop must not carry warning severity"
        );
        assert_eq!(finding.evidence, EvidenceClass::StaticIndicator);
        assert!(
            finding.message.contains("statically bounded"),
            "the message must state the boundedness class: {}",
            finding.message
        );
        assert!(
            finding.message.contains("no wait call"),
            "the message must state the no-yield fact: {}",
            finding.message
        );
        assert!(finding.span.is_some(), "finding must carry its span");
        assert!(
            finding.action.is_some(),
            "finding must link to the while action"
        );
    }
}

#[test]
fn while_without_wait_classifies_unknown_boundedness() {
    // Data-dependent condition (a < b) with no counter pattern; conditional
    // progress (modify nested in an if); and the issue #103 counterexample
    // classes (away-writer, conditional reset, subroutine writer, non-literal
    // step, nested-loop writer) plus the nested-loop termination corner cases
    // (a non-terminating inner `while true:` that does not write the counter,
    // and a zero-step `for` nested inside an `if`) must all be reported
    // explicitly as unknown boundedness, not guessed. The only non-unknown
    // findings are the two inner `while true:` loops, which are obviously
    // unbounded.
    let program = lower_program(&local_fixture_path("no-yield-unknown"));
    let findings = findings_by_code(&program, "while-without-wait");
    assert_eq!(
        findings.len(),
        11,
        "nine waardless while loops plus the two nested inner `while true:` loops"
    );
    let unknown: Vec<&analysis::Finding> = findings
        .iter()
        .filter(|finding| finding.boundedness == Some(Boundedness::Unknown))
        .collect();
    assert_eq!(
        unknown.len(),
        9,
        "every outer/data loop is classified unknown; only the two nested inner loops are not"
    );
    for finding in &findings {
        assert_eq!(
            finding.severity,
            Severity::Warning,
            "every no-yield finding in this fixture carries warning severity: {}",
            finding.message
        );
        assert_eq!(finding.evidence, EvidenceClass::StaticIndicator);
        assert!(finding.span.is_some(), "finding must carry its span");
        assert!(
            finding.action.is_some(),
            "finding must link to the while action"
        );
    }
    for finding in &unknown {
        assert!(
            finding.message.contains("unknown"),
            "the message must state the unknown boundedness class: {}",
            finding.message
        );
    }
    let obvious: Vec<&analysis::Finding> = findings
        .iter()
        .filter(|finding| finding.boundedness == Some(Boundedness::ObviouslyUnbounded))
        .collect();
    assert_eq!(
        obvious.len(),
        2,
        "the two nested inner `while true:` loops are the obviously-unbounded findings"
    );
    for finding in &obvious {
        assert!(
            finding.message.contains("statically true"),
            "the inner loop message states the statically-true condition: {}",
            finding.message
        );
    }
}

#[test]
fn while_without_wait_unbounded_finding_does_not_claim_a_guaranteed_crash() {
    let program = lower_program(&local_fixture_path("no-yield-unbounded"));
    let findings = findings_by_code(&program, "while-without-wait");
    assert_eq!(findings.len(), 1, "one obviously unbounded no-yield loop");
    assert_eq!(
        findings[0].boundedness,
        Some(Boundedness::ObviouslyUnbounded)
    );
    assert_eq!(findings[0].severity, Severity::Warning);
    assert!(
        findings[0].message.contains("never terminates on its own"),
        "the message must state the static non-termination fact"
    );
    assert!(
        findings[0].message.contains("not statically measurable"),
        "the message must not claim a measured runtime cost"
    );
    assert!(
        !findings[0].message.to_lowercase().contains("crash"),
        "the message must not claim a guaranteed crash"
    );
    assert!(findings[0].span.is_some(), "finding must carry its span");
    assert!(
        findings[0].action.is_some(),
        "finding must link to the while action"
    );
}

#[test]
fn while_without_wait_does_not_fire_when_body_has_wait() {
    // Literal wait, computed wait duration, wait nested in an If, and a
    // waardless `For Global Variable` loop must all stay silent.
    let program = lower_program(&local_fixture_path("while-without-wait-negative"));
    assert!(
        findings_by_code(&program, "while-without-wait").is_empty(),
        "no while-without-wait finding on any REQ-002 negative case"
    );
}

#[test]
fn while_without_wait_does_not_fire_on_the_corpus() {
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
            findings_by_code(&program, "while-without-wait").is_empty(),
            "{fixture_id} must produce no while-without-wait findings"
        );
    }
}

#[test]
fn while_without_wait_findings_are_deterministic() {
    let program = lower_program(&local_fixture_path("while-without-wait-positive"));
    let first = findings_by_code(&program, "while-without-wait");
    let second = findings_by_code(&program, "while-without-wait");
    assert_eq!(
        first.len(),
        second.len(),
        "finding count must be identical across runs"
    );
    for (a, b) in first.iter().zip(second.iter()) {
        assert_eq!(a.code, b.code, "finding codes must be stable");
        assert_eq!(a.severity, b.severity, "finding severities must be stable");
        assert_eq!(a.span, b.span, "finding spans must be stable");
        assert_eq!(a.action, b.action, "finding actions must be stable");
        assert_eq!(
            a.boundedness, b.boundedness,
            "finding boundedness must be stable"
        );
    }
}
