//! Fix plans for `exact` findings (#556): `duplicate-condition` carries a
//! dead-branch removal plan and `repeated-value` carries `Evaluate Once`
//! wrapping — only where the mechanically determined correction keeps the
//! authored semantics.

use workshop_rs::catalog::Locale;
use workshop_rs::parser;
use workshop_rs::source::Span;
use wright_analyzer::canonical::{Finding, LintFix, analyze};
use wright_analyzer::registry::LintConfig;

fn analyze_source(source: &str) -> (workshop_rs::Program, Vec<Finding>) {
    let catalog = wright_analyzer::catalog::builtin().unwrap();
    let program = parser::parse_with_context(source, &catalog, &Locale::new("en-US"), &*catalog)
        .unwrap_or_else(|error| panic!("Workshop source must parse: {error}"));
    let findings = analyze(&program, &LintConfig::default());
    (program, findings)
}

fn coded<'a>(findings: &'a [Finding], code: &str) -> Vec<&'a Finding> {
    findings
        .iter()
        .filter(|finding| finding.code == code)
        .collect()
}

/// The authored text a span covers, resolved through the retained source.
fn span_text(program: &workshop_rs::Program, source: &str, span: Span) -> String {
    let range = program
        .source(span.file)
        .and_then(|document| document.byte_range(span))
        .unwrap_or_else(|| panic!("span resolves against the source: {span:?}"));
    source[range].to_string()
}

const DUPLICATE: &str = r#"variables {
    global:
        0: index
        1: other
}
rule ("duplicates") {
    event {
        Ongoing - Global;
    }
    actions {
        If(Compare(Global.index, ==, 0));
            Set Global Variable(other, 1);
        Else If(Compare(Global.index, ==, 1));
            Set Global Variable(other, 2);
        Else If(Compare(Global.index, ==, 0));
            Set Global Variable(other, 3);
        Else;
            Set Global Variable(other, 4);
        End;
    }
}
"#;

#[test]
fn duplicate_condition_offers_a_dead_branch_fix_over_the_branch_extent() {
    let (program, findings) = analyze_source(DUPLICATE);
    let [finding] = coded(&findings, "duplicate-condition")[..] else {
        panic!("one duplicate-condition finding: {findings:?}")
    };
    let Some(LintFix::RemoveDeadBranch { span }) = &finding.fix else {
        panic!("the finding carries a dead-branch fix: {finding:?}")
    };
    let removed = span_text(&program, DUPLICATE, *span);
    assert!(
        removed.contains("Else If(Compare(Global.index, ==, 0))")
            && removed.contains("Set Global Variable(other, 3)"),
        "the fix removes the marker and its body: {removed:?}"
    );
    assert!(
        !removed.contains("Else If(Compare(Global.index, ==, 1))") && !removed.contains("Else;"),
        "the fix stops before the next chain marker: {removed:?}"
    );
    // The deletion is the two dead lines exactly: it begins at the marker
    // line's start and ends where `Else;`'s line begins, so the standard
    // half-open splice removes them with their newlines.
    assert_eq!(
        (span.start.line, span.start.col, span.end.line, span.end.col),
        (15, 1, 17, 1)
    );
}

#[test]
fn duplicate_condition_fix_spans_a_nested_block_inside_the_dead_branch() {
    // A dead branch containing its own `If … End` block must not stop at
    // the nested `End`: the removal runs through the real chain boundary.
    let nested = r#"variables {
    global:
        0: index
        1: other
}
rule ("nested") {
    event {
        Ongoing - Global;
    }
    actions {
        If(Compare(Global.index, ==, 0));
            Set Global Variable(other, 1);
        Else If(Compare(Global.index, ==, 0));
            If(Compare(Global.other, >, 1));
                Set Global Variable(other, 2);
            End;
            Set Global Variable(other, 3);
        Else;
            Set Global Variable(other, 4);
        End;
    }
}
"#;
    let (program, findings) = analyze_source(nested);
    let [finding] = coded(&findings, "duplicate-condition")[..] else {
        panic!("one duplicate-condition finding: {findings:?}")
    };
    let Some(LintFix::RemoveDeadBranch { span }) = &finding.fix else {
        panic!("the finding carries a dead-branch fix: {finding:?}")
    };
    let removed = span_text(&program, nested, *span);
    assert!(
        removed.contains("Else If(Compare(Global.index, ==, 0))")
            && removed.contains("Set Global Variable(other, 3)"),
        "the fix covers the whole dead branch: {removed:?}"
    );
    assert!(
        removed.matches("End;").count() == 1,
        "the nested block's End is inside the removal, the chain's is not: {removed:?}"
    );
    assert!(
        !removed.contains("Else;") && !removed.contains("Set Global Variable(other, 4)"),
        "the fix stops at the Else marker: {removed:?}"
    );
}

