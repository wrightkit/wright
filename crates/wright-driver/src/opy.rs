//! Temporary `.opy` frontend bridge (M6).
//!
//! Until the native `.opy` frontend lands in M7, `.opy` sources are converted
//! through the pinned OverPy adapter (`adapter/bin/wright-adapter.js`) into
//! an Opy HIR v1 payload, which the core then ingests natively. This module
//! owns that subprocess boundary: locating the adapter, invoking Node,
//! mapping failures to structured diagnostics, and returning the protocol
//! JSON. The M7 native frontend replaces this module behind the same driver
//! contract, so no caller changes.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::diag::{Diagnostic, Stage};
use crate::input::ResolvedInput;

/// The environment variable pointing at the adapter script (or its directory).
pub const ADAPTER_ENV: &str = "WRIGHT_ADAPTER_PATH";

/// Run the adapter over one resolved `.opy` input and return the Opy HIR v1
/// protocol JSON.
pub fn run_adapter(input: &ResolvedInput) -> Result<String, Diagnostic> {
    let adapter = locate_adapter().ok_or_else(|| {
        Diagnostic::error(
            "adapter-unavailable",
            Stage::Frontend,
            format!(
                "the `.opy` frontend bridge (OverPy adapter) was not found; \
                 set {ADAPTER_ENV} to the adapter script path or a directory \
                 containing `bin/wright-adapter.js` (the native `.opy` frontend \
                 replaces this bridge in M7)"
            ),
        )
    })?;

    let Some(path) = &input.path else {
        return Err(Diagnostic::error(
            "stdin-opy-unsupported",
            Stage::Frontend,
            "`.opy` input on stdin is not supported by the adapter bridge; \
             pass a file path (the native `.opy` frontend replaces the bridge in M7)",
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
                    "cannot run the `.opy` adapter bridge (`node {}`): {error}; \
                     Node.js is required until the native frontend lands in M7",
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

/// Locate the adapter script: `WRIGHT_ADAPTER_PATH` first (file path or
/// directory containing `bin/wright-adapter.js`), then cwd-relative
/// (walking up toward a repository root) and executable-relative candidates.
fn locate_adapter() -> Option<PathBuf> {
    if let Ok(value) = std::env::var(ADAPTER_ENV) {
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
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    // Walk up from the cwd looking for a repository root with the adapter.
    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = Some(cwd.as_path());
        for _ in 0..6 {
            if let Some(current) = dir {
                candidates.push(current.join("adapter/bin/wright-adapter.js"));
                dir = current.parent();
            }
        }
    }
    candidates.push(PathBuf::from("adapter/bin/wright-adapter.js"));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // target/debug/wright -> ../../adapter/bin/wright-adapter.js
            candidates.push(dir.join("../../adapter/bin/wright-adapter.js"));
            candidates.push(dir.join("../adapter/bin/wright-adapter.js"));
        }
    }
    candidates.into_iter().find(|candidate| candidate.is_file())
}
