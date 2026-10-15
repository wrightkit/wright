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

/// Decode source bytes into the text every consumer sees (#636): a leading
/// U+FEFF byte-order mark is a file-encoding artifact, not source text, so
/// loaded, defaulted, and re-read views all compare and parse the same
/// BOM-free text.
pub(crate) fn decode_source_text(bytes: &[u8]) -> String {
    strip_bom(&String::from_utf8_lossy(bytes)).to_owned()
}

fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{FEFF}').unwrap_or(text)
}

/// The disk state a [`ResolvedInput`] was loaded from (#471).
///
/// `ToolService` captures one fingerprint at load time and recomputes it
/// before each request; a changed digest means a source file the load could
/// observe was edited, added, or removed, so the program must be reloaded
/// rather than served stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DiskFingerprint {
    /// Stdin input has no disk state; it can never go stale.
    Detached,
    /// A digest of every file the input can observe: paths and contents.
    Digest(String),
}

/// Fingerprint the disk state `resolved` was resolved from.
///
/// A content digest rather than an mtime comparison, so copy/restore and
/// checkout patterns cannot masquerade as unchanged. Files that cannot be
/// read contribute a placeholder entry — the digest still changes when they
/// appear, disappear, or heal, so the next request retries the load and
/// surfaces the resolver's diagnostic instead of serving the old program.
pub(crate) fn disk_fingerprint(
    config: &SessionConfig,
    resolved: &ResolvedInput,
) -> DiskFingerprint {
    let InputSpec::Path(path) = &config.input else {
        return DiskFingerprint::Detached;
    };
    let path = absolute_from(&resolved.cwd, path);
    if resolved.kind == SourceKind::Opy {
        // The provider's source closure is opaque to Wright: `#!mainFile`,
        // `import`, `.po` catalogs, and external settings paths all resolve
        // inside the entry's directory, so the digest covers every file
        // under it rather than the entry alone.
        let root = if path.is_dir() {
            path
        } else {
            path.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."))
        };
        return digest_tree(&root);
    }
    if path.is_dir() {
        return digest_files(direct_source_files(&path, resolved.kind));
    }
    digest_files(vec![path])
}

/// Every regular file under `root`, recursively. Hidden entries (`.git`,
/// `.DS_Store`, caches) never feed an input, and symlinked directories are
/// not followed — a symlink cycle would never terminate — so only direct
/// directory children are walked.
fn digest_tree(root: &Path) -> DiskFingerprint {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if kind.is_dir() {
                stack.push(entry.path());
            } else if kind.is_file() || entry.path().is_file() {
                // `file_type` does not traverse a symlink, so a link to a
                // file would be invisible to the fingerprint while
                // discovery follows it (#636); `is_file` resolves through
                // the link. Directories keep the `file_type` test so a
                // symlinked directory is still never walked.
                files.push(entry.path());
            }
        }
    }
    files.sort();
    digest_files(files)
}

fn digest_files(files: Vec<PathBuf>) -> DiskFingerprint {
    let mut hasher = Sha256::new();
    for path in &files {
        hasher.update(path.as_os_str().as_encoded_bytes());
        hasher.update([0]);
        match std::fs::read(path) {
            Ok(bytes) => hasher.update(&bytes),
            Err(_) => hasher.update([0xff]),
        }
        hasher.update([0]);
    }
    DiskFingerprint::Digest(format!("{:x}", hasher.finalize()))
}

pub fn resolve(config: &SessionConfig) -> Result<ResolvedInput, Diagnostic> {
    match &config.input {
        InputSpec::Path(path) => resolve_path(path, config),
        InputSpec::Text { text, path } => resolve_text(text, path.as_deref(), config),
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
    let path = absolute_from(&cwd, path);
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
    let text = decode_source_text(&bytes);
    let kind = match config.kind {
        SourceKind::Auto => kind_from_extension(path)?,
        other => other,
    };
    let root = config
        .root
        .as_deref()
        .map(|root| absolute_from(&cwd, root))
        .unwrap_or_else(|| {
            path.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."))
        });
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
        // Hidden entries never feed an input — the same convention
        // `digest_tree` fingerprints by (#636).
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let entry_path = entry.path();
        if entry_path.is_file() && directory_member(&entry_path, kind) {
            files.push(entry_path);
        }
    }
    files.sort();
    files
}