#[test]
fn duplicate_condition_fix_ignores_disabled_markers_inside_the_dead_branch() {
    // `disabled` statements inside the dead branch delete with it, and a
    // disabled `If` block still counts toward depth — its `End` is not the
    // chain boundary.
    let disabled = r#"variables {
    global:
        0: index
        1: other
}
rule ("disabled") {
    event {
        Ongoing - Global;
    }
    actions {
        If(Compare(Global.index, ==, 0));
            Set Global Variable(other, 1);
        Else If(Compare(Global.index, ==, 0));
            disabled If(Compare(Global.other, >, 1));
                Set Global Variable(other, 2);
            End;
            disabled Set Global Variable(other, 3);
        Else;
            Set Global Variable(other, 4);
        End;
    }
}
"#;
    let (program, findings) = analyze_source(disabled);
    let [finding] = coded(&findings, "duplicate-condition")[..] else {
        panic!("one duplicate-condition finding: {findings:?}")
    };
    let Some(LintFix::RemoveDeadBranch { span }) = &finding.fix else {
        panic!("the finding carries a dead-branch fix: {finding:?}")
    };
    let removed = span_text(&program, disabled, *span);
    assert!(
        removed.contains("disabled If")
            && removed.contains("disabled Set Global Variable(other, 3)"),
        "disabled statements inside the branch delete with it: {removed:?}"
    );
    assert!(
        removed.matches("End;").count() == 1 && !removed.contains("Else;"),
        "the disabled block's End is inside the removal: {removed:?}"
    );
}

#[test]
fn duplicate_condition_fix_materializes_for_a_localized_source() {
    // Fix spans resolve through the retained source bytes, so a zh-CN
    // document plans the same dead-branch removal on its own spellings.
    let localized = r#"变量 {
    全局:
        0: index
        1: other
}
规则 ("duplicates") {
    事件 {
        持续 - 全局;
    }
    动作 {
        If(比较(全局.index, ==, 0));
            设置全局变量(other, 1);
        Else If(比较(全局.index, ==, 0));
            设置全局变量(other, 2);
        Else;
            设置全局变量(other, 3);
        End;
    }
}
"#;
    let catalog = wright_analyzer::catalog::builtin().unwrap();
    let program = parser::parse_with_context(localized, &catalog, &Locale::new("zh-CN"), &*catalog)
        .unwrap_or_else(|error| panic!("zh-CN source must parse: {error}"));
    let findings = analyze(&program, &LintConfig::default());
    let [finding] = coded(&findings, "duplicate-condition")[..] else {
        panic!("one duplicate-condition finding: {findings:?}")
    };
    let Some(LintFix::RemoveDeadBranch { span }) = &finding.fix else {
        panic!("the finding carries a dead-branch fix: {finding:?}")
    };
    let removed = span_text(&program, localized, *span);
    assert!(
        removed.contains("Else If(比较(全局.index, ==, 0))")
            && removed.contains("设置全局变量(other, 2)"),
        "the removal covers the localized dead branch: {removed:?}"
    );
    assert!(
        !removed.contains("Else;") && !removed.contains("设置全局变量(other, 3)"),
        "the fix stops at the Else marker: {removed:?}"
    );
}

#[test]
fn duplicate_condition_refuses_a_fix_when_the_condition_can_move() {
    let volatile = DUPLICATE.replace(
        "Else If(Compare(Global.index, ==, 0));",
        "Else If(Compare(Global.index, ==, Random Integer(0, 1)));",
    );
    // The duplicated condition itself must be the volatile one: repeat it.
    let volatile = volatile.replace(
        "If(Compare(Global.index, ==, 0));\n            Set Global Variable(other, 1);",
        "If(Compare(Global.index, ==, Random Integer(0, 1)));\n            Set Global Variable(other, 1);",
    );
    let (_, findings) = analyze_source(&volatile);
    let [finding] = coded(&findings, "duplicate-condition")[..] else {
        panic!("the volatile repeat still reports: {findings:?}")
    };
    assert!(
        finding.fix.is_none(),
        "a condition that can move between evaluations keeps its branch: {finding:?}"
    );
}

