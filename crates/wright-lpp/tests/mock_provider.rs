//! End-to-end LPP client tests against the reference conformance mock
//! provider from the `language-provider-protocol` repository.

#![allow(clippy::result_large_err)]

use std::path::{Path, PathBuf};

use wright_lpp::{
    Capability, ClientInfo, Document, DocumentSet, LanguageProvider, LppErrorKind, Position,
    ProviderConfig, ProviderError, ProviderRegistry, RegistryError, StdioLanguageProvider,
    TextEdit, WorkshopArtifact,
};

const PINNED_LPP_COMMIT: &str = "416b293e26e6fb2d29061608a493a7aecd2ce14f";
const DEMO_LANGUAGE_ID: &str = "x-demo-lang";
const DEMO_ARTIFACT_FORMAT: &str = "x-demo/puzzle-eval-v1";
const CLEAN_PUZZLE: &str = "puzzle clean {\n  target = 40\n  start = 10\n  ops {\n    double: x => x * 2\n    plus1: x => x + 1\n  }\n  solution = [ double, double ]\n}";
const BROKEN_PUZZLE: &str = "puzzle broken {\n  target = 40\n  start = 10\n  ops {\n    double: x => x * 2\n  }\n  solution = [ triple ]\n}";
const PUZZLE_URI: &str = "file:///project/puzzle.xdl";

fn mock_provider_path() -> Option<PathBuf> {
    match std::env::var("LPP_MOCK_PROVIDER") {
        Ok(path) if !path.is_empty() => Some(PathBuf::from(path)),
        _ => {
            eprintln!(
                "SKIPPED: LPP_MOCK_PROVIDER is not set; build from commit {PINNED_LPP_COMMIT}"
            );
            None
        }
    }
}

fn demo_document(uri: &str, text: &str) -> Document {
    Document {
        uri: uri.to_string(),
        language_id: DEMO_LANGUAGE_ID.to_string(),
        version: 3,
        text: text.to_string(),
    }
}

fn doc_set(uri: &str, text: &str) -> DocumentSet {
    let mut docs = DocumentSet::new();
    docs.insert(uri.to_string(), demo_document(uri, text));
    docs
}

fn clean_document_set() -> DocumentSet {
    doc_set(PUZZLE_URI, CLEAN_PUZZLE)
}

fn broken_document_set() -> DocumentSet {
    doc_set("file:///project/broken.xdl", BROKEN_PUZZLE)
}

fn initialized() -> Option<(StdioLanguageProvider, wright_lpp::InitializeResult)> {
    let path = mock_provider_path()?;
    let mut provider =
        StdioLanguageProvider::spawn(&path, &[], std::time::Duration::from_secs(30)).ok()?;
    let result = provider
        .initialize(Some(&ClientInfo {
            name: "wright".to_string(),
            version: "0.2.0".to_string(),
        }))
        .ok()?;
    Some((provider, result))
}

fn with_mock(f: impl FnOnce(&mut StdioLanguageProvider)) {
    let Some((mut p, _)) = initialized() else {
        return;
    };
    f(&mut p);
    p.shutdown().expect("shutdown");
}

fn pos(line: u32, character: u32) -> Position {
    Position { line, character }
}

#[test]
fn initializes_and_negotiates_capabilities_with_x_demo_lang() {
    let Some((mut provider, result)) = initialized() else {
        return;
    };
    assert_eq!(result.protocol_version, "1.0");
    assert_eq!(result.server_info.name, "lpp-mock-provider");
    assert_eq!(result.languages.len(), 1);
    assert_eq!(result.languages[0].id, DEMO_LANGUAGE_ID);
    assert_eq!(result.languages[0].extensions, vec!["xdl"]);
    let negotiated = provider.capabilities().expect("negotiated");
    assert_eq!(negotiated.language_ids(), vec![DEMO_LANGUAGE_ID]);
    for cap in Capability::ALL {
        if cap != Capability::ProjectLoading {
            assert!(
                negotiated.supports(cap),
                "capability {} negotiated",
                cap.as_str()
            );
        }
    }
    provider.shutdown().expect("shutdown");
    assert_eq!(provider.exit_status(), Some(0));
}

