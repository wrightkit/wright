//! The `ds.toml` project model and quoted-import resolution.
//!
//! An OSTW project is defined by a `ds.toml` in the project root. This task
//! supports the `entry_point` key (the project-relative entry source file)
//! and rejects other configuration keys with a structured diagnostic. The
//! project source set is the committed `.ostw`/`.del` closure under the
//! root; quoted imports resolve relative to the importing file (handling
//! `..`), and imports that resolve outside the closure become structured
//! `ostw-missing-import` diagnostics rather than aborting the in-closure
//! parse (#117). Every source file is lexed and parsed with exact spans
//! through the shared `wright_ir::source` registry.

use std::collections::BTreeMap;
use std::path::Path;

use wright_ir::source::{FileId, Position, Span};

use crate::cst;
use crate::diag::{FrontendError, FrontendResult};
use crate::lexer::{self, LexInput};
use crate::parser;

/// A resolved project file: `ds.toml` (id 0) and every `.ostw`/`.del` source.
#[derive(Debug, Clone)]
pub struct FileRecord {
    /// The registry id used by spans and import edges.
    pub id: u32,
    /// The project-relative path (`ds.toml`, `main.ostw`,
    /// `interface/HeroSelect.del`).
    pub path: String,
    /// Whether this is a `.ostw`/`.del` source file (`false` for `ds.toml`).
    pub source: bool,
    /// Whether the source file lexed and parsed cleanly.
    pub parsed: bool,
    /// Resolved import edges, in source order.
    pub imports: Vec<ResolvedImport>,
    /// The parsed CST, when the file is a source that parsed.
    pub cst: Option<cst::File>,
}

/// One resolved `import "path";` edge.
#[derive(Debug, Clone)]
pub struct ResolvedImport {
    /// The path exactly as written in the import statement.
    pub path: String,
    pub span: Span,
    /// The target file id when the import resolves inside the closure,
    /// `None` when it points outside it (a missing-import diagnostic).
    pub target: Option<u32>,
}

/// The loaded OSTW project.
#[derive(Debug, Clone)]
pub struct Project {
    /// The `ds.toml` `entry_point` value.
    pub entry: String,
    /// The project file registry: `ds.toml` at id 0, then every source.
    pub files: Vec<FileRecord>,
}

/// The outcome of a project compile: the registry is retained even when
/// diagnostics exist, so the driver can map spans to file identities.
#[derive(Debug, Clone)]
pub struct OstwOutcome {
    /// The project file registry (always populated when `ds.toml` loads).
    pub project: Option<Project>,
    /// A fatal project-load error (`ds.toml` missing or invalid).
    pub error: Option<FrontendError>,
    /// Non-fatal structured diagnostics (parse errors, missing imports,
    /// unsupported `ds.toml` keys, invalid entry point).
    pub diagnostics: Vec<FrontendError>,
}

/// Compile an OSTW project rooted at `root`.
///
/// `main_text` is the input file's text and `main_path` its project-relative
/// path when the input is a file under `root` (`None` for stdin, in which
/// case `main_text` is used as the `entry_point` file's content).
pub fn compile(main_text: &str, main_path: Option<&str>, root: &Path) -> OstwOutcome {
    match load(main_text, main_path, root) {
        Ok(outcome) => outcome,
        Err(error) => OstwOutcome {
            project: None,
            error: Some(error),
            diagnostics: Vec::new(),
        },
    }
}

