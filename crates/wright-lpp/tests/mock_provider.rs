//! End-to-end LPP client tests against the reference conformance mock
//! provider from the `language-provider-protocol` repository.
//!
//! The mock provider serves the deliberately foreign reference language
//! `x-demo-lang`, so every assertion below is also evidence that the client
//! has no source-language-specific protocol logic: the language id is an
//! opaque key, and a document tagged with an unserved language id is refused
//! by the provider through the normal `invalidLanguage` error path.
//!
//! The provider binary is located through the `LPP_MOCK_PROVIDER`
//! environment variable. When the variable is absent the tests are skipped
//! with a clear reason; CI sets it (see `.github/workflows/ci.yml`) so the
//! suite is REQUIRED there. Build the provider from the pinned
//! `language-provider-protocol` commit with:
//!
//! ```text
//! git clone https://github.com/wrightkit/language-provider-protocol
//! git -C language-provider-protocol checkout 416b293e26e6fb2d29061608a493a7aecd2ce14f
//! cargo build -p lpp-mock-provider
//! ```
//!
//! then run:
//!
//! ```text
//! LPP_MOCK_PROVIDER=language-provider-protocol/target/debug/lpp-mock-provider \
//!   cargo test -p wright-lpp --test mock_provider
//! ```

#![allow(clippy::result_large_err)]

use std::path::{Path, PathBuf};

use wright_lpp::{
    Capability, ClientInfo, Document, DocumentSet, LanguageProvider, LppErrorKind, Position,
    ProviderConfig, ProviderError, ProviderRegistry, RegistryError, StdioLanguageProvider,
    TextEdit, WorkshopArtifact,
};

/// The pinned language-provider-protocol commit the tests were written and
/// validated against.
const PINNED_LPP_COMMIT: &str = "416b293e26e6fb2d29061608a493a7aecd2ce14f";

/// The reference mock provider's deliberately foreign language id.
const DEMO_LANGUAGE_ID: &str = "x-demo-lang";

/// The mock provider's artifact format for compiled puzzles.
const DEMO_ARTIFACT_FORMAT: &str = "x-demo/puzzle-eval-v1";

/// A clean x-demo-lang puzzle document (from the LPP v1 spec transcript).
const CLEAN_PUZZLE: &str = "puzzle clean {\n  target = 40\n  start = 10\n  ops {\n    double: x => x * 2\n    plus1: x => x + 1\n  }\n  solution = [ double, double ]\n}";

/// A puzzle with an unresolved op reference.
const BROKEN_PUZZLE: &str = "puzzle broken {\n  target = 40\n  start = 10\n  ops {\n    double: x => x * 2\n  }\n  solution = [ triple ]\n}";

