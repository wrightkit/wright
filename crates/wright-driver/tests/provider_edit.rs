//! Provider-driven mutation end-to-end tests (#139) through the tool
//! service and Wright's LPP client against the reference conformance mock
//! provider.
//!
//! These tests prove the #139 seam end to end: semantic rename target
//! resolution and edit generation route through the LPP `rename` capability,
//! the resulting source edits are wrapped in Wright's own edit transaction
//! (identity/version preconditions, deterministic ordering, overlap checks,
//! atomic preview), and semantic validation routes through the provider's
//! project semantics (`lpp/validateEdits` per edited document, then
//! `lpp/check` over the edited project). Provider refusals, unsupported
//! capabilities, stale sources, and semantic validation failures must all be
//! structured refusals with no partial edit set.
//!
//! The mock provider serves the deliberately foreign reference language
//! `x-demo-lang`, so these tests also prove the seam has no OPY/DEL-specific
//! logic. The provider binary is located through the `LPP_MOCK_PROVIDER`
//! environment variable; when absent the mock-dependent tests are skipped
//! with a clear reason (CI sets it; see `.github/workflows/ci.yml` and
//! `crates/wright-lpp/tests/mock_provider.rs` for the pinned commit).

#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wright_driver::CompilerSession;
use wright_driver::config::{InputSpec, SessionConfig, SourceKind};
use wright_driver::edit::{EditRange, EditTransaction, SourceEdit};
use wright_driver::provider_edit::ProviderMutation;
use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_lpp::{Document, DocumentSet, Position};

/// The reference mock provider's deliberately foreign language id.
const DEMO_LANGUAGE_ID: &str = "x-demo-lang";

/// A clean x-demo-lang puzzle document (from the LPP v1 spec transcript).
const CLEAN_PUZZLE: &str = "puzzle clean {\n  target = 40\n  start = 10\n  ops {\n    double: x => x * 2\n    plus1: x => x + 1\n  }\n  solution = [ double, double ]\n}";

/// A puzzle declaring only the `double` op.
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

/// A tool session with the given provider registry and no unrelated source
/// project to load.
fn session_with_providers(providers: wright_lpp::ProviderRegistry) -> CompilerSession {
    let config = SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        providers,
        ..SessionConfig::default()
    };
    CompilerSession::new(config).expect("session")
}

/// A tool service over a session with the mock provider registered (with
/// optional `--without` capability flags).
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

/// The caller's current text view for a document set (identical texts).
fn sources_of(documents: &DocumentSet) -> BTreeMap<String, String> {
    documents
        .iter()
        .map(|(uri, document)| (uri.clone(), document.text.clone()))
        .collect()
}

/// The rename request the way an agent would send it through the tool
/// service.
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

// ---------------------------------------------------------------------------
// Semantic rename through the provider
// ---------------------------------------------------------------------------

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

    // Wright-owned transaction: exact-range rename edits carrying the
    // identity precondition of the text they were computed against,
    // deterministically ordered (declaration first, then references).
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

    // Atomic preview: the complete edited text, all references renamed.
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

    // Atomic preview covers every edited source.
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
    assert_eq!(mutation.diagnostics[0].code, "provider-refusal");
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
    // The caller's current text no longer matches the snapshot the provider
    // computed the rename against: the Wright-owned identity precondition
    // refuses before anything can be applied.
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

// ---------------------------------------------------------------------------
// Capability and failure refusals
// ---------------------------------------------------------------------------

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
    assert_eq!(mutation.diagnostics[0].code, "provider-error");
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
    // The rename capability is available but the edit-validation capability
    // is not: the provider computes the rename, then the mandatory semantic
    // gate refuses. The failure happens after edit generation, so this
    // proves nothing partial is ever returned.
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
    assert_eq!(mutation.diagnostics[0].code, "provider-error");
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
    // The source session itself refuses before any static frontend can be
    // selected; provider edit requests therefore cannot inherit a fallback.
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

// ---------------------------------------------------------------------------
// Caller-proposed transaction validation through the provider
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Provider operations are independent of the session project (#512)
// ---------------------------------------------------------------------------

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
    // The configured session input does not exist; the service still
    // starts, without a snapshot.
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
    // Neither flow triggered the unrelated session-project load.
    assert!(service.loaded().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Session-derived document sets (#548)
// ---------------------------------------------------------------------------

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

/// A tool service over a loaded Workshop session with the session-docs fake
/// provider registered, plus the file the provider records the shown
/// document URIs into.
#[cfg(unix)]
fn session_docs_service() -> (PathBuf, PathBuf, ToolService<'static>) {
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
    (dir, capture, ToolService::new(session).expect("service"))
}

/// The session member's identity in the defaulted document set: the
/// input's `file://` URI.
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
    let (dir, capture, mut service) = session_docs_service();
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
    // The provider was shown exactly the session's member set at version 0.
    let shown = std::fs::read_to_string(&capture).unwrap();
    assert_eq!(shown.trim(), member_uri);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The same defaulting serves `providerValidateEdit`: a transaction naming
/// a session member by its `file://` URI applies against the derived set.
#[cfg(unix)]
#[test]
fn an_omitted_document_set_validates_a_session_member_edit() {
    let (dir, _capture, mut service) = session_docs_service();
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
    let (dir, _capture, mut service) = session_docs_service();
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