fn load(main_text: &str, main_path: Option<&str>, root: &Path) -> FrontendResult<OstwOutcome> {
    let ds_path = root.join("ds.toml");
    if !ds_path.is_file() {
        return Err(FrontendError::new(
            "ostw-ds-toml-missing",
            format!(
                "no ds.toml found in '{}'; OSTW projects are defined by a ds.toml project file",
                root.display()
            ),
        ));
    }
    let ds_text = std::fs::read_to_string(&ds_path).map_err(|error| {
        FrontendError::new(
            "ostw-ds-toml-unreadable",
            format!("cannot read ds.toml: {error}"),
        )
    })?;

    // Registry: ds.toml is id 0; sources follow in deterministic path order.
    let mut files = vec![FileRecord {
        id: 0,
        path: "ds.toml".to_string(),
        source: false,
        parsed: false,
        imports: Vec::new(),
        cst: None,
    }];

    let (entry, mut diagnostics) = parse_ds_toml(&ds_text, 0);

    let mut sources = walk_sources(root);
    sources.sort();
    for (index, path) in sources.iter().enumerate() {
        files.push(FileRecord {
            id: index as u32 + 1,
            path: path.clone(),
            source: true,
            parsed: false,
            imports: Vec::new(),
            cst: None,
        });
    }

    let entry = match entry {
        Some(entry) => entry,
        None => {
            diagnostics.push(FrontendError::new(
                "ostw-ds-toml-no-entry",
                "ds.toml does not declare an entry_point",
            ));
            return Ok(OstwOutcome {
                project: Some(Project {
                    entry: String::new(),
                    files,
                }),
                error: None,
                diagnostics,
            });
        }
    };

    // Validate the entry point against the source set.
    if !files.iter().any(|file| file.source && file.path == entry) {
        diagnostics.push(FrontendError::new(
            "ostw-entry-not-found",
            format!("entry_point '{entry}' is not a source file in the project"),
        ));
    }

    // Parse every source file. The input file (when it is part of the
    // closure) uses the caller-provided text so stdin/original text is
    // preserved; everything else is read from disk.
    let mut path_to_id: BTreeMap<String, u32> = BTreeMap::new();
    for file in &files {
        path_to_id.insert(file.path.clone(), file.id);
    }
    let main_id = main_path.and_then(|path| path_to_id.get(path).copied());
    for record in files.iter_mut() {
        if !record.source {
            continue;
        }
        let text = if Some(record.id) == main_id || (record.path == entry && main_path.is_none()) {
            main_text.to_string()
        } else {
            match std::fs::read_to_string(root.join(&record.path)) {
                Ok(text) => text,
                Err(error) => {
                    diagnostics.push(FrontendError::new(
                        "ostw-source-unreadable",
                        format!("cannot read '{}': {error}", record.path),
                    ));
                    continue;
                }
            }
        };
        let tokens = match lexer::lex(LexInput {
            file_id: FileId::from_index(record.id as usize),
            text: &text,
        }) {
            Ok(tokens) => tokens,
            Err(error) => {
                diagnostics.push(error);
                continue;
            }
        };
        match parser::parse(tokens) {
            Ok(file) => {
                record.parsed = true;
                record.cst = Some(file);
            }
            Err(error) => {
                diagnostics.push(error);
            }
        }
    }

    // Resolve imports relative to each importing file's directory.
    for record in files.iter_mut() {
        if !record.source {
            continue;
        }
        let Some(cst) = &record.cst else {
            continue;
        };
        let dir = parent_dir(&record.path);
        let mut imports = Vec::new();
        for import in &cst.imports {
            let resolved = match resolve_import_path(&dir, &import.path) {
                Some(path) => path,
                None => {
                    diagnostics.push(FrontendError::at(
                        "ostw-missing-import",
                        format!(
                            "import '{}' resolves outside the project closure",
                            import.path
                        ),
                        import.span,
                    ));
                    imports.push(ResolvedImport {
                        path: import.path.clone(),
                        span: import.span,
                        target: None,
                    });
                    continue;
                }
            };
            let target = path_to_id.get(&resolved).copied();
            if target.is_none() {
                diagnostics.push(FrontendError::at(
                    "ostw-missing-import",
                    format!(
                        "import '{}' does not exist in the project closure",
                        import.path
                    ),
                    import.span,
                ));
            }
            imports.push(ResolvedImport {
                path: import.path.clone(),
                span: import.span,
                target,
            });
        }
        record.imports = imports;
    }

    Ok(OstwOutcome {
        project: Some(Project { entry, files }),
        error: None,
        diagnostics,
    })
}

/// Walk the project root for `.ostw`/`.del` source files (recursive),
/// returning project-relative paths.
fn walk_sources(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
                if extension.eq_ignore_ascii_case("ostw") || extension.eq_ignore_ascii_case("del") {
                    if let Ok(relative) = path.strip_prefix(root) {
                        out.push(relative.to_string_lossy().replace('\\', "/"));
                    }
                }
            }
        }
    }
    out
}

/// Parse a minimal `ds.toml`: `key = "value"` lines. Only `entry_point` is
/// supported; any other key yields a structured unsupported-config
/// diagnostic. Returns the entry point (when present) and diagnostics.
fn parse_ds_toml(text: &str, file_id: u32) -> (Option<String>, Vec<FrontendError>) {
    let mut entry = None;
    let mut diagnostics = Vec::new();
    for (line_index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((raw_key, raw_value)) = line.split_once('=') else {
            let span = line_span(file_id, line_index, raw_line.len());
            diagnostics.push(FrontendError::at(
                "ostw-ds-toml-invalid",
                format!("invalid ds.toml line: '{line}'"),
                span,
            ));
            continue;
        };
        let key = raw_key.trim();
        let value = raw_value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'));
        let Some(value) = value else {
            let span = line_span(file_id, line_index, raw_line.len());
            diagnostics.push(FrontendError::at(
                "ostw-ds-toml-invalid",
                format!("ds.toml value for '{key}' must be a quoted string"),
                span,
            ));
            continue;
        };
        if key == "entry_point" {
            entry = Some(value.to_string());
        } else {
            let span = line_span(file_id, line_index, raw_line.len());
            diagnostics.push(FrontendError::at(
                "ostw-ds-toml-unsupported-key",
                format!("unsupported ds.toml key '{key}' (only entry_point is supported in this milestone)"),
                span,
            ));
        }
    }
    (entry, diagnostics)
}

fn line_span(file_id: u32, line_index: usize, line_len: usize) -> Span {
    let line = line_index as u32 + 1;
    Span::new(
        FileId::from_index(file_id as usize),
        Position::new(line, 1),
        Position::new(line, line_len as u32 + 1),
    )
}

/// The project-relative directory of a path (`""` for a root file).
fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(index) => path[..index].to_string(),
        None => String::new(),
    }
}

/// Resolve an import path written in `dir` to a normalized project-relative
/// path. Returns `None` when the path escapes the project root (`..` above
/// the root). `\` separators are normalized to `/`.
fn resolve_import_path(dir: &str, import_path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in dir.split('/') {
        if !part.is_empty() {
            parts.push(part);
        }
    }
    for part in import_path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}
