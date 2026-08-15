//! The reusable compiler/session driver (M6, issue #37).
//!
//! [`CompilerSession`] is the single orchestration path shared by the CLI,
//! library consumers, and (in later milestones) tool APIs and LSP: input
//! resolution → frontend selection → validation → lowering → analysis →
//! emission. Frontends are selected by [`SourceKind`] behind one contract, so
//! the M7 native `.opy` frontend can replace the temporary adapter bridge
//! without changing callers. Every workflow returns a typed [`Envelope`]
//! whose JSON serialization is the machine-readable CLI contract.

use std::path::Path;
use std::sync::Arc;

use wright_analyzer::registry::LintConfig;
use wright_analyzer::service::{Origin as ServiceOrigin, Request, SemanticService};
use wright_ir::wir;

use crate::config::{SessionConfig, SourceKind};
use crate::diag::{Diagnostic, Origin, Position, SourceSpan, Stage};
use crate::input::{self, ResolvedInput};
use crate::result::{
    AnalyzeResult, CheckResult, CompileResult, CompiledOutput, Envelope, InspectResult, LintResult,
    OstwFileSummary, OstwProjectSummary, exit_code_from, version_info,
};
use crate::{input_identity, opy};

/// A successfully loaded program with its input and origin metadata.
#[derive(Clone)]
pub struct Loaded {
    /// The validated Workshop IR program. Empty for OSTW loads that did not
    /// reach a semantic HIR (the frontend outcome is carried in [`Loaded::ostw`]).
    pub program: Arc<wir::Program>,
    /// The native OSTW frontend outcome, present only for `.ostw`/`.del`
    /// inputs (#117).
    pub ostw: Option<Arc<wright_ostw::OstwOutcome>>,
    /// The #118 semantic-phase outcome (frontend-neutral HIR plus its
    /// structured boundary diagnostics), present only for OSTW inputs that
    /// loaded their project.
    pub ostw_semantic: Option<Arc<wright_ostw::SemanticOutcome>>,
    /// Origin metadata carried into diagnostics and results.
    pub origin: Origin,
    /// The resolved input.
    pub input: ResolvedInput,
}

/// One reusable compiler session.
pub struct CompilerSession {
    /// The session configuration (input, frontend, overrides, format).
    pub config: SessionConfig,
    catalog: wright_workshop::catalog::Catalog,
    loaded: Option<Loaded>,
    diagnostics: Vec<Diagnostic>,
}

impl CompilerSession {
    /// Build a session from a configuration.
    pub fn new(config: SessionConfig) -> Result<CompilerSession, Diagnostic> {
        let catalog = wright_workshop::catalog::Catalog::builtin().map_err(|error| {
            Diagnostic::error(
                "catalog-error",
                Stage::Internal,
                format!("cannot load the built-in Workshop catalog: {error}"),
            )
        })?;
        Ok(CompilerSession {
            config,
            catalog,
            loaded: None,
            diagnostics: Vec::new(),
        })
    }

    /// Load (or reuse) the validated program for this session.
    ///
    /// Loading is idempotent: repeated calls return the same program without
    /// re-reading the input. Returns an owned snapshot so callers can hold it
    /// while mutating the session.
    pub fn load(&mut self) -> Result<Loaded, Diagnostic> {
        if let Some(loaded) = &self.loaded {
            return Ok(loaded.clone());
        }
        let mut resolved = input::resolve(&self.config)?;
        if resolved.kind == SourceKind::Ostw {
            return self.load_ostw(&mut resolved);
        }
        let mut program = match resolved.kind {
            SourceKind::Workshop => {
                let (program, locale) = self.load_workshop(&resolved)?;
                resolved.origin.locale = Some(locale);
                program
            }
            SourceKind::Protocol => {
                let json = resolved.text.clone();
                self.load_protocol(&json, &resolved)?
            }
            SourceKind::Opy => {
                if opy::adapter_fallback_requested() {
                    let json = opy::run_adapter(&resolved)?;
                    self.load_protocol(&json, &resolved)?
                } else {
                    // Default: the native Rust `.opy` frontend (no Node/OverPy).
                    // Use the outcome form so the file registry survives errors
                    // and diagnostics can name included files (#83).
                    let outcome = wright_opy::compile_with_overlay_outcome(
                        &resolved.text,
                        &resolved.display,
                        &resolved.root,
                        &std::collections::BTreeMap::new(),
                    );
                    let program = match outcome.hir {
                        Some(hir) => hir,
                        None => {
                            let error = outcome
                                .error
                                .expect("a failed compile outcome always carries an error");
                            return Err(opy_diag(error, &outcome.files, &resolved));
                        }
                    };
                    self.load_hir(program, &resolved)?
                }
            }
            SourceKind::Auto => {
                return Err(Diagnostic::error(
                    "input-kind-unknown",
                    Stage::Discovery,
                    "input kind could not be detected; pass `--kind opy|ostw|workshop|protocol`",
                ));
            }
            SourceKind::Ostw => unreachable!("OSTW dispatches to load_ostw above"),
        };
        // Apply the selected transformation profile (validated before/after).
        if self.config.profile != wright_transform::Profile::Off {
            wright_transform::run(&mut program, self.config.profile).map_err(|error| {
                Diagnostic::error(
                    "transform-error",
                    Stage::Internal,
                    format!("WIR transformation failed: {error}"),
                )
            })?;
        }
        let loaded = Loaded {
            program: Arc::new(program),
            ostw: None,
            ostw_semantic: None,
            origin: resolved.origin.clone(),
            input: resolved,
        };
        self.loaded = Some(loaded.clone());
        Ok(loaded)
    }

