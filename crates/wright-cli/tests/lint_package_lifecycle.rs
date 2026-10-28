mod support;

use support::*;

#[test]
fn outdated_reports_every_state_without_mutating_the_store() {
    let dirs = TestDirs::new("outdated");
    write_entry(&dirs.root, "community", "rules", SHA_A, RULE);
    write_entry(&dirs.root, "team", "moved", SHA_A, RULE);
    write_entry(&dirs.root, "gone", "pkg", SHA_A, RULE);
    let damaged = entry(&dirs.root, "bad", "entry", SHA_A).join("rules");
    std::fs::create_dir_all(&damaged).unwrap();
    let server = TestServer::serve(|request| match request.path.as_str() {
        "/repos/community/rules/commits?per_page=1" => {
            Response::json(&format!("[{{\"sha\":\"{SHA_A}\"}}]"))
        }
        "/repos/team/moved/commits?per_page=1" => {
            Response::json(&format!("[{{\"sha\":\"{SHA_B}\"}}]"))
        }
        _ => Response::not_found(),
    });
    let output = run(
        &dirs,
        Some(&server),
        &["lint", "package", "outdated", "--renderer", "plain"],
    );
    let text = transcript(&dirs, &output);
    assert!(text.contains("PACKAGE-OUTDATED package-outdated"), "{text}");
    assert!(
        text.contains(
            "summary: packages=4 unchanged=1 updated=1 unavailable=1 error=1 store=<store>"
        ),
        "{text}"
    );
    assert!(
        text.contains("package: source=bad/entry status=error"),
        "the damaged entry is an error row, unchecked: {text}"
    );
    assert!(
        text.contains("package: source=community/rules status=unchanged")
            && text.contains(&format!("remote={SHA_A}")),
        "{text}"
    );
    assert!(
        text.contains("package: source=gone/pkg status=unavailable"),
        "{text}"
    );
    assert!(
        text.contains("package: source=team/moved status=updated")
            && text.contains(&format!("remote={SHA_B}")),
        "{text}"
    );
    assert!(
        !server.paths().iter().any(|p| p.contains("tar.gz")),
        "outdated never fetches: {:?}",
        server.paths()
    );
    assert!(
        entry(&dirs.root, "team", "moved", SHA_A)
            .join("rules/rule.yaml")
            .exists()
    );
    assert!(!entry(&dirs.root, "team", "moved", SHA_B).exists());
}

#[test]
fn update_installs_the_new_head_and_keeps_the_old_entry() {
    let dirs = TestDirs::new("update");
    write_entry(&dirs.root, "community", "rules", SHA_A, RULE);
    write_entry(&dirs.root, "team", "current", SHA_A, RULE);
    let archive = package_tarball(RULE);
    let server = TestServer::serve(move |request| match request.path.as_str() {
        "/repos/community/rules/commits?per_page=1" => {
            Response::json(&format!("[{{\"sha\":\"{SHA_B}\"}}]"))
        }
        "/repos/team/current/commits?per_page=1" => {
            Response::json(&format!("[{{\"sha\":\"{SHA_A}\"}}]"))
        }
        p if p == format!("/community/rules/tar.gz/{SHA_B}") => Response::ok(archive.clone()),
        _ => Response::not_found(),
    });
    let output = run(
        &dirs,
        Some(&server),
        &["lint", "package", "update", "--renderer", "plain"],
    );
    let text = transcript(&dirs, &output);
    assert!(text.contains("PASS package-update"), "{text}");
    assert!(
        text.contains("package: source=community/rules status=updated")
            && text.contains(&format!("sha={SHA_A}"))
            && text.contains(&format!("remote={SHA_B}"))
            && text.contains(&format!("dir=<store>/community/rules/{SHA_B}")),
        "{text}"
    );
    assert!(
        text.contains("package: source=team/current status=unchanged"),
        "{text}"
    );
    assert!(
        entry(&dirs.root, "community", "rules", SHA_A)
            .join("rules/rule.yaml")
            .exists()
    );
    assert!(
        entry(&dirs.root, "community", "rules", SHA_B)
            .join("rules/rule.yaml")
            .exists()
    );
    assert_eq!(
        server
            .paths()
            .iter()
            .filter(|p| p.contains("tar.gz"))
            .count(),
        1,
        "only the moved package was fetched: {:?}",
        server.paths()
    );
}

