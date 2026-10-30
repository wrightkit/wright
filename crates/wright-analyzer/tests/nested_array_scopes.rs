use workshop_rs::{Event, Program, Rule, Value};
use wright_analyzer::canonical::analyze;
use wright_analyzer::registry::LintConfig;

#[test]
fn nested_binders_require_a_read_of_their_array_binding() {
    let binders = [
        "filteredArray",
        "sortedArray",
        "mappedArray",
        "isTrueForAny",
        "isTrueForAll",
    ];
    for outer in binders {
        for inner in binders {
            for (source, body, expected) in [
                (Value::Array(vec![]), Value::Bool(true), None),
                (
                    Value::Array(vec![]),
                    Value::call("currentArrayElement", []),
                    Some(inner),
                ),
                (
                    Value::Array(vec![]),
                    Value::call("currentArrayIndex", []),
                    Some(inner),
                ),
                (
                    Value::Array(vec![Value::call("currentArrayElement", [])]),
                    Value::Bool(true),
                    Some(inner),
                ),
                (
                    Value::Array(vec![]),
                    Value::call(
                        "isTrueForAny",
                        [Value::Array(vec![]), Value::call("currentArrayElement", [])],
                    ),
                    Some("isTrueForAny"),
                ),
            ] {
                let mut program = Program::new();
                program.rule(Rule::new("nested binding reads", Event::Global).condition(
                    Value::call(
                        outer,
                        [Value::Array(vec![]), Value::call(inner, [source, body])],
                    ),
                ));
                let findings = analyze(&program, &LintConfig::default());
                let nested: Vec<_> = findings
                    .iter()
                    .filter(|finding| finding.code == "nested-binder-lambda")
                    .collect();
                assert_eq!(
                    nested.len(),
                    usize::from(expected.is_some()),
                    "{outer}/{inner}: {nested:?}"
                );
                if let Some(reported_inner) = expected {
                    assert!(
                        nested[0]
                            .message
                            .starts_with(&format!("`{reported_inner}` nested"))
                    );
                }
            }
        }
    }
}
