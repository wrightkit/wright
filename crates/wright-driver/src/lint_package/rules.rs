//! The package content half: extract only `rules/*.yaml|yml` from a GitHub
//! tarball and validate it through the existing SPEC-309 declarative rule
//! loader. Repository code is never executed and never even unpacked.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;
use wright_analyzer::registry::{LintConfig, LintRegistry};

use super::LintPackageError;

const MAX_RULE_BYTES: u64 = 1024 * 1024;

/// Extract only `rules/*.yaml|yml` entries — files directly inside `rules/`,
/// non-recursively — into `staging/rules/`. Every other archive member is
/// ignored.
pub(super) fn extract_rules(archive: &[u8], staging: &Path) -> Result<(), LintPackageError> {
    let rules = staging.join("rules");
    std::fs::create_dir_all(&rules).map_err(|e| {
        LintPackageError::install(format!(
            "cannot create staged lint package rules '{}': {e}",
            rules.display()
        ))
    })?;
    let decoder = GzDecoder::new(archive);
    let mut tar = tar::Archive::new(decoder);
    let entries = tar.entries().map_err(|error| {
        LintPackageError::remote(format!("cannot read the lint package archive: {error}"))
    })?;
    for entry in entries {
        let mut entry = entry.map_err(|error| {
            LintPackageError::remote(format!("cannot read a lint package archive entry: {error}"))
        })?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let output = {
            let path = entry.path().map_err(|error| {
                LintPackageError::remote(format!(
                    "cannot inspect a lint package archive path: {error}"
                ))
            })?;
            rule_file_name(&path).map(|name| rules.join(name))
        };
        let Some(output) = output else {
            continue;
        };
        let mut file = std::fs::File::create(&output).map_err(|error| {
            LintPackageError::install(format!(
                "cannot create staged lint rule '{}': {error}",
                output.display()
            ))
        })?;
        let copied = std::io::copy(&mut (&mut entry).take(MAX_RULE_BYTES + 1), &mut file).map_err(
            |error| {
                LintPackageError::install(format!(
                    "cannot unpack staged lint rule '{}': {error}",
                    output.display()
                ))
            },
        )?;
        if copied > MAX_RULE_BYTES {
            return Err(LintPackageError::invalid(
                &output,
                "rule file exceeds the 1 MiB limit",
            ));
        }
    }
    Ok(())
}

/// The file name of a `<top>/rules/<name>.yaml|yml` or `rules/<name>.yaml|yml`
/// archive member — exactly three (or two) normal path components, the
/// second (or first) of which is `rules`.
fn rule_file_name(path: &Path) -> Option<&std::ffi::OsStr> {
    let mut components = path.components();
    let file = match (
        components.next(),
        components.next(),
        components.next(),
        components.next(),
    ) {
        (Some(Component::Normal(rules)), Some(Component::Normal(name)), None, None)
            if rules == "rules" =>
        {
            name
        }
        (
            Some(Component::Normal(_top)),
            Some(Component::Normal(rules)),
            Some(Component::Normal(name)),
            None,
        ) if rules == "rules" => name,
        _ => return None,
    };
    matches!(
        Path::new(file).extension().and_then(|ext| ext.to_str()),
        Some("yaml" | "yml")
    )
    .then_some(file)
}

/// `*.yaml|yml` files directly inside `rules_dir`, in lexical order — the
/// same file set the `--rule-file <dir>` loader consumes.
fn rule_files(rules_dir: &Path) -> Result<Vec<PathBuf>, LintPackageError> {
    let mut files = std::fs::read_dir(rules_dir)
        .map_err(|e| {
            LintPackageError::invalid(
                rules_dir,
                format!("cannot read the lint package rules directory: {e}"),
            )
        })?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("yaml" | "yml")
                )
        })
        .collect::<Vec<_>>();
    files.sort();
    if files.is_empty() {
        return Err(LintPackageError::invalid(
            rules_dir,
            "the lint package has no rules/*.yaml or rules/*.yml files",
        ));
    }
    Ok(files)
}

/// A store entry that is present and still shaped like a package.
pub(super) fn intact_package(dir: &Path) -> bool {
    dir.is_dir() && rule_files(&dir.join("rules")).is_ok()
}

/// Validate a stored package directory before reuse: an existing entry must
/// still be a complete, loadable package.
pub(super) fn validate_stored(
    dir: &Path,
    first_party: bool,
) -> Result<Vec<String>, LintPackageError> {
    if !intact_package(dir) {
        return Err(LintPackageError::install(format!(
            "the lint package store entry '{}' is incomplete or damaged; remove it and retry",
            dir.display()
        )));
    }
    validate_package(&dir.join("rules"), first_party)
}

/// Load every rule file through the existing SPEC-309 loader — the
/// first-party package under its bare-ID contract, every other package
/// under the external namespaced contract (issue #843). The first failure
/// is reported as a `lint-package-invalid` diagnostic naming the file; a
/// passing package returns its declarative rule IDs in load order.
pub(super) fn validate_package(
    rules_dir: &Path,
    first_party: bool,
) -> Result<Vec<String>, LintPackageError> {
    let files = rule_files(rules_dir)?;
    let mut registry = LintRegistry::default();
    for file in &files {
        let input = std::fs::read_to_string(file).map_err(|e| {
            LintPackageError::invalid(file, format!("cannot read the rule file: {e}"))
        })?;
        let loaded = if first_party {
            registry.load_first_party_yaml_str(&input)
        } else {
            registry.load_yaml_str(&input)
        };
        loaded.map_err(|error| LintPackageError::invalid(file, error))?;
    }
    Ok(registry
        .descriptors(&LintConfig::default())
        .into_iter()
        .filter(|descriptor| descriptor.kind == "declarative")
        .map(|descriptor| descriptor.id)
        .collect())
}
