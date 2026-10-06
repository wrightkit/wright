//! `lookup` (ADR-0021, #529): the operation answers from language
//! vocabulary alone — with or without a loadable project — through the
//! workshop-rs catalog in process and the configured provider's LPP 1.5
//! `lpp/lookup` capability for `opy`.

use std::path::{Path, PathBuf};

use serde_json::Value;
use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceKind};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn workshop_path() -> PathBuf {
    workspace_root().join("tests/fixtures/workshop/synthetic/control-flow.ws")
}

fn session(input: PathBuf) -> CompilerSession {
    CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("session")
}

fn lookup_request(language: &str, query: Option<&str>) -> ToolRequest {
    ToolRequest::Lookup {
        language: language.to_string(),
        query: query.map(str::to_string),
        kind: None,
        within: None,
        locale: None,
        limit: None,
    }
}

fn result_of(response: ToolResponse) -> Value {
    match response {
        ToolResponse::Ok { result } => result,
        ToolResponse::Error { error } => panic!("lookup failed: {error:?}"),
    }
}

fn entries_of(response: ToolResponse) -> Vec<Value> {
    result_of(response)["entries"]
        .as_array()
        .expect("entries")
        .clone()
}

#[test]
fn workshop_lookup_resolves_a_display_name_to_an_action() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();
    let entries = entries_of(service.handle(&lookup_request("workshop", Some("Create HUD Text"))));

    assert!(!entries.is_empty(), "expected lookup entries");
    let first = &entries[0];
    assert_eq!(first["kind"], "action");
    assert_eq!(first["identity"], "createHudText");
    assert_eq!(first["spelling"], "Create HUD Text");
    let signature = first["signature"].as_str().expect("rendered signature");
    assert!(
        signature.starts_with("Create HUD Text("),
        "signature renders the head and parameters: {signature}"
    );
}

#[test]
fn workshop_lookup_renders_signature_from_owner_facts() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();
    // `wait` takes a required duration and an optional enum-typed
    // behavior whose declared enum default renders as its spelling.
    let entries = entries_of(service.handle(&lookup_request("workshop", Some("Wait"))));
    let wait = entries
        .iter()
        .find(|entry| entry["identity"] == "wait")
        .expect("the wait action");
    assert_eq!(
        wait["signature"],
        "Wait(time: Any, waitBehavior=Ignore Condition)"
    );
}

#[test]
fn workshop_lookup_lists_enum_members_within_a_domain() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();
    let entries = entries_of(service.handle(&ToolRequest::Lookup {
        language: "workshop".to_string(),
        query: None,
        kind: None,
        within: Some("Team".to_string()),
        locale: None,
        limit: Some(10),
    }));

    assert!(!entries.is_empty());
    assert!(entries.iter().all(|entry| entry["kind"] == "enumMember"));
    let spellings: Vec<&str> = entries
        .iter()
        .map(|entry| entry["spelling"].as_str().unwrap())
        .collect();
    assert!(spellings.contains(&"All Teams"), "members: {spellings:?}");
    assert!(
        entries.iter().any(|entry| entry["identity"] == "Team.ALL"),
        "member identities carry the domain: {entries:?}"
    );
}

#[test]
fn workshop_lookup_lists_callable_parameters_within() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();
    let entries = entries_of(service.handle(&ToolRequest::Lookup {
        language: "workshop".to_string(),
        query: None,
        kind: None,
        within: Some("createHudText".to_string()),
        locale: None,
        limit: Some(10),
    }));

    assert!(!entries.is_empty());
    assert!(entries.iter().all(|entry| entry["kind"] == "parameter"));
    assert_eq!(entries[0]["spelling"], "VisibleTo");
    assert_eq!(entries[0]["parameter"]["required"], true);
}

#[test]
fn workshop_lookup_lists_settings_children_under_a_prefix() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();
    let entries = entries_of(service.handle(&ToolRequest::Lookup {
        language: "workshop".to_string(),
        query: None,
        kind: None,
        within: Some("heroes".to_string()),
        locale: None,
        limit: Some(10),
    }));

    // `heroes` has two child segments: the team template and `general`.
    assert_eq!(entries.len(), 2, "heroes children: {entries:?}");
    assert!(entries.iter().all(|entry| entry["kind"] == "settingPath"));
    assert_eq!(entries[0]["spelling"], "<team>");
    assert_eq!(entries[0]["identity"], "heroes.<team>");
    assert_eq!(entries[1]["spelling"], "general");
}

