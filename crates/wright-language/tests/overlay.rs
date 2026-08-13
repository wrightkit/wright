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

const SHARED_GOOD: &str = "subroutine showStatus\n\n# showStatus is an unsaved overlay\n\ndef showStatus():\n    print(\"overlay\")\n";
const SHARED_BROKEN: &str = "this is not valid opy\n";
const SHARED_HOT: &str = "rule \"hot\":\n    @Event global\n    while true:\n        wait()\n";

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
fn rename_uses_open_overlay_content_over_filesystem() {
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

    // shared.opy is not on disk; only the open unsaved overlay provides it.
    let result = service.rename(
        &main_uri,
        Position {
            line: 4,
            character: 5,
        },
        "refresh",
    );
    assert!(
        result.ok,
        "overlay rename validates: {:?}",
        result.diagnostics
    );
    let shared_edit = result
        .edits
        .iter()
        .find(|edit| edit.source.ends_with("shared.opy"))
        .expect("shared edit from the overlay");
    assert!(
        shared_edit.new_text.contains("subroutine refresh"),
        "overlay declaration renamed: {}",
        shared_edit.new_text
    );
    assert!(
        shared_edit.new_text.contains("print(\"overlay\")"),
        "overlay body survives the rename: {}",
        shared_edit.new_text
    );
    assert!(
        shared_edit.new_text.contains("def refresh()"),
        "overlay definition renamed: {}",
        shared_edit.new_text
    );
    assert!(
        shared_edit
            .new_text
            .contains("# showStatus is an unsaved overlay"),
        "overlay comment text survives unchanged: {}",
        shared_edit.new_text
    );
    // Only the comment retains the old spelling; every semantic occurrence is
    // renamed.
    assert_eq!(
        shared_edit.new_text.matches("showStatus").count(),
        1,
        "only the comment occurrence of the old spelling remains: {}",
        shared_edit.new_text
    );
    // The overlay is the source of truth: shared.opy does not exist on disk
    // in this fixture, so the renamed overlay text proves filesystem content
    // was never substituted for an open document.
    assert!(
        !overlay_root().join("shared.opy").exists(),
        "the fixture has no filesystem shared.opy"
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
fn overlay_include_semantic_diagnostics_keep_source_identity() {
    let root = overlay_root();
    let mut service = LanguageService::new(root.clone());
    let main_uri = "file:///main.opy".to_string();
    let shared_uri = format!("file://{}", root.join("shared.opy").display());
    service
        .store
        .open(Document::new(main_uri.clone(), main_text(), root.clone()));
    service
        .store
        .open(Document::new(shared_uri.clone(), SHARED_HOT, root.clone()));

    let diagnostics = service.diagnostics(&main_uri);
    let finding = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "min-wait-loop")
        .expect("hot loop in the overlay produces a semantic finding");
    assert!(
        finding.source.ends_with("shared.opy"),
        "semantic finding keeps the overlay source identity: {}",
        finding.source
    );
    assert_eq!(
        finding.range.start.line, 2,
        "range is source-local to shared.opy (the while line): {:?}",
        finding.range
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
