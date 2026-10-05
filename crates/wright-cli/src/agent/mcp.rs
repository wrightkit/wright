//! `wright agent mcp` — register Wright's own MCP server in a coding-agent
//! harness's project-local config (#509). Wright writes only the minimum
//! Wright-owned entry (`wright serve --transport mcp`); the harness starts the
//! stdio process itself, and unrelated configuration is left untouched.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::AgentError;
use crate::cli::{AgentMcpArgs, McpTarget};

const SERVER_NAME: &str = "wright";
const COMMAND: &str = "wright";
const SERVE_ARGS: [&str; 3] = ["serve", "--transport", "mcp"];

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum McpStatus {
    Created(PathBuf),
    Updated(PathBuf),
    UpToDate(PathBuf),
    Removed(PathBuf),
    NotConfigured(PathBuf),
    DryRun(PathBuf),
}

struct Layout {
    file: &'static str,
    servers_key: &'static str,
    typed: bool,
    /// The harness's workspace-root variable, used as the session input when
    /// the harness does not start servers in the project root.
    input: Option<&'static str>,
}

impl McpTarget {
    fn layout(self) -> Layout {
        match self {
            McpTarget::Claude => Layout {
                file: ".mcp.json",
                servers_key: "mcpServers",
                typed: true,
                input: None,
            },
            McpTarget::Cursor => Layout {
                file: ".cursor/mcp.json",
                servers_key: "mcpServers",
                typed: false,
                input: Some("${workspaceFolder}"),
            },
            McpTarget::Vscode => Layout {
                file: ".vscode/mcp.json",
                servers_key: "servers",
                typed: true,
                input: Some("${workspaceFolder}"),
            },
        }
    }
}

fn desired_entry(layout: &Layout) -> Value {
    let mut args: Vec<&str> = SERVE_ARGS.to_vec();
    args.extend(layout.input);
    let mut entry = Map::new();
    if layout.typed {
        entry.insert("type".into(), json!("stdio"));
    }
    entry.insert("command".into(), json!(COMMAND));
    entry.insert("args".into(), json!(args));
    Value::Object(entry)
}

/// A server entry that runs `wright serve --transport mcp`, whatever its key.
fn is_wright_owned(entry: &Value) -> bool {
    entry.get("command") == Some(&json!(COMMAND))
        && entry
            .get("args")
            .and_then(Value::as_array)
            .is_some_and(|args| {
                args.iter()
                    .map(Value::as_str)
                    .take(3)
                    .eq(SERVE_ARGS.map(Some))
            })
}

fn read_config(path: &Path) -> Result<Option<Value>, AgentError> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map(Some).map_err(|error| {
            AgentError::Rejected(format!(
                "{} is not plain JSON ({error}); fix it or add the `{SERVER_NAME}` server by hand",
                path.display()
            ))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AgentError::Failed(format!(
            "could not read {}: {error}",
            path.display()
        ))),
    }
}

/// The key of the Wright-owned entry in `servers`, or `Ok(None)`. A foreign
/// entry squatting on the `wright` key is refused unless `force`.
fn locate(
    servers: &Map<String, Value>,
    path: &Path,
    force: bool,
) -> Result<Option<String>, AgentError> {
    let owned: Vec<&String> = servers
        .iter()
        .filter(|(_, entry)| is_wright_owned(entry))
        .map(|(key, _)| key)
        .collect();
    if owned.len() > 1 {
        return Err(AgentError::Rejected(format!(
            "{} configures the Wright MCP server more than once ({}); remove the extras first",
            path.display(),
            owned
                .iter()
                .map(|key| key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if let Some(key) = owned.first() {
        return Ok(Some((*key).clone()));
    }
    if servers.contains_key(SERVER_NAME) && !force {
        return Err(AgentError::Rejected(format!(
            "{} already has a `{SERVER_NAME}` server that does not run `wright serve --transport mcp`; remove it or pass --force to replace it",
            path.display()
        )));
    }
    Ok(servers
        .contains_key(SERVER_NAME)
        .then(|| SERVER_NAME.to_string()))
}

fn servers_of<'a>(
    config: &'a mut Value,
    key: &str,
    path: &Path,
) -> Result<&'a mut Map<String, Value>, AgentError> {
    config
        .as_object_mut()
        .and_then(|root| root.entry(key).or_insert_with(|| json!({})).as_object_mut())
        .ok_or_else(|| {
            AgentError::Rejected(format!(
                "{} does not hold a JSON object under `{key}`; fix it by hand",
                path.display()
            ))
        })
}

fn write_config(path: &Path, config: &Value) -> Result<(), AgentError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            AgentError::Failed(format!("could not create {}: {error}", parent.display()))
        })?;
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(config).expect("config serializes") + "\n",
    )
    .map_err(|error| AgentError::Failed(format!("could not write {}: {error}", path.display())))
}

