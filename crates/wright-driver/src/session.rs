mod diagnostics;
mod edit;
#[cfg(not(target_arch = "wasm32"))]
mod protocol;
mod registry;
mod semantic;

#[cfg(not(target_arch = "wasm32"))]
use diagnostics::provider_error_diagnostic;
use diagnostics::workshop_diag_for_provider_artifact;
#[cfg(not(target_arch = "wasm32"))]
use protocol::mutual_protocol_version;
pub(crate) use semantic::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use workshop_rs::Program;
use wright_analyzer::canonical::SemanticService;
use wright_analyzer::registry::{LintConfig, LintRegistry};

use crate::cache;
use crate::config::{InputSpec, SessionConfig, SourceKind};
use crate::diag::{
    Diagnostic, Origin, Position, Severity, SourceSpan, Stage, Status, source_provider_unavailable,
    source_span, workshop_diag,
};
use crate::input::{self, InputTarget, ResolvedInput};
use crate::input_identity;
#[cfg(not(target_arch = "wasm32"))]
use crate::opy_provider;
use crate::output::{CompileWrite, OutputDest, Overwrite};
use crate::progress::{ProgressEvent, ProgressObserver, ProgressPhase};
use crate::result::{
    CheckResult, CompileResult, CompiledOutput, ConvertResult, ConvertTarget, Envelope,
    version_info,
};
use crate::source_provider::{
    SourceBackend, SourceLanguage, SourceProvenance, SourceProvider, SourceProviderError,
    SourceTarget, provider_uri_path,
};

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
    pub(crate) source_files: Arc<Vec<String>>,
    pub(crate) completeness_issues: Option<Vec<workshop_rs::rules::SemanticIssue>>,
    pub(crate) load_diagnostics: Arc<Vec<Diagnostic>>,
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
pub const WORKSHOP_CLIENT_ELEMENT_LIMIT: usize = 32768;

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
    ///
    /// Configuration is fixed at construction (#511): assign every field
    /// before `new`/`with_source_provider`. The field stays public for
    /// `wright-embedding/v1` source compatibility, but mutating it is
    /// refused `session-config-changed` — changing configuration means
    /// constructing a new session.
    pub config: SessionConfig,
    fixed_config: SessionConfig,
    catalog: &'static workshop_rs::catalog::Catalog,
    lint_registry: Arc<LintRegistry>,
    loaded: Option<Loaded>,
    loaded_operation: Option<ProviderOperation>,
    semantic: Option<SessionSemantic>,
    semantic_builds: usize,
    diagnostics: Vec<Diagnostic>,
    progress_observer: Option<Arc<dyn ProgressObserver>>,
    source_provider: Option<Box<dyn SourceProvider>>,
}

struct SessionSemantic {
    program_key: Arc<Program>,
    service: Arc<SemanticService<'static>>,
}

impl CompilerSession {
    /// Build a session from a configuration.
    pub fn new(config: SessionConfig) -> Result<CompilerSession, Diagnostic> {
        let catalog = workshop_rs::catalog::Catalog::builtin_ref().map_err(|error| {
            Diagnostic::error(
                "catalog-error",
                Stage::Internal,
                format!("cannot load the built-in Workshop catalog: {}", error),
            )
        })?;
        let lint_registry = registry::compose(&config)?;
        Ok(CompilerSession {
            fixed_config: config.clone(),
            config,
            catalog,
            lint_registry: Arc::new(lint_registry),
            loaded: None,
            loaded_operation: None,
            semantic: None,
            semantic_builds: 0,
            diagnostics: Vec::new(),
            progress_observer: None,
            source_provider: None,
        })
    }

    /// Build a session whose source-language workflow must use `provider`.
    /// The provider is injected at the product boundary; its transport and
    /// project model are not visible to the session. Provider failures are
    /// surfaced as-is, never falling back to an in-process OPY path.
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

    pub(crate) fn verify_fixed_config(&self) -> Result<(), Diagnostic> {
        let changed = self.config.changed_fields(&self.fixed_config);
        if changed.is_empty() {
            return Ok(());
        }
        Err(Diagnostic::error(
            "session-config-changed",
            Stage::Discovery,
            format!(
                "session configuration is fixed after construction; changed {} — construct a new CompilerSession to change configuration",
                changed.join(", ")
            ),
        ))
    }

