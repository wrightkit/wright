mod support;

use std::net::TcpListener;
use std::path::{Path, PathBuf};

use wright_driver::LintPackageStore;

use support::*;

fn write_entry(root: &Path, sha: &str, extra: &[(&str, &[u8])]) -> PathBuf {
    let dir = root.join("community/rules").join(sha);
    std::fs::create_dir_all(dir.join("rules")).unwrap();
    std::fs::write(dir.join("rules").join("rule.yaml"), RULE).unwrap();
    for (name, body) in extra {
        std::fs::write(dir.join(name), body).unwrap();
    }
    dir
}

#[test]
fn entries_enumerates_intact_and_damaged_entries_in_order() {
    let dir = TestDir::new("entries");
    write_entry(dir.path(), SHA_A, &[]);
    write_entry(dir.path(), SHA_B, &[]);
    let damaged = dir.path().join("damaged/pkg").join(SHA_A).join("rules");
    std::fs::create_dir_all(&damaged).unwrap();
    std::fs::create_dir_all(dir.path().join(".lint-package-staging")).unwrap();
    std::fs::create_dir_all(dir.path().join("community/rules/latest")).unwrap();
    std::fs::write(dir.path().join("README.md"), b"notes").unwrap();
    let server = TestServer::serve(|_| Response::not_found());
    let store = store(dir.path(), &server);
    let entries = store.entries().unwrap();
    assert_eq!(entries.len(), 3);
    let slugs: Vec<String> = entries
        .iter()
        .map(|e| format!("{}@{}", e.source.slug(), &e.sha[..7]))
        .collect();
    assert_eq!(
        slugs,
        [
            format!("community/rules@{}", &SHA_A[..7]),
            format!("community/rules@{}", &SHA_B[..7]),
            format!("damaged/pkg@{}", &SHA_A[..7]),
        ]
    );
    assert_eq!(entries[0].rules, ["community/minimum-wait"]);
    assert!(entries[0].damage.is_none());
    let damaged = &entries[2];
    assert_eq!(
        damaged.damage.as_ref().map(|e| e.code()),
        Some("lint-package-install")
    );
    assert!(damaged.rules.is_empty());
    assert!(server.requests().is_empty());
}

#[test]
fn remove_deletes_the_entry_and_prunes_empty_parents() {
    let dir = TestDir::new("remove");
    write_entry(dir.path(), SHA_A, &[]);
    write_entry(dir.path(), SHA_B, &[]);
    let server = TestServer::serve(|_| Response::not_found());
    let store = store(dir.path(), &server);
    let removed = store.remove(&source(), SHA_A).unwrap();
    assert!(removed.dir_removed);
    assert!(!dir.path().join("community/rules").join(SHA_A).exists());
    assert_eq!(
        store_entries(dir.path()),
        [format!("community/rules/{SHA_B}")]
    );
    store.remove(&source(), SHA_B).unwrap();
    assert_eq!(store_entries(dir.path()), Vec::<String>::new());
    assert!(!dir.path().join("community").exists());
}

#[test]
fn remove_preserves_a_user_owned_file_and_prunes_the_rules_tree() {
    let dir = TestDir::new("remove-pruned");
    let entry = write_entry(dir.path(), SHA_A, &[("notes.txt", b"mine")]);
    let server = TestServer::serve(|_| Response::not_found());
    let store = store(dir.path(), &server);
    let removed = store.remove(&source(), SHA_A).unwrap();
    assert!(!removed.dir_removed, "unrelated files keep the entry dir");
    assert!(!entry.join("rules").exists(), "managed rules are gone");
    assert_eq!(
        std::fs::read_to_string(entry.join("notes.txt")).unwrap(),
        "mine",
        "the user-owned file survives"
    );
}

#[test]
fn remove_of_a_missing_entry_is_not_found() {
    let dir = TestDir::new("remove-missing");
    let server = TestServer::serve(|_| Response::not_found());
    let store = store(dir.path(), &server);
    let error = store.remove(&source(), SHA_A).unwrap_err();
    assert_eq!(error.code(), "lint-package-not-found");
}