    /// Load an OSTW project through the shared session path: the native
    /// frontend parses the `ds.toml` project closure, resolves imports, and
    /// runs the #118 semantic phase; the resolved HIR is lowered through the
    /// shared validate→lower→validate path into the session program, and the
    /// project outcome (file registry + project and semantic diagnostics) is
    /// retained on the session so spans keep their multi-file provenance.
    fn load_ostw(&mut self, resolved: &mut ResolvedInput) -> Result<Loaded, Diagnostic> {
        let relative = resolved
            .path
            .as_ref()
            .and_then(|path| path.strip_prefix(&resolved.root).ok())
            .map(|relative| relative.to_string_lossy().replace('\\', "/"));
        let (outcome, semantic) = wright_ostw::compile_with_semantics(
            &resolved.text,
            relative.as_deref(),
            &resolved.root,
        );
        if let Some(error) = &outcome.error {
            return Err(ostw_diag(error.clone(), &outcome, resolved));
        }
        let program = match &semantic.hir {
            Some(hir) => self.load_ir_model(hir, resolved)?,
            None => wir::Program::default(),
        };
        let loaded = Loaded {
            program: Arc::new(program),
            ostw: Some(Arc::new(outcome)),
            ostw_semantic: Some(Arc::new(semantic)),
            origin: resolved.origin.clone(),
            input: resolved.clone(),
        };
        self.loaded = Some(loaded.clone());
        Ok(loaded)
    }

    /// Ingest an already-resolved internal HIR model (e.g. the #118 semantic
    /// HIR of an OSTW project) through the shared validate→lower→validate
    /// path shared with the protocol/OPY frontends.
    fn load_ir_model(
        &mut self,
        model: &wright_ir::hir::Program,
        resolved: &ResolvedInput,
    ) -> Result<wir::Program, Diagnostic> {
        model
            .validate()
            .map_err(|error| ir_diag("validation-error", Stage::Validation, error, resolved))?;
        let program = wright_ir::lower::lower(model)
            .map_err(|error| ir_diag("lower-error", Stage::Lowering, error, resolved))?;
        program
            .validate()
            .map_err(|error| ir_diag("validation-error", Stage::Validation, error, resolved))?;
        Ok(program)
    }

    /// The diagnostics accumulated by the last workflow run.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// The locale a Workshop input resolved to, if the last load was
    /// Workshop-origin.
    pub fn resolved_locale(&self) -> Option<String> {
        self.loaded
            .as_ref()
            .and_then(|loaded| loaded.origin.locale.clone())
    }

