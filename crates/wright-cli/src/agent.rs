//! `wright agent` — the coding-agent surface that does not need a project
//! parse (#415). `install` writes the canonical Wright agent guide (vendored
//! from `wrightkit/skills`, see `agent-guide/UPSTREAM.md`) into the project's
//! agent skills directory so agents discover Wright's capabilities without
//! npm or a manual copy.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use wright_driver::sha256_hex;

use crate::cli::{AgentArgs, AgentInstallArgs, AgentSubcommand};

mod mcp;

mod exit {
    pub(super) const SUCCESS: u8 = 0;
    pub(super) const USER_ERROR: u8 = 1;
    pub(super) const USAGE: u8 = 2;
    pub(super) const INTERNAL: u8 = 4;
}

const SKILL_NAME: &str = "wright";
const INSTALLED_BY: &str = "wright agent install";
const UPSTREAM: &str = "wrightkit/skills@c1ae8ee";
const DEFAULT_DEST: &str = ".agents/skills";

/// The guide files `install` writes, as `(relative path, content)`.
const FILES: &[(&str, &str)] = &[
    (
        "SKILL.md",
        include_str!("../../../agent-guide/wright/SKILL.md"),
    ),
    (
        "references/language-notes.md",
        include_str!("../../../agent-guide/wright/references/language-notes.md"),
    ),
];

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AgentError {
    Usage(String),
    Rejected(String),
    Failed(String),
}

impl AgentError {
    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            AgentError::Usage(_) => exit::USAGE,
            AgentError::Rejected(_) => exit::USER_ERROR,
            AgentError::Failed(_) => exit::INTERNAL,
        }
    }

    pub(crate) fn message(&self) -> &str {
        match self {
            AgentError::Usage(msg) | AgentError::Rejected(msg) | AgentError::Failed(msg) => msg,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InstallStatus {
    Created(PathBuf),
    Updated(PathBuf),
    UpToDate(PathBuf),
    DryRun(PathBuf),
}

/// The content hash the install record carries: same recipe as the benchmark's
/// `content_hash` (relative path + per-file sha256 over every `*.md`), so an
/// installed guide verifies like a pinned skill. Python's `sorted(Path)` is
/// component-wise, so the key splits on `/` rather than sorting raw strings.
static SKILL_SHA256: LazyLock<String> = LazyLock::new(|| {
    let mut files = FILES.to_vec();
    files.sort_by_key(|(path, _)| path.split('/').collect::<Vec<_>>());
    let mut material = String::new();
    for (path, content) in files {
        material.push_str(path);
        material.push_str(&sha256_hex(content.as_bytes()));
    }
    sha256_hex(material.as_bytes())
});

fn build_record() -> serde_json::Value {
    serde_json::json!({
        "name": SKILL_NAME,
        "installedBy": INSTALLED_BY,
        "wright": crate::CLI_VERSION,
        "upstream": UPSTREAM,
        "skillSha256": *SKILL_SHA256,
    })
}

/// The `BUILD.json` left by `wright agent install`, or None for anything else.
fn wright_record(target: &Path) -> Option<serde_json::Value> {
    let record: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(target.join("BUILD.json")).ok()?).ok()?;
    (record.get("installedBy") == Some(&serde_json::Value::from(INSTALLED_BY))
        && record.get("name") == Some(&serde_json::Value::from(SKILL_NAME)))
    .then_some(record)
}

fn write_files(target: &Path) -> Result<(), AgentError> {
    for (name, content) in FILES {
        let path = target.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                AgentError::Failed(format!("could not create {}: {error}", parent.display()))
            })?;
        }
        std::fs::write(&path, content).map_err(|error| {
            AgentError::Failed(format!("could not write {}: {error}", path.display()))
        })?;
    }
    Ok(())
}

/// A `*.md` file in `target` that the guide does not ship. The bench's content
/// hash covers every `*.md` in the skill directory, so a stray file makes the
/// installed copy diverge even when our files match.
fn has_stray_markdown(target: &Path) -> bool {
    let mut stack = vec![target.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push(entry.path());
            } else if entry.path().extension().is_some_and(|ext| ext == "md")
                && !FILES
                    .iter()
                    .any(|(name, _)| entry.path() == target.join(name))
            {
                return true;
            }
        }
    }
    false
}

