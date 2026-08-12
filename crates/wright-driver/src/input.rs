//! Source/project discovery: input kinds, path normalization, and stdin.
//!
//! The driver resolves one [`SessionConfig`] into a concrete
//! [`ResolvedInput`]: the input text, the concrete frontend kind, a stable
//! display identity for diagnostics, an include root for `.opy`, and a
//! deterministic SHA-256 input identity. Automatic detection fails
//! explicitly with actionable guidance whenever the input is ambiguous or
//! outside the supported surface; an explicit `--kind`/locale override always
//! wins over detection.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::config::{InputSpec, SessionConfig, SourceKind};
use crate::diag::{Diagnostic, Origin, Stage};

/// A resolved, ready-to-load input.
#[derive(Debug, Clone)]
pub struct ResolvedInput {
    /// The concrete frontend kind (never `Auto`).
    pub kind: SourceKind,
    /// The input text.
    pub text: String,
    /// The on-disk path, when the input came from a file.
    pub path: Option<PathBuf>,
    /// The include root (`.opy` include base); the input's directory by default.
    pub root: PathBuf,
    /// A stable display identity used in diagnostics (`<stdin>` for stdin).
    pub display: String,
    /// SHA-256 hex of the input bytes (deterministic input identity).
    pub identity: String,
    /// Origin metadata carried into diagnostics and results.
    pub origin: Origin,
}

/// SHA-256 hex digest of a byte slice.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Resolve a session config into a concrete, loadable input.
pub fn resolve(config: &SessionConfig) -> Result<ResolvedInput, Diagnostic> {
    match &config.input {
        InputSpec::Path(path) => resolve_path(path, config),
        InputSpec::Stdin => resolve_stdin(config),
    }
}

fn resolve_path(path: &Path, config: &SessionConfig) -> Result<ResolvedInput, Diagnostic> {
    let bytes = std::fs::read(path).map_err(|error| {
        Diagnostic::error(
            "input-io",
            Stage::Discovery,
            format!("cannot read input '{}': {error}", path.display()),
        )
    })?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let kind = match config.kind {
        SourceKind::Auto => kind_from_extension(path)?,
        other => other,
    };
    let root = match &config.root {
        Some(root) => root.clone(),
        None => path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")),
    };
    let display = display_path(path);
    let origin = origin_for(kind, config.locale.as_deref());
    Ok(ResolvedInput {
        kind,
        text,
        path: Some(path.to_path_buf()),
        root,
        display,
        identity: sha256_hex(&bytes),
        origin,
    })
}

fn resolve_stdin(config: &SessionConfig) -> Result<ResolvedInput, Diagnostic> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .read_to_end(&mut bytes)
        .map_err(|error| {
            Diagnostic::error(
                "stdin-io",
                Stage::Discovery,
                format!("cannot read standard input: {error}"),
            )
        })?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let kind = match config.kind {
        SourceKind::Auto => kind_from_stdin(&text)?,
        SourceKind::Opy => {
            return Err(Diagnostic::error(
                "stdin-opy-unsupported",
                Stage::Discovery,
                "`.opy` input on stdin is not supported by the adapter bridge; \
                 pass a file path (the native `.opy` frontend replaces the bridge in M7)",
            ));
        }
        other => other,
    };
    let root = config
        .root
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let display = "<stdin>".to_string();
    let origin = origin_for(kind, config.locale.as_deref());
    Ok(ResolvedInput {
        kind,
        text,
        path: None,
        root,
        display,
        identity: sha256_hex(&bytes),
        origin,
    })
}

/// Map a file extension to a source kind; unknown extensions fail explicitly.
fn kind_from_extension(path: &Path) -> Result<SourceKind, Diagnostic> {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return Err(Diagnostic::error(
            "input-kind-unknown",
            Stage::Discovery,
            format!(
                "cannot detect the input kind of '{}' (no extension); \
                 pass `--kind opy|workshop|protocol` to override",
                path.display()
            ),
        ));
    };
    match extension.to_ascii_lowercase().as_str() {
        "opy" => Ok(SourceKind::Opy),
        "json" => Ok(SourceKind::Protocol),
        "txt" | "ws" | "workshop" => Ok(SourceKind::Workshop),
        other => Err(Diagnostic::error(
            "input-kind-unknown",
            Stage::Discovery,
            format!(
                "cannot detect the input kind of '{}' (unknown extension '.{other}'); \
                 pass `--kind opy|workshop|protocol` to override",
                path.display()
            ),
        )),
    }
}

/// Detect the frontend kind of stdin text: protocol JSON starts with `{`,
/// Workshop text does not. Anything else fails explicitly.
fn kind_from_stdin(text: &str) -> Result<SourceKind, Diagnostic> {
    let trimmed = text.trim_start();
    if trimmed.is_empty() {
        return Err(Diagnostic::error(
            "stdin-empty",
            Stage::Discovery,
            "standard input is empty; provide a program on stdin or pass an input path",
        ));
    }
    if trimmed.starts_with('{') {
        Ok(SourceKind::Protocol)
    } else {
        Ok(SourceKind::Workshop)
    }
}

/// A stable display path: relative to the cwd when possible, else absolute.
pub fn display_path(path: &Path) -> String {
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(relative) = path.strip_prefix(&cwd) {
            if !relative.as_os_str().is_empty() {
                return relative.display().to_string();
            }
        }
    }
    path.display().to_string()
}

/// Origin metadata for a resolved input.
fn origin_for(kind: SourceKind, locale: Option<&str>) -> Origin {
    match kind {
        SourceKind::Opy => Origin {
            kind: "opy".to_string(),
            locale: None,
        },
        SourceKind::Workshop => Origin {
            kind: "workshop".to_string(),
            locale: locale.map(str::to_string),
        },
        SourceKind::Protocol => Origin {
            kind: "protocol".to_string(),
            locale: None,
        },
        SourceKind::Auto => Origin {
            kind: "auto".to_string(),
            locale: None,
        },
    }
}
