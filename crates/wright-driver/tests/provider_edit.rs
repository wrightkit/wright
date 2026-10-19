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

/// A clean x-demo-lang puzzle document (from the LPP v1 spec transcript).
const CLEAN_PUZZLE: &str = "puzzle clean {\n  target = 40\n  start = 10\n  ops {\n    double: x => x * 2\n    plus1: x => x + 1\n  }\n  solution = [ double, double ]\n}";

const SINGLE_OP_PUZZLE: &str = "puzzle clean {\n  target = 40\n  start = 10\n  ops {\n    double: x => x * 2\n  }\n  solution = [ double, double ]\n}";

const URI: &str = "file:///project/puzzle.xdl";
const URI_SECOND: &str = "file:///project/puzzle2.xdl";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn workshop_path() -> PathBuf {
    workspace_root().join("tests/fixtures/workshop/synthetic/control-flow.ws")
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

fn session_with_providers(providers: wright_lpp::ProviderRegistry) -> CompilerSession {
    let config = SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        providers,
        ..SessionConfig::default()
    };
    CompilerSession::new(config).expect("session")
}

fn tool_service(without: &[&str]) -> ToolService<'static> {
    let path = mock_provider_path().expect("mock provider path");
    let mut registry = wright_lpp::ProviderRegistry::new();
    let mut args = Vec::new();
    for capability in without {
        args.push("--without".to_string());
        args.push((*capability).to_string());
    }
    registry
        .register(wright_lpp::ProviderConfig::new(
            DEMO_LANGUAGE_ID,
            path,
            args,
        ))
        .expect("registered");
    let session = session_with_providers(registry);
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
    let mut documents = DocumentSet::new();
    documents.insert(URI.to_string(), demo_document(URI, CLEAN_PUZZLE, 3));
    documents
}

fn two_document_set() -> DocumentSet {
    let mut documents = DocumentSet::new();
    documents.insert(URI.to_string(), demo_document(URI, CLEAN_PUZZLE, 3));
    documents.insert(
        URI_SECOND.to_string(),
        demo_document(URI_SECOND, CLEAN_PUZZLE, 5),
    );
    documents
}

fn sources_of(documents: &DocumentSet) -> BTreeMap<String, String> {
    documents
        .iter()
        .map(|(uri, document)| (uri.clone(), document.text.clone()))
        .collect()
}

fn rename_request(
    documents: DocumentSet,
    position_document_uri: &str,
    position: Position,
    new_name: &str,
    sources: BTreeMap<String, String>,
) -> ToolRequest {
    ToolRequest::ProviderSemanticRename {
        language_id: DEMO_LANGUAGE_ID.to_string(),
        documents: Some(documents),
        position_document_uri: position_document_uri.to_string(),
        position,
        new_name: new_name.to_string(),
        project_root: Some("file:///project".to_string()),
        sources: Some(sources),
    }
}

/// The position of the `double` op declaration (0-based line 4, char 6).
const DOUBLE_POSITION: Position = Position {
    line: 4,
    character: 6,
};

fn handle(service: &mut ToolService<'_>, request: &ToolRequest) -> ProviderMutation {
    match service.handle(request) {
        ToolResponse::Ok { result } => {
            serde_json::from_value(result).expect("provider mutation deserializes")
        }
        ToolResponse::Error { error } => panic!("tool request failed: {error:?}"),
    }
}

#[test]
fn single_file_rename_routes_through_the_provider() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let mut service = tool_service(&[]);
    let documents = single_document_set();
    let mutation = handle(
        &mut service,
        &rename_request(
            documents,
            URI,
            DOUBLE_POSITION,
            "twice",
            sources_of(&single_document_set()),
        ),
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
            .all(|edit| edit.edit_kind == "rename")
    );
    assert!(
        transaction.edits.iter().all(|edit| {
            edit.source == URI
                && edit.source_identity == wright_driver::input_identity(CLEAN_PUZZLE)
        }),
        "every edit carries the source identity precondition"
    );

    let preview = mutation.preview.expect("preview");
    assert_eq!(preview.len(), 1);
    assert_eq!(preview[0].source, URI);
    assert!(preview[0].new_text.contains("twice: x => x * 2"));
    assert!(preview[0].new_text.contains("solution = [ twice, twice ]"));
    assert!(
        !preview[0].new_text.contains("double"),
        "every occurrence renamed"
    );
}

#[test]
fn multi_file_rename_edits_every_received_document_consistently() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let mut service = tool_service(&[]);
    let documents = two_document_set();
    let mutation = handle(
        &mut service,
        &rename_request(
            documents,
            URI,
            DOUBLE_POSITION,
            "twice",
            sources_of(&two_document_set()),
        ),
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
    assert_eq!(
        per_source.get(URI),
        Some(&3),
        "first document: declaration plus references"
    );
    assert_eq!(
        per_source.get(URI_SECOND),
        Some(&3),
        "second document: declaration plus references"
    );

    let preview = mutation.preview.expect("preview");
    assert_eq!(preview.len(), 2);
    for source_preview in &preview {
        assert!(source_preview.new_text.contains("twice: x => x * 2"));
        assert!(
            source_preview
                .new_text
                .contains("solution = [ twice, twice ]")
        );
    }
}

