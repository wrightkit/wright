//! Frontends are selected by [`SourceKind`] behind one contract, so
//! the provider-backed `.opy` workflow can replace the retired adapter bridge
//! without changing callers. Every workflow returns a typed [`Envelope`]
//! whose JSON serialization is the machine-readable CLI contract.

use std::path::Path;
use std::sync::Arc;

use workshop_rs::Program;
use wright_analyzer::canonical::SemanticService;
use wright_analyzer::registry::{LintConfig, LintRegistry};
use wright_analyzer::service::{Origin as ServiceOrigin, Request};

use crate::WorkshopProvider;
use crate::config::{InputSpec, SessionConfig, SourceKind};
use crate::diag::{Diagnostic, Origin, Position, Severity, SourceSpan, Stage};
use crate::input::{self, InputTarget, ResolvedInput};
use crate::input_identity;
use crate::opy_provider;
use crate::progress::{ProgressEvent, ProgressObserver, ProgressPhase, ProgressUnit};
use crate::result::{
    AnalyzeResult, CheckResult, CompileResult, CompiledOutput, ConvertResult, ConvertTarget,
    Envelope, InspectResult, LintResult, exit_code_from, version_info,
};
use crate::source_provider::{
    SourceBackend, SourceLanguage, SourceProvenance, SourceProvider, SourceProviderError,
    SourceTarget,
};

fn error_kind(error: &opy_provider::OpyProviderError) -> wright_lpp::LocalProviderErrorKind {
    use opy_provider::OpyProviderError as O;
    use wright_lpp::LocalProviderErrorKind as K;
    match error {
        O::Missing(_) => K::Missing,
        O::UnsupportedPlatform(_) => K::UnsupportedPlatform,
        O::Offline(_) => K::Offline,
        O::Download(_) => K::Download,
        O::Integrity(_) => K::Integrity,
        O::Install(_) => K::Install,
    }
}

/// A successfully loaded program with its input and origin metadata.
#[derive(Clone)]
pub struct Loaded {
    /// The validated canonical Workshop program.
    pub program: Arc<Program>,
    /// Origin metadata carried into diagnostics and results.
    pub origin: Origin,
    /// The resolved input.
    pub input: ResolvedInput,
    /// Whether semantic spans can be mapped to authored source files.
    pub provenance: Provenance,
    /// Source identities retained by the frontend in canonical file-id order.
    pub(crate) source_files: Arc<Vec<String>>,
}

/// Provenance of the semantic program handed to Wright's analyzer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    /// The program was parsed from the source files represented by `input`.
    Source,
    /// The program came from an unmapped provider-returned canonical artifact.
    Unmapped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderOperation {
    Check,
    Compile,
}

