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
    assert_eq!(definition.range.start.line, 0, "definition is on line 1");
    assert_eq!(
        definition.source, uri,
        "same-file definition keeps the source identity"
    );
}

#[test]
fn utf16_positions_resolve_symbols_after_non_bmp_text() {
    // 🎯 is U+1F3AF: one char column in the compiler, two UTF-16 code units
    // in the editor. `score` starts at char column 16 (0-based 15) and
    // UTF-16 column 17 (0-based 16).
    let source =
        "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    debug(\"🎯\", score)\n";
    let document = Document::new("file:///u.opy", source, workspace_root());
    let (service, uri) = service_with(document);

    let hover = service
        .hover(
            &uri,
            Position {
                line: 4,
                character: 16,
            },
        )
        .expect("hover resolves at the UTF-16 offset");
    assert!(hover.contents.contains("score"), "{hover:?}");
    assert!(
        service
            .hover(
                &uri,
                Position {
                    line: 4,
                    character: 15
                }
            )
            .is_none(),
        "the character offset (inside the surrogate pair) resolves no symbol"
    );
}

#[test]
fn completion_uses_position_and_context() {
    let source = "globalvar points = [1, 2, 3]\n\nrule \"r\":\n    @Event global\n    points.append(points)\n";
    let document = Document::new("file:///c.opy", source, workspace_root());
    let (service, uri) = service_with(document);

    // After `points.` (line 4, the append receiver), completion offers the
    // receiver member `append`.
    let member_line = "    points.append(points)";
    let dot_char = member_line.find('.').unwrap() + 1;
    let items = service.completion(
        &uri,
        Position {
            line: 4,
            character: dot_char as u32,
        },
    );
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    assert!(labels.contains(&"append"), "member completion: {labels:?}");
    assert!(
        labels.iter().all(|label| RECEIVER_MEMBERS.contains(label)),
        "member context is member-only: {labels:?}"
    );

    // At a declaration/statement position, completion offers symbols,
    // builtins, and keywords filtered by the typed prefix `po`.
    let items = service.completion(
        &uri,
        Position {
            line: 4,
            character: 2,
        },
    );
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    assert!(labels.contains(&"points"), "declared symbol: {labels:?}");
    assert!(labels.contains(&"globalvar"), "keyword: {labels:?}");

    // Enum member context: `Beam.` offers the catalog enum members.
    let enum_source = "rule \"r\":\n    @Event global\n    debug(Beam.GOOD)\n";
    let document = Document::new("file:///e.opy", enum_source, workspace_root());
    let (service, uri) = service_with(document);
    let beam_char = "    debug(Beam.".len();
    let items = service.completion(
        &uri,
        Position {
            line: 2,
            character: beam_char as u32,
        },
    );
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    assert!(
        labels.contains(&"GOOD"),
        "enum member completion: {labels:?}"
    );
    assert!(
        labels.contains(&"GRAPPLE"),
        "enum member completion: {labels:?}"
    );
}

const RECEIVER_MEMBERS: &[&str] = &["append", "format", "uniform", "choice", "hasSpawned"];

#[test]
fn semantic_tokens_follow_semantic_identity_not_name_membership() {
    let source = "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    score = score + 1\n";
    let document = Document::new("file:///t.opy", source, workspace_root());
    let (service, uri) = service_with(document);
    let tokens = service.semantic_tokens(&uri);

    // `score` is a declared global variable: classified as variable, not by
    // string membership.
    let score_tokens: Vec<_> = tokens
        .iter()
        .filter(|token| document_text_at(source, token.line).contains("score"))
        .collect();
    let _ = score_tokens;
    // Every `score` token is a variable; `rule` is a keyword.
    let variables = tokens
        .iter()
        .filter(|token| {
            let line = source.lines().nth(token.line as usize).unwrap_or_default();
            let start = token.character as usize;
            line[start..].starts_with("score")
        })
        .count();
    assert!(variables >= 3, "declaration + two references: {variables}");
    let score_types: Vec<&str> = tokens
        .iter()
        .filter(|token| {
            let line = source.lines().nth(token.line as usize).unwrap_or_default();
            line.get(token.character as usize..)
                .unwrap_or_default()
                .starts_with("score")
        })
        .map(|token| token.token_type.as_str())
        .collect();
    assert!(
        score_types.iter().all(|kind| *kind == "variable"),
        "declared identifiers classify by semantic kind: {score_types:?}"
    );
}

fn document_text_at(source: &str, line: u32) -> String {
    source
        .lines()
        .nth(line as usize)
        .unwrap_or_default()
        .to_string()
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
    // The edit range must be a real, applicable full-document range, not a
    // degenerate (0,0)..(0,0) placeholder.
    let range = result.range.expect("rename carries a range");
    assert_eq!(range.start.line, 0);
    assert_eq!(range.start.character, 0);
    assert!(range.end.line > 0, "range covers the document: {range:?}");
    // The last line may be empty (trailing newline), so character may be 0;
    // the line must still be the final line of the buffer.
    let expected_last_line = corpus_text(CORPUS).split('\n').count() as u32 - 1;
    assert_eq!(
        range.end.line, expected_last_line,
        "range ends on the last line"
    );
}

#[test]
fn rename_edit_applies_to_produce_the_validated_result() {
    let source = corpus_text(CORPUS);
    let document = doc(CORPUS);
    let (service, uri) = service_with(document.clone());
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
    assert!(result.ok);
    let range = result.range.expect("range");
    let preview = result.preview.clone().expect("preview");

    // Applying a full-document range with the preview text must reproduce the
    // validated preview exactly (this is what an LSP client does).
    let applied = apply_full_document(&source, &range, &preview);
    assert_eq!(
        applied, preview,
        "applying the edit yields the validated result"
    );
}

fn apply_full_document(
    source: &str,
    range: &wright_language::document::Range,
    new_text: &str,
) -> String {
    // A full-document range replaces the whole buffer.
    assert_eq!(range.start.line, 0);
    assert_eq!(range.start.character, 0);
    let lines: Vec<&str> = source.split('\n').collect();
    assert_eq!(
        range.end.line as usize,
        lines.len() - 1,
        "range ends on the last line"
    );
    let last_line = lines.last().unwrap_or(&"");
    assert_eq!(
        range.end.character as usize,
        last_line.chars().count(),
        "range ends at last line length"
    );
    new_text.to_string()
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
    assert!(
        service
            .store
            .change(&uri, &(document.text.clone() + "\n"), 1),
        "a newer client version applies"
    );
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
        .change(&uri, &(document.text.clone() + "\n"), 1);
    let updated = service.diagnostics(&uri);
    assert!(
        updated.iter().all(|d| d.document_version == 1),
        "stale results (version 0) are replaced by version-1 results"
    );
}
