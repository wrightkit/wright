#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wright_driver::CompilerSession;
use wright_driver::config::{InputSpec, SessionConfig, SourceKind};
use wright_driver::edit::{EditRange, EditTransaction, SourceEdit};
use wright_driver::provider_edit::ProviderMutation;
use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_lpp::{Document, DocumentSet};

const DEMO_LANGUAGE_ID: &str = "x-demo-lang";

/// A clean x-demo-lang puzzle document (from the LPP v1 spec transcript).
const CLEAN_PUZZLE: &str = "puzzle clean {\n  target = 40\n  start = 10\n  ops {\n    double: x => x * 2\n    plus1: x => x + 1\n  }\n  solution = [ double, double ]\n}";

const URI: &str = "file:///project/puzzle.xdl";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn workshop_path() -> PathBuf {
    workspace_root().join("tests/fixtures/workshop/synthetic/control-flow.ws")
}

/// The `#[ignore]` reason gates each provider test — see
/// `crates/wright-lpp/tests/mock_provider.rs` for the contract.
fn mock_provider_path() -> PathBuf {
    match std::env::var("LPP_MOCK_PROVIDER") {
        Ok(path) if !path.is_empty() => PathBuf::from(path),
        _ => panic!(
            "LPP_MOCK_PROVIDER is not set; build the LPP mock provider from the pinned \
             language-provider-protocol commit and point LPP_MOCK_PROVIDER at it \
             (see crates/wright-lpp/tests/mock_provider.rs)"
        ),
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
    let path = mock_provider_path();
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

fn sources_of(documents: &DocumentSet) -> BTreeMap<String, String> {
    documents
        .iter()
        .map(|(uri, document)| (uri.clone(), document.text.clone()))
        .collect()
}

fn handle(service: &mut ToolService<'_>, request: &ToolRequest) -> ProviderMutation {
    match service.handle(request) {
        ToolResponse::Ok { result } => {
            serde_json::from_value(result).expect("provider mutation deserializes")
        }
        ToolResponse::Error { error } => panic!("tool request failed: {error:?}"),
    }
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

#[ignore = "requires LPP_MOCK_PROVIDER pointing at a built lpp-mock-provider; CI sets it (see .github/workflows/ci.yml)"]
#[test]
fn caller_transaction_validates_through_the_provider() {
    let _ = mock_provider_path();
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

#[ignore = "requires LPP_MOCK_PROVIDER pointing at a built lpp-mock-provider; CI sets it (see .github/workflows/ci.yml)"]
#[test]
fn caller_transaction_that_breaks_the_source_refuses() {
    let _ = mock_provider_path();
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
#[ignore = "requires LPP_MOCK_PROVIDER pointing at a built lpp-mock-provider; CI sets it (see .github/workflows/ci.yml)"]
#[test]
fn provider_mutations_do_not_require_the_session_project() {
    let provider = mock_provider_path();
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
/// `lpp/validateEdits` accepts every edit and `lpp/check` reports a clean
/// project. It records every document URI the project-level `lpp/check` is
/// shown so the test can prove which set the session supplied.
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
    elif method == "lpp/validateEdits":
        reply(id, {"valid": True, "version": request["params"]["document"]["version"]})
    elif method == "lpp/check":
        documents = request["params"]["documents"]
        with open(CAPTURE, "a") as capture:
            capture.write("\n".join(sorted(documents)) + "\n")
        reply(id, {"documents": [{"uri": uri, "version": doc["version"], "diagnostics": []}
                                 for uri, doc in documents.items()]})
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
    let transaction = EditTransaction::new(vec![SourceEdit {
        edit_kind: "edit".to_string(),
        source: member_uri.clone(),
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
/// document's text — the transaction's identity precondition verifies
/// against that text.
#[cfg(unix)]
#[test]
fn omitted_sources_default_to_each_documents_text() {
    let SessionDocs {
        dir, mut service, ..
    } = session_docs_service();
    let transaction = EditTransaction::new(vec![SourceEdit {
        edit_kind: "edit".to_string(),
        source: URI.to_string(),
        source_identity: wright_driver::input_identity(CLEAN_PUZZLE),
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
            documents: Some(single_document_set()),
            transaction,
            sources: None,
            project_root: None,
        },
    );
    assert!(
        mutation.ok,
        "the validation succeeds with defaulted sources: {:?}",
        mutation.diagnostics
    );
    let _ = std::fs::remove_dir_all(&dir);
}