#[test]
fn duplicate_condition_fix_boundaries_cover_else_if_and_end_markers() {
    // Dead branch bounded by another `Else If`, then by `End`.
    let bounded_by_elseif = r#"variables {
    global:
        0: index
}
rule ("chain") {
    event {
        Ongoing - Global;
    }
    actions {
        If(Compare(Global.index, ==, 0));
            Wait(1, Ignore Condition);
        Else If(Compare(Global.index, ==, 0));
            Wait(1, Ignore Condition);
        Else If(Compare(Global.index, ==, 2));
            Wait(1, Ignore Condition);
        End;
    }
}
"#;
    let (program, findings) = analyze_source(bounded_by_elseif);
    let [finding] = coded(&findings, "duplicate-condition")[..] else {
        panic!("one duplicate-condition finding: {findings:?}")
    };
    let Some(LintFix::RemoveDeadBranch { span }) = &finding.fix else {
        panic!("dead-branch fix: {finding:?}")
    };
    let removed = span_text(&program, bounded_by_elseif, *span);
    assert!(removed.contains("Else If(Compare(Global.index, ==, 0))"));
    assert!(
        removed.trim_end().ends_with("Wait(1, Ignore Condition);")
            || removed.ends_with("Wait(1, Ignore Condition);\n")
    );
    let boundary_lines = bounded_by_elseif
        .lines()
        .nth(span.end.line as usize - 1)
        .unwrap();
    assert!(
        boundary_lines.trim_start().starts_with("Else If"),
        "the span ends at the next chain marker's line: {boundary_lines:?}"
    );

    let bounded_by_end = bounded_by_elseif.replace(
        "Else If(Compare(Global.index, ==, 2));\n            Wait(1, Ignore Condition);\n        End;",
        "End;",
    );
    let (_, findings) = analyze_source(&bounded_by_end);
    let [finding] = coded(&findings, "duplicate-condition")[..] else {
        panic!("one duplicate-condition finding: {findings:?}")
    };
    let Some(LintFix::RemoveDeadBranch { span }) = &finding.fix else {
        panic!("dead-branch fix: {finding:?}")
    };
    let boundary_line = bounded_by_end
        .lines()
        .nth(span.end.line as usize - 1)
        .unwrap();
    assert!(
        boundary_line.trim_start().starts_with("End"),
        "the span ends at the closing End's line: {boundary_line:?}"
    );
}