/// The mock provider binary path from the environment, skipping the test
/// with a clear reason when absent.
fn mock_provider_path() -> Option<PathBuf> {
    match std::env::var("LPP_MOCK_PROVIDER") {
        Ok(path) if !path.is_empty() => Some(PathBuf::from(path)),
        _ => {
            eprintln!(
                "SKIPPED: LPP_MOCK_PROVIDER is not set; build the LPP mock provider \
                 from the pinned language-provider-protocol commit {PINNED_LPP_COMMIT} \
                 (cargo build -p lpp-mock-provider) and point LPP_MOCK_PROVIDER at it."
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

fn clean_document_set() -> DocumentSet {
    let mut documents = DocumentSet::new();
    documents.insert(
        "file:///project/puzzle.xdl".to_string(),
        demo_document("file:///project/puzzle.xdl", CLEAN_PUZZLE),
    );
    documents
}

fn provider() -> StdioLanguageProvider {
    let path = mock_provider_path().expect("mock provider path");
    StdioLanguageProvider::spawn(&path, &[], std::time::Duration::from_secs(30))
        .expect("provider spawns")
}

fn initialize() -> (StdioLanguageProvider, wright_lpp::InitializeResult) {
    let mut provider = provider();
    let result = provider
        .initialize(Some(&ClientInfo {
            name: "wright".to_string(),
            version: "0.2.0".to_string(),
        }))
        .expect("initialize succeeds");
    (provider, result)
}

// ---------------------------------------------------------------------------
// Initialize / handshake / capability negotiation
// ---------------------------------------------------------------------------

#[test]
fn initializes_and_negotiates_capabilities_with_x_demo_lang() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let (mut provider, result) = initialize();
    assert_eq!(result.protocol_version, "1.0");
    assert_eq!(result.server_info.name, "lpp-mock-provider");
    assert_eq!(result.languages.len(), 1);
    assert_eq!(result.languages[0].id, DEMO_LANGUAGE_ID);
    assert_eq!(result.languages[0].extensions, vec!["xdl"]);
    // The reference language is deliberately foreign: the opaque language
    // id is served exactly as declared.
    let negotiated = provider.capabilities().expect("negotiated");
    assert_eq!(negotiated.language_ids(), vec![DEMO_LANGUAGE_ID]);
    for capability in Capability::ALL {
        if capability == Capability::ProjectLoading {
            continue;
        }
        assert!(
            negotiated.supports(capability),
            "capability {} negotiated",
            capability.as_str()
        );
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
    // A second registration for the same id refuses.
    assert_eq!(
        registry
            .register(ProviderConfig::new(
                DEMO_LANGUAGE_ID,
                PathBuf::from("ignored"),
                Vec::new(),
            ))
            .expect_err("duplicate"),
        RegistryError::DuplicateLanguage {
            language_id: DEMO_LANGUAGE_ID.to_string(),
        }
    );
    // A configured id spawns a session (dropped below; the provider is
    // terminated on drop).
    drop(registry.spawn(DEMO_LANGUAGE_ID).expect("spawns"));
    // An unconfigured language id refuses explicitly; there is no fallback.
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

// ---------------------------------------------------------------------------
// Document operations through the full session
// ---------------------------------------------------------------------------

#[test]
fn check_reports_clean_and_broken_documents() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let (mut provider, _) = initialize();

    let clean = clean_document_set();
    let checked = provider
        .check(&clean, Some("file:///project"))
        .expect("check");
    assert_eq!(checked.documents.len(), 1);
    assert!(checked.documents[0].diagnostics.is_empty(), "clean puzzle");

    let mut broken = DocumentSet::new();
    broken.insert(
        "file:///project/broken.xdl".to_string(),
        demo_document("file:///project/broken.xdl", BROKEN_PUZZLE),
    );
    let checked = provider.check(&broken, None).expect("check");
    let diagnostics = &checked.documents[0].diagnostics;
    assert_eq!(diagnostics.len(), 1, "one unresolved op reference");
    assert_eq!(
        diagnostics[0].severity,
        wright_lpp::DiagnosticSeverity::Error
    );
    assert_eq!(diagnostics[0].code.as_deref(), Some("x-demo/unresolved-op"));
    assert_eq!(diagnostics[0].source.as_deref(), Some(DEMO_LANGUAGE_ID));
    provider.shutdown().expect("shutdown");
}

#[test]
fn compile_produces_an_opaque_artifact_and_refuses_on_errors() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let (mut provider, _) = initialize();

    let compiled = provider
        .compile(&clean_document_set(), Some("file:///project"))
        .expect("compile");
    let artifact = compiled.artifact.expect("clean puzzle compiles");
    assert_eq!(artifact.format, DEMO_ARTIFACT_FORMAT);
    let content: serde_json::Value =
        serde_json::from_str(&artifact.content).expect("artifact content is the provider's own");
    assert_eq!(content["name"], "clean");
    assert_eq!(content["target"], 40);

    let mut broken = DocumentSet::new();
    broken.insert(
        "file:///project/broken.xdl".to_string(),
        demo_document("file:///project/broken.xdl", BROKEN_PUZZLE),
    );
    let compiled = provider.compile(&broken, None).expect("compile");
    assert!(
        compiled.artifact.is_none(),
        "artifact must be null on error diagnostics"
    );
    assert_eq!(
        compiled.diagnostics[0].diagnostics[0].code.as_deref(),
        Some("x-demo/unresolved-op")
    );

    // Compiling more than one document is refused by the provider with a
    // structured refusal, which the client surfaces as a refusal.
    let mut multi = clean_document_set();
    multi.insert(
        "file:///project/second.xdl".to_string(),
        demo_document("file:///project/second.xdl", CLEAN_PUZZLE),
    );
    let error = provider
        .compile(&multi, None)
        .expect_err("multi-document refusal");
    assert_eq!(error.code(), "refusal");
    assert_eq!(error.refusal_code(), Some("compile.requiresSingleDocument"));
    provider.shutdown().expect("shutdown");
}

#[test]
fn reconstruct_roundtrips_an_artifact_and_refuses_unknown_formats() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let (mut provider, _) = initialize();

    let compiled = provider
        .compile(&clean_document_set(), None)
        .expect("compile")
        .artifact
        .expect("artifact");
    let reconstructed = provider.reconstruct(&compiled).expect("reconstruct");
    assert!(reconstructed.source.contains("puzzle clean"));

    let unsupported = provider
        .reconstruct(&WorkshopArtifact {
            format: "other/format".to_string(),
            content: "{}".to_string(),
        })
        .expect_err("unsupported format");
    assert_eq!(unsupported.code(), "refusal");
    assert_eq!(
        unsupported.refusal_code(),
        Some("reconstruct.artifactFormatUnsupported")
    );

    let malformed = provider
        .reconstruct(&WorkshopArtifact {
            format: DEMO_ARTIFACT_FORMAT.to_string(),
            content: "not a puzzle sheet".to_string(),
        })
        .expect_err("malformed content");
    assert_eq!(malformed.code(), "invalid-artifact");
    assert!(matches!(
        &malformed,
        ProviderError::Lpp(lpp) if lpp.kind == LppErrorKind::InvalidArtifact
    ));
    provider.shutdown().expect("shutdown");
}

#[test]
fn symbols_definition_and_references_resolve_across_the_document() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let (mut provider, _) = initialize();
    let documents = clean_document_set();

    let symbols = provider.symbols(&documents, None).expect("symbols");
    let names: Vec<(&str, &str)> = symbols.documents[0]
        .symbols
        .iter()
        .map(|symbol| (symbol.name.as_str(), symbol.kind.as_str()))
        .collect();
    assert_eq!(
        names,
        vec![("clean", "puzzle"), ("double", "op"), ("plus1", "op")],
        "declaration order, provider-defined kinds"
    );

    // Position inside the `double` op name (0-based line 4, char 6).
    let position = Position {
        line: 4,
        character: 6,
    };
    let definition = provider
        .definition(&documents["file:///project/puzzle.xdl"], position)
        .expect("definition");
    assert_eq!(definition.locations.len(), 1);
    assert_eq!(definition.locations[0].range.start.character, 4);

    let references = provider
        .references(&documents["file:///project/puzzle.xdl"], position, true)
        .expect("references");
    let spans: Vec<(u32, u32)> = references
        .locations
        .iter()
        .map(|location| (location.range.start.line, location.range.start.character))
        .collect();
    assert_eq!(
        spans,
        vec![(4, 4), (7, 15), (7, 23)],
        "declaration first, then sorted references"
    );

    // No symbol at a position: a structured refusal, session stays healthy.
    let error = provider
        .definition(
            &documents["file:///project/puzzle.xdl"],
            Position {
                line: 1,
                character: 0,
            },
        )
        .expect_err("no symbol at position");
    assert_eq!(error.refusal_code(), Some("definition.noSymbolAtPosition"));

    // Position outside the document: invalidPosition LPP error.
    let error = provider
        .definition(
            &documents["file:///project/puzzle.xdl"],
            Position {
                line: 99,
                character: 0,
            },
        )
        .expect_err("position outside document");
    assert_eq!(error.code(), "invalid-position");
    provider.shutdown().expect("shutdown");
}

#[test]
fn rename_computes_source_edits_and_refuses_invalid_names_and_collisions() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let (mut provider, _) = initialize();
    let documents = clean_document_set();
    let uri = "file:///project/puzzle.xdl";

    let renamed = provider
        .rename(
            &documents,
            uri,
            Position {
                line: 4,
                character: 6,
            },
            "twice",
            None,
        )
        .expect("rename");
    assert_eq!(renamed.edits.len(), 1);
    let edits = &renamed.edits[0];
    assert_eq!(edits.document_uri, uri);
    assert_eq!(edits.version, 3);
    assert_eq!(
        edits.text_edits.len(),
        3,
        "declaration plus both references"
    );
    assert!(edits.text_edits.iter().all(|edit| edit.new_text == "twice"));

    let invalid = provider
        .rename(
            &documents,
            uri,
            Position {
                line: 4,
                character: 6,
            },
            "not a name!",
            None,
        )
        .expect_err("invalid name");
    assert_eq!(invalid.refusal_code(), Some("rename.invalidName"));

    let collision = provider
        .rename(
            &documents,
            uri,
            Position {
                line: 4,
                character: 6,
            },
            "plus1",
            None,
        )
        .expect_err("collision");
    assert_eq!(collision.refusal_code(), Some("rename.nameCollision"));
    provider.shutdown().expect("shutdown");
}