#[test]
fn workshop_lookup_answers_a_setting_key() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();
    let entries = entries_of(service.handle(&ToolRequest::Lookup {
        language: "workshop".to_string(),
        query: None,
        kind: None,
        within: Some("lobby".to_string()),
        locale: None,
        limit: Some(10),
    }));

    assert!(!entries.is_empty(), "lobby has settings keys");
    assert!(
        entries.iter().all(|entry| matches!(
            entry["kind"].as_str(),
            Some("setting") | Some("settingPath")
        )),
        "settings children are settings or segments: {entries:?}"
    );
}

#[test]
fn workshop_lookup_filters_by_kind_and_bounds_the_limit() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();
    let entries = entries_of(service.handle(&ToolRequest::Lookup {
        language: "workshop".to_string(),
        query: Some("hero".to_string()),
        kind: Some("enumMember".to_string()),
        within: None,
        locale: None,
        limit: Some(1),
    }));

    assert_eq!(entries.len(), 1, "limit bounds the result");
    assert_eq!(entries[0]["kind"], "enumMember");
}

#[test]
fn workshop_lookup_bounds_scoped_results_by_limit() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();
    // `within` listings obey the same bound as unscoped results: the
    // default of 3 caps a wider scope without an explicit limit.
    let entries = entries_of(service.handle(&ToolRequest::Lookup {
        language: "workshop".to_string(),
        query: None,
        kind: None,
        within: Some("lobby".to_string()),
        locale: None,
        limit: None,
    }));
    assert_eq!(entries.len(), 3, "default limit bounds a scoped listing");

    let entries = entries_of(service.handle(&ToolRequest::Lookup {
        language: "workshop".to_string(),
        query: None,
        kind: None,
        within: Some("Team".to_string()),
        locale: None,
        limit: Some(2),
    }));
    assert_eq!(entries.len(), 2, "explicit limit bounds a scoped listing");
    assert_eq!(entries[0]["identity"], "Team.ALL");
}

#[test]
fn workshop_lookup_filters_scoped_children_by_the_owner_matcher() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();
    // `within` + `query` routes matching and ranking through workshop-rs:
    // the result is the owner's ranked unscoped matches intersected with
    // the scope's children, so "team 1" resolves the members the owner
    // scored — never a Wright-side substring filter.
    let entries = entries_of(service.handle(&ToolRequest::Lookup {
        language: "workshop".to_string(),
        query: Some("team 1".to_string()),
        kind: None,
        within: Some("Team".to_string()),
        locale: None,
        limit: Some(10),
    }));
    assert!(!entries.is_empty(), "the owner matched scoped children");
    assert!(
        entries.iter().all(|entry| entry["identity"]
            .as_str()
            .unwrap_or_default()
            .starts_with("Team.")),
        "every matched child belongs to the scope: {entries:?}"
    );
    assert_eq!(
        entries[0]["identity"], "Team.TEAM_1",
        "owner ranking puts the closest match first"
    );

    // Parameters are outside the owner's match vocabulary — an unscoped
    // request never returns them — so a scoped query drops them rather
    // than guessing at a spelling.
    let entries = entries_of(service.handle(&ToolRequest::Lookup {
        language: "workshop".to_string(),
        query: Some("text".to_string()),
        kind: None,
        within: Some("createHudText".to_string()),
        locale: None,
        limit: None,
    }));
    assert!(
        entries.is_empty(),
        "parameters are not matchable vocabulary: {entries:?}"
    );
}