pub(crate) fn install(
    root: &Path,
    target: McpTarget,
    force: bool,
    dry_run: bool,
) -> Result<McpStatus, AgentError> {
    let layout = target.layout();
    let path = root.join(layout.file);
    let existing = read_config(&path)?;
    let created = existing.is_none();
    let mut config = existing.unwrap_or_else(|| json!({}));
    let servers = servers_of(&mut config, layout.servers_key, &path)?;
    let key = locate(servers, &path, force)?;
    let entry = desired_entry(&layout);
    if key.as_ref().is_some_and(|key| servers[key] == entry) {
        return Ok(McpStatus::UpToDate(path));
    }
    servers.insert(key.clone().unwrap_or_else(|| SERVER_NAME.into()), entry);
    if dry_run {
        return Ok(McpStatus::DryRun(path));
    }
    write_config(&path, &config)?;
    Ok(if created || key.is_none() {
        McpStatus::Created(path)
    } else {
        McpStatus::Updated(path)
    })
}

pub(crate) fn remove(
    root: &Path,
    target: McpTarget,
    dry_run: bool,
) -> Result<McpStatus, AgentError> {
    let layout = target.layout();
    let path = root.join(layout.file);
    let Some(mut config) = read_config(&path)? else {
        return Ok(McpStatus::NotConfigured(path));
    };
    let servers = servers_of(&mut config, layout.servers_key, &path)?;
    let Some(key) = locate(servers, &path, false)? else {
        return Ok(McpStatus::NotConfigured(path));
    };
    if !is_wright_owned(&servers[&key]) {
        return Err(AgentError::Rejected(format!(
            "{} has a `{key}` server that Wright did not configure; remove it by hand",
            path.display()
        )));
    }
    servers.remove(&key);
    if dry_run {
        return Ok(McpStatus::DryRun(path));
    }
    write_config(&path, &config)?;
    Ok(McpStatus::Removed(path))
}

