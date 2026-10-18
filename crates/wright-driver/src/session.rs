//! Frontends are selected by [`SourceKind`] behind one contract, so
//! the provider-backed `.opy` workflow can replace the retired adapter bridge
//! without changing callers. Every workflow returns a typed [`Envelope`]
//! whose JSON serialization is the machine-readable CLI contract.

mod edit;
mod semantic;

pub(crate) use semantic::{prepare_findings, resolve_span_paths};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use workshop_rs::Program;
use wright_analyzer::canonical::SemanticService;
use wright_analyzer::registry::{LintConfig, LintRegistry};

use crate::cache;
use crate::config::{InputSpec, SessionConfig, SourceKind};
use crate::diag::{
    Diagnostic, Origin, Position, Severity, SourceSpan, Stage, source_provider_unavailable,
    source_span,
};
use crate::input::{self, InputTarget, ResolvedInput};
use crate::input_identity;
use crate::opy_provider;
use crate::output::{CompileWrite, OutputDest, Overwrite};
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
    /// Diagnostics the load itself produced — a provider's own diagnostics
    /// and source-map warnings — replayed whenever this cached snapshot
    /// serves a request so a repeated `check`/`compile` reports the same
    /// provider diagnostics as the first (#638).
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
pub(crate) const WORKSHOP_CLIENT_ELEMENT_LIMIT: usize = 32768;

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
    /// source compatibility within `wright-embedding/v1`, but
    /// post-construction mutation is not a supported capability — the next
    /// workflow or `ToolService` construction refuses with
    /// `session-config-changed` rather than serving state derived from the
    /// earlier configuration. Changing configuration means constructing a
    /// new session.
    pub config: SessionConfig,
    /// The construction-time snapshot `config` is verified against before
    /// every workflow (#511).
    fixed_config: SessionConfig,
    catalog: Arc<workshop_rs::catalog::Catalog>,
    lint_registry: Arc<LintRegistry>,
    loaded: Option<Loaded>,
    loaded_operation: Option<ProviderOperation>,
    /// The semantic service shared by every semantic workflow over the
    /// current `loaded` snapshot (#513).
    semantic: Option<SessionSemantic>,
    /// Full semantic-service constructions — the observable hook proving an
    /// unchanged loaded snapshot is not rebuilt (#513).
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
        // A `--disable-rule`, `--rule-severity`, or lint-config `rules:` key
        // names an effective-policy id: an unknown id is a usage error, not
        // a silent no-op that leaves the intended rule enabled (#640).
        if let Some(rule_id) = config
            .lint
            .rules
            .keys()
            .filter(|id| !lint_registry.contains(id))
            .min()
        {
            return Err(Diagnostic::error(
                "invalid-selection",
                Stage::Discovery,
                format!("unknown rule id '{rule_id}'"),
            ));
        }
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

    /// Refuse `session-config-changed` when `config` diverged from the
    /// construction-time snapshot (#511): nothing derived from the earlier
    /// configuration may serve the next result.
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
    ///
    /// On the provider backend this is the canonical-snapshot read, not a
    /// workflow (#591): it serves the snapshot a compile-producing operation
    /// already established and refuses when the session holds none — the
    /// provider declined compile or has not been asked to compile. Workflows
    /// that may compile (check, compile, lint, analyze, inspect, and the
    /// `inspect` queries) call [`Self::load_with_operation`] instead, so the
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

    /// Drop the cached program and resolve the input from disk again (#471):
    /// `ToolService` calls this when the input's disk fingerprint changed, so
    /// a long-lived session reflects the current project. A failed reload
    /// leaves the cache empty — the next `load` retries the same resolution
    /// rather than resurrecting the stale program.
    pub(crate) fn reload(&mut self) -> Result<Loaded, Diagnostic> {
        self.drop_loaded();
        self.load()
    }

    /// Drop the cached program without reloading it (#591): `ToolService`
    /// calls this when the disk fingerprint changed ahead of a `check` or
    /// `compile` workflow, whose own operation-aware load must re-resolve
    /// rather than serve the stale snapshot — including a provider-backed
    /// `check`, which compiles to produce its canonical snapshot (#617).
    /// `loaded_operation` survives: it records the provider operation that
    /// last produced a canonical snapshot, so a compile-capable provider
    /// still recompiles on the next `load` where a check-only one refuses.
    pub(crate) fn drop_loaded(&mut self) {
        self.loaded = None;
        self.semantic = None;
    }

    /// The session's cached canonical snapshot, when the last load produced
    /// one — `None` for a check-only provider `check`, an unloadable input,
    /// or after [`Self::drop_loaded`]. `ToolService` adopts this snapshot
    /// after a successful workflow load (#591); it cannot call `load()` for
    /// it because a check-only provider backend refuses `load()` outright.
    pub(crate) fn cached_loaded(&self) -> Option<Loaded> {
        self.loaded.clone()
    }

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
                // The cached snapshot still reports the diagnostics its
                // own load produced — a repeated `check`/`compile` must not
                // silently return an empty provider-diagnostic set (#638).
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
        // ADR-0028: profiles apply to raw Workshop input only. With an
        // explicitly non-Workshop `--kind` the serve surface already rejects
        // the combination at startup; here the kind may have been resolved
        // from the input, so an OPY/OSTW source with a profile is rejected
        // as a usage error before any load work runs.
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
            // A caller-held text is not a provider input: the provider would
            // resolve the entry from disk and answer for different bytes.
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
        // Diagnostics appended from here belong to the loaded snapshot:
        // they are captured onto `Loaded` so a later cache hit replays the
        // same provider diagnostics (#638).
        let diagnostics_start = self.diagnostics.len();
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
            // LPP 1.4 lets the provider return a source-mapped artifact. A
            // provider without it keeps the unmapped LPP 1.1/1.2 session on a
            // restarted process, as the protocol requires after a version
            // mismatch.
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
                // A compile-capable provider also produces the canonical
                // artifact, so `check` runs the same canonical validation
                // `compile` surfaces — an agent that gates on `check` must see
                // every error `compile` would report (#617). A provider
                // without compile capability keeps the check-only path.
                match provider.compile(&target) {
                    Ok(mut compilation) => {
                        for diagnostic in check.diagnostics {
                            if !compilation.diagnostics.contains(&diagnostic) {
                                compilation.diagnostics.push(diagnostic);
                            }
                        }
                        compilation
                    }
                    // A provider without the compile capability keeps the
                    // check-only path: `Unsupported` covers non-LPP
                    // providers, while an LPP provider declines through
                    // `capability-unavailable` (#638).
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
        // A canonical artifact needs the provider's source identity for a
        // directory target — whatever operation produced it, including a
        // `check` that compiled to validate (#617).
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
        // The check-only provider path: `provider.check` answered and the
        // provider declined to compile, so no canonical artifact exists to
        // validate. `check` then reports only the provider's own diagnostics —
        // the documented difference from `compile` (#617).
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
                    // Retain each mapped member's authored text so source
                    // extents (lint fixes, previews) derive against what was
                    // actually written — the map alone carries only paths
                    // (#583). Relative member spellings resolve against the
                    // session's project root, not the process working
                    // directory.
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
                            Err(error) => unreadable.push(format!("{member} ({error})")),
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
        // Provider artifacts are OPY/OSTW output; profiles apply to raw
        // Workshop input only (ADR-0028), so no pass runs here — and the
        // session rejects `--profile` with a non-Workshop input upstream.
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
        // Reaching here means a canonical artifact was parsed and validated;
        // the snapshot serves every later operation — including a `check`
        // that compiled to validate (#617) — as a compile product.
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
    pub fn language_provider(
        &self,
        id: &str,
    ) -> Result<Box<dyn wright_lpp::LanguageProvider>, wright_lpp::ProviderError> {
        // Provider workflows consume `config.providers`/`config.opy_provider`
        // live; a changed configuration refuses through the provider refusal
        // channel carrying `session-config-changed` (#511).
        if let Err(diagnostic) = self.verify_fixed_config() {
            return Err(wright_lpp::ProviderError::lpp(
                wright_lpp::LppErrorKind::Refusal,
                serde_json::json!({ "refusalCode": diagnostic.code }),
                diagnostic.message,
            ));
        }
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

    /// The element-cost snapshot of the currently loaded program, for the
    /// project run cache (ADR-0027, #723): the canonical total plus each
    /// rule's share, in canonical order. `None` when the count cannot run.
    pub(crate) fn cost_snapshot(&self, loaded: &Loaded) -> Option<cache::CostSnapshot> {
        let report = loaded.program.element_count(&self.catalog).ok()?;
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

    /// The `CostRun` key material for the loaded input: the resolved input
    /// file, kind, locale, and profile — so runs differing only in those
    /// never compare with each other (ADR-0027 decision 12).
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

    /// Compare the current run's element cost against the stored reference
    /// and record it as the new previous run (plus the named baseline when
    /// `--save-baseline` is given). Returns the comparison when one was
    /// available; every cache failure is swallowed (ADR-0027 decision 10).
    /// An explicitly named but absent baseline is a usage error instead.
    pub(crate) fn run_cost_comparison(&self) -> Option<cache::Comparison> {
        let loaded = self.loaded.as_ref()?;
        let run = self.cost_run(loaded);
        if run.baseline.is_some() && !cache::baseline_exists(&run) {
            return None;
        }
        let snapshot = self.cost_snapshot(loaded)?;
        cache::compare_and_record(&run, &snapshot)
    }

    /// Whether the explicitly named `--baseline` exists — `compile` turns a
    /// missing one into a usage error like `inspect cost` does (#723).
    pub(crate) fn named_baseline_missing(&self) -> Option<String> {
        let run = self.cost_run(self.loaded.as_ref()?);
        let name = run.baseline.as_deref()?;
        (!cache::baseline_exists(&run)).then(|| name.to_string())
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
        hotpath::measure_block!(
            "load::validate_canonical",
            workshop_rs::validate::validate_canonical_ids_tolerating_residuals(
                &program,
                &self.catalog
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
                // A successful run becomes the new reference; the change
                // against the previous run rides the envelope (in-process
                // only — never the default JSON, ADR-0027 decision 3).
                missing_baseline = self.named_baseline_missing();
                if missing_baseline.is_none() {
                    comparison = self.run_cost_comparison();
                }
            }
            Err(diagnostic) => self.diagnostics.push(diagnostic),
        }
        let mut envelope = self.finish("compile", result);
        envelope.comparison = comparison.map(|c| c.to_json());
        // An explicitly named baseline that does not exist is a usage
        // error, reported like the gate diagnostics — but it must not
        // change the compiled bytes, so it lands after the write above.
        if let Some(name) = missing_baseline {
            envelope.diagnostics.push(Diagnostic::error(
                "invalid-selection",
                Stage::Emission,
                format!("unknown baseline '{name}': no record under .wright/ for this input kind/locale/profile"),
            ));
            envelope.ok = false;
            envelope.exit = crate::result::exit::SOURCE_ERROR;
        }
        envelope
    }

    fn compile_output(
        &mut self,
        to: Option<&str>,
    ) -> Result<(CompiledOutput, Option<PathBuf>), Diagnostic> {
        let loaded = self.load_with_operation(ProviderOperation::Compile)?;
        let locale = match to {
            Some(name) => {
                // An output locale the catalog does not declare is a usage
                // error (#759): the emitter's missing-mapping contract
                // covers undeclared spellings per identity, not an unknown
                // locale itself.
                let locale = workshop_rs::catalog::Locale::new(name);
                if !self.catalog.supports(&locale) {
                    return Err(Diagnostic::error(
                        "invalid-locale",
                        Stage::Discovery,
                        format!(
                            "unknown output locale '{name}': declared locales are {}",
                            self.catalog
                                .locales()
                                .iter()
                                .map(|locale| locale.to_string())
                                .collect::<Vec<_>>()
                                .join(", "),
                        ),
                    ));
                }
                locale
            }
            None => Self::locale_for(&loaded),
        };
        self.progress(ProgressEvent::new(ProgressPhase::Emission));
        let text = hotpath::measure_block!("compile::emit", {
            workshop_rs::emitter::emit(&loaded.program, &self.catalog, &locale)
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

    /// The semantic service over `loaded`, shared by every semantic workflow
    /// serving that snapshot (#513): the index and default lint pass are
    /// built once per loaded program rather than per request. Reuse is
    /// keyed on the program's `Arc` identity — a reload or a provider
    /// re-resolution produces a fresh `Arc`, which rebuilds instead of
    /// serving a stale index. Configured consumers derive their view through
    /// [`SemanticService::with_lint_config`], which shares the same index.
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
                format!("Workshop transformation failed: {error}"),
            )
        })?;
    }
    Ok(())
}

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
        workshop_rs::WorkshopError::Unknown { kind, span, .. }
        | workshop_rs::WorkshopError::UnknownWithCandidates { kind, span, .. } => {
            (format!("unknown-{kind}"), Stage::Frontend, to_span(*span))
        }
        workshop_rs::WorkshopError::NotDetected { kind, span, .. } => {
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

/// One provider failure on the `convert` reconstruction path (#569): the
/// same classification the source-provider seam applies, with the failed
/// operation named in the human message. The refusal code survives in the
/// diagnostic code (`provider-refusal-<refusalCode>`) so consumers can
/// classify it without parsing the message.
fn provider_error_diagnostic(error: wright_lpp::ProviderError, operation: &str) -> Diagnostic {
    Diagnostic::error(
        crate::source_provider::provider_failure_code(&error),
        crate::source_provider::provider_failure_stage(&error),
        format!("the provider could not {operation} the artifact: {error}"),
    )
}

/// The highest LPP version both sides implement, honoring the target's
/// negotiation floor (LPP §19.2, #638): entry requests cannot drop below
/// 1.1 project loading, and a directory target cannot drop below the 1.3
/// `sourceIdentity` contract its canonical load depends on.
fn mutual_protocol_version(supported: &[String], target: InputTarget) -> Option<&'static str> {
    let floor = match target {
        InputTarget::Directory => wright_lpp::LPP_SOURCE_IDENTITY_PROTOCOL_VERSION,
        _ => wright_lpp::LPP_PROJECT_LOADING_PROTOCOL_VERSION,
    };
    let floor = protocol_version_tuple(floor)?;
    wright_lpp::LPP_SUPPORTED_PROTOCOL_VERSIONS
        .iter()
        .copied()
        .filter_map(|version| protocol_version_tuple(version).map(|tuple| (tuple, version)))
        .filter(|(tuple, _)| *tuple >= floor)
        .filter(|(_, version)| supported.iter().any(|s| s == version))
        .max_by_key(|(tuple, _)| *tuple)
        .map(|(_, version)| version)
}

fn protocol_version_tuple(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #638: the highest-mutual retry applies the target's negotiation
    /// floor — entry requests cannot drop below LPP 1.1 project loading,
    /// and a directory target cannot drop below the 1.3 source-identity
    /// contract.
    #[test]
    fn mutual_protocol_version_honors_the_target_floor() {
        let supported =
            |versions: &[&str]| versions.iter().map(|v| v.to_string()).collect::<Vec<_>>();

        // Highest mutual wins over the nearest advertised version.
        assert_eq!(
            mutual_protocol_version(&supported(&["1.0", "1.1", "1.3"]), InputTarget::File),
            Some("1.3")
        );
        assert_eq!(
            mutual_protocol_version(&supported(&["1.1", "1.4"]), InputTarget::File),
            Some("1.4")
        );

        // A file entry cannot negotiate below the 1.1 project-loading
        // contract its `check_entry`/`compile_entry` calls require.
        assert_eq!(
            mutual_protocol_version(&supported(&["1.0"]), InputTarget::File),
            None
        );
        assert_eq!(
            mutual_protocol_version(&supported(&["1.0", "1.1"]), InputTarget::File),
            Some("1.1")
        );

        // A directory target cannot negotiate below the 1.3
        // source-identity contract.
        assert_eq!(
            mutual_protocol_version(&supported(&["1.1", "1.2"]), InputTarget::Directory),
            None
        );
        assert_eq!(
            mutual_protocol_version(&supported(&["1.2", "1.3"]), InputTarget::Directory),
            Some("1.3")
        );

        // LPP 1.6 (`resolveIds`) parses but is not negotiated: the client
        // has no `lpp/resolveIds` implementation.
        assert_eq!(
            mutual_protocol_version(&supported(&["1.6"]), InputTarget::File),
            None
        );
    }
}
