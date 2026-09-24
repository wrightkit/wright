//! Workspace contract: Wright consumes exactly one released or pinned
//! candidate workshop-rs package, through direct, non-renamed dependencies
//! that share one requirement. Hosted here because this crate is a direct
//! workshop-rs consumer covered by both the stable and MSRV test gates.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

const CANDIDATE_REPOSITORY: &str = "git+https://github.com/wrightkit/workshop-rs.git?rev=";

/// Accepts only `git+<workshop-rs>?rev=<sha>#<sha>` where both are the same
/// full 40-character lowercase hex revision.
fn is_pinned_git_candidate(source: Option<&str>) -> bool {
    let Some(pin) = source.and_then(|source| source.strip_prefix(CANDIDATE_REPOSITORY)) else {
        return false;
    };
    let Some((requested, resolved)) = pin.split_once('#') else {
        return false;
    };
    let is_revision = |text: &str| {
        text.len() == 40
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    is_revision(requested) && requested == resolved
}

/// Matches `^MAJOR.MINOR.PATCH` with an optional `-prerelease` suffix.
fn is_compatible_semver_requirement(requirement: &str) -> bool {
    let Some(version) = requirement.strip_prefix('^') else {
        return false;
    };
    let (core, prerelease) = match version.split_once('-') {
        Some((core, prerelease)) => (core, Some(prerelease)),
        None => (version, None),
    };
    let core_ok = {
        let parts: Vec<&str> = core.split('.').collect();
        parts.len() == 3
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    };
    let prerelease_ok = prerelease.is_none_or(|text| {
        !text.is_empty()
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    });
    core_ok && prerelease_ok
}

fn cargo_metadata() -> Value {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .args(["metadata", "--locked", "--format-version", "1"])
        .current_dir(root)
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse cargo metadata")
}

#[test]
fn workspace_consumes_one_released_or_pinned_workshop_rs() {
    let metadata = cargo_metadata();
    let packages = metadata["packages"].as_array().expect("packages");

    let workshop: Vec<&Value> = packages
        .iter()
        .filter(|package| package["name"] == "workshop-rs")
        .collect();
    assert_eq!(
        workshop.len(),
        1,
        "expected exactly one resolved workshop-rs package, found: {:?}",
        workshop
            .iter()
            .map(|package| format!("{} ({})", package["version"], package["source"]))
            .collect::<Vec<_>>()
    );
    let source = workshop[0]["source"].as_str();
    let is_registry = source.is_some_and(|source| source.starts_with("registry+"));
    assert!(
        is_registry || is_pinned_git_candidate(source),
        "workshop-rs must come from a released registry or pinned git candidate, got {}",
        source.unwrap_or("unpublished")
    );

    let members: BTreeSet<&str> = metadata["workspace_members"]
        .as_array()
        .expect("workspace_members")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let direct: Vec<(&str, &Value)> = packages
        .iter()
        .filter(|package| {
            package["id"]
                .as_str()
                .is_some_and(|id| members.contains(id))
        })
        .flat_map(|package| {
            let name = package["name"].as_str().expect("package name");
            package["dependencies"]
                .as_array()
                .expect("dependencies")
                .iter()
                .filter(|dependency| dependency["name"] == "workshop-rs")
                .map(move |dependency| (name, dependency))
        })
        .collect();
    assert!(
        !direct.is_empty(),
        "no workspace package directly consumes workshop-rs"
    );

    let aliases: Vec<String> = direct
        .iter()
        .filter_map(|(package, dependency)| {
            dependency["rename"]
                .as_str()
                .map(|rename| format!("{package}: {rename}"))
        })
        .collect();
    assert!(
        aliases.is_empty(),
        "renamed workshop-rs dependencies are not allowed: {aliases:?}"
    );

    let requirements: BTreeSet<&str> = direct
        .iter()
        .map(|(_, dependency)| dependency["req"].as_str().expect("requirement"))
        .collect();
    let consumers: Vec<String> = direct
        .iter()
        .map(|(package, dependency)| {
            format!("{package} ({})", dependency["req"].as_str().unwrap_or("?"))
        })
        .collect();
    if is_registry {
        assert!(
            requirements.len() == 1
                && requirements
                    .iter()
                    .all(|requirement| is_compatible_semver_requirement(requirement)),
            "direct consumers must use one ordinary compatible SemVer requirement, found {consumers:?}"
        );
    } else {
        assert_eq!(
            requirements,
            BTreeSet::from(["*"]),
            "git candidate direct consumers must use '*', found {consumers:?}"
        );
    }
}

#[test]
fn pinned_git_candidate_requires_exact_full_revision() {
    let revision = "ac5a6a4cf15bfccc5597cfd6ccb7b5028dfd5053";
    let repo = "git+https://github.com/wrightkit/workshop-rs.git";

    assert!(is_pinned_git_candidate(Some(&format!(
        "{repo}?rev={revision}#{revision}"
    ))));

    for source in [
        format!("{repo}#{revision}"),
        format!("{repo}?branch=main#{revision}"),
        format!("{repo}?rev=v1.0.0#{revision}"),
        format!("{repo}?rev={revision}#0000000000000000000000000000000000000000"),
        format!("git+https://github.com/other/workshop-rs.git?rev={revision}#{revision}"),
    ] {
        assert!(!is_pinned_git_candidate(Some(&source)), "accepted {source}");
    }
    assert!(!is_pinned_git_candidate(None));
}

#[test]
fn registry_requirement_must_be_compatible_semver() {
    for requirement in ["^1.0.0", "^0.5.12", "^1.0.0-rc.1"] {
        assert!(
            is_compatible_semver_requirement(requirement),
            "{requirement}"
        );
    }
    for requirement in ["1.0.0", "=1.0.0", "^1.0", "^1.0.0-", ">=1.0.0", "*"] {
        assert!(
            !is_compatible_semver_requirement(requirement),
            "{requirement}"
        );
    }
}
