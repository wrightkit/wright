use std::path::{Path, PathBuf};

use serde_json::Value;
use workshop_rs::catalog::{Catalog, Kind};
use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceKind};

fn workshop_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/workshop/synthetic/control-flow.ws")
}

fn session(input: PathBuf) -> CompilerSession {
    CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("session")
}

fn list(
    kind: &str,
    within: Option<&str>,
    page_size: Option<usize>,
    cursor: Option<&str>,
) -> ToolRequest {
    ToolRequest::ListEntries {
        language: "workshop".to_string(),
        kind: kind.to_string(),
        within: within.map(str::to_string),
        locale: None,
        page_size,
        cursor: cursor.map(str::to_string),
    }
}

fn ok(response: ToolResponse) -> Value {
    match response {
        ToolResponse::Ok { result } => result,
        ToolResponse::Error { error } => panic!("listEntries failed: {error:?}"),
    }
}

fn refusal_code(response: ToolResponse) -> String {
    match response {
        ToolResponse::Error { error } => error.code,
        ToolResponse::Ok { result } => panic!("expected a refusal, got {result}"),
    }
}

fn all_pages(
    service: &mut ToolService<'_>,
    kind: &str,
    within: Option<&str>,
    page_size: usize,
) -> (Vec<Value>, u64, usize) {
    let mut entries = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    let total;
    loop {
        let page = ok(service.handle(&list(kind, within, Some(page_size), cursor.as_deref())));
        pages += 1;
        entries.extend(page["entries"].as_array().expect("entries").clone());
        match page["nextCursor"].as_str() {
            Some(next) => cursor = Some(next.to_string()),
            None => {
                total = page["total"].as_u64().expect("total");
                break;
            }
        }
    }
    (entries, total, pages)
}

#[test]
fn every_page_together_is_the_owners_listing_in_owner_order() {
    let catalog = Catalog::builtin().unwrap();
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();

    for (kind, owner_kind) in [
        ("action", Kind::Action),
        ("value", Kind::Value),
        ("event", Kind::Event),
    ] {
        let expected: Vec<String> = catalog
            .entries_of(owner_kind)
            .map(|entry| entry.id.clone())
            .collect();
        let (entries, total, pages) = all_pages(&mut service, kind, None, 7);
        let identities: Vec<String> = entries
            .iter()
            .map(|entry| entry["identity"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(identities, expected, "{kind} identities, in owner order");
        assert_eq!(total as usize, expected.len(), "{kind} total");
        assert!(pages > 1, "{kind} spans several pages at pageSize 7");
    }

    let expected: Vec<String> = catalog
        .enum_domains()
        .map(|domain| domain.domain.clone())
        .collect();
    let (entries, total, _) = all_pages(&mut service, "enum", None, 50);
    let identities: Vec<String> = entries
        .iter()
        .map(|entry| entry["identity"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(identities, expected);
    assert_eq!(total as usize, expected.len());
}

#[test]
fn a_listed_entry_equals_the_lookup_entry_for_the_same_identity() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();

    for kind in ["action", "value", "event", "enum"] {
        let (entries, _, _) = all_pages(&mut service, kind, None, 500);
        for entry in entries.iter().step_by(23) {
            let spelling = entry["spelling"].as_str().unwrap();
            let found = ok(service.handle(&ToolRequest::Lookup {
                language: "workshop".to_string(),
                query: Some(spelling.to_string()),
                kind: Some(kind.to_string()),
                within: None,
                locale: None,
                limit: Some(10),
            }));
            let looked_up = found["entries"]
                .as_array()
                .unwrap()
                .iter()
                .find(|candidate| candidate["identity"] == entry["identity"])
                .unwrap_or_else(|| panic!("lookup finds {}", entry["identity"]));
            assert_eq!(looked_up, entry, "{kind} {}", entry["identity"]);
        }
    }
}

#[test]
fn enum_members_list_every_member_of_a_domain_beyond_the_lookup_cap() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();

    let (members, total, _) = all_pages(&mut service, "enumMember", Some("Hero"), 20);
    assert!(
        members.len() > 10,
        "Hero has more members than lookup's cap"
    );
    assert_eq!(total as usize, members.len());
    assert!(members.iter().all(|entry| entry["kind"] == "enumMember"));
    let mut identities: Vec<&str> = members
        .iter()
        .map(|entry| entry["identity"].as_str().unwrap())
        .collect();
    let count = identities.len();
    identities.sort_unstable();
    identities.dedup();
    assert_eq!(identities.len(), count, "no duplicate members");
}

#[test]
fn repeated_requests_return_identical_pages() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();
    let first = ok(service.handle(&list("action", None, Some(10), None)));
    let again = ok(service.handle(&list("action", None, Some(10), None)));
    assert_eq!(first, again);
    let cursor = first["nextCursor"].as_str().unwrap();
    let second = ok(service.handle(&list("action", None, Some(10), Some(cursor))));
    let second_again = ok(service.handle(&list("action", None, Some(10), Some(cursor))));
    assert_eq!(second, second_again);
    assert_ne!(first["entries"], second["entries"]);
}

#[test]
fn a_cursor_from_another_request_or_a_tampered_one_is_refused() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();
    let page = ok(service.handle(&list("action", None, Some(10), None)));
    let cursor = page["nextCursor"].as_str().unwrap().to_string();

    assert_eq!(
        refusal_code(service.handle(&list("value", None, Some(10), Some(&cursor)))),
        "invalid-cursor"
    );
    let other_locale = ToolRequest::ListEntries {
        language: "workshop".to_string(),
        kind: "action".to_string(),
        within: None,
        locale: Some("zh-CN".to_string()),
        page_size: Some(10),
        cursor: Some(cursor.clone()),
    };
    assert_eq!(
        refusal_code(service.handle(&other_locale)),
        "invalid-cursor"
    );

    let (offset, _) = cursor.split_once('.').unwrap();
    let foreign = format!("{offset}.0000000000000000");
    assert_eq!(
        refusal_code(service.handle(&list("action", None, Some(10), Some(&foreign)))),
        "invalid-cursor"
    );
    for garbage in ["", "zz", "a.b.c", "ffffffff.", "not a cursor"] {
        assert_eq!(
            refusal_code(service.handle(&list("action", None, Some(10), Some(garbage)))),
            "invalid-cursor",
            "{garbage:?}"
        );
    }
    let (_, binding) = cursor.split_once('.').unwrap();
    let past = format!("fffff.{binding}");
    assert_eq!(
        refusal_code(service.handle(&list("action", None, Some(10), Some(&past)))),
        "invalid-cursor"
    );
}

