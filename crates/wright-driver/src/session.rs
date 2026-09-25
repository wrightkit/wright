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
    SourceTarget, provider_uri_path,
};

fn error_kind(error: &opy_provider::OpyProviderError) -> wright_lpp::LocalProviderErrorKind {
    match error {
        opy_provider::OpyProviderError::Missing(_) => wright_lpp::LocalProviderErrorKind::Missing,
        opy_provider::OpyProviderError::UnsupportedPlatform(_) => {
            wright_lpp::LocalProviderErrorKind::UnsupportedPlatform
        }
        opy_provider::OpyProviderError::Offline(_) => wright_lpp::LocalProviderErrorKind::Offline,
        opy_provider::OpyProviderError::Download(_) => wright_lpp::LocalProviderErrorKind::Download,
        opy_provider::OpyProviderError::Integrity(_) => {
            wright_lpp::LocalProviderErrorKind::Integrity
        }
        opy_provider::OpyProviderError::Install(_) => wright_lpp::LocalProviderErrorKind::Install,
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
    /// The program came from a provider-returned canonical artifact whose
    /// source map was applied; nodes without an authored origin stay unmapped.
    Mapped,
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
            let is_provider = self.config.source_backend != SourceBackend::Native
                && loaded.input.kind == SourceKind::Opy;
            if !is_provider
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
        let provider_backend = match self.config.source_backend {
            SourceBackend::Native => false,
            SourceBackend::Provider => true,
            SourceBackend::Auto => resolved.kind == SourceKind::Opy,
        };
        if provider_backend && resolved.kind == SourceKind::Ostw {
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
        if resolved.kind == SourceKind::Ostw {
            return Err(source_provider_unavailable());
        }
        if provider_backend {
            return self.load_from_source_provider(&mut resolved, provider_operation);
        }
        let (mut program, source_files) = match resolved.kind {
            SourceKind::Workshop => {
                self.progress(ProgressEvent::new(ProgressPhase::Parsing));
                let (prog, loc) = self.load_workshop(&resolved)?;
                resolved.origin.locale = Some(loc);
                (prog, vec![resolved.display.clone()])
            }
            SourceKind::Protocol => {
                return Err(Diagnostic::error(
                    "input-kind-unsupported",
                    Stage::Discovery,
                    "the retired protocol Workshop representation is not a canonical Program input",
                ));
            }
            SourceKind::Opy => return Err(source_provider_unavailable()),
            SourceKind::Auto => {
                return Err(Diagnostic::error(
                    "input-kind-unknown",
                    Stage::Discovery,
                    "input kind could not be detected; pass `--kind opy|ostw|workshop|protocol`",
                ));
            }
            SourceKind::Ostw => unreachable!(),
        };
        if self.config.profile != wright_transform::Profile::Off {
            wright_transform::run_canonical(&mut program, self.config.profile).map_err(|e| {
                Diagnostic::error(
                    "transform-error",
                    Stage::Internal,
                    format!("Workshop transformation failed: {e}"),
                )
            })?;
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
            let client_info = wright_lpp::ClientInfo {
                name: wright_lpp::LPP_CLIENT_NAME.to_string(),
                version: crate::result::DRIVER_VERSION.to_string(),
            };
            // LPP 1.4 lets the provider return a source-mapped artifact; a
            // provider without it keeps the unmapped LPP 1.1/1.2 session.
            let initialize = match provider.initialize_artifact_negotiation(Some(&client_info)) {
                Err(error) if error.code() == "protocol-version-mismatch" => {
                    match resolved.target {
                        InputTarget::File => {
                            provider.initialize_project_loading(Some(&client_info))
                        }
                        InputTarget::Directory => {
                            provider.initialize_project_target(Some(&client_info))
                        }
                    }
                }
                other => other,
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
            let Some(id) = compilation
                .source_identity
                .as_ref()
                .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            else {
                return Err(Diagnostic::error(
                    "source-provider-identity",
                    Stage::Frontend,
                    if compilation.source_identity.is_none() {
                        "the source provider returned no source identity for the directory target"
                    } else {
                        "the source provider returned an invalid SHA-256 source identity"
                    },
                ));
            };
            resolved.identity = id.clone();
        }
        let mut provider_diagnostics = compilation.diagnostics;
        if let Some(index) = provider_diagnostics
            .iter()
            .position(|d| d.severity == Severity::Error)
        {
            let first = provider_diagnostics.remove(index);
            self.diagnostics.extend(provider_diagnostics);
            return Err(first);
        }
        self.diagnostics.extend(provider_diagnostics);
        let source_map = match compilation.provenance {
            SourceProvenance::Unmapped => None,
            SourceProvenance::Mapped(map) => Some(map),
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
                provenance: Provenance::Unmapped,
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
        let mut program = workshop_rs::parser::parse_with_context(
            &workshop_text,
            &self.catalog,
            &locale,
            &self.catalog,
        )
        .map_err(|error| workshop_diag_for_provider_artifact(error, resolved, &[]))?;
        let mut provenance = Provenance::Unmapped;
        let mut source_files = vec![resolved.display.clone()];
        if let Some(map) = &source_map {
            match map.apply(&mut program) {
                Ok(()) => {
                    provenance = Provenance::Mapped;
                    source_files = map.files().iter().map(|f| provider_uri_path(f)).collect();
                }
                Err(error) => self.diagnostics.push(Diagnostic::warning(
                    "source-map-mismatch",
                    Stage::Frontend,
                    format!(
                        "the provider source map does not match its Workshop artifact ({error}); findings are reported as unmapped"
                    ),
                )),
            }
        }
        self.progress(ProgressEvent::new(ProgressPhase::Validation));
        program.validate().map_err(|error| {
            workshop_diag_for_provider_artifact(
                error,
                resolved,
                if provenance == Provenance::Mapped {
                    &source_files
                } else {
                    &[]
                },
            )
        })?;
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
            source_files: Arc::new(source_files),
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
        id: &str,
    ) -> Result<Box<dyn wright_lpp::LanguageProvider>, wright_lpp::ProviderError> {
        if id == opy_provider::OPY_LANGUAGE_ID && !self.config.providers.contains(id) {
            let res = self.config.opy_provider.resolve().map_err(|e| {
                wright_lpp::ProviderError::Local {
                    kind: error_kind(&e),
                    message: e.to_string(),
                }
            })?;
            let mut p = self.config.providers.clone();
            p.register(wright_lpp::ProviderConfig::new(
                opy_provider::OPY_LANGUAGE_ID,
                res.executable,
                Vec::new(),
            ))
            .expect("registered");
            return p
                .spawn(id)
                .map(|b| Box::new(b) as Box<dyn wright_lpp::LanguageProvider>);
        }
        self.config
            .providers
            .spawn(id)
            .map(|b| Box::new(b) as Box<dyn wright_lpp::LanguageProvider>)
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

    pub fn compile(&mut self) -> Envelope<CompileResult> {
        let mut result = CompileResult::default();
        let output = match self.compile_output() {
            Ok(output) => output,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish("compile", result);
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
                    return self.finish("compile", result);
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
        self.finish("compile", result)
    }

    fn compile_output(&mut self) -> Result<CompiledOutput, Diagnostic> {
        let loaded = self.load_with_operation(ProviderOperation::Compile)?;
        let locale = loaded
            .origin
            .locale
            .as_deref()
            .map(workshop_rs::catalog::Locale::new)
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

    pub fn check(&mut self) -> Envelope<CheckResult> {
        let loaded = match self.load_with_operation(ProviderOperation::Check) {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish("check", CheckResult {});
            }
        };
        self.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
        self.attach_workshop_completeness(&loaded);
        self.finish("check", CheckResult {})
    }

    pub fn analyze(&mut self) -> Envelope<AnalyzeResult> {
        let loaded = match self.load_with_operation(ProviderOperation::Compile) {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish("analyze", AnalyzeResult::default());
            }
        };
        let service = match self.service(&loaded) {
            Ok(service) => service,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish("analyze", AnalyzeResult::default());
            }
        };
        self.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
        let mut program = service_response(&service, &Request::Program);
        if let serde_json::Value::Object(object) = &mut program {
            object.remove("findings");
        }
        let facts = semantic_facts(&service);
        self.finish("analyze", AnalyzeResult { program, facts })
    }

    /// `inspect`: load and produce the structural/semantic program model.
    pub fn inspect(&mut self) -> Envelope<InspectResult> {
        let loaded = match self.load() {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish("inspect", InspectResult::default());
            }
        };
        let service = match self.service(&loaded) {
            Ok(service) => service,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish("inspect", InspectResult::default());
            }
        };
        self.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
        let program = service_response(&service, &Request::Program);
        let rules = service_response(&service, &Request::ListRules);
        let symbols = service_response(&service, &Request::ListSymbols { kind: None });
        let references = serde_json::Value::Array(
            symbols
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|s| s.get("id").and_then(serde_json::Value::as_u64))
                        .map(|id| {
                            service_response(
                                &service,
                                &Request::FindReferences { symbol: id as u32 },
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
        );
        self.finish(
            "inspect",
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
        let loaded = match self.load_with_operation(ProviderOperation::Compile) {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish(command, LintResult::default());
            }
        };
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
        let (rules, config, skipped) = if let serde_json::Value::Object(mut object) = lint_rules {
            (
                object
                    .remove("rules")
                    .unwrap_or_else(|| serde_json::json!([])),
                object
                    .remove("config")
                    .unwrap_or_else(|| serde_json::json!({})),
                object
                    .remove("skipped")
                    .unwrap_or_else(|| serde_json::json!([])),
            )
        } else {
            (
                serde_json::json!([]),
                serde_json::json!({}),
                serde_json::json!([]),
            )
        };
        self.finish(
            "lint",
            LintResult {
                input_identity: loaded.input.identity.clone(),
                program,
                rules,
                config,
                findings,
                skipped,
            },
        )
    }

    pub fn convert(&mut self, target: ConvertTarget) -> Envelope<ConvertResult> {
        let loaded = match self.load() {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish("convert", ConvertResult::default());
            }
        };
        if loaded.input.kind != SourceKind::Workshop {
            self.diagnostics.push(Diagnostic::error(
                "convert-input-kind",
                Stage::Discovery,
                format!(
                    "convert reconstructs Workshop input; got '{}' input (the declared conversion surface has no direct OPY ↔ OSTW path)",
                    loaded.input.kind.as_str()
                ),
            ));
            return self.finish("convert", ConvertResult::default());
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
                    "convert",
                    ConvertResult {
                        target,
                        text,
                        sha256,
                    },
                )
            }
            Err(()) => self.finish("convert", ConvertResult::default()),
        }
    }

    fn convert_opy(&mut self, loaded: &Loaded) -> Result<String, ()> {
        let locale = loaded
            .origin
            .locale
            .as_deref()
            .map(workshop_rs::catalog::Locale::new)
            .unwrap_or_else(|| workshop_rs::catalog::Locale::new("en-US"));
        let artifact = workshop_rs::emitter::emit(&loaded.program, &self.catalog, &locale)
            .map_err(|e| {
                self.diagnostics.push(Diagnostic::error(
                    "workshop-emission",
                    Stage::Emission,
                    e.to_string(),
                ));
            })?;
        let mut provider = self
            .language_provider(opy_provider::OPY_LANGUAGE_ID)
            .map_err(|e| {
                self.diagnostics.push(provider_error_diagnostic(e));
            })?;
        provider
            .initialize(Some(&wright_lpp::ClientInfo {
                name: crate::result::DRIVER_VERSION.to_string(),
                version: crate::result::DRIVER_VERSION.to_string(),
            }))
            .map_err(|e| {
                self.diagnostics.push(provider_error_diagnostic(e));
            })?;
        let result = provider
            .reconstruct(&wright_lpp::WorkshopArtifact {
                format: "workshop-rs/text-v1".to_string(),
                content: artifact,
            })
            .map_err(|e| {
                self.diagnostics.push(provider_error_diagnostic(e));
            })?;
        let _ = provider.shutdown();
        Ok(result.source)
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
            Ok(p) => p,
            Err(e) => {
                self.diagnostics.push(Diagnostic::error(
                    "workshop-provider-init",
                    Stage::Internal,
                    e.to_string(),
                ));
                return;
            }
        };
        let path = loaded
            .input
            .path
            .as_deref()
            .unwrap_or_else(|| Path::new("<stdin>"));
        match crate::provider::LanguageProvider::check(&provider, &loaded.input.text, path) {
            Ok(diagnostics) => {
                for d in diagnostics {
                    self.diagnostics.push(Diagnostic {
                        code: d.code,
                        stage: Stage::Analysis,
                        severity: match d.severity {
                            crate::provider::Severity::Error => Severity::Error,
                            crate::provider::Severity::Warning => Severity::Warning,
                            crate::provider::Severity::Info => Severity::Info,
                        },
                        message: d.message,
                        span: Some(SourceSpan {
                            file: 0,
                            path: d.span.file.display().to_string(),
                            start: Position {
                                line: d.span.start_line,
                                col: d.span.start_col,
                            },
                            end: Position {
                                line: d.span.end_line,
                                col: d.span.end_col,
                            },
                        }),
                        status: Some(d.status),
                        source: Some(loaded.origin.clone()),
                    });
                }
            }
            Err(e) => self.diagnostics.push(Diagnostic::error(
                "workshop-provider-check",
                Stage::Internal,
                e.to_string(),
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
    let symbols = service_response(service, &Request::ListSymbols { kind: None })
        .as_array()
        .map(|symbols| {
            symbols
                .iter()
                .map(|s| {
                    let id = s
                        .get("id")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or_default() as u32;
                    let usage = service_response(service, &Request::GetUsage { symbol: id });
                    serde_json::json!({
                        "id": s["id"],
                        "kind": s["kind"],
                        "name": s["name"],
                        "span": s.get("span").cloned().unwrap_or(serde_json::Value::Null),
                        "usage": usage,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let rules = service_response(service, &Request::ListRules)
        .as_array()
        .map(|rules| {
            rules
                .iter()
                .map(|r| {
                    let id = r
                        .get("id")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or_default() as u32;
                    let cfg = service_response(service, &Request::GetCfg { rule: id });
                    let blocks = cfg["blocks"].as_array().cloned().unwrap_or_default();
                    let edge_count = blocks
                        .iter()
                        .map(|b| b["successors"].as_array().map_or(0, Vec::len))
                        .sum::<usize>();
                    let wait_blocks = blocks
                        .iter()
                        .filter(|b| b["waits"].as_bool().unwrap_or(false))
                        .count();
                    let loop_blocks = blocks
                        .iter()
                        .filter(|b| matches!(b["kind"].as_str(), Some("while" | "for")))
                        .count();
                    serde_json::json!({
                        "id": r["id"],
                        "name": r["name"],
                        "span": r.get("span").cloned().unwrap_or(serde_json::Value::Null),
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
/// the retained frontend file registry. `<file N>` is the fallback when no
/// registry entry resolves (matching the [`span_from_json`] convention), and
/// stdin inputs fall back to their display identity (`<stdin>`).
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
        let path = if loaded.provenance == Provenance::Unmapped {
            "<provider-artifact>".to_string()
        } else if loaded.provenance == Provenance::Mapped {
            // Every mapped file is an authored source; file 0 is not the input.
            let file = span.get("file").and_then(serde_json::Value::as_u64);
            match file.and_then(|file| loaded.source_files.get(file as usize)) {
                Some(source) => root_relative(Some(Path::new(source)), &loaded.input.root)
                    .unwrap_or_else(|| source.clone()),
                None => "<provider-artifact>".to_string(),
            }
        } else if let Some(file) = span.get("file").and_then(serde_json::Value::as_u64) {
            if let Some(source) = loaded.source_files.get(file as usize) {
                let p = if file == 0 {
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
                p.and_then(|path| root_relative(Some(path), &loaded.input.root))
                    .unwrap_or_else(|| source.clone())
            } else {
                format!("<file {file}>")
            }
        } else {
            loaded.input.display.clone()
        };
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
pub(crate) fn workshop_diag(
    error: workshop_rs::WorkshopError,
    resolved: &ResolvedInput,
) -> Diagnostic {
    let to_span = |s: Option<workshop_rs::source::Span>| {
        s.map(|span| SourceSpan {
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
        })
    };
    let (code, stage, span) = match &error {
        workshop_rs::WorkshopError::Catalog(catalog) => {
            return Diagnostic::error(
                "catalog-error",
                Stage::Internal,
                format!("{}: {}", catalog.code, catalog.message),
            );
        }
        workshop_rs::WorkshopError::Unknown { kind, span, .. } => {
            (format!("unknown-{kind}"), Stage::Frontend, to_span(*span))
        }
        workshop_rs::WorkshopError::Malformed { span, .. } => {
            ("parse-error".to_string(), Stage::Frontend, to_span(*span))
        }
        workshop_rs::WorkshopError::Unsupported { span, .. } => (
            "unsupported-construct".to_string(),
            Stage::Frontend,
            to_span(*span),
        ),
        workshop_rs::WorkshopError::MissingMapping { .. } => {
            ("missing-mapping".to_string(), Stage::Frontend, None)
        }
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

fn provider_artifact_origin(resolved: &ResolvedInput) -> Origin {
    Origin {
        kind: "provider-artifact".to_string(),
        locale: resolved.origin.locale.clone(),
    }
}

/// Map a Workshop error on a provider artifact to a driver diagnostic.
///
/// `mapped_files` is the applied source map's file table: a span into it is an
/// authored location. Without one, the span points into the provider artifact.
fn workshop_diag_for_provider_artifact(
    error: workshop_rs::WorkshopError,
    resolved: &ResolvedInput,
    mapped_files: &[String],
) -> Diagnostic {
    let mut diagnostic = workshop_diag(error, resolved);
    if mapped_files.is_empty() {
        if let Some(span) = &mut diagnostic.span {
            span.path = "<provider-artifact>".to_string();
        }
        diagnostic.source = Some(provider_artifact_origin(resolved));
    } else if let Some(span) = &mut diagnostic.span {
        match mapped_files.get(span.file) {
            Some(path) => {
                span.path = root_relative(Some(Path::new(path)), &resolved.root)
                    .unwrap_or_else(|| path.clone());
            }
            None => diagnostic.span = None,
        }
    }
    if diagnostic.span.is_none() {
        diagnostic.source = Some(provider_artifact_origin(resolved));
    }
    diagnostic
}

fn provider_error_diagnostic(error: wright_lpp::ProviderError) -> Diagnostic {
    let unsupported = matches!(
        &error,
        wright_lpp::ProviderError::Lpp(lpp)
            if matches!(
                lpp.kind,
                wright_lpp::LppErrorKind::CapabilityUnavailable | wright_lpp::LppErrorKind::Refusal
            )
    );
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