fn search_page(hits: &[(&str, Option<&str>)], total: u64) -> String {
    let items: Vec<String> = hits
        .iter()
        .map(|(full_name, description)| {
            let description = description
                .map(|d| format!("\"{d}\""))
                .unwrap_or_else(|| "null".to_string());
            format!("{{\"full_name\":\"{full_name}\",\"description\":{description}}}")
        })
        .collect();
    format!(
        "{{\"total_count\":{total},\"incomplete_results\":false,\"items\":[{}]}}",
        items.join(",")
    )
}

#[test]
fn search_pages_the_wrightkit_lint_topic_query() {
    let dir = TestDir::new("search");
    let mut first: Vec<(&str, Option<&str>)> = (0..100)
        .map(|i| {
            (
                Box::leak(format!("owner-{i:03}/repo").into_boxed_str()) as &str,
                None,
            )
        })
        .collect();
    first.push(("bad full_name", None));
    let server = TestServer::serve(move |request| {
        if request.path.starts_with("/search/repositories?") && request.path.ends_with("page=1") {
            Response::json(&search_page(&first, 101))
        } else if request.path.ends_with("page=2") {
            Response::json(&search_page(
                &[("community/rules", Some("wait rules"))],
                101,
            ))
        } else {
            Response::not_found()
        }
    });
    let store = store(dir.path(), &server);
    let page = store.search(None).unwrap();
    assert_eq!(page.total, 101);
    assert!(!page.incomplete);
    assert_eq!(page.hits.len(), 101, "invalid full_names are skipped");
    assert_eq!(page.hits[0].source.slug(), "owner-000/repo");
    assert_eq!(page.hits[100].source.slug(), "community/rules");
    assert_eq!(page.hits[100].description.as_deref(), Some("wait rules"));
    let requests = server.find("/search/repositories");
    assert_eq!(requests.len(), 2);
    assert!(
        requests[0]
            .path
            .contains("q=topic:wrightkit-lint%20archived:false%20fork:false&per_page=100&page=1"),
        "the topic filter excludes archives and forks: {}",
        requests[0].path
    );
}

#[test]
fn search_appends_terms_to_the_topic_query() {
    let dir = TestDir::new("search-terms");
    let server = TestServer::serve(move |request| {
        if request.path.starts_with("/search/repositories?") {
            Response::json(&search_page(&[("community/rules", None)], 1))
        } else {
            Response::not_found()
        }
    });
    let store = store(dir.path(), &server);
    let page = store.search(Some("wait loops")).unwrap();
    assert_eq!(page.hits.len(), 1);
    let requests = server.find("/search/repositories");
    assert_eq!(
        requests[0].path,
        "/search/repositories?q=topic:wrightkit-lint%20archived:false%20fork:false%20wait%20loops&per_page=100&page=1"
    );
}

#[test]
fn search_surfaces_remote_error_classes() {
    let dir = TestDir::new("search-errors");
    let server = TestServer::serve(|_| Response {
        status: "403 Forbidden",
        headers: vec![
            ("x-ratelimit-remaining".to_string(), "0".to_string()),
            ("x-ratelimit-reset".to_string(), "1893456000".to_string()),
        ],
        body: b"API rate limit exceeded".to_vec(),
    });
    let store = store(dir.path(), &server);
    assert_eq!(
        store.search(None).unwrap_err().code(),
        "lint-package-rate-limited"
    );
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let dead = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let offline = LintPackageStore::new(dir.path())
        .with_api_base(&dead)
        .with_token(None);
    assert_eq!(
        offline.search(None).unwrap_err().code(),
        "lint-package-offline"
    );
    let malformed = TestServer::serve(|_| Response::json("{\"unexpected\":true}"));
    let store = LintPackageStore::new(dir.path())
        .with_api_base(malformed.url())
        .with_token(None);
    assert_eq!(
        store.search(None).unwrap_err().code(),
        "lint-package-remote"
    );
}

#[test]
fn entries_of_an_empty_or_missing_store_is_empty() {
    let dir = TestDir::new("entries-empty");
    let server = TestServer::serve(|_| Response::not_found());
    let store = LintPackageStore::new(dir.path().join("absent"))
        .with_api_base(server.url())
        .with_token(None);
    assert!(store.entries().unwrap().is_empty());
}