    fn load_workshop(
        &mut self,
        resolved: &ResolvedInput,
    ) -> Result<(wir::Program, String), Diagnostic> {
        let override_locale = self
            .config
            .locale
            .as_deref()
            .map(wright_workshop::catalog::Locale::new);
        let locale = wright_workshop::detect::resolve_locale(
            &resolved.text,
            &self.catalog,
            override_locale.as_ref(),
        )
        .map_err(|error| workshop_diag(error, resolved))?;
        // Context-sensitive bare-enum resolution (#111): the #109 canonical
        // signature metadata pins the expected domain for call arguments, so
        // emitter-produced text like `Chase Global Variable Over Time(...,
        // None)` reparses instead of failing on the ambiguous `None`. The
        // canonical Workshop catalog supplies the remaining domains the
        // manifest does not document (e.g. Create HUD Text's `HudReeval`
        // reevaluation argument, #118).
        let manifest = wright_opy::manifest::Manifest::builtin().map_err(|error| {
            Diagnostic::error(
                "manifest-error",
                Stage::Frontend,
                format!("cannot load the OPY semantic compatibility manifest: {error}"),
            )
        })?;
        let context = wright_core::signatures::ChainedExpectedDomain::new(manifest, &self.catalog);
        let program = wright_workshop::parser::parse_with_context(
            &resolved.text,
            &self.catalog,
            &locale,
            &context,
        )
        .map_err(|error| workshop_diag(error, resolved))?;
        program
            .validate()
            .map_err(|error| ir_diag("validation-error", Stage::Validation, error, resolved))?;
        Ok((program, locale.to_string()))
    }

    fn load_protocol(
        &mut self,
        json: &str,
        resolved: &ResolvedInput,
    ) -> Result<wir::Program, Diagnostic> {
        let protocol =
            wright_core::hir::parse_str(json).map_err(|error| hir_diag(error, resolved))?;
        self.load_hir(protocol, resolved)
    }

    /// Ingest an already-parsed Opy HIR program: validate, convert, lower.
    fn load_hir(
        &mut self,
        protocol: wright_core::hir::Program,
        resolved: &ResolvedInput,
    ) -> Result<wir::Program, Diagnostic> {
        // The native path is protocol-validated here for the first time
        // (settings domain checks against the emission table, #86); the
        // adapter path validates inside parse_str, so this is a double
        // validation there — acceptable.
        protocol
            .validate()
            .map_err(|error| hir_diag(error, resolved))?;
        let model = protocol
            .to_ir()
            .map_err(|error| ir_diag("convert-error", Stage::Lowering, error, resolved))?;
        let program = wright_ir::lower::lower(&model)
            .map_err(|error| ir_diag("lower-error", Stage::Lowering, error, resolved))?;
        program
            .validate()
            .map_err(|error| ir_diag("validation-error", Stage::Validation, error, resolved))?;
        Ok(program)
    }

