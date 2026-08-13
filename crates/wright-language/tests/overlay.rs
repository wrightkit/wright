//! Open-document overlay tests (#63/#64): unsaved editor buffers participate
//! in include resolution (never silently ignored) and include changes
//! invalidate dependent results.

use std::path::PathBuf;

use wright_language::LanguageService;
use wright_language::document::{Document, Position};

fn overlay_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/overlay")
}

fn main_text() -> String {
    std::fs::read_to_string(overlay_root().join("main.opy")).unwrap()
}

const SHARED_GOOD: &str = "subroutine showStatus\n\ndef showStatus():\n    print(\"overlay\")\n";
const SHARED_BROKEN: &str = "this is not valid opy\n";

#[test]
fn open_unsaved_include_participates_in_resolution() {
    // shared.opy is not on disk; only the open overlay provides it.
    let root = overlay_root();
    let mut service = LanguageService::new(root.clone());
    let main_uri = "file:///main.opy".to_string();
    let shared_uri = format!("file://{}", root.join("shared.opy").display());
    service
        .store
        .open(Document::new(main_uri.clone(), main_text(), root.clone()));
    service
        .store
        .open(Document::new(shared_uri.clone(), SHARED_GOOD, root.clone()));

    // showStatus() is on line 5 (0-based 4) of main.opy.
    let definition = service
        .definition(
            &main_uri,
            Position {
                line: 4,
                character: 5,
            },
        )
        .expect("overlay include resolves the symbol");
    assert!(
        definition.source.ends_with("shared.opy"),
        "definition resolves into the overlay, not a missing filesystem file: {}",
        definition.source
    );
}

#[test]
fn include_change_invalidates_and_restores_dependent_results() {
    let root = overlay_root();
    let mut service = LanguageService::new(root.clone());
    let main_uri = "file:///main.opy".to_string();
    let shared_uri = format!("file://{}", root.join("shared.opy").display());
    service
        .store
        .open(Document::new(main_uri.clone(), main_text(), root.clone()));
    service
        .store
        .open(Document::new(shared_uri.clone(), SHARED_GOOD, root.clone()));

    let before = service.diagnostics(&main_uri);
    assert!(
        !before.iter().any(|d| d.severity == "error"),
        "good overlay compiles: {before:?}"
    );

    // Valid -> invalid without reopening the root document.
    assert!(service.store.change(&shared_uri, SHARED_BROKEN, 1));
    let invalid = service.diagnostics(&main_uri);
    assert!(
        invalid.iter().any(|d| d.severity == "error"),
        "include change invalidates dependent diagnostics: {invalid:?}"
    );

    // Invalid -> valid without reopening the root document.
    assert!(service.store.change(&shared_uri, SHARED_GOOD, 2));
    let restored = service.diagnostics(&main_uri);
    assert!(
        !restored.iter().any(|d| d.severity == "error"),
        "restored include clears dependent diagnostics: {restored:?}"
    );
}

#[test]
fn include_diagnostics_keep_source_identity_and_local_range() {
    let root = overlay_root();
    let mut service = LanguageService::new(root.clone());
    let main_uri = "file:///main.opy".to_string();
    let shared_uri = format!("file://{}", root.join("shared.opy").display());
    service
        .store
        .open(Document::new(main_uri.clone(), main_text(), root.clone()));
    service.store.open(Document::new(
        shared_uri.clone(),
        SHARED_BROKEN,
        root.clone(),
    ));

    let diagnostics = service.diagnostics(&main_uri);
    let error = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.severity == "error")
        .expect("broken overlay produces an error diagnostic");
    assert!(
        error.source.ends_with("shared.opy"),
        "diagnostic source is the include, not the requesting document: {}",
        error.source
    );
    assert_eq!(
        error.range.start.line, 0,
        "range is source-local to shared.opy, not the requesting document line: {:?}",
        error.range
    );
    assert_eq!(
        error.document_version, 0,
        "requesting document version preserved"
    );
}

#[test]
fn dependent_documents_include_roots_that_include_the_changed_overlay() {
    let root = overlay_root();
    let mut service = LanguageService::new(root.clone());
    let main_uri = "file:///main.opy".to_string();
    let shared_uri = format!("file://{}", root.join("shared.opy").display());
    service
        .store
        .open(Document::new(main_uri.clone(), main_text(), root.clone()));
    service
        .store
        .open(Document::new(shared_uri.clone(), SHARED_GOOD, root.clone()));

    let affected = service.dependent_documents(&shared_uri);
    assert!(
        affected.contains(&shared_uri),
        "changed document is affected"
    );
    assert!(
        affected.contains(&main_uri),
        "root including the overlay is affected: {affected:?}"
    );
}
