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
fn list_reports_installed_and_damaged_entries() {
    let dirs = TestDirs::new("list");
    write_entry(&dirs.root, "community", "rules", SHA_A, RULE);
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
