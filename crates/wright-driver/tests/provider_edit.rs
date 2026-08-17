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

use wright_driver::config::{InputSpec, SessionConfig, SourceKind};
use wright_driver::edit::{EditRange, EditTransaction, SourceEdit};
use wright_driver::provider_edit::ProviderMutation;
use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::{CompilerSession, Profile};
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

/// A session configured with an OPY input (the tool service loads a project
/// eagerly) plus the given provider registry.
fn session_with_providers(providers: wright_lpp::ProviderRegistry) -> CompilerSession {
    let config = SessionConfig {
        input: InputSpec::Path(
            workspace_root().join("compatibility/fixtures/synthetic/basic-rule/source.opy"),
        ),
        kind: SourceKind::Opy,
        profile: Profile::Compat,
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
    let mut session = session_with_providers(registry);
    let _ = session.load().expect("loads");
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
        documents,
        position_document_uri: position_document_uri.to_string(),
        position,
        new_name: new_name.to_string(),
        project_root: Some("file:///project".to_string()),
        sources,
    }
}

/// The position of the `double` op declaration (0-based line 4, char 6).
const DOUBLE_POSITION: Position = Position {
    line: 4,
    character: 6,
};

fn handle(service: &ToolService<'_>, request: &ToolRequest) -> ProviderMutation {
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
    let service = tool_service(&[]);
    let documents = single_document_set();
    let mutation = handle(
        &service,
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
    let service = tool_service(&[]);
    let documents = two_document_set();
    let mutation = handle(
        &service,
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
    let service = tool_service(&[]);
    let documents = single_document_set();
    let mutation = handle(
        &service,
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
    let service = tool_service(&[]);
    let documents = single_document_set();
    let mutation = handle(
        &service,
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
    let service = tool_service(&[]);
    let documents = single_document_set();
    // The caller's current text no longer matches the snapshot the provider
    // computed the rename against: the Wright-owned identity precondition
    // refuses before anything can be applied.
    let mut stale_sources = sources_of(&documents);
    stale_sources.insert(URI.to_string(), format!("{CLEAN_PUZZLE}\n"));
    let mutation = handle(
        &service,
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
    let service = tool_service(&["rename"]);
    let documents = single_document_set();
    let mutation = handle(
        &service,
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
    let service = tool_service(&["editValidation"]);
    let documents = single_document_set();
    let mutation = handle(
        &service,
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
    let service = tool_service(&[]);
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
        &service,
        &rename_request(documents, URI, DOUBLE_POSITION, "plus1", sources),
    );
    assert!(!mutation.ok);
    assert_eq!(mutation.diagnostics[0].code, "provider-validation-failed");
    assert!(mutation.transaction.is_none(), "no partial edit set");
    assert!(mutation.preview.is_none(), "no partial preview");
}

#[test]
fn unconfigured_language_id_refuses_explicitly() {
    // No mock provider needed: an unconfigured language id is refused by
    // the session's registry before anything is spawned.
    let mut session = session_with_providers(wright_lpp::ProviderRegistry::new());
    let _ = session.load().expect("loads");
    let session = Box::leak(Box::new(session));
    let service = ToolService::new(session).expect("service");
    let documents = single_document_set();
    let mutation = handle(
        &service,
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
        Some("provider-not-configured")
    );
    assert!(mutation.transaction.is_none());
    assert!(mutation.preview.is_none());
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
    let service = tool_service(&[]);
    let documents = single_document_set();
    let request = ToolRequest::ProviderValidateEdit {
        language_id: DEMO_LANGUAGE_ID.to_string(),
        documents,
        transaction: rename_all_occurrences_transaction(),
        sources: sources_of(&single_document_set()),
        project_root: Some("file:///project".to_string()),
    };
    let mutation = handle(&service, &request);
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
    let service = tool_service(&[]);
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
        documents,
        transaction,
        sources: sources_of(&single_document_set()),
        project_root: None,
    };
    let mutation = handle(&service, &request);
    assert!(!mutation.ok);
    assert_eq!(mutation.diagnostics[0].code, "provider-validation-failed");
    assert!(mutation.transaction.is_none());
    assert!(mutation.preview.is_none(), "no partial preview");
}
