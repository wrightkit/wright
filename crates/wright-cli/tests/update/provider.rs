use std::collections::HashMap;

mod support;

use support::*;

#[test]
fn update_installs_a_named_provider_into_an_empty_store() {
    if provider_target().is_none() {
        return;
    }
    let dir = install_dir("provider-install");
    let mut files = HashMap::new();
    provider_release(PROVIDER_RELEASE, &mut files);
    let server = MockServer::new(files);
    let output = run_update(&dir, &["update", "provider", "opy"], &provider_env(&server));
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!("installed opy provider {PROVIDER_RELEASE}")),
        "{stdout}"
    );
    let store = provider_store(&dir);
    assert_eq!(
        std::fs::read_to_string(store.join("active"))
            .unwrap()
            .trim(),
        PROVIDER_RELEASE
    );
    let executable = store
        .join(PROVIDER_RELEASE)
        .join(provider_target().unwrap())
        .join(provider_binary());
    assert_eq!(read(&executable), provider_body(PROVIDER_RELEASE));
    assert!(!stdout.contains("installing wright"), "{stdout}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_upgrades_the_installed_provider_alongside_self() {
    if provider_target().is_none() {
        return;
    }
    let dir = install_dir("provider-upgrade");
    seed_provider(&dir, PROVIDER_OLD);
    let mut files = release(RELEASE).files;
    provider_release(PROVIDER_RELEASE, &mut files);
    let server = MockServer::new(files);
    let output = run_update(&dir, &["update"], &provider_env(&server));
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!(
            "updated opy provider {PROVIDER_OLD} -> {PROVIDER_RELEASE}"
        )),
        "{stdout}"
    );
    assert_eq!(
        std::fs::read_to_string(provider_store(&dir).join("active"))
            .unwrap()
            .trim(),
        PROVIDER_RELEASE
    );
    assert!(
        String::from_utf8_lossy(&read(&dir.join("wright")))
            .contains(&format!("fake wright {RELEASE}")),
        "the self target still updated"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_does_not_install_an_absent_provider() {
    let dir = install_dir("provider-absent");
    let server = MockServer::new(release(RELEASE).files);
    let output = run_update(&dir, &["update"], &server.env());
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("opy provider is not installed; skipping"),
        "{stdout}"
    );
    assert!(
        !provider_store(&dir).join("active").exists(),
        "a bare update must not create a provider installation"
    );
    assert!(
        String::from_utf8_lossy(&read(&dir.join("wright")))
            .contains(&format!("fake wright {RELEASE}")),
        "the self target still updated"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn check_reports_provider_availability_without_modifying_anything() {
    if provider_target().is_none() {
        return;
    }
    let dir = install_dir("provider-check");
    seed_provider(&dir, PROVIDER_OLD);
    let mut files = release(RELEASE).files;
    provider_release(PROVIDER_RELEASE, &mut files);
    let server = MockServer::new(files);
    let before = read(&dir.join("wright"));
    let output = run_update(&dir, &["update", "--check"], &provider_env(&server));
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!(
            "update available: opy provider {PROVIDER_OLD} -> {PROVIDER_RELEASE}"
        )),
        "{stdout}"
    );
    assert_eq!(
        std::fs::read_to_string(provider_store(&dir).join("active"))
            .unwrap()
            .trim(),
        PROVIDER_OLD,
        "--check must not touch the active provider"
    );
    assert_eq!(read(&dir.join("wright")), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_provider_without_a_name_skips_an_empty_store() {
    let dir = install_dir("provider-none");
    let output = run_update(&dir, &["update", "provider"], &[]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("opy provider is not installed; skipping"),
        "{stdout}"
    );
    assert!(!provider_store(&dir).join("active").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn package_managed_wright_skips_self_update_but_updates_providers() {
    if provider_target().is_none() {
        return;
    }
    let dir = homebrew_install_dir("managed");
    seed_provider(&dir, PROVIDER_OLD);
    let mut files = HashMap::new();
    provider_release(PROVIDER_RELEASE, &mut files);
    let server = MockServer::new(files);
    let before = read(&dir.join("wright"));
    let output = run_update(&dir, &["update"], &provider_env(&server));
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("skipped wright self-update") && stdout.contains("brew upgrade"),
        "the skip line carries actionable package-manager guidance: {stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "updated opy provider {PROVIDER_OLD} -> {PROVIDER_RELEASE}"
        )),
        "{stdout}"
    );
    assert_eq!(read(&dir.join("wright")), before, "self-update was skipped");
    assert_eq!(
        std::fs::read_to_string(provider_store(&dir).join("active"))
            .unwrap()
            .trim(),
        PROVIDER_RELEASE
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_provider_check_reports_availability_without_installing() {
    if provider_target().is_none() {
        return;
    }
    let dir = install_dir("provider-check-flag");
    seed_provider(&dir, PROVIDER_OLD);
    let mut files = HashMap::new();
    provider_release(PROVIDER_RELEASE, &mut files);
    let server = MockServer::new(files);
    let output = run_update(
        &dir,
        &["update", "provider", "--check"],
        &provider_env(&server),
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(&format!(
            "update available: opy provider {PROVIDER_OLD} -> {PROVIDER_RELEASE}"
        )),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        std::fs::read_to_string(provider_store(&dir).join("active"))
            .unwrap()
            .trim(),
        PROVIDER_OLD,
        "--check must not touch the active provider"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_provider_update_makes_update_exit_nonzero() {
    if provider_target().is_none() {
        return;
    }
    let dir = homebrew_install_dir("provider-fails");
    seed_provider(&dir, PROVIDER_OLD);
    let server = MockServer::new(HashMap::new());
    let output = run_update(&dir, &["update"], &provider_env(&server));
    assert_ne!(
        output.status.code(),
        Some(0),
        "a failed provider update must not report success"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("opy"),
        "stderr names the failing target: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}