#[test]
fn workshop_lookup_answers_without_a_loadable_project() {
    // A session whose configured project cannot load: lookup is a
    // vocabulary query and neither requires nor triggers a load (#512).
    let dir = std::env::temp_dir().join(format!("wright-lookup-512-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut owned = session(dir.join("missing.ws"));
    let mut service = ToolService::new(&mut owned).expect("service starts");
    assert!(service.loaded().is_none());

    let entries = entries_of(service.handle(&lookup_request("workshop", Some("Create HUD Text"))));
    assert!(
        entries
            .iter()
            .any(|entry| entry["identity"] == "createHudText"),
        "lookup answers without a loaded program: {entries:?}"
    );
    assert!(
        service.loaded().is_none(),
        "lookup did not trigger a project load"
    );

    // The request path stays project-free even after a program-reading
    // failure invalidated the service.
    let _ = service.handle(&ToolRequest::Check);
    let entries = entries_of(service.handle(&lookup_request("workshop", Some("Wait"))));
    assert!(!entries.is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lookup_refuses_an_unknown_language_or_kind() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();

    let language = match service.handle(&lookup_request("deltin", Some("x"))) {
        ToolResponse::Error { error } => error,
        ToolResponse::Ok { result } => panic!("expected a refusal, got {result:?}"),
    };
    assert_eq!(language.code, "invalid-language");

    let kind = match service.handle(&ToolRequest::Lookup {
        language: "workshop".to_string(),
        query: Some("x".to_string()),
        kind: Some("nonsense".to_string()),
        within: None,
        locale: None,
        limit: None,
    }) {
        ToolResponse::Error { error } => error,
        ToolResponse::Ok { result } => panic!("expected a refusal, got {result:?}"),
    };
    assert_eq!(kind.code, "invalid-kind");
}

#[test]
fn lookup_refuses_an_unknown_within_scope() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();
    let error = match service.handle(&ToolRequest::Lookup {
        language: "workshop".to_string(),
        query: None,
        kind: None,
        within: Some("not-a-scope".to_string()),
        locale: None,
        limit: None,
    }) {
        ToolResponse::Error { error } => error,
        ToolResponse::Ok { result } => panic!("expected a refusal, got {result:?}"),
    };
    assert_eq!(error.code, "lookup.unknownWithin");
}

/// An LPP 1.5 fake provider serving `lpp/lookup` for `opy` from canned
/// vocabulary facts; the callable fact exercises Wright-side signature
/// rendering.
#[cfg(unix)]
const LOOKUP_PROVIDER: &str = r#"#!/usr/bin/env python3
import json, sys

def reply(id, result=None, error=None):
    message = {"jsonrpc": "2.0", "id": id}
    message.update({"error": error} if error else {"result": result})
    print(json.dumps(message), flush=True)

for line in sys.stdin:
    request = json.loads(line)
    id, method = request["id"], request["method"]
    if method == "lpp/initialize":
        reply(id, {"protocolVersion": "1.5", "serverInfo": {"name": "fake", "version": "0"},
                   "languages": [{"id": "opy", "extensions": ["opy"]}],
                   "capabilities": {"check": True, "compile": True, "reconstruct": False,
                                    "symbols": False, "definition": False, "references": False,
                                    "rename": False, "editValidation": False,
                                    "projectLoading": False, "lookup": True}})
    elif method == "lpp/lookup":
        within = request["params"].get("within")
        # The provider accepts the identities it issued only verbatim, for
        # the matching selector kind — a client that rewrote them (e.g.
        # stripped the `opy:setting/` prefix) gets `lookup.unknownWithin`.
        known = (within is None
                 or (within["kind"] == "enum" and within["value"] == "opy:enum/RoundingMode")
                 or (within["kind"] == "settings" and within["value"] == "opy:setting/heroes"))
        if not known:
            reply(id, error={"code": -32000, "message": "within selector names no known scope",
                             "data": {"lpp": {"kind": "refusal",
                                      "details": {"refusalCode": "lookup.unknownWithin"}}}})
        else:
            reply(id, {"entries": [
                {"identity": "opy:callable/hudText", "kind": "action",
                 "spelling": "hudText", "displayName": "HUD Text",
                 "callable": {"parameters": [
                     {"name": "visibleTo", "type": "Player", "required": True,
                      "enum": {"domain": "opy:enum/Player", "members": ["All", "One"]}},
                     {"name": "header", "type": "String", "required": True},
                     {"name": "rounding", "type": "RoundingMode", "required": False,
                      "default": "Up", "enum": {"domain": "opy:enum/RoundingMode",
                                                "members": ["Up", "Down"]}}]}},
                {"identity": "opy:enum/RoundingMode", "kind": "enum",
                 "spelling": "RoundingMode", "displayName": "Rounding Mode",
                 "enum": {"domain": "opy:enum/RoundingMode", "members": ["Up", "Down"]}},
            ]})
    else:
        reply(id, {})
"#;

/// A provider that negotiates but never advertises `lookup` (a pre-1.5
/// session shape).
#[cfg(unix)]
const NO_LOOKUP_PROVIDER: &str = r#"#!/usr/bin/env python3
import json, sys

for line in sys.stdin:
    request = json.loads(line)
    id, method = request["id"], request["method"]
    if method == "lpp/initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": id, "result": {
            "protocolVersion": "1.4", "serverInfo": {"name": "fake", "version": "0"},
            "languages": [{"id": "opy", "extensions": ["opy"]}],
            "capabilities": {"check": True, "compile": True, "reconstruct": False,
                             "symbols": False, "definition": False, "references": False,
                             "rename": False, "editValidation": False,
                             "projectLoading": False}}}), flush=True)
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": id, "result": {}}), flush=True)
"#;

