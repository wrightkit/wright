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
    assert!(
        entries.iter().all(|entry| entry["kind"] != "setting"
            || !entry["spelling"].as_str().unwrap_or_default().contains('.')),
        "leaf settings spell a single segment: {entries:?}"
    );
}

#[test]
fn workshop_lookup_callable_carries_the_owners_return_type_and_variadic() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();

    for (identity, returns) in [
        ("distance", "Number"),
        ("eventPlayer", "Player"),
        ("vector", "Vector"),
    ] {
        let entries = entries_of(service.handle(&lookup_request("workshop", Some(identity))));
        let entry = entries
            .iter()
            .find(|entry| entry["identity"] == identity)
            .unwrap_or_else(|| panic!("lookup returned no '{identity}' entry: {entries:?}"));
        assert_eq!(entry["kind"], "value", "{entry}");
        assert_eq!(
            entry["callable"]["returns"], returns,
            "the owner's declared return type: {entry}"
        );
    }

    let entries = entries_of(service.handle(&lookup_request("workshop", Some("array"))));
    let array = entries
        .iter()
        .find(|entry| entry["identity"] == "array")
        .unwrap_or_else(|| panic!("lookup returned no 'array' entry: {entries:?}"));
    assert_eq!(
        array["callable"]["variadic"], true,
        "array's last parameter repeats: {array}"
    );

    let entries = entries_of(service.handle(&lookup_request("workshop", Some("distance"))));
    let distance = entries
        .iter()
        .find(|entry| entry["identity"] == "distance")
        .expect("the distance value");
    assert!(
        distance["callable"].get("variadic").is_none(),
        "distance is not variadic: {distance}"
    );

    let entries = entries_of(service.handle(&lookup_request("workshop", Some("Wait"))));
    let wait = entries
        .iter()
        .find(|entry| entry["identity"] == "wait")
        .expect("the wait action");
    assert_eq!(wait["kind"], "action");
    assert!(
        wait["callable"].get("returns").is_none(),
        "an action declares no return type: {wait}"
    );
}

#[test]
fn lookup_rejects_an_out_of_range_limit_as_malformed() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();

    for limit in [0, 11, usize::MAX] {
        let error = match service.handle(&ToolRequest::Lookup {
            language: "workshop".to_string(),
            query: Some("wait".to_string()),
            kind: None,
            within: None,
            locale: None,
            limit: Some(limit),
        }) {
            ToolResponse::Error { error } => error,
            ToolResponse::Ok { result } => {
                panic!("limit {limit} expected a refusal, got {result:?}")
            }
        };
        assert_eq!(error.code, "malformed-request", "limit {limit}: {error:?}");
        assert!(error.message.contains("limit"), "{error:?}");
    }

    let entries = entries_of(service.handle(&ToolRequest::Lookup {
        language: "workshop".to_string(),
        query: None,
        kind: None,
        within: Some("Hero".to_string()),
        locale: None,
        limit: Some(10),
    }));
    assert_eq!(entries.len(), 10, "the boundary limit still answers");

    let entries = entries_of(service.handle(&ToolRequest::Lookup {
        language: "workshop".to_string(),
        query: None,
        kind: None,
        within: Some("Hero".to_string()),
        locale: None,
        limit: None,
    }));
    assert_eq!(entries.len(), 3, "an omitted limit keeps the default");
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

    let entries = entries_of(service.handle(&ToolRequest::Lookup {
        language: "workshop".to_string(),
        query: Some("visibleTo".to_string()),
        kind: None,
        within: Some("createHudText".to_string()),
        locale: None,
        limit: None,
    }));
    assert_eq!(
        entries.len(),
        1,
        "the scoped param query resolves: {entries:?}"
    );
    assert_eq!(entries[0]["identity"], "createHudText.VisibleTo");
    assert_eq!(entries[0]["kind"], "parameter");
}

#[test]
fn workshop_lookup_answers_without_a_loadable_project() {
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

    let _ = service.handle(&ToolRequest::Check { all: true });
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
    assert!(
        kind.message.contains("'parameter'") && kind.message.contains("'structural'"),
        "the refusal names every accepted kind: {}",
        kind.message
    );
}