pub(crate) fn run(args: &AgentMcpArgs) -> Result<u8, AgentError> {
    let root = Path::new(".");
    let (status, guide) = match &args.action {
        crate::cli::AgentMcpAction::Install(install_args) => (
            install(
                root,
                install_args.target,
                install_args.force,
                install_args.dry_run,
            )?,
            install_args.guide,
        ),
        crate::cli::AgentMcpAction::Remove(remove_args) => (
            remove(root, remove_args.target, remove_args.dry_run)?,
            false,
        ),
    };
    match &status {
        McpStatus::Created(path) => {
            println!("==> configured the Wright MCP server in {}", path.display())
        }
        McpStatus::Updated(path) => {
            println!("==> updated the Wright MCP server in {}", path.display())
        }
        McpStatus::UpToDate(path) => println!(
            "the Wright MCP server in {} is already up to date",
            path.display()
        ),
        McpStatus::Removed(path) => {
            println!("==> removed the Wright MCP server from {}", path.display())
        }
        McpStatus::NotConfigured(path) => {
            println!("no Wright MCP server is configured in {}", path.display())
        }
        McpStatus::DryRun(path) => {
            println!("would change the Wright MCP server in {}", path.display())
        }
    }
    if guide {
        super::run(&crate::cli::AgentArgs {
            subcommand: Some(crate::cli::AgentSubcommand::Install(
                crate::cli::AgentInstallArgs {
                    dest: None,
                    force: false,
                    dry_run: matches!(status, McpStatus::DryRun(_)),
                },
            )),
        })?;
    }
    Ok(super::exit::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wright-mcp-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read(root: &Path, file: &str) -> Value {
        serde_json::from_str(&std::fs::read_to_string(root.join(file)).unwrap()).unwrap()
    }

    #[test]
    fn writes_each_target_in_its_own_format_and_is_idempotent() {
        for (target, file, key, typed, last_arg) in [
            (McpTarget::Claude, ".mcp.json", "mcpServers", true, "mcp"),
            (
                McpTarget::Cursor,
                ".cursor/mcp.json",
                "mcpServers",
                false,
                "${workspaceFolder}",
            ),
            (
                McpTarget::Vscode,
                ".vscode/mcp.json",
                "servers",
                true,
                "${workspaceFolder}",
            ),
        ] {
            let root = scratch(&format!("fmt-{file:.4}-{key}").replace(['/', '.'], ""));
            let path = root.join(file);
            assert_eq!(
                install(&root, target, false, false),
                Ok(McpStatus::Created(path.clone()))
            );
            let entry = read(&root, file)[key][SERVER_NAME].clone();
            assert_eq!(entry["command"], "wright");
            assert!(is_wright_owned(&entry));
            assert_eq!(entry["args"].as_array().unwrap().last().unwrap(), last_arg);
            assert_eq!(entry.get("type").is_some(), typed);
            assert_eq!(
                install(&root, target, false, false),
                Ok(McpStatus::UpToDate(path))
            );
            assert_eq!(read(&root, file)[key].as_object().unwrap().len(), 1);
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn preserves_unrelated_configuration_through_install_and_remove() {
        let root = scratch("preserve");
        let path = root.join(".mcp.json");
        let original = json!({
            "mcpServers": {"other": {"command": "x", "args": ["1"]}},
            "extra": [1, 2],
        });
        std::fs::write(&path, original.to_string()).unwrap();
        assert_eq!(
            install(&root, McpTarget::Claude, false, false),
            Ok(McpStatus::Created(path.clone()))
        );
        let installed = read(&root, ".mcp.json");
        assert_eq!(
            installed["mcpServers"]["other"],
            original["mcpServers"]["other"]
        );
        assert_eq!(installed["extra"], original["extra"]);
        assert_eq!(
            remove(&root, McpTarget::Claude, false),
            Ok(McpStatus::Removed(path.clone()))
        );
        assert_eq!(read(&root, ".mcp.json"), original);
        assert_eq!(
            remove(&root, McpTarget::Claude, false),
            Ok(McpStatus::NotConfigured(path))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn updates_a_stale_wright_entry_in_place_without_duplicating_it() {
        let root = scratch("stale");
        let path = root.join(".mcp.json");
        std::fs::write(
            &path,
            json!({"mcpServers": {"my-wright": {"command": "wright", "args": ["serve", "--transport", "mcp", "old.ws"]}}}).to_string(),
        )
        .unwrap();
        assert_eq!(
            install(&root, McpTarget::Claude, false, false),
            Ok(McpStatus::Updated(path))
        );
        let servers = read(&root, ".mcp.json")["mcpServers"].clone();
        assert_eq!(servers.as_object().unwrap().len(), 1);
        assert_eq!(servers["my-wright"]["args"], json!(SERVE_ARGS));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_a_foreign_wright_entry_unless_forced() {
        let root = scratch("foreign");
        let path = root.join(".mcp.json");
        let foreign = json!({"mcpServers": {"wright": {"command": "other"}}});
        std::fs::write(&path, foreign.to_string()).unwrap();
        assert!(matches!(
            install(&root, McpTarget::Claude, false, false),
            Err(AgentError::Rejected(_))
        ));
        assert!(matches!(
            remove(&root, McpTarget::Claude, false),
            Err(AgentError::Rejected(_))
        ));
        assert_eq!(read(&root, ".mcp.json"), foreign);
        assert_eq!(
            install(&root, McpTarget::Claude, true, false),
            Ok(McpStatus::Updated(path))
        );
        assert_eq!(
            read(&root, ".mcp.json")["mcpServers"]["wright"]["command"],
            "wright"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_unparseable_config_and_dry_run_writes_nothing() {
        let root = scratch("bad");
        std::fs::write(root.join(".mcp.json"), "{ // comment\n}").unwrap();
        assert!(matches!(
            install(&root, McpTarget::Claude, true, false),
            Err(AgentError::Rejected(_))
        ));
        let clean = scratch("dry");
        assert!(matches!(
            install(&clean, McpTarget::Cursor, false, true),
            Ok(McpStatus::DryRun(_))
        ));
        assert!(!clean.join(".cursor").exists());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&clean);
    }
}
