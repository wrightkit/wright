use std::path::Path;

use wright_driver::WorkshopProvider;
use wright_driver::provider::Status;

#[test]
fn workshop_provider_check_and_parse_failure() {
    let provider = WorkshopProvider::new().expect("provider initializes");
    let error = provider
        .check("not Workshop source", Path::new("broken.txt"))
        .expect_err("malformed Workshop must not disappear");
    assert_eq!(error.code, "workshop.locale");

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = std::fs::read_to_string(
        root.join("compatibility/fixtures/synthetic/basic-rule/workshop.ws"),
    )
    .expect("Workshop fixture");
    let diagnostics = provider
        .check(&source, Path::new("basic-rule.txt"))
        .expect("valid Workshop input reaches semantic inspection");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.status != Some(Status::Supported))
    );
}
