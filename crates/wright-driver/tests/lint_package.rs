//! Offline lint package store tests: every remote answer is served by a
//! recorded-response HTTP server on localhost; no test touches real network.

mod support;

use std::collections::HashMap;
use std::net::TcpListener;

use wright_driver::{LintPackageSource, LintPackageStore, RemoteCheck};

use support::*;
#[test]
fn full_sha_install_stores_only_rule_files() {
    let dir = TestDir::new("full-sha");
    let archive = package_tarball(RULE);
    let server = TestServer::serve(move |request| {
        if request.path == format!("/community/rules/tar.gz/{SHA_A}") {
            Response::ok(archive.clone())
        } else {
            Response::not_found()
        }
    });
    let store = store(dir.path(), &server);
    let installed = store.install(&source(), Some(SHA_A)).unwrap();
    assert!(installed.downloaded);
    assert_eq!(installed.sha, SHA_A);
    assert_eq!(installed.rules, ["community/minimum-wait"]);
    assert_eq!(
        store.installed(&source(), SHA_A),
        Some(dir.path().join("community/rules").join(SHA_A))
    );
    // Only rules/*.yaml|yml was extracted; repository code was not read.
    let rules = installed.rules_dir();
    assert_eq!(
        std::fs::read_dir(&rules)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["rule.yaml"]
    );
    assert!(
        std::fs::read_to_string(rules.join("rule.yaml"))
            .unwrap()
            .contains("community/minimum-wait")
    );
    // A full SHA needs no commits-API resolution.
    assert!(server.find("/repos/").is_empty());
}

#[test]
fn multi_rule_packages_load_in_lexical_file_order() {
    let dir = TestDir::new("multi-rule");
    let archive = tarball(&[
        // Lexical file order, not id order, sets the load order.
        ("o-r-aaaaaaaa/rules/b-second.yaml", RULE.as_bytes()),
        ("o-r-aaaaaaaa/rules/a-first.yaml", SECOND_RULE.as_bytes()),
    ]);
    let server = TestServer::serve(move |request| {
        if request.path == format!("/community/rules/tar.gz/{SHA_A}") {
            Response::ok(archive.clone())
        } else {
            Response::not_found()
        }
    });
    let store = store(dir.path(), &server);
    let installed = store.install(&source(), Some(SHA_A)).unwrap();
    assert_eq!(
        installed.rules,
        ["community/has-wait", "community/minimum-wait"]
    );
}

#[test]
fn second_fetch_of_an_intact_entry_makes_no_download() {
    let dir = TestDir::new("no-redownload");
    let archive = package_tarball(RULE);
    let server = TestServer::serve(move |request| {
        if request.path == format!("/community/rules/tar.gz/{SHA_A}") {
            Response::ok(archive.clone())
        } else {
            Response::not_found()
        }
    });
    let store = store(dir.path(), &server);
    store.install(&source(), Some(SHA_A)).unwrap();
    let again = store.install(&source(), Some(SHA_A)).unwrap();
    assert!(!again.downloaded);
    assert_eq!(again.rules, ["community/minimum-wait"]);
    assert_eq!(
        server.requests().len(),
        1,
        "no second download: {:?}",
        server.requests()
    );
}

#[test]
fn a_tag_resolves_to_the_full_sha_it_points_at() {
    let dir = TestDir::new("tag");
    let archive = package_tarball(RULE);
    let server = TestServer::serve(move |request| {
        if request.path == "/repos/community/rules/commits/v1.0" {
            Response::json(&format!("{{\"sha\":\"{SHA_A}\"}}"))
        } else if request.path == format!("/community/rules/tar.gz/{SHA_A}") {
            Response::ok(archive.clone())
        } else {
            Response::not_found()
        }
    });
    let store = store(dir.path(), &server);
    let resolved = store.resolve(&source(), Some("v1.0")).unwrap();
    assert_eq!(resolved, SHA_A);
    let installed = store.install(&source(), Some("v1.0")).unwrap();
    assert_eq!(installed.sha, SHA_A);
    assert_eq!(
        store_entries(dir.path()),
        [format!("community/rules/{SHA_A}")]
    );
}