fn up_to_date(target: &Path, record: &serde_json::Value) -> bool {
    record.get("skillSha256") == Some(&serde_json::Value::from(SKILL_SHA256.as_str()))
        && FILES.iter().all(|(name, content)| {
            std::fs::read(target.join(name)).is_ok_and(|bytes| bytes == content.as_bytes())
        })
        && !has_stray_markdown(target)
}

pub(crate) fn install(args: &AgentInstallArgs) -> Result<InstallStatus, AgentError> {
    let target = args
        .dest
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DEST))
        .join(SKILL_NAME);
    let record = wright_record(&target);
    // `symlink_metadata` so a dangling symlink at the destination still counts
    // as occupied (foreign refusal / force removal) instead of falling through
    // to a confusing `create_dir_all` failure.
    let present = target.symlink_metadata().is_ok();
    if present && record.is_none() && !args.force {
        return Err(AgentError::Rejected(format!(
            "{} exists and was not installed by `{INSTALLED_BY}`; remove it or pass --force to replace it",
            target.display()
        )));
    }
    if let Some(record) = &record {
        if up_to_date(&target, record) {
            return Ok(InstallStatus::UpToDate(target));
        }
    }
    if args.dry_run {
        return Ok(InstallStatus::DryRun(target));
    }
    let updating = present;
    if updating {
        let cleared = if target.is_dir() {
            std::fs::remove_dir_all(&target)
        } else {
            std::fs::remove_file(&target)
        };
        cleared.map_err(|error| {
            AgentError::Failed(format!("could not clear {}: {error}", target.display()))
        })?;
    }
    std::fs::create_dir_all(&target).map_err(|error| {
        AgentError::Failed(format!("could not create {}: {error}", target.display()))
    })?;
    // The record lands first so any partial state still counts as ours and the
    // next run refreshes instead of refusing as foreign.
    let record_path = target.join("BUILD.json");
    std::fs::write(
        &record_path,
        serde_json::to_string_pretty(&build_record()).expect("record serializes") + "\n",
    )
    .map_err(|error| {
        AgentError::Failed(format!(
            "could not write {}: {error}",
            record_path.display()
        ))
    })?;
    write_files(&target)?;
    Ok(if updating {
        InstallStatus::Updated(target)
    } else {
        InstallStatus::Created(target)
    })
}

fn report_guide(status: &InstallStatus) {
    match status {
        InstallStatus::Created(path) | InstallStatus::Updated(path) => {
            let verb = if matches!(status, InstallStatus::Created(_)) {
                "installed"
            } else {
                "updated"
            };
            println!("==> {verb} the Wright agent guide in {}", path.display());
            println!("remove that directory to uninstall; re-run `wright agent install` to update");
        }
        InstallStatus::UpToDate(path) => {
            println!(
                "the Wright agent guide in {} is already up to date",
                path.display()
            );
        }
        InstallStatus::DryRun(path) => {
            println!("would install the Wright agent guide to {}", path.display());
        }
    }
}

fn report_mcp(status: &mcp::McpStatus) {
    match status {
        mcp::McpStatus::Created(path) => {
            println!("==> configured the Wright MCP server in {}", path.display());
            println!("delete its `wright` entry to remove it");
        }
        mcp::McpStatus::Replaced(path) => {
            println!(
                "==> replaced the Wright MCP server entry in {}",
                path.display()
            );
        }
        mcp::McpStatus::UpToDate(path) => {
            println!(
                "the Wright MCP server in {} is already up to date",
                path.display()
            );
        }
        mcp::McpStatus::DryRun(path) => {
            println!(
                "would configure the Wright MCP server in {}",
                path.display()
            );
        }
    }
}

