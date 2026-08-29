//! The reusable compiler/session driver (issue #37).
//!
//! [`CompilerSession`] is the single orchestration path shared by the CLI,
//! library consumers, and (in later milestones) tool APIs and LSP: input
//! resolution → frontend selection → validation → lowering → analysis →
//! emission. Frontends are selected by [`SourceKind`] behind one contract, so
//! the native `.opy` frontend can replace the temporary adapter bridge
//! without changing callers. Every workflow returns a typed [`Envelope`]
//! whose JSON serialization is the machine-readable CLI contract.

use std::path::Path;
use std::sync::Arc;

use workshop_rs::wir;
use wright_analyzer::registry::LintConfig;
use wright_analyzer::service::{Origin as ServiceOrigin, Request, SemanticService};

use crate::WorkshopProvider;
use crate::config::{SessionConfig, SourceKind};
use crate::diag::{Diagnostic, Origin, Position, Severity, SourceSpan, Stage};
use crate::input::{self, ResolvedInput};
use crate::progress::{ProgressEvent, ProgressObserver, ProgressPhase, ProgressUnit};
use crate::result::{
    AnalyzeResult, CheckResult, CompileResult, CompiledOutput, ConvertResult, ConvertTarget,
    Envelope, InspectResult, LintResult, OstwFileSummary, OstwProjectSummary, exit_code_from,
    version_info,
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
    catalog: workshop_rs::catalog::Catalog,
    loaded: Option<Loaded>,
    diagnostics: Vec<Diagnostic>,
    progress_observer: Option<Arc<dyn ProgressObserver>>,
}

impl CompilerSession {
    /// Build a session from a configuration.
    pub fn new(config: SessionConfig) -> Result<CompilerSession, Diagnostic> {
        let catalog = workshop_rs::catalog::Catalog::builtin().map_err(|error| {
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
            progress_observer: None,
        })
    }

    /// Attach a transport-neutral observer for real workflow phase events.
    pub fn set_progress_observer(&mut self, observer: Arc<dyn ProgressObserver>) {
        self.progress_observer = Some(observer);
    }

    /// Detach the current progress observer before a caller renders a result.
    pub fn clear_progress_observer(&mut self) {
        self.progress_observer = None;
    }

