//! The driver resolves one [`SessionConfig`] into a concrete [`ResolvedInput`].

use crate::config::{InputSpec, SessionConfig, SourceKind};
use crate::diag::{Diagnostic, Origin, Stage};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputTarget {
    File,
    Directory,
}

#[derive(Debug, Clone)]
pub struct ResolvedInput {
    pub kind: SourceKind,
    pub text: String,
    pub path: Option<PathBuf>,
    pub target: InputTarget,
    pub root: PathBuf,
    pub cwd: PathBuf,
    pub display: String,
    pub identity: String,
    pub origin: Origin,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn resolve(config: &SessionConfig) -> Result<ResolvedInput, Diagnostic> {
    match &config.input {
        InputSpec::Path(path) => resolve_path(path, config),
        InputSpec::Stdin => resolve_stdin(config),
    }
}

fn resolve_path(path: &Path, config: &SessionConfig) -> Result<ResolvedInput, Diagnostic> {
    let cwd = std::env::current_dir().map_err(|e| {
        Diagnostic::error(
            "cwd-io",
            Stage::Discovery,
            format!("cannot determine the invocation working directory: {e}"),
        )
    })?;
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let metadata = std::fs::metadata(&path).map_err(|e| {
        Diagnostic::error(
            "input-io",
            Stage::Discovery,
            format!("cannot read input '{}': {e}", path.display()),
        )
    })?;
    if metadata.is_dir() {
        resolve_directory(&path, config, cwd)
    } else {
        resolve_file(&path, config, cwd)
    }
}

fn resolve_file(
    path: &Path,
    config: &SessionConfig,
    cwd: PathBuf,
) -> Result<ResolvedInput, Diagnostic> {
    let bytes = std::fs::read(path).map_err(|e| {
        Diagnostic::error(
            "input-io",
            Stage::Discovery,
            format!("cannot read input '{}': {e}", path.display()),
        )
    })?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let kind = match config.kind {
        SourceKind::Auto => kind_from_extension(path)?,
        other => other,
    };
    let root = match &config.root {
        Some(r) if r.is_absolute() => r.clone(),
        Some(r) => cwd.join(r),
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
        .map(|r| absolute_from(&cwd, r))
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
        .any(|c| c.is_file());
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
    if workshop_project {
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
                    .map(|k| k.as_str())
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

fn kind_from_extension(path: &Path) -> Result<SourceKind, Diagnostic> {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return Err(Diagnostic::error(
            "input-kind-unknown",
            Stage::Discovery,
            format!(
                "cannot detect the input kind of '{}' (no extension); pass `--kind opy|ostw|workshop|protocol` to override",
                path.display()
            ),
        ));
    };
    match ext.to_ascii_lowercase().as_str() {
        "opy" => Ok(SourceKind::Opy),
        "ostw" | "del" => Ok(SourceKind::Ostw),
        "txt" | "ow" | "ws" | "workshop" => Ok(SourceKind::Workshop),
        "json" => Ok(SourceKind::Protocol),
        other => Err(Diagnostic::error(
            "input-kind-unknown",
            Stage::Discovery,
            format!(
                "cannot detect the input kind of '{}' (unknown extension '.{other}'); pass `--kind opy|ostw|workshop|protocol` to override",
                path.display()
            ),
        )),
    }
}

fn resolve_stdin(config: &SessionConfig) -> Result<ResolvedInput, Diagnostic> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin().read_to_end(&mut bytes).map_err(|e| {
        Diagnostic::error(
            "stdin-io",
            Stage::Discovery,
            format!("cannot read standard input: {e}"),
        )
    })?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let kind = match config.kind {
        SourceKind::Auto => kind_from_stdin(&text)?,
        other => other,
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = config
        .root
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let origin = origin_for(kind, config.locale.as_deref());
    Ok(ResolvedInput {
        kind,
        text,
        path: None,
        target: InputTarget::File,
        root,
        cwd,
        display: "<stdin>".to_string(),
        identity: sha256_hex(&bytes),
        origin,
    })
}

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

pub fn display_path(path: &Path) -> String {
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(rel) = path.strip_prefix(&cwd) {
            if !rel.as_os_str().is_empty() {
                return rel.display().to_string();
            }
        }
    }
    path.display().to_string()
}

fn origin_for(kind: SourceKind, locale: Option<&str>) -> Origin {
    match kind {
        SourceKind::Workshop => Origin {
            kind: "workshop".to_string(),
            locale: locale.map(str::to_string),
        },
        _ => Origin {
            kind: kind.as_str().to_string(),
            locale: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn directory_detection_reports_workshop_and_opy_as_ambiguous() {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "wright-input-mixed-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&directory).expect("create test directory");
        std::fs::write(directory.join("main.opy"), "rule \"main\":\n    pass\n")
            .expect("write OPY source");
        std::fs::write(directory.join("generated.txt"), "rule (\"generated\") {}\n")
            .expect("write Workshop source");
        std::fs::write(directory.join("another.txt"), "rule (\"another\") {}\n")
            .expect("write second Workshop source");

        let mut config = SessionConfig {
            input: InputSpec::Path(directory.clone()),
            kind: SourceKind::Auto,
            ..SessionConfig::default()
        };
        let error = resolve(&config).expect_err("mixed source kinds must be ambiguous");
        assert_eq!(error.code, "input-kind-ambiguous");
        assert!(error.message.contains("opy"));
        assert!(error.message.contains("workshop"));

        config.kind = SourceKind::Workshop;
        let error = resolve(&config).expect_err("multiple Workshop files must be ambiguous");
        assert_eq!(error.code, "input-kind-ambiguous");

        std::fs::remove_dir_all(directory).expect("remove test directory");
    }
}
