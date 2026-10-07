use std::path::PathBuf;

use wright_language::document::{Document, Position, line_col_position, uri_to_path};
use wright_language::{LanguageService, Range};

fn service() -> LanguageService {
    LanguageService::new(PathBuf::from("."))
}

/// A raw Workshop program with a declared global variable, a subroutine
/// called from a rule, and two rules. `wright inspect` on the same text is
/// the reference the semantic queries are compared against.
const WORKSHOP_SOURCE: &str = concat!(
    "variables {\n",
    "    global:\n",
    "        0: score\n",
    "}\n",
    "\n",
    "subroutines {\n",
    "    0: shared\n",
    "}\n",
    "\n",
    "rule (\"Subroutine shared\") {\n",
    "    event {\n",
    "        Subroutine;\n",
    "        shared;\n",
    "    }\n",
    "    actions {\n",
    "        Set Global Variable(score, 1);\n",
    "    }\n",
    "}\n",
    "\n",
    "rule (\"main\") {\n",
    "    event {\n",
    "        Ongoing - Global;\n",
    "    }\n",
    "    actions {\n",
    "        Call Subroutine(shared);\n",
    "        Set Global Variable(score, 2);\n",
    "    }\n",
    "}\n",
);

fn open_workshop(service: &mut LanguageService, text: &str) -> String {
    let uri = "file:///workspace/main.ws".to_string();
    service.store.open(
        Document::new(uri.clone(), text.to_string(), PathBuf::from("."))
            .with_language_id("workshop"),
    );
    uri
}

/// A 1-based `{start,end}` span from the inspect JSON contract as the
/// document's UTF-16 range.
fn inspect_span(text: &str, span: &serde_json::Value) -> Range {
    let at = |key: &str| {
        line_col_position(
            text,
            span[key]["line"].as_u64().unwrap() as u32,
            span[key]["col"].as_u64().unwrap() as u32,
        )
    };
    Range {
        start: at("start"),
        end: at("end"),
    }
}

#[test]
fn source_documents_report_provider_boundary_without_static_fallback() {
    let mut service = service();
    for ext in ["opy", "ostw", "del"] {
        let uri = format!("file:///workspace/main.{ext}");
        service.store.open(Document::new(
            uri.clone(),
            "globalvar score = 0\n".to_string(),
            PathBuf::from("."),
        ));
        let diagnostics = service.diagnostics(&uri);
        assert_eq!(
            diagnostics[0].code, "source-provider-unavailable",
            "{ext} documents surface an explicit provider refusal"
        );
    }
}

#[test]
fn workshop_diagnostics_match_check_for_the_same_text() {
    // The reference is the same text through the `Path` input — the
    // pipeline `wright check` itself runs.
    let text = concat!(
        "variables {\n",
        "    global:\n",
        "        0: score\n",
        "}\n",
        "\n",
        "rule (\"x\") {\n",
        "    event {\n",
        "        Ongoing - Global;\n",
        "    }\n",
        "    actions {\n",
        "        Set Global Variable(score, Bogus(1));\n",
        "    }\n",
        "}\n",
    );
    let temp = std::env::temp_dir().join(format!("wright-language-{}", std::process::id()));
    std::fs::create_dir_all(&temp).expect("temp dir");
    let path = temp.join("buffer.ws");
    std::fs::write(&path, text).expect("write reference input");
    let mut reference = wright_driver::CompilerSession::new(wright_driver::SessionConfig {
        input: wright_driver::InputSpec::Path(path),
        kind: wright_driver::SourceKind::Workshop,
        ..wright_driver::SessionConfig::default()
    })
    .expect("reference session");
    let expected = reference.check().diagnostics.clone();

    let mut service = service();
    let uri = open_workshop(&mut service, text);
    let diagnostics = service.diagnostics(&uri);
    std::fs::remove_dir_all(&temp).ok();

    assert_eq!(
        diagnostics.len(),
        expected.len(),
        "every check diagnostic publishes: {diagnostics:?} vs {expected:?}"
    );
    for (diagnostic, expected) in diagnostics.iter().zip(expected.iter()) {
        assert_eq!(diagnostic.code, expected.code);
        assert_eq!(diagnostic.severity, expected.severity.as_str());
        let span = expected.span.as_ref().expect("check span");
        assert_eq!(
            diagnostic.range,
            Range {
                start: line_col_position(text, span.start.line, span.start.col),
                end: line_col_position(text, span.end.line, span.end.col),
            },
            "the check span lands on the buffer position: {diagnostic:?}"
        );
        assert_eq!(diagnostic.document_version, 0);
    }
}

#[test]
fn workshop_diagnostics_follow_the_current_buffer() {
    let mut service = service();
    let uri = open_workshop(&mut service, WORKSHOP_SOURCE);
    assert!(
        service.diagnostics(&uri).is_empty(),
        "a clean buffer publishes no diagnostics"
    );
    service.store.change(&uri, "rule (\"x\" {\n", 1);
    let diagnostics = service.diagnostics(&uri);
    assert!(
        diagnostics.iter().any(|d| d.severity == "error"),
        "the changed buffer republishes its own diagnostics: {diagnostics:?}"
    );
    assert_eq!(diagnostics[0].document_version, 1);
}

