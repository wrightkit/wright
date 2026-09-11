//! Session-aware tool service tests (#57/#58): the service exposes the
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
        capabilities["operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|operation| operation == "persistentObjects")
    );
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
fn call_graph_is_empty_for_owner_supported_program() {
    let service = service_for("synthetic/control-flow");
    let graph = handle_ok(&service, &ToolRequest::CallGraph);
    let edges = graph.as_array().unwrap();
    assert!(edges.is_empty(), "call graph: {edges:?}");
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
    assert!(!metadata["enumDomains"].as_array().unwrap().is_empty());
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
        &service_for("synthetic/control-flow"),
        &ToolRequest::Findings,
    );
    let second = handle_ok(
        &service_for("synthetic/control-flow"),
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

#[test]
fn findings_and_lint_requests_resolve_span_paths() {
    // The tool/agent surfaces resolve finding spans exactly like the CLI
    // workflows (#102): `Findings` and `Lint` responses carry a non-empty,
    // root-relative `path` on every finding, and both surfaces agree.
    let service = service_for("synthetic/control-flow");
    let findings = handle_ok(&service, &ToolRequest::Findings);
    let findings_list = findings.as_array().unwrap();
    assert!(!findings_list.is_empty(), "control-flow produces findings");
    for finding in findings_list {
        let path = finding["span"]["path"].as_str().unwrap_or_default();
        assert!(!path.is_empty(), "Findings spans carry a resolved path");
        assert_eq!(
            path, "source.opy",
            "the path is the root-relative file name"
        );
    }
    let lint = handle_ok(&service, &ToolRequest::Lint);
    let lint_findings = lint["findings"].as_array().unwrap();
    assert_eq!(lint_findings.len(), findings_list.len());
    for (found, linted) in findings_list.iter().zip(lint_findings) {
        assert_eq!(
            found["span"]["path"], linted["span"]["path"],
            "Findings and Lint must agree on span.path"
        );
    }
}

#[test]
fn persistent_object_queries_resolve_span_paths_without_lint_diagnostics() {
    let service = service_for("synthetic/control-flow");
    let objects = handle_ok(&service, &ToolRequest::PersistentObjects);
    let objects = objects.as_array().unwrap();
    assert!(!objects.is_empty());
    assert_eq!(objects[0]["reevaluation"]["domain"], "HudReeval");
    assert!(objects[0]["sameKindCleanupInRule"].is_boolean());
    assert_eq!(objects[0]["span"]["path"], "source.opy");
    let findings = handle_ok(&service, &ToolRequest::Findings);
    assert!(
        findings
            .as_array()
            .unwrap()
            .iter()
            .all(|finding| finding["code"] != "persistent-object-lifecycle")
    );
}

fn workshop_service() -> ToolService<'static> {
    let oracle = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(
            workspace_root()
                .join("compatibility/fixtures/synthetic/declarations-rules/oracle.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let text = oracle["compile"]["workshop"].as_str().unwrap().to_string();
    let path = std::env::temp_dir().join(format!(
        "wright-service-workshop-{}-{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, text).unwrap();
    let config = SessionConfig {
        input: InputSpec::Path(path),
        kind: SourceKind::Workshop,
        profile: Profile::Compat,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::new(config).unwrap();
    let _ = session.load().unwrap();
    let session = Box::leak(Box::new(session));
    ToolService::new(session).unwrap()
}

/// The same shared-service assertions across input kinds: every language
/// surfaces a non-trivial program, rules, symbols, findings, and lint rule
/// metadata through the identical in-process queries (#120 acceptance 5).
fn assert_shared_service_surface(service: &ToolService<'_>) {
    let project = handle_ok(service, &ToolRequest::Project);
    assert!(
        project["rules"].as_u64().unwrap() >= 1,
        "rules surface: {project}"
    );
    assert!(project["symbols"].as_u64().unwrap() > 0, "symbols surface");

    let rules = handle_ok(service, &ToolRequest::Rules);
    assert!(rules.as_array().is_some_and(|r| !r.is_empty()));

    let symbols = handle_ok(service, &ToolRequest::Symbols { kind: None });
    assert!(symbols.as_array().is_some_and(|s| !s.is_empty()));

    let findings = handle_ok(service, &ToolRequest::Findings);
    assert!(findings.as_array().is_some());

    let lint = handle_ok(service, &ToolRequest::Lint);
    assert!(
        lint["rules"].as_array().is_some_and(|r| !r.is_empty()),
        "lint returns shared rule metadata"
    );
}

#[test]
fn cross_language_shared_service_behavior_is_frontend_neutral() {
    // #120 acceptance 5: the same shared semantic-service assertions hold
    // over OPY and Workshop inputs — no language-specific stack.
    assert_shared_service_surface(&service_for("synthetic/control-flow"));
    assert_shared_service_surface(&workshop_service());
}

// -- #130: validated mutation through the shared tool API ----------------------

fn corpus_source(id: &str) -> String {
    std::fs::read_to_string(
        workspace_root()
            .join("compatibility/fixtures")
            .join(id)
            .join("source.opy"),
    )
    .unwrap()
}

#[test]
fn tool_service_validates_and_previews_a_source_edit_transaction() {
    // #130: agents request edit validation/preview through the shared tool
    // API; the response carries the source identities, ranges, previews, and
    // structured diagnostics for safe caller-side application.
    let service = service_for("synthetic/declarations-numbers");
    let main = service.loaded().input.display.clone();
    let source = corpus_source("synthetic/declarations-numbers");
    let identity = wright_driver::input_identity(&source);
    let line_count = source.lines().count().max(1) as u32;
    let end_col = source
        .lines()
        .last()
        .map(|line| line.chars().count() as u32 + 1)
        .unwrap_or(1);
    let transaction =
        wright_driver::edit::EditTransaction::new(vec![wright_driver::edit::SourceEdit {
            edit_kind: "rename".to_string(),
            source: main.clone(),
            source_identity: identity,
            range: wright_driver::edit::EditRange {
                start_line: 1,
                start_col: 1,
                end_line: line_count,
                end_col,
            },
            new_text: source.replace("j", "total"),
        }])
        .unwrap();
    let response = service.handle(&ToolRequest::ValidateEdit {
        sources: std::collections::BTreeMap::from([(main, source)]),
        transaction,
    });
    let ToolResponse::Ok { result } = response else {
        panic!("validateEditTransaction must return a structured result");
    };
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["diagnostics"].as_array().unwrap().len(), 0);
    let previews = result["preview"].as_array().unwrap();
    assert_eq!(previews.len(), 1, "one affected source previewed");
    assert!(
        previews[0]["new_text"]
            .as_str()
            .unwrap()
            .contains("globalvar total"),
        "the preview carries the edited text"
    );
    assert!(
        previews[0]["source_identity"].as_str().unwrap().len() == 64,
        "the preview carries the new-source identity"
    );
}

#[test]
fn tool_service_semantic_rename_returns_the_validated_transaction() {
    // #130: semantic rename through the shared tool API returns the same
    // exact-range transaction an in-process consumer gets, with no partial
    // result on refusal.
    let service = service_for("synthetic/declarations-numbers");
    let main = service.loaded().input.display.clone();
    let source = corpus_source("synthetic/declarations-numbers");
    let sources = std::collections::BTreeMap::from([(main.clone(), source)]);
    let response = service.handle(&ToolRequest::SemanticRename {
        sources,
        target: wright_driver::edit::RenameTarget {
            source: main,
            line: 1,
            col: 11,
            to: "total".to_string(),
        },
    });
    let ToolResponse::Ok { result } = response else {
        panic!("semanticRename must return a structured result");
    };
    assert_eq!(result["ok"], true, "{result}");
    let transaction = result["transaction"]
        .as_object()
        .expect("transaction present");
    let edits = transaction["edits"].as_array().unwrap();
    assert!(
        edits.iter().all(|edit| edit["new_text"] == "total"),
        "every edit is an exact occurrence replacement"
    );
    let previews = result["preview"].as_array().unwrap();
    assert_eq!(previews.len(), 1);
    assert!(
        previews[0]["new_text"]
            .as_str()
            .unwrap()
            .contains("globalvar total")
    );

    // Unsupported/unsafe requests return structured refusals, never a
    // partially applicable edit set.
    let service = service_for("synthetic/declarations-numbers");
    let main = service.loaded().input.display.clone();
    let source = corpus_source("synthetic/declarations-numbers");
    let response = service.handle(&ToolRequest::SemanticRename {
        sources: std::collections::BTreeMap::from([(main.clone(), source)]),
        target: wright_driver::edit::RenameTarget {
            source: main,
            line: 1,
            col: 11,
            to: "h".to_string(),
        },
    });
    let ToolResponse::Ok { result } = response else {
        panic!("refusals are structured results");
    };
    assert_eq!(result["ok"], false);
    assert_eq!(result["transaction"], serde_json::Value::Null);
    assert!(
        result["diagnostics"][0]["code"] == "rename-collision",
        "the refusal names the cause: {result}"
    );
}

#[test]
fn tool_service_mutation_capabilities_advertise_the_support_boundary() {
    let service = service_for("synthetic/control-flow");
    let capabilities = handle_ok(&service, &ToolRequest::Capabilities);
    let operations = capabilities["operations"].as_array().unwrap();
    assert!(
        operations.iter().any(|op| op == "validateEditTransaction"),
        "validateEditTransaction advertised: {operations:?}"
    );
    assert!(
        operations.iter().any(|op| op == "semanticRename"),
        "semanticRename advertised: {operations:?}"
    );
}
