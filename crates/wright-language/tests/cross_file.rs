//! Cross-file navigation tests (#65): definition/references preserve
//! `span.file` provenance and return source-aware locations pointing at the
//! correct included file, not the requesting document.

use std::path::PathBuf;

use wright_language::LanguageService;
use wright_language::document::{Document, Position};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multifile")
}

fn main_document() -> Document {
    let root = fixtures();
    let text = std::fs::read_to_string(root.join("main.opy")).unwrap();
    Document::new("file:///project/main.opy", text, root)
}

fn broken_include_fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/broken-include")
}

#[test]
fn definition_points_to_the_included_file() {
    let document = main_document();
    let mut service = LanguageService::new(fixtures());
    let uri = document.uri.clone();
    service.store.open(document);

    // `showStatus()` call is on line 4 (0-based line 3), col 5.
    let definition = service
        .definition(
            &uri,
            Position {
                line: 4,
                character: 5,
            },
        )
        .expect("definition resolves");

    // The declaration is in shared.opy, not main.opy.
    assert_ne!(
        definition.source, uri,
        "cross-file definition source differs from requester"
    );
    assert!(
        definition.source.ends_with("shared.opy"),
        "definition points at shared.opy: {}",
        definition.source
    );
    // `def showStatus` is on line 3 of shared.opy (0-based line 2).
    assert_eq!(
        definition.range.start.line, 2,
        "declaration line in shared.opy"
    );
}

#[test]
fn references_span_both_files() {
    let document = main_document();
    let mut service = LanguageService::new(fixtures());
    let uri = document.uri.clone();
    service.store.open(document);

    let references = service.references(
        &uri,
        Position {
            line: 4,
            character: 5,
        },
    );
    assert!(
        references.len() >= 2,
        "declaration + call site: {references:?}"
    );
    assert!(
        references
            .iter()
            .any(|location| location.source.ends_with("shared.opy")),
        "the declaration lives in shared.opy: {references:?}"
    );
    assert!(
        references.iter().any(|location| location.source == uri),
        "the call site lives in main.opy: {references:?}"
    );
}

#[test]
fn filesystem_include_semantic_diagnostics_keep_source_identity() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/semantic-include");
    let main = std::fs::read_to_string(root.join("main.opy")).unwrap();
    let uri = format!("file://{}", root.join("main.opy").display());
    let mut service = LanguageService::new(root.clone());
    service
        .store
        .open(Document::new(uri.clone(), main, root.clone()));

    let diagnostics = service.diagnostics(&uri);
    let finding = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "min-wait-loop")
        .expect("hot loop in shared.opy produces a semantic finding");
    assert!(
        finding.source.ends_with("shared.opy"),
        "semantic finding keeps the include source identity: {}",
        finding.source
    );
    assert_eq!(
        finding.range.start.line, 2,
        "range is source-local to shared.opy (the while line): {:?}",
        finding.range
    );
}

#[test]
fn filesystem_include_preprocess_diagnostics_keep_source_identity() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/preprocess-include");
    let main = std::fs::read_to_string(root.join("main.opy")).unwrap();
    let uri = format!("file://{}", root.join("main.opy").display());
    let mut service = LanguageService::new(root.clone());
    service
        .store
        .open(Document::new(uri.clone(), main, root.clone()));

    let diagnostics = service.diagnostics(&uri);
    let error = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "unsupported-directive")
        .expect("unsupported directive in the include produces a structured error");
    assert!(
        error.source.ends_with("broken.opy"),
        "preprocess diagnostic keeps the include source identity: {}",
        error.source
    );
    assert_eq!(
        error.range.start.line, 0,
        "range is source-local to broken.opy (the directive line): {:?}",
        error.range
    );
}