#[test]
fn workshop_semantic_queries_match_inspect_for_the_same_text() {
    let mut service = service();
    let uri = open_workshop(&mut service, WORKSHOP_SOURCE);

    // `wright inspect` over the same buffer text is the reference every
    // query resolves against.
    let mut session = wright_driver::CompilerSession::new(wright_driver::SessionConfig {
        input: wright_driver::InputSpec::Text {
            text: WORKSHOP_SOURCE.to_string(),
            path: uri_to_path(&uri),
        },
        kind: wright_driver::SourceKind::Workshop,
        ..wright_driver::SessionConfig::default()
    })
    .expect("inspect session");
    let inspect = session.inspect();
    let symbols = inspect.result.symbols.as_array().expect("symbols");
    let references = inspect.result.references.as_array().expect("references");
    let symbol_id = |kind: &str, name: &str| {
        symbols
            .iter()
            .find(|s| s["kind"] == kind && s["name"] == name)
            .and_then(|s| s["id"].as_u64())
            .expect("symbol exists") as usize
    };
    let shared = symbol_id("subroutine", "shared");

    // Hover at the `shared` call site names the symbol inspect reports.
    let hover = service
        .hover(
            &uri,
            Position {
                line: 24,
                character: 25,
            },
        )
        .expect("hover on an identifier occurrence");
    assert_eq!(hover.contents, "`subroutine shared`");

    // Definition targets the declared span inspect reports for the symbol.
    let definition = service
        .definition(
            &uri,
            Position {
                line: 24,
                character: 25,
            },
        )
        .expect("definition on a call site");
    assert_eq!(definition.uri, uri);
    assert_eq!(
        definition.range,
        inspect_span(WORKSHOP_SOURCE, &symbols[shared]["span"])
    );

    // References are the occurrence spans inspect reports for the symbol.
    let mut locations: Vec<Range> = service
        .references(
            &uri,
            Position {
                line: 24,
                character: 25,
            },
            true,
        )
        .expect("references on a call site")
        .into_iter()
        .map(|location| location.range)
        .collect();
    locations.sort_by_key(|r| (r.start.line, r.start.character));
    let mut expected: Vec<Range> = references[shared]
        .as_array()
        .expect("references array")
        .iter()
        .map(|reference| inspect_span(WORKSHOP_SOURCE, &reference["span"]))
        .collect();
    expected.sort_by_key(|r| (r.start.line, r.start.character));
    assert_eq!(locations, expected);

    // `includeDeclaration` filters the declaration kind inspect reports.
    let filtered = service
        .references(
            &uri,
            Position {
                line: 24,
                character: 25,
            },
            false,
        )
        .expect("references");
    assert_eq!(
        filtered.len(),
        expected.len() - 1,
        "the declaration reference drops out"
    );

    // A rule name resolves its rule symbol.
    let rule_hover = service
        .hover(
            &uri,
            Position {
                line: 19,
                character: 9,
            },
        )
        .expect("hover on a rule name");
    assert_eq!(rule_hover.contents, "`rule main`");

    // Positions outside any identifier occurrence resolve nothing.
    for position in [
        Position {
            line: 0,
            character: 2,
        },
        Position {
            line: 20,
            character: 4,
        },
    ] {
        assert!(service.hover(&uri, position).is_none());
        assert!(service.definition(&uri, position).is_none());
        assert!(service.references(&uri, position, true).is_none());
    }
}

#[test]
fn non_workshop_documents_keep_their_boundaries() {
    // A `.txt` buffer a host tags as plaintext stays quiet even though the
    // extension is a Workshop spelling; an untagged `.md` file is not a
    // Workshop document either.
    let mut service = service();
    for (name, language_id) in [("notes.txt", Some("plaintext")), ("notes.md", None)] {
        let uri = format!("file:///workspace/{name}");
        let mut document = Document::new(uri.clone(), WORKSHOP_SOURCE, PathBuf::from("."));
        document.language_id = language_id.map(str::to_string);
        service.store.open(document);
        assert!(
            service.diagnostics(&uri).is_empty(),
            "{name} is not a Workshop document"
        );
        assert!(
            service
                .hover(
                    &uri,
                    Position {
                        line: 24,
                        character: 25
                    }
                )
                .is_none()
        );
    }

    // Source-language documents keep the provider boundary even when a
    // client mistags the language id.
    let uri = "file:///workspace/main.opy";
    service.store.open(
        Document::new(uri.to_string(), WORKSHOP_SOURCE, PathBuf::from("."))
            .with_language_id("workshop"),
    );
    assert_eq!(
        service.diagnostics(uri)[0].code,
        "source-provider-unavailable",
        "the provider boundary wins over an extension tag"
    );
    assert!(
        service
            .hover(
                uri,
                Position {
                    line: 0,
                    character: 0
                }
            )
            .is_none()
    );
    assert!(
        service
            .definition(
                uri,
                Position {
                    line: 0,
                    character: 0
                }
            )
            .is_none()
    );
    assert!(
        service
            .references(
                uri,
                Position {
                    line: 0,
                    character: 0
                },
                true
            )
            .is_none()
    );
}