/// One reusable compiler session.
pub struct CompilerSession {
    /// The session configuration (input, frontend, overrides, format).
    pub config: SessionConfig,
    catalog: workshop_rs::catalog::Catalog,
    lint_registry: Arc<LintRegistry>,
    loaded: Option<Loaded>,
    loaded_operation: Option<ProviderOperation>,
    diagnostics: Vec<Diagnostic>,
    progress_observer: Option<Arc<dyn ProgressObserver>>,
    source_provider: Option<Box<dyn SourceProvider>>,
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
        let mut lint_registry = LintRegistry::default();
        for path in &config.lint_rule_paths {
            lint_registry.load_path(path).map_err(|error| {
                Diagnostic::error("lint-rule-error", Stage::Analysis, error.to_string())
            })?;
        }
        Ok(CompilerSession {
            config,
            catalog,
            lint_registry: Arc::new(lint_registry),
            loaded: None,
            loaded_operation: None,
            diagnostics: Vec::new(),
            progress_observer: None,
            source_provider: None,
        })
    }

    /// Build a session whose source-language workflow must use `provider`.
    ///
    /// The provider is injected at the product boundary; its transport and
    /// source project model are not visible to the session. A provider failure
    /// is surfaced as-is and never falls back to an in-process OPY path.
    pub fn with_source_provider(
        mut config: SessionConfig,
        provider: Box<dyn SourceProvider>,
    ) -> Result<CompilerSession, Diagnostic> {
        config.source_backend = SourceBackend::Provider;
        let mut session = Self::new(config)?;
        session.source_provider = Some(provider);
        Ok(session)
    }

    /// Attach a transport-neutral observer for real workflow phase events.
    pub fn set_progress_observer(&mut self, observer: Arc<dyn ProgressObserver>) {
        self.progress_observer = Some(observer);
    }

    /// Detach the current progress observer before a caller renders a result.
    pub fn clear_progress_observer(&mut self) {
        self.progress_observer = None;
    }

    pub(crate) fn lint_registry(&self) -> &Arc<LintRegistry> {
        &self.lint_registry
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
        if self.config.source_backend == SourceBackend::Provider
            && self.loaded_operation != Some(ProviderOperation::Compile)
        {
            return Err(SourceProviderError::Unsupported {
                message: "provider-backed load requires a canonical compile result; lint, analyze, inspect, and convert are not available on the check-only provider path".to_string(),
            }
            .diagnostic());
        }
        self.load_with_operation(if self.config.source_backend == SourceBackend::Native {
            ProviderOperation::Check
        } else {
            ProviderOperation::Compile
        })
    }

    fn load_with_operation(
        &mut self,
        provider_operation: ProviderOperation,
    ) -> Result<Loaded, Diagnostic> {
        if let Some(loaded) = &self.loaded {
            let provider_backend = self.config.source_backend != SourceBackend::Native
                && loaded.input.kind == SourceKind::Opy;
            if !provider_backend
                || self.loaded_operation == Some(provider_operation)
                || (self.loaded_operation == Some(ProviderOperation::Compile)
                    && provider_operation == ProviderOperation::Check)
            {
                return Ok(loaded.clone());
            }
        }
        if self.config.source_backend == SourceBackend::Provider
            && matches!(self.config.input, InputSpec::Stdin)
        {
            return Err(SourceProviderError::Unsupported {
                message: "provider-backed source workflows require an entry path; stdin has no entry identity".to_string(),
            }
            .diagnostic());
        }
        self.progress(ProgressEvent::new(ProgressPhase::InputResolution));
        let mut resolved = input::resolve(&self.config)?;
        if resolved.kind == SourceKind::Ostw {
            return Err(source_provider_unavailable());
        }
        if self.config.source_backend == SourceBackend::Provider && resolved.kind != SourceKind::Opy
        {
            return Err(Diagnostic::error(
                "source-provider-kind",
                Stage::Discovery,
                format!(
                    "the provider backend currently supports only OPY input; got '{}'",
                    resolved.kind.as_str()
                ),
            ));
        }
        let provider_backend = match self.config.source_backend {
            SourceBackend::Native => false,
            SourceBackend::Provider => true,
            SourceBackend::Auto => resolved.kind == SourceKind::Opy,
        };
        if provider_backend {
            return self.load_from_source_provider(&mut resolved, provider_operation);
        }
        let (mut program, source_files) = match resolved.kind {
            SourceKind::Workshop => {
                self.progress(ProgressEvent::new(ProgressPhase::Parsing));
                let (program, locale) = self.load_workshop(&resolved)?;
                resolved.origin.locale = Some(locale);
                (program, vec![resolved.display.clone()])
            }
            SourceKind::Protocol => {
                return Err(Diagnostic::error(
                    "input-kind-unsupported",
                    Stage::Discovery,
                    "the retired protocol Workshop representation is not a canonical Program input",
                ));
            }
            SourceKind::Opy => {
                return Err(source_provider_unavailable());
            }
            SourceKind::Auto => {
                return Err(Diagnostic::error(
                    "input-kind-unknown",
                    Stage::Discovery,
                    "input kind could not be detected; pass `--kind opy|ostw|workshop|protocol`",
                ));
            }
            SourceKind::Ostw => unreachable!("OSTW is rejected before native dispatch"),
        };
        // Apply the selected transformation profile (validated before/after).
        if self.config.profile != wright_transform::Profile::Off {
            wright_transform::run_canonical(&mut program, self.config.profile).map_err(
                |error| {
                    Diagnostic::error(
                        "transform-error",
                        Stage::Internal,
                        format!("Workshop transformation failed: {error}"),
                    )
                },
            )?;
        }
        let loaded = Loaded {
            program: Arc::new(program),
            origin: resolved.origin.clone(),
            input: resolved,
            provenance: Provenance::Source,
            source_files: Arc::new(source_files),
        };
        self.loaded = Some(loaded.clone());
        Ok(loaded)
    }

    /// Load a source-language result through the explicit product provider
    /// seam, then hand its canonical Workshop text to `workshop-rs`.
    fn load_from_source_provider(
        &mut self,
        resolved: &mut ResolvedInput,
        operation: ProviderOperation,
    ) -> Result<Loaded, Diagnostic> {
        let language = match resolved.kind {
            SourceKind::Opy => SourceLanguage::Opy,
            other => {
                return Err(Diagnostic::error(
                    "source-provider-kind",
                    Stage::Discovery,
                    format!(
                        "the provider backend currently supports only OPY input; got '{}'",
                        other.as_str()
                    ),
                ));
            }
        };
        let entry = resolved
            .path
            .clone()
            .unwrap_or_else(|| Path::new("<stdin>").to_path_buf());
        let target = match resolved.target {
            InputTarget::File => SourceTarget::new(language, entry, resolved.cwd.clone()),
            InputTarget::Directory => {
                SourceTarget::directory(language, entry, resolved.cwd.clone())
            }
        }
        .with_project_root(resolved.root.clone());
        if self.source_provider.is_none() {
            let mut provider = self
                .language_provider(opy_provider::OPY_LANGUAGE_ID)
                .map_err(|error| {
                    SourceProviderError::Failed {
                        code: error.code().to_string(),
                        message: error.to_string(),
                    }
                    .diagnostic()
                })?;
            let info = wright_lpp::ClientInfo {
                name: wright_lpp::LPP_CLIENT_NAME.to_string(),
                version: crate::result::DRIVER_VERSION.to_string(),
            };
            let initialize = match resolved.target {
                InputTarget::File => provider.initialize_project_loading(Some(&info)),
                InputTarget::Directory => provider.initialize_project_target(Some(&info)),
            };
            initialize.map_err(|error| {
                SourceProviderError::Failed {
                    code: error.code().to_string(),
                    message: error.to_string(),
                }
                .diagnostic()
            })?;
            self.source_provider = Some(Box::new(crate::source_provider::LppSourceProvider::new(
                provider,
                self.config.locale.clone(),
            )));
        }
        let Some(provider) = self.source_provider.as_mut() else {
            return Err(SourceProviderError::NotConfigured { language }.diagnostic());
        };
        if provider.language() != language {
            return Err(Diagnostic::error(
                "source-provider-language",
                Stage::Discovery,
                format!(
                    "the injected provider serves '{}' but the selected source is '{}'",
                    provider.language().as_str(),
                    language.as_str()
                ),
            ));
        }
        let compilation = match operation {
            ProviderOperation::Check => provider.check(&target),
            ProviderOperation::Compile => provider.compile(&target),
        }
        .map_err(|error| error.diagnostic())?;
        if operation == ProviderOperation::Compile && resolved.target == InputTarget::Directory {
            let Some(source_identity) = compilation.source_identity.as_ref() else {
                return Err(Diagnostic::error(
                    "source-provider-identity",
                    Stage::Frontend,
                    "the source provider returned no source identity for the directory target",
                ));
            };
            if source_identity.len() != 64
                || !source_identity.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(Diagnostic::error(
                    "source-provider-identity",
                    Stage::Frontend,
                    "the source provider returned an invalid SHA-256 source identity",
                ));
            }
            resolved.identity = source_identity.clone();
        }
        let mut provider_diagnostics = compilation.diagnostics;
        if let Some(index) = provider_diagnostics
            .iter()
            .position(|diagnostic| diagnostic.severity == Severity::Error)
        {
            let first = provider_diagnostics.remove(index);
            self.diagnostics.extend(provider_diagnostics);
            return Err(first);
        }
        self.diagnostics.extend(provider_diagnostics);
        let provenance = match compilation.provenance {
            SourceProvenance::Unmapped => Provenance::Unmapped,
        };
        let locale_name = compilation
            .locale
            .or_else(|| resolved.origin.locale.clone())
            .unwrap_or_else(|| "en-US".to_string());
        let locale = workshop_rs::catalog::Locale::new(&locale_name);
        if operation == ProviderOperation::Check {
            return Ok(Loaded {
                program: Arc::new(Program::default()),
                origin: resolved.origin.clone(),
                input: resolved.clone(),
                provenance,
                source_files: Arc::new(vec![resolved.display.clone()]),
            });
        }
        let Some(workshop_text) = compilation.workshop_text else {
            return Err(Diagnostic::error(
                "source-provider-no-artifact",
                Stage::Frontend,
                "the source provider returned no canonical Workshop output",
            ));
        };
        self.progress(ProgressEvent::new(ProgressPhase::Parsing));
        let program = workshop_rs::parser::parse_with_context(
            &workshop_text,
            &self.catalog,
            &locale,
            &self.catalog,
        )
        .map_err(|error| workshop_diag_for_unmapped_provider_artifact(error, resolved))?;
        self.progress(ProgressEvent::new(ProgressPhase::Validation));
        let mut program = program;
        program
            .validate()
            .map_err(|error| workshop_diag_for_unmapped_provider_artifact(error, resolved))?;
        if self.config.profile != wright_transform::Profile::Off {
            self.progress(ProgressEvent::new(ProgressPhase::Lowering));
            wright_transform::run_canonical(&mut program, self.config.profile).map_err(
                |error| {
                    Diagnostic::error(
                        "transform-error",
                        Stage::Internal,
                        format!("Workshop transformation failed: {error}"),
                    )
                },
            )?;
        }
        resolved.origin.locale = Some(locale.to_string());
        let loaded = Loaded {
            program: Arc::new(program),
            origin: resolved.origin.clone(),
            input: resolved.clone(),
            provenance,
            source_files: Arc::new(vec![resolved.display.clone()]),
        };
        self.loaded = Some(loaded.clone());
        self.loaded_operation = Some(operation);
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
        if language_id == opy_provider::OPY_LANGUAGE_ID
            && !self.config.providers.contains(language_id)
        {
            let resolved = self.config.opy_provider.resolve().map_err(|error| {
                wright_lpp::ProviderError::Local {
                    kind: error_kind(&error),
                    message: error.to_string(),
                }
            })?;
            let mut providers = self.config.providers.clone();
            providers
                .register(wright_lpp::ProviderConfig::new(
                    opy_provider::OPY_LANGUAGE_ID,
                    resolved.executable,
                    Vec::new(),
                ))
                .expect("the first-party OPY provider language id is not registered");
            return providers
                .spawn(language_id)
                .map(|provider| Box::new(provider) as Box<dyn wright_lpp::LanguageProvider>);
        }
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

    fn load_workshop(&mut self, resolved: &ResolvedInput) -> Result<(Program, String), Diagnostic> {
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
            .map_err(|error| workshop_diag(error, resolved))?;
        Ok((program, locale.to_string()))
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
        let written_to = match &self.config.output {
            Some(path) => {
                if let Err(error) = std::fs::write(path, &output.text) {
                    self.diagnostics.push(Diagnostic::error(
                        "output-io",
                        Stage::Emission,
                        format!("cannot write output '{}': {error}", path.display()),
                    ));
                    return self.finish(command, result);
                }
                crate::input::display_path(path)
            }
            None => "stdout".to_string(),
        };
        result.output = Some(CompiledOutput {
            written_to,
            ..output
        });
        self.finish(command, result)
    }

    fn compile_output(&mut self) -> Result<CompiledOutput, Diagnostic> {
        let loaded = self.load_with_operation(ProviderOperation::Compile)?;
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
        let loaded = match self.load_with_operation(ProviderOperation::Check) {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, CheckResult {});
            }
        };
        self.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
        self.attach_workshop_completeness(&loaded);
        self.finish(command, CheckResult {})
    }

    fn with_service<T: Default + serde::Serialize>(
        &mut self,
        command: &str,
        load_compile: bool,
        lint_config: LintConfig,
        f: impl FnOnce(&mut Self, &Loaded, &SemanticService<'_>) -> T,
    ) -> Envelope<T> {
        let loaded = match if load_compile {
            self.load_with_operation(ProviderOperation::Compile)
        } else {
            self.load()
        } {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, T::default());
            }
        };
        let service = match self.service_with(&loaded, lint_config) {
            Ok(service) => service,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, T::default());
            }
        };
        self.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
        let result = f(self, &loaded, &service);
        self.finish(command, result)
    }

    /// `analyze`: load and produce the semantic summary and structural facts.
    pub fn analyze(&mut self) -> Envelope<AnalyzeResult> {
        self.with_service(
            "analyze",
            true,
            LintConfig::default(),
            |_sess, _loaded, service| {
                let mut program = service_response(service, &Request::Program);
                if let serde_json::Value::Object(object) = &mut program {
                    object.remove("findings");
                }
                AnalyzeResult {
                    program,
                    facts: semantic_facts(service),
                }
            },
        )
    }

    /// `inspect`: load and produce the structural/semantic program model.
    pub fn inspect(&mut self) -> Envelope<InspectResult> {
        self.with_service(
            "inspect",
            false,
            LintConfig::default(),
            |_sess, _loaded, service| {
                let program = service_response(service, &Request::Program);
                let rules = service_response(service, &Request::ListRules);
                let symbols = service_response(service, &Request::ListSymbols { kind: None });
                let references = serde_json::Value::Array(
                    symbols
                        .as_array()
                        .map_or(&[][..], Vec::as_slice)
                        .iter()
                        .filter_map(|s| s.get("id").and_then(serde_json::Value::as_u64))
                        .map(|id| {
                            service_response(
                                service,
                                &Request::FindReferences { symbol: id as u32 },
                            )
                        })
                        .collect(),
                );
                InspectResult {
                    program,
                    rules,
                    symbols,
                    references,
                }
            },
        )
    }

    /// `lint`: load and produce the source identity, program summary, rule
    /// metadata, effective configuration, and findings (#98).
    pub fn lint(&mut self) -> Envelope<LintResult> {
        let config = self.config.lint.clone();
        self.with_service("lint", true, config, |sess, loaded, service| {
            sess.attach_workshop_completeness(loaded);
            let program = service_response(service, &Request::Program);
            let lint_rules = service_response(service, &Request::LintRules);
            let count = lint_rules
                .pointer("/rules")
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len);
            sess.progress(ProgressEvent::with_count(
                ProgressPhase::Linting,
                count,
                ProgressUnit::Rules,
            ));
            let mut findings = service_response(service, &Request::GetFindings);
            resolve_finding_span_paths(&mut findings, loaded);
            let (rules, config, skipped) = match lint_rules {
                serde_json::Value::Object(mut o) => (
                    o.remove("rules").unwrap_or_else(|| serde_json::json!([])),
                    o.remove("config").unwrap_or_else(|| serde_json::json!({})),
                    o.remove("skipped").unwrap_or_else(|| serde_json::json!([])),
                ),
                _ => (
                    serde_json::json!([]),
                    serde_json::json!({}),
                    serde_json::json!([]),
                ),
            };
            LintResult {
                input_identity: loaded.input.identity.clone(),
                program,
                rules,
                config,
                findings,
                skipped,
            }
        })
    }

    /// `convert`: load validated Workshop input and reconstruct canonical OPY
    /// source through the negotiated provider, or refuse the unavailable OSTW
    /// provider target (#126).
    ///
    /// The operation is the shared driver/session conversion contract: it
    /// reuses the [`CompilerSession::load`] path to obtain the validated
    /// canonical Workshop program and delegates per target to
    /// the owner reconstructor — no reconstruction logic lives in the driver,
    /// and there is no generic transpiler matrix or direct OPY ↔ OSTW path.
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
            ConvertTarget::Ostw => {
                self.diagnostics.push(source_provider_unavailable());
                Err(())
            }
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

    /// Reconstruct canonical OPY source through the negotiated provider.
    fn convert_opy(&mut self, loaded: &Loaded) -> Result<String, ()> {
        let locale = loaded
            .origin
            .locale
            .as_deref()
            .map(workshop_rs::catalog::Locale::new)
            .unwrap_or_else(|| workshop_rs::catalog::Locale::new("en-US"));
        let artifact = workshop_rs::emitter::emit(&loaded.program, &self.catalog, &locale)
            .map_err(|error| {
                self.diagnostics.push(Diagnostic::error(
                    "workshop-emission",
                    Stage::Emission,
                    error.to_string(),
                ));
            })?;
        let mut provider = self
            .language_provider(opy_provider::OPY_LANGUAGE_ID)
            .map_err(|error| {
                self.diagnostics.push(provider_error_diagnostic(error));
            })?;
        let client_info = wright_lpp::ClientInfo {
            name: crate::result::DRIVER_VERSION.to_string(),
            version: crate::result::DRIVER_VERSION.to_string(),
        };
        provider.initialize(Some(&client_info)).map_err(|error| {
            self.diagnostics.push(provider_error_diagnostic(error));
        })?;
        let result = provider
            .reconstruct(&wright_lpp::WorkshopArtifact {
                format: "workshop-rs/text-v1".to_string(),
                content: artifact,
            })
            .map_err(|error| {
                self.diagnostics.push(provider_error_diagnostic(error));
            })?;
        let _ = provider.shutdown();
        Ok(result.source)
    }

    /// Build the semantic service over a loaded program with an explicit lint
    /// configuration.
    fn service_with<'a>(
        &self,
        loaded: &'a Loaded,
        config: LintConfig,
    ) -> Result<SemanticService<'a>, Diagnostic> {
        let origin = ServiceOrigin {
            kind: if loaded.provenance == Provenance::Unmapped {
                "provider-artifact".to_string()
            } else {
                loaded.origin.kind.clone()
            },
            locale: loaded.origin.locale.clone(),
        };
        Ok(SemanticService::with_origin_and_config_and_registry(
            &loaded.program,
            origin,
            config,
            Arc::clone(&self.lint_registry),
        ))
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
        match provider.check(&loaded.input.text, path) {
            Ok(diagnostics) => {
                for mut diagnostic in diagnostics {
                    diagnostic.source = Some(loaded.origin.clone());
                    self.diagnostics.push(diagnostic);
                }
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
            .any(|d| d.severity == crate::diag::Severity::Error);
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
}

/// Extract the `result` payload of a semantic-service request as JSON.
fn service_response(service: &SemanticService<'_>, request: &Request) -> serde_json::Value {
    match service.handle(request) {
        wright_analyzer::service::Response::Ok { result } => result,
        wright_analyzer::service::Response::Error { .. } => serde_json::Value::Null,
    }
}

fn semantic_facts(service: &SemanticService<'_>) -> serde_json::Value {
    let mut symbols = service_response(service, &Request::ListSymbols { kind: None });
    if let Some(list) = symbols.as_array_mut() {
        for s in list {
            let id = s.get("id").and_then(serde_json::Value::as_u64).unwrap_or(0) as u32;
            s["usage"] = service_response(service, &Request::GetUsage { symbol: id });
        }
    }
    let mut rules = service_response(service, &Request::ListRules);
    if let Some(list) = rules.as_array_mut() {
        for r in list {
            let id = r.get("id").and_then(serde_json::Value::as_u64).unwrap_or(0) as u32;
            let cfg = service_response(service, &Request::GetCfg { rule: id });
            let blocks = cfg["blocks"].as_array().map_or(&[][..], Vec::as_slice);
            let (mut edges, mut wait_blocks, mut loop_blocks) = (0, 0, 0);
            for b in blocks {
                edges += b["successors"].as_array().map_or(0, Vec::len);
                wait_blocks += b["waits"].as_bool().unwrap_or(false) as usize;
                loop_blocks += matches!(b["kind"].as_str(), Some("while" | "for")) as usize;
            }
            r["controlFlow"] = serde_json::json!({
                "blocks": blocks.len(),
                "edges": edges,
                "loopBlocks": loop_blocks,
                "waitBlocks": wait_blocks,
            });
        }
    }
    serde_json::json!({ "symbols": symbols, "rules": rules })
}

pub(crate) fn resolve_finding_span_paths(findings: &mut serde_json::Value, loaded: &Loaded) {
    let Some(list) = findings.as_array_mut() else {
        return;
    };
    for finding in list {
        let Some(span) = finding.get_mut("span").filter(|s| s.is_object()) else {
            continue;
        };
        let path = if loaded.provenance == Provenance::Unmapped {
            "<provider-artifact>".into()
        } else {
            span.get("file")
                .and_then(serde_json::Value::as_u64)
                .map(|file| {
                    loaded
                        .source_files
                        .get(file as usize)
                        .map(|source| {
                            let path = if file == 0 {
                                loaded
                                    .input
                                    .path
                                    .as_deref()
                                    .or_else(|| Some(Path::new(source)))
                            } else if Path::new(source).is_absolute() {
                                Some(Path::new(source))
                            } else {
                                None
                            };
                            path.and_then(|p| root_relative(Some(p), &loaded.input.root))
                                .unwrap_or_else(|| source.clone())
                        })
                        .unwrap_or_else(|| format!("<file {file}>"))
                })
                .unwrap_or_else(|| loaded.input.display.clone())
        };
        span["path"] = serde_json::Value::String(path);
    }
}

fn root_relative(path: Option<&Path>, root: &Path) -> Option<String> {
    let path = path?;
    let abs_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let abs_root = root
        .canonicalize()
        .or_else(|_| {
            if root.as_os_str().is_empty() {
                std::env::current_dir()
            } else {
                Ok(root.to_path_buf())
            }
        })
        .ok()?;
    abs_path
        .strip_prefix(&abs_root)
        .ok()
        .filter(|r| !r.as_os_str().is_empty())
        .map(|r| r.display().to_string())
}

pub(crate) fn workshop_diag(
    error: workshop_rs::WorkshopError,
    resolved: &ResolvedInput,
) -> Diagnostic {
    use workshop_rs::WorkshopError as WE;
    let (code, stage, span) = match &error {
        WE::Catalog(c) => {
            return Diagnostic::error(
                "catalog-error",
                Stage::Internal,
                format!("{}: {}", c.code, c.message),
            );
        }
        WE::MissingMapping { kind, id, locale } => {
            return Diagnostic::error(
                "missing-mapping",
                Stage::Frontend,
                format!(
                    "missing {kind} mapping for locale '{locale}': '{id}' (fallback emission is opt-in; see workshop-rs EmitOptions)"
                ),
            );
        }
        WE::Unknown { kind, span, .. } => (format!("unknown-{kind}"), Stage::Frontend, span),
        WE::Malformed { span, .. } => ("parse-error".into(), Stage::Frontend, span),
        WE::Unsupported { span, .. } => ("unsupported-construct".into(), Stage::Frontend, span),
    };
    let span = span.map(|s| SourceSpan {
        file: s.file.index(),
        path: resolved.display.clone(),
        start: Position {
            line: s.start.line,
            col: s.start.col,
        },
        end: Position {
            line: s.end.line,
            col: s.end.col,
        },
    });
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

fn provider_artifact_origin(resolved: &ResolvedInput) -> Origin {
    Origin {
        kind: "provider-artifact".into(),
        locale: resolved.origin.locale.clone(),
    }
}

fn workshop_diag_for_unmapped_provider_artifact(
    error: workshop_rs::WorkshopError,
    resolved: &ResolvedInput,
) -> Diagnostic {
    let mut diag = workshop_diag(error, resolved);
    if let Some(span) = &mut diag.span {
        span.path = "<provider-artifact>".into();
    }
    diag.source = Some(provider_artifact_origin(resolved));
    diag
}

fn provider_error_diagnostic(error: wright_lpp::ProviderError) -> Diagnostic {
    let unsupported = matches!(&error, wright_lpp::ProviderError::Lpp(lpp) if matches!(lpp.kind, wright_lpp::LppErrorKind::CapabilityUnavailable | wright_lpp::LppErrorKind::Refusal));
    Diagnostic::error(
        error.code(),
        if unsupported {
            Stage::Frontend
        } else {
            Stage::Internal
        },
        error.to_string(),
    )
}

fn source_provider_unavailable() -> Diagnostic {
    Diagnostic::error(
        "source-provider-unavailable",
        Stage::Internal,
        "the requested source-provider workflow is not currently shipped with Wright",
    )
}
