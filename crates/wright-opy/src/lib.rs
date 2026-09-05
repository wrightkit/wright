//! Narrow Wright adapter for the owner-side `opy-rs` implementation.
//!
//! This crate owns no OPY parsing, semantic resolution, HIR, manifest, or
//! reconstruction rules. It preserves the historical Wright-facing boundary
//! while delegating those capabilities to `opy-rs`.

pub use opy_rs::{cst, diag, lexer, parser, preprocess, settings, support, tooling};

pub mod manifest {
    pub use opy_rs::manifest::{CatalogLink, Function, FunctionKind, Param, ParamDefault};

    use opy_rs::manifest::ManifestError;

    pub struct EnumDomain {
        pub domain: String,
        pub members: Vec<String>,
    }

    pub struct Manifest {
        pub functions: &'static [Function],
        inner: &'static opy_rs::manifest::Manifest,
    }

    impl Manifest {
        pub fn builtin() -> Result<Self, ManifestError> {
            let inner = opy_rs::manifest::Manifest::builtin()?;
            Ok(Self {
                functions: &inner.functions,
                inner,
            })
        }

        pub fn enum_domain(&self, name: &str) -> Option<EnumDomain> {
            if !self.inner.domain_identity(name) {
                return None;
            }
            let catalog = workshop_rs::catalog::Catalog::builtin().ok()?;
            let domain = catalog.enum_domain(name)?;
            Some(EnumDomain {
                domain: name.to_string(),
                members: domain
                    .members
                    .iter()
                    .map(|member| member.member.clone())
                    .collect(),
            })
        }
    }

    impl workshop_rs::signatures::ExpectedDomain for Manifest {
        fn expected_domain(&self, catalog_id: &str, arg_index: usize) -> Option<&str> {
            self.functions
                .iter()
                .find(|function| function.catalog_id.as_deref() == Some(catalog_id))
                .and_then(|function| function.params.get(arg_index))
                .and_then(|param| param.domain.as_deref())
        }
    }
}

pub use diag::{OpyError, OpyResult};

pub struct CompileOutcome {
    pub program: Option<workshop_rs::wir::Program>,
    pub error: Option<OpyError>,
    pub files: Vec<preprocess::FileRecord>,
}

fn compiler_error(error: opy_rs::IntegrationError) -> OpyError {
    let diagnostic = error.diagnostic;
    match diagnostic.span {
        Some(span) => OpyError::at(
            diagnostic.code,
            diagnostic.message,
            opy_rs::diag::Span::new(
                span.file,
                opy_rs::diag::Position::new(span.start.line, span.start.col),
                opy_rs::diag::Position::new(span.end.line, span.end.col),
            ),
        ),
        None => OpyError::new(diagnostic.code, diagnostic.message),
    }
}

pub fn compile(
    source: &str,
    main_path: &str,
    root: &std::path::Path,
) -> OpyResult<workshop_rs::wir::Program> {
    compile_with_overlay(source, main_path, root, &std::collections::BTreeMap::new())
}

pub fn compile_with_overlay(
    source: &str,
    main_path: &str,
    root: &std::path::Path,
    overlay: &std::collections::BTreeMap<String, String>,
) -> OpyResult<workshop_rs::wir::Program> {
    let outcome = compile_with_overlay_outcome(source, main_path, root, overlay);
    outcome
        .program
        .ok_or_else(|| outcome.error.expect("failed compile outcome has an error"))
}

pub fn compile_with_overlay_outcome(
    source: &str,
    main_path: &str,
    root: &std::path::Path,
    overlay: &std::collections::BTreeMap<String, String>,
) -> CompileOutcome {
    let owner = opy_rs::compile_with_overlay_outcome(source, main_path, root, overlay);
    let files = owner.files;
    let Some(hir) = owner.hir else {
        return CompileOutcome {
            program: None,
            error: owner.error,
            files,
        };
    };
    let compiler = match opy_rs::Compiler::new() {
        Ok(compiler) => compiler,
        Err(error) => {
            return CompileOutcome {
                program: None,
                error: Some(compiler_error(error)),
                files,
            };
        }
    };
    match compiler.compile_hir(&hir) {
        Ok(artifact) => CompileOutcome {
            program: Some(artifact.wir),
            error: None,
            files,
        },
        Err(error) => CompileOutcome {
            program: None,
            error: Some(compiler_error(error)),
            files,
        },
    }
}

pub mod reconstruct {
    pub use opy_rs::reconstruct::{ReconstructError, ReconstructIssue, reconstruct};
}
