//! Source edits remain Wright-owned data contracts. Raw Workshop
//! transactions validate through `workshop-rs` reparse (#434); source
//! languages route to the provider operations instead of a static frontend.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use wright_driver::edit::{EditRange, EditTransaction, SourceEdit, validate_transaction};
use wright_driver::{InputSpec, SessionConfig, SourceKind};

fn edit(source: &str, range: EditRange) -> SourceEdit {
    SourceEdit {
        edit_kind: "rename".to_string(),
        source: "program.opy".to_string(),
        source_identity: wright_driver::input_identity(source),
        range,
        new_text: "total".to_string(),
    }
}

fn temp_source(name: &str, text: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-edit-it-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    (dir, path)
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
fn opy_validation_routes_to_the_provider_operation() {
    let source = "globalvar score = 0\n";
    let (dir, path) = temp_source("program.opy", source);
    let transaction = EditTransaction::new(vec![SourceEdit {
        source: path.to_string_lossy().into_owned(),
        ..edit(
            source,
            EditRange {
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 6,
            },
        )
    }])
    .expect("transaction is structurally valid");
    let sources = BTreeMap::from([(path.to_string_lossy().into_owned(), source.to_string())]);
    let result = validate_transaction(
        &SessionConfig {
            input: InputSpec::Path(path),
            kind: SourceKind::Opy,
            ..SessionConfig::default()
        },
        &workshop_rs::catalog::Catalog::builtin().expect("catalog"),
        Some(&sources),
        &transaction,
    );
    assert!(!result.ok);
    assert_eq!(result.diagnostics[0].code, "edit-requires-provider");
    assert!(
        result.diagnostics[0]
            .message
            .contains("providerValidateEdit"),
        "the refusal names the provider operation: {}",
        result.diagnostics[0].message
    );
    assert!(result.preview.is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn workshop_validation_applies_and_reparses() {
    let source = "variables {\n    global:\n        0: score\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(score, 5);\n    }\n}\n";
    let (dir, path) = temp_source("program.ws", source);
    let transaction = EditTransaction::new(vec![SourceEdit {
        edit_kind: "edit".to_string(),
        source: path.to_string_lossy().into_owned(),
        source_identity: wright_driver::input_identity(source),
        range: EditRange {
            start_line: 3,
            start_col: 12,
            end_line: 3,
            end_col: 17,
        },
        new_text: "total".to_string(),
    }])
    .expect("transaction is structurally valid");
    let sources = BTreeMap::from([(path.to_string_lossy().into_owned(), source.to_string())]);
    let result = validate_transaction(
        &SessionConfig {
            input: InputSpec::Path(path),
            kind: SourceKind::Workshop,
            ..SessionConfig::default()
        },
        &workshop_rs::catalog::Catalog::builtin().expect("catalog"),
        Some(&sources),
        &transaction,
    );
    assert!(result.ok, "{:?}", result.diagnostics);
    let preview = result.preview.expect("a valid transaction previews");
    assert!(preview[0].new_text.contains("0: total"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn empty_transaction_is_rejected_without_source_access() {
    let error = EditTransaction::new(Vec::new()).unwrap_err();
    assert_eq!(error.code, "edit-empty-transaction");
}
