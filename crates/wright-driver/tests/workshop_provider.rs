use std::path::Path;

use wright_driver::WorkshopProvider;
use wright_driver::provider::{LanguageProvider, Status};

#[test]
fn provider_checks_a_real_workshop_fixture_without_swallowing_failure() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source =
        std::fs::read_to_string(root.join("tests/fixtures/workshop/synthetic/basic-rule.ws"))
            .expect("Workshop fixture");
    let provider = WorkshopProvider::new().expect("provider initializes");
    let diagnostics = provider
        .check(&source, Path::new("basic-rule.txt"))
        .expect("valid Workshop input reaches semantic inspection");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.status != Status::Supported)
    );
}

/// The native provider check composes the owner's catalog-aware canonical
/// validation with residual inspection: a catalog signature violation
/// surfaces as an error-severity diagnostic carrying the owner span, and
/// catalog-defaulted optional arguments stay accepted.
#[test]
fn provider_check_reports_canonical_rejection_as_error_diagnostic() {
    let provider = WorkshopProvider::new().expect("provider initializes");
    let source = "rule (\"r\") { event { Ongoing - Global; } actions { Wait(); } }";
    let diagnostics = provider
        .check(source, Path::new("arity.txt"))
        .expect("a canonical rejection is a diagnostic, not a provider failure");
    let rejection = diagnostics
        .iter()
        .find(|d| d.code == "workshop.catalog-validation")
        .unwrap_or_else(|| panic!("the owner rejection is reported: {diagnostics:?}"));
    assert_eq!(rejection.severity, wright_driver::Severity::Error);
    assert_eq!(rejection.status, Status::Unsupported);
    let wait_col = u32::try_from(source.find("Wait").unwrap() + 1).unwrap();
    assert_eq!(
        (
            rejection.span.start_line,
            rejection.span.start_col,
            rejection.span.end_col
        ),
        (1, wait_col, wait_col + 4),
        "the span covers the violating call name"
    );

    let accepted = provider
        .check(
            "rule (\"r\") { event { Ongoing - Global; } actions { Wait(1); } }",
            Path::new("defaulted.txt"),
        )
        .expect("optional defaults stay accepted");
    assert!(
        accepted
            .iter()
            .all(|d| d.code != "workshop.catalog-validation"),
        "no canonical rejection for defaulted arguments: {accepted:?}"
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