    /// `compile`: load, emit localized Workshop text, and write it out.
    pub fn compile(&mut self) -> Envelope<CompileResult> {
        let command = "compile";
        let mut result = CompileResult::default();
        let output = match self.compile_output() {
            Ok(output) => output,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, result);
            }
        };
        match &self.config.output {
            Some(path) => {
                if let Err(error) = std::fs::write(path, &output.text) {
                    self.diagnostics.push(Diagnostic::error(
                        "output-io",
                        Stage::Emission,
                        format!("cannot write output '{}': {error}", path.display()),
                    ));
                    return self.finish(command, result);
                }
                result.output = Some(CompiledOutput {
                    written_to: crate::input::display_path(path),
                    ..output
                });
            }
            None => {
                result.output = Some(CompiledOutput {
                    written_to: "stdout".to_string(),
                    ..output
                });
            }
        }
        self.finish(command, result)
    }

    fn compile_output(&mut self) -> Result<CompiledOutput, Diagnostic> {
        let loaded = self.load()?;
        if loaded.ostw.is_some() {
            // OSTW (#119): the load lowered the semantic HIR into the
            // session program. Project-boundary diagnostics (missing
            // imports) and semantic-boundary diagnostics are errors — an
            // unsupported or unresolved reachable form fails compilation
            // with a deterministic, structured, source-located diagnostic
            // instead of being deferred to emission.
            let outcome = loaded
                .ostw
                .as_ref()
                .expect("an OSTW load always carries its frontend outcome");
            let semantic = loaded.ostw_semantic.as_ref();
            let error = outcome
                .diagnostics
                .iter()
                .chain(
                    semantic
                        .map(|semantic| semantic.diagnostics.as_slice())
                        .unwrap_or_default(),
                )
                .next();
            if let Some(error) = error {
                return Err(ostw_diag(error.clone(), outcome, &loaded.input));
            }
        }
        let locale = loaded
            .origin
            .locale
            .clone()
            .map(|locale| wright_workshop::catalog::Locale::new(&locale))
            .unwrap_or_else(|| wright_workshop::catalog::Locale::new("en-US"));
        let text = wright_workshop::emitter::emit(&loaded.program, &self.catalog, &locale)
            .map_err(|error| workshop_diag(error, &loaded.input))?;
        let sha256 = input_identity(&text);
        Ok(CompiledOutput {
            text,
            sha256,
            locale: locale.to_string(),
            written_to: String::new(),
            input_identity: loaded.input.identity.clone(),
        })
    }

    /// `check`: load, validate, and surface analysis findings as diagnostics.
    pub fn check(&mut self) -> Envelope<CheckResult> {
        let command = "check";
        let loaded = match self.load() {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, CheckResult { ostw: None });
            }
        };
        if let Some(outcome) = &loaded.ostw {
            // OSTW: report the frontend project + #118 semantic boundary
            // diagnostics through the shared diagnostics contract, and carry
            // the project summary (#117).
            self.push_ostw_diagnostics(&loaded);
            let summary = ostw_project_summary(outcome);
            return self.finish(
                command,
                CheckResult {
                    ostw: Some(summary),
                },
            );
        }
        self.attach_analysis(&loaded);
        self.finish(command, CheckResult { ostw: None })
    }

    /// `analyze`: load and produce the semantic summary and findings.
    pub fn analyze(&mut self) -> Envelope<AnalyzeResult> {
        let command = "analyze";
        let loaded = match self.load() {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, AnalyzeResult::default());
            }
        };
        if loaded.ostw.is_some() {
            // OSTW: surface the frontend + semantic boundary diagnostics,
            // then run the shared semantic service over the lowered program.
            self.push_ostw_diagnostics(&loaded);
        }
        let service = match self.service(&loaded) {
            Ok(service) => service,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, AnalyzeResult::default());
            }
        };
        let program = service_response(&service, &Request::Program);
        let mut findings = service_response(&service, &Request::GetFindings);
        resolve_finding_span_paths(&mut findings, &loaded);
        self.finish(command, AnalyzeResult { program, findings })
    }

    /// `inspect`: load and produce the structural/semantic program model.
    pub fn inspect(&mut self) -> Envelope<InspectResult> {
        let command = "inspect";
        let loaded = match self.load() {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, InspectResult::default());
            }
        };
        if loaded.ostw.is_some() {
            // OSTW: surface the frontend + semantic boundary diagnostics,
            // then run the shared semantic service over the lowered program.
            self.push_ostw_diagnostics(&loaded);
        }
        let service = match self.service(&loaded) {
            Ok(service) => service,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, InspectResult::default());
            }
        };
        let program = service_response(&service, &Request::Program);
        let rules = service_response(&service, &Request::ListRules);
        let symbols = service_response(&service, &Request::ListSymbols { kind: None });
        let references: serde_json::Value = {
            let symbol_ids: Vec<u32> = symbols
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|symbol| symbol.get("id").and_then(serde_json::Value::as_u64))
                        .map(|id| id as u32)
                        .collect()
                })
                .unwrap_or_default();
            let list: Vec<serde_json::Value> = symbol_ids
                .iter()
                .map(|id| service_response(&service, &Request::FindReferences { symbol: *id }))
                .collect();
            serde_json::Value::Array(list)
        };
        self.finish(
            command,
            InspectResult {
                program,
                rules,
                symbols,
                references,
            },
        )
    }

    /// `lint`: load and produce the source identity, program summary, rule
    /// metadata, effective configuration, and findings (M12, #98).
    ///
    /// Lint findings are reported in `result.findings`, not in the envelope
    /// diagnostics (like `analyze`). Rule enable/disable and severity come
    /// from `self.config.lint`, the same configuration the CLI flags and
    /// programmatic consumers set.
    pub fn lint(&mut self) -> Envelope<LintResult> {
        let command = "lint";
        let loaded = match self.load() {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, LintResult::default());
            }
        };
        if loaded.ostw.is_some() {
            // OSTW: surface the frontend + semantic boundary diagnostics,
            // then run the shared semantic service over the lowered program.
            self.push_ostw_diagnostics(&loaded);
        }
        let service = match self.service_with(&loaded, self.config.lint.clone()) {
            Ok(service) => service,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, LintResult::default());
            }
        };
        let program = service_response(&service, &Request::Program);
        let lint_rules = service_response(&service, &Request::LintRules);
        let mut findings = service_response(&service, &Request::GetFindings);
        resolve_finding_span_paths(&mut findings, &loaded);
        let (rules, config) = match lint_rules {
            serde_json::Value::Object(mut object) => (
                object
                    .remove("rules")
                    .unwrap_or_else(|| serde_json::json!([])),
                object
                    .remove("config")
                    .unwrap_or_else(|| serde_json::json!({})),
            ),
            _ => (serde_json::json!([]), serde_json::json!({})),
        };
        self.finish(
            command,
            LintResult {
                input_identity: loaded.input.identity.clone(),
                program,
                rules,
                config,
                findings,
            },
        )
    }

    /// Build the semantic service over a loaded program.
    fn service<'a>(&self, loaded: &'a Loaded) -> Result<SemanticService<'a>, Diagnostic> {
        self.service_with(loaded, LintConfig::default())
    }

    /// Build the semantic service over a loaded program with an explicit lint
    /// configuration.
    fn service_with<'a>(
        &self,
        loaded: &'a Loaded,
        config: LintConfig,
    ) -> Result<SemanticService<'a>, Diagnostic> {
        let origin = ServiceOrigin {
            kind: loaded.origin.kind.clone(),
            locale: loaded.origin.locale.clone(),
        };
        SemanticService::with_origin_and_config(&loaded.program, origin, config)
            .map_err(|error| ir_diag("analysis-error", Stage::Analysis, error, &loaded.input))
    }

    /// Attach semantic-analysis findings to the diagnostic list (for `check`).
    fn attach_analysis(&mut self, loaded: &Loaded) {
        let Ok(service) = self.service(loaded) else {
            return;
        };
        let findings = service_response(&service, &Request::GetFindings);
        let Some(findings) = findings.as_array() else {
            return;
        };
        for finding in findings {
            let code = finding
                .get("code")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("finding");
            let severity = match finding.get("severity").and_then(serde_json::Value::as_str) {
                Some("error") => crate::diag::Severity::Error,
                Some("warning") => crate::diag::Severity::Warning,
                _ => crate::diag::Severity::Info,
            };
            let message = finding
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            self.diagnostics.push(Diagnostic {
                code: code.to_string(),
                stage: Stage::Analysis,
                severity,
                message,
                span: span_from_json(finding.get("span")),
                source: Some(loaded.origin.clone()),
            });
        }
    }

    fn finish<T: serde::Serialize>(&mut self, command: &str, result: T) -> Envelope<T> {
        let diagnostics = std::mem::take(&mut self.diagnostics);
        let has_error = diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == crate::diag::Severity::Error);
        let exit = if has_error {
            exit_code_from(&diagnostics)
        } else {
            crate::result::exit::SUCCESS
        };
        Envelope {
            wright: version_info(),
            command: command.to_string(),
            ok: !has_error,
            exit,
            diagnostics,
            result,
        }
    }

    /// Surface the OSTW frontend project diagnostics plus the #118
    /// semantic-phase boundary diagnostics through the shared diagnostic
    /// contract, so every workflow reports the same structured,
    /// source-located boundary signals as the OPY/Workshop frontends.
    fn push_ostw_diagnostics(&mut self, loaded: &Loaded) {
        let Some(outcome) = &loaded.ostw else {
            return;
        };
        for diagnostic in &outcome.diagnostics {
            self.diagnostics
                .push(ostw_diag(diagnostic.clone(), outcome, &loaded.input));
        }
        if let Some(semantic) = &loaded.ostw_semantic {
            for diagnostic in &semantic.diagnostics {
                self.diagnostics
                    .push(ostw_diag(diagnostic.clone(), outcome, &loaded.input));
            }
        }
    }
}

