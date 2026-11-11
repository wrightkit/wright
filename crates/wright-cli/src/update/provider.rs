use std::cmp::Ordering;

use wright_driver::result::exit;
use wright_driver::{OpyProviderConfig, OpyProviderError, ResolvedOpyProvider};

use super::{compare_versions, parse_version, report_update_error};

pub(super) fn provider_target(
    check_only: bool,
    requested: Option<&str>,
    may_install: bool,
    pres: crate::present::Presentation,
) -> u8 {
    let installed = match OpyProviderConfig::default().installed() {
        Ok(installed) => installed,
        Err(error @ OpyProviderError::UnsupportedPlatform(_)) if !may_install => {
            println!("skipped opy provider: {error}");
            return exit::SUCCESS;
        }
        Err(error) => return provider_failure(&error, pres),
    };
    if check_only {
        return provider_check(installed.as_ref(), requested, may_install, pres);
    }
    if installed.is_none() && !may_install {
        println!("opy provider is not installed; skipping");
        return exit::SUCCESS;
    }
    let previous = installed
        .as_ref()
        .and_then(|p| p.version.as_deref())
        .map(str::to_string);
    match OpyProviderConfig::default().update(requested) {
        Ok(resolved) => {
            if previous != resolved.version {
                println!("==> warming up opy provider");
                if let Err(error) = resolved.warm_up()
                    && matches!(error.code(), "provider-spawn" | "provider-exited")
                {
                    eprintln!("warning: could not start opy provider for warm-up: {error}");
                }
            }
            provider_report(previous.as_deref(), &resolved);
            exit::SUCCESS
        }
        Err(error) => provider_failure(&error, pres),
    }
}

fn provider_report(previous: Option<&str>, resolved: &ResolvedOpyProvider) {
    let Some(version) = resolved.version.as_deref() else {
        println!(
            "installed opy provider at {}",
            resolved.executable.display()
        );
        return;
    };
    match previous {
        None => println!(
            "installed opy provider {version} at {}",
            resolved.executable.display()
        ),
        Some(current) => match compare_versions(current, version) {
            Some(Ordering::Equal) => println!("opy provider {version} is already up to date"),
            Some(Ordering::Less) => println!("updated opy provider {current} -> {version}"),
            Some(Ordering::Greater) => println!("downgraded opy provider {current} -> {version}"),
            None => println!("opy provider {version} is active (was {current})"),
        },
    }
}

fn provider_check(
    installed: Option<&ResolvedOpyProvider>,
    requested: Option<&str>,
    may_install: bool,
    pres: crate::present::Presentation,
) -> u8 {
    let current = installed.and_then(|p| p.version.as_deref());
    if current.is_none() && !may_install {
        println!("opy provider is not installed; skipping");
        return exit::SUCCESS;
    }
    let target = match requested {
        Some(version) => match parse_version(version) {
            Ok(_) => version.trim_start_matches('v').to_string(),
            Err(error) => {
                report_update_error(
                    pres,
                    crate::present::terminal::exit_word(error.exit_code()),
                    error.message(),
                    &error.record(),
                );
                return error.exit_code();
            }
        },
        None => match OpyProviderConfig::default().latest_version() {
            Ok(version) => version,
            Err(error) => return provider_failure(&error, pres),
        },
    };
    let Some(current) = current else {
        println!(
            "opy provider is not installed; `wright update provider opy` would install {target}"
        );
        return exit::SUCCESS;
    };
    match compare_versions(current, &target) {
        Some(Ordering::Equal) => println!("opy provider {current} is already up to date"),
        Some(Ordering::Less) => println!(
            "update available: opy provider {current} -> {target} (run `wright update provider opy` to install)"
        ),
        Some(Ordering::Greater) if requested.is_some() => println!(
            "opy provider {target} is older than the installed {current}; `wright update provider opy --version {target}` would downgrade"
        ),
        Some(Ordering::Greater) => println!(
            "installed opy provider {current} is newer than the latest release {target}; keeping {current}"
        ),
        None => println!(
            "installed opy provider {current} is not a recognized release version; latest release is {target}"
        ),
    }
    exit::SUCCESS
}

fn provider_failure(error: &OpyProviderError, pres: crate::present::Presentation) -> u8 {
    report_update_error(
        pres,
        crate::present::terminal::exit_word(error.exit_code()),
        &format!("{}: {}", error.code(), error),
        &format!(
            "error: {} {}",
            error.code(),
            crate::plain::field("message", &error.to_string())
        ),
    );
    error.exit_code()
}