#[test]
fn an_unchanged_ref_reuses_the_etag_stored_resolution() {
    let dir = TestDir::new("etag");
    let body = format!("{{\"sha\":\"{SHA_A}\"}}");
    let server = TestServer::serve(move |request| {
        if request.path == "/repos/community/rules/commits/main" {
            if request.if_none_match.as_deref() == Some("\"main-etag\"") {
                return Response {
                    status: "304 Not Modified",
                    headers: Vec::new(),
                    body: Vec::new(),
                };
            }
            return Response {
                status: "200 OK",
                headers: vec![("ETag".to_string(), "\"main-etag\"".to_string())],
                body: body.clone().into_bytes(),
            };
        }
        Response::not_found()
    });
    let store = store(dir.path(), &server);
    assert_eq!(store.resolve(&source(), Some("main")).unwrap(), SHA_A);
    assert_eq!(store.resolve(&source(), Some("main")).unwrap(), SHA_A);
    let requests = server.find("/repos/community/rules/commits/main");
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].if_none_match, None);
    assert_eq!(requests[1].if_none_match.as_deref(), Some("\"main-etag\""));
}

#[test]
fn a_remote_check_distinguishes_all_four_states() {
    let dir = TestDir::new("remote-check");
    let server = TestServer::serve(move |request| match request.path.as_str() {
        "/repos/community/rules/commits/main" => {
            Response::json(&format!("{{\"sha\":\"{SHA_A}\"}}"))
        }
        "/repos/community/rules/commits/next" => {
            Response::json(&format!("{{\"sha\":\"{SHA_B}\"}}"))
        }
        "/repos/community/rules/commits/gone" => Response::not_found(),
        _ => Response {
            status: "500 Internal Server Error",
            headers: Vec::new(),
            body: b"boom".to_vec(),
        },
    });
    let store = store(dir.path(), &server);
    match store.check_remote(&source(), Some("main"), SHA_A) {
        RemoteCheck::Unchanged { sha } => assert_eq!(sha, SHA_A),
        other => panic!("expected unchanged, got {}", other.status()),
    }
    match store.check_remote(&source(), Some("next"), SHA_A) {
        RemoteCheck::Updated { sha } => assert_eq!(sha, SHA_B),
        other => panic!("expected updated, got {}", other.status()),
    }
    match store.check_remote(&source(), Some("gone"), SHA_A) {
        RemoteCheck::Unavailable { reason } => {
            assert!(reason.contains("not found"), "{reason}")
        }
        other => panic!("expected unavailable, got {}", other.status()),
    }
    match store.check_remote(&source(), Some("error"), SHA_A) {
        RemoteCheck::Error(error) => assert_eq!(error.code(), "lint-package-remote"),
        other => panic!("expected error, got {}", other.status()),
    }
}

#[test]
fn the_default_ref_uses_the_default_branch_head() {
    let dir = TestDir::new("default-head");
    let archive = package_tarball(RULE);
    let server = TestServer::serve(move |request| {
        if request.path == "/repos/community/rules/commits?per_page=1" {
            Response::json(&format!("[{{\"sha\":\"{SHA_A}\"}}]"))
        } else if request.path == format!("/community/rules/tar.gz/{SHA_A}") {
            Response::ok(archive.clone())
        } else {
            Response::not_found()
        }
    });
    let store = store(dir.path(), &server);
    let installed = store.install(&source(), None).unwrap();
    assert_eq!(installed.sha, SHA_A);
}

