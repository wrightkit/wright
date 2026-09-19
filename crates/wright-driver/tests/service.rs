//! ToolService preserves the machine contract over canonical Workshop input
//! and reports provider-owned OPY gaps as structured refusals.

use std::path::{Path, PathBuf};

use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceKind};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn workshop_path() -> PathBuf {
    workspace_root().join("tests/fixtures/workshop/synthetic/control-flow.ws")
}

#[test]
fn tool_service_queries_canonical_workshop() {
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let service = ToolService::new(&mut session).unwrap();
    for request in [
        ToolRequest::Capabilities,
        ToolRequest::Project,
        ToolRequest::Rules,
        ToolRequest::Findings,
        ToolRequest::CostEstimate,
    ] {
        assert!(matches!(service.handle(&request), ToolResponse::Ok { .. }));
    }
}

#[test]
fn tool_service_keeps_provider_refusals_structured() {
    let path = workspace_root().join("tests/fixtures/opy/basic-rule.opy");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path),
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    })
    .unwrap();
    let diagnostic = match session.load() {
        Ok(_) => panic!("static OPY fallback must not load"),
        Err(diagnostic) => diagnostic,
    };
    assert_eq!(diagnostic.code, "source-provider-unavailable");
}
