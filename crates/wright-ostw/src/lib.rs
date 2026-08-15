//! Wright's native OSTW frontend (milestone M13, issues #117/#118).
//!
//! Owns the OSTW surface evidenced by the pinned protect-ban corpus: a
//! lexer, a CST/parser with structured diagnostics, the `ds.toml` project
//! model (`entry_point`) with quoted-import resolution (#117), and a
//! semantic phase that resolves the entry-point reachable graph into
//! frontend-neutral Wright HIR (#118). Workshop actions/values/enums resolve
//! through the canonical Wright-owned Workshop catalog
//! (`wright-workshop`'s `catalog`), with only OSTW source-name bindings kept
//! here; no OSTW game-derived table is imported. Upstream .NET/OSTW remains a
//! reference-only oracle and never enters the production dependency graph.
//!
//! Pipeline: [`lexer::lex`] → [`parser::parse`] → [`project::compile`] →
//! [`semantic::compile`].

pub mod cst;
pub mod diag;
pub mod lexer;
pub mod parser;
pub mod project;
pub mod reconstruct;
pub mod semantic;
pub mod signature;

use std::path::Path;

pub use diag::{FrontendError, FrontendResult};
pub use project::{FileRecord, OstwOutcome, Project, ResolvedImport};
pub use semantic::{SemanticOutcome, compile as compile_semantic};

/// The frontend's supported identity.
pub const FRONTEND_NAME: &str = "wright/ostw-native";
pub const FRONTEND_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Load and parse an OSTW project rooted at `root`.
///
/// `main_text` is the input file's text; `main_path` is its project-relative
/// path when the input is a file under `root` (`None` for stdin). The
/// outcome retains the file registry and every structured diagnostic, so the
/// driver maps spans to file identities through the shared provenance
/// contracts even when the project does not load cleanly.
pub fn compile(main_text: &str, main_path: Option<&str>, root: &Path) -> OstwOutcome {
    project::compile(main_text, main_path, root)
}

/// Load the project and resolve its semantic surface into frontend-neutral
/// HIR (#118). The returned HIR validates structurally; boundary forms
/// (missing imports, Cursor/Math, classes, define function macros) surface
/// as deterministic structured diagnostics in the outcome.
pub fn compile_with_semantics(
    main_text: &str,
    main_path: Option<&str>,
    root: &Path,
) -> (OstwOutcome, SemanticOutcome) {
    let project_outcome = project::compile(main_text, main_path, root);
    match &project_outcome.project {
        Some(project) => {
            let semantic = semantic::compile(project);
            (project_outcome, semantic)
        }
        None => {
            let semantic = SemanticOutcome {
                hir: None,
                diagnostics: Vec::new(),
            };
            (project_outcome, semantic)
        }
    }
}