    pub(crate) fn catalog(&self) -> &workshop_rs::catalog::Catalog {
        self.catalog
    }

    fn progress(&self, event: ProgressEvent) {
        if let Some(observer) = &self.progress_observer {
            observer.on_progress(event);
        }
    }

    /// Load (or reuse) the validated program for this session.
    ///
    /// Loading is idempotent: repeated calls return the same program.
    /// Returns an owned snapshot so callers can hold it while mutating the
    /// session.
    ///
    /// On the provider backend this is the canonical-snapshot read, not a
    /// workflow (#591): it serves the snapshot a compile-producing operation
    /// established and refuses when the session holds none. Workflows that
    /// may compile call [`Self::load_with_operation`] instead, so the
    /// provider's own compile answer — including a capability decline —
    /// reaches the caller (#655).
    pub fn load(&mut self) -> Result<Loaded, Diagnostic> {
        self.verify_fixed_config()?;
        if self.config.source_backend == SourceBackend::Provider
            && self.loaded_operation != Some(ProviderOperation::Compile)
        {
            return Err(SourceProviderError::Unsupported {
                message: "the provider session holds no canonical compile result — the provider declined its compile operation or no compile-producing workflow has run".to_string(),
            }
            .diagnostic());
        }
        self.load_with_operation(ProviderOperation::Compile)
    }

    pub(crate) fn reload(&mut self) -> Result<Loaded, Diagnostic> {
        self.drop_loaded();
        self.load()
    }

    pub(crate) fn drop_loaded(&mut self) {
        self.loaded = None;
        self.semantic = None;
    }

    pub(crate) fn cached_loaded(&self) -> Option<Loaded> {
        self.loaded.clone()
    }

