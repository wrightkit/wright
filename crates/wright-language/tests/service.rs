use std::path::PathBuf;

use wright_language::document::{Document, Position, line_col_position, uri_to_path};
use wright_language::{LanguageService, Range};

fn service() -> LanguageService {
    // An explicit but nonexistent provider executable keeps provider
    // resolution deterministic and offline: `resolve()` refuses it
    // without consulting a store or the network, so `.opy` documents
    // reach the structured provider boundary (#738, #1004).
    LanguageService::with_config(
        PathBuf::from("."),
        wright_language::SessionConfig {
            opy_provider: wright_language::OpyProviderConfig::with_executable(
                std::env::temp_dir()
                    .join(format!("wright-language-missing-{}", std::process::id()))
                    .join("opy-provider"),
            ),
            ..wright_language::SessionConfig::default()
        },
    )
}

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
        // `.opy` resolves a provider (the explicit executable above fails
        // validation); `.ostw`/`.del` have no provider to resolve at all —
        // both surface an explicit refusal, never a static fallback.
        let expected = if ext == "opy" {
            "provider-missing"
        } else {
            "source-provider-unavailable"
        };
        assert_eq!(
            diagnostics[0].code, expected,
            "{ext} documents surface an explicit provider refusal"
        );
    }
}

#[test]
fn workshop_diagnostics_match_check_for_the_same_text() {
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

    let definition = service
        .definition(
            &uri,
            Position {
                line: 24,
                character: 25,
            },
        )
        .expect("definition on a call site");
    assert_eq!(definition.len(), 1, "one declared span: {definition:?}");
    assert_eq!(definition[0].uri, uri);
    assert_eq!(
        definition[0].range,
        inspect_span(WORKSHOP_SOURCE, &symbols[shared]["span"])
    );

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

    let score = symbol_id("globalVariable", "score");
    let variable_definition = service
        .definition(
            &uri,
            Position {
                line: 15,
                character: 30,
            },
        )
        .expect("definition on a variable write");
    assert_eq!(
        variable_definition[0].range,
        inspect_span(WORKSHOP_SOURCE, &symbols[score]["span"])
    );
    let variable_hover = service
        .hover(
            &uri,
            Position {
                line: 15,
                character: 30,
            },
        )
        .expect("hover on a variable write");
    assert_eq!(variable_hover.contents, "`globalVariable score`");

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

    let uri = "file:///workspace/main.opy";
    service.store.open(
        Document::new(uri.to_string(), WORKSHOP_SOURCE, PathBuf::from("."))
            .with_language_id("workshop"),
    );
    assert!(
        !service.diagnostics(uri).is_empty(),
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

/// The language service's OPY provider path runs the shared connect path
/// (#1004): the provider session is spawned through `resolve()` (no
/// `installed()` fallback), `initialize` retries once at the highest
/// mutual version on `protocol-version-mismatch`, and a provider refusal
/// reaches the document surface under the driver's
/// `provider-refusal-<code>` classification, not the raw refusal code.
#[cfg(unix)]
#[test]
fn an_opy_provider_negotiates_and_classifies_refusals_like_the_driver() {
    use std::os::unix::fs::PermissionsExt;

    const PROVIDER: &str = r#"#!/usr/bin/env python3
import json, sys, pathlib
log = pathlib.Path(__file__).resolve().parent.joinpath("init-log")
main = pathlib.Path(__file__).resolve().parent.joinpath("main.opy").as_uri()
def reply(id, result=None, error=None):
    message = {"jsonrpc": "2.0", "id": id}
    message.update({"error": error} if error else {"result": result})
    print(json.dumps(message), flush=True)
def lpp_error(id, kind, details, text):
    reply(id, error={"code": -32000, "message": text, "data": {"lpp": {"kind": kind, "details": details}}})
for line in sys.stdin:
    request = json.loads(line)
    id, method = request["id"], request["method"]
    if method == "lpp/initialize":
        version = request["params"]["protocolVersion"]
        with open(log, "a") as f:
            f.write(version + "\n")
        if version == "1.0":
            lpp_error(id, "protocolVersionMismatch", {"supportedProtocolVersions": ["1.2"]}, "unsupported")
        else:
            reply(id, {"protocolVersion": version, "serverInfo": {"name": "fake", "version": "0"},
                       "languages": [{"id": "opy", "extensions": ["opy"]}],
                       "capabilities": {"check": True}})
    elif method == "lpp/check":
        lpp_error(id, "refusal", {"refusalCode": "check.tooLarge"}, "the project is too large")
    else:
        reply(id, {})
"#;

    let dir = std::env::temp_dir().join(format!(
        "wright-language-provider-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp directory");
    let script = dir.join("fake-provider");
    std::fs::write(&script, PROVIDER).expect("provider script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");

    let mut service = LanguageService::with_config(
        dir.clone(),
        wright_language::SessionConfig {
            opy_provider: wright_language::OpyProviderConfig::with_executable(&script),
            ..wright_language::SessionConfig::default()
        },
    );
    let uri = url::Url::from_file_path(dir.join("main.opy"))
        .expect("entry uri")
        .to_string();
    service.store.open(Document::new(
        uri.clone(),
        "globalvar score = 0\n".to_string(),
        dir.clone(),
    ));

    let diagnostics = service.diagnostics(&uri);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(
        diagnostics[0].code, "provider-refusal-check.tooLarge",
        "the language service emits the driver's refusal classification"
    );
    let attempts: Vec<String> = std::fs::read_to_string(dir.join("init-log"))
        .expect("init log")
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(
        attempts,
        vec!["1.0".to_string(), "1.2".to_string()],
        "the rejected preferred version retries once at the highest mutual version"
    );

    std::fs::remove_dir_all(&dir).expect("remove test directory");
}