#[test]
fn registry_lookup_is_by_opaque_language_id() {
    let Some(path) = mock_provider_path() else {
        return;
    };
    let mut registry = ProviderRegistry::new();
    registry
        .register(ProviderConfig::new(DEMO_LANGUAGE_ID, path, Vec::new()))
        .expect("registered");
    assert_eq!(
        registry
            .register(ProviderConfig::new(
                DEMO_LANGUAGE_ID,
                PathBuf::from("ignored"),
                Vec::new(),
            ))
            .unwrap_err(),
        RegistryError::DuplicateLanguage {
            language_id: DEMO_LANGUAGE_ID.to_string(),
        }
    );
    drop(registry.spawn(DEMO_LANGUAGE_ID).expect("spawns"));
    let error = registry
        .spawn("x-other-lang")
        .err()
        .expect("not configured");
    assert_eq!(error.code(), "provider-not-configured");
    assert_eq!(
        error,
        ProviderError::NotConfigured {
            language_id: "x-other-lang".to_string(),
        }
    );
}

#[test]
fn check_reports_clean_and_broken_documents() {
    with_mock(|p| {
        let checked = p
            .check(&clean_document_set(), Some("file:///project"))
            .expect("check");
        assert_eq!(checked.documents.len(), 1);
        assert!(checked.documents[0].diagnostics.is_empty(), "clean puzzle");

        let checked = p.check(&broken_document_set(), None).expect("check");
        let diagnostics = &checked.documents[0].diagnostics;
        assert_eq!(diagnostics.len(), 1, "one unresolved op reference");
        assert_eq!(
            diagnostics[0].severity,
            wright_lpp::DiagnosticSeverity::Error
        );
        assert_eq!(diagnostics[0].code.as_deref(), Some("x-demo/unresolved-op"));
        assert_eq!(diagnostics[0].source.as_deref(), Some(DEMO_LANGUAGE_ID));
    });
}

#[test]
fn compile_produces_an_opaque_artifact_and_refuses_on_errors() {
    with_mock(|p| {
        let compiled = p
            .compile(&clean_document_set(), Some("file:///project"))
            .expect("compile");
        let artifact = compiled.artifact.expect("clean puzzle compiles");
        assert_eq!(artifact.format, DEMO_ARTIFACT_FORMAT);
        let content: serde_json::Value =
            serde_json::from_str(&artifact.content).expect("artifact content");
        assert_eq!(content["name"], "clean");
        assert_eq!(content["target"], 40);

        let compiled = p.compile(&broken_document_set(), None).expect("compile");
        assert!(
            compiled.artifact.is_none(),
            "artifact must be null on error"
        );
        assert_eq!(
            compiled.diagnostics[0].diagnostics[0].code.as_deref(),
            Some("x-demo/unresolved-op")
        );

        let mut multi = clean_document_set();
        multi.insert(
            "file:///project/second.xdl".into(),
            demo_document("file:///project/second.xdl", CLEAN_PUZZLE),
        );
        let error = p.compile(&multi, None).expect_err("multi-doc refusal");
        assert_eq!(error.code(), "refusal");
        assert_eq!(error.refusal_code(), Some("compile.requiresSingleDocument"));
    });
}