const REPEATED: &str = r#"variables {
    global:
        0: index
        1: other
}
rule ("repeat") {
    event {
        Ongoing - Global;
    }
    actions {
        While(Compare(Global.index, <, 3));
            Set Global Variable(other, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            Set Global Variable(index, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            Wait(0.016, Ignore Condition);
        End;
    }
}
"#;

#[test]
fn repeated_value_fix_wraps_each_occurrences_complete_expression() {
    let (program, findings) = analyze_source(REPEATED);
    let [finding] = coded(&findings, "repeated-value")[..] else {
        panic!("one repeated-value finding: {findings:?}")
    };
    let Some(LintFix::EvaluateOnce { occurrences }) = &finding.fix else {
        panic!("the finding carries an evaluate-once fix: {finding:?}")
    };
    assert_eq!(occurrences.len(), 2);
    for occurrence in occurrences {
        assert_eq!(
            span_text(&program, REPEATED, *occurrence),
            "Distance Between(Position Of(Event Player), Vector(0, 0, 0))",
            "the occurrence covers the complete call, not just its callee"
        );
    }
}

#[test]
fn repeated_value_treats_evaluate_once_as_already_resolved() {
    let wrapped = REPEATED.replace(
        "Set Global Variable(index, Distance Between(Position Of(Event Player), Vector(0, 0, 0)))",
        "Set Global Variable(index, Evaluate Once(Distance Between(Position Of(Event Player), Vector(0, 0, 0))))",
    );
    let (_, findings) = analyze_source(&wrapped);
    assert!(
        coded(&findings, "repeated-value").is_empty(),
        "the wrapped subtree is opaque, so no family remains: {findings:?}"
    );
}

#[test]
fn repeated_value_fix_wraps_a_multiline_expression_extent() {
    // The wrap inserts `Evaluate Once(` at the expression's start byte and
    // `)` at its end byte under standard splice semantics — an extent that
    // spans lines must cover the whole expression, not one line.
    let multiline = r#"variables {
    global:
        0: index
        1: other
}
rule ("multiline") {
    event {
        Ongoing - Global;
    }
    actions {
        While(Compare(Global.index, <, 3));
            Set Global Variable(other, Distance Between(
                Position Of(Event Player),
                Vector(0, 0, 0)));
            Set Global Variable(index, Distance Between(
                Position Of(Event Player),
                Vector(0, 0, 0)));
            Wait(0.016, Ignore Condition);
        End;
    }
}
"#;
    let (program, findings) = analyze_source(multiline);
    let [finding] = coded(&findings, "repeated-value")[..] else {
        panic!("one repeated-value finding: {findings:?}")
    };
    let Some(LintFix::EvaluateOnce { occurrences }) = &finding.fix else {
        panic!("both occurrences carry authored extents: {finding:?}")
    };
    assert_eq!(occurrences.len(), 2);
    for occurrence in occurrences {
        let text = span_text(&program, multiline, *occurrence);
        assert_eq!(
            text,
            "Distance Between(\n                Position Of(Event Player),\n                Vector(0, 0, 0))",
            "the extent covers the whole multiline call: {text:?}"
        );
    }
}

#[test]
fn repeated_value_never_wraps_the_loop_condition() {
    let in_condition = r#"variables {
    global:
        0: index
        1: other
}
rule ("loop") {
    event {
        Ongoing - Global;
    }
    actions {
        While(Compare(Global.index, <, Distance Between(Position Of(Event Player), Vector(0, 0, 0))));
            Set Global Variable(other, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            Set Global Variable(index, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            Wait(0.016, Ignore Condition);
        End;
    }
}
"#;
    let (_, findings) = analyze_source(in_condition);
    let [finding] = coded(&findings, "repeated-value")[..] else {
        panic!("one repeated-value finding: {findings:?}")
    };
    let Some(LintFix::EvaluateOnce { occurrences }) = &finding.fix else {
        panic!("the body occurrences still wrap: {finding:?}")
    };
    assert_eq!(
        occurrences.len(),
        2,
        "the re-evaluated While condition keeps its live read"
    );
    for occurrence in occurrences {
        let line = in_condition
            .lines()
            .nth(occurrence.start.line as usize - 1)
            .unwrap();
        assert!(
            line.trim_start().starts_with("Set Global Variable"),
            "wrapped occurrences stay inside loop-body actions: {line:?}"
        );
    }
}

#[test]
fn repeated_value_never_wraps_a_wait_until_condition() {
    let polled = r#"variables {
    global:
        0: index
        1: other
}
rule ("poll") {
    event {
        Ongoing - Global;
    }
    actions {
        While(Compare(Global.index, <, 3));
            Wait Until(Compare(Global.index, <, Distance Between(Position Of(Event Player), Vector(0, 0, 0))), 10, 30);
            Set Global Variable(other, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            Set Global Variable(index, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            Wait(0.016, Ignore Condition);
        End;
    }
}
"#;
    let (_, findings) = analyze_source(polled);
    let [finding] = coded(&findings, "repeated-value")[..] else {
        panic!("one repeated-value finding: {findings:?}")
    };
    let Some(LintFix::EvaluateOnce { occurrences }) = &finding.fix else {
        panic!("the polled condition is left live; the rest wrap: {finding:?}")
    };
    assert_eq!(occurrences.len(), 2);
    let polled_line = polled
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("Wait Until"))
        .map(|(index, _)| index as u32 + 1)
        .unwrap();
    assert!(
        occurrences
            .iter()
            .all(|occurrence| occurrence.start.line != polled_line),
        "Wait Until's polled arguments are never wrapped"
    );
}

#[test]
fn repeated_value_never_wraps_update_every_frame() {
    // `Update Every Frame` re-evaluates its argument every tick; freezing
    // inside it — or the call itself — would stale the live read it exists
    // to keep. The finding still reports; no fix is offered.
    let framed = r#"variables {
    global:
        0: index
        1: other
}
rule ("frames") {
    event {
        Ongoing - Global;
    }
    actions {
        While(Compare(Global.index, <, 3));
            Set Global Variable(other, Update Every Frame(Distance Between(Position Of(Event Player), Vector(0, 0, 0))));
            Set Global Variable(index, Update Every Frame(Distance Between(Position Of(Event Player), Vector(0, 0, 0))));
            Wait(0.016, Ignore Condition);
        End;
    }
}
"#;
    let (_, findings) = analyze_source(framed);
    let [finding] = coded(&findings, "repeated-value")[..] else {
        panic!("one repeated-value finding: {findings:?}")
    };
    assert!(
        finding.fix.is_none(),
        "both occurrences live inside Update Every Frame: {finding:?}"
    );

    // One framed occurrence still leaves a fix over the others.
    let mixed = framed.replacen(
        "Update Every Frame(Distance Between(Position Of(Event Player), Vector(0, 0, 0)))",
        "Distance Between(Position Of(Event Player), Vector(0, 0, 0))",
        1,
    );
    let (program, findings) = analyze_source(&mixed);
    let [finding] = coded(&findings, "repeated-value")[..] else {
        panic!("one repeated-value finding: {findings:?}")
    };
    let Some(LintFix::EvaluateOnce { occurrences }) = &finding.fix else {
        panic!("the plain occurrence still wraps: {finding:?}")
    };
    assert_eq!(occurrences.len(), 1);
    assert_eq!(
        span_text(&program, &mixed, occurrences[0]),
        "Distance Between(Position Of(Event Player), Vector(0, 0, 0))"
    );
}

#[test]
fn repeated_value_never_wraps_a_shape_containing_update_every_frame() {
    // The duplicated shape itself contains `Update Every Frame`: wrapping
    // the whole expression would freeze its per-tick update.
    let containing = r#"variables {
    global:
        0: index
        1: other
}
rule ("containing") {
    event {
        Ongoing - Global;
    }
    actions {
        While(Compare(Global.index, <, 3));
            Set Global Variable(other, Compare(Update Every Frame(Distance Between(Position Of(Event Player), Vector(0, 0, 0))), >, 4));
            Set Global Variable(index, Compare(Update Every Frame(Distance Between(Position Of(Event Player), Vector(0, 0, 0))), >, 4));
            Wait(0.016, Ignore Condition);
        End;
    }
}
"#;
    let (_, findings) = analyze_source(containing);
    let [finding] = coded(&findings, "repeated-value")[..] else {
        panic!("one repeated-value finding: {findings:?}")
    };
    assert!(
        finding.fix.is_none(),
        "an occurrence containing Update Every Frame is not wrappable: {finding:?}"
    );
}

