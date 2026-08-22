use std::path::Path;

use wright_core::provider::LanguageProvider;
use wright_driver::WorkshopProvider;

#[test]
fn provider_checks_a_real_workshop_fixture_without_swallowing_failure() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let oracle = std::fs::read_to_string(
        root.join("compatibility/fixtures/synthetic/basic-rule/oracle.json"),
    )
    .expect("fixture oracle");
    let value: serde_json::Value = serde_json::from_str(&oracle).expect("fixture JSON");
    let source = value["compile"]["workshop"]
        .as_str()
        .expect("Workshop fixture");
    let provider = WorkshopProvider::new().expect("provider initializes");
    let diagnostics = provider
        .check(source, Path::new("basic-rule.txt"))
        .expect("valid Workshop input reaches semantic inspection");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.status != wright_core::provider::Status::Supported)
    );
}

#[test]
fn provider_parse_failure_is_an_explicit_result_error() {
    let provider = WorkshopProvider::new().expect("provider initializes");
    let error = provider
        .check("not Workshop source", Path::new("broken.txt"))
        .expect_err("malformed Workshop must not disappear");
    assert_eq!(error.code, "workshop.locale");
}
