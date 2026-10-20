//! `wright lint package` integration tests (#844): discovery, install,
//! and list. Every remote answer is a recorded response from a localhost
//! HTTP server — the `WRIGHT_LINT_PACKAGE_*` endpoint overrides point the
//! command at it — and every store lives in a per-test temp dir. No test
//! touches real network or real GitHub.

mod support;

use support::*;

#[test]
fn search_lists_the_topic_packages_with_stable_records() {
    let dirs = TestDirs::new("search");
    let server = TestServer::serve(|request| {
        if request.path.starts_with("/search/repositories?") {
            Response::json(&search_page(
                &[
                    ("community/rules", Some("minimum wait rules")),
                    ("team/extra-rules", None),
                ],
                2,
            ))
        } else {
            Response::not_found()
        }
    });
    let output = run(
        &dirs,
        Some(&server),
        &["lint", "package", "search", "--renderer", "plain"],
    );
    let text = transcript(&dirs, &output);
    assert_eq!(
        text,
        "exit 0\n== stdout\n\
         PACKAGE-SEARCH package-search\n\
         summary: packages=2 store=<store>\n\
         package: source=community/rules status=found description=\"minimum wait rules\"\n\
         package: source=team/extra-rules status=found\n\
         next: wright lint package install <owner/repo>\n\
         \n\
         == stderr\n",
        "{text}"
    );
    let requests = server.paths();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].starts_with(
            "/search/repositories?q=topic:wrightkit-lint%20archived:false%20fork:false"
        ),
        "the topic filter applies: {}",
        requests[0]
    );
}

#[test]
fn search_terms_narrow_the_query_and_search_errors_report_their_code() {
    let dirs = TestDirs::new("search-fail");
    let server = TestServer::serve(|request| {
        if request.path.starts_with("/search/repositories?") {
            Response::json(&search_page(&[("community/rules", None)], 1))
        } else {
            Response::not_found()
        }
    });
    let output = run(
        &dirs,
        Some(&server),
        &["lint", "package", "search", "wait", "--renderer", "plain"],
    );
    assert_eq!(output.status.code(), Some(0));
    assert!(
        server.paths()[0].contains("fork:false%20wait&per_page=100"),
        "terms append to the topic query: {}",
        server.paths()[0]
    );

    let limited = TestServer::serve(|_| Response {
        status: "403 Forbidden",
        headers: vec![
            ("x-ratelimit-remaining".to_string(), "0".to_string()),
            ("x-ratelimit-reset".to_string(), "1893456000".to_string()),
        ],
        body: b"API rate limit exceeded".to_vec(),
    });
    let output = run(
        &dirs,
        Some(&limited),
        &["lint", "package", "search", "--renderer", "plain"],
    );
    let text = transcript(&dirs, &output);
    assert!(
        text.contains("ERROR package-search")
            && text.contains("diagnostic: error lint-package-rate-limited"),
        "{text}"
    );
    assert_eq!(output.status.code(), Some(4));
}

#[test]
fn install_fetches_into_the_store_and_never_activates() {
    let dirs = TestDirs::new("install");
    let server = package_server("community", "rules", SHA_A, package_tarball(RULE));
    let output = run(
        &dirs,
        Some(&server),
        &[
            "lint",
            "package",
            "install",
            "community/rules",
            "--renderer",
            "plain",
        ],
    );
    let text = transcript(&dirs, &output);
    assert_eq!(
        text,
        "exit 0\n== stdout\n\
         PASS package-install\n\
         summary: packages=1 store=<store>\n\
         package: source=community/rules status=installed sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa dir=<store>/community/rules/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa rules=community/minimum-wait downloaded=true\n\
         next: wright lint package list\n\
         next: wright lint package outdated\n\
         \n\
         == stderr\n",
        "{text}"
    );
    // The package landed under <owner>/<repo>/<sha>/ and nothing else was
    // written — no project files were created or modified.
    assert_eq!(
        std::fs::read_to_string(
            dirs.root
                .join("community/rules")
                .join(SHA_A)
                .join("rules/rule.yaml")
        )
        .unwrap(),
        RULE
    );
    assert!(
        std::fs::read_dir(&dirs.cwd).unwrap().next().is_none(),
        "install writes only to the store"
    );

    // A second install reuses the intact entry — no second download.
    let again = run(
        &dirs,
        Some(&server),
        &[
            "lint",
            "package",
            "install",
            "community/rules",
            "--renderer",
            "plain",
        ],
    );
    let text = transcript(&dirs, &again);
    assert!(text.contains("downloaded=false"), "{text}");
    assert!(
        !server
            .paths()
            .iter()
            .any(|p| p.contains("tar.gz") && server.paths().iter().filter(|q| *q == p).count() > 1),
        "one archive fetch total: {:?}",
        server.paths()
    );
}