#[test]
fn repeated_value_never_wraps_a_reevaluating_persistent_object() {
    // A persistent-object action whose reevaluation mode is enabled —
    // spelled as a non-`None` literal, or computed so it cannot be proven
    // `None` — re-evaluates the fields its mode selects; the catalog pins
    // which parameter carries the mode. A literal `None` mode does not
    // re-evaluate, so its occurrence still wraps.
    for (mode, wraps) in [
        ("Visible To and String", false),
        ("Global.mode", false),
        ("None", true),
    ] {
        let reevaluating = format!(
            r#"variables {{
    global:
        0: index
        1: other
        2: mode
}}
rule ("reevaluating") {{
    event {{
        Ongoing - Global;
    }}
    actions {{
        While(Compare(Global.index, <, 3));
            Set Objective Description(All Players(All Teams), Distance Between(Position Of(Event Player), Vector(0, 0, 0)), {mode});
            Set Global Variable(other, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            Wait(0.016, Ignore Condition);
        End;
    }}
}}
"#,
        );
        let (_, findings) = analyze_source(&reevaluating);
        let [finding] = coded(&findings, "repeated-value")[..] else {
            panic!("one repeated-value finding for mode {mode}: {findings:?}")
        };
        let Some(LintFix::EvaluateOnce { occurrences }) = &finding.fix else {
            panic!("one unwrappable occurrence still leaves a fix for mode {mode}: {finding:?}")
        };
        let wrapped_on_objective = occurrences.iter().any(|occurrence| {
            reevaluating
                .lines()
                .nth(occurrence.start.line as usize - 1)
                .is_some_and(|line| line.trim_start().starts_with("Set Objective Description"))
        });
        assert_eq!(
            wrapped_on_objective, wraps,
            "mode {mode}: occurrences {occurrences:?}"
        );
        assert_eq!(
            occurrences.len(),
            if wraps { 2 } else { 1 },
            "mode {mode}: {occurrences:?}"
        );
    }
}

#[test]
fn duplicate_condition_fix_applies_to_tick_snapshot_objective_reads() {
    // Positions and objective state are per-tick snapshot reads — they
    // cannot differ between the two synchronous evaluations, so the dead
    // branch fix still applies (unlike randoms, clocks, or sampled load).
    let snapshot = DUPLICATE.replace(
        "Compare(Global.index, ==, 0)",
        "Compare(Flag Position(Team 1), !=, Vector(0, 0, 0))",
    );
    let (program, findings) = analyze_source(&snapshot);
    let [finding] = coded(&findings, "duplicate-condition")[..] else {
        panic!("one duplicate-condition finding: {findings:?}")
    };
    let Some(LintFix::RemoveDeadBranch { span }) = &finding.fix else {
        panic!("snapshot-state duplicates still fix: {finding:?}")
    };
    assert!(span_text(&program, &snapshot, *span).contains("Flag Position"));
}