#[test]
fn rename_collision_is_a_structured_atomic_refusal() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let mut service = tool_service(&[]);
    let documents = single_document_set();
    let mutation = handle(
        &mut service,
        &rename_request(
            documents,
            URI,
            DOUBLE_POSITION,
            "plus1", // already declared: the provider refuses the collision
            sources_of(&single_document_set()),
        ),
    );
    assert!(!mutation.ok);
    // #570: a deliberate refusal is a request outcome (frontend), not a
    // provider process failure — the refusalCode rides in the diagnostic
    // code under the `provider-refusal-` namespace and verbatim in
    // `provider_code`.
    assert_eq!(
        mutation.diagnostics[0].code,
        "provider-refusal-rename.nameCollision"
    );
    assert_eq!(
        mutation.provider_code.as_deref(),
        Some("rename.nameCollision"),
        "the provider's machine-readable refusal code is preserved"
    );
    assert!(mutation.transaction.is_none(), "no partial edit set");
    assert!(mutation.preview.is_none(), "no partial preview");
}

#[test]
fn invalid_new_name_is_a_structured_atomic_refusal() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let mut service = tool_service(&[]);
    let documents = single_document_set();
    let mutation = handle(
        &mut service,
        &rename_request(
            documents,
            URI,
            DOUBLE_POSITION,
            "not a name!",
            sources_of(&single_document_set()),
        ),
    );
    assert!(!mutation.ok);
    assert_eq!(
        mutation.provider_code.as_deref(),
        Some("rename.invalidName")
    );
    assert!(mutation.transaction.is_none());
    assert!(mutation.preview.is_none());
}

#[test]
fn stale_current_sources_refuse_with_no_partial_edit_set() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let mut service = tool_service(&[]);
    let documents = single_document_set();
    let mut stale_sources = sources_of(&documents);
    stale_sources.insert(URI.to_string(), format!("{CLEAN_PUZZLE}\n"));
    let mutation = handle(
        &mut service,
        &rename_request(documents, URI, DOUBLE_POSITION, "twice", stale_sources),
    );
    assert!(!mutation.ok);
    assert_eq!(mutation.diagnostics[0].code, "edit-stale-source");
    assert!(mutation.transaction.is_none(), "no partial edit set");
    assert!(mutation.preview.is_none(), "no partial preview");
}

#[test]
fn missing_rename_capability_refuses_explicitly() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let mut service = tool_service(&["rename"]);
    let documents = single_document_set();
    let mutation = handle(
        &mut service,
        &rename_request(
            documents,
            URI,
            DOUBLE_POSITION,
            "twice",
            sources_of(&single_document_set()),
        ),
    );
    assert!(!mutation.ok);
    assert_eq!(mutation.diagnostics[0].code, "capability-unavailable");
    assert_eq!(
        mutation.provider_code.as_deref(),
        Some("capability-unavailable"),
        "no silent fallback to textual search/replace"
    );
    assert!(mutation.transaction.is_none());
    assert!(mutation.preview.is_none());
}

#[test]
fn provider_failure_mid_rename_applies_nothing() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let mut service = tool_service(&["editValidation"]);
    let documents = single_document_set();
    let mutation = handle(
        &mut service,
        &rename_request(
            documents,
            URI,
            DOUBLE_POSITION,
            "twice",
            sources_of(&single_document_set()),
        ),
    );
    assert!(!mutation.ok);
    assert_eq!(mutation.diagnostics[0].code, "capability-unavailable");
    assert_eq!(
        mutation.provider_code.as_deref(),
        Some("capability-unavailable"),
        "the missing validation capability refuses after edit generation"
    );
    assert!(mutation.transaction.is_none(), "no partial edit set");
    assert!(mutation.preview.is_none(), "no partial preview");
}

#[test]
fn semantic_validation_failure_is_atomic_across_documents() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let mut service = tool_service(&[]);
    // Document 1 declares only `double`; document 2 declares `double` and
    // `plus1`. Renaming `double` to `plus1` in document 1 passes the
    // provider's rename-time collision check (which is scoped to the
    // position document) but creates a duplicate `plus1` op in document 2.
    // The provider's project-aware semantic validation of the edit set
    // catches it: the whole mutation refuses with no partial application.
    let mut documents = DocumentSet::new();
    documents.insert(URI.to_string(), demo_document(URI, SINGLE_OP_PUZZLE, 3));
    documents.insert(
        URI_SECOND.to_string(),
        demo_document(URI_SECOND, CLEAN_PUZZLE, 5),
    );
    let sources = sources_of(&documents);
    let mutation = handle(
        &mut service,
        &rename_request(documents, URI, DOUBLE_POSITION, "plus1", sources),
    );
    assert!(!mutation.ok);
    assert_eq!(mutation.diagnostics[0].code, "provider-validation-failed");
    assert!(mutation.transaction.is_none(), "no partial edit set");
    assert!(mutation.preview.is_none(), "no partial preview");
}