#[test]
fn lookup_kind_filter_accepts_every_kind_the_service_emits() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();

    for (query, kind) in [("If", "structural"), ("Team", "enum")] {
        let entries = entries_of(service.handle(&ToolRequest::Lookup {
            language: "workshop".to_string(),
            query: Some(query.to_string()),
            kind: Some(kind.to_string()),
            within: None,
            locale: None,
            limit: None,
        }));
        assert!(
            !entries.is_empty() && entries.iter().all(|entry| entry["kind"] == kind),
            "kind '{kind}' is accepted and filters: {entries:?}"
        );
    }

    for (within, kind) in [("createHudText", "parameter"), ("heroes", "settingPath")] {
        let entries = entries_of(service.handle(&ToolRequest::Lookup {
            language: "workshop".to_string(),
            query: None,
            kind: Some(kind.to_string()),
            within: Some(within.to_string()),
            locale: None,
            limit: Some(10),
        }));
        assert!(
            !entries.is_empty() && entries.iter().all(|entry| entry["kind"] == kind),
            "scoped kind '{kind}' filters: {entries:?}"
        );
    }

    let entries = entries_of(service.handle(&ToolRequest::Lookup {
        language: "workshop".to_string(),
        query: Some("health".to_string()),
        kind: Some("memberAction".to_string()),
        within: None,
        locale: None,
        limit: None,
    }));
    assert!(entries.is_empty(), "workshop emits no memberAction");
}

#[test]
fn lookup_accepts_every_kind_any_response_emits() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();

    let mut emitted = std::collections::BTreeSet::new();
    for within in [None, Some("Team"), Some("createHudText"), Some("heroes")] {
        let entries = entries_of(service.handle(&ToolRequest::Lookup {
            language: "workshop".to_string(),
            query: within.is_none().then(|| "a".to_string()),
            kind: None,
            within: within.map(str::to_string),
            locale: None,
            limit: Some(10),
        }));
        for entry in entries {
            emitted.insert(
                entry["kind"]
                    .as_str()
                    .unwrap_or_else(|| panic!("entry has no kind: {entry}"))
                    .to_string(),
            );
        }
    }
    assert!(!emitted.is_empty(), "the probes emitted entries");
    for kind in &emitted {
        match service.handle(&ToolRequest::Lookup {
            language: "workshop".to_string(),
            query: Some("a".to_string()),
            kind: Some(kind.clone()),
            within: None,
            locale: None,
            limit: None,
        }) {
            ToolResponse::Ok { .. } => {}
            ToolResponse::Error { error } => {
                panic!("emitted kind '{kind}' refused as {error:?}")
            }
        }
    }

    #[cfg(unix)]
    {
        let (dir, mut opy) = service_with_provider(LOOKUP_PROVIDER);
        for kind in ["action", "enum", "memberAction"] {
            match opy.handle(&ToolRequest::Lookup {
                language: "opy".to_string(),
                query: Some("hud".to_string()),
                kind: Some(kind.to_string()),
                within: None,
                locale: None,
                limit: None,
            }) {
                ToolResponse::Ok { .. } => {}
                ToolResponse::Error { error } => {
                    panic!("opy kind '{kind}' refused as {error:?}")
                }
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }
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

#[cfg(unix)]
fn write_provider(script: &str) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-lookup-provider-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
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

#[test]
fn workshop_lookup_reports_parameter_content_facts() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();
    let mut lookup = |query: &str| {
        entries_of(service.handle(&ToolRequest::Lookup {
            language: "workshop".to_string(),
            query: Some(query.to_string()),
            kind: None,
            within: None,
            locale: None,
            limit: Some(10),
        }))
    };

    let chase = lookup("chaseOverTime")
        .into_iter()
        .find(|entry| entry["identity"] == "chaseOverTime")
        .expect("the chase action");
    let params = chase["callable"]["parameters"]
        .as_array()
        .expect("parameters");
    assert_eq!(
        params[0]["content"]["fold"],
        serde_json::json!({ "call": "globalVariable", "arg": 0 }),
        "the variable target folds surface parameters into the call: {params:?}"
    );

    let modify = lookup("modifyGlobalVariable")
        .into_iter()
        .find(|entry| entry["identity"] == "modifyGlobalVariable")
        .expect("the modify action");
    let params = modify["callable"]["parameters"]
        .as_array()
        .expect("parameters");
    let operations = params[1]["content"]["operations"]
        .as_array()
        .expect("declared modify operations");
    assert!(operations.contains(&serde_json::json!("add")));
    assert!(operations.contains(&serde_json::json!("removeFromArrayByIndex")));
}
