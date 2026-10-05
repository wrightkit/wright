//! MCP bootstrap for `wright agent install --mcp <target>` (#509): register
//! Wright's own MCP server in a coding-agent harness's project-local config.
//! Wright writes only the one entry it generates (`wright serve --transport
//! mcp`); the harness starts the stdio process itself. Anything else already
//! in the config, including a differing `wright` entry, is never rewritten
//! without `--force`.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::AgentError;
use crate::cli::McpTarget;

const SERVER_NAME: &str = "wright";
const COMMAND: &str = "wright";
const SERVE_ARGS: [&str; 3] = ["serve", "--transport", "mcp"];

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum McpStatus {
    Created(PathBuf),
    Replaced(PathBuf),
    UpToDate(PathBuf),
    DryRun(PathBuf),
}

struct Layout {
    file: &'static str,
    servers_key: &'static str,
    typed: bool,
    /// The harness's project-root expansion, passed as the session input so
    /// the project does not depend on the server's working directory.
    input: &'static str,
}

impl McpTarget {
    fn layout(self) -> Layout {
        match self {
            McpTarget::Claude => Layout {
                file: ".mcp.json",
                servers_key: "mcpServers",
                typed: true,
                input: "${CLAUDE_PROJECT_DIR:-.}",
            },
            McpTarget::Cursor => Layout {
                file: ".cursor/mcp.json",
                servers_key: "mcpServers",
                typed: false,
                input: "${workspaceFolder}",
            },
            McpTarget::Vscode => Layout {
                file: ".vscode/mcp.json",
                servers_key: "servers",
                typed: true,
                input: "${workspaceFolder}",
            },
        }
    }
}

fn generated_entry(layout: &Layout) -> Value {
    let mut entry = Map::new();
    if layout.typed {
        entry.insert("type".into(), json!("stdio"));
    }
    entry.insert("command".into(), json!(COMMAND));
    entry.insert(
        "args".into(),
        json!([SERVE_ARGS[0], SERVE_ARGS[1], SERVE_ARGS[2], layout.input]),
    );
    Value::Object(entry)
}

