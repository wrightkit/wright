#![allow(clippy::result_large_err)]

use std::path::{Path, PathBuf};

use wright_driver::CompilerSession;
use wright_driver::config::{InputSpec, SessionConfig, SourceKind};
use wright_driver::service::ToolService;
use wright_lpp::{ClientInfo, Document, DocumentSet, ProviderError};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn workshop_path() -> PathBuf {
    workspace_root().join("tests/fixtures/workshop/synthetic/control-flow.ws")
}

fn mock_provider_path() -> PathBuf {
    let path = std::env::var("LPP_MOCK_PROVIDER")
        .expect("set LPP_MOCK_PROVIDER to the pinned LPP mock-provider binary");
    assert!(!path.is_empty(), "LPP_MOCK_PROVIDER must not be empty");
    PathBuf::from(path)
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
#[ignore = "requires LPP_MOCK_PROVIDER pointing to the pinned LPP mock provider"]
fn session_spawns_initializes_and_queries_a_provider() {
    let path = mock_provider_path();
    let mut registry = wright_lpp::ProviderRegistry::new();
    registry
        .register(wright_lpp::ProviderConfig::new(
            "x-demo-lang",
            path,
            Vec::new(),
        ))
        .expect("registered");
    let config = SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
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
    let session = CompilerSession::new(SessionConfig::default()).expect("session");
    let error = session
        .language_provider("x-unconfigured-lang")
        .err()
        .expect("not configured");
    assert_eq!(error.code(), "provider-not-configured");
    assert_eq!(
        error,
        ProviderError::NotConfigured {
            language_id: "x-unconfigured-lang".to_string(),
        }
    );
}

#[test]
fn missing_first_party_opy_provider_is_a_structured_local_failure() {
    let config = SessionConfig {
        opy_provider: wright_driver::OpyProviderConfig::with_executable(
            workspace_root().join("does-not-exist/opy-provider"),
        ),
        ..SessionConfig::default()
    };
    let session = CompilerSession::new(config).expect("session");
    let error = session
        .language_provider("opy")
        .err()
        .expect("missing provider must be visible");
    assert_eq!(error.code(), "provider-missing");
    assert!(matches!(
        error,
        ProviderError::Local {
            kind: wright_lpp::LocalProviderErrorKind::Missing,
            ..
        }
    ));
}

#[test]
#[ignore = "requires LPP_MOCK_PROVIDER pointing to the pinned LPP mock provider"]
fn tool_service_exposes_the_provider_seam() {
    let path = mock_provider_path();
    let mut registry = wright_lpp::ProviderRegistry::new();
    registry
        .register(wright_lpp::ProviderConfig::new(
            "x-demo-lang",
            path,
            Vec::new(),
        ))
        .expect("registered");
    let config = SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        providers: registry,
        ..SessionConfig::default()
    };
    let session = CompilerSession::new(config).expect("session");
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
