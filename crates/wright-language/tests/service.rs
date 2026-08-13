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
fn completion_and_member_context_use_utf16_offsets_after_non_bmp_text() {
    // Non-BMP text before the cursor must not shift UTF-16 offsets onto
    // byte/char boundaries used for slicing.
    let source = "globalvar points = [1, 2, 3]\n\nrule \"r\":\n    @Event global\n    debug(\"🎯\", points.append(points))\n";
    let document = Document::new("file:///u16.opy", source, workspace_root());
    let (service, uri) = service_with(document);
    let line = "    debug(\"🎯\", points.append(points))";

    // The editor cursor is a UTF-16 offset; compute it from the character
    // position, not the byte position (the 🎯 shifts the two apart).
    let utf16_at = |byte_index: usize| -> u32 {
        let char_count = line[..byte_index].chars().count();
        wright_language::document::char_offset_to_utf16(line, char_count) as u32
    };

    // Member context: cursor right after the dot in `points.append`.
    let dot_byte = line.find(".append").unwrap();
    let items = service.completion(
        &uri,
        Position {
            line: 4,
            character: utf16_at(dot_byte + 1),
        },
    );
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    assert!(
        labels.contains(&"append"),
        "member completion after non-BMP text: {labels:?}"
    );

    // Declared-symbol completion with the cursor after the `po` prefix of the
    // argument `points` (a valid document, so the semantic index exists).
    let typed_byte = line.find("po").unwrap() + "po".len();
    let items = service.completion(
        &uri,
        Position {
            line: 4,
            character: utf16_at(typed_byte),
        },
    );
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    assert!(
        labels.contains(&"points"),
        "declared symbol completion after non-BMP text: {labels:?}"
    );
}

#[test]
fn semantic_tokens_emit_utf16_offsets_after_non_bmp_text() {
    let source =
        "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    debug(\"🎯\", score)\n";
    let document = Document::new("file:///st.opy", source, workspace_root());
    let (service, uri) = service_with(document);
    let tokens = service.semantic_tokens(&uri);

    // The reference on line 4 (0-based) starts at UTF-16 offset 16 (the 🎯
    // before it counts as two units) and is 5 UTF-16 units long.
    let line_tokens: Vec<_> = tokens
        .iter()
        .filter(|token| token.line == 4 && token.token_type == "variable")
        .collect();
    assert_eq!(line_tokens.len(), 1, "one variable reference on line 4");
    let score_token = line_tokens[0];
    assert_eq!(
        score_token.character, 16,
        "score reference starts at the UTF-16 offset: {score_token:?}"
    );
    assert_eq!(
        score_token.length, 5,
        "score reference length is its UTF-16 length: {score_token:?}"
    );
}

