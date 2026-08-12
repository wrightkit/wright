//! Wright's native `.opy` frontend (milestone M7).
//!
//! Owns the source-language surface declared by the M7 support matrix
//! (`docs/opy/support-matrix.md`): a lexer, an indentation-aware
//! CST/parser with structured diagnostics and recovery, token-level
//! preprocessing (includes and `#!define` macros), semantic resolution, and
//! lowering into the existing Wright-owned Opy HIR contract
//! (`wright_core::hir::Program`). The frontend never depends on OverPy or
//! Node; the OverPy adapter remains a separate compatibility oracle.
//!
//! Pipeline: [`lexer::lex`] → [`preprocess::preprocess`] →
//! [`parser::parse`] → [`lower::lower`].

pub mod cst;
pub mod diag;
pub mod lexer;
pub mod lower;
pub mod parser;
pub mod preprocess;

use std::path::Path;

pub use diag::{FrontendError, FrontendResult};
pub use lower::lower;
pub use parser::parse;
pub use preprocess::preprocess;

/// The frontend's supported protocol identity for generated HIR.
pub const FRONTEND_NAME: &str = "wright/opy-native";
pub const FRONTEND_VERSION: &str = "0.1.0";

/// Compile one `.opy` source end-to-end into the Opy HIR contract:
/// preprocess (includes/defines) → parse (CST) → lower (HIR).
///
/// `main_path` is the file's display path recorded in the HIR file registry;
/// `root` is the include base. `compile` never requires Node or OverPy.
pub fn compile(
    source: &str,
    main_path: &str,
    root: &Path,
) -> FrontendResult<wright_core::hir::Program> {
    let (preprocessed, files) = preprocess(source, main_path, root)?;
    let parsed = parse(&preprocessed.tokens);
    if let Some(error) = parsed.errors.first() {
        return Err(error.clone());
    }
    let program = parsed
        .program
        .expect("program present when errors are empty");
    let defines = preprocessed
        .defines
        .iter()
        .map(|define| wright_core::hir::types::Define {
            name: define.name.clone(),
            is_function: define.is_function,
            span: define.span.map(Into::into),
        })
        .collect();
    let files = files
        .into_iter()
        .map(|file| wright_core::hir::types::SourceFile {
            id: file.id,
            path: file.path,
        })
        .collect();
    lower(&program, files, defines)
}