pub(crate) fn run(args: &AgentArgs) -> Result<u8, AgentError> {
    match &args.subcommand {
        Some(AgentSubcommand::Install(install_args)) => {
            if !install_args.no_guide {
                report_guide(&install(install_args)?);
            }
            if let Some(target) = install_args.mcp {
                report_mcp(&mcp::install(
                    Path::new("."),
                    target,
                    install_args.force,
                    install_args.dry_run,
                )?);
            }
            Ok(exit::SUCCESS)
        }
        Some(AgentSubcommand::Tools(tools_args)) => {
            let document = match tools_args.format {
                crate::cli::ToolsFormat::Messages => crate::agenttools::messages_tools(),
                crate::cli::ToolsFormat::JsonSchema => crate::agenttools::json_schema_tools(),
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&document).expect("tool document serializes")
            );
            Ok(exit::SUCCESS)
        }
        None => Err(AgentError::Usage(
            "specify an agent command (run `wright agent --help` for details)".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(dest: Option<PathBuf>, force: bool, dry_run: bool) -> AgentInstallArgs {
        AgentInstallArgs {
            dest,
            force,
            dry_run,
            mcp: None,
            no_guide: false,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wright-agent-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn installs_the_vendored_guide_and_refreshes_it() {
        let root = scratch("fresh");
        let dest = Some(root.clone());
        let target = root.join("wright");
        assert_eq!(
            install(&args(dest.clone(), false, false)),
            Ok(InstallStatus::Created(target.clone()))
        );
        for (name, content) in FILES {
            assert_eq!(
                std::fs::read_to_string(target.join(name)).unwrap(),
                *content
            );
        }
        assert_eq!(
            install(&args(dest.clone(), false, false)),
            Ok(InstallStatus::UpToDate(target.clone()))
        );
        std::fs::write(target.join("SKILL.md"), "stale").unwrap();
        assert_eq!(
            install(&args(dest, false, false)),
            Ok(InstallStatus::Updated(target.clone()))
        );
        assert_eq!(
            std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
            FILES[0].1
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_a_foreign_directory_without_force() {
        let root = scratch("foreign");
        let target = root.join("wright");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("SKILL.md"), "not ours").unwrap();
        // A BUILD.json without the installer's identity is still foreign.
        std::fs::write(target.join("BUILD.json"), r#"{"name":"other"}"#).unwrap();
        assert_eq!(
            install(&args(Some(root.clone()), false, false)),
            Err(AgentError::Rejected(format!(
                "{} exists and was not installed by `{INSTALLED_BY}`; remove it or pass --force to replace it",
                target.display()
            )))
        );
        assert_eq!(
            install(&args(Some(root.clone()), true, false)),
            Ok(InstallStatus::Updated(target.clone()))
        );
        assert_eq!(
            std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
            FILES[0].1
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn force_replaces_a_plain_file_and_refresh_clears_strays() {
        let root = scratch("stray");
        let target = root.join("wright");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&target, "not ours").unwrap();
        assert_eq!(
            install(&args(Some(root.clone()), true, false)),
            Ok(InstallStatus::Updated(target.clone()))
        );
        assert!(target.is_dir());
        // A stray guide markdown file makes the copy diverge: refresh removes it.
        let stray = target.join("references/notes.md");
        std::fs::write(&stray, "user note").unwrap();
        assert_eq!(
            install(&args(Some(root.clone()), false, false)),
            Ok(InstallStatus::Updated(target.clone()))
        );
        assert!(!stray.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dry_run_reports_without_writing_and_bare_command_is_usage() {
        let root = scratch("dry");
        let target = root.join("wright");
        assert_eq!(
            install(&args(Some(root.clone()), false, true)),
            Ok(InstallStatus::DryRun(target.clone()))
        );
        assert!(!target.exists());
        // A foreign destination refuses even under --dry-run.
        std::fs::create_dir_all(&target).unwrap();
        assert_eq!(
            install(&args(Some(root.clone()), false, true)).map_err(|e| e.exit_code()),
            Err(exit::USER_ERROR)
        );
        assert_eq!(
            run(&AgentArgs { subcommand: None }).map_err(|e| e.exit_code()),
            Err(exit::USAGE)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn record_identifies_the_guide_as_wright_installed() {
        let record = build_record();
        assert_eq!(record["name"], "wright");
        assert_eq!(record["installedBy"], INSTALLED_BY);
        assert_eq!(record["skillSha256"].as_str().unwrap().len(), 64);
        assert!(
            FILES[0].1.starts_with("---\nname: wright\n"),
            "the embedded guide keeps its skill identity"
        );
    }
}
