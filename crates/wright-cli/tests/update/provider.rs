use std::collections::HashMap;

mod support;

use support::*;

// The fake provider is a script, which Windows cannot run as `opy-provider.exe`.
#[cfg(unix)]
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
    assert_eq!(
        stdout,
        format!(
            "==> warming up opy provider\ninstalled opy provider {PROVIDER_RELEASE} at {}\n",
            executable.display()
        )
    );
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    let log = read(&warmup_log(&dir));
    assert_eq!(
        String::from_utf8_lossy(&log),
        format!("active={PROVIDER_RELEASE}\nlpp/initialize\nlpp/shutdown\n")
    );

    let output = run_update(&dir, &["update", "provider", "opy"], &provider_env(&server));
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("opy provider {PROVIDER_RELEASE} is already up to date\n")
    );
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    assert_eq!(
        read(&warmup_log(&dir)),
        log,
        "no warm-up for a no-op update"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// The fake provider is a script, which Windows cannot run as `opy-provider.exe`.
#[cfg(unix)]
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
    assert_eq!(
        std::fs::read_to_string(warmup_log(&dir)).unwrap(),
        format!("active={PROVIDER_RELEASE}\nlpp/initialize\nlpp/shutdown\n")
    );
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
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
    assert!(
        !warmup_log(&dir).exists(),
        "--check must not run the provider"
    );
    assert!(!stdout.contains("warming up"), "{stdout}");
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
    assert!(!warmup_log(&dir).exists());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("warming up"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn failed_provider_checksum_does_not_warm_up_or_change_the_install() {
    if provider_target().is_none() {
        return;
    }
    let dir = install_dir("provider-checksum-fails");
    seed_provider(&dir, PROVIDER_OLD);
    let mut files = HashMap::new();
    provider_release(PROVIDER_RELEASE, &mut files);
    for (path, body) in &mut files {
        if path.ends_with(".sha256") {
            *body = format!("{}\n", "0".repeat(64)).into_bytes();
        }
    }
    let server = MockServer::new(files);
    let output = run_update(&dir, &["update", "provider", "opy"], &provider_env(&server));
    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty(), "{:?}", output.stdout);
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("provider-integrity"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let store = provider_store(&dir);
    assert_eq!(
        std::fs::read_to_string(store.join("active"))
            .unwrap()
            .trim(),
        PROVIDER_OLD
    );
    assert!(!store.join(PROVIDER_RELEASE).exists());
    assert!(!warmup_log(&dir).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

// The fake provider is a script, which Windows cannot run as `opy-provider.exe`.
#[cfg(unix)]
#[test]
fn a_provider_exiting_during_warmup_warns_and_leaves_the_install_active() {
    if provider_target().is_none() {
        return;
    }
    let dir = install_dir("provider-warmup-exits");
    seed_provider(&dir, PROVIDER_OLD);
    let mut files = HashMap::new();
    provider_release(PROVIDER_RELEASE, &mut files);
    let server = MockServer::new(files);
    let mut env = provider_env(&server);
    env.push(("WRIGHT_TEST_PROVIDER_WARMUP", "exit".to_string()));
    let output = run_update(&dir, &["update", "provider", "opy"], &env);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!(
            "==> warming up opy provider\nupdated opy provider {PROVIDER_OLD} -> {PROVIDER_RELEASE}\n"
        )
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("warning:"), "{stderr}");
    assert!(
        stderr.contains("opy provider") && stderr.contains("23"),
        "{stderr}"
    );
    let store = provider_store(&dir);
    assert_eq!(
        std::fs::read_to_string(store.join("active"))
            .unwrap()
            .trim(),
        PROVIDER_RELEASE
    );
    assert_eq!(
        read(
            &store
                .join(PROVIDER_RELEASE)
                .join(provider_target().unwrap())
                .join(provider_binary())
        ),
        provider_body(PROVIDER_RELEASE)
    );
    assert_eq!(
        std::fs::read_to_string(warmup_log(&dir)).unwrap(),
        format!("active={PROVIDER_RELEASE}\n")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// The fake provider is a script, which Windows cannot run as `opy-provider.exe`.
#[cfg(unix)]
#[test]
fn failed_or_slow_handshakes_leave_the_install_active_without_a_warning() {
    if provider_target().is_none() {
        return;
    }
    for mode in ["malformed", "timeout", "shutdown-timeout"] {
        let dir = install_dir(&format!("provider-warmup-{mode}"));
        let mut files = HashMap::new();
        provider_release(PROVIDER_RELEASE, &mut files);
        let server = MockServer::new(files);
        let mut env = provider_env(&server);
        env.push(("WRIGHT_TEST_PROVIDER_WARMUP", mode.to_string()));
        let started = std::time::Instant::now();
        let output = run_update(&dir, &["update", "provider", "opy"], &env);
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stderr.is_empty(), "{:?}", output.stderr);
        assert!(started.elapsed() < std::time::Duration::from_secs(30));
        assert_eq!(
            std::fs::read_to_string(provider_store(&dir).join("active"))
                .unwrap()
                .trim(),
            PROVIDER_RELEASE
        );
        let log = std::fs::read_to_string(warmup_log(&dir)).unwrap();
        assert!(
            log.starts_with(&format!("active={PROVIDER_RELEASE}\n")),
            "{log}"
        );
        assert_eq!(
            log.contains("lpp/shutdown"),
            mode == "shutdown-timeout",
            "{log}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