#[test]
fn rename_from_root_updates_declaration_and_references_across_files() {
    let document = main_document();
    let root = fixtures();
    let mut service = LanguageService::new(root.clone());
    let uri = document.uri.clone();
    service.store.open(document);

    // The showStatus() call is on line 5 (0-based line 4), col 5.
    let result = service
        .rename(
            &uri,
            Position {
                line: 4,
                character: 5,
            },
            "refresh",
        )
        .expect("rename resolves at the call site");
    assert!(
        result.ok,
        "cross-file rename validates: {:?}",
        result.diagnostics
    );
    assert_eq!(result.edits.len(), 2, "both sources are covered");
    let main_edit = result
        .edits
        .iter()
        .find(|edit| edit.source.ends_with("main.opy"))
        .expect("root edit");
    let shared_edit = result
        .edits
        .iter()
        .find(|edit| edit.source.ends_with("shared.opy"))
        .expect("include edit");
    assert!(
        main_edit.new_text.contains("refresh()"),
        "call site renamed in the root: {}",
        main_edit.new_text
    );
    assert!(
        !main_edit.new_text.contains("showStatus"),
        "old name gone from the root: {}",
        main_edit.new_text
    );
    assert!(
        shared_edit.new_text.contains("subroutine refresh"),
        "declaration renamed in the include: {}",
        shared_edit.new_text
    );
    assert!(
        shared_edit.new_text.contains("def refresh()"),
        "definition renamed in the include: {}",
        shared_edit.new_text
    );
    assert!(
        !shared_edit.new_text.contains("showStatus"),
        "old name gone from the include: {}",
        shared_edit.new_text
    );

    // Applying all edits yields sources that compile together.
    let main_text = main_edit.new_text.clone();
    let shared_text = shared_edit.new_text.clone();
    let overlay = [("shared.opy".to_string(), shared_text.clone())]
        .into_iter()
        .collect();
    let compiled = wright_opy::compile_with_overlay(&main_text, &uri, &root, &overlay);
    assert!(
        compiled.is_ok(),
        "edited project compiles: {:?}",
        compiled.err()
    );
}

#[test]
fn rename_from_include_declaration_updates_the_root() {
    let root = fixtures();
    let mut service = LanguageService::new(root.clone());
    let main_uri = format!("file://{}", root.join("main.opy").display());
    let shared_uri = format!("file://{}", root.join("shared.opy").display());
    service.store.open(Document::new(
        main_uri.clone(),
        std::fs::read_to_string(root.join("main.opy")).unwrap(),
        root.clone(),
    ));
    service.store.open(Document::new(
        shared_uri.clone(),
        std::fs::read_to_string(root.join("shared.opy")).unwrap(),
        root.clone(),
    ));

    // `subroutine showStatus` on line 1 of shared.opy; the name starts at
    // character 11.
    let result = service
        .rename(
            &shared_uri,
            Position {
                line: 0,
                character: 11,
            },
            "refresh",
        )
        .expect("rename resolves at the declaration site");
    assert!(
        result.ok,
        "rename from the declaration validates: {:?}",
        result.diagnostics
    );
    let main_edit = result
        .edits
        .iter()
        .find(|edit| edit.source.ends_with("main.opy"))
        .expect("root edit");
    assert!(
        main_edit.new_text.contains("refresh()"),
        "root call site renamed: {}",
        main_edit.new_text
    );
    let shared_edit = result
        .edits
        .iter()
        .find(|edit| edit.source.ends_with("shared.opy"))
        .expect("include edit");
    assert!(
        shared_edit.new_text.contains("subroutine refresh"),
        "include declaration renamed: {}",
        shared_edit.new_text
    );
}

#[test]
fn filesystem_include_diagnostics_keep_source_identity() {
    let root = broken_include_fixtures();
    let main = std::fs::read_to_string(root.join("main.opy")).unwrap();
    let uri = format!("file://{}", root.join("main.opy").display());
    let mut service = LanguageService::new(root.clone());
    service
        .store
        .open(Document::new(uri.clone(), main, root.clone()));

    let diagnostics = service.diagnostics(&uri);
    let error = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.severity == "error")
        .expect("broken filesystem include produces an error diagnostic");
    assert!(
        error.source.ends_with("bad.opy"),
        "diagnostic source is the filesystem include, not the requesting document: {}",
        error.source
    );
    assert_eq!(
        error.range.start.line, 0,
        "range is source-local to bad.opy, not the requesting document line: {:?}",
        error.range
    );
}
