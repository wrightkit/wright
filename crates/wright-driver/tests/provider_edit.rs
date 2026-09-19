//! Provider-driven mutation end-to-end tests (#139) through the tool
//! service and Wright's LPP client against the reference conformance mock
//! provider.

#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wright_driver::CompilerSession;
use wright_driver::config::{InputSpec, SessionConfig, SourceKind};
use wright_driver::edit::{EditRange, EditTransaction, SourceEdit};
use wright_driver::provider_edit::ProviderMutation;
use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_lpp::{Document, DocumentSet, Position};

const DEMO_LANGUAGE_ID: &str = "x-demo-lang";
const CLEAN_PUZZLE: &str = "puzzle clean {\n  target = 40\n  start = 10\n  ops {\n    double: x => x * 2\n    plus1: x => x + 1\n  }\n  solution = [ double, double ]\n}";
const SINGLE_OP_PUZZLE: &str = "puzzle clean {\n  target = 40\n  start = 10\n  ops {\n    double: x => x * 2\n  }\n  solution = [ double, double ]\n}";
const URI: &str = "file:///project/puzzle.xdl";
const URI_SECOND: &str = "file:///project/puzzle2.xdl";
const DOUBLE_POSITION: Position = Position {
    line: 4,
    character: 6,
};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn mock_provider_path() -> Option<PathBuf> {
    match std::env::var("LPP_MOCK_PROVIDER") {
        Ok(path) if !path.is_empty() => Some(PathBuf::from(path)),
        _ => None,
    }
}

fn tool_service(without: &[&str]) -> ToolService<'static> {
    let path = mock_provider_path().expect("mock provider path");
    let mut registry = wright_lpp::ProviderRegistry::new();
    let args: Vec<String> = without
        .iter()
        .flat_map(|c| ["--without".to_string(), (*c).to_string()])
        .collect();
    registry
        .register(wright_lpp::ProviderConfig::new(
            DEMO_LANGUAGE_ID,
            path,
            args,
        ))
        .expect("registered");
    let session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(
            workspace_root().join("compatibility/fixtures/synthetic/control-flow/workshop.ws"),
        ),
        kind: SourceKind::Workshop,
        providers: registry,
        ..SessionConfig::default()
    })
    .expect("session");
    let session = Box::leak(Box::new(session));
    ToolService::new(session).expect("service")
}

fn demo_document(uri: &str, text: &str, version: i64) -> Document {
    Document {
        uri: uri.to_string(),
        language_id: DEMO_LANGUAGE_ID.to_string(),
        version,
        text: text.to_string(),
    }
}

fn single_document_set() -> DocumentSet {
    DocumentSet::from([(URI.to_string(), demo_document(URI, CLEAN_PUZZLE, 3))])
}

fn two_document_set() -> DocumentSet {
    DocumentSet::from([
        (URI.to_string(), demo_document(URI, CLEAN_PUZZLE, 3)),
        (
            URI_SECOND.to_string(),
            demo_document(URI_SECOND, CLEAN_PUZZLE, 5),
        ),
    ])
}

fn sources_of(documents: &DocumentSet) -> BTreeMap<String, String> {
    documents
        .iter()
        .map(|(uri, doc)| (uri.clone(), doc.text.clone()))
        .collect()
}

fn rename_request(
    documents: DocumentSet,
    uri: &str,
    new_name: &str,
    sources: BTreeMap<String, String>,
) -> ToolRequest {
    ToolRequest::ProviderSemanticRename {
        language_id: DEMO_LANGUAGE_ID.to_string(),
        documents,
        position_document_uri: uri.to_string(),
        position: DOUBLE_POSITION,
        new_name: new_name.to_string(),
        project_root: Some("file:///project".to_string()),
        sources,
    }
}

fn handle(service: &ToolService<'_>, request: &ToolRequest) -> ProviderMutation {
    match service.handle(request) {
        ToolResponse::Ok { result } => {
            serde_json::from_value(result).expect("provider mutation deserializes")
        }
        ToolResponse::Error { error } => panic!("tool request failed: {error:?}"),
    }
}

fn assert_refusal(mutation: &ProviderMutation, code: &str, provider_code: Option<&str>) {
    assert!(!mutation.ok);
    assert_eq!(mutation.diagnostics[0].code, code);
    if let Some(pc) = provider_code {
        assert_eq!(mutation.provider_code.as_deref(), Some(pc));
    }
    assert!(mutation.transaction.is_none(), "no partial edit set");
    assert!(mutation.preview.is_none(), "no partial preview");
}

fn make_edit(line: u32, start_col: u32, end_col: u32, new_text: &str) -> SourceEdit {
    SourceEdit {
        edit_kind: "rename".to_string(),
        source: URI.to_string(),
        source_identity: wright_driver::input_identity(CLEAN_PUZZLE),
        range: EditRange {
            start_line: line,
            start_col,
            end_line: line,
            end_col,
        },
        new_text: new_text.to_string(),
    }
}

fn test_service(without: &[&str]) -> Option<ToolService<'static>> {
    mock_provider_path().map(|_| tool_service(without))
}

fn single_rename(new_name: &str) -> ToolRequest {
    rename_request(
        single_document_set(),
        URI,
        new_name,
        sources_of(&single_document_set()),
    )
}

