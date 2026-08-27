//! Narrow Wright adapter for the owner-side `opy-rs` implementation.
//!
//! This crate owns no OPY parsing, semantic resolution, HIR, manifest, or
//! reconstruction rules. It preserves the historical Wright-facing boundary
//! while delegating those capabilities to `opy-rs` and `opy-compiler`.

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

fn validate_builtin_enum_members(program: &opy_rs::hir::Program) -> OpyResult<()> {
    let catalog = workshop_rs::catalog::Catalog::builtin()
        .map_err(|error| OpyError::new("catalog-load", error.to_string()))?;
    for declaration in &program.declarations {
        match declaration {
            opy_rs::hir::Declaration::GlobalVariable { initializer, .. }
            | opy_rs::hir::Declaration::PlayerVariable { initializer, .. } => {
                if let Some(initializer) = initializer {
                    validate_expr(initializer, &catalog)?;
                }
            }
            opy_rs::hir::Declaration::Constant { value, .. } => {
                validate_expr(value, &catalog)?;
            }
            opy_rs::hir::Declaration::Macro { body, .. } => {
                validate_stmts(body, &catalog)?;
            }
            opy_rs::hir::Declaration::Subroutine { .. } => {}
        }
    }
    for rule in &program.rules {
        match rule {
            opy_rs::hir::RuleEntry::Rule(rule) => {
                for argument in &rule.event.args {
                    validate_expr(argument, &catalog)?;
                }
                for condition in &rule.conditions {
                    validate_expr(condition, &catalog)?;
                }
                validate_stmts(&rule.actions, &catalog)?;
            }
            opy_rs::hir::RuleEntry::SubroutineDef { body, .. } => {
                validate_stmts(body, &catalog)?;
            }
        }
    }
    Ok(())
}

fn validate_stmts(
    statements: &[opy_rs::hir::Stmt],
    catalog: &workshop_rs::catalog::Catalog,
) -> OpyResult<()> {
    for statement in statements {
        match statement {
            opy_rs::hir::Stmt::Expr { expr, .. } => validate_expr(expr, catalog)?,
            opy_rs::hir::Stmt::Assign { target, value, .. } => {
                validate_expr(target, catalog)?;
                validate_expr(value, catalog)?;
            }
            opy_rs::hir::Stmt::If {
                branches, r#else, ..
            } => {
                for branch in branches {
                    validate_expr(&branch.condition, catalog)?;
                    validate_stmts(&branch.body, catalog)?;
                }
                if let Some(body) = r#else {
                    validate_stmts(body, catalog)?;
                }
            }
            opy_rs::hir::Stmt::For {
                variable,
                iterable,
                body,
                ..
            } => {
                validate_expr(variable, catalog)?;
                validate_expr(iterable, catalog)?;
                validate_stmts(body, catalog)?;
            }
            opy_rs::hir::Stmt::While {
                condition, body, ..
            }
            | opy_rs::hir::Stmt::DoWhile {
                condition, body, ..
            } => {
                validate_expr(condition, catalog)?;
                validate_stmts(body, catalog)?;
            }
            opy_rs::hir::Stmt::Switch { value, arms, .. } => {
                validate_expr(value, catalog)?;
                for arm in arms {
                    match arm {
                        opy_rs::hir::SwitchArm::Case { value, body, .. } => {
                            validate_expr(value, catalog)?;
                            validate_stmts(body, catalog)?;
                        }
                        opy_rs::hir::SwitchArm::Default { body, .. } => {
                            validate_stmts(body, catalog)?;
                        }
                    }
                }
            }
            opy_rs::hir::Stmt::Break { .. }
            | opy_rs::hir::Stmt::CallSubroutine { .. }
            | opy_rs::hir::Stmt::Pass { .. } => {}
        }
    }
    Ok(())
}

fn validate_expr(
    expr: &opy_rs::hir::Expr,
    catalog: &workshop_rs::catalog::Catalog,
) -> OpyResult<()> {
    use opy_rs::hir::Expr;

    match expr {
        Expr::Enum {
            value_type,
            value,
            span,
        } => {
            if let Some(domain) = catalog.enum_domain(value_type)
                && !domain.members.iter().any(|member| member.member == *value)
            {
                let message = format!("enum '{value_type}' has no member '{value}'");
                return match span {
                    Some(span) => Err(OpyError::at(
                        "unknown-enum-member",
                        message,
                        opy_rs::diag::Span::new(
                            span.file,
                            opy_rs::diag::Position::new(span.start.line, span.start.col),
                            opy_rs::diag::Position::new(span.end.line, span.end.col),
                        ),
                    )),
                    None => Err(OpyError::new("unknown-enum-member", message)),
                };
            }
        }
        Expr::Array { elements, .. } => {
            for element in elements {
                validate_expr(element, catalog)?;
            }
        }
        Expr::Dict { entries, .. } => {
            for entry in entries {
                validate_expr(&entry.key, catalog)?;
                validate_expr(&entry.value, catalog)?;
            }
        }
        Expr::Comprehension {
            element,
            iterable,
            condition,
            ..
        } => {
            validate_expr(element, catalog)?;
            validate_expr(iterable, catalog)?;
            if let Some(condition) = condition {
                validate_expr(condition, catalog)?;
            }
        }
        Expr::Lambda { body, .. } => validate_expr(body, catalog)?,
        Expr::Vector { x, y, z, .. } => {
            validate_expr(x, catalog)?;
            validate_expr(y, catalog)?;
            validate_expr(z, catalog)?;
        }
        Expr::PlayerVar { player, .. } => validate_expr(player, catalog)?,
        Expr::Member { receiver, .. } => validate_expr(receiver, catalog)?,
        Expr::Call { args, .. } | Expr::MacroCall { args, .. } => {
            for argument in args {
                validate_expr(argument, catalog)?;
            }
        }
        Expr::ReceiverCall { receiver, args, .. } => {
            validate_expr(receiver, catalog)?;
            for argument in args {
                validate_expr(argument, catalog)?;
            }
        }
        Expr::Binary { left, right, .. } => {
            validate_expr(left, catalog)?;
            validate_expr(right, catalog)?;
        }
        Expr::Unary { operand, .. } => {
            validate_expr(operand, catalog)?;
        }
        Expr::Index { array, index, .. } => {
            validate_expr(array, catalog)?;
            validate_expr(index, catalog)?;
        }
        Expr::Format { args, .. } => {
            for argument in args {
                validate_expr(argument, catalog)?;
            }
        }
        Expr::Number { .. }
        | Expr::String { .. }
        | Expr::Bool { .. }
        | Expr::Null { .. }
        | Expr::StringModifier { .. }
        | Expr::Local { .. }
        | Expr::GlobalVar { .. }
        | Expr::EventPlayer { .. }
        | Expr::Constant { .. }
        | Expr::MacroParam { .. } => {}
    }
    Ok(())
}

pub struct CompileOutcome {
    pub program: Option<workshop_rs::wir::Program>,
    pub error: Option<OpyError>,
    pub files: Vec<preprocess::FileRecord>,
}

fn compiler_error(error: opy_compiler::IntegrationError) -> OpyError {
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
    if let Err(error) = validate_builtin_enum_members(&hir) {
        return CompileOutcome {
            program: None,
            error: Some(error),
            files,
        };
    }

    let compiler = match opy_compiler::Compiler::new() {
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
    pub use opy_compiler::reconstruct::{ReconstructError, ReconstructIssue, reconstruct};
}
