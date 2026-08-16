//! The `.opy` frontend integration for the driver.
//!
//! The default `.opy` path is the native Rust frontend
//! (`wright_opy`): no Node, no OverPy, stdin supported. The pinned OverPy
//! adapter bridge remains available as an explicit compatibility fallback
//! when `WRIGHT_ADAPTER_PATH` is set (migration/debugging only) — it is never
//! selected silently.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::diag::{Diagnostic, Stage};
use crate::input::ResolvedInput;

/// The environment variable that selects the adapter fallback.
pub const ADAPTER_ENV: &str = "WRIGHT_ADAPTER_PATH";

/// Whether the explicit adapter fallback is requested.
pub fn adapter_fallback_requested() -> bool {
    std::env::var_os(ADAPTER_ENV).is_some()
}

/// Run the pinned OverPy adapter over one resolved `.opy` input and return
/// the Opy HIR v1 protocol JSON. Used only when `WRIGHT_ADAPTER_PATH` is set.
pub fn run_adapter(input: &ResolvedInput) -> Result<String, Diagnostic> {
    let adapter = locate_adapter().ok_or_else(|| {
        Diagnostic::error(
            "adapter-unavailable",
            Stage::Frontend,
            "WRIGHT_ADAPTER_PATH is set but the adapter script was not found; \
             point it at `bin/wright-adapter.js` or a directory containing it",
        )
    })?;

    let Some(path) = &input.path else {
        return Err(Diagnostic::error(
            "adapter-stdin-unsupported",
            Stage::Frontend,
            "the adapter fallback cannot read `.opy` from stdin; use a file path",
        ));
    };

    let main_file = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("main.opy")
        .to_string();

    let temp_dir = std::env::temp_dir().join(format!("wright-adapter-{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).map_err(|error| {
        Diagnostic::error(
            "adapter-internal",
            Stage::Frontend,
            format!("cannot create adapter temp dir: {error}"),
        )
    })?;
    let output_path = temp_dir.join("out.json");

    let output = Command::new("node")
        .arg(&adapter)
        .arg("--input")
        .arg(path)
        .arg("--root")
        .arg(&input.root)
        .arg("--main-file")
        .arg(&main_file)
        .arg("--output")
        .arg(&output_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let output = match output {
        Ok(output) => output,
        Err(error) => {
            return Err(Diagnostic::error(
                "adapter-internal",
                Stage::Frontend,
                format!(
                    "cannot run the `.opy` adapter bridge (`node {}`): {error}",
                    path.display()
                ),
            ));
        }
    };

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        let code = if stderr.contains("unsupported") {
            "unsupported-construct"
        } else {
            "adapter-failed"
        };
        let message = stderr.trim();
        return Err(Diagnostic::error(
            code,
            Stage::Frontend,
            if message.is_empty() {
                format!(
                    "the `.opy` adapter bridge failed with exit {}",
                    output.status
                )
            } else {
                format!("{} (adapter exit {})", message, output.status)
            },
        ));
    }

    let json = std::fs::read_to_string(&output_path).map_err(|error| {
        Diagnostic::error(
            "adapter-internal",
            Stage::Frontend,
            format!("the `.opy` adapter produced no output payload: {error}"),
        )
    })?;
    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(json)
}

/// Locate the adapter script when the fallback is requested: the env var is a
/// file path or a directory containing `bin/wright-adapter.js`.
fn locate_adapter() -> Option<PathBuf> {
    let value = std::env::var(ADAPTER_ENV).ok()?;
    let candidate = PathBuf::from(&value);
    if candidate.is_file() {
        return Some(candidate);
    }
    if candidate.is_dir() {
        let nested = candidate.join("bin/wright-adapter.js");
        if nested.is_file() {
            return Some(nested);
        }
    }
    None
}

/// The include root for a resolved input (used by the native frontend).
pub fn include_root(input: &ResolvedInput) -> &Path {
    &input.root
}