/// An entry that starts the Wright MCP server, however it was authored.
fn runs_wright_mcp(entry: &Value) -> bool {
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

/// Refuse anything that is not a regular file inside a real directory, so the
/// write cannot be redirected outside the project through a link.
fn ensure_not_linked(root: &Path, path: &Path) -> Result<(), AgentError> {
    let mut current = root.to_path_buf();
    for component in path
        .strip_prefix(root)
        .expect("path is under root")
        .components()
    {
        current.push(component);
        match current.symlink_metadata() {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(AgentError::Rejected(format!(
                    "{} is a symbolic link; Wright will not write MCP configuration through it",
                    current.display()
                )));
            }
            Ok(meta) if current == path && !meta.is_file() => {
                return Err(AgentError::Rejected(format!(
                    "{} is not a regular file",
                    current.display()
                )));
            }
            _ => {}
        }
    }
    Ok(())
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

/// Replace `path` atomically: write a sibling temp file, then rename it over
/// the original so a failed write never truncates existing configuration.
fn write_config(path: &Path, config: &Value) -> Result<(), AgentError> {
    let fail = |error: std::io::Error| {
        AgentError::Failed(format!("could not write {}: {error}", path.display()))
    };
    let parent = path.parent().expect("config path has a parent");
    std::fs::create_dir_all(parent).map_err(fail)?;
    let temp = parent.join(format!(".wright-mcp-{}.tmp", std::process::id()));
    let text = serde_json::to_string_pretty(config).expect("config serializes") + "\n";
    let result = std::fs::write(&temp, text)
        .and_then(|()| match std::fs::metadata(path) {
            Ok(meta) => std::fs::set_permissions(&temp, meta.permissions()),
            Err(_) => Ok(()),
        })
        .and_then(|()| std::fs::rename(&temp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result.map_err(fail)
}

pub(crate) fn install(
    root: &Path,
    target: McpTarget,
    force: bool,
    dry_run: bool,
) -> Result<McpStatus, AgentError> {
    let layout = target.layout();
    let path = root.join(layout.file);
    ensure_not_linked(root, &path)?;
    let mut config = read_config(&path)?.unwrap_or_else(|| json!({}));
    let wrong_shape = || {
        AgentError::Rejected(format!(
            "{} does not hold a JSON object under `{}`; fix it by hand",
            path.display(),
            layout.servers_key
        ))
    };
    let servers = config
        .as_object_mut()
        .ok_or_else(wrong_shape)?
        .entry(layout.servers_key)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(wrong_shape)?;
    if let Some(duplicate) = servers
        .iter()
        .find(|(key, entry)| key.as_str() != SERVER_NAME && runs_wright_mcp(entry))
        .map(|(key, _)| key)
    {
        return Err(AgentError::Rejected(format!(
            "{} already runs the Wright MCP server as `{duplicate}`; remove it first so the server is not configured twice",
            path.display()
        )));
    }
    let entry = generated_entry(&layout);
    let existing = servers.get(SERVER_NAME);
    if existing == Some(&entry) {
        return Ok(McpStatus::UpToDate(path));
    }
    if existing.is_some() && !force {
        return Err(AgentError::Rejected(format!(
            "{} already has a `{SERVER_NAME}` server that differs from the one `wright agent install` generates; pass --force to replace it, or remove it first",
            path.display()
        )));
    }
    let replaced = existing.is_some();
    servers.insert(SERVER_NAME.into(), entry);
    if dry_run {
        return Ok(McpStatus::DryRun(path));
    }
    write_config(&path, &config)?;
    Ok(if replaced {
        McpStatus::Replaced(path)
    } else {
        McpStatus::Created(path)
    })
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
        for (target, file, key, typed, input) in [
            (
                McpTarget::Claude,
                ".mcp.json",
                "mcpServers",
                true,
                "${CLAUDE_PROJECT_DIR:-.}",
            ),
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
            let root = scratch(&format!("fmt-{key}-{typed}-{}", input.len()));
            let path = root.join(file);
            assert_eq!(
                install(&root, target, false, false),
                Ok(McpStatus::Created(path.clone()))
            );
            let entry = read(&root, file)[key][SERVER_NAME].clone();
            assert_eq!(entry["command"], "wright");
            assert_eq!(entry["args"], json!(["serve", "--transport", "mcp", input]));
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
    fn preserves_unrelated_configuration() {
        let root = scratch("preserve");
        let original = json!({
            "mcpServers": {"other": {"command": "x", "args": ["1"]}},
            "extra": [1, 2],
        });
        std::fs::write(root.join(".mcp.json"), original.to_string()).unwrap();
        assert!(install(&root, McpTarget::Claude, false, false).is_ok());
        let installed = read(&root, ".mcp.json");
        assert_eq!(
            installed["mcpServers"]["other"],
            original["mcpServers"]["other"]
        );
        assert_eq!(installed["extra"], original["extra"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_any_differing_wright_entry_unless_forced() {
        let root = scratch("differing");
        let path = root.join(".mcp.json");
        // Even a Wright-looking entry with user-authored extras is not ours to rewrite.
        let custom = json!({"mcpServers": {"wright": {
            "command": "wright",
            "args": ["serve", "--transport", "mcp", "other.ws"],
            "env": {"KEEP": "1"},
        }}});
        std::fs::write(&path, custom.to_string()).unwrap();
        assert!(matches!(
            install(&root, McpTarget::Claude, false, false),
            Err(AgentError::Rejected(_))
        ));
        assert_eq!(read(&root, ".mcp.json"), custom);
        assert!(matches!(
            install(&root, McpTarget::Claude, false, true),
            Err(AgentError::Rejected(_))
        ));
        assert_eq!(
            install(&root, McpTarget::Claude, true, true),
            Ok(McpStatus::DryRun(path.clone()))
        );
        assert_eq!(read(&root, ".mcp.json"), custom);
        assert_eq!(
            install(&root, McpTarget::Claude, true, false),
            Ok(McpStatus::Replaced(path))
        );
        assert!(
            read(&root, ".mcp.json")["mcpServers"]["wright"]
                .get("env")
                .is_none()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_a_duplicate_wright_server_under_another_key() {
        let root = scratch("duplicate");
        let other = json!({"mcpServers": {"my-wright": {"command": "wright", "args": ["serve", "--transport", "mcp"]}}});
        std::fs::write(root.join(".mcp.json"), other.to_string()).unwrap();
        assert!(matches!(
            install(&root, McpTarget::Claude, true, false),
            Err(AgentError::Rejected(_))
        ));
        assert_eq!(read(&root, ".mcp.json"), other);
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

    #[cfg(unix)]
    #[test]
    fn refuses_symlinked_config_and_directories_and_leaves_no_temp_file() {
        let root = scratch("links");
        let outside = scratch("links-outside");
        std::fs::write(outside.join("real.json"), "{}").unwrap();
        std::os::unix::fs::symlink(outside.join("real.json"), root.join(".mcp.json")).unwrap();
        assert!(matches!(
            install(&root, McpTarget::Claude, true, false),
            Err(AgentError::Rejected(_))
        ));
        assert_eq!(
            std::fs::read_to_string(outside.join("real.json")).unwrap(),
            "{}"
        );
        std::os::unix::fs::symlink(&outside, root.join(".cursor")).unwrap();
        assert!(matches!(
            install(&root, McpTarget::Cursor, false, false),
            Err(AgentError::Rejected(_))
        ));
        assert!(!outside.join("mcp.json").exists());
        let ok = scratch("links-ok");
        assert!(install(&ok, McpTarget::Claude, false, false).is_ok());
        assert_eq!(std::fs::read_dir(&ok).unwrap().count(), 1);
        for dir in [&root, &outside, &ok] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
