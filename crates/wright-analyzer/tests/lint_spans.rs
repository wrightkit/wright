//! Findings locate themselves even when provenance is coarse: provider-backed
//! programs (OPy compilation) carry rule/condition/action-level spans but no
//! nested value provenance, so a finding over a value inside a condition or
//! action argument falls back to the nearest recorded ancestor rather than
//! reporting `null`.

use serde_json::Value;
use workshop_rs::catalog::Locale;
use workshop_rs::{MappedText, Program, SourceMap, parser};
use wright_analyzer::canonical::{Finding, analyze};
use wright_analyzer::registry::LintConfig;

fn parse(source: &str) -> Program {
    let catalog = wright_analyzer::catalog::builtin().unwrap();
    parser::parse_with_context(source, &catalog, &Locale::new("en-US"), &*catalog)
        .unwrap_or_else(|error| panic!("Workshop source must parse: {error}"))
}

/// A program carrying only rule/condition/action-argument provenance — the
/// shape a source provider emits: every recorded value's `children` cleared.
fn shallowly_mapped(source: &str) -> Program {
    let program = parse(source);
    let mut artifact: Value =
        serde_json::from_str(&MappedText::new(source, SourceMap::extract(&program)).to_json())
            .unwrap();
    strip_value_children(&mut artifact);
    let mapped = MappedText::from_json(&serde_json::to_string(&artifact).unwrap()).unwrap();
    let mut program = parse(source);
    mapped.map.apply(&mut program).unwrap();
    program
}

fn strip_value_children(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.insert("children".into(), Value::Array(vec![]));
            for child in map.values_mut() {
                strip_value_children(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                strip_value_children(item);
            }
        }
        _ => {}
    }
}

fn coded<'a>(findings: &'a [Finding], code: &str) -> Vec<&'a Finding> {
    findings
        .iter()
        .filter(|finding| finding.code == code)
        .collect()
}

const HOT_CONDITION: &str = r#"rule ("hot condition") {
    event {
        Ongoing - Global;
    }
    conditions {
        Compare(Distance Between(Global.target, Vector(0, 0, 0)), >, 10);
        Global.index > 0;
    }
    actions {
        Set Global Variable(index, 1);
    }
}
"#;

#[test]
fn ongoing_condition_finding_falls_back_to_the_condition_span() {
    let program = shallowly_mapped(HOT_CONDITION);
    let findings = analyze(&program, &LintConfig::default());
    let [finding] = coded(&findings, "ongoing-condition-hot-path")[..] else {
        panic!("one ongoing-condition-hot-path finding: {findings:?}")
    };
    assert_eq!(
        finding.span,
        program.condition_span(0, 0),
        "the finding carries the enclosing condition's span"
    );
}

const HOT_LOOP: &str = r#"variables {
    global:
        0: index
        1: other
}
rule ("hot loop") {
    event {
        Ongoing - Global;
    }
    actions {
        While(Compare(Global.index, <, 3));
            Set Global Variable(other, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            Wait(0.016, Ignore Condition);
        End;
    }
}
"#;

#[test]
fn loop_body_finding_falls_back_to_the_argument_span() {
    let program = shallowly_mapped(HOT_LOOP);
    let findings = analyze(&program, &LintConfig::default());
    let [finding] = coded(&findings, "expensive-loop-check")[..] else {
        panic!("one expensive-loop-check finding: {findings:?}")
    };
    // The expensive call is the `Set Global Variable` value argument: with no
    // nested provenance the finding reports the argument's own span — the
    // tightest recorded ancestor that still encloses it. The variable slot is
    // an identifier, not a value argument, so the call is argument 0.
    assert_eq!(
        finding.span,
        program.action_argument_span(0, 1, 0),
        "the finding carries the enclosing argument's span"
    );
}

/// Two distinct duplicated shapes in one loop: each `repeated-value` finding
/// names its expression in canonical spelling, so the findings are
/// distinguishable from the message alone (#735).
const TWO_REPEATED: &str = r#"variables {
    global:
        0: index
        1: other
}
rule ("two repeated expressions") {
    event {
        Ongoing - Global;
    }
    actions {
        While(Compare(Global.index, <, 3));
            Set Global Variable(other, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            Set Global Variable(other, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            Set Global Variable(other, Vector Towards(Position Of(Event Player), Vector(1, 1, 1)));
            Set Global Variable(other, Vector Towards(Position Of(Event Player), Vector(1, 1, 1)));
            Wait(0.016, Ignore Condition);
        End;
    }
}
"#;

#[test]
fn repeated_value_names_each_repeated_expression() {
    let program = parse(TWO_REPEATED);
    let findings = analyze(&program, &LintConfig::default());
    let repeated = coded(&findings, "repeated-value");
    let [distance, towards] = repeated[..] else {
        panic!("one finding per repeated expression: {findings:?}")
    };
    assert_eq!(
        distance.message,
        "`distance(getPosition(eventPlayer), vector(0, 0, 0))` is evaluated 2 times within the same loop scope"
    );
    assert_eq!(
        towards.message,
        "`vectorTowards(getPosition(eventPlayer), vector(1, 1, 1))` is evaluated 2 times within the same loop scope"
    );
    // Fully spanned Workshop provenance locates each finding on an occurrence
    // of its expression — never on the enclosing loop header.
    assert_ne!(distance.span, towards.span);
    assert!(
        ![distance.span, towards.span].contains(&program.action_span(0, 0)),
        "the findings carry expression spans, not the loop action's: {distance:?} {towards:?}"
    );
}

/// The provider shape — rule/action-level provenance with no nested value
/// spans — still names the expression and says the location is the enclosing
/// loop rather than the expression's own (#735).
#[test]
fn repeated_value_without_value_spans_says_the_location_is_the_loop() {
    let source = r#"variables {
    global:
        0: index
        1: other
}
rule ("nested repeat") {
    event {
        Ongoing - Global;
    }
    actions {
        While(Compare(Global.index, <, 3));
            Set Global Variable(other, Add(Distance Between(Position Of(Event Player), Vector(0, 0, 0)), 1));
            Set Global Variable(other, Add(Distance Between(Position Of(Event Player), Vector(0, 0, 0)), 2));
            Wait(0.016, Ignore Condition);
        End;
    }
}
"#;
    let program = shallowly_mapped(source);
    let findings = analyze(&program, &LintConfig::default());
    let [finding] = coded(&findings, "repeated-value")[..] else {
        panic!("one repeated-value finding: {findings:?}")
    };
    assert_eq!(
        finding.message,
        "`distance(getPosition(eventPlayer), vector(0, 0, 0))` is evaluated 2 times within the same loop scope; the expression's own span is not recorded, so the location is the enclosing loop"
    );
    assert_eq!(
        finding.span,
        program.action_span(0, 0),
        "with no recorded value span the finding locates the enclosing While"
    );
}