#[test]
fn install_accepts_ref_spellings_and_a_full_sha() {
    let dirs = TestDirs::new("install-refs");
    let server = package_server("community", "rules", SHA_A, package_tarball(RULE));

    // `owner/repo@<ref>` resolves through the commits API.
    let output = run(
        &dirs,
        Some(&server),
        &[
            "lint",
            "package",
            "install",
            "community/rules@v1",
            "--renderer",
            "plain",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        transcript(&dirs, &output)
    );
    assert!(
        server
            .paths()
            .contains(&"/repos/community/rules/commits/v1".to_string())
    );

    // A full SHA needs no resolution at all.
    let dirs = TestDirs::new("install-refs-sha");
    let server = package_server("community", "rules", SHA_B, package_tarball(RULE));
    let output = run(
        &dirs,
        Some(&server),
        &[
            "lint",
            "package",
            "install",
            &format!("community/rules@{SHA_B}"),
            "--renderer",
            "plain",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        transcript(&dirs, &output)
    );
    assert!(
        !server.paths().iter().any(|p| p.starts_with("/repos/")),
        "a full SHA skips the commits API: {:?}",
        server.paths()
    );

    // A URL source selects the repository; --ref selects the revision.
    let dirs = TestDirs::new("install-refs-url");
    let server = package_server("community", "rules", SHA_A, package_tarball(RULE));
    let output = run(
        &dirs,
        Some(&server),
        &[
            "lint",
            "package",
            "install",
            "https://github.com/community/rules",
            "--ref",
            "v1",
            "--renderer",
            "plain",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        transcript(&dirs, &output)
    );
}

#[test]
fn install_failures_report_stable_error_codes() {
    let dirs = TestDirs::new("install-errors");
    let gone = TestServer::serve(|_| Response::not_found());
    let output = run(
        &dirs,
        Some(&gone),
        &[
            "lint",
            "package",
            "install",
            "community/rules",
            "--renderer",
            "plain",
        ],
    );
    let text = transcript(&dirs, &output);
    assert!(
        text.contains("ERROR package-install")
            && text.contains("diagnostic: error lint-package-not-found"),
        "{text}"
    );
    assert_eq!(output.status.code(), Some(4));

    let malformed = tarball(&[("o-r-bad/rules/bad.yaml", b"id: bad\nmetadata: []\n")]);
    let server = package_server("community", "rules", SHA_A, malformed);
    let output = run(
        &dirs,
        Some(&server),
        &[
            "lint",
            "package",
            "install",
            "community/rules",
            "--renderer",
            "plain",
        ],
    );
    let text = transcript(&dirs, &output);
    assert!(
        text.contains("ERROR package-install")
            && text.contains("diagnostic: error lint-package-invalid"),
        "{text}"
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(
        !entry(&dirs.root, "community", "rules", SHA_A).exists(),
        "a refused package leaves no partial store entry"
    );

    // A malformed source is a usage error, not a remote failure.
    let output = run(
        &dirs,
        Some(&server),
        &[
            "lint",
            "package",
            "install",
            "gitlab.com/community/rules",
            "--renderer",
            "plain",
        ],
    );
    let text = transcript(&dirs, &output);
    assert!(
        text.contains("USAGE package-install")
            && text.contains("diagnostic: error lint-package-source"),
        "{text}"
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn list_reports_installed_and_damaged_entries() {
    let dirs = TestDirs::new("list");
    write_entry(&dirs.root, "community", "rules", SHA_A, RULE);
    // A damaged entry reports `damaged` with the reason it fails validation.
    let damaged = entry(&dirs.root, "team", "pkg", SHA_B).join("rules");
    std::fs::create_dir_all(&damaged).unwrap();
    std::fs::write(damaged.join("bad.yaml"), b"id: bad\nmetadata: []\n").unwrap();
    let output = run(
        &dirs,
        None,
        &["lint", "package", "list", "--renderer", "plain"],
    );
    let text = transcript(&dirs, &output);
    assert!(text.contains("PACKAGE-LIST package-list"), "{text}");
    assert!(
        text.contains("summary: packages=2 installed=1 damaged=1 store=<store>"),
        "{text}"
    );
    assert!(
        text.contains("package: source=community/rules status=installed sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa dir=<store>/community/rules/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa rules=community/minimum-wait downloaded=false"),
        "{text}"
    );
    assert!(
        text.contains("package: source=team/pkg status=damaged")
            && text.contains("code=lint-package-invalid"),
        "{text}"
    );
}
