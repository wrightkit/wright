use std::path::Path;

use serde_json::Value;

use super::AgentError;

pub(super) fn ensure_not_linked(root: &Path, path: &Path) -> Result<(), AgentError> {
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
                    "{} is a symbolic link; Wright will not write agent configuration through it",
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

pub(super) fn read_config(path: &Path) -> Result<Option<Value>, AgentError> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map(Some).map_err(|error| {
            AgentError::Rejected(format!(
                "{} is not plain JSON ({error}); fix it or add the Wright entry by hand",
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

pub(super) fn write_config(path: &Path, config: &Value) -> Result<(), AgentError> {
    let fail = |error: std::io::Error| {
        AgentError::Failed(format!("could not write {}: {error}", path.display()))
    };
    let parent = path.parent().expect("config path has a parent");
    std::fs::create_dir_all(parent).map_err(fail)?;
    let temp = parent.join(format!(".wright-agent-{}.tmp", std::process::id()));
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
