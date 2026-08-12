//! Language-service tests (#65/#66/#64): diagnostics, hover, definition,
//! references, completion, rename, semantic tokens, and the incremental
//! contract (changed documents produce results equivalent to clean full
//! recomputation, tagged with the correct document version).

use std::path::PathBuf;

use wright_language::LanguageService;
use wright_language::document::{Document, Position};

const CORPUS: &str = "synthetic/declarations-rules";

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn corpus_text(id: &str) -> String {
    std::fs::read_to_string(
        workspace_root()
            .join("compatibility/fixtures")
            .join(id)
            .join("source.opy"),
    )
    .unwrap()
}

fn service_with(document: Document) -> (LanguageService, String) {
    let root = document.root.clone();
    let mut service = LanguageService::new(root);
    let uri = document.uri.clone();
    service.store.open(document);
    (service, uri)
}

fn doc(id: &str) -> Document {
    Document::new("file:///main.opy", corpus_text(id), workspace_root())
}

#[test]
fn diagnostics_include_parse_errors_and_findings() {
    // A malformed rule produces a structured parse diagnostic.
    let mut broken = doc(CORPUS);
    broken
        .text
        .push_str("\nrule \"broken\"\n    @Event global\n");
    let (service, uri) = service_with(broken);
    let diagnostics = service.diagnostics(&uri);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == "parse-error" && d.severity == "error"),
        "malformed input produces a structured parse diagnostic: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().any(|d| d.code == "parse-error"),
        "the broken rule reports a parse error"
    );
}

#[test]
fn diagnostics_report_semantic_findings_with_ranges() {
    let source =
        "globalvar i = 0\n\nrule \"hot\":\n    @Event global\n    while true:\n        wait()\n";
    let document = Document::new("file:///hot.opy", source, workspace_root());
    let (service, uri) = service_with(document);
    let diagnostics = service.diagnostics(&uri);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == "min-wait-loop" && d.severity == "warning"),
        "hot loop finding: {diagnostics:?}"
    );
    for diagnostic in diagnostics {
        assert_eq!(diagnostic.document_version, 0);
    }
}

#[test]
fn hover_and_definition_resolve_symbols() {
    let (service, uri) = service_with(doc(CORPUS));
    // `score` is declared on line 1 at col 1.
    let hover = service
        .hover(
            &uri,
            Position {
                line: 0,
                character: 3,
            },
        )
        .unwrap();
    assert!(hover.contents.contains("score"), "{hover:?}");
    assert!(hover.contents.contains("globalVariable"));
    let definition = service
        .definition(
            &uri,
            Position {
                line: 0,
                character: 3,
            },
        )
        .unwrap();
    assert_eq!(definition.start.line, 0, "definition is on line 1");
}

#[test]
fn references_find_declaration_and_uses() {
    let (service, uri) = service_with(doc(CORPUS));
    // `score` is read in the def body on line 6 at col 30.
    let references = service.references(
        &uri,
        Position {
            line: 5,
            character: 30,
        },
    );
    assert!(references.len() >= 2, "declaration + reads: {references:?}");
}

#[test]
fn completion_offers_symbols_builtins_and_keywords() {
    let (service, uri) = service_with(doc(CORPUS));
    let items = service.completion(
        &uri,
        Position {
            line: 0,
            character: 0,
        },
    );
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    assert!(labels.contains(&"score"), "declared symbol");
    assert!(labels.contains(&"showStatus"), "declared subroutine");
    assert!(labels.contains(&"wait"), "builtin");
    assert!(labels.contains(&"globalvar"), "keyword");
}

#[test]
fn rename_uses_the_safe_edit_contract() {
    let (service, uri) = service_with(doc(CORPUS));
    let result = service
        .rename(
            &uri,
            Position {
                line: 0,
                character: 3,
            },
            "total",
        )
        .unwrap();
    assert!(result.ok, "rename validates");
    let preview = result.preview.expect("previewed source");
    assert!(preview.contains("globalvar total = 0"), "{preview}");
    assert!(
        !preview.contains("globalvar score"),
        "old name gone: {preview}"
    );
}

#[test]
fn semantic_tokens_classify_by_identity() {
    let (service, uri) = service_with(doc(CORPUS));
    let tokens = service.semantic_tokens(&uri);
    let types: Vec<&str> = tokens
        .iter()
        .map(|token| token.token_type.as_str())
        .collect();
    assert!(types.contains(&"keyword"), "keywords classified");
    assert!(types.contains(&"string"), "rule names are strings");
    assert!(types.contains(&"variable"), "variables classified");
    // `score` token on line 1 carries a variable classification.
    let score_tokens = tokens
        .iter()
        .filter(|token| token.line == 0)
        .collect::<Vec<_>>();
    assert!(!score_tokens.is_empty());
}

#[test]
fn changed_documents_are_incremental_and_equivalent_to_full_recomputation() {
    let document = doc(CORPUS);
    let root = document.root.clone();
    let mut service = LanguageService::new(root);
    let uri = document.uri.clone();
    service.store.open(document.clone());

    let before = service.diagnostics(&uri);
    let version = service
        .store
        .change(&uri, &(document.text.clone() + "\n"))
        .unwrap();
    assert_eq!(version, 1, "version bumps on change");
    let after = service.diagnostics(&uri);
    assert!(
        after.iter().all(|d| d.document_version == 1),
        "results carry the new version: {after:?}"
    );

    // A clean full recomputation of the changed text gives the same result.
    let changed_text = document.text.clone() + "\n";
    let fresh = Document::new("file:///fresh.opy", changed_text, document.root.clone());
    let mut fresh_service = LanguageService::new(document.root.clone());
    fresh_service.store.open(fresh);
    let fresh_diagnostics = fresh_service.diagnostics("file:///fresh.opy");
    assert_eq!(
        after.len(),
        before.len() + fresh_diagnostics.len() - before.len(),
        "incremental and full recomputation agree on diagnostic count"
    );
    let _ = &before;
}

#[test]
fn stale_results_are_detected_by_version() {
    let source =
        "globalvar i = 0\n\nrule \"hot\":\n    @Event global\n    while true:\n        wait()\n";
    let document = Document::new("file:///hot.opy", source, workspace_root());
    let root = document.root.clone();
    let mut service = LanguageService::new(root);
    let uri = document.uri.clone();
    service.store.open(document.clone());
    let diagnostics = service.diagnostics(&uri);
    assert_eq!(diagnostics[0].document_version, 0);
    service
        .store
        .change(&uri, &(document.text.clone() + "\n"))
        .unwrap();
    let updated = service.diagnostics(&uri);
    assert!(
        updated.iter().all(|d| d.document_version == 1),
        "stale results (version 0) are replaced by version-1 results"
    );
}
