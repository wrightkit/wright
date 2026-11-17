use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::AgentError;
use super::config_file::{ensure_not_linked, read_config, write_config};
use super::stored::{Stored, load_failure};
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
    input: &'static str,
}

impl McpTarget {
    fn name(self) -> String {
        clap::ValueEnum::to_possible_value(&self)
            .expect("every target has a name")
            .get_name()
            .to_string()
    }

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

fn generated_entry(layout: &Layout, stored: &Stored) -> Value {
    let mut entry = Map::new();
    if layout.typed {
        entry.insert("type".into(), json!("stdio"));
    }
    entry.insert("command".into(), json!(COMMAND));
    let args: Vec<&str> = SERVE_ARGS
        .into_iter()
        .chain(stored.tokens.iter().map(String::as_str))
        .collect();
    entry.insert("args".into(), json!(args));
    Value::Object(entry)
}

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

pub(crate) fn install(
    root: &Path,
    target: McpTarget,
    serve: &[String],
    force: bool,
    dry_run: bool,
) -> Result<McpStatus, AgentError> {
    let layout = target.layout();
    let stored = Stored::new(layout.input, root, serve);
    let config = stored.session_config()?;
    if !force && let Some(failure) = load_failure(config) {
        return Err(AgentError::Rejected(format!(
            "the Wright MCP server would not load the project ({failure}); pass its input and options after `--`, for example `wright agent install --mcp {} -- src/main.opy --kind opy`, or pass --force to register it anyway",
            target.name()
        )));
    }
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
    let entry = generated_entry(&layout, &stored);
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
    use super::super::stored::anchored_tokens;
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wright-mcp-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("session.ws"),
            include_str!("../../../../tests/fixtures/workshop/synthetic/basic-rule.ws"),
        )
        .unwrap();
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
                install(&root, target, &[], false, false),
                Ok(McpStatus::Created(path.clone()))
            );
            let entry = read(&root, file)[key][SERVER_NAME].clone();
            assert_eq!(entry["command"], "wright");
            assert_eq!(entry["args"], json!(["serve", "--transport", "mcp", input]));
            assert_eq!(entry.get("type").is_some(), typed);
            assert_eq!(
                install(&root, target, &[], false, false),
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
        assert!(install(&root, McpTarget::Claude, &[], false, false).is_ok());
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
        let custom = json!({"mcpServers": {"wright": {
            "command": "wright",
            "args": ["serve", "--transport", "mcp", "other.ws"],
            "env": {"KEEP": "1"},
        }}});
        std::fs::write(&path, custom.to_string()).unwrap();
        assert!(matches!(
            install(&root, McpTarget::Claude, &[], false, false),
            Err(AgentError::Rejected(_))
        ));
        assert_eq!(read(&root, ".mcp.json"), custom);
        assert!(matches!(
            install(&root, McpTarget::Claude, &[], false, true),
            Err(AgentError::Rejected(_))
        ));
        assert_eq!(
            install(&root, McpTarget::Claude, &[], true, true),
            Ok(McpStatus::DryRun(path.clone()))
        );
        assert_eq!(read(&root, ".mcp.json"), custom);
        assert_eq!(
            install(&root, McpTarget::Claude, &[], true, false),
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
            install(&root, McpTarget::Claude, &[], true, false),
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
            install(&root, McpTarget::Claude, &[], true, false),
            Err(AgentError::Rejected(_))
        ));
        let clean = scratch("dry");
        assert!(matches!(
            install(&clean, McpTarget::Cursor, &[], false, true),
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
            install(&root, McpTarget::Claude, &[], true, false),
            Err(AgentError::Rejected(_))
        ));
        assert_eq!(
            std::fs::read_to_string(outside.join("real.json")).unwrap(),
            "{}"
        );
        std::os::unix::fs::symlink(&outside, root.join(".cursor")).unwrap();
        assert!(matches!(
            install(&root, McpTarget::Cursor, &[], false, false),
            Err(AgentError::Rejected(_))
        ));
        assert!(!outside.join("mcp.json").exists());
        let ok = scratch("links-ok");
        assert!(install(&ok, McpTarget::Claude, &[], false, false).is_ok());
        assert_eq!(std::fs::read_dir(&ok).unwrap().count(), 2);
        for dir in [&root, &outside, &ok] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    fn opts(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn stores_the_input_and_options_and_treats_them_as_part_of_the_entry() {
        let root = scratch("options");
        let path = root.join(".mcp.json");
        let given = opts(&["session.ws", "--kind", "workshop", "--locale", "en-US"]);
        assert_eq!(
            install(&root, McpTarget::Claude, &given, false, true),
            Ok(McpStatus::DryRun(path.clone()))
        );
        assert!(!path.exists());
        assert_eq!(
            install(&root, McpTarget::Claude, &given, false, false),
            Ok(McpStatus::Created(path.clone()))
        );
        assert_eq!(
            read(&root, ".mcp.json")["mcpServers"]["wright"]["args"],
            json!([
                "serve",
                "--transport",
                "mcp",
                "--kind",
                "workshop",
                "--locale",
                "en-US",
                "${CLAUDE_PROJECT_DIR:-.}/session.ws"
            ])
        );
        assert_eq!(
            install(&root, McpTarget::Claude, &given, false, false),
            Ok(McpStatus::UpToDate(path.clone()))
        );
        let other = opts(&["session.ws", "--kind", "workshop"]);
        assert!(matches!(
            install(&root, McpTarget::Claude, &other, false, false),
            Err(AgentError::Rejected(_))
        ));
        assert_eq!(
            install(&root, McpTarget::Claude, &other, true, false),
            Ok(McpStatus::Replaced(path))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_a_project_that_does_not_load_unless_forced() {
        let root = scratch("unloadable");
        std::fs::write(root.join("main.opy"), "").unwrap();
        let path = root.join(".mcp.json");
        let Err(AgentError::Rejected(message)) =
            install(&root, McpTarget::Claude, &[], false, false)
        else {
            panic!("an ambiguous project must be refused");
        };
        assert!(message.contains("input-kind-ambiguous"), "{message}");
        assert!(message.contains("-- src/main.opy"), "{message}");
        assert!(!path.exists());
        assert_eq!(
            install(
                &root,
                McpTarget::Claude,
                &opts(&["session.ws"]),
                false,
                true
            ),
            Ok(McpStatus::DryRun(path.clone()))
        );
        assert_eq!(
            install(&root, McpTarget::Claude, &[], true, false),
            Ok(McpStatus::Created(path))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_options_serve_would_not_accept() {
        let root = scratch("bad-options");
        for bad in [
            &["--kind", "nope"][..],
            &["--transport", "stdio"],
            &["a", "b"],
        ] {
            assert!(
                matches!(
                    install(&root, McpTarget::Claude, &opts(bad), false, false),
                    Err(AgentError::Usage(_))
                ),
                "{bad:?}"
            );
        }
        assert!(!root.join(".mcp.json").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn force_skips_only_the_load_never_option_validation() {
        let root = scratch("force-options");
        std::fs::write(root.join("main.opy"), "").unwrap();
        for bad in [
            &["session.ws", "--kind", "nope"][..],
            &["session.ws", "--transport", "stdio"],
        ] {
            assert!(
                matches!(
                    install(&root, McpTarget::Claude, &opts(bad), true, false),
                    Err(AgentError::Usage(_))
                ),
                "{bad:?}"
            );
        }
        assert!(!root.join(".mcp.json").exists());
        assert!(install(&root, McpTarget::Claude, &[], true, false).is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn anchors_relative_option_paths_like_the_input() {
        let given = opts(&[
            "src/main.opy",
            "--lint-config",
            "lint.yml",
            "--rule-file=rules/a.yml",
            "--rule-file",
            "/abs/b.yml",
            "--locale",
            "lint.yml",
        ]);
        assert_eq!(
            anchored_tokens("${V}", &given),
            opts(&[
                "--lint-config",
                "${V}/lint.yml",
                "--rule-file=${V}/rules/a.yml",
                "--rule-file",
                "/abs/b.yml",
                "--locale",
                "lint.yml",
                "${V}/src/main.opy",
            ])
        );
        let root = scratch("anchor");
        std::fs::create_dir(root.join("rules")).unwrap();
        assert!(
            install(
                &root,
                McpTarget::Claude,
                &opts(&["session.ws", "--rule-file", "rules"]),
                false,
                false
            )
            .is_ok()
        );
        assert_eq!(
            read(&root, ".mcp.json")["mcpServers"]["wright"]["args"][4],
            "${CLAUDE_PROJECT_DIR:-.}/rules"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