#[test]
fn validate_edits_applies_the_normative_rules() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let (mut provider, _) = initialize();
    let document = demo_document("file:///project/puzzle.xdl", CLEAN_PUZZLE);

    let renamed = provider
        .rename(
            &clean_document_set(),
            "file:///project/puzzle.xdl",
            Position {
                line: 4,
                character: 6,
            },
            "twice",
            None,
        )
        .expect("rename");
    let edits: Vec<TextEdit> = renamed.edits[0].text_edits.clone();
    let validated = provider
        .validate_edits(&document, &edits)
        .expect("validation");
    assert!(validated.valid, "the rename edits apply cleanly");
    assert_eq!(validated.version, 3);

    let overlapping = provider
        .validate_edits(&document, &[edits[0].clone(), edits[0].clone()])
        .expect("validation");
    assert!(!overlapping.valid);
    assert_eq!(overlapping.reason.as_deref(), Some("overlappingEdits"));
    assert_eq!(overlapping.failing_edit_index, Some(1));
    provider.shutdown().expect("shutdown");
}

#[test]
fn unserved_language_id_is_refused_by_the_provider() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let (mut provider, _) = initialize();
    let mut documents = DocumentSet::new();
    documents.insert(
        "file:///project/other.xdl".to_string(),
        Document {
            uri: "file:///project/other.xdl".to_string(),
            language_id: "x-other-lang".to_string(),
            version: 1,
            text: "whatever".to_string(),
        },
    );
    let error = provider
        .check(&documents, None)
        .expect_err("unserved language");
    assert_eq!(error.code(), "invalid-language");
    assert!(matches!(
        &error,
        ProviderError::Lpp(lpp) if lpp.kind == LppErrorKind::InvalidLanguage
    ));
    provider.shutdown().expect("shutdown");
}