/// Whether `path` counts toward a directory's `kind` source set — the
/// single membership definition discovery and the disk fingerprint share
/// (#636). A bare `.txt` is too weak a signal to claim Workshop ownership
/// of a directory: `version.txt`, `requirements.txt`, `notes.txt` are
/// ambient in any project root and would misroute discovery or force a
/// spurious ambiguity. An explicit `.txt` file path still resolves through
/// `kind_from_extension`.
fn directory_member(path: &Path, kind: SourceKind) -> bool {
    if kind_from_extension(path).ok() != Some(kind) {
        return false;
    }
    kind != SourceKind::Workshop
        || path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_none_or(|extension| !extension.eq_ignore_ascii_case("txt"))
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
            "{} directory '{}' must contain exactly one source file; {detail}; pass an explicit file path",
            kind.as_str(),
            path.display(),
        ),
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

/// Resolve caller-held text into an input (#555): the buffer replaces the
/// disk read, so an open document whose file is unsaved or absent still
/// loads — diagnostics and spans keep the file identity `path` provides.
fn resolve_text(
    text: &str,
    path: Option<&Path>,
    config: &SessionConfig,
) -> Result<ResolvedInput, Diagnostic> {
    let text = strip_bom(text);
    let cwd = std::env::current_dir().map_err(|e| {
        Diagnostic::error(
            "cwd-io",
            Stage::Discovery,
            format!("cannot determine the invocation working directory: {e}"),
        )
    })?;
    let path = path.map(|path| absolute_from(&cwd, path));
    let kind = match config.kind {
        SourceKind::Auto => match &path {
            Some(path) => kind_from_extension(path)?,
            None => {
                return Err(Diagnostic::error(
                    "input-kind-unknown",
                    Stage::Discovery,
                    "cannot detect the input kind of an anonymous document; pass `--kind opy|ostw|workshop|protocol` to override",
                ));
            }
        },
        other => other,
    };
    let root = config
        .root
        .as_deref()
        .map(|root| absolute_from(&cwd, root))
        .or_else(|| {
            path.as_deref()
                .and_then(|path| path.parent().map(Path::to_path_buf))
        })
        .unwrap_or_else(|| cwd.clone());
    let display = path
        .as_deref()
        .map(display_path)
        .unwrap_or_else(|| "<document>".to_string());
    let origin = origin_for(kind, config.locale.as_deref());
    Ok(ResolvedInput {
        kind,
        text: text.to_string(),
        path,
        target: InputTarget::File,
        root,
        cwd,
        display,
        identity: sha256_hex(text.as_bytes()),
        origin,
    })
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
    let text = decode_source_text(&bytes);
    let kind = match config.kind {
        SourceKind::Auto => kind_from_stdin(&text)?,
        other => other,
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = config.root.clone().unwrap_or_else(|| cwd.clone());
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

    fn temp_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "wright-input-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&directory).expect("create test directory");
        directory
    }

    #[test]
    fn bom_prefixed_input_resolves_bom_free_text() {
        // #636: a U+FEFF byte-order mark is a file-encoding artifact, not
        // source text — the file resolves exactly like the BOM-less file.
        let directory = temp_dir("bom");
        let path = directory.join("bom.ws");
        std::fs::write(&path, b"\xef\xbb\xbfrule (\"r\") {}\n").expect("write BOM source");

        let resolved = resolve(&SessionConfig {
            input: InputSpec::Path(path),
            kind: SourceKind::Auto,
            ..SessionConfig::default()
        })
        .expect("a BOM-prefixed file resolves");
        assert_eq!(resolved.kind, SourceKind::Workshop);
        assert_eq!(resolved.text, "rule (\"r\") {}\n");

        let resolved = resolve(&SessionConfig {
            input: InputSpec::Text {
                text: "\u{FEFF}rule (\"r\") {}\n".to_string(),
                path: Some(PathBuf::from("doc.ws")),
            },
            kind: SourceKind::Auto,
            ..SessionConfig::default()
        })
        .expect("a BOM-prefixed document resolves");
        assert_eq!(resolved.text, "rule (\"r\") {}\n");

        let text = decode_source_text(b"\xef\xbb\xbf{\"op\":\"check\"}");
        assert_eq!(
            kind_from_stdin(&text).expect("detected"),
            SourceKind::Protocol,
            "a BOM-prefixed JSON stdin is protocol input, not Workshop"
        );

        std::fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn directory_membership_ignores_dotfiles_and_stray_txt() {
        // #636: discovery and the disk fingerprint share one membership
        // definition — hidden entries and a bare `.txt` never count.
        let directory = temp_dir("members");
        std::fs::write(directory.join("main.ws"), "rule (\"main\") {}\n")
            .expect("write main source");
        std::fs::write(directory.join(".notes.ws"), "rule (\"notes\") {}\n")
            .expect("write hidden source");
        std::fs::write(directory.join("._main.ws"), "rule (\"apple\") {}\n")
            .expect("write AppleDouble source");
        std::fs::write(directory.join("notes.txt"), "not a source\n")
            .expect("write stray text file");

        let resolved = resolve(&SessionConfig {
            input: InputSpec::Path(directory.clone()),
            kind: SourceKind::Auto,
            ..SessionConfig::default()
        })
        .expect("main.ws is the single member");
        assert_eq!(resolved.kind, SourceKind::Workshop);
        assert_eq!(
            resolved.path.as_deref(),
            Some(directory.join("main.ws").as_path())
        );

        let hidden = temp_dir("hidden-only");
        std::fs::write(hidden.join("._main.ws"), "rule (\"r\") {}\n")
            .expect("write AppleDouble source");
        let error = resolve(&SessionConfig {
            input: InputSpec::Path(hidden.clone()),
            kind: SourceKind::Auto,
            ..SessionConfig::default()
        })
        .expect_err("hidden-only directory has no source member");
        assert_eq!(error.code, "input-kind-unknown");

        std::fs::remove_dir_all(directory).expect("remove test directory");
        std::fs::remove_dir_all(hidden).expect("remove test directory");
    }

    #[test]
    #[cfg(unix)]
    fn digest_tree_follows_file_symlinks() {
        // #636: `DirEntry::file_type` does not traverse a symlink, so a
        // linked file must be fingerprinted through the resolved path or
        // edits to its target would never read as a change.
        let directory = temp_dir("links");
        std::fs::write(directory.join("target.txt"), "v1\n").expect("write target");
        let without_link = digest_tree(&directory);
        std::os::unix::fs::symlink(directory.join("target.txt"), directory.join("linked.txt"))
            .expect("create symlink");
        let with_link = digest_tree(&directory);
        assert_ne!(
            without_link, with_link,
            "the symlinked file joins the fingerprint"
        );
        std::fs::write(directory.join("target.txt"), "v2\n").expect("edit target");
        assert_ne!(
            with_link,
            digest_tree(&directory),
            "an edit to the link's target is observed"
        );

        // A symlinked directory is still never walked: a link cycle would
        // never terminate.
        std::os::unix::fs::symlink(&directory, directory.join("cycle"))
            .expect("create dir symlink");
        let _ = digest_tree(&directory);

        std::fs::remove_dir_all(directory).expect("remove test directory");
    }

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
        std::fs::write(directory.join("generated.ws"), "rule (\"generated\") {}\n")
            .expect("write Workshop source");
        std::fs::write(directory.join("another.ws"), "rule (\"another\") {}\n")
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

    #[test]
    fn directory_detection_ignores_a_stray_txt() {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "wright-input-stray-txt-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&directory).expect("create test directory");
        std::fs::write(directory.join("version.txt"), "0.11.0\n").expect("write text file");

        let config = SessionConfig {
            input: InputSpec::Path(directory.clone()),
            kind: SourceKind::Auto,
            ..SessionConfig::default()
        };
        let error = resolve(&config).expect_err("a lone .txt must not claim the directory");
        assert_eq!(error.code, "input-kind-unknown");

        std::fs::remove_dir_all(directory).expect("remove test directory");
    }
}