#[test]
fn malformed_requests_are_refused_without_a_listing() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();

    assert_eq!(
        refusal_code(service.handle(&list("enumMember", None, None, None))),
        "malformed-request"
    );
    assert_eq!(
        refusal_code(service.handle(&list("action", Some("Hero"), None, None))),
        "malformed-request"
    );
    assert_eq!(
        refusal_code(service.handle(&list("action", None, Some(0), None))),
        "malformed-request"
    );
    assert_eq!(
        refusal_code(service.handle(&list("action", None, Some(501), None))),
        "malformed-request"
    );
    assert_eq!(
        refusal_code(service.handle(&list("setting", None, None, None))),
        "invalid-kind"
    );
    assert_eq!(
        refusal_code(service.handle(&list("enumMember", Some("NotADomain"), None, None))),
        "lookup.unknownWithin"
    );
    assert_eq!(
        refusal_code(service.handle(&list("enumMember", Some("createHudText"), None, None))),
        "lookup.unknownWithin"
    );
    let unknown_language = ToolRequest::ListEntries {
        language: "klingon".to_string(),
        kind: "action".to_string(),
        within: None,
        locale: None,
        page_size: None,
        cursor: None,
    };
    assert_eq!(
        refusal_code(service.handle(&unknown_language)),
        "invalid-language"
    );
}

#[test]
fn page_size_bounds_a_page_and_the_default_is_one_hundred() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();
    let default_page = ok(service.handle(&list("action", None, None, None)));
    assert_eq!(default_page["entries"].as_array().unwrap().len(), 100);
    let largest = ok(service.handle(&list("value", None, Some(500), None)));
    assert!(largest["entries"].as_array().unwrap().len() <= 500);
}

#[test]
fn opy_listing_is_unavailable_and_names_the_capability() {
    let mut owned = session(workshop_path());
    let mut service = ToolService::new(&mut owned).unwrap();
    let result = ok(service.handle(&ToolRequest::ListEntries {
        language: "opy".to_string(),
        kind: "action".to_string(),
        within: None,
        locale: None,
        page_size: None,
        cursor: None,
    }));
    assert_eq!(result["owner"], "opy-rs");
    assert_eq!(result["entries"].as_array().unwrap().len(), 0);
    assert_eq!(result["unavailable"]["capability"], "listEntries");
}

#[test]
fn listing_answers_without_a_loadable_project_and_does_not_load_one() {
    let dir = std::env::temp_dir().join(format!("wright-list-entries-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut owned = session(dir.join("missing.ws"));
    let mut service = ToolService::new(&mut owned).expect("service starts");
    assert!(service.loaded().is_none());

    let page = ok(service.handle(&list("event", None, None, None)));
    assert!(!page["entries"].as_array().unwrap().is_empty());
    assert!(
        service.loaded().is_none(),
        "listEntries did not trigger a project load"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