// ---------------------------------------------------------------------------
// Capability negotiation and explicit refusal
// ---------------------------------------------------------------------------

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
            vec!["--without".to_string(), "compile".to_string()],
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
        panic!("expected a typed LPP error");
    };
    assert_eq!(lpp.capability(), Some("compile"));
    assert_eq!(lpp.method(), Some("lpp/compile"));

    // The session is healthy: an available capability still works.
    provider
        .check(&clean_document_set(), None)
        .expect("check works");
    provider.shutdown().expect("shutdown");
}

// ---------------------------------------------------------------------------
// Process lifecycle and failure handling
// ---------------------------------------------------------------------------

#[test]
fn graceful_shutdown_exits_with_status_zero() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let mut provider = provider();
    provider.initialize(None).expect("initialize");
    provider.shutdown().expect("shutdown");
    assert_eq!(provider.exit_status(), Some(0));
}

#[test]
fn provider_crash_fails_requests_with_an_exit_status() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let (command, args): (&str, Vec<String>) = if cfg!(windows) {
        ("cmd", vec!["/C".to_string(), "exit 3".to_string()])
    } else {
        ("sh", vec!["-c".to_string(), "exit 3".to_string()])
    };
    let mut provider =
        StdioLanguageProvider::spawn(Path::new(command), &args, std::time::Duration::from_secs(5))
            .expect("spawns");
    let error = provider
        .initialize(Some(&ClientInfo {
            name: "wright".to_string(),
            version: "0.2.0".to_string(),
        }))
        .expect_err("provider exited");
    assert_eq!(error.code(), "provider-exited");
    assert_eq!(provider.exit_status(), Some(3));
}

#[test]
fn spawn_failure_is_deterministic() {
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
}

#[test]
fn provider_without_shutdown_is_cleaned_up_on_drop() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    // Dropping an initialized provider (without shutdown) must not leave the
    // process running: the child is terminated on drop. The test completes
    // without hanging, and the graceful path is covered by
    // `graceful_shutdown_exits_with_status_zero`.
    let (provider, _) = initialize();
    drop(provider);
}