#[test]
fn malformed_incompatible_empty_and_duplicate_packages_are_refused() {
    let dir = TestDir::new("invalid");
    let cases: HashMap<&str, Vec<u8>> = [
        (
            "malformed",
            tarball(&[(
                "o-r-bad/rules/bad.yaml",
                b"id: community/bad\nmetadata: []\nmatcher: {}\n",
            )]),
        ),
        (
            "bare-id",
            tarball(&[(
                "o-r-bare/rules/bare.yaml",
                b"id: bare-id\nmetadata:\n  summary: s\n  rationale: r\n  documentation: d\n  known-limits: k\n  tags: []\nmatcher: {}\n",
            )]),
        ),
        (
            "reserved",
            tarball(&[(
                "o-r-wr/rules/wr.yaml",
                b"id: wright/official\nmetadata:\n  summary: s\n  rationale: r\n  documentation: d\n  known-limits: k\n  tags: []\nmatcher: {}\n",
            )]),
        ),
        ("empty", tarball(&[("o-r-e/README.md", b"nothing")])),
        (
            "nested-only",
            tarball(&[(
                "o-r-n/rules/nested/deep.yaml",
                RULE.as_bytes(),
            )]),
        ),
        (
            "duplicate",
            tarball(&[
                ("o-r-d/rules/a.yaml", RULE.as_bytes()),
                ("o-r-d/rules/b.yml", RULE.as_bytes()),
            ]),
        ),
    ]
    .into_iter()
    .collect();
    let server = TestServer::serve(move |request| {
        if let Some(name) = request.path.strip_prefix("/community/rules/tar.gz/") {
            match name {
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" => {
                    return Response::ok(cases["malformed"].clone());
                }
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" => {
                    return Response::ok(cases["bare-id"].clone());
                }
                "cccccccccccccccccccccccccccccccccccccccc" => {
                    return Response::ok(cases["reserved"].clone());
                }
                "dddddddddddddddddddddddddddddddddddddddd" => {
                    return Response::ok(cases["empty"].clone());
                }
                "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee" => {
                    return Response::ok(cases["nested-only"].clone());
                }
                "ffffffffffffffffffffffffffffffffffffffff" => {
                    return Response::ok(cases["duplicate"].clone());
                }
                _ => {}
            }
        }
        Response::not_found()
    });
    let store = store(dir.path(), &server);
    for (sha, expect_file) in [
        (SHA_A, "bad.yaml"),
        (SHA_B, "bare.yaml"),
        ("cccccccccccccccccccccccccccccccccccccccc", "wr.yaml"),
        ("dddddddddddddddddddddddddddddddddddddddd", "rules"),
        ("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee", "rules"),
        ("ffffffffffffffffffffffffffffffffffffffff", "b.yml"),
    ] {
        let error = store.install(&source(), Some(sha)).unwrap_err();
        assert_eq!(error.code(), "lint-package-invalid", "sha {sha}: {error}");
        assert!(
            error.to_string().contains(expect_file),
            "diagnostic names the file ({expect_file}): {error}"
        );
        assert!(
            !store.package_dir(&source(), sha).exists(),
            "no partial store entry for {sha}"
        );
    }
    assert_eq!(store_entries(dir.path()), Vec::<String>::new());
    // A refused install also leaves no staging debris behind.
    let debris: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(".lint-package-")
        })
        .collect();
    assert!(debris.is_empty());
}

#[test]
fn a_missing_repository_ref_or_sha_is_a_not_found_error() {
    let dir = TestDir::new("not-found");
    let server = TestServer::serve(|_| Response::not_found());
    let store = store(dir.path(), &server);
    let error = store.resolve(&source(), Some("v9")).unwrap_err();
    assert_eq!(error.code(), "lint-package-not-found");
    let error = store.install(&source(), Some(SHA_A)).unwrap_err();
    assert_eq!(error.code(), "lint-package-not-found");
    assert_eq!(store_entries(dir.path()), Vec::<String>::new());
}

#[test]
fn a_network_failure_is_an_offline_error() {
    let dir = TestDir::new("offline");
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let dead = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let store = LintPackageStore::new(dir.path())
        .with_api_base(&dead)
        .with_codeload_base(&dead)
        .with_token(None);
    let error = store.resolve(&source(), Some("v1")).unwrap_err();
    assert_eq!(error.code(), "lint-package-offline");
    let error = store.install(&source(), Some(SHA_A)).unwrap_err();
    assert_eq!(error.code(), "lint-package-offline");
    assert_eq!(store_entries(dir.path()), Vec::<String>::new());
}