/// Extract the `result` payload of a semantic-service request as JSON.
fn service_response(service: &SemanticService<'_>, request: &Request) -> serde_json::Value {
    match service.handle(request) {
        wright_analyzer::service::Response::Ok { result } => result,
        wright_analyzer::service::Response::Error { .. } => serde_json::Value::Null,
    }
}

/// Convert a JSON span value (from the semantic service) to a diagnostic span.
fn span_from_json(value: Option<&serde_json::Value>) -> Option<SourceSpan> {
    let value = value?;
    let file = value.get("file")?.as_u64()? as usize;
    let start = value.get("start")?;
    let end = value.get("end")?;
    Some(SourceSpan {
        file,
        path: format!("<file {file}>"),
        start: Position {
            line: start.get("line")?.as_u64()? as u32,
            col: start.get("col")?.as_u64()? as u32,
        },
        end: Position {
            line: end.get("line")?.as_u64()? as u32,
            col: end.get("col")?.as_u64()? as u32,
        },
    })
}

/// Add the resolved `path` to every finding span.
///
/// File 0 is the main input and resolves root-relative to the include root
/// (`--root`, defaulting to the input's directory); other files resolve from
/// the program file registry, joined with the root when the registry path is
/// relative. `<file N>` is the fallback when no registry entry resolves
/// (matching the [`span_from_json`] convention), and stdin inputs fall back
/// to their display identity (`<stdin>`).
pub(crate) fn resolve_finding_span_paths(findings: &mut serde_json::Value, loaded: &Loaded) {
    let Some(list) = findings.as_array_mut() else {
        return;
    };
    for finding in list {
        let Some(span) = finding.get_mut("span") else {
            continue;
        };
        if !span.is_object() {
            continue;
        }
        let path = span
            .get("file")
            .and_then(serde_json::Value::as_u64)
            .map(|file| {
                if file == 0 {
                    root_relative(loaded.input.path.as_deref(), &loaded.input.root)
                        .unwrap_or_else(|| loaded.input.display.clone())
                } else {
                    loaded
                        .program
                        .files
                        .get(wright_ir::source::FileId::from_index(file as usize))
                        .map(|source_file| {
                            root_relative(
                                Some(&loaded.input.root.join(&source_file.path)),
                                &loaded.input.root,
                            )
                            .unwrap_or_else(|| format!("<file {file}>"))
                        })
                        .unwrap_or_else(|| format!("<file {file}>"))
                }
            })
            .unwrap_or_else(|| loaded.input.display.clone());
        span["path"] = serde_json::Value::String(path);
    }
}