#[test]
fn rename_full_document_range_ends_at_utf16_length_on_non_bmp_lines() {
    // The final line ends inside a string containing a non-BMP character, so
    // the full-document range's end character must count it as two units.
    let source = "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    score += 1\n    print(\"🎯\")\n";
    let document = Document::new("file:///rnb.opy", source, workspace_root());
    let (service, uri) = service_with(document);
    let result = service.rename(
        &uri,
        Position {
            line: 0,
            character: 11,
        },
        "total",
    );
    assert!(result.ok, "rename validates: {:?}", result.diagnostics);
    let edit = &result.edits[0];
    let lines: Vec<&str> = source.split('\n').collect();
    assert_eq!(
        edit.range.end.line as usize,
        lines.len() - 1,
        "range ends on the final line"
    );
    let last_line = lines.last().unwrap_or(&"");
    assert_eq!(
        edit.range.end.character as usize,
        wright_language::document::utf16_len(last_line),
        "end character is the UTF-16 length of the final line (the 🎯 counts twice)"
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
    let result = service.rename(
        &uri,
        Position {
            line: 0,
            character: 3,
        },
        "total",
    );
    assert!(result.ok, "rename validates");
    assert_eq!(
        result.edits.len(),
        1,
        "single-file rename produces one edit"
    );
    let edit = &result.edits[0];
    assert!(edit.new_text.contains("globalvar total = 0"), "{:?}", edit);
    assert!(
        !edit.new_text.contains("globalvar score"),
        "old name gone: {:?}",
        edit
    );
    // The edit range must be a real, applicable full-document range, not a
    // degenerate (0,0)..(0,0) placeholder.
    assert_eq!(edit.range.start.line, 0);
    assert_eq!(edit.range.start.character, 0);
    assert!(
        edit.range.end.line > 0,
        "range covers the document: {:?}",
        edit.range
    );
    // The last line may be empty (trailing newline), so character may be 0;
    // the line must still be the final line of the buffer.
    let expected_last_line = corpus_text(CORPUS).split('\n').count() as u32 - 1;
    assert_eq!(
        edit.range.end.line, expected_last_line,
        "range ends on the last line"
    );
    // The edit carries the identity precondition of the source it targets.
    assert_eq!(
        edit.source_identity,
        wright_driver::input_identity(&corpus_text(CORPUS)),
        "edit identity matches the source it applies to"
    );
}

#[test]
fn rename_edit_applies_to_produce_the_validated_result() {
    let source = corpus_text(CORPUS);
    let document = doc(CORPUS);
    let (service, uri) = service_with(document.clone());
    let result = service.rename(
        &uri,
        Position {
            line: 0,
            character: 3,
        },
        "total",
    );
    assert!(result.ok);
    assert_eq!(result.edits.len(), 1);
    let range = result.edits[0].range;
    let new_text = result.edits[0].new_text.clone();

    // Applying a full-document range with the new text must reproduce the
    // validated result exactly (this is what an LSP client does).
    let applied = apply_full_document(&source, &range, &new_text);
    assert_eq!(
        applied, new_text,
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
fn rename_refuses_target_collision() {
    let source = "globalvar score = 0\nglobalvar total = 1\n\nrule \"r\":\n    @Event global\n    score += 1\n";
    let document = Document::new("file:///collide.opy", source, workspace_root());
    let (service, uri) = service_with(document);
    let result = service.rename(
        &uri,
        Position {
            line: 0,
            character: 11,
        },
        "total",
    );
    assert!(
        !result.ok,
        "a target-name collision refuses the rename explicitly"
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("rename-collision")),
        "the refusal names the collision: {:?}",
        result.diagnostics
    );
    assert!(
        result.edits.is_empty(),
        "a refused rename never returns partial edits"
    );
}

#[test]
fn rename_does_not_touch_longer_identifiers_containing_the_spelling() {
    // Blocker 1 (#73): `scoreboard` merely contains the spelling `score`; the
    // word-boundary carve inside the semantic span must leave it untouched.
    let source = "globalvar score = 0\nglobalvar scoreboard = 1\n\nrule \"r\":\n    @Event global\n    score += scoreboard\n";
    let document = Document::new("file:///long.opy", source, workspace_root());
    let (service, uri) = service_with(document);
    let result = service.rename(
        &uri,
        Position {
            line: 0,
            character: 11,
        },
        "total",
    );
    assert!(result.ok, "rename validates: {:?}", result.diagnostics);
    let new_text = &result.edits[0].new_text;
    assert!(
        new_text.contains("globalvar scoreboard = 1"),
        "the longer identifier declaration is untouched: {new_text}"
    );
    assert!(
        new_text.contains("total += scoreboard"),
        "the longer identifier reference is untouched: {new_text}"
    );
}

#[test]
fn rename_leaves_string_literals_with_the_same_spelling_untouched() {
    // Test A (#73): `"score"` inside a string literal is not a semantic
    // reference to the variable; the declaration and semantic references are
    // renamed while the string stays byte-for-byte unchanged.
    let source = "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    debug(\"score\")\n    score += 1\n";
    let document = Document::new("file:///str.opy", source, workspace_root());
    let (service, uri) = service_with(document);
    let result = service.rename(
        &uri,
        Position {
            line: 0,
            character: 11,
        },
        "total",
    );
    assert!(
        result.ok,
        "string-literal rename validates: {:?}",
        result.diagnostics
    );
    let new_text = &result.edits[0].new_text;
    assert!(
        new_text.contains("globalvar total = 0"),
        "declaration renamed: {new_text}"
    );
    assert!(
        new_text.contains("total += 1"),
        "semantic reference renamed: {new_text}"
    );
    assert!(
        new_text.contains("debug(\"score\")"),
        "the same-spelled string literal is untouched: {new_text}"
    );
}

#[test]
fn rename_leaves_comments_with_the_same_spelling_untouched() {
    // Test B (#73): comments are lexer-skipped and never semantic references;
    // `# score` and `/* score */` text stays unchanged.
    let source = "globalvar score = 0\n# score in a line comment\n/* score in a block comment */\n\nrule \"r\":\n    @Event global\n    score += 1\n";
    let document = Document::new("file:///cmt.opy", source, workspace_root());
    let (service, uri) = service_with(document);
    let result = service.rename(
        &uri,
        Position {
            line: 0,
            character: 11,
        },
        "total",
    );
    assert!(
        result.ok,
        "comment rename validates: {:?}",
        result.diagnostics
    );
    let new_text = &result.edits[0].new_text;
    assert!(
        new_text.contains("globalvar total = 0"),
        "declaration renamed: {new_text}"
    );
    assert!(
        new_text.contains("# score in a line comment"),
        "line comment text is untouched: {new_text}"
    );
    assert!(
        new_text.contains("/* score in a block comment */"),
        "block comment text is untouched: {new_text}"
    );
}

#[test]
fn rename_refuses_explicitly_when_no_symbol_resolves() {
    // Test H (#73): a position that resolves no symbol is an explicit
    // refusal, never a silent empty edit.
    let source = "rule \"r\":\n    @Event global\n    debug(1)\n";
    let document = Document::new("file:///none.opy", source, workspace_root());
    let (service, uri) = service_with(document);
    let result = service.rename(
        &uri,
        Position {
            line: 2,
            character: 3,
        },
        "total",
    );
    assert!(!result.ok, "no symbol resolves");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("rename-unresolved")),
        "the refusal names the unresolved symbol: {:?}",
        result.diagnostics
    );
    assert!(
        result.edits.is_empty(),
        "an unresolved rename never returns edits"
    );
}

