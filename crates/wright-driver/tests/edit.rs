//! Source edits remain Wright-owned data contracts, while semantic validation
//! is now delegated to provider capabilities instead of a static OPY frontend.

use std::collections::BTreeMap;

use wright_driver::edit::{EditRange, EditTransaction, SourceEdit, validate_transaction};
use wright_driver::{InputSpec, SessionConfig};

fn edit(source: &str, range: EditRange) -> SourceEdit {
    SourceEdit {
        edit_kind: "rename".to_string(),
        source: "program.opy".to_string(),
        source_identity: wright_driver::input_identity(source),
        range,
        new_text: "total".to_string(),
    }
}

#[test]
fn transaction_rejects_invalid_ranges_before_provider_validation() {
    let source = "globalvar score = 0\n";
    let transaction = EditTransaction::new(vec![edit(
        source,
        EditRange {
            start_line: 1,
            start_col: 0,
            end_line: 1,
            end_col: 2,
        },
    )]);
    let transaction = transaction.expect("range validity is checked during application");
    let sources = BTreeMap::from([("program.opy".to_string(), source.to_string())]);
    let error = transaction.apply(&sources).unwrap_err();
    assert_eq!(error.code, "edit-invalid-range");
}

#[test]
fn opy_validation_refuses_without_a_provider_capability() {
    let source = "globalvar score = 0\n";
    let transaction = EditTransaction::new(vec![edit(
        source,
        EditRange {
            start_line: 1,
            start_col: 1,
            end_line: 1,
            end_col: 6,
        },
    )])
    .expect("transaction is structurally valid");
    let sources = BTreeMap::from([("program.opy".to_string(), source.to_string())]);
    let result = validate_transaction(
        &SessionConfig {
            input: InputSpec::Path("program.opy".into()),
            ..SessionConfig::default()
        },
        &sources,
        &transaction,
    );
    assert!(!result.ok);
    assert_eq!(result.diagnostics[0].code, "source-provider-unavailable");
    assert!(result.preview.is_none());
}

#[test]
fn empty_transaction_is_rejected_without_source_access() {
    let error = EditTransaction::new(Vec::new()).unwrap_err();
    assert_eq!(error.code, "edit-empty-transaction");
}