#[test]
fn single_file_rename_routes_through_the_provider() {
    let Some(service) = test_service(&[]) else {
        return;
    };
    let docs = single_document_set();
    let mutation = handle(
        &service,
        &rename_request(docs, URI, "twice", sources_of(&single_document_set())),
    );
    assert!(mutation.ok, "rename succeeds: {:?}", mutation.diagnostics);

    let transaction = mutation.transaction.expect("transaction");
    assert_eq!(
        transaction.edits.len(),
        3,
        "declaration plus both references"
    );
    assert!(
        transaction
            .edits
            .iter()
            .all(|edit| edit.edit_kind == "rename"
                && edit.source == URI
                && edit.source_identity == wright_driver::input_identity(CLEAN_PUZZLE))
    );

    let preview = mutation.preview.expect("preview");
    assert_eq!(preview.len(), 1);
    assert_eq!(preview[0].source, URI);
    assert!(preview[0].new_text.contains("twice: x => x * 2"));
    assert!(preview[0].new_text.contains("solution = [ twice, twice ]"));
    assert!(!preview[0].new_text.contains("double"));
}

#[test]
fn multi_file_rename_edits_every_received_document_consistently() {
    let Some(service) = test_service(&[]) else {
        return;
    };
    let docs = two_document_set();
    let mutation = handle(
        &service,
        &rename_request(docs, URI, "twice", sources_of(&two_document_set())),
    );
    assert!(mutation.ok, "rename succeeds: {:?}", mutation.diagnostics);

    let transaction = mutation.transaction.expect("transaction");
    let per_source: BTreeMap<&str, usize> =
        transaction
            .edits
            .iter()
            .fold(BTreeMap::new(), |mut counts, edit| {
                *counts.entry(edit.source.as_str()).or_default() += 1;
                counts
            });
    assert_eq!(per_source.get(URI), Some(&3));
    assert_eq!(per_source.get(URI_SECOND), Some(&3));

    let preview = mutation.preview.expect("preview");
    assert_eq!(preview.len(), 2);
    for p in &preview {
        assert!(p.new_text.contains("twice: x => x * 2"));
        assert!(p.new_text.contains("solution = [ twice, twice ]"));
    }
}

#[test]
fn rename_refusals_and_capability_guards() {
    let Some(service) = test_service(&[]) else {
        return;
    };
    let mutation = handle(&service, &single_rename("plus1"));
    assert_refusal(&mutation, "provider-refusal", Some("rename.nameCollision"));

    let mutation = handle(&service, &single_rename("not a name!"));
    assert_refusal(&mutation, "provider-refusal", Some("rename.invalidName"));

    let docs = single_document_set();
    let mut stale = sources_of(&docs);
    stale.insert(URI.to_string(), format!("{CLEAN_PUZZLE}\n"));
    let mutation = handle(&service, &rename_request(docs, URI, "twice", stale));
    assert_refusal(&mutation, "edit-stale-source", None);

    for cap in ["rename", "editValidation"] {
        let Some(service) = test_service(&[cap]) else {
            return;
        };
        let mutation = handle(&service, &single_rename("twice"));
        assert_refusal(&mutation, "provider-error", Some("capability-unavailable"));
    }
}

#[test]
fn caller_transaction_validation_and_refusals() {
    let Some(service) = test_service(&[]) else {
        return;
    };
    let transaction = EditTransaction::new(vec![
        make_edit(5, 5, 11, "twice"),
        make_edit(8, 16, 22, "twice"),
        make_edit(8, 24, 30, "twice"),
    ])
    .expect("transaction");
    let mutation = handle(
        &service,
        &ToolRequest::ProviderValidateEdit {
            language_id: DEMO_LANGUAGE_ID.to_string(),
            documents: single_document_set(),
            transaction,
            sources: sources_of(&single_document_set()),
            project_root: Some("file:///project".to_string()),
        },
    );
    assert!(
        mutation.ok,
        "provider accepts transaction: {:?}",
        mutation.diagnostics
    );
    let preview = mutation.preview.expect("preview");
    assert!(preview[0].new_text.contains("twice: x => x * 2"));
    assert!(preview[0].new_text.contains("solution = [ twice, twice ]"));

    let transaction = EditTransaction::new(vec![SourceEdit {
        edit_kind: "edit".to_string(),
        source: URI.to_string(),
        source_identity: wright_driver::input_identity(CLEAN_PUZZLE),
        range: EditRange {
            start_line: 2,
            start_col: 12,
            end_line: 2,
            end_col: 14,
        },
        new_text: String::new(),
    }])
    .expect("transaction");
    let mutation = handle(
        &service,
        &ToolRequest::ProviderValidateEdit {
            language_id: DEMO_LANGUAGE_ID.to_string(),
            documents: single_document_set(),
            transaction,
            sources: sources_of(&single_document_set()),
            project_root: None,
        },
    );
    assert_refusal(&mutation, "provider-validation-failed", None);

    let docs = DocumentSet::from([
        (URI.to_string(), demo_document(URI, SINGLE_OP_PUZZLE, 3)),
        (
            URI_SECOND.to_string(),
            demo_document(URI_SECOND, CLEAN_PUZZLE, 5),
        ),
    ]);
    let mutation = handle(
        &service,
        &rename_request(docs.clone(), URI, "plus1", sources_of(&docs)),
    );
    assert_refusal(&mutation, "provider-validation-failed", None);
}