#[test]
fn a_used_up_rate_limit_is_a_rate_limited_error() {
    let dir = TestDir::new("rate-limit");
    let server = TestServer::serve(|_| Response {
        status: "403 Forbidden",
        headers: vec![
            ("x-ratelimit-remaining".to_string(), "0".to_string()),
            ("x-ratelimit-reset".to_string(), "1893456000".to_string()),
        ],
        body: b"API rate limit exceeded".to_vec(),
    });
    let store = store(dir.path(), &server);
    let error = store.resolve(&source(), Some("v1")).unwrap_err();
    assert_eq!(error.code(), "lint-package-rate-limited");
    assert!(error.to_string().contains("GITHUB_TOKEN"));
    match store.check_remote(&source(), Some("v1"), SHA_A) {
        RemoteCheck::Error(error) => assert_eq!(error.code(), "lint-package-rate-limited"),
        other => panic!("expected error, got {}", other.status()),
    }
}

#[test]
fn the_github_token_reaches_only_the_api_host() {
    let dir = TestDir::new("token");
    let archive = package_tarball(RULE);
    let server = TestServer::serve(move |request| {
        if request.path == "/repos/community/rules/commits/main" {
            Response::json(&format!("{{\"sha\":\"{SHA_A}\"}}"))
        } else if request.path == format!("/community/rules/tar.gz/{SHA_A}") {
            Response::ok(archive.clone())
        } else {
            Response::not_found()
        }
    });
    let store = LintPackageStore::new(dir.path())
        .with_api_base(server.url())
        .with_codeload_base(server.url())
        .with_token(Some("test-token".to_string()));
    store.install(&source(), Some("main")).unwrap();
    let api = server.find("/repos/");
    assert_eq!(
        api[0].authorization.as_deref(),
        Some("Bearer test-token"),
        "the API request carries the token"
    );
    let codeload = server.find("/community/rules/tar.gz/");
    assert_eq!(
        codeload[0].authorization, None,
        "the token is never sent to codeload"
    );
}

#[test]
fn a_damaged_store_entry_is_refused_not_overwritten() {
    let dir = TestDir::new("damaged");
    let broken = dir.path().join("community/rules").join(SHA_A).join("rules");
    std::fs::create_dir_all(&broken).unwrap();
    let server = TestServer::serve(|_| Response::not_found());
    let store = store(dir.path(), &server);
    let error = store.install(&source(), Some(SHA_A)).unwrap_err();
    assert_eq!(error.code(), "lint-package-install");
    assert!(server.requests().is_empty(), "no download was attempted");
}

#[test]
fn source_parsing_accepts_github_spellings_and_rejects_others() {
    let (src, reference) = LintPackageSource::parse("community/rules").unwrap();
    assert_eq!(src.slug(), "community/rules");
    assert_eq!(reference, None);
    let (src, reference) = LintPackageSource::parse("https://github.com/community/rules").unwrap();
    assert_eq!(src.slug(), "community/rules");
    assert_eq!(reference, None);
    let (src, reference) =
        LintPackageSource::parse("https://github.com/community/rules.git").unwrap();
    assert_eq!(src.slug(), "community/rules");
    assert_eq!(reference, None);
    let (src, reference) = LintPackageSource::parse("git@github.com:community/rules.git").unwrap();
    assert_eq!(src.slug(), "community/rules");
    assert_eq!(reference, None);
    let (src, reference) =
        LintPackageSource::parse("https://github.com/community/rules/tree/feat/x").unwrap();
    assert_eq!(src.slug(), "community/rules");
    assert_eq!(reference.as_deref(), Some("feat/x"));
    let (src, reference) =
        LintPackageSource::parse("https://github.com/community/rules/releases/tag/v1.0").unwrap();
    assert_eq!(src.slug(), "community/rules");
    assert_eq!(reference.as_deref(), Some("v1.0"));
    for bad in [
        "community",
        "community/rules/extra",
        "https://gitlab.com/community/rules",
        "https://github.com/",
        "../escape/rules",
    ] {
        assert!(
            LintPackageSource::parse(bad).is_err(),
            "'{bad}' is not a lint package source"
        );
    }
}
