//! LPP integration through the driver session and tool service (#142).
//!
//! These tests prove the consumer seam: a session/tool holds an LPP provider
//! client for an opaque language id and consumes negotiated capabilities
//! through the transport-neutral `LanguageProvider` trait. They require the
//! LPP conformance mock provider binary (see
//! `crates/wright-lpp/tests/mock_provider.rs` for build instructions and the
//! pinned `language-provider-protocol` commit); the `LPP_MOCK_PROVIDER`
//! environment variable names it, and CI sets it. Without the variable the
//! tests are skipped with a clear reason.

#![allow(clippy::result_large_err)]

use std::path::{Path, PathBuf};

use wright_driver::config::{InputSpec, SessionConfig, SourceKind};
use wright_driver::service::ToolService;
use wright_driver::{CompilerSession, Profile};
use wright_lpp::{ClientInfo, Document, DocumentSet, ProviderError};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn mock_provider_path() -> Option<PathBuf> {
    match std::env::var("LPP_MOCK_PROVIDER") {
        Ok(path) if !path.is_empty() => Some(PathBuf::from(path)),
        _ => {
            eprintln!(
                "SKIPPED: LPP_MOCK_PROVIDER is not set; build the LPP mock provider from the \
                 pinned language-provider-protocol commit and point LPP_MOCK_PROVIDER at it \
                 (see crates/wright-lpp/tests/mock_provider.rs)"
            );
            None
        }
    }
}

fn demo_documents() -> DocumentSet {
    let mut documents = DocumentSet::new();
    documents.insert(
        "file:///project/puzzle.xdl".to_string(),
        Document {
            uri: "file:///project/puzzle.xdl".to_string(),
            language_id: "x-demo-lang".to_string(),
            version: 1,
            text: "puzzle clean {\n  target = 40\n  start = 10\n  ops {\n    double: x => x * 2\n  }\n  solution = [ double, double ]\n}"
                .to_string(),
        },
    );
    documents
}

#[test]
fn session_spawns_initializes_and_queries_a_provider() {
    let Some(path) = mock_provider_path() else {
        return;
    };
    let mut registry = wright_lpp::ProviderRegistry::new();
    registry
        .register(wright_lpp::ProviderConfig::new(
            "x-demo-lang",
            path,
            Vec::new(),
        ))
        .expect("registered");
    let config = SessionConfig {
        providers: registry,
        ..SessionConfig::default()
    };
    let session = CompilerSession::new(config).expect("session");
    let mut provider = session.language_provider("x-demo-lang").expect("provider");

    let result = provider
        .initialize(Some(&ClientInfo {
            name: "wright".to_string(),
            version: "0.2.0".to_string(),
        }))
        .expect("initialize");
    assert_eq!(result.protocol_version, "1.0");
    assert_eq!(result.languages[0].id, "x-demo-lang");

    let checked = provider.check(&demo_documents(), None).expect("check");
    assert!(checked.documents[0].diagnostics.is_empty());
    provider.shutdown().expect("shutdown");
    assert_eq!(provider.exit_status(), Some(0));
}

#[test]
fn unconfigured_language_refuses_explicitly() {
    // "opy" here is an unconfigured opaque language id — there is no
    // OPY-specific branch in the client; the refusal is the registry's.
    let session = CompilerSession::new(SessionConfig::default()).expect("session");
    let error = session
        .language_provider("opy")
        .err()
        .expect("not configured");
    assert_eq!(error.code(), "provider-not-configured");
    assert_eq!(
        error,
        ProviderError::NotConfigured {
            language_id: "opy".to_string(),
        }
    );
}

#[test]
fn tool_service_exposes_the_provider_seam() {
    let Some(path) = mock_provider_path() else {
        return;
    };
    let mut registry = wright_lpp::ProviderRegistry::new();
    registry
        .register(wright_lpp::ProviderConfig::new(
            "x-demo-lang",
            path,
            Vec::new(),
        ))
        .expect("registered");
    let config = SessionConfig {
        input: InputSpec::Path(
            workspace_root().join("compatibility/fixtures/synthetic/basic-rule/source.opy"),
        ),
        kind: SourceKind::Opy,
        profile: Profile::Compat,
        providers: registry,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::new(config).expect("session");
    let _ = session.load().expect("loads");
    let session = Box::leak(Box::new(session));
    let service = ToolService::new(session).expect("service");

    let mut provider = service.language_provider("x-demo-lang").expect("provider");
    let result = provider
        .initialize(Some(&ClientInfo {
            name: "wright".to_string(),
            version: "0.2.0".to_string(),
        }))
        .expect("initialize");
    let negotiated = provider.capabilities().expect("negotiated");
    assert!(negotiated.supports(wright_lpp::Capability::Check));
    assert_eq!(result.languages[0].extensions, vec!["xdl"]);
    provider.shutdown().expect("shutdown");
}
