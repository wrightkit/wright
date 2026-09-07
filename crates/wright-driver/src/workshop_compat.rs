//! Semantic comparison used by the OPY integration gate.
//!
//! Workshop meaning remains owned by `workshop-rs`; this module only adapts
//! its parser and equivalence contract to the gate's two text inputs and
//! preserves the comparison evidence for CI reports.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct WorkshopSemanticComparison {
    pub equivalent: bool,
    pub expected: WorkshopSemanticSide,
    pub actual: WorkshopSemanticSide,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkshopSemanticSide {
    pub parsed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dump: Option<String>,
}

/// Compare two emitted Workshop texts using canonical WIR equivalence.
pub fn compare_workshop_texts(expected: &str, actual: &str) -> WorkshopSemanticComparison {
    let catalog = match workshop_rs::catalog::Catalog::builtin() {
        Ok(catalog) => catalog,
        Err(error) => return catalog_failure(error.to_string()),
    };
    let locale = workshop_rs::catalog::Locale::new("en-US");
    let mut expected = parse_side(expected, &catalog, &locale);
    let mut actual = parse_side(actual, &catalog, &locale);
    let equivalent = match (expected.program.as_mut(), actual.program.as_mut()) {
        (Some(expected), Some(actual)) => {
            canonicalize_numeric_representations(expected, actual);
            workshop_rs::roundtrip::equivalent(expected, actual)
        }
        _ => false,
    };
    WorkshopSemanticComparison {
        equivalent,
        expected: expected.side,
        actual: actual.side,
    }
}

struct ParsedSide {
    side: WorkshopSemanticSide,
    program: Option<workshop_rs::wir::Program>,
}

fn parse_side(
    text: &str,
    catalog: &workshop_rs::catalog::Catalog,
    locale: &workshop_rs::catalog::Locale,
) -> ParsedSide {
    let text = canonicalize_presentation_aliases(text);
    let mut program = match workshop_rs::parser::parse_with_context(&text, catalog, locale, catalog)
    {
        Ok(program) => program,
        Err(error) => {
            return ParsedSide {
                side: WorkshopSemanticSide {
                    parsed: false,
                    error: Some(error.to_string()),
                    dump: None,
                },
                program: None,
            };
        }
    };
    if let Err(error) = program.validate() {
        return ParsedSide {
            side: WorkshopSemanticSide {
                parsed: false,
                error: Some(error.to_string()),
                dump: None,
            },
            program: None,
        };
    }
    canonicalize_debug_hud(&mut program);
    let dump = program.dump();
    ParsedSide {
        side: WorkshopSemanticSide {
            parsed: true,
            error: None,
            dump: Some(dump),
        },
        program: Some(program),
    }
}

fn catalog_failure(error: String) -> WorkshopSemanticComparison {
    let side = WorkshopSemanticSide {
        parsed: false,
        error: Some(format!("catalog initialization failed: {error}")),
        dump: None,
    };
    WorkshopSemanticComparison {
        equivalent: false,
        expected: side.clone(),
        actual: side,
    }
}

fn canonicalize_presentation_aliases(text: &str) -> String {
    let text = replace_outside_strings(text, "All Players(Team(All Teams))", "All Players");
    let text = replace_outside_strings(&text, "All Players(Team.ALL)", "All Players");
    let text = replace_outside_strings(&text, "All Players(All Teams)", "All Players");
    replace_word_outside_strings(&text, "Up", "Vector(0, 1, 0)")
}

fn replace_outside_strings(text: &str, needle: &str, replacement: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0;
    while index < text.len() {
        let rest = &text[index..];
        if !in_string && rest.starts_with(needle) {
            output.push_str(replacement);
            index += needle.len();
            continue;
        }
        let character = rest
            .chars()
            .next()
            .expect("index stays on a character boundary");
        output.push(character);
        index += character.len_utf8();
        if in_string && escaped {
            escaped = false;
        } else if in_string && character == '\\' {
            escaped = true;
        } else if character == '"' {
            in_string = !in_string;
        }
    }
    output
}

fn replace_word_outside_strings(text: &str, word: &str, replacement: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0;
    while index < text.len() {
        let rest = &text[index..];
        if !in_string && rest.starts_with(word) {
            let before = text[..index].chars().next_back();
            let after = rest[word.len()..].chars().next();
            let boundary = |character: Option<char>| {
                character.is_none_or(|character| {
                    !(character.is_ascii_alphanumeric() || character == '_')
                })
            };
            if boundary(before) && boundary(after) {
                output.push_str(replacement);
                index += word.len();
                continue;
            }
        }
        let character = rest
            .chars()
            .next()
            .expect("index stays on a character boundary");
        output.push(character);
        index += character.len_utf8();
        if in_string && escaped {
            escaped = false;
        } else if in_string && character == '\\' {
            escaped = true;
        } else if character == '"' {
            in_string = !in_string;
        }
    }
    output
}

