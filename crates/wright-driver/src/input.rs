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

/// Whether a resolved input is a single source file or a project target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputTarget {
    File,
    Directory,
}

/// A resolved, ready-to-load input.
#[derive(Debug, Clone)]
pub struct ResolvedInput {
    /// The concrete frontend kind (never `Auto`).
    pub kind: SourceKind,
    /// The input text.
    pub text: String,
    /// The on-disk path, when the input came from a file.
    pub path: Option<PathBuf>,
    /// The resolved filesystem target shape.
    pub target: InputTarget,
    /// The include root (`.opy` include base); the input's directory by default.
    pub root: PathBuf,
    /// The invocation working directory used to resolve a relative entry.
    pub cwd: PathBuf,
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
    let cwd = std::env::current_dir().map_err(|error| {
        Diagnostic::error(
            "cwd-io",
            Stage::Discovery,
            format!("cannot determine the invocation working directory: {error}"),
        )
    })?;
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let metadata = std::fs::metadata(&path).map_err(|error| {
        Diagnostic::error(
            "input-io",
            Stage::Discovery,
            format!("cannot read input '{}': {error}", path.display()),
        )
    })?;
    if metadata.is_dir() {
        return resolve_directory(&path, config, cwd);
    }
    resolve_file(&path, config, cwd)
}

fn resolve_file(
    path: &Path,
    config: &SessionConfig,
    cwd: PathBuf,
) -> Result<ResolvedInput, Diagnostic> {
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
        Some(root) if root.is_absolute() => root.clone(),
        Some(root) => cwd.join(root),
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
        target: InputTarget::File,
        root,
        cwd,
        display,
        identity: sha256_hex(&bytes),
        origin,
    })
}

fn resolve_directory(
    path: &Path,
    config: &SessionConfig,
    cwd: PathBuf,
) -> Result<ResolvedInput, Diagnostic> {
    let kind = match config.kind {
        SourceKind::Auto => detect_directory_kind(path)?,
        SourceKind::Workshop => {
            let files = direct_source_files(path, SourceKind::Workshop);
            if files.len() != 1 {
                return Err(directory_source_count_error(
                    path,
                    SourceKind::Workshop,
                    &files,
                ));
            }
            return resolve_file(&files[0], config, cwd);
        }
        SourceKind::Protocol => {
            return Err(Diagnostic::error(
                "input-kind-directory-unsupported",
                Stage::Discovery,
                format!(
                    "protocol input '{}' must name a file, not a directory",
                    path.display()
                ),
            ));
        }
        other => other,
    };
    if kind == SourceKind::Workshop {
        let files = direct_source_files(path, SourceKind::Workshop);
        if files.len() != 1 {
            return Err(directory_source_count_error(
                path,
                SourceKind::Workshop,
                &files,
            ));
        }
        return resolve_file(&files[0], config, cwd);
    }
    if kind == SourceKind::Auto {
        return Err(Diagnostic::error(
            "input-kind-unknown",
            Stage::Discovery,
            format!(
                "cannot detect a source owner in directory '{}'; pass `--kind opy|ostw|workshop` or target a file",
                path.display()
            ),
        ));
    }
    let root = config
        .root
        .as_ref()
        .map(|root| absolute_from(&cwd, root))
        .unwrap_or_else(|| path.to_path_buf());
    let display = display_path(path);
    let origin = origin_for(kind, config.locale.as_deref());
    Ok(ResolvedInput {
        kind,
        text: String::new(),
        path: Some(path.to_path_buf()),
        target: InputTarget::Directory,
        root,
        cwd,
        display,
        // A directory has no Wright-owned source bytes. Provider-backed
        // compile results replace this placeholder with the owner-selected
        // primary source identity before it reaches result contracts.
        identity: String::new(),
        origin,
    })
}

fn absolute_from(cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn detect_directory_kind(path: &Path) -> Result<SourceKind, Diagnostic> {
    let opy_project = [path.join("main.opy"), path.join("src/main.opy")]
        .iter()
        .any(|candidate| candidate.is_file());
    let ostw_project =
        path.join("ds.toml").is_file() || !direct_source_files(path, SourceKind::Ostw).is_empty();
    let workshop_project = !direct_source_files(path, SourceKind::Workshop).is_empty();
    let mut kinds = Vec::new();
    if opy_project {
        kinds.push(SourceKind::Opy);
    }
    if ostw_project {
        kinds.push(SourceKind::Ostw);
    }
    // A conventional OPY/DEL project may contain generated Workshop files;
    // only consider raw Workshop when no source-owner project candidate exists.
    if kinds.is_empty() && workshop_project {
        kinds.push(SourceKind::Workshop);
    }
    match kinds.as_slice() {
        [kind] => Ok(*kind),
        [] => Err(Diagnostic::error(
            "input-kind-unknown",
            Stage::Discovery,
            format!(
                "cannot detect a source owner in directory '{}'; pass `--kind opy|ostw|workshop` or target a file",
                path.display()
            ),
        )),
        _ => Err(Diagnostic::error(
            "input-kind-ambiguous",
            Stage::Discovery,
            format!(
                "directory '{}' contains multiple source kinds ({}); pass `--kind opy|ostw|workshop` or target a file",
                path.display(),
                kinds
                    .iter()
                    .map(|kind| kind.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

fn direct_source_files(path: &Path, kind: SourceKind) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(path) else {
        return files;
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        if entry_path.is_file() && kind_from_extension(&entry_path).ok() == Some(kind) {
            files.push(entry_path);
        }
    }
    files.sort();
    files
}

fn directory_source_count_error(path: &Path, kind: SourceKind, files: &[PathBuf]) -> Diagnostic {
    let detail = if files.is_empty() {
        "no matching source file was found".to_string()
    } else {
        format!(
            "multiple matching source files were found: {}",
            files
                .iter()
                .map(|file| file.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    Diagnostic::error(
        "input-kind-ambiguous",
        Stage::Discovery,
        format!(
            "Workshop directory '{}' must contain exactly one source file; {detail}; pass an explicit file path",
            path.display()
        )
        .replace("Workshop directory", &format!("{} directory", kind.as_str())),
    )
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
        // The native `.opy` frontend reads source from stdin; the include root
        // defaults to the working directory.
        other => other,
    };
    let root = config
        .root
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let display = "<stdin>".to_string();
    let origin = origin_for(kind, config.locale.as_deref());
    Ok(ResolvedInput {
        kind,
        text,
        path: None,
        target: InputTarget::File,
        root,
        cwd,
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
                 pass `--kind opy|ostw|workshop|protocol` to override",
                path.display()
            ),
        ));
    };
    match extension.to_ascii_lowercase().as_str() {
        "opy" => Ok(SourceKind::Opy),
        "ostw" | "del" => Ok(SourceKind::Ostw),
        "json" => Ok(SourceKind::Protocol),
        "txt" | "ow" | "ws" | "workshop" => Ok(SourceKind::Workshop),
        other => Err(Diagnostic::error(
            "input-kind-unknown",
            Stage::Discovery,
            format!(
                "cannot detect the input kind of '{}' (unknown extension '.{other}'); \
                 pass `--kind opy|ostw|workshop|protocol` to override",
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
        SourceKind::Ostw => Origin {
            kind: "ostw".to_string(),
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
