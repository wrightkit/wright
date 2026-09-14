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
    let program = match workshop_rs::parser::parse_with_context(text, catalog, locale, catalog) {
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
    fn rejects_a_changed_workshop_value() {
        let expected = cake_workshop();
        let actual = expected.replacen("0.75", "0.5", 1);
        let comparison = compare_workshop_texts(&expected, &actual);
        assert!(!comparison.equivalent);
        assert!(comparison.expected.dump.is_some());
        assert!(comparison.actual.dump.is_some());
    }
}