#[test]
fn unconfigured_language_id_refuses_explicitly() {
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workspace_root().join("tests/fixtures/opy/basic-rule.opy")),
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    })
    .expect("session");
    let diagnostic = match session.load() {
        Ok(_) => panic!("missing source provider must refuse"),
        Err(diagnostic) => diagnostic,
    };
    assert_eq!(diagnostic.code, "source-provider-unavailable");
}

/// A Wright transaction renaming every occurrence of `double` to `twice` in
/// the clean puzzle (declaration at 5:5-5:11, references at 8:16-8:22 and
/// 8:24-8:30, 1-based columns).
fn rename_all_occurrences_transaction() -> EditTransaction {
    let source_identity = wright_driver::input_identity(CLEAN_PUZZLE);
    EditTransaction::new(vec![
        SourceEdit {
            edit_kind: "rename".to_string(),
            source: URI.to_string(),
            source_identity: source_identity.clone(),
            range: EditRange {
                start_line: 5,
                start_col: 5,
                end_line: 5,
                end_col: 11,
            },
            new_text: "twice".to_string(),
        },
        SourceEdit {
            edit_kind: "rename".to_string(),
            source: URI.to_string(),
            source_identity: source_identity.clone(),
            range: EditRange {
                start_line: 8,
                start_col: 16,
                end_line: 8,
                end_col: 22,
            },
            new_text: "twice".to_string(),
        },
        SourceEdit {
            edit_kind: "rename".to_string(),
            source: URI.to_string(),
            source_identity,
            range: EditRange {
                start_line: 8,
                start_col: 24,
                end_line: 8,
                end_col: 30,
            },
            new_text: "twice".to_string(),
        },
    ])
    .expect("transaction")
}

#[test]
fn caller_transaction_validates_through_the_provider() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let mut service = tool_service(&[]);
    let documents = single_document_set();
    let request = ToolRequest::ProviderValidateEdit {
        language_id: DEMO_LANGUAGE_ID.to_string(),
        documents: Some(documents),
        transaction: rename_all_occurrences_transaction(),
        sources: Some(sources_of(&single_document_set())),
        project_root: Some("file:///project".to_string()),
    };
    let mutation = handle(&mut service, &request);
    assert!(
        mutation.ok,
        "provider accepts the caller transaction: {:?}",
        mutation.diagnostics
    );
    let preview = mutation.preview.expect("preview");
    assert!(preview[0].new_text.contains("twice: x => x * 2"));
    assert!(preview[0].new_text.contains("solution = [ twice, twice ]"));
}

#[test]
fn caller_transaction_that_breaks_the_source_refuses() {
    let Some(_) = mock_provider_path() else {
        return;
    };
    let mut service = tool_service(&[]);
    let documents = single_document_set();
    // Deleting the target value produces a syntax error; the provider's
    // normative edit validation (apply, then re-parse) catches it.
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
    let request = ToolRequest::ProviderValidateEdit {
        language_id: DEMO_LANGUAGE_ID.to_string(),
        documents: Some(documents),
        transaction,
        sources: Some(sources_of(&single_document_set())),
        project_root: None,
    };
    let mutation = handle(&mut service, &request);
    assert!(!mutation.ok);
    assert_eq!(mutation.diagnostics[0].code, "provider-validation-failed");
    assert!(mutation.transaction.is_none());
    assert!(mutation.preview.is_none(), "no partial preview");
}