#[test]
fn reconstruct_roundtrips_an_artifact_and_refuses_unknown_formats() {
    with_mock(|p| {
        let compiled = p
            .compile(&clean_document_set(), None)
            .expect("compile")
            .artifact
            .expect("artifact");
        let reconstructed = p.reconstruct(&compiled).expect("reconstruct");
        assert!(reconstructed.source.contains("puzzle clean"));

        let unsupported = p
            .reconstruct(&WorkshopArtifact {
                format: "other/format".into(),
                content: "{}".into(),
            })
            .expect_err("unsupported format");
        assert_eq!(unsupported.code(), "refusal");
        assert_eq!(
            unsupported.refusal_code(),
            Some("reconstruct.artifactFormatUnsupported")
        );

        let malformed = p
            .reconstruct(&WorkshopArtifact {
                format: DEMO_ARTIFACT_FORMAT.into(),
                content: "not a puzzle sheet".into(),
            })
            .expect_err("malformed content");
        assert_eq!(malformed.code(), "invalid-artifact");
        assert!(
            matches!(&malformed, ProviderError::Lpp(lpp) if lpp.kind == LppErrorKind::InvalidArtifact)
        );
    });
}

#[test]
fn symbols_definition_and_references_resolve_across_the_document() {
    with_mock(|p| {
        let documents = clean_document_set();
        let symbols = p.symbols(&documents, None).expect("symbols");
        let names: Vec<(&str, &str)> = symbols.documents[0]
            .symbols
            .iter()
            .map(|s| (s.name.as_str(), s.kind.as_str()))
            .collect();
        assert_eq!(
            names,
            vec![("clean", "puzzle"), ("double", "op"), ("plus1", "op")]
        );

        let p_pos = pos(4, 6);
        let definition = p
            .definition(&documents[PUZZLE_URI], p_pos)
            .expect("definition");
        assert_eq!(definition.locations.len(), 1);
        assert_eq!(definition.locations[0].range.start.character, 4);

        let references = p
            .references(&documents[PUZZLE_URI], p_pos, true)
            .expect("references");
        let spans: Vec<(u32, u32)> = references
            .locations
            .iter()
            .map(|l| (l.range.start.line, l.range.start.character))
            .collect();
        assert_eq!(spans, vec![(4, 4), (7, 15), (7, 23)]);

        let err = p.definition(&documents[PUZZLE_URI], pos(1, 0)).unwrap_err();
        assert_eq!(err.refusal_code(), Some("definition.noSymbolAtPosition"));

        let err = p
            .definition(&documents[PUZZLE_URI], pos(99, 0))
            .unwrap_err();
        assert_eq!(err.code(), "invalid-position");
    });
}

#[test]
fn rename_computes_source_edits_and_refuses_invalid_names_and_collisions() {
    with_mock(|p| {
        let documents = clean_document_set();
        let p_pos = pos(4, 6);
        let renamed = p
            .rename(&documents, PUZZLE_URI, p_pos, "twice", None)
            .expect("rename");
        assert_eq!(renamed.edits.len(), 1);
        let edits = &renamed.edits[0];
        assert_eq!(edits.document_uri, PUZZLE_URI);
        assert_eq!(edits.version, 3);
        assert_eq!(edits.text_edits.len(), 3);
        assert!(edits.text_edits.iter().all(|e| e.new_text == "twice"));

        let invalid = p
            .rename(&documents, PUZZLE_URI, p_pos, "not a name!", None)
            .unwrap_err();
        assert_eq!(invalid.refusal_code(), Some("rename.invalidName"));

        let collision = p
            .rename(&documents, PUZZLE_URI, p_pos, "plus1", None)
            .unwrap_err();
        assert_eq!(collision.refusal_code(), Some("rename.nameCollision"));
    });
}

#[test]
fn validate_edits_applies_the_normative_rules() {
    with_mock(|p| {
        let document = demo_document(PUZZLE_URI, CLEAN_PUZZLE);
        let renamed = p
            .rename(&clean_document_set(), PUZZLE_URI, pos(4, 6), "twice", None)
            .expect("rename");
        let edits: Vec<TextEdit> = renamed.edits[0].text_edits.clone();
        let validated = p.validate_edits(&document, &edits).expect("validation");
        assert!(validated.valid);
        assert_eq!(validated.version, 3);

        let overlapping = p
            .validate_edits(&document, &[edits[0].clone(), edits[0].clone()])
            .expect("validation");
        assert!(!overlapping.valid);
        assert_eq!(overlapping.reason.as_deref(), Some("overlappingEdits"));
        assert_eq!(overlapping.failing_edit_index, Some(1));
    });
}

