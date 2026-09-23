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

pub fn compare_workshop_texts(expected: &str, actual: &str) -> WorkshopSemanticComparison {
    let catalog = match workshop_rs::catalog::Catalog::builtin() {
        Ok(c) => c,
        Err(e) => return catalog_failure(e.to_string()),
    };
    let locale = workshop_rs::catalog::Locale::new("en-US");
    let exp = parse_side(expected, &catalog, &locale);
    let act = parse_side(actual, &catalog, &locale);
    let equivalent = matches!((&exp.program, &act.program), (Some(e), Some(a)) if workshop_rs::roundtrip::equivalent(e, a));
    WorkshopSemanticComparison {
        equivalent,
        expected: exp.side,
        actual: act.side,
    }
}

struct ParsedSide {
    side: WorkshopSemanticSide,
    program: Option<workshop_rs::Program>,
}

fn err_side(error: String) -> ParsedSide {
    ParsedSide {
        side: WorkshopSemanticSide {
            parsed: false,
            error: Some(error),
            dump: None,
        },
        program: None,
    }
}

fn parse_side(
    text: &str,
    catalog: &workshop_rs::catalog::Catalog,
    locale: &workshop_rs::catalog::Locale,
) -> ParsedSide {
    let mut program = match workshop_rs::parser::parse_with_context(text, catalog, locale, catalog)
    {
        Ok(p) => p,
        Err(e) => return err_side(e.to_string()),
    };
    if let Err(e) = program.validate() {
        return err_side(e.to_string());
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
            if name != "createHudText"
                || args.len() < 3
                || !matches!(args.get(1), Some(Value::Null))
            {
                return;
            }
            if let Some(Value::Call {
                name: body_name,
                args: body_args,
            }) = args.get_mut(2)
            {
                if body_name == "customString" {
                    for value in body_args {
                        canonicalize_debug_strings(value);
                    }
                }
            }
        }
        _ => {}
    }
}

fn canonicalize_debug_strings(value: &mut workshop_rs::Value) {
    use workshop_rs::Value;
    match value {
        Value::String(val) => *val = "<wright-debug-string>".to_string(),
        Value::Array(values) => {
            for v in values {
                canonicalize_debug_strings(v);
            }
        }
        Value::Vector { x, y, z } => {
            canonicalize_debug_strings(x);
            canonicalize_debug_strings(y);
            canonicalize_debug_strings(z);
        }
        Value::PlayerVariable { player, .. } => canonicalize_debug_strings(player),
        Value::Call { args, .. } => {
            for v in args {
                canonicalize_debug_strings(v);
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