    fn progress(&self, event: ProgressEvent) {
        if let Some(observer) = &self.progress_observer {
            observer.on_progress(event);
        }
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
        self.progress(ProgressEvent::new(ProgressPhase::InputResolution));
        let mut resolved = input::resolve(&self.config)?;
        if resolved.kind == SourceKind::Ostw {
            self.progress(ProgressEvent::new(ProgressPhase::ProjectLoading));
            return self.load_ostw(&mut resolved);
        }
        let mut program = match resolved.kind {
            SourceKind::Workshop => {
                self.progress(ProgressEvent::new(ProgressPhase::Parsing));
                let (program, locale) = self.load_workshop(&resolved)?;
                resolved.origin.locale = Some(locale);
                program
            }
            SourceKind::Protocol => {
                self.progress(ProgressEvent::new(ProgressPhase::Parsing));
                let json = resolved.text.clone();
                self.load_protocol(&json, &resolved)?
            }
            SourceKind::Opy => {
                self.progress(ProgressEvent::new(ProgressPhase::Parsing));
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
                    let program = match outcome.program {
                        Some(program) => program,
                        None => {
                            let error = outcome
                                .error
                                .expect("a failed compile outcome always carries an error");
                            return Err(opy_diag(error, &outcome.files, &resolved));
                        }
                    };
                    self.progress(ProgressEvent::new(ProgressPhase::Lowering));
                    program.validate().map_err(|error| {
                        ir_diag("validation-error", Stage::Validation, error, &resolved)
                    })?;
                    program
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
        self.progress(ProgressEvent::new(ProgressPhase::Parsing));
        self.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
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
        let program = semantic.wir.clone().unwrap_or_default();
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

    /// The diagnostics accumulated by the last workflow run.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Spawn the LPP provider client configured for `language_id` (#142).
    ///
    /// Providers are discovered by opaque language id through the session's
    /// provider registry; no source-language branch lives here. When no
    /// provider is configured for the id, or when a required capability was
    /// not negotiated, the failure is an explicit structured
    /// `wright_lpp::ProviderError` — there is no silent fallback to
    /// in-process compiler semantics.
    pub fn language_provider(
        &self,
        language_id: &str,
    ) -> Result<Box<dyn wright_lpp::LanguageProvider>, wright_lpp::ProviderError> {
        self.config
            .providers
            .spawn(language_id)
            .map(|provider| Box::new(provider) as Box<dyn wright_lpp::LanguageProvider>)
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
            .map(workshop_rs::catalog::Locale::new);
        let locale = workshop_rs::detect::resolve_locale(
            &resolved.text,
            &self.catalog,
            override_locale.as_ref(),
        )
        .map_err(|error| workshop_diag(error, resolved))?;
        let program = workshop_rs::parser::parse_with_context(
            &resolved.text,
            &self.catalog,
            &locale,
            &self.catalog,
        )
        .map_err(|error| workshop_diag(error, resolved))?;
        self.progress(ProgressEvent::new(ProgressPhase::Validation));
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
        self.progress(ProgressEvent::new(ProgressPhase::Validation));
        protocol
            .validate()
            .map_err(|error| hir_diag(error, resolved))?;
        self.progress(ProgressEvent::new(ProgressPhase::Lowering));
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
        if let Some(outcome) = &loaded.ostw {
            // OSTW (#119): the load lowered the semantic HIR into the
            // session program. Project-boundary diagnostics (missing
            // imports) and semantic-boundary diagnostics are errors — an
            // unsupported or unresolved reachable form fails compilation
            // with a deterministic, structured, source-located diagnostic
            // instead of being deferred to emission.
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
            .map(|locale| workshop_rs::catalog::Locale::new(&locale))
            .unwrap_or_else(|| workshop_rs::catalog::Locale::new("en-US"));
        self.progress(ProgressEvent::new(ProgressPhase::Emission));
        let text = workshop_rs::emitter::emit(&loaded.program, &self.catalog, &locale)
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

    /// `check`: load, validate, and surface frontend/project/semantic
    /// validation diagnostics. Configurable lint findings are not part of the
    /// correctness gate.
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
        self.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
        self.attach_workshop_completeness(&loaded);
        self.finish(command, CheckResult { ostw: None })
    }

    /// `analyze`: load and produce the semantic summary and structural facts.
    /// This report deliberately does not execute or expose the lint registry.
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
        self.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
        let mut program = service_response(&service, &Request::Program);
        if let serde_json::Value::Object(object) = &mut program {
            // The service also supports the legacy findings query for agents,
            // but an analyze report must not be a view of that lint registry.
            object.remove("findings");
        }
        let facts = semantic_facts(&service);
        self.finish(command, AnalyzeResult { program, facts })
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
        self.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
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
    /// metadata, effective configuration, and findings (#98).
    ///
    /// Lint rule findings are reported in `result.findings`; frontend and
    /// Workshop semantic-completeness diagnostics remain in the envelope.
    /// Rule enable/disable and severity come from `self.config.lint`, the same
    /// configuration the CLI flags and programmatic consumers set.
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
        self.attach_workshop_completeness(&loaded);
        let service = match self.service_with(&loaded, self.config.lint.clone()) {
            Ok(service) => service,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, LintResult::default());
            }
        };
        self.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
        let program = service_response(&service, &Request::Program);
        let lint_rules = service_response(&service, &Request::LintRules);
        let lint_rule_count = lint_rules
            .pointer("/rules")
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len);
        self.progress(ProgressEvent::with_count(
            ProgressPhase::Linting,
            lint_rule_count,
            ProgressUnit::Rules,
        ));
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

    /// `convert`: load validated Workshop input and reconstruct canonical
    /// source for an explicit target (`opy` | `ostw`) through the
    /// language-owned reconstructors (#126).
    ///
    /// The operation is the shared driver/session conversion contract: it
    /// reuses the [`CompilerSession::load`] path to obtain the validated
    /// Workshop WIR program and delegates per target to
    /// `wright_opy::reconstruct::reconstruct` /
    /// `wright_ostw::reconstruct::reconstruct` — no reconstruction logic
    /// lives in the driver, and there is no generic transpiler matrix or
    /// direct OPY ↔ OSTW path. Non-representable constructs fail
    /// deterministically with the reconstructor's stable structured
    /// diagnostics (stage `reconstruction`, unsupported exit code 3) and
    /// never produce partial source. Non-Workshop inputs are rejected
    /// explicitly: the operation is declared over Workshop input only.
    pub fn convert(&mut self, target: ConvertTarget) -> Envelope<ConvertResult> {
        let command = "convert";
        let loaded = match self.load() {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, ConvertResult::default());
            }
        };
        if loaded.input.kind != SourceKind::Workshop {
            self.diagnostics.push(Diagnostic::error(
                "convert-input-kind",
                Stage::Discovery,
                format!(
                    "convert reconstructs Workshop input; got '{}' input (the declared \
                     conversion surface has no direct OPY ↔ OSTW path)",
                    loaded.input.kind.as_str()
                ),
            ));
            return self.finish(command, ConvertResult::default());
        }
        self.progress(ProgressEvent::new(ProgressPhase::Conversion));
        let text = match target {
            ConvertTarget::Opy => self.convert_opy(&loaded),
            ConvertTarget::Ostw => self.convert_ostw(&loaded),
        };
        match text {
            Ok(text) => {
                let sha256 = input_identity(&text);
                self.finish(
                    command,
                    ConvertResult {
                        target,
                        text,
                        sha256,
                    },
                )
            }
            Err(()) => self.finish(command, ConvertResult::default()),
        }
    }

    /// Reconstruct canonical OPY source for a loaded Workshop program.
    fn convert_opy(&mut self, loaded: &Loaded) -> Result<String, ()> {
        match wright_opy::reconstruct::reconstruct(&loaded.program) {
            Ok(text) => Ok(text),
            Err(error) => {
                for issue in &error.issues {
                    self.diagnostics.push(reconstruct_diag(
                        issue.code,
                        &issue.message,
                        issue.span,
                        loaded,
                    ));
                }
                Err(())
            }
        }
    }

    /// Reconstruct canonical OSTW source for a loaded Workshop program.
    fn convert_ostw(&mut self, loaded: &Loaded) -> Result<String, ()> {
        match wright_ostw::reconstruct::reconstruct(&loaded.program, &self.catalog) {
            Ok(text) => Ok(text),
            Err(errors) => {
                for error in &errors {
                    self.diagnostics.push(reconstruct_diag(
                        error.code,
                        &error.message,
                        error.span,
                        loaded,
                    ));
                }
                Err(())
            }
        }
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

    /// Structural validation permits source-preserving Workshop fallbacks.
    /// Surface those nodes as blocking semantic diagnostics before presenting
    /// check/lint output as definitive. The catalog remains owned by
    /// workshop-rs; this is only the consumer-side diagnostic projection.
    fn attach_workshop_completeness(&mut self, loaded: &Loaded) {
        if loaded.input.kind != SourceKind::Workshop {
            return;
        }
        let provider = match WorkshopProvider::new() {
            Ok(provider) => provider,
            Err(error) => {
                self.diagnostics.push(Diagnostic::error(
                    "workshop-provider-init",
                    Stage::Internal,
                    error.to_string(),
                ));
                return;
            }
        };
        let path = loaded
            .input
            .path
            .as_deref()
            .unwrap_or_else(|| Path::new("<stdin>"));
        match wright_core::provider::LanguageProvider::check(&provider, &loaded.input.text, path) {
            Ok(diagnostics) => {
                self.diagnostics
                    .extend(diagnostics.into_iter().map(|diagnostic| Diagnostic {
                        code: diagnostic.code,
                        stage: Stage::Analysis,
                        severity: match diagnostic.severity {
                            wright_core::provider::Severity::Error => Severity::Error,
                            wright_core::provider::Severity::Warning => Severity::Warning,
                            wright_core::provider::Severity::Info => Severity::Info,
                        },
                        message: diagnostic.message,
                        span: Some(SourceSpan {
                            file: 0,
                            path: diagnostic.span.file.display().to_string(),
                            start: Position {
                                line: diagnostic.span.start_line,
                                col: diagnostic.span.start_col,
                            },
                            end: Position {
                                line: diagnostic.span.end_line,
                                col: diagnostic.span.end_col,
                            },
                        }),
                        status: Some(diagnostic.status),
                        source: Some(loaded.origin.clone()),
                    }));
            }
            Err(error) => self.diagnostics.push(Diagnostic::error(
                "workshop-provider-check",
                Stage::Internal,
                error.to_string(),
            )),
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

/// Build the initial `analyze` report from existing semantic query surfaces.
///
/// Keeping this composition here makes the product boundary explicit: the
/// report contains symbol usage and CFG measurements, while lint rules remain
/// owned by `LintRegistry` and are only exposed by `lint`/`findings` queries.
fn semantic_facts(service: &SemanticService<'_>) -> serde_json::Value {
    let symbols = service_response(service, &Request::ListSymbols { kind: None });
    let symbols = symbols
        .as_array()
        .map(|symbols| {
            symbols
                .iter()
                .map(|symbol| {
                    let id = symbol
                        .get("id")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or_default() as u32;
                    let usage = service_response(service, &Request::GetUsage { symbol: id });
                    serde_json::json!({
                        "id": symbol.get("id").cloned().unwrap_or_default(),
                        "kind": symbol.get("kind").cloned().unwrap_or_default(),
                        "name": symbol.get("name").cloned().unwrap_or_default(),
                        "span": symbol.get("span").cloned().unwrap_or(serde_json::Value::Null),
                        "usage": usage,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let rules = service_response(service, &Request::ListRules);
    let rules = rules
        .as_array()
        .map(|rules| {
            rules
                .iter()
                .map(|rule| {
                    let id = rule
                        .get("id")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or_default() as u32;
                    let cfg = service_response(service, &Request::GetCfg { rule: id });
                    let blocks = cfg
                        .get("blocks")
                        .and_then(serde_json::Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let edge_count = blocks
                        .iter()
                        .map(|block| {
                            block
                                .get("successors")
                                .and_then(serde_json::Value::as_array)
                                .map_or(0, Vec::len)
                        })
                        .sum::<usize>();
                    let wait_blocks = blocks
                        .iter()
                        .filter(|block| {
                            block
                                .get("waits")
                                .and_then(serde_json::Value::as_bool)
                                .unwrap_or(false)
                        })
                        .count();
                    let loop_blocks = blocks
                        .iter()
                        .filter(|block| {
                            matches!(
                                block.get("kind").and_then(serde_json::Value::as_str),
                                Some("while" | "for")
                            )
                        })
                        .count();
                    serde_json::json!({
                        "id": rule.get("id").cloned().unwrap_or_default(),
                        "name": rule.get("name").cloned().unwrap_or_default(),
                        "span": rule.get("span").cloned().unwrap_or(serde_json::Value::Null),
                        "controlFlow": {
                            "blocks": blocks.len(),
                            "edges": edge_count,
                            "loopBlocks": loop_blocks,
                            "waitBlocks": wait_blocks,
                        },
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    serde_json::json!({
        "symbols": symbols,
        "rules": rules,
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
                        .get(workshop_rs::source::FileId::from_index(file as usize))
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
fn workshop_diag(error: workshop_rs::WorkshopError, resolved: &ResolvedInput) -> Diagnostic {
    let (code, stage, span) = match &error {
        workshop_rs::WorkshopError::Catalog(catalog) => {
            return Diagnostic::error(
                "catalog-error",
                Stage::Internal,
                format!("{}: {}", catalog.code, catalog.message),
            );
        }
        workshop_rs::WorkshopError::Unknown { kind, span, .. } => (
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
        workshop_rs::WorkshopError::Malformed { span, .. } => (
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
        workshop_rs::WorkshopError::Unsupported { span, .. } => (
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
        // The workshop-rs emitter reports missing target-locale spellings as
        // a first-class error (ADR-0001 Decision 7; wright#143): conversion
        // or emission into a locale without a mapping is a diagnostic, never
        // a guess or a silent passthrough.
        workshop_rs::WorkshopError::MissingMapping { kind, id, locale } => (
            "missing-mapping".to_string(),
            Stage::Frontend,
            Diagnostic::error(
                "missing-mapping",
                Stage::Frontend,
                format!(
                    "missing {kind} mapping for locale '{locale}': '{id}' \
                     (fallback emission is opt-in; see workshop-rs EmitOptions)"
                ),
            )
            .span
            .map(|_| unreachable!("constructed above without a span")),
        ),
    };
    Diagnostic {
        code,
        stage,
        severity: crate::diag::Severity::Error,
        message: error.to_string(),
        status: None,
        span,
        source: Some(resolved.origin.clone()),
    }
}

/// Map a native frontend error to a driver diagnostic.
///
/// Span paths resolve through the frontend file registry so a failure inside
/// an included file names that file; file 0 (the main file) carries the
/// resolved display path by construction (#83).
pub(crate) fn opy_diag(
    error: wright_opy::OpyError,
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
        status: None,
        span,
        source: Some(resolved.origin.clone()),
    }
}

/// Map a native OSTW frontend error to a driver diagnostic.
///
/// Span paths resolve through the OSTW project registry, so a failure inside
/// an imported file names that file with its project-relative path.
pub(crate) fn ostw_diag(
    error: wright_ostw::SourceError,
    outcome: &wright_ostw::OstwOutcome,
    resolved: &ResolvedInput,
) -> Diagnostic {
    let span = error.span.map(|span| SourceSpan {
        file: span.file.index(),
        path: outcome
            .project
            .as_ref()
            .and_then(|project| {
                project
                    .files
                    .iter()
                    .find(|file| file.id == span.file.index() as u32)
            })
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
        status: None,
        span,
        source: Some(resolved.origin.clone()),
    }
}

/// Map a reconstructor rejection (shared shape: stable code, message,
/// optional WIR span) into the driver diagnostic contract (#126).
///
/// The reconstructor's stable code is preserved verbatim; the stage is
/// `reconstruction` (the "recognized but unsupported" class, exit code 3).
/// A manifest/catalog load failure is an environment failure, not a
/// reconstruction rejection, so it maps to the internal stage. Span paths
/// resolve through the program file registry, falling back to the input
/// display path.
fn reconstruct_diag(
    code: &str,
    message: &str,
    span: Option<workshop_rs::source::Span>,
    loaded: &Loaded,
) -> Diagnostic {
    let stage = match code {
        "manifest-error" | "catalog-error" => Stage::Internal,
        _ => Stage::Reconstruction,
    };
    let span = span.map(|span| SourceSpan {
        file: span.file.index(),
        path: loaded
            .program
            .files
            .get(span.file)
            .map(|file| file.path.clone())
            .unwrap_or_else(|| loaded.input.display.clone()),
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
        code: code.to_string(),
        stage,
        severity: crate::diag::Severity::Error,
        message: message.to_string(),
        status: None,
        span,
        source: Some(loaded.origin.clone()),
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
pub(crate) fn hir_diag(error: wright_core::hir::HirError, resolved: &ResolvedInput) -> Diagnostic {
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
        status: None,
        span,
        source: Some(resolved.origin.clone()),
    }
}

/// Map an IR error to a driver diagnostic.
pub(crate) fn ir_diag(
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
        status: None,
        span: None,
        source: Some(resolved.origin.clone()),
    }
}