#[test]
fn unserved_language_id_is_refused_by_the_provider() {
    with_mock(|p| {
        let mut documents = DocumentSet::new();
        documents.insert(
            "file:///project/other.xdl".into(),
            Document {
                uri: "file:///project/other.xdl".into(),
                language_id: "x-other-lang".into(),
                version: 1,
                text: "whatever".into(),
            },
        );
        let error = p.check(&documents, None).expect_err("unserved language");
        assert_eq!(error.code(), "invalid-language");
        assert!(
            matches!(&error, ProviderError::Lpp(lpp) if lpp.kind == LppErrorKind::InvalidLanguage)
        );
    });
}

#[test]
fn missing_capability_is_an_explicit_refusal_not_a_fallback() {
    let Some(path) = mock_provider_path() else {
        return;
    };
    let mut registry = ProviderRegistry::new();
    registry
        .register(ProviderConfig::new(
            DEMO_LANGUAGE_ID,
            path,
            vec!["--without".into(), "compile".into()],
        ))
        .expect("registered");
    let mut provider = registry.spawn(DEMO_LANGUAGE_ID).expect("spawns");
    let result = provider
        .initialize(Some(&ClientInfo {
            name: "wright".to_string(),
            version: "0.2.0".to_string(),
        }))
        .expect("initialize");
    assert!(!result.capabilities.supports(Capability::Compile));
    assert!(result.capabilities.supports(Capability::Check));

    let error = provider
        .compile(&clean_document_set(), None)
        .expect_err("compile not negotiated");
    assert_eq!(error.code(), "capability-unavailable");
    let ProviderError::Lpp(lpp) = &error else {
        panic!("expected typed LPP error")
    };
    assert_eq!(lpp.capability(), Some("compile"));
    assert_eq!(lpp.method(), Some("lpp/compile"));

    provider
        .check(&clean_document_set(), None)
        .expect("check works");
    provider.shutdown().expect("shutdown");
}

#[test]
fn provider_lifecycle_exit_and_drop() {
    if let Some((mut provider, _)) = initialized() {
        provider.shutdown().expect("shutdown");
        assert_eq!(provider.exit_status(), Some(0));
    }
    if mock_provider_path().is_some() {
        let (command, args): (&str, Vec<String>) = if cfg!(windows) {
            ("cmd", vec!["/C".into(), "exit 3".into()])
        } else {
            ("sh", vec!["-c".into(), "exit 3".into()])
        };
        let mut p = StdioLanguageProvider::spawn(
            Path::new(command),
            &args,
            std::time::Duration::from_secs(5),
        )
        .expect("spawns");
        let client_info = ClientInfo {
            name: "wright".into(),
            version: "0.2.0".into(),
        };
        let err = p
            .initialize(Some(&client_info))
            .expect_err("provider exited");
        assert_eq!(err.code(), "provider-exited");
        assert_eq!(p.exit_status(), Some(3));
    }
    let missing = if cfg!(windows) {
        PathBuf::from("Z:\\definitely\\missing\\lpp-provider.exe")
    } else {
        PathBuf::from("/definitely/missing/lpp-provider")
    };
    let error = StdioLanguageProvider::spawn(&missing, &[], std::time::Duration::from_secs(5))
        .err()
        .expect("spawn fails");
    assert_eq!(error.code(), "provider-spawn");
    assert!(matches!(error, ProviderError::Spawn { .. }));

    if let Some((provider, _)) = initialized() {
        drop(provider);
    }
}