#[cfg(unix)]
fn write_provider(script: &str) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!(
        "wright-lookup-provider-{}-{}",
        std::process::id(),
        script.len()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fake-provider");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    (dir, path)
}

#[cfg(unix)]
fn service_with_provider(script: &str) -> (PathBuf, ToolService<'static>) {
    let (dir, path) = write_provider(script);
    let session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        opy_provider: wright_driver::OpyProviderConfig::with_executable(path),
        ..SessionConfig::default()
    })
    .expect("session");
    let session = Box::leak(Box::new(session));
    let service = ToolService::new(session).expect("service");
    (dir, service)
}

#[cfg(unix)]
#[test]
fn opy_lookup_serves_provider_entries_with_a_rendered_signature() {
    let (dir, mut service) = service_with_provider(LOOKUP_PROVIDER);
    let result = result_of(service.handle(&lookup_request("opy", Some("hud"))));
    assert_eq!(result["owner"], "opy-rs");
    let entries = result["entries"].as_array().expect("entries");
    assert!(!entries.is_empty());

    let callable = &entries[0];
    assert_eq!(callable["identity"], "opy:callable/hudText");
    // Wright renders the signature from the provider's facts (ADR-0021):
    // the optional enum parameter shows its default, not its members.
    assert_eq!(
        callable["signature"],
        "hudText(visibleTo: Player(All|One), header: String, rounding=Up)"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn opy_lookup_resolves_a_within_identity_through_the_provider() {
    let (dir, mut service) = service_with_provider(LOOKUP_PROVIDER);
    let result = result_of(service.handle(&ToolRequest::Lookup {
        language: "opy".to_string(),
        query: None,
        kind: None,
        within: Some("opy:enum/RoundingMode".to_string()),
        locale: None,
        limit: None,
    }));
    let entries = result["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 2, "the canned provider's entries");

    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn opy_lookup_passes_scoped_identities_through_unchanged() {
    let (dir, mut service) = service_with_provider(LOOKUP_PROVIDER);
    // The provider accepts `opy:setting/heroes` only verbatim under the
    // `settings` selector: a client that rewrote the opaque identity —
    // stripping `opy:setting/` to "expose" the path — would earn
    // `lookup.unknownWithin` here instead (`name-lookup.md` §21.3).
    let result = result_of(service.handle(&ToolRequest::Lookup {
        language: "opy".to_string(),
        query: None,
        kind: None,
        within: Some("opy:setting/heroes".to_string()),
        locale: None,
        limit: None,
    }));
    let entries = result["entries"].as_array().expect("entries");
    assert!(
        !entries.is_empty(),
        "the provider-issued identity passed through unchanged: {result:?}"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn opy_lookup_reports_unavailable_when_the_provider_lacks_the_capability() {
    let (dir, mut service) = service_with_provider(NO_LOOKUP_PROVIDER);
    let result = result_of(service.handle(&lookup_request("opy", Some("hud"))));
    assert_eq!(result["owner"], "opy-rs");
    assert_eq!(result["entries"], serde_json::json!([]));
    assert_eq!(result["unavailable"]["capability"], "lookup");

    let _ = std::fs::remove_dir_all(dir);
}
