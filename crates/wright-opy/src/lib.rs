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
pub use preprocess::{preprocess, preprocess_with_overlay};

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
    compile_with_overlay(source, main_path, root, &std::collections::BTreeMap::new())
}

/// Compile with open-document overlays: includes resolve to overlay text
/// (keyed by the include string or the resolved canonical path) before the
/// filesystem, so unsaved editor buffers participate in include resolution.
pub fn compile_with_overlay(
    source: &str,
    main_path: &str,
    root: &Path,
    overlay: &std::collections::BTreeMap<String, String>,
) -> FrontendResult<wright_core::hir::Program> {
    let outcome = compile_with_overlay_outcome(source, main_path, root, overlay);
    match outcome.hir {
        Some(hir) => Ok(hir),
        None => Err(outcome
            .error
            .expect("a failed compile outcome always carries an error")),
    }
}

/// The outcome of a compile with overlays.
///
/// Unlike [`compile_with_overlay`], this retains the frontend file registry
/// even when parsing or lowering fails, so language tooling can map span file
/// ids to their actual source identities without building a diagnostics-only
/// project model.
pub struct CompileOutcome {
    pub hir: Option<wright_core::hir::Program>,
    pub error: Option<FrontendError>,
    pub files: Vec<preprocess::FileRecord>,
}

/// Compile with open-document overlays while retaining the frontend file
/// registry on parse/lower failure.
pub fn compile_with_overlay_outcome(
    source: &str,
    main_path: &str,
    root: &Path,
    overlay: &std::collections::BTreeMap<String, String>,
) -> CompileOutcome {
    let preprocess::PreprocessOutcome { result, files } =
        preprocess::preprocess_with_overlay_outcome(source, main_path, root, overlay);
    let preprocessed = match result {
        Ok((preprocessed, _)) => preprocessed,
        Err(error) => {
            return CompileOutcome {
                hir: None,
                error: Some(error),
                files,
            };
        }
    };
    let parsed = parse(&preprocessed.tokens);
    if let Some(error) = parsed.errors.first() {
        return CompileOutcome {
            hir: None,
            error: Some(error.clone()),
            files,
        };
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
    let hir_files = files
        .iter()
        .map(|file| wright_core::hir::types::SourceFile {
            id: file.id,
            path: file.path.clone(),
        })
        .collect();
    match lower(&program, hir_files, defines) {
        Ok(hir) => CompileOutcome {
            hir: Some(hir),
            error: None,
            files,
        },
        Err(error) => CompileOutcome {
            hir: None,
            error: Some(error),
            files,
        },
    }
}