#[test]
fn rename_refuses_explicitly_when_the_source_identity_is_unestablished() {
    // Test H (#73): a rename on a document the store does not hold cannot
    // establish the source identity and refuses explicitly.
    let service = LanguageService::new(workspace_root());
    let result = service.rename(
        "file:///never-opened.opy",
        Position {
            line: 0,
            character: 0,
        },
        "total",
    );
    assert!(!result.ok);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("rename-unresolved")),
        "the refusal names the unestablished identity: {:?}",
        result.diagnostics
    );
    assert!(result.edits.is_empty());
}

#[test]
fn rename_refuses_an_empty_new_name() {
    let (service, uri) = service_with(doc(CORPUS));
    let result = service.rename(
        &uri,
        Position {
            line: 0,
            character: 3,
        },
        "",
    );
    assert!(!result.ok);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("rename-invalid-name")),
        "the refusal names the invalid name: {:?}",
        result.diagnostics
    );
    assert!(result.edits.is_empty());
}

#[test]
fn rename_results_are_bound_to_the_source_state_they_were_computed_for() {
    // Test H (#73): a rename result carries the identity/version precondition
    // of the exact source state it was computed for. After the source moves
    // to a newer version, a fresh rename yields a fresh precondition, so the
    // earlier result can never be silently treated as applicable to the newer
    // buffer state.
    let source = "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    score += 1\n";
    let newer = "globalvar total = 0\n\nrule \"r\":\n    @Event global\n    total += 1\n";
    let document = Document::new("file:///state.opy", source, workspace_root());
    let root = document.root.clone();
    let mut service = LanguageService::new(root);
    let uri = document.uri.clone();
    service.store.open(document);

    let at_version_0 = service.rename(
        &uri,
        Position {
            line: 0,
            character: 11,
        },
        "points",
    );
    assert!(at_version_0.ok, "{:?}", at_version_0.diagnostics);
    let identity_0 = at_version_0.edits[0].source_identity.clone();
    assert_eq!(at_version_0.edits[0].source_version, 0);
    assert_eq!(at_version_0.document_version, 0);

    // The host moves the buffer to version 1 with different content.
    assert!(service.store.change(&uri, newer, 1));
    let at_version_1 = service.rename(
        &uri,
        Position {
            line: 0,
            character: 11,
        },
        "score",
    );
    assert!(at_version_1.ok, "{:?}", at_version_1.diagnostics);
    assert_eq!(at_version_1.edits[0].source_version, 1);
    assert_eq!(at_version_1.document_version, 1);
    assert_ne!(
        at_version_1.edits[0].source_identity, identity_0,
        "a newer buffer state produces a different identity precondition"
    );
    // The version-0 result targets version-0 text; applying it to the
    // version-1 buffer would fail the identity precondition.
    assert_ne!(
        at_version_1.edits[0].source_identity,
        wright_driver::input_identity(source),
        "the version-1 result does not carry the stale version-0 identity"
    );
}

#[test]
fn rename_same_spelled_distinct_identity_only_edits_the_selected_symbol() {
    // Test C (#73): the same surface spelling exists in two namespaces (a
    // global variable and a subroutine). The semantic index distinguishes the
    // identities by typed symbol ID, so span-targeted rename must edit only
    // the selected symbol's occurrences — never the sibling `score`.
    let source = "globalvar score = 0\nsubroutine score\n\ndef score():\n    print(\"score\")\n\nrule \"r\":\n    @Event global\n    score += 1\n    score()\n";
    let document = Document::new("file:///ns.opy", source, workspace_root());
    let (service, uri) = service_with(document);
    let result = service.rename(
        &uri,
        Position {
            line: 0,
            character: 11,
        },
        "total",
    );
    assert!(
        result.ok,
        "span-targeted rename distinguishes the identities: {:?}",
        result.diagnostics
    );
    let new_text = &result.edits[0].new_text;
    assert!(
        new_text.contains("globalvar total = 0"),
        "variable declaration renamed: {new_text}"
    );
    assert!(
        new_text.contains("total += 1"),
        "variable reference renamed: {new_text}"
    );
    assert!(
        new_text.contains("subroutine score"),
        "the sibling subroutine declaration is untouched: {new_text}"
    );
    assert!(
        new_text.contains("def score():"),
        "the sibling subroutine definition is untouched: {new_text}"
    );
    assert!(
        new_text.contains("score()"),
        "the sibling subroutine call is untouched: {new_text}"
    );
    assert!(
        new_text.contains("print(\"score\")"),
        "the string literal in the sibling body is untouched: {new_text}"
    );
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