/// The root-relative form of `path` when it sits under `root`.
///
/// `canonicalize` makes both paths absolute and resolves symlinks, so
/// absolute, relative, and cwd-relative spellings of the same file all
/// produce the same root-relative result. Returns `None` when there is no
/// path (stdin) or the path is not under the root.
fn root_relative(path: Option<&Path>, root: &Path) -> Option<String> {
    let path = path?;
    let abs_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    // A single-component relative input (`loop.opy`) has an empty parent, so
    // the resolved root is the empty path; canonicalizing it fails, so treat
    // it as the cwd (its directory by definition).
    let abs_root = match root.canonicalize() {
        Ok(root) => root,
        Err(_) if root.as_os_str().is_empty() => std::env::current_dir().ok()?,
        Err(_) => root.to_path_buf(),
    };
    match abs_path.strip_prefix(&abs_root) {
        Ok(relative) if !relative.as_os_str().is_empty() => Some(relative.display().to_string()),
        _ => None,
    }
}

/// Map a Workshop-language error to a driver diagnostic.
fn workshop_diag(error: wright_workshop::WorkshopError, resolved: &ResolvedInput) -> Diagnostic {
    let (code, stage, span) = match &error {
        wright_workshop::WorkshopError::Catalog(catalog) => {
            return Diagnostic::error(
                "catalog-error",
                Stage::Internal,
                format!("{}: {}", catalog.code, catalog.message),
            );
        }
        wright_workshop::WorkshopError::Unknown { kind, span, .. } => (
            format!("unknown-{kind}"),
            Stage::Frontend,
            span.map(|span| SourceSpan {
                file: span.file.index(),
                path: resolved.display.clone(),
                start: Position {
                    line: span.start.line,
                    col: span.start.col,
                },
                end: Position {
                    line: span.end.line,
                    col: span.end.col,
                },
            }),
        ),
        wright_workshop::WorkshopError::Malformed { span, .. } => (
            "parse-error".to_string(),
            Stage::Frontend,
            span.map(|span| SourceSpan {
                file: span.file.index(),
                path: resolved.display.clone(),
                start: Position {
                    line: span.start.line,
                    col: span.start.col,
                },
                end: Position {
                    line: span.end.line,
                    col: span.end.col,
                },
            }),
        ),
        wright_workshop::WorkshopError::Unsupported { span, .. } => (
            "unsupported-construct".to_string(),
            Stage::Frontend,
            span.map(|span| SourceSpan {
                file: span.file.index(),
                path: resolved.display.clone(),
                start: Position {
                    line: span.start.line,
                    col: span.start.col,
                },
                end: Position {
                    line: span.end.line,
                    col: span.end.col,
                },
            }),
        ),
    };
    Diagnostic {
        code,
        stage,
        severity: crate::diag::Severity::Error,
        message: error.to_string(),
        span,
        source: Some(resolved.origin.clone()),
    }
}

