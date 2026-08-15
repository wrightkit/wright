//! The `ds.toml` project model, compilation graph, and quoted-import
//! resolution.
//!
//! An OSTW project is defined by a `ds.toml` in the project root. This task
//! supports the `entry_point` key (the project-relative entry source file)
//! and rejects other configuration keys with a structured diagnostic.
//!
//! Compilation membership follows the pinned reference's entry-point
//! semantics (#117): the project's compilation graph starts at
//! `ds.toml.entry_point` and recursively includes exactly the files reachable
//! through resolved import statements (a visited set makes cycles and
//! duplicate imports include each file once). Only reachable files are parsed
//! and only their imports resolved for project/check diagnostics; an
//! unreachable source with broken syntax or a missing import cannot make the
//! entry-point project fail. The workspace/source inventory (every
//! `.ostw`/`.del` under the root) is retained as a distinct, non-diagnostic
//! structure for tooling.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use wright_ir::source::{FileId, Position, Span};

use crate::cst;
use crate::diag::{FrontendError, FrontendResult};
use crate::lexer::{self, LexInput};
use crate::parser;

/// A compilation-graph file: `ds.toml` (id 0) and every entry-point
/// import-reachable source.
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
    /// The target file id when the import resolves inside the project,
    /// `None` when it points outside it (a missing-import diagnostic).
    pub target: Option<u32>,
}

/// The loaded OSTW project.
#[derive(Debug, Clone)]
pub struct Project {
    /// The `ds.toml` `entry_point` value.
    pub entry: String,
    /// The compilation graph: `ds.toml` at id 0, then the entry-point
    /// import-reachable closure in deterministic walk order.
    pub files: Vec<FileRecord>,
    /// The independent workspace/source inventory: every `.ostw`/`.del`
    /// under the project root, in sorted order. Tooling only — it never
    /// feeds project diagnostics or compilation membership.
    pub inventory: Vec<String>,
}

/// The outcome of a project compile: the registry is retained even when
/// diagnostics exist, so the driver can map spans to file identities.
#[derive(Debug, Clone)]
pub struct OstwOutcome {
    /// The project compilation graph (always populated when `ds.toml`
    /// loads).
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

    let (entry, mut diagnostics) = parse_ds_toml(&ds_text, 0);

    // The workspace/source inventory: every .ostw/.del under the root, in
    // sorted order. This is the resolution universe and a tooling inventory;
    // it is not compilation membership.
    let mut inventory = walk_sources(root);
    inventory.sort();

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
                    files: vec![FileRecord {
                        id: 0,
                        path: "ds.toml".to_string(),
                        source: false,
                        parsed: false,
                        imports: Vec::new(),
                        cst: None,
                    }],
                    inventory,
                }),
                error: None,
                diagnostics,
            });
        }
    };

    // Validate the entry point against the inventory.
    if !inventory.iter().any(|path| path == &entry) {
        diagnostics.push(FrontendError::new(
            "ostw-entry-not-found",
            format!("entry_point '{entry}' is not a source file in the project"),
        ));
    }

    // Build the compilation graph: breadth-first walk from the entry,
    // following resolved import targets. A visited set makes cycles and
    // duplicate imports include each file exactly once, and unreachable
    // sources never enter the graph (so their defects produce no project
    // diagnostics).
    let mut files = vec![FileRecord {
        id: 0,
        path: "ds.toml".to_string(),
        source: false,
        parsed: false,
        imports: Vec::new(),
        cst: None,
    }];
    let mut path_to_id: BTreeMap<String, u32> = BTreeMap::new();
    path_to_id.insert("ds.toml".to_string(), 0);
    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    if inventory.iter().any(|path| path == &entry) {
        queue.push_back(entry.clone());
    }
    // Imports whose target id is only known after the walk completes.
    let mut pending_imports: Vec<PendingImport> = Vec::new();

    while let Some(path) = queue.pop_front() {
        if !visited.insert(path.clone()) {
            continue;
        }
        let id = files.len() as u32;
        path_to_id.insert(path.clone(), id);
        let text = if (path == entry && main_path.is_none()) || Some(path.as_str()) == main_path {
            main_text.to_string()
        } else {
            match std::fs::read_to_string(root.join(&path)) {
                Ok(text) => text,
                Err(error) => {
                    diagnostics.push(FrontendError::new(
                        "ostw-source-unreadable",
                        format!("cannot read '{path}': {error}"),
                    ));
                    continue;
                }
            }
        };
        let mut record = FileRecord {
            id,
            path: path.clone(),
            source: true,
            parsed: false,
            imports: Vec::new(),
            cst: None,
        };
        let tokens = match lexer::lex(LexInput {
            file_id: FileId::from_index(id as usize),
            text: &text,
        }) {
            Ok(tokens) => tokens,
            Err(error) => {
                diagnostics.push(error);
                files.push(record);
                continue;
            }
        };
        match parser::parse(tokens) {
            Ok(file) => {
                let dir = parent_dir(&path);
                let mut discovered = Vec::new();
                for import in &file.imports {
                    match resolve_import_path(&dir, &import.path) {
                        None => {
                            diagnostics.push(FrontendError::at(
                                "ostw-missing-import",
                                format!(
                                    "import '{}' resolves outside the project closure",
                                    import.path
                                ),
                                import.span,
                            ));
                            record.imports.push(ResolvedImport {
                                path: import.path.clone(),
                                span: import.span,
                                target: None,
                            });
                        }
                        Some(resolved) => {
                            pending_imports.push(PendingImport {
                                from: id,
                                path: import.path.clone(),
                                span: import.span,
                                resolved: resolved.clone(),
                            });
                            discovered.push(resolved);
                        }
                    }
                }
                record.parsed = true;
                record.cst = Some(file);
                // Enqueue newly discovered targets (deduplicated on dequeue
                // and via the queue membership check).
                for target in discovered {
                    if inventory.iter().any(|inv| inv == &target)
                        && !visited.contains(&target)
                        && !queue.contains(&target)
                    {
                        queue.push_back(target);
                    }
                }
            }
            Err(error) => {
                diagnostics.push(error);
            }
        }
        files.push(record);
    }

    // Resolve import targets against the final id map and surface
    // missing-import diagnostics for in-closure import statements whose
    // target is not part of the project.
    for import in pending_imports {
        let target = path_to_id.get(&import.resolved).copied();
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
        if let Some(record) = files.get_mut(import.from as usize) {
            record.imports.push(ResolvedImport {
                path: import.path,
                span: import.span,
                target,
            });
        }
    }

    Ok(OstwOutcome {
        project: Some(Project {
            entry,
            files,
            inventory,
        }),
        error: None,
        diagnostics,
    })
}

/// An import statement whose target id is resolved after the graph walk.
struct PendingImport {
    /// The importing file's registry id.
    from: u32,
    /// The path exactly as written in the import statement.
    path: String,
    pub span: Span,
    /// The lexically resolved project-relative path.
    resolved: String,
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
