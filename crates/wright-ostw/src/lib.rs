//! Wright's native OSTW frontend (milestone M13, issue #117).
//!
//! Owns the OSTW syntax/project surface evidenced by the pinned protect-ban
//! corpus: a lexer, a CST/parser with structured diagnostics, and the
//! `ds.toml` project model (`entry_point`) with quoted-import resolution.
//! This is syntax/project infrastructure only: no type/name resolution, no
//! HIR lowering, and no Workshop emission (those belong to #118 and later).
//! Upstream .NET/OSTW remains a reference-only oracle and never enters the
//! production dependency graph.
//!
//! Pipeline: [`lexer::lex`] → [`parser::parse`] → [`project::compile`].

pub mod cst;
pub mod diag;
pub mod lexer;
pub mod parser;
pub mod project;

use std::path::Path;

pub use diag::{FrontendError, FrontendResult};
pub use project::{FileRecord, OstwOutcome, Project, ResolvedImport};

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
