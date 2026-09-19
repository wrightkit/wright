//! Workshop meaning remains owned by `workshop-rs`; this module only adapts
//! its canonical parser and equivalence contract to the gate's two text
//! inputs and preserves comparison evidence for CI reports.

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

/// Compare two emitted Workshop texts using canonical `Program` equivalence.
pub fn compare_workshop_texts(expected: &str, actual: &str) -> WorkshopSemanticComparison {
    let catalog = match workshop_rs::catalog::Catalog::builtin() {
        Ok(catalog) => catalog,
        Err(error) => return catalog_failure(error.to_string()),
    };
    let locale = workshop_rs::catalog::Locale::new("en-US");
    let expected = parse_side(expected, &catalog, &locale);
    let actual = parse_side(actual, &catalog, &locale);
    let equivalent = match (&expected.program, &actual.program) {
        (Some(expected), Some(actual)) => workshop_rs::roundtrip::equivalent(expected, actual),
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
    program: Option<workshop_rs::Program>,
}

fn parse_side(
    text: &str,
    catalog: &workshop_rs::catalog::Catalog,
    locale: &workshop_rs::catalog::Locale,
) -> ParsedSide {
    let mut program = match workshop_rs::parser::parse_with_context(text, catalog, locale, catalog)
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

/// Debug HUD text is Wright presentation, not Workshop semantic identity.
/// Normalize only the established debug marker before asking the owner
/// equivalence contract to compare the canonical programs.
fn canonicalize_debug_hud(program: &mut workshop_rs::Program) {
    for rule in &mut program.rules {
        for action in &mut rule.actions {
            canonicalize_debug_action(action);
        }
    }
}

fn canonicalize_debug_action(action: &mut workshop_rs::Action) {
    use workshop_rs::{Action, Value};
    match action {
        Action::Disabled { action } => canonicalize_debug_action(action),
        Action::Call { name, args } => {
            if name != "createHudText" || args.len() < 3 {
                return;
            }
            if !matches!(args.get(1), Some(Value::Null)) {
                return;
            }
            let Some(Value::Call {
                name: body_name,
                args: body_args,
            }) = args.get_mut(2)
            else {
                return;
            };
            if body_name == "customString" {
                for value in body_args {
                    canonicalize_debug_strings(value);
                }
            }
        }
        _ => {}
    }
}

fn canonicalize_debug_strings(value: &mut workshop_rs::Value) {
    use workshop_rs::Value;
    match value {
        Value::String(value) => *value = "<wright-debug-string>".to_string(),
        Value::Array(values) => {
            for value in values {
                canonicalize_debug_strings(value);
            }
        }
        Value::Vector { x, y, z } => {
            canonicalize_debug_strings(x);
            canonicalize_debug_strings(y);
            canonicalize_debug_strings(z);
        }
        Value::PlayerVariable { player, .. } => canonicalize_debug_strings(player),
        Value::Call { args, .. } => {
            for value in args {
                canonicalize_debug_strings(value);
            }
        }
        _ => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn cake_workshop() -> String {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        std::fs::read_to_string(root.join("tests/fixtures/workshop/real-world/overpy-cake.ws"))
            .unwrap()
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
}