/// Map a native frontend error to a driver diagnostic.
///
/// Span paths resolve through the frontend file registry so a failure inside
/// an included file names that file; file 0 (the main file) carries the
/// resolved display path by construction (#83).
fn opy_diag(
    error: wright_opy::FrontendError,
    files: &[wright_opy::preprocess::FileRecord],
    resolved: &ResolvedInput,
) -> Diagnostic {
    let span = error.span.map(|span| SourceSpan {
        file: span.file as usize,
        path: files
            .get(span.file as usize)
            .map(|file| file.path.clone())
            .unwrap_or_else(|| resolved.display.clone()),
        start: Position {
            line: span.start.line,
            col: span.start.col,
        },
        end: Position {
            line: span.end.line,
            col: span.end.col,
        },
    });
    Diagnostic {
        code: error.code,
        stage: Stage::Frontend,
        severity: crate::diag::Severity::Error,
        message: error.message,
        span,
        source: Some(resolved.origin.clone()),
    }
}

/// Map a native OSTW frontend error to a driver diagnostic.
///
/// Span paths resolve through the OSTW project registry, so a failure inside
/// an imported file names that file with its project-relative path.
fn ostw_diag(
    error: wright_ostw::FrontendError,
    outcome: &wright_ostw::OstwOutcome,
    resolved: &ResolvedInput,
) -> Diagnostic {
    let span = error.span.map(|span| SourceSpan {
        file: span.file.index(),
        path: outcome
            .project
            .as_ref()
            .and_then(|project| project.files.get(span.file.index()))
            .map(|file| file.path.clone())
            .unwrap_or_else(|| resolved.display.clone()),
        start: Position {
            line: span.start.line,
            col: span.start.col,
        },
        end: Position {
            line: span.end.line,
            col: span.end.col,
        },
    });
    Diagnostic {
        code: error.code,
        stage: Stage::Frontend,
        severity: crate::diag::Severity::Error,
        message: error.message,
        span,
        source: Some(resolved.origin.clone()),
    }
}

/// The `check`-result summary of an OSTW project outcome.
fn ostw_project_summary(outcome: &wright_ostw::OstwOutcome) -> OstwProjectSummary {
    let Some(project) = &outcome.project else {
        return OstwProjectSummary {
            entry: String::new(),
            files: Vec::new(),
            inventory: Vec::new(),
        };
    };
    let path_by_id: std::collections::BTreeMap<u32, String> = project
        .files
        .iter()
        .map(|file| (file.id, file.path.clone()))
        .collect();
    let files = project
        .files
        .iter()
        .map(|file| OstwFileSummary {
            path: file.path.clone(),
            id: file.id,
            source: file.source,
            parsed: file.parsed,
            imports: file
                .imports
                .iter()
                .filter_map(|import| {
                    import
                        .target
                        .and_then(|target| path_by_id.get(&target).cloned())
                })
                .collect(),
        })
        .collect();
    OstwProjectSummary {
        entry: project.entry.clone(),
        files,
        inventory: project.inventory.clone(),
    }
}

/// Map an Opy HIR ingestion error to a driver diagnostic.
fn hir_diag(error: wright_core::hir::HirError, resolved: &ResolvedInput) -> Diagnostic {
    let stage = match &error {
        wright_core::hir::HirError::Invalid { .. } => Stage::Validation,
        _ => Stage::Frontend,
    };
    let span = error.span().map(|span| SourceSpan {
        file: span.file as usize,
        path: resolved.display.clone(),
        start: Position {
            line: span.start.line,
            col: span.start.col,
        },
        end: Position {
            line: span.end.line,
            col: span.end.col,
        },
    });
    Diagnostic {
        code: error.code().to_string(),
        stage,
        severity: crate::diag::Severity::Error,
        message: error.message(),
        span,
        source: Some(resolved.origin.clone()),
    }
}

/// Map an IR error to a driver diagnostic.
fn ir_diag(
    code: &'static str,
    stage: Stage,
    error: wright_ir::error::IrError,
    resolved: &ResolvedInput,
) -> Diagnostic {
    Diagnostic {
        code: code.to_string(),
        stage,
        severity: crate::diag::Severity::Error,
        message: error.to_string(),
        span: None,
        source: Some(resolved.origin.clone()),
    }
}
