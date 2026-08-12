//! Session-aware tool service tests (#57/#58): the M9 service exposes the
//! same operations in-process that the stdio/JSON-RPC adapters serve, with
//! deterministic structured results, capability negotiation, cost
//! inspection, and target metadata.

use std::path::{Path, PathBuf};

use wright_driver::config::{InputSpec, SessionConfig, SourceKind};
use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::{CompilerSession, Profile};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn service_for(id: &str) -> ToolService<'static> {
    let path = workspace_root()
        .join("compatibility/fixtures")
        .join(id)
        .join("source.opy");
    let config = SessionConfig {
        input: InputSpec::Path(path),
        kind: SourceKind::Opy,
        profile: Profile::Compat,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::new(config).unwrap();
    let _ = session.load().unwrap();
    let session = Box::leak(Box::new(session));
    ToolService::new(session).unwrap()
}

fn handle_ok(service: &ToolService<'_>, request: &ToolRequest) -> serde_json::Value {
    match service.handle(request) {
        ToolResponse::Ok { result } => result,
        ToolResponse::Error { error } => panic!("request failed: {error:?}"),
    }
}

#[test]
fn capabilities_negotiate_version_and_operations() {
    let service = service_for("synthetic/basic-rule");
    let capabilities = handle_ok(&service, &ToolRequest::Capabilities);
    assert_eq!(capabilities["contract"], "wright-result/v1");
    assert_eq!(capabilities["name"], "wright-tool-service");
    assert!(capabilities["operations"].as_array().unwrap().len() >= 10);
    assert!(
        capabilities["languages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|language| language == "opy")
    );
}

#[test]
fn project_reports_origin_and_input_identity() {
    let service = service_for("synthetic/control-flow");
    let project = handle_ok(&service, &ToolRequest::Project);
    assert_eq!(project["origin"]["kind"], "opy");
    assert_eq!(project["rules"], 2);
    assert_eq!(project["inputIdentity"].as_str().unwrap().len(), 64);
    assert!(
        project["findings"].as_u64().unwrap() >= 1,
        "min-wait-loop finding"
    );
}

#[test]
fn call_graph_lists_caller_to_callee_edges() {
    let service = service_for("synthetic/declarations-rules");
    let graph = handle_ok(&service, &ToolRequest::CallGraph);
    let edges = graph.as_array().unwrap();
    assert!(
        edges
            .iter()
            .any(|edge| edge["caller"] == "player starts" && edge["callee"] == "showStatus"),
        "call graph: {edges:?}"
    );
}

#[test]
fn cost_estimate_distinguishes_exact_counts_from_findings() {
    let service = service_for("synthetic/control-flow");
    let cost = handle_ok(&service, &ToolRequest::CostEstimate);
    let exact = &cost["exact"];
    assert!(exact["emittedBytes"].as_u64().unwrap() > 0);
    assert!(exact["wirActions"].as_u64().unwrap() >= 1);
    assert!(
        exact["waitActions"].as_u64().unwrap() >= 1,
        "wait() present"
    );
    assert!(
        cost["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["code"] == "min-wait-loop"),
        "static findings are separate from exact counts"
    );
}

#[test]
fn target_metadata_reports_catalog_surface() {
    let service = service_for("synthetic/basic-rule");
    let metadata = handle_ok(&service, &ToolRequest::TargetMetadata);
    assert!(metadata["actions"].as_u64().unwrap() > 0);
    assert!(metadata["values"].as_u64().unwrap() > 0);
    assert!(metadata["enumDomains"].as_array().unwrap().len() > 0);
    assert!(
        metadata["locales"]
            .as_array()
            .unwrap()
            .iter()
            .any(|locale| locale == "en-us")
    );
}

#[test]
fn semantic_queries_are_deterministic() {
    let first = handle_ok(
        &service_for("synthetic/declarations-rules"),
        &ToolRequest::Findings,
    );
    let second = handle_ok(
        &service_for("synthetic/declarations-rules"),
        &ToolRequest::Findings,
    );
    assert_eq!(first, second, "tool service output is deterministic");
}

#[test]
fn invalid_rule_id_returns_structured_error() {
    let service = service_for("synthetic/basic-rule");
    match service.handle(&ToolRequest::Cfg { rule: 99 }) {
        ToolResponse::Ok { .. } => panic!("expected an error for an invalid rule id"),
        ToolResponse::Error { error } => assert_eq!(error.code, "invalid-id"),
    }
}
