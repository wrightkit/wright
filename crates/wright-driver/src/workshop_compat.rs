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
    let catalog = match workshop_rs_gate::catalog::Catalog::builtin() {
        Ok(catalog) => catalog,
        Err(error) => return catalog_failure(error.to_string()),
    };
    let locale = workshop_rs_gate::catalog::Locale::new("en-US");
    let mut expected = parse_side(expected, &catalog, &locale);
    let mut actual = parse_side(actual, &catalog, &locale);
    let equivalent = match (expected.program.as_mut(), actual.program.as_mut()) {
        (Some(expected), Some(actual)) => workshop_rs_gate::roundtrip::equivalent(expected, actual),
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
    program: Option<workshop_rs_gate::wir::Program>,
}

fn parse_side(
    text: &str,
    catalog: &workshop_rs_gate::catalog::Catalog,
    locale: &workshop_rs_gate::catalog::Locale,
) -> ParsedSide {
    let mut program =
        match workshop_rs_gate::parser::parse_with_context(text, catalog, locale, catalog) {
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

fn canonicalize_debug_hud(program: &mut workshop_rs_gate::wir::Program) {
    for index in 0..program.actions.len() {
        let action_id = workshop_rs_gate::ids::Id::from_index(index);
        let Some(workshop_rs_gate::wir::Action::Call { name, args, .. }) =
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
            .is_some_and(|node| matches!(node.value, workshop_rs_gate::wir::Value::Null));
        if !is_debug_label {
            continue;
        }
        let Some(workshop_rs_gate::wir::ValueNode {
            value: workshop_rs_gate::wir::Value::Call { name, args },
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
            if let Some(workshop_rs_gate::wir::ValueNode {
                value: workshop_rs_gate::wir::Value::String(value),
                ..
            }) = program.values.get_mut(string_id)
            {
                *value = "<wright-debug-string>".to_string();
            }
        }
    }
}

fn collect_string_ids(
    program: &workshop_rs_gate::wir::Program,
    id: workshop_rs_gate::wir::ValueId,
    visited: &mut [bool],
    string_ids: &mut Vec<workshop_rs_gate::wir::ValueId>,
) {
    if id.index() >= visited.len() || visited[id.index()] {
        return;
    }
    visited[id.index()] = true;
    let Some(node) = program.values.get(id) else {
        return;
    };
    match &node.value {
        workshop_rs_gate::wir::Value::String(_) => string_ids.push(id),
        workshop_rs_gate::wir::Value::Array(values)
        | workshop_rs_gate::wir::Value::Call { args: values, .. } => {
            for value in values {
                collect_string_ids(program, *value, visited, string_ids);
            }
        }
        workshop_rs_gate::wir::Value::Vector { x, y, z } => {
            collect_string_ids(program, *x, visited, string_ids);
            collect_string_ids(program, *y, visited, string_ids);
            collect_string_ids(program, *z, visited, string_ids);
        }
        workshop_rs_gate::wir::Value::PlayerVariable { player, .. } => {
            collect_string_ids(program, *player, visited, string_ids);
        }
        workshop_rs_gate::wir::Value::Number { .. }
        | workshop_rs_gate::wir::Value::LocalizedString(_)
        | workshop_rs_gate::wir::Value::Bool(_)
        | workshop_rs_gate::wir::Value::Null
        | workshop_rs_gate::wir::Value::Enum { .. }
        | workshop_rs_gate::wir::Value::GlobalVariable(_)
        | workshop_rs_gate::wir::Value::Subroutine(_)
        | workshop_rs_gate::wir::Value::EventPlayer => {}
    }
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