    #[allow(unsafe_code)] // hotpath 0.28 emits `#[unsafe(export_name)]` statics
    #[hotpath::measure]
    fn load_with_operation(
        &mut self,
        provider_operation: ProviderOperation,
    ) -> Result<Loaded, Diagnostic> {
        self.verify_fixed_config()?;
        if let Some(loaded) = &self.loaded {
            let is_provider = self.config.source_backend != SourceBackend::Native
                && loaded.input.kind == SourceKind::Opy;
            if !is_provider
                || self.loaded_operation == Some(provider_operation)
                || (self.loaded_operation == Some(ProviderOperation::Compile)
                    && provider_operation == ProviderOperation::Check)
            {
                self.diagnostics
                    .extend(loaded.load_diagnostics.iter().cloned());
                return Ok(loaded.clone());
            }
        }
        if self.config.source_backend == SourceBackend::Provider
            && !matches!(self.config.input, InputSpec::Path(_))
        {
            return Err(SourceProviderError::Unsupported {
                message: "provider-backed source workflows require a path input; the provider resolves the project sources itself, so stdin and in-memory text have no entry identity it can load".to_string(),
            }
            .diagnostic());
        }
        self.progress(ProgressEvent::new(ProgressPhase::InputResolution));
        let mut resolved =
            hotpath::measure_block!("load::resolve_input", input::resolve(&self.config))?;
        if self.config.profile != wright_transform::Profile::Off
            && resolved.kind != SourceKind::Workshop
        {
            return Err(Diagnostic::error(
                "profile-workshop-only",
                Stage::Discovery,
                format!(
                    "--profile applies to Workshop input only; got '{}' input",
                    resolved.kind.as_str()
                ),
            ));
        }
        let provider_backend = match self.config.source_backend {
            SourceBackend::Native => false,
            SourceBackend::Provider => true,
            SourceBackend::Auto => {
                resolved.kind == SourceKind::Opy
                    && !matches!(self.config.input, InputSpec::Text { .. })
            }
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
        let completeness_issues = (resolved.kind == SourceKind::Workshop
            && self.config.profile != wright_transform::Profile::Off)
            .then(|| {
                hotpath::measure_block!("load::semantic_issues", {
                    program.semantic_issues(self.catalog)
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
            load_diagnostics: Arc::new(Vec::new()),
        };
        self.loaded = Some(loaded.clone());
        Ok(loaded)
    }

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
        let diagnostics_start = self.diagnostics.len();
        // The LPP provider spawn path is native-only (ADR-0037 decision 5):
        // an injected `SourceProvider` still serves the compile below on
        // WebAssembly, but no process transport exists to create one.
        #[cfg(not(target_arch = "wasm32"))]
        if self.source_provider.is_none() {
            let spawn = |session: &Self| {
                session
                    .language_provider(opy_provider::OPY_LANGUAGE_ID)
                    .map_err(|error| {
                        crate::source_provider::provider_error(
                            error,
                            "the 'opy' source provider could not be started",
                        )
                        .diagnostic()
                    })
            };
            let mut provider = spawn(self)?;
            let client_info = wright_lpp::ClientInfo {
                name: wright_lpp::LPP_CLIENT_NAME.to_string(),
                version: crate::result::DRIVER_VERSION.to_string(),
            };
            let mut initialize = provider.initialize_artifact_negotiation(Some(&client_info));
            if initialize
                .as_ref()
                .is_err_and(|error| error.code() == "protocol-version-mismatch")
            {
                // LPP §19.2: retry at the highest mutually supported
                // version the provider advertised, honoring the target's
                // floor — entry requests need 1.1 project loading, and a
                // directory target cannot drop below the 1.3
                // source-identity contract it loads under (#638).
                let supported = initialize
                    .as_ref()
                    .err()
                    .map(|error| error.supported_protocol_versions())
                    .unwrap_or_default();
                let _ = provider.shutdown();
                provider = spawn(self)?;
                initialize = match mutual_protocol_version(&supported, resolved.target) {
                    Some(version) => provider.initialize_version(version, Some(&client_info)),
                    None => Err(wright_lpp::ProviderError::ProtocolVersionMismatch {
                        supported,
                        message: format!(
                            "no mutually supported LPP protocol version serves a {:?} target",
                            resolved.target
                        ),
                    }),
                };
            }
            let init = initialize.map_err(|error| {
                crate::source_provider::provider_error(
                    error,
                    "the 'opy' source provider could not be initialized",
                )
                .diagnostic()
            })?;
            // A directory target's canonical identity is the provider's
            // `sourceIdentity` (LPP 1.3): the spec requires the client to
            // refuse a session that negotiated without the capability
            // (#638).
            if resolved.target == InputTarget::Directory && !init.capabilities.source_identity {
                return Err(crate::source_provider::provider_error(
                    wright_lpp::ProviderError::lpp(
                        wright_lpp::LppErrorKind::CapabilityUnavailable,
                        serde_json::json!({
                            "capability": "sourceIdentity",
                            "method": "lpp/compile",
                        }),
                        "directory targets require the LPP 1.3 'sourceIdentity' capability",
                    ),
                    "the 'opy' source provider could not be initialized",
                )
                .diagnostic());
            }
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
            ProviderOperation::Check => {
                let check = provider
                    .check(&target)
                    .map_err(|error| error.diagnostic())?;
                match provider.compile(&target) {
                    Ok(mut compilation) => {
                        for diagnostic in check.diagnostics {
                            if !compilation.diagnostics.contains(&diagnostic) {
                                compilation.diagnostics.push(diagnostic);
                            }
                        }
                        compilation
                    }
                    Err(error)
                        if matches!(error, SourceProviderError::Unsupported { .. })
                            || error.code() == "capability-unavailable" =>
                    {
                        check
                    }
                    Err(error) => {
                        self.diagnostics.extend(check.diagnostics);
                        return Err(error.diagnostic());
                    }
                }
            }
            ProviderOperation::Compile => provider.compile(&target).map_err(|e| e.diagnostic())?,
        };
        if (operation == ProviderOperation::Compile || compilation.workshop_text.is_some())
            && resolved.target == InputTarget::Directory
        {
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
        if operation == ProviderOperation::Check && compilation.workshop_text.is_none() {
            return Ok(Loaded {
                program: Arc::new(Program::default()),
                origin: resolved.origin.clone(),
                input: resolved.clone(),
                provenance: Provenance::Unmapped,
                source_files: Arc::new(vec![resolved.display.clone()]),
                completeness_issues: None,
                load_diagnostics: Arc::new(Vec::new()),
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
            self.catalog,
            &locale,
            self.catalog,
        )
        .map_err(|error| workshop_diag_for_provider_artifact(error, resolved, &[]))?;
        let mut provenance = Provenance::Unmapped;
        let mut source_files = vec![resolved.display.clone()];
        if let Some(map) = &source_map {
            match map.apply(&mut program) {
                Ok(()) => {
                    provenance = Provenance::Mapped;
                    source_files = map.files().iter().map(|f| provider_uri_path(f)).collect();
                    let mut unreadable = Vec::new();
                    for (index, member) in source_files.iter().enumerate() {
                        let member_path = Path::new(member);
                        let member_path = if member_path.is_absolute() {
                            member_path.to_path_buf()
                        } else {
                            resolved.root.join(member_path)
                        };
                        match std::fs::read_to_string(&member_path) {
                            Ok(text) => {
                                program.set_file_source(
                                    workshop_rs::source::FileId::from_index(index),
                                    text,
                                );
                            }
                            Err(error) => unreadable.push(format!(
                                "{} ({})",
                                member,
                                error
                            )),
                        }
                    }
                    if !unreadable.is_empty() {
                        self.diagnostics.push(Diagnostic::warning(
                            "source-map-member-unreadable",
                            Stage::Frontend,
                            format!(
                                "{} provider source-map member(s) could not be read, so their source extents are unavailable: {}",
                                unreadable.len(),
                                unreadable.join(", "),
                            ),
                        ));
                    }
                }
                Err(error) => self.diagnostics.push(Diagnostic::warning(
                    "source-map-mismatch",
                    Stage::Frontend,
                    format!(
                        "the provider source map does not match its Workshop artifact ({}); findings are reported as unmapped",
                        error
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
        resolved.origin.locale = Some(locale.to_string());
        let loaded = Loaded {
            program: Arc::new(program),
            origin: resolved.origin.clone(),
            input: resolved.clone(),
            provenance,
            source_files: Arc::new(source_files),
            completeness_issues: None,
            load_diagnostics: Arc::new(self.diagnostics[diagnostics_start..].to_vec()),
        };
        self.loaded = Some(loaded.clone());
        self.loaded_operation = Some(ProviderOperation::Compile);
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
    /// in-process compiler semantics. A session whose configuration changed
    /// after construction refuses through the LPP refusal channel with
    /// `refusalCode` `session-config-changed` (#511).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn language_provider(
        &self,
        id: &str,
    ) -> Result<Box<dyn wright_lpp::LanguageProvider>, wright_lpp::ProviderError> {
        if let Err(diagnostic) = self.verify_fixed_config() {
            return Err(wright_lpp::ProviderError::lpp(
                wright_lpp::LppErrorKind::Refusal,
                serde_json::json!({ "refusalCode": diagnostic.code }),
                diagnostic.message,
            ));
        }
        if id == opy_provider::OPY_LANGUAGE_ID && !self.config.providers.contains(id) {
            let res = self.config.opy_provider.resolve()?;
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
    #[cfg(not(target_arch = "wasm32"))]
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

    pub(crate) fn cost_snapshot(&self, loaded: &Loaded) -> Option<cache::CostSnapshot> {
        let report = loaded.program.element_count(self.catalog).ok()?;
        Some(cache::CostSnapshot {
            total: report.total,
            budget: WORKSHOP_CLIENT_ELEMENT_LIMIT,
            rules: report
                .rules
                .iter()
                .enumerate()
                .map(|(index, node)| (loaded.program.rules[index].name.clone(), node.count))
                .collect(),
        })
    }

    pub(crate) fn cost_run(&self, loaded: &Loaded) -> cache::CostRun {
        let run = &self.config.run;
        cache::CostRun {
            no_cache: run.no_cache,
            root: loaded.input.root.clone(),
            input_path: loaded.input.path.clone(),
            kind: loaded.input.kind.as_str().to_string(),
            locale: Self::locale_for(loaded).to_string(),
            profile: self.config.profile.as_str().to_string(),
            baseline: run.baseline.clone(),
            save_baseline: run.save_baseline.clone(),
        }
    }

    pub(crate) fn run_cost_comparison(&self) -> Option<cache::Comparison> {
        let loaded = self.loaded.as_ref()?;
        let run = self.cost_run(loaded);
        let environment = cache::CacheEnvironment::from_env();
        if run.baseline.is_some() && !cache::baseline_exists_in(&run, environment) {
            return None;
        }
        let snapshot = self.cost_snapshot(loaded)?;
        cache::compare_and_record_in(&run, &snapshot, environment)
    }

    pub(crate) fn named_baseline_missing(&self) -> Option<String> {
        let run = self.cost_run(self.loaded.as_ref()?);
        let name = run.baseline.as_deref()?;
        let environment = cache::CacheEnvironment::from_env();
        (!environment.ci && !cache::baseline_exists_in(&run, environment)).then(|| name.to_string())
    }

    #[allow(unsafe_code)] // hotpath 0.28 emits `#[unsafe(export_name)]` statics
    fn load_workshop(&mut self, resolved: &ResolvedInput) -> Result<(Program, String), Diagnostic> {
        let override_locale = self
            .config
            .locale
            .as_deref()
            .map(workshop_rs::catalog::Locale::new);
        let locale = hotpath::measure_block!("load::detect_locale", {
            workshop_rs::detect::resolve_locale(
                &resolved.text,
                self.catalog,
                override_locale.as_ref(),
            )
        })
        .map_err(|error| workshop_diag(error, resolved))?;
        let program = hotpath::measure_block!("load::parse", {
            workshop_rs::parser::parse_with_context(
                &resolved.text,
                self.catalog,
                &locale,
                self.catalog,
            )
        })
        .map_err(|error| workshop_diag(error, resolved))?;
        self.progress(ProgressEvent::new(ProgressPhase::Validation));
        hotpath::measure_block!("load::validate", program.validate())
            .map_err(|error| workshop_diag(error, resolved))?;
        hotpath::measure_block!(
            "load::validate_canonical",
            workshop_rs::validate::validate_canonical_ids_tolerating_residuals(
                &program,
                self.catalog
            )
        )
        .map_err(|error| workshop_diag(error, resolved))?;
        Ok((program, locale.to_string()))
    }

    /// Compile without delivering the program: it stays in the in-process
    /// result's `output.text` (never serialized) and `written_to` is
    /// `stdout`. Use [`Self::compile_to`] to write a file.
    #[hotpath::measure]
    pub fn compile(&mut self) -> Envelope<CompileResult> {
        self.compile_to(CompileWrite {
            dest: OutputDest::Stdout,
            overwrite: Overwrite::Refuse,
            to: None,
        })
    }

    /// Compile and deliver the program as `write` directs (ADR-0025). A
    /// failed compile or a refused destination writes nothing. `write.to`
    /// selects the output locale (#759); `None` emits the loaded input's
    /// locale.
    pub fn compile_to(&mut self, write: CompileWrite<'_>) -> Envelope<CompileResult> {
        let mut result = CompileResult::default();
        let (output, entry) = match self.compile_output(write.to.as_deref()) {
            Ok(compiled) => compiled,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish("compile", result);
            }
        };
        if self
            .diagnostics
            .iter()
            .any(|d| d.severity == crate::diag::Severity::Error)
        {
            return self.finish("compile", result);
        }
        let written = {
            let this = &*self;
            let CompileWrite {
                dest, overwrite, ..
            } = write;
            let text = output.text.as_str();
            match overwrite {
                Overwrite::Ask(ask) => {
                    let mut confirm = |path: &Path| {
                        this.progress(ProgressEvent::new(ProgressPhase::Confirmation));
                        ask(path)
                    };
                    crate::output::write_program(
                        text,
                        entry.as_deref(),
                        CompileWrite {
                            dest,
                            overwrite: Overwrite::Ask(&mut confirm),
                            to: None,
                        },
                    )
                }
                overwrite => crate::output::write_program(
                    text,
                    entry.as_deref(),
                    CompileWrite {
                        dest,
                        overwrite,
                        to: None,
                    },
                ),
            }
        };
        let mut comparison = None;
        let mut missing_baseline = None;
        match written {
            Ok(written_to) => {
                result.output = Some(CompiledOutput {
                    written_to,
                    ..output
                });
                missing_baseline = self.named_baseline_missing();
                if missing_baseline.is_none() {
                    comparison = self.run_cost_comparison();
                }
            }
            Err(diagnostic) => self.diagnostics.push(diagnostic),
        }
        let mut envelope = self.finish("compile", result);
        envelope.comparison = comparison.map(|c| c.to_json());
        if let Some(name) = missing_baseline {
            envelope.diagnostics.push(Diagnostic::error(
                "invalid-selection",
                Stage::Emission,
                format!("unknown baseline '{}': no record under .wright/ for this input kind/locale/profile", name),
            ));
            envelope.ok = false;
            envelope.exit = crate::result::exit::SOURCE_ERROR;
        }
        envelope
    }

    #[allow(unsafe_code)] // hotpath 0.28 emits `#[unsafe(export_name)]` statics
    fn compile_output(
        &mut self,
        to: Option<&str>,
    ) -> Result<(CompiledOutput, Option<PathBuf>), Diagnostic> {
        let named = match to {
            Some(name) => {
                let locale = workshop_rs::catalog::Locale::new(name);
                if !self.catalog.supports(&locale) {
                    return Err(Diagnostic::error(
                        "invalid-locale",
                        Stage::Discovery,
                        format!(
                            "unknown output locale '{}': declared locales are {}",
                            name,
                            self.catalog
                                .locales()
                                .iter()
                                .map(|locale| locale.to_string())
                                .collect::<Vec<_>>()
                                .join(", "),
                        ),
                    ));
                }
                Some(locale)
            }
            None => None,
        };
        let loaded = self.load_with_operation(ProviderOperation::Compile)?;
        let locale = named.unwrap_or_else(|| Self::locale_for(&loaded));
        self.progress(ProgressEvent::new(ProgressPhase::Emission));
        let text = hotpath::measure_block!("compile::emit", {
            workshop_rs::emitter::emit(&loaded.program, self.catalog, &locale)
        })
        .map_err(|error| workshop_diag(error, &loaded.input))?;
        self.attach_target_limits(&loaded);
        let sha256 = hotpath::measure_block!("compile::sha256", input_identity(&text));
        let compiled = CompiledOutput {
            text,
            sha256,
            locale: locale.to_string(),
            written_to: String::new(),
            input_identity: loaded.input.identity.clone(),
        };
        Ok((compiled, loaded.input.path.clone()))
    }

    #[hotpath::measure]
    pub fn check(&mut self) -> Envelope<CheckResult> {
        self.with_loaded(
            "check",
            |session| session.load_with_operation(ProviderOperation::Check),
            |session, loaded| {
                session.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
                session.attach_workshop_completeness(&loaded);
                CheckResult {
                    program: Some(semantic::check_program_summary(&loaded)),
                    next: crate::result::next::list(crate::result::next::CHECK),
                }
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

    #[cfg(target_arch = "wasm32")]
    fn convert_opy(&mut self, _loaded: &Loaded) -> Result<String, ()> {
        self.diagnostics.push(source_provider_unavailable());
        Err(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn convert_opy(&mut self, loaded: &Loaded) -> Result<String, ()> {
        let locale = Self::locale_for(loaded);
        let artifact =
            workshop_rs::emitter::emit(&loaded.program, self.catalog, &locale).map_err(|e| {
                self.diagnostics.push(Diagnostic::error(
                    "workshop-emission",
                    Stage::Emission,
                    e.to_string(),
                ));
            })?;
        let mut provider = self
            .language_provider(opy_provider::OPY_LANGUAGE_ID)
            .map_err(|e| {
                self.diagnostics.push(provider_error_diagnostic(e, "start"));
            })?;
        provider
            .initialize(Some(&wright_lpp::ClientInfo {
                name: wright_lpp::LPP_CLIENT_NAME.to_string(),
                version: crate::result::DRIVER_VERSION.to_string(),
            }))
            .map_err(|e| {
                self.diagnostics
                    .push(provider_error_diagnostic(e, "initialize"));
            })?;
        let result = provider
            .reconstruct(&wright_lpp::WorkshopArtifact {
                format: "workshop-rs/text-v1".to_string(),
                content: artifact,
            })
            .map_err(|e| {
                self.diagnostics
                    .push(provider_error_diagnostic(e, "reconstruct"));
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

    pub(crate) fn shared_semantic(&mut self, loaded: &Loaded) -> Arc<SemanticService<'static>> {
        if self
            .semantic
            .as_ref()
            .is_none_or(|cached| !Arc::ptr_eq(&cached.program_key, &loaded.program))
        {
            self.semantic_builds += 1;
            self.semantic = Some(SessionSemantic {
                program_key: Arc::clone(&loaded.program),
                service: Arc::new(self.shared_service_with(loaded, LintConfig::default())),
            });
        }
        Arc::clone(
            &self
                .semantic
                .as_ref()
                .expect("semantic is populated")
                .service,
        )
    }

    /// How many full semantic services this session has built (#513):
    /// repeated semantic workflows over an unchanged loaded snapshot share
    /// one build — a stable count across requests proves reuse, and each
    /// reload adds exactly one.
    pub fn semantic_build_count(&self) -> usize {
        self.semantic_builds
    }

    #[hotpath::measure]
    fn shared_service_with(&self, loaded: &Loaded, config: LintConfig) -> SemanticService<'static> {
        SemanticService::with_shared_program(
            Arc::clone(&loaded.program),
            loaded_origin(loaded),
            config,
            Arc::clone(&self.lint_registry),
        )
    }

    #[allow(unsafe_code)] // hotpath 0.28 emits `#[unsafe(export_name)]` statics
    fn attach_target_limits(&mut self, loaded: &Loaded) {
        let report = match hotpath::measure_block!("compile::element_count", {
            loaded.program.element_count(self.catalog)
        }) {
            Ok(report) => report,
            Err(error) => {
                self.diagnostics.push(Diagnostic {
                    code: "element-count-unavailable".to_string(),
                    stage: Stage::Emission,
                    severity: Severity::Info,
                    message: format!(
                        "the canonical element count is unavailable ({}); the {WORKSHOP_CLIENT_ELEMENT_LIMIT}-element client import limit was not evaluated",
                        error
                    ),
                    status: None,
                    span: None,
                    source: Some(loaded_origin(loaded)),
                    fields: Default::default(),
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
            fields: Default::default(),
        };
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
                computed = loaded.program.semantic_issues(self.catalog);
                &computed
            }
        };
        for issue in residuals {
            self.diagnostics
                .push(map_issue(issue, path, &loaded.origin));
        }
    }

    pub(crate) fn lint_registry(&self) -> &LintRegistry {
        &self.lint_registry
    }

    fn finish<T: serde::Serialize>(&mut self, command: &str, result: T) -> Envelope<T> {
        let diagnostics = std::mem::take(&mut self.diagnostics);
        let (diagnostics, selection, exit) =
            crate::select::envelope_diagnostics(&self.config, self.loaded.as_ref(), diagnostics);
        Envelope {
            wright: version_info(),
            command: command.to_string(),
            ok: exit == crate::result::exit::SUCCESS,
            exit,
            diagnostics,
            selection,
            gated: false,
            result,
            comparison: None,
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
        let result = match self.verify_fixed_config().and_then(|()| load(self)) {
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
                format!("Workshop transformation failed: {}", error),
            )
        })?;
    }
    Ok(())
}

fn root_relative(path: Option<&Path>, root: &Path) -> Option<String> {
    let path = path?;
    let abs_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let canonical_root = match root.canonicalize() {
        Ok(root) => root,
        Err(_) if root.as_os_str().is_empty() => std::env::current_dir().ok()?,
        Err(_) => root.to_path_buf(),
    };
    match abs_path.strip_prefix(&canonical_root) {
        Ok(relative) if !relative.as_os_str().is_empty() => Some(relative.display().to_string()),
        _ => None,
    }
}

fn map_issue(
    issue: &workshop_rs::rules::SemanticIssue,
    path: &Path,
    origin: &Origin,
) -> Diagnostic {
    let (kind_code, severity) = match issue.kind {
        workshop_rs::rules::IncompletenessKind::RawSetting => ("raw-setting", Severity::Warning),
        workshop_rs::rules::IncompletenessKind::UnknownAction => {
            ("unknown-action", Severity::Error)
        }
        workshop_rs::rules::IncompletenessKind::UnknownValue => ("unknown-value", Severity::Error),
        workshop_rs::rules::IncompletenessKind::OpaqueAction => ("opaque-action", Severity::Error),
    };
    let status = status_for_classification(issue.classification);
    let mut message = format!(
        "Workshop construct '{}' is {} ({})",
        issue.name,
        status_name(status),
        issue.classification.as_str()
    );
    if let Some(spelling) = &issue.suggestion {
        message.push_str(&format!(" (did you mean '{}'?)", spelling));
    }
    Diagnostic {
        code: diagnostic_code(kind_code, &issue.name),
        stage: Stage::Analysis,
        severity,
        message,
        status: Some(status),
        span: Some(issue_span(issue.span, path)),
        source: Some(origin.clone()),
        fields: Default::default(),
    }
}

fn status_for_classification(classification: workshop_rs::rules::ResidualClassification) -> Status {
    match classification {
        workshop_rs::rules::ResidualClassification::ProjectDefinedConstruct
        | workshop_rs::rules::ResidualClassification::SourceDeclaredVariable => Status::Partial,
        workshop_rs::rules::ResidualClassification::ProducerExtension
        | workshop_rs::rules::ResidualClassification::LegacyOpaque
        | workshop_rs::rules::ResidualClassification::UnresolvedIdentifier
        | workshop_rs::rules::ResidualClassification::CatalogSpellingNearMiss => {
            Status::Unsupported
        }
    }
}

fn diagnostic_code(kind: &str, identity: &str) -> String {
    format!(
        "workshop.{kind}.{}",
        identity
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect::<String>()
    )
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Supported => "supported",
        Status::Partial => "partially supported",
        Status::Unsupported => "unsupported",
    }
}

fn issue_span(span: Option<workshop_rs::source::Span>, path: &Path) -> SourceSpan {
    let (start_line, start_col, end_line, end_col) = span
        .map(|span| (span.start.line, span.start.col, span.end.line, span.end.col))
        .unwrap_or((1, 1, 1, 1));
    SourceSpan {
        file: 0,
        path: path.display().to_string(),
        start: Position {
            line: start_line,
            col: start_col,
        },
        end: Position {
            line: end_line,
            col: end_col,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn mutual_protocol_version_honors_the_target_floor() {
        let supported =
            |versions: &[&str]| versions.iter().map(|v| v.to_string()).collect::<Vec<_>>();

        assert_eq!(
            mutual_protocol_version(&supported(&["1.0", "1.1", "1.3"]), InputTarget::File),
            Some("1.3")
        );
        assert_eq!(
            mutual_protocol_version(&supported(&["1.1", "1.4"]), InputTarget::File),
            Some("1.4")
        );

        assert_eq!(
            mutual_protocol_version(&supported(&["1.0"]), InputTarget::File),
            None
        );
        assert_eq!(
            mutual_protocol_version(&supported(&["1.0", "1.1"]), InputTarget::File),
            Some("1.1")
        );

        assert_eq!(
            mutual_protocol_version(&supported(&["1.1", "1.2"]), InputTarget::Directory),
            None
        );
        assert_eq!(
            mutual_protocol_version(&supported(&["1.2", "1.3"]), InputTarget::Directory),
            Some("1.3")
        );

        assert_eq!(
            mutual_protocol_version(&supported(&["1.6"]), InputTarget::File),
            None
        );
    }

    #[test]
    fn residual_classifications_map_fail_closed() {
        use workshop_rs::rules::ResidualClassification as C;
        for (classification, expected) in [
            (C::ProjectDefinedConstruct, Status::Partial),
            (C::SourceDeclaredVariable, Status::Partial),
            (C::ProducerExtension, Status::Unsupported),
            (C::LegacyOpaque, Status::Unsupported),
            (C::UnresolvedIdentifier, Status::Unsupported),
            (C::CatalogSpellingNearMiss, Status::Unsupported),
        ] {
            assert_eq!(status_for_classification(classification), expected);
        }
    }
}