/// Provider mutations carry their own documents and project context: an
/// unrelated broken session project neither blocks them nor gets loaded by
/// them.
#[test]
fn provider_mutations_do_not_require_the_session_project() {
    let Some(provider) = mock_provider_path() else {
        return;
    };
    let mut registry = wright_lpp::ProviderRegistry::new();
    registry
        .register(wright_lpp::ProviderConfig::new(
            DEMO_LANGUAGE_ID,
            provider,
            Vec::new(),
        ))
        .expect("registered");
    let dir = std::env::temp_dir().join(format!("wright-provider-512-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(dir.join("missing.ws")),
        kind: SourceKind::Workshop,
        providers: registry,
        ..SessionConfig::default()
    })
    .expect("session");
    let session = Box::leak(Box::new(session));
    let mut service = ToolService::new(session).expect("service starts without a snapshot");
    assert!(service.loaded().is_none());

    let rename = handle(
        &mut service,
        &rename_request(
            single_document_set(),
            URI,
            DOUBLE_POSITION,
            "twice",
            sources_of(&single_document_set()),
        ),
    );
    assert!(
        rename.ok,
        "the provider rename succeeds over its own documents: {:?}",
        rename.diagnostics
    );
    let validate = handle(
        &mut service,
        &ToolRequest::ProviderValidateEdit {
            language_id: DEMO_LANGUAGE_ID.to_string(),
            documents: Some(single_document_set()),
            transaction: rename_all_occurrences_transaction(),
            sources: Some(sources_of(&single_document_set())),
            project_root: None,
        },
    );
    assert!(
        validate.ok,
        "the provider validation succeeds over its own documents: {:?}",
        validate.diagnostics
    );
    assert!(service.loaded().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// An LPP fake provider that accepts the mutation surface unconditionally:
/// `lpp/rename` answers one five-character edit on the first document,
/// `lpp/validateEdits` accepts every edit, `lpp/check` is clean. It records
/// every document URI it is shown so the test can prove which set the
/// session supplied.
#[cfg(unix)]
const SESSION_DOCS_PROVIDER: &str = r#"#!/usr/bin/env python3
import json, sys

CAPTURE = "@CAPTURE@"

def reply(id, result=None, error=None):
    message = {"jsonrpc": "2.0", "id": id}
    message.update({"error": error} if error else {"result": result})
    print(json.dumps(message), flush=True)

for line in sys.stdin:
    request = json.loads(line)
    id, method = request["id"], request["method"]
    if method == "lpp/initialize":
        reply(id, {"protocolVersion": "1.0", "serverInfo": {"name": "fake", "version": "0"},
                   "languages": [{"id": "x-demo-lang", "extensions": ["xdl"]}],
                   "capabilities": {"check": True, "compile": True, "reconstruct": False,
                                    "symbols": False, "definition": False, "references": False,
                                    "rename": True, "editValidation": True,
                                    "projectLoading": False, "lookup": False}})
    elif method == "lpp/rename":
        documents = request["params"]["documents"]
        with open(CAPTURE, "a") as capture:
            capture.write("\n".join(sorted(documents)) + "\n")
        uri = sorted(documents)[0]
        reply(id, {"edits": [{"documentUri": uri,
                              "version": documents[uri]["version"],
                              "textEdits": [{"range": {"start": {"line": 0, "character": 0},
                                                       "end": {"line": 0, "character": 5}},
                                             "newText": "RENAMED"}]}]})
    elif method == "lpp/validateEdits":
        reply(id, {"valid": True, "version": request["params"]["document"]["version"]})
    elif method == "lpp/check":
        reply(id, {"documents": [{"uri": uri, "version": doc["version"], "diagnostics": []}
                                 for uri, doc in request["params"]["documents"].items()]})
    else:
        reply(id, {})
"#;

#[cfg(unix)]
struct SessionDocs {
    dir: PathBuf,
    capture: PathBuf,
    service: ToolService<'static>,
}

#[cfg(unix)]
fn session_docs_service() -> SessionDocs {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-provider-548-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let capture = dir.join("documents.txt");
    let provider_path = dir.join("fake-provider");
    std::fs::write(
        &provider_path,
        SESSION_DOCS_PROVIDER.replace("@CAPTURE@", &capture.display().to_string()),
    )
    .unwrap();
    std::fs::set_permissions(&provider_path, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut registry = wright_lpp::ProviderRegistry::new();
    registry
        .register(wright_lpp::ProviderConfig::new(
            DEMO_LANGUAGE_ID,
            provider_path,
            Vec::new(),
        ))
        .expect("registered");
    let session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        providers: registry,
        ..SessionConfig::default()
    })
    .expect("session");
    let session = Box::leak(Box::new(session));
    SessionDocs {
        dir,
        capture,
        service: ToolService::new(session).expect("service"),
    }
}

fn member_uri() -> String {
    url::Url::from_file_path(workshop_path())
        .expect("file uri")
        .to_string()
}

/// `documents`/`sources` may be omitted (#548): the operation then derives
/// the provider's document view from the session's loaded project — member
/// disk text at version 0 under `file://` URIs — and `sources` defaults to
/// each document's text.
#[cfg(unix)]
#[test]
fn an_omitted_document_set_derives_from_the_loaded_project() {
    let SessionDocs {
        dir,
        capture,
        mut service,
    } = session_docs_service();
    let member_uri = member_uri();
    let member_text = std::fs::read_to_string(workshop_path()).unwrap();

    let mutation = handle(
        &mut service,
        &ToolRequest::ProviderSemanticRename {
            language_id: DEMO_LANGUAGE_ID.to_string(),
            documents: None,
            position_document_uri: member_uri.clone(),
            position: Position {
                line: 0,
                character: 0,
            },
            new_name: "renamed".to_string(),
            project_root: None,
            sources: None,
        },
    );
    assert!(
        mutation.ok,
        "the session-derived rename succeeds: {:?}",
        mutation.diagnostics
    );
    let transaction = mutation.transaction.expect("transaction");
    assert_eq!(transaction.edits[0].source, member_uri);
    assert_eq!(
        transaction.edits[0].source_identity,
        wright_driver::input_identity(&member_text),
        "`sources` defaulted to the member's disk text"
    );
    let preview = mutation.preview.expect("preview");
    assert!(
        preview[0].new_text.starts_with("RENAMED"),
        "{}",
        preview[0].new_text
    );
    let shown = std::fs::read_to_string(&capture).unwrap();
    assert_eq!(shown.trim(), member_uri);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn an_omitted_document_set_validates_a_session_member_edit() {
    let SessionDocs {
        dir, mut service, ..
    } = session_docs_service();
    let member_text = std::fs::read_to_string(workshop_path()).unwrap();
    let transaction = EditTransaction::new(vec![SourceEdit {
        edit_kind: "edit".to_string(),
        source: member_uri(),
        source_identity: wright_driver::input_identity(&member_text),
        range: EditRange {
            start_line: 1,
            start_col: 1,
            end_line: 1,
            end_col: 6,
        },
        new_text: "RENAMED".to_string(),
    }])
    .expect("transaction");

    let mutation = handle(
        &mut service,
        &ToolRequest::ProviderValidateEdit {
            language_id: DEMO_LANGUAGE_ID.to_string(),
            documents: None,
            transaction,
            sources: None,
            project_root: None,
        },
    );
    assert!(
        mutation.ok,
        "the session-derived validation succeeds: {:?}",
        mutation.diagnostics
    );
    let preview = mutation.preview.expect("preview");
    assert!(
        preview[0].new_text.starts_with("RENAMED"),
        "{}",
        preview[0].new_text
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `sources` alone may be omitted (#548): it defaults to each supplied
/// document's text.
#[cfg(unix)]
#[test]
fn omitted_sources_default_to_each_documents_text() {
    let SessionDocs {
        dir, mut service, ..
    } = session_docs_service();
    let mutation = handle(
        &mut service,
        &ToolRequest::ProviderSemanticRename {
            language_id: DEMO_LANGUAGE_ID.to_string(),
            documents: Some(single_document_set()),
            position_document_uri: URI.to_string(),
            position: DOUBLE_POSITION,
            new_name: "renamed".to_string(),
            project_root: None,
            sources: None,
        },
    );
    assert!(
        mutation.ok,
        "the rename succeeds with defaulted sources: {:?}",
        mutation.diagnostics
    );
    let transaction = mutation.transaction.expect("transaction");
    assert_eq!(
        transaction.edits[0].source_identity,
        wright_driver::input_identity(CLEAN_PUZZLE),
        "`sources` defaulted to the document text"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// #739: `wright rename` on `.opy` input is the provider-owned rename the
/// agent surface exposes, reached through the CLI's `NAME` addressing.
/// This staged `opy` provider serves a canned compile artifact (the
/// session's project load) and a scripted `lpp/symbols` listing plus an
/// ordered queue of `lpp/rename` outcomes, so each test controls which
/// declaration sites the name resolves to and what each site answers.
/// `lpp/validateEdits` accepts and `lpp/check` reports a clean project;
/// every `lpp/rename` call site is recorded to `rename-calls.jsonl`.
#[cfg(unix)]
const OPY_RENAME_PROVIDER: &str = r#"#!/usr/bin/env python3
import json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))

def read(name):
    with open(os.path.join(HERE, name)) as handle:
        return json.load(handle)

def reply(id, result=None, error=None):
    message = {"jsonrpc": "2.0", "id": id}
    message.update({"error": error} if error is not None else {"result": result})
    print(json.dumps(message), flush=True)

def refusal(id, code, text):
    reply(id, error={"code": -32000, "message": text,
                     "data": {"lpp": {"kind": "refusal", "details": {"refusalCode": code}}}})

for line in sys.stdin:
    request = json.loads(line)
    id, method = request["id"], request["method"]
    if method == "lpp/initialize":
        reply(id, {"protocolVersion": request["params"]["protocolVersion"],
                   "serverInfo": {"name": "fake-opy", "version": "0"},
                   "languages": [{"id": "opy", "extensions": ["opy"]}],
                   "capabilities": {"check": True, "compile": True, "reconstruct": False,
                                    "symbols": True, "definition": False, "references": False,
                                    "rename": True, "editValidation": True,
                                    "projectLoading": True, "lookup": False}})
    elif method == "lpp/compile":
        reply(id, {"diagnostics": [], "artifact": read("artifact.json")})
    elif method == "lpp/symbols":
        reply(id, read("symbols.json"))
    elif method == "lpp/rename":
        params = request["params"]
        with open(os.path.join(HERE, "rename-calls.jsonl"), "a") as calls:
            calls.write(json.dumps({"uri": params["positionDocumentUri"],
                                    "position": params["position"],
                                    "newName": params["newName"]}) + "\n")
        queue = read("renames.json")
        state_file = os.path.join(HERE, "rename-state")
        index = int(open(state_file).read().strip()) if os.path.exists(state_file) else 0
        with open(state_file, "w") as state:
            state.write(str(index + 1))
        outcome = queue[index] if index < len(queue) else queue[-1]
        if "refusalCode" in outcome:
            refusal(id, outcome["refusalCode"], outcome.get("message", "the provider declined"))
        else:
            reply(id, outcome["result"])
    elif method == "lpp/validateEdits":
        reply(id, {"valid": True, "version": request["params"]["document"]["version"]})
    elif method == "lpp/check":
        reply(id, {"documents": [{"uri": uri, "version": doc["version"], "diagnostics": []}
                                 for uri, doc in request["params"]["documents"].items()]})
    else:
        reply(id, {})
"#;

/// An OPY project staged on disk next to its scripted provider: the member
/// files `lpp/symbols`/`lpp/rename` report on, the canned compile artifact,
/// and the session wired to the provider under the `opy` language id.
#[cfg(unix)]
struct FakeOpyProject {
    dir: PathBuf,
    entry: PathBuf,
    calls: PathBuf,
    session: CompilerSession,
}

/// The document URI `provider_document_set` keys a member under — the
/// `file://` spelling of its disk path.
#[cfg(unix)]
fn member_file_uri(dir: &Path, name: &str) -> String {
    url::Url::from_file_path(dir.join(name))
        .expect("absolute member path")
        .to_string()
}

/// The `lpp/compile` artifact the load handshake consumes: `None` maps an
/// unmapped `workshop-rs/text-v1` answer (the entry file is the only
/// member), `Some(members)` a `workshop-rs/mapped-text-v1` answer whose
/// file table lists every member URI — the shape `#!include` projects
/// load under.
#[cfg(unix)]
fn compile_artifact(member_uris: Option<Vec<String>>) -> serde_json::Value {
    let text = std::fs::read_to_string(workshop_path()).expect("workshop fixture");
    match member_uris {
        None => serde_json::json!({"format": "workshop-rs/text-v1", "content": text}),
        Some(files) => {
            let catalog = workshop_rs::catalog::Catalog::builtin_ref().expect("catalog");
            let locale = workshop_rs::catalog::Locale::new("en-US");
            let program = workshop_rs::parser::parse_with_context(&text, catalog, &locale, catalog)
                .expect("workshop fixture parses");
            let map = workshop_rs::program::SourceMap::extract(&program);
            let mut artifact: serde_json::Value =
                serde_json::from_str(&workshop_rs::program::MappedText::new(&text, map).to_json())
                    .expect("mapped text serializes");
            artifact["files"] = serde_json::json!(
                files
                    .iter()
                    .map(|uri| { serde_json::json!({ "path": uri }) })
                    .collect::<Vec<_>>()
            );
            artifact["spans"] = serde_json::json!([]);
            serde_json::json!({"format": "workshop-rs/mapped-text-v1", "content": artifact.to_string()})
        }
    }
}

/// One `lpp/symbols` listing entry: a declaration `symbol` for `name` at
/// the zero-based `line`/`character` in `uri`'s document.
#[cfg(unix)]
fn symbol_doc(uri: &str, name: &str, kind: &str, line: u32, character: u32) -> serde_json::Value {
    serde_json::json!({
        "uri": uri,
        "version": 0,
        "symbols": [{
            "name": name,
            "kind": kind,
            "range": {"start": {"line": line, "character": character},
                      "end": {"line": line, "character": character + name.len() as u32}}
        }]
    })
}

/// `lpp/rename` edits: `(uri, line, character)` spans replaced with
/// `new_name` — every span covers `name`'s length on its own line.
#[cfg(unix)]
fn rename_edits(name: &str, new_name: &str, sites: &[(&str, u32, u32)]) -> serde_json::Value {
    let mut per_document: BTreeMap<&str, Vec<serde_json::Value>> = BTreeMap::new();
    for (uri, line, character) in sites {
        per_document
            .entry(uri)
            .or_default()
            .push(serde_json::json!({
                "range": {"start": {"line": line, "character": character},
                          "end": {"line": line, "character": character + name.len() as u32}},
                "newText": new_name
            }));
    }
    serde_json::json!({
        "edits": per_document.iter().map(|(uri, edits)| serde_json::json!({
            "documentUri": uri,
            "version": 0,
            "textEdits": edits
        })).collect::<Vec<_>>()
    })
}

#[cfg(unix)]
impl FakeOpyProject {
    /// Stage one scripted answer file the provider serves (`artifact.json`,
    /// `symbols.json`, `renames.json`). The provider reads them at request
    /// time, so tests stage them once the staged member paths — and their
    /// `file://` URIs — exist.
    fn stage(&self, name: &str, value: serde_json::Value) {
        std::fs::write(self.dir.join(name), value.to_string()).unwrap();
    }

    fn member_file_uri(&self, name: &str) -> String {
        member_file_uri(&self.dir, name)
    }

    fn member_path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

#[cfg(unix)]
fn fake_opy_project(members: &[(&str, &str)]) -> FakeOpyProject {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-opy-rename-739-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text) in members {
        std::fs::write(dir.join(name), text).unwrap();
    }
    let provider_path = dir.join("fake-opy-provider");
    std::fs::write(&provider_path, OPY_RENAME_PROVIDER).unwrap();
    std::fs::set_permissions(&provider_path, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut registry = wright_lpp::ProviderRegistry::new();
    registry
        .register(wright_lpp::ProviderConfig::new(
            wright_driver::opy_provider::OPY_LANGUAGE_ID,
            provider_path,
            Vec::new(),
        ))
        .expect("registered");
    let session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(dir.join("main.opy")),
        kind: SourceKind::Opy,
        source_backend: wright_driver::source_provider::SourceBackend::Provider,
        providers: registry,
        ..SessionConfig::default()
    })
    .expect("session");
    FakeOpyProject {
        calls: dir.join("rename-calls.jsonl"),
        entry: dir.join("main.opy"),
        dir,
        session,
    }
}

/// The `lpp/rename` call sites the provider saw, `(uri, line, character)`.
#[cfg(unix)]
fn rename_calls(calls: &Path) -> Vec<(String, u64, u64)> {
    std::fs::read_to_string(calls)
        .unwrap_or_default()
        .lines()
        .map(|line| {
            let call: serde_json::Value = serde_json::from_str(line).unwrap();
            (
                call["uri"].as_str().unwrap().to_string(),
                call["position"]["line"].as_u64().unwrap(),
                call["position"]["character"].as_u64().unwrap(),
            )
        })
        .collect()
}

/// `helper` is declared at 0-based line 0 char 4 and called at line 4
/// char 4 of `main.opy`; `util.opy` calls it at line 1 char 4.
#[cfg(unix)]
const MAIN_OPY: &str = "def helper():\n    pass\n\nrule \"setup\":\n    helper()\n";

#[cfg(unix)]
const UTIL_OPY: &str = "def util():\n    helper()\n";

#[cfg(unix)]
#[test]
fn opy_rename_resolves_the_name_to_its_declaration_site_and_previews() {
    let mut project = fake_opy_project(&[("main.opy", MAIN_OPY)]);
    let uri = project.member_file_uri("main.opy");
    project.stage("artifact.json", compile_artifact(None));
    project.stage(
        "symbols.json",
        serde_json::json!({"documents": [symbol_doc(&uri, "helper", "def", 0, 4)]}),
    );
    project.stage(
        "renames.json",
        serde_json::json!([{"result": rename_edits("helper", "assist", &[(&uri, 0, 4), (&uri, 4, 4)])}]),
    );
    let rename = project.session.rename("helper", "assist", false);
    assert!(rename.ok, "{:?}", rename.diagnostics);
    assert_eq!(
        rename_calls(&project.calls),
        vec![(uri.clone(), 0, 4)],
        "NAME resolved through `lpp/symbols` to the declaration position"
    );
    let transaction = rename.result.transaction.expect("transaction");
    assert_eq!(transaction.edits.len(), 2);
    assert!(transaction.edits.iter().all(|edit| edit.source == uri));
    let preview = rename.result.preview.expect("preview");
    assert_eq!(preview[0].source, project.entry.display().to_string());
    assert!(preview[0].new_text.contains("def assist():"));
    assert!(preview[0].new_text.contains("    assist()"));
    assert_eq!(
        std::fs::read_to_string(&project.entry).unwrap(),
        MAIN_OPY,
        "a preview run writes nothing"
    );
    let _ = std::fs::remove_dir_all(&project.dir);
}

/// #739: a multi-member project — the shape a `#!include` graph or a
/// directory input loads under a mapped artifact — renames across every
/// member the provider edited; `--write` applies the previews atomically.
#[cfg(unix)]
#[test]
fn opy_rename_writes_every_member_the_provider_edited() {
    let mut project = fake_opy_project(&[("main.opy", MAIN_OPY), ("util.opy", UTIL_OPY)]);
    let main_uri = project.member_file_uri("main.opy");
    let util_uri = project.member_file_uri("util.opy");
    project.stage(
        "artifact.json",
        compile_artifact(Some(vec![main_uri.clone(), util_uri.clone()])),
    );
    project.stage(
        "symbols.json",
        serde_json::json!({"documents": [
            symbol_doc(&main_uri, "helper", "def", 0, 4),
            {"uri": util_uri, "version": 0, "symbols": [{"name": "util", "kind": "def",
                "range": {"start": {"line": 0, "character": 4},
                          "end": {"line": 0, "character": 8}}}]}
        ]}),
    );
    project.stage(
        "renames.json",
        serde_json::json!([{"result": rename_edits("helper", "assist", &[
            (&main_uri, 0, 4),
            (&main_uri, 4, 4),
            (&util_uri, 1, 4),
        ])}]),
    );
    let rename = project.session.rename("helper", "assist", true);
    assert!(rename.ok, "{:?}", rename.diagnostics);
    assert_eq!(rename_calls(&project.calls), vec![(main_uri.clone(), 0, 4)]);
    let transaction = rename.result.transaction.expect("transaction");
    let per_source: BTreeMap<&str, usize> =
        transaction
            .edits
            .iter()
            .fold(BTreeMap::new(), |mut counts, edit| {
                *counts.entry(edit.source.as_str()).or_default() += 1;
                counts
            });
    assert_eq!(per_source.get(main_uri.as_str()), Some(&2));
    assert_eq!(per_source.get(util_uri.as_str()), Some(&1));
    let mut written = rename.result.written.clone();
    written.sort();
    assert_eq!(
        written,
        vec![
            project.member_path("main.opy").display().to_string(),
            project.member_path("util.opy").display().to_string(),
        ],
        "--write reports disk paths, not provider URIs"
    );
    assert_eq!(
        std::fs::read_to_string(project.member_path("main.opy")).unwrap(),
        "def assist():\n    pass\n\nrule \"setup\":\n    assist()\n"
    );
    assert_eq!(
        std::fs::read_to_string(project.member_path("util.opy")).unwrap(),
        "def util():\n    assist()\n"
    );
    let _ = std::fs::remove_dir_all(&project.dir);
}

/// #739: a name the loaded project does not declare refuses
/// `unknown-symbol` without ever calling `lpp/rename`.
#[cfg(unix)]
#[test]
fn opy_rename_unknown_name_never_reaches_the_provider_rename() {
    let mut project = fake_opy_project(&[("main.opy", MAIN_OPY)]);
    let uri = project.member_file_uri("main.opy");
    project.stage("artifact.json", compile_artifact(None));
    project.stage(
        "symbols.json",
        serde_json::json!({"documents": [symbol_doc(&uri, "helper", "def", 0, 4)]}),
    );
    project.stage("renames.json", serde_json::json!([]));
    let rename = project.session.rename("missing", "assist", false);
    assert!(!rename.ok);
    assert_eq!(rename.diagnostics[0].code, "unknown-symbol");
    assert!(
        !project.calls.exists(),
        "no `lpp/rename` is sent for an undeclared name"
    );
    assert!(rename.result.transaction.is_none());
    assert!(rename.result.preview.is_none());
    let _ = std::fs::remove_dir_all(&project.dir);
}

/// #739: two declaration sites spelling the name that `lpp/rename` answers
/// identically — a `subroutine` declaration and its `def` — are one
/// symbol; divergent answers are a real `ambiguous-symbol` refusal naming
/// each candidate's `path:line:col`.
#[cfg(unix)]
#[test]
fn opy_rename_probes_multi_site_names_and_refuses_only_real_ambiguity() {
    // Same answer from both sites: one semantic symbol, one rename.
    let mut project = fake_opy_project(&[("main.opy", MAIN_OPY)]);
    let uri = project.member_file_uri("main.opy");
    project.stage("artifact.json", compile_artifact(None));
    project.stage(
        "symbols.json",
        serde_json::json!({"documents": [{
        "uri": uri, "version": 0,
        "symbols": [
            {"name": "helper", "kind": "subroutine",
             "range": {"start": {"line": 0, "character": 4},
                       "end": {"line": 0, "character": 10}}},
            {"name": "helper", "kind": "def",
             "range": {"start": {"line": 4, "character": 4},
                       "end": {"line": 4, "character": 10}}}
        ]}]}),
    );
    let edits = rename_edits("helper", "assist", &[(&uri, 0, 4), (&uri, 4, 4)]);
    project.stage(
        "renames.json",
        serde_json::json!([{"result": edits}, {"result": edits}]),
    );
    let rename = project.session.rename("helper", "assist", false);
    assert!(rename.ok, "{:?}", rename.diagnostics);
    assert_eq!(rename_calls(&project.calls).len(), 2);
    let _ = std::fs::remove_dir_all(&project.dir);

    // Divergent answers: two distinct symbols share the name.
    let mut project = fake_opy_project(&[("main.opy", MAIN_OPY), ("util.opy", UTIL_OPY)]);
    let main_uri = project.member_file_uri("main.opy");
    let util_uri = project.member_file_uri("util.opy");
    project.stage(
        "artifact.json",
        compile_artifact(Some(vec![main_uri.clone(), util_uri.clone()])),
    );
    project.stage(
        "symbols.json",
        serde_json::json!({"documents": [
            symbol_doc(&main_uri, "helper", "def", 0, 4),
            symbol_doc(&util_uri, "helper", "def", 0, 4)
        ]}),
    );
    project.stage(
        "renames.json",
        serde_json::json!([
            {"result": rename_edits("helper", "assist", &[(&main_uri, 0, 4)])},
            {"result": rename_edits("helper", "assist", &[(&util_uri, 0, 4)])}
        ]),
    );
    let rename = project.session.rename("helper", "assist", true);
    assert!(!rename.ok);
    assert_eq!(rename.diagnostics[0].code, "ambiguous-symbol");
    let message = &rename.diagnostics[0].message;
    for name in ["main.opy", "util.opy"] {
        let path = project.member_path(name).display().to_string();
        assert!(message.contains(&path), "{message}");
    }
    assert!(rename.result.transaction.is_none());
    assert!(rename.result.preview.is_none());
    assert_eq!(
        std::fs::read_to_string(project.member_path("main.opy")).unwrap(),
        MAIN_OPY,
        "an ambiguous rename writes nothing"
    );
    let _ = std::fs::remove_dir_all(&project.dir);
}

/// #739: the provider's own refusal — here `rename.invalidName` — keeps
/// its machine-readable code under the `provider-refusal-` namespace and
/// previews/writes nothing.
#[cfg(unix)]
#[test]
fn opy_rename_passes_provider_refusals_through() {
    let mut project = fake_opy_project(&[("main.opy", MAIN_OPY)]);
    let uri = project.member_file_uri("main.opy");
    project.stage("artifact.json", compile_artifact(None));
    project.stage(
        "symbols.json",
        serde_json::json!({"documents": [symbol_doc(&uri, "helper", "def", 0, 4)]}),
    );
    project.stage(
        "renames.json",
        serde_json::json!([{"refusalCode": "rename.invalidName",
                            "message": "'not a name' is not a valid identifier"}]),
    );
    let rename = project.session.rename("helper", "not a name", true);
    assert!(!rename.ok);
    assert_eq!(
        rename.diagnostics[0].code,
        "provider-refusal-rename.invalidName"
    );
    assert!(rename.result.transaction.is_none());
    assert!(rename.result.preview.is_none());
    assert_eq!(
        std::fs::read_to_string(&project.entry).unwrap(),
        MAIN_OPY,
        "a refused rename writes nothing"
    );
    let _ = std::fs::remove_dir_all(&project.dir);
}