#[test]
fn update_of_one_source_updates_only_that_repository() {
    let dirs = TestDirs::new("update-source");
    write_entry(&dirs.root, "community", "rules", SHA_A, RULE);
    write_entry(&dirs.root, "team", "other", SHA_A, RULE);
    let archive = package_tarball(RULE);
    let server = TestServer::serve(move |request| match request.path.as_str() {
        "/repos/community/rules/commits?per_page=1" => {
            Response::json(&format!("[{{\"sha\":\"{SHA_B}\"}}]"))
        }
        p if p == format!("/community/rules/tar.gz/{SHA_B}") => Response::ok(archive.clone()),
        _ => Response::not_found(),
    });
    let output = run(
        &dirs,
        Some(&server),
        &[
            "lint",
            "package",
            "update",
            "community/rules",
            "--renderer",
            "plain",
        ],
    );
    let text = transcript(&dirs, &output);
    assert!(text.contains("packages=1"), "{text}");
    assert!(
        !server.paths().iter().any(|p| p.contains("/repos/team/")),
        "{text}"
    );
    let output = run(
        &dirs,
        Some(&server),
        &[
            "lint",
            "package",
            "update",
            "missing/pkg",
            "--renderer",
            "plain",
        ],
    );
    let text = transcript(&dirs, &output);
    assert!(
        text.contains("ERROR package-update")
            && text.contains("diagnostic: error lint-package-not-found"),
        "{text}"
    );
    assert_eq!(output.status.code(), Some(4));
}

#[test]
fn update_fails_on_row_errors_and_refuses_damaged_entries() {
    let dirs = TestDirs::new("update-fail");
    write_entry(&dirs.root, "gone", "pkg", SHA_A, RULE);
    let damaged = write_entry(&dirs.root, "bad", "entry", SHA_A, RULE);
    std::fs::write(damaged.join("rules/rule.yaml"), b"locally edited").unwrap();
    let server = TestServer::serve(|_| Response::not_found());
    let output = run(
        &dirs,
        Some(&server),
        &["lint", "package", "update", "--renderer", "plain"],
    );
    let text = transcript(&dirs, &output);
    assert!(text.contains("FAIL package-update"), "{text}");
    assert!(
        text.contains("package: source=bad/entry status=error")
            && text.contains("code=lint-package-invalid"),
        "{text}"
    );
    assert!(
        text.contains("package: source=gone/pkg status=unavailable"),
        "{text}"
    );
    assert!(
        text.contains("diagnostic: error lint-package-invalid")
            && text.contains("diagnostic: error lint-package-not-found"),
        "{text}"
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        std::fs::read_to_string(damaged.join("rules/rule.yaml")).unwrap(),
        "locally edited"
    );
}

#[test]
fn remove_at_ref_removes_only_the_matching_entry() {
    let dirs = TestDirs::new("remove-ref");
    write_entry(&dirs.root, "community", "rules", SHA_A, RULE);
    write_entry(&dirs.root, "community", "rules", SHA_B, RULE);
    let output = run(
        &dirs,
        None,
        &[
            "lint",
            "package",
            "remove",
            &format!("community/rules@{SHA_A}"),
            "--renderer",
            "plain",
        ],
    );
    let text = transcript(&dirs, &output);
    assert!(text.contains("packages=1"), "{text}");
    assert!(!entry(&dirs.root, "community", "rules", SHA_A).exists());
    assert!(entry(&dirs.root, "community", "rules", SHA_B).exists());
}

#[test]
fn the_json_envelope_is_machine_readable() {
    let dirs = TestDirs::new("json");
    let server = package_server("community", "rules", SHA_A, package_tarball(RULE));
    let output = run(
        &dirs,
        Some(&server),
        &[
            "lint",
            "package",
            "install",
            "community/rules",
            "-f",
            "json",
        ],
    );
    assert_eq!(output.status.code(), Some(0));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON envelope");
    assert_eq!(value["command"], "package-install");
    assert_eq!(value["ok"], true);
    assert_eq!(value["exit"], 0);
    assert_eq!(value["wright"]["contract"], "wright-result/v1");
    let row = &value["result"]["packages"][0];
    assert_eq!(row["source"], "community/rules");
    assert_eq!(row["status"], "installed");
    assert_eq!(row["sha"], SHA_A);
    assert_eq!(row["downloaded"], true);
    assert_eq!(row["rules"], serde_json::json!(["community/minimum-wait"]));
    assert!(row["dir"].as_str().unwrap().contains("community/rules/"));
    assert_eq!(
        value["result"]["store"],
        dirs.root.display().to_string().as_str()
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(!text.contains("WRIGHT_LINT_PACKAGE_API_BASE"));
}