fn canonicalize_debug_hud(program: &mut workshop_rs::wir::Program) {
    for index in 0..program.actions.len() {
        let action_id = workshop_rs::ids::Id::from_index(index);
        let Some(workshop_rs::wir::Action::Call { name, args, .. }) =
            program.actions.get(action_id)
        else {
            continue;
        };
        if name != "createHudText" || args.len() < 3 {
            continue;
        }
        let [_, label, text, ..] = args.as_slice() else {
            continue;
        };
        let is_debug_label = program
            .values
            .get(*label)
            .is_some_and(|node| matches!(node.value, workshop_rs::wir::Value::Null));
        if !is_debug_label {
            continue;
        }
        let Some(workshop_rs::wir::ValueNode {
            value: workshop_rs::wir::Value::Call { name, args },
            ..
        }) = program.values.get(*text)
        else {
            continue;
        };
        if name != "customString" || args.is_empty() {
            continue;
        }
        let mut string_ids = Vec::new();
        let mut visited = vec![false; program.values.len()];
        collect_string_ids(program, *text, &mut visited, &mut string_ids);
        for string_id in string_ids {
            if let Some(workshop_rs::wir::ValueNode {
                value: workshop_rs::wir::Value::String(value),
                ..
            }) = program.values.get_mut(string_id)
            {
                *value = "<wright-debug-string>".to_string();
            }
        }
    }
}

fn collect_string_ids(
    program: &workshop_rs::wir::Program,
    id: workshop_rs::wir::ValueId,
    visited: &mut [bool],
    string_ids: &mut Vec<workshop_rs::wir::ValueId>,
) {
    if id.index() >= visited.len() || visited[id.index()] {
        return;
    }
    visited[id.index()] = true;
    let Some(node) = program.values.get(id) else {
        return;
    };
    match &node.value {
        workshop_rs::wir::Value::String(_) => string_ids.push(id),
        workshop_rs::wir::Value::Array(values)
        | workshop_rs::wir::Value::Call { args: values, .. } => {
            for value in values {
                collect_string_ids(program, *value, visited, string_ids);
            }
        }
        workshop_rs::wir::Value::Vector { x, y, z } => {
            collect_string_ids(program, *x, visited, string_ids);
            collect_string_ids(program, *y, visited, string_ids);
            collect_string_ids(program, *z, visited, string_ids);
        }
        workshop_rs::wir::Value::PlayerVariable { player, .. } => {
            collect_string_ids(program, *player, visited, string_ids);
        }
        workshop_rs::wir::Value::Number { .. }
        | workshop_rs::wir::Value::LocalizedString(_)
        | workshop_rs::wir::Value::Bool(_)
        | workshop_rs::wir::Value::Null
        | workshop_rs::wir::Value::Enum { .. }
        | workshop_rs::wir::Value::GlobalVariable(_)
        | workshop_rs::wir::Value::Subroutine(_)
        | workshop_rs::wir::Value::EventPlayer => {}
    }
}

fn canonicalize_numeric_representations(
    expected: &mut workshop_rs::wir::Program,
    actual: &mut workshop_rs::wir::Program,
) {
    for index in 0..expected.values.len().min(actual.values.len()) {
        let id = workshop_rs::ids::Id::from_index(index);
        let Some((expected_value, actual_value)) = expected
            .values
            .get(id)
            .and_then(|node| match &node.value {
                workshop_rs::wir::Value::Number { value, .. } => Some(*value),
                _ => None,
            })
            .zip(actual.values.get(id).and_then(|node| match &node.value {
                workshop_rs::wir::Value::Number { value, .. } => Some(*value),
                _ => None,
            }))
        else {
            continue;
        };
        if float_equivalent(expected_value, actual_value) {
            if let Some(workshop_rs::wir::ValueNode {
                value: workshop_rs::wir::Value::Number { value, .. },
                ..
            }) = actual.values.get_mut(id)
            {
                *value = expected_value;
            }
        }
    }
}

fn float_equivalent(left: f64, right: f64) -> bool {
    if left == right {
        return true;
    }
    let scale = left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= f64::EPSILON * scale * 4.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn cake_workshop() -> String {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let value: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(
                root.join("compatibility/fixtures/real-world/overpy-cake/oracle.json"),
            )
            .unwrap(),
        )
        .unwrap();
        value["compile"]["workshop"].as_str().unwrap().to_string()
    }

    #[test]
    fn accepts_declared_workshop_representation_aliases() {
        let expected = cake_workshop();
        let actual = expected
            .replace("All Players(All Teams)", "All Players")
            .replace("Up", "Vector(0, 1, 0)");
        assert!(compare_workshop_texts(&expected, &actual).equivalent);
    }

    #[test]
    fn rejects_a_changed_workshop_value() {
        let expected = cake_workshop();
        let actual = expected.replacen("0.75", "0.5", 1);
        let comparison = compare_workshop_texts(&expected, &actual);
        assert!(!comparison.equivalent);
        assert!(comparison.expected.dump.is_some());
        assert!(comparison.actual.dump.is_some());
    }

    #[test]
    fn accepts_owner_defined_float_representation_precision() {
        let expected = cake_workshop();
        let actual = expected.replace("1.810660171779821", "1.8106601717798212");
        assert!(compare_workshop_texts(&expected, &actual).equivalent);
    }
}
