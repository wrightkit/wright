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
