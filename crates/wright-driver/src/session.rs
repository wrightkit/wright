//! Frontends are selected by [`SourceKind`] behind one contract, so
//! the provider-backed `.opy` workflow can replace the retired adapter bridge
//! without changing callers. Every workflow returns a typed [`Envelope`]
//! whose JSON serialization is the machine-readable CLI contract.

mod edit;
mod semantic;

pub(crate) use semantic::resolve_span_paths;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use workshop_rs::Program;
use wright_analyzer::canonical::SemanticService;
use wright_analyzer::registry::{LintConfig, LintRegistry};

use crate::config::{InputSpec, SessionConfig, SourceKind};
use crate::diag::{
    Diagnostic, Origin, Position, Severity, SourceSpan, Stage, source_provider_unavailable,
    source_span,
};
use crate::input::{self, InputTarget, ResolvedInput};
use crate::input_identity;
use crate::opy_provider;
use crate::progress::{ProgressEvent, ProgressObserver, ProgressPhase};
use crate::result::{
    CheckResult, CompileResult, CompiledOutput, ConvertResult, ConvertTarget, Envelope,
    exit_code_from, version_info,
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
    /// Completeness residuals captured before a transform profile rewrote the
    /// program, when one ran. `None` when the loaded program is the authored
    /// parse and `semantic_issues` can run on it directly (#442).
    pub(crate) completeness_issues: Option<Vec<workshop_rs::rules::SemanticIssue>>,
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

/// The exact Overwatch client element limit (2^15): above it the client
/// refuses an otherwise valid Workshop program at import. Evidence: the
/// pinned upstream OverPy oracle (9.7.10) and current upstream `master` both
/// declare `ELEMENT_LIMIT = 32768` and warn `w_element_limit` past it while
/// still emitting output; the Bastion regression project failed client
/// import over it (`OWBastion/Bastion#263`). `workshop-rs` owns the
/// canonical count; the budget policy is Wright's consumer policy
/// (workshop-rs ADR-0010).
const WORKSHOP_CLIENT_ELEMENT_LIMIT: usize = 32768;

/// The origin reported on diagnostics and services for a loaded program:
/// an unmapped provider artifact is not presented as authored source.
fn loaded_origin(loaded: &Loaded) -> Origin {
    Origin {
        kind: if loaded.provenance == Provenance::Unmapped {
            "provider-artifact".to_string()
        } else {
            loaded.origin.kind.clone()
        },
        locale: loaded.origin.locale.clone(),
    }
}

/// One reusable compiler session.
pub struct CompilerSession {
    /// The session configuration (input, frontend, overrides, format).
    pub config: SessionConfig,
    catalog: Arc<workshop_rs::catalog::Catalog>,
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
        let catalog = wright_analyzer::catalog::builtin().map_err(|error| {
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
        if let Err(message) = config.selection.validate(&lint_registry) {
            return Err(Diagnostic::error(
                "invalid-selection",
                Stage::Discovery,
                message,
            ));
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

    pub(crate) fn catalog(&self) -> &workshop_rs::catalog::Catalog {
        &self.catalog
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
        self.load_with_operation(ProviderOperation::Compile)
    }

    /// Drop the cached program and resolve the input from disk again (#471):
    /// `ToolService` calls this when the input's disk fingerprint changed, so
    /// a long-lived session reflects the current project. A failed reload
    /// leaves the cache empty — the next `load` retries the same resolution
    /// rather than resurrecting the stale program.
    pub(crate) fn reload(&mut self) -> Result<Loaded, Diagnostic> {
        self.loaded = None;
        self.load()
    }

    #[hotpath::measure]
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
        let mut resolved =
            hotpath::measure_block!("load::resolve_input", input::resolve(&self.config))?;
        let provider_backend = match self.config.source_backend {
            SourceBackend::Native => false,
            SourceBackend::Provider => true,
            SourceBackend::Auto => resolved.kind == SourceKind::Opy,
        };
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
        // Completeness residuals describe the authored source; profile passes
        // can fold flagged constructs away, so capture them before transforms
        // run (#442).
        let completeness_issues = (resolved.kind == SourceKind::Workshop
            && self.config.profile != wright_transform::Profile::Off)
            .then(|| {
                hotpath::measure_block!("load::semantic_issues", {
                    program.semantic_issues(&self.catalog)
                })
            });
        apply_profile(&mut program, self.config.profile)?;
        let loaded = Loaded {
            program: Arc::new(program),
            origin: resolved.origin.clone(),
            input: resolved,
            provenance: Provenance::Source,
            source_files: Arc::new(source_files),
            completeness_issues,
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
        let language = SourceLanguage::Opy;
        let entry = resolved
            .path
            .clone()
            .unwrap_or_else(|| Path::new("<stdin>").to_path_buf());
        let target = SourceTarget {
            kind: resolved.target,
            ..SourceTarget::new(language, entry, resolved.cwd.clone())
        }
        .with_project_root(resolved.root.clone());
        if self.source_provider.is_none() {
            let spawn = |session: &Self| {
                session
                    .language_provider(opy_provider::OPY_LANGUAGE_ID)
                    .map_err(|error| crate::source_provider::provider_error(error).diagnostic())
            };
            let mut provider = spawn(self)?;
            let client_info = wright_lpp::ClientInfo {
                name: wright_lpp::LPP_CLIENT_NAME.to_string(),
                version: crate::result::DRIVER_VERSION.to_string(),
            };
            // LPP 1.4 lets the provider return a source-mapped artifact. A
            // provider without it keeps the unmapped LPP 1.1/1.2 session on a
            // restarted process, as the protocol requires after a version
            // mismatch.
            let mut initialize = provider.initialize_artifact_negotiation(Some(&client_info));
            if initialize
                .as_ref()
                .is_err_and(|error| error.code() == "protocol-version-mismatch")
            {
                let _ = provider.shutdown();
                provider = spawn(self)?;
                initialize = match resolved.target {
                    InputTarget::File => provider.initialize_project_loading(Some(&client_info)),
                    InputTarget::Directory => {
                        provider.initialize_project_target(Some(&client_info))
                    }
                };
            }
            initialize
                .map_err(|error| crate::source_provider::provider_error(error).diagnostic())?;
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
                completeness_issues: None,
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
            &*self.catalog,
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
        }
        apply_profile(&mut program, self.config.profile)?;
        resolved.origin.locale = Some(locale.to_string());
        let loaded = Loaded {
            program: Arc::new(program),
            origin: resolved.origin.clone(),
            input: resolved.clone(),
            provenance,
            source_files: Arc::new(source_files),
            completeness_issues: None,
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

    /// Run a provider-driven mutation flow (#139) over a fresh provider
    /// session: spawn by opaque language id, initialize with `client`, run
    /// the flow, and terminate gracefully.
    ///
    /// Any failure before the flow — an unconfigured language id, a spawn
    /// failure, a failed handshake — is the same structured
    /// [`crate::provider_edit::ProviderMutation`] refusal surface the flow
    /// itself uses, so callers handle one refusal contract. The provider
    /// process never outlives the request: graceful shutdown when possible,
    /// and the provider's drop guard terminates it otherwise.
    pub fn run_provider_flow(
        &self,
        language_id: &str,
        client: &wright_lpp::ClientInfo,
        flow: impl FnOnce(
            &mut dyn wright_lpp::LanguageProvider,
        ) -> crate::provider_edit::ProviderMutation,
    ) -> crate::provider_edit::ProviderMutation {
        let mut provider = match self.language_provider(language_id) {
            Ok(provider) => provider,
            Err(error) => return crate::provider_edit::provider_failure(&error),
        };
        if let Err(error) = provider.initialize(Some(client)) {
            return crate::provider_edit::provider_failure(&error);
        }
        let mutation = flow(provider.as_mut());
        let _ = provider.shutdown();
        mutation
    }

    /// The locale a Workshop input resolved to, if the last load was
    /// Workshop-origin.
    pub fn resolved_locale(&self) -> Option<String> {
        self.loaded
            .as_ref()
            .and_then(|loaded| loaded.origin.locale.clone())
    }

    /// The include root the last load resolved against, if any. Reported
    /// `span.path` spellings on finding surfaces are root-relative to it.
    pub fn input_root(&self) -> Option<PathBuf> {
        self.loaded.as_ref().map(|loaded| loaded.input.root.clone())
    }

    fn load_workshop(&mut self, resolved: &ResolvedInput) -> Result<(Program, String), Diagnostic> {
        let override_locale = self
            .config
            .locale
            .as_deref()
            .map(workshop_rs::catalog::Locale::new);
        let locale = hotpath::measure_block!("load::detect_locale", {
            workshop_rs::detect::resolve_locale(
                &resolved.text,
                &self.catalog,
                override_locale.as_ref(),
            )
        })
        .map_err(|error| workshop_diag(error, resolved))?;
        let program = hotpath::measure_block!("load::parse", {
            workshop_rs::parser::parse_with_context(
                &resolved.text,
                &self.catalog,
                &locale,
                &*self.catalog,
            )
        })
        .map_err(|error| workshop_diag(error, resolved))?;
        self.progress(ProgressEvent::new(ProgressPhase::Validation));
        hotpath::measure_block!("load::validate", program.validate())
            .map_err(|error| workshop_diag(error, resolved))?;
        Ok((program, locale.to_string()))
    }

    #[hotpath::measure]
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
        let locale = Self::locale_for(&loaded);
        self.progress(ProgressEvent::new(ProgressPhase::Emission));
        let text = hotpath::measure_block!("compile::emit", {
            workshop_rs::emitter::emit(&loaded.program, &self.catalog, &locale)
        })
        .map_err(|error| workshop_diag(error, &loaded.input))?;
        self.attach_target_limits(&loaded);
        let sha256 = hotpath::measure_block!("compile::sha256", input_identity(&text));
        Ok(CompiledOutput {
            text,
            sha256,
            locale: locale.to_string(),
            written_to: String::new(),
            input_identity: loaded.input.identity.clone(),
        })
    }

    #[hotpath::measure]
    pub fn check(&mut self) -> Envelope<CheckResult> {
        self.with_loaded(
            "check",
            |session| session.load_with_operation(ProviderOperation::Check),
            |session, loaded| {
                session.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
                session.attach_workshop_completeness(&loaded);
                CheckResult {}
            },
        )
    }

    pub fn convert(&mut self, target: ConvertTarget) -> Envelope<ConvertResult> {
        self.with_loaded(
            "convert",
            |session| session.load(),
            |session, loaded| {
                if loaded.input.kind != SourceKind::Workshop {
                    session.diagnostics.push(Diagnostic::error(
                        "convert-input-kind",
                        Stage::Discovery,
                        format!(
                            "convert reconstructs Workshop input; got '{}' input (the declared conversion surface has no direct OPY ↔ OSTW path)",
                            loaded.input.kind.as_str()
                        ),
                    ));
                    return ConvertResult::default();
                }
                session.progress(ProgressEvent::new(ProgressPhase::Conversion));
                let text = match target {
                    ConvertTarget::Opy => session.convert_opy(&loaded),
                    ConvertTarget::Ostw => {
                        session.diagnostics.push(source_provider_unavailable());
                        Err(())
                    }
                };
                match text {
                    Ok(text) => ConvertResult {
                        target,
                        sha256: input_identity(&text),
                        text,
                    },
                    Err(()) => ConvertResult::default(),
                }
            },
        )
    }

    fn convert_opy(&mut self, loaded: &Loaded) -> Result<String, ()> {
        let locale = Self::locale_for(loaded);
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

    pub(crate) fn locale_for(loaded: &Loaded) -> workshop_rs::catalog::Locale {
        loaded
            .origin
            .locale
            .as_deref()
            .map(workshop_rs::catalog::Locale::new)
            .unwrap_or_else(|| workshop_rs::catalog::Locale::new("en-US"))
    }

    /// Build the semantic service over a loaded program.
    fn service<'a>(&self, loaded: &'a Loaded) -> SemanticService<'a> {
        self.service_with(loaded, LintConfig::default())
    }

    /// Build the semantic service over a loaded program with an explicit lint
    /// configuration.
    pub(crate) fn service_with<'a>(
        &self,
        loaded: &'a Loaded,
        config: LintConfig,
    ) -> SemanticService<'a> {
        SemanticService::with_origin_and_config_and_registry(
            &loaded.program,
            loaded_origin(loaded),
            config,
            Arc::clone(&self.lint_registry),
        )
    }

    #[hotpath::measure]
    pub(crate) fn shared_service_with(
        &self,
        loaded: &Loaded,
        config: LintConfig,
    ) -> SemanticService<'static> {
        SemanticService::with_shared_program(
            Arc::clone(&loaded.program),
            loaded_origin(loaded),
            config,
            Arc::clone(&self.lint_registry),
        )
    }

    /// Known exact Overwatch client import constraints, measured on the
    /// emitted program during `compile` (#488).
    ///
    /// `workshop-rs` owns the canonical measurement; Wright owns the limit
    /// policy and reports a violated import budget as a warning — a program
    /// over the budget still emits its artifact, matching upstream
    /// `w_element_limit`. New exact constraints attach to this path as
    /// `workshop-rs` exposes them (`wrightkit/workshop-rs#346`), so target
    /// diagnostics stay free of source-language logic. `check` does not run
    /// this: it is a source-correctness workflow, not a client-import gate.
    fn attach_target_limits(&mut self, loaded: &Loaded) {
        let report = match hotpath::measure_block!("compile::element_count", {
            loaded.program.element_count(&self.catalog)
        }) {
            Ok(report) => report,
            Err(error) => {
                // Without the canonical count the budget is unevaluated —
                // say so rather than let a missing warning read as "within
                // the limit".
                self.diagnostics.push(Diagnostic {
                    code: "element-count-unavailable".to_string(),
                    stage: Stage::Emission,
                    severity: Severity::Info,
                    message: format!(
                        "the canonical element count is unavailable ({error}); the {WORKSHOP_CLIENT_ELEMENT_LIMIT}-element client import limit was not evaluated"
                    ),
                    status: None,
                    span: None,
                    source: Some(loaded_origin(loaded)),
                });
                return;
            }
        };
        if report.total <= WORKSHOP_CLIENT_ELEMENT_LIMIT {
            return;
        }
        let total = report.total;
        let mut diagnostic = Diagnostic {
            code: "target-element-limit".to_string(),
            stage: Stage::Emission,
            severity: Severity::Warning,
            message: format!(
                "generated Workshop program uses {total} elements, over the {WORKSHOP_CLIENT_ELEMENT_LIMIT}-element client import limit; the artifact was still emitted but is expected to fail Overwatch client import"
            ),
            status: None,
            span: None,
            source: Some(loaded_origin(loaded)),
        };
        // Point at the largest contributing rule: the concise hotspot that
        // `analyze` reports in full.
        if let Some(top) = report.rules.iter().max_by_key(|rule| rule.count) {
            diagnostic.message.push_str(&format!(
                "; largest contributor: rule \"{}\" ({} elements)",
                top.name, top.count
            ));
            diagnostic.span = top.span.map(|span| {
                source_span(
                    span,
                    semantic::span_path(Some(span.file.index() as u64), loaded),
                )
            });
        }
        self.diagnostics.push(diagnostic);
    }

    /// Structural validation permits source-preserving Workshop fallbacks.
    /// Surface those nodes as blocking semantic diagnostics before presenting
    /// check/lint output as definitive. The catalog remains owned by
    /// workshop-rs; this is only the consumer-side diagnostic projection.
    ///
    /// Residuals come from the loaded canonical `Program` — pre-captured when
    /// a transform profile ran — so check/lint never reparse or revalidate
    /// the source (#442).
    fn attach_workshop_completeness(&mut self, loaded: &Loaded) {
        if loaded.input.kind != SourceKind::Workshop {
            return;
        }
        let path = loaded
            .input
            .path
            .as_deref()
            .unwrap_or_else(|| Path::new("<stdin>"));
        let computed;
        let residuals = match &loaded.completeness_issues {
            Some(residuals) => residuals.as_slice(),
            None => {
                computed = loaded.program.semantic_issues(&self.catalog);
                &computed
            }
        };
        for issue in residuals {
            let d = crate::workshop_provider::map_issue(issue, path);
            self.diagnostics.push(Diagnostic {
                code: d.code,
                stage: Stage::Analysis,
                severity: d.severity,
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

    /// The lint registry backing this session, for finding-selection
    /// validation (#430).
    pub(crate) fn lint_registry(&self) -> &LintRegistry {
        &self.lint_registry
    }

    /// The bases under which a `file` selection and reported `span.path`
    /// spellings resolve (#430): the loaded input's cwd and root, or the
    /// config-derived equivalents so pre-load diagnostics select the same
    /// way. Extra bases are safe — they only ever add true file identities.
    fn selection_file_bases(&self) -> Vec<PathBuf> {
        if let Some(loaded) = &self.loaded {
            return crate::select::file_bases(&loaded.input);
        }
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let absolute = |path: &Path| {
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                cwd.join(path)
            }
        };
        let mut bases = vec![cwd.clone()];
        if let Some(root) = &self.config.root {
            bases.push(absolute(root));
        }
        if let InputSpec::Path(path) = &self.config.input {
            let path = absolute(path);
            bases.push(path.clone());
            if let Some(parent) = path.parent() {
                bases.push(parent.to_path_buf());
            }
        }
        bases
    }

    fn finish<T: serde::Serialize>(&mut self, command: &str, result: T) -> Envelope<T> {
        let diagnostics = std::mem::take(&mut self.diagnostics);
        let exit = exit_code_from(&diagnostics);
        // Selection narrows the reported diagnostics after the exit code is
        // fixed; filtering must never change the verdict.
        let bases = self.selection_file_bases();
        let (diagnostics, selection) = self.config.selection.apply_diagnostics(diagnostics, &bases);
        Envelope {
            wright: version_info(),
            command: command.to_string(),
            ok: exit == crate::result::exit::SUCCESS,
            exit,
            diagnostics,
            selection,
            result,
        }
    }

    fn with_loaded<T>(
        &mut self,
        command: &str,
        load: impl FnOnce(&mut Self) -> Result<Loaded, Diagnostic>,
        run: impl FnOnce(&mut Self, Loaded) -> T,
    ) -> Envelope<T>
    where
        T: Default + serde::Serialize,
    {
        let result = match load(self) {
            Ok(loaded) => run(self, loaded),
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                T::default()
            }
        };
        self.finish(command, result)
    }
}

#[hotpath::measure]
fn apply_profile(
    program: &mut Program,
    profile: wright_transform::Profile,
) -> Result<(), Diagnostic> {
    if profile != wright_transform::Profile::Off {
        wright_transform::run_validated(program, profile).map_err(|error| {
            Diagnostic::error(
                "transform-error",
                Stage::Internal,
                format!("Workshop transformation failed: {error}"),
            )
        })?;
    }
    Ok(())
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
        s.map(|span| crate::diag::source_span(span, resolved.display.clone()))
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
        _ => ("workshop-error".to_string(), Stage::Internal, None),
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
