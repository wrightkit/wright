//! [`ToolService`] exposes Wright's compile/check/analyze/query workflows and
//! agent-oriented semantic queries over stable public contracts, reusing the
//! driver session. It is transport-neutral: the `serve` transport adapters
//! (#60) and the embedding API are thin mappings over the same operations,
//! so behavior is testable in-process without a transport.
//!
//! Capability/version negotiation is provided by [`Capabilities`]; cost and
//! resource inspection ([`ToolRequest::CostEstimate`]) consumes the
//! Wright-owned generated-resource semantics established by the `wright-bench`
//! harness (emitted bytes, canonical program counts, action/rule counts) and
//! distinguishes exact counts from static findings.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::diag::Diagnostic;
use crate::input;
use crate::result::{AnalyzeResult, CheckResult, CompileResult, Envelope, InspectResult};
use crate::{CompilerSession, Loaded, RESULT_CONTRACT};
use wright_analyzer::canonical::{SemanticIndex, SemanticService};
/// A structured tool error.
pub use wright_analyzer::service::ErrorInfo as ToolErrorInfo;
/// A tool response: a structured owned result or a structured error.
pub use wright_analyzer::service::Response as ToolResponse;
use wright_analyzer::service::{Request, Response};

/// The tool-service name and version.
pub const SERVICE_NAME: &str = "wright-tool-service";
pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const AGENT_CONTRACT: &str = "wright-agent/v1";

/// A semantic query target on the agent contract (#429): a numeric id or a
/// declared name.
///
/// Numeric ids keep their established meaning — the semantic index's symbol
/// id for `references`/`usage`, the rule index for `cfg` — so existing
/// numeric requests are unchanged. A name resolves against the loaded
/// program's semantic index: unmatched or ambiguous names produce a
/// structured error, never a guess.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Address {
    /// The numeric id assigned by the loaded program's semantic addressing.
    Id(u32),
    /// The declared name (`rule("name")`, variable, or subroutine name).
    Name(String),
}

impl From<u32> for Address {
    fn from(id: u32) -> Self {
        Self::Id(id)
    }
}

impl From<String> for Address {
    fn from(name: String) -> Self {
        Self::Name(name)
    }
}

impl From<&str> for Address {
    fn from(name: &str) -> Self {
        Self::Name(name.to_string())
    }
}

/// A tool request: the owned query surface plus agent-oriented operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum ToolRequest {
    /// Service identity, contract, and supported operations.
    Capabilities,
    /// Compile the loaded project.
    Compile,
    /// Check the loaded project.
    Check,
    /// Analyze the loaded project.
    Analyze,
    /// Inspect the loaded project.
    Inspect,
    /// The loaded canonical program summary (origin, files, counts, findings).
    Project,
    /// Every rule.
    Rules,
    /// Symbols, optionally filtered by kind.
    Symbols {
        #[serde(default)]
        kind: Option<String>,
    },
    /// References to a symbol, addressed by its numeric id or its name (#429).
    References { symbol: Address },
    /// Usage counts for a symbol, addressed by its numeric id or its name
    /// (#429).
    Usage { symbol: Address },
    /// The control-flow graph of one rule, addressed by its numeric index or
    /// its name (#429).
    Cfg { rule: Address },
    /// Every static-analysis finding, optionally narrowed by an inline
    /// selection (`severity`, `rule`, `file`, `max`; #430).
    Findings(crate::select::FindingSelection),
    /// Persistent Workshop object facts, separate from lint diagnostics.
    PersistentObjects,
    /// Lint findings plus per-rule id/effective severity and the effective
    /// configuration (#98); `lintRules` serves full rule metadata (#431).
    /// Optionally narrowed by an inline selection (#430).
    Lint(crate::select::FindingSelection),
    /// The registered lint rules with full metadata and the effective lint
    /// configuration.
    LintRules,
    /// The subroutine call graph (caller rules → callee subroutines).
    CallGraph,
    /// Generated-resource cost estimates (exact counts + findings),
    /// optionally narrowed by an inline selection (#430).
    CostEstimate(crate::select::FindingSelection),
    /// Target/catalog metadata (actions, values, events, enum domains).
    TargetMetadata,
    /// Validate and preview a caller-supplied source-edit transaction
    /// against the session's project (#130): atomic all-or-nothing
    /// semantics, structured refusal diagnostics, no filesystem writes.
    /// Raw Workshop input validates by reparsing the edited sources through
    /// `workshop-rs` (#434); source languages route to
    /// `providerValidateEdit`.
    #[serde(rename = "validateEditTransaction")]
    ValidateEdit {
        /// The current text of every source the transaction touches, keyed
        /// by the same source identities the edits carry. Optional (#472):
        /// when absent the service reads the current text of the files the
        /// transaction names from disk; when present it stays the
        /// caller-side precondition text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sources: Option<std::collections::BTreeMap<String, String>>,
        transaction: crate::edit::EditTransaction,
    },
    /// Request a semantic rename through the shared refactoring contract
    /// (#129/#130/#434): returns the validated exact-range transaction or
    /// structured refusal diagnostics. On raw Workshop input the target is
    /// a `symbol` (numeric id or declared name, as `references`/`usage`
    /// address them) or a `source`/`line`/`col` position, and occurrences
    /// rewrite through `workshop-rs` identifier provenance. Source
    /// languages route to `providerSemanticRename`. Wright
    /// proposes/validates; applying edits to disk is an explicit consumer
    /// responsibility.
    SemanticRename {
        /// The current text of every source the rename may edit, keyed by
        /// the same source identities the target names. Optional (#472):
        /// when absent the service reads the loaded input's current text
        /// from disk; when present it stays the caller-side precondition
        /// text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sources: Option<std::collections::BTreeMap<String, String>>,
        target: crate::edit::RenameTarget,
    },
    /// Provider-driven semantic rename (#139): rename target resolution and
    /// edit generation route through the LPP `rename` capability of the
    /// provider configured for `language_id`; the resulting source edits are
    /// wrapped in Wright's own transaction (identity/version preconditions,
    /// deterministic ordering, overlap checks, atomic preview) and validated
    /// through the provider's project semantics (`lpp/validateEdits` per
    /// edited document, then `lpp/check` over the edited project) before
    /// success. Provider refusals, unsupported capabilities, stale sources,
    /// and semantic validation failures are structured refusals with no
    /// partial edit set; there is no fallback to textual search/replace.
    #[serde(rename = "providerSemanticRename")]
    ProviderSemanticRename {
        /// The opaque language id of the configured provider.
        language_id: String,
        /// The document set the rename is computed against (the provider's
        /// view: text, language id, version).
        documents: wright_lpp::DocumentSet,
        /// The URI (a key of `documents`) in which `position` is
        /// interpreted.
        position_document_uri: String,
        /// The position of the symbol to rename (0-based LSP conventions).
        position: wright_lpp::Position,
        /// The new name; the provider validates it against the language's
        /// identifier rules.
        new_name: String,
        /// The project the documents belong to (informational).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project_root: Option<String>,
        /// The caller's current text for every source the rename may edit,
        /// keyed by document URI (the identity/version precondition view).
        sources: std::collections::BTreeMap<String, String>,
    },
    /// Provider-driven edit validation (#139): validate a caller-proposed
    /// source-edit transaction against the provider's project semantics
    /// (`lpp/validateEdits` per edited document, then `lpp/check` over the
    /// edited project) before any application. The transaction must carry
    /// document URIs as source identities and the identity of the text the
    /// edits were computed against; stale, malformed, or semantically
    /// invalid transactions refuse with no partial edit set.
    #[serde(rename = "providerValidateEdit")]
    ProviderValidateEdit {
        /// The opaque language id of the configured provider.
        language_id: String,
        /// The unmodified project as the provider sees it.
        documents: wright_lpp::DocumentSet,
        /// The caller-proposed transaction (Wright-owned edit contract).
        transaction: crate::edit::EditTransaction,
        /// The caller's current text for every edited source, keyed by
        /// document URI (the identity/version precondition view).
        sources: std::collections::BTreeMap<String, String>,
        /// The project the documents belong to (informational).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project_root: Option<String>,
    },
}

/// The capability/version contract of the service.
#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub name: String,
    pub version: String,
    pub contract: String,
    pub agent_contract: String,
    pub operations: Vec<String>,
    pub languages: Vec<String>,
    pub profiles: Vec<String>,
}

/// The session-aware tool service.
pub struct ToolService<'a> {
    session: &'a mut CompilerSession,
    /// The current program snapshot, when one exists. The service may exist
    /// without one (#512): a configured project that cannot load at
    /// construction is deferred to the first program-reading request rather
    /// than failing the service, so `None` is the unloaded state — never an
    /// empty placeholder program.
    loaded: Option<Loaded>,
    /// The shared semantic service over `loaded` — `Some` exactly when a
    /// snapshot is held.
    semantic: Option<Arc<SemanticService<'static>>>,
    lint_semantic: Option<SemanticService<'static>>,
    /// The disk state `loaded` was read from, recomputed before every
    /// program-reading request so a long-lived session observes edits, file
    /// additions, and removals (#471). `None` while no snapshot is held.
    fingerprint: Option<input::DiskFingerprint>,
    /// Whether the symbol ids a client holds were issued by the current
    /// program (#471): a reload clears it until a `symbols` listing or an
    /// `ambiguous-symbol` refusal names the current space.
    symbol_ids_current: bool,
    /// The rule-index counterpart of `symbol_ids_current`, re-established by
    /// `rules` and `ambiguous-rule` (#471).
    rule_ids_current: bool,
    /// How many times an observed disk change reloaded the program — the
    /// observable hook for "unchanged inputs are not reloaded" (#471).
    reloads: usize,
}

impl<'a> ToolService<'a> {
    /// Build the service over a session.
    ///
    /// A session whose configuration changed after construction is refused
    /// with `session-config-changed` (#511): the service never adopts state
    /// derived from a superseded configuration.
    ///
    /// The configured project is loaded eagerly when it can be, but a load
    /// failure does not fail construction (#512): the service exists without
    /// a program snapshot, project-independent operations (`capabilities`,
    /// `targetMetadata`, the `provider*` mutations) stay available, and each
    /// program-reading request retries the load — surfacing the loader's
    /// structured diagnostic until the project heals.
    #[hotpath::measure]
    pub fn new(session: &'a mut CompilerSession) -> Result<ToolService<'a>, Diagnostic> {
        session.verify_fixed_config()?;
        let mut service = ToolService {
            session,
            loaded: None,
            semantic: None,
            lint_semantic: None,
            fingerprint: None,
            symbol_ids_current: true,
            rule_ids_current: true,
            reloads: 0,
        };
        if let Ok(loaded) = service.session.load() {
            service.adopt(loaded);
        }
        Ok(service)
    }

    /// Reloads caused by observed disk changes (#471). An input whose
    /// fingerprint has not changed is never reloaded, so a stable value
    /// across requests proves the session kept serving the same snapshot.
    /// The deferred initial load (#512) is not a reload.
    pub fn reload_count(&self) -> usize {
        self.reloads
    }

    /// The current program snapshot (origin, input identity, canonical
    /// program), or `None` while the configured project cannot load (#512).
    /// The first successful program-reading request populates it.
    pub fn loaded(&self) -> Option<&Loaded> {
        self.loaded.as_ref()
    }

    /// Full semantic-service builds behind this service's session (#513) —
    /// the observable hook proving repeated requests share the loaded
    /// snapshot's index rather than rebuilding it.
    pub fn semantic_build_count(&self) -> usize {
        self.session.semantic_build_count()
    }

    /// The capability/version contract.
    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            name: SERVICE_NAME.to_string(),
            version: SERVICE_VERSION.to_string(),
            contract: RESULT_CONTRACT.to_string(),
            agent_contract: AGENT_CONTRACT.to_string(),
            operations: vec![
                "capabilities",
                "project",
                "rules",
                "symbols",
                "references",
                "usage",
                "cfg",
                "findings",
                "persistentObjects",
                "lint",
                "lintRules",
                "callGraph",
                "costEstimate",
                "targetMetadata",
                "compile",
                "check",
                "analyze",
                "inspect",
                "validateEditTransaction",
                "semanticRename",
                "providerSemanticRename",
                "providerValidateEdit",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            languages: vec!["opy".to_string(), "workshop".to_string()],
            profiles: vec![
                crate::Profile::Off.as_str().to_string(),
                crate::Profile::Compat.as_str().to_string(),
                crate::Profile::Aggressive.as_str().to_string(),
            ],
        }
    }

    /// Handle one tool request, returning a structured owned response.
    ///
    /// A request that reads the loaded program requires a valid current
    /// snapshot at request time (#471, #512): the deferred initial load runs
    /// here when the service was built over an unloadable project, and a
    /// changed input fingerprint reloads the session, so a long-lived
    /// session serves the project as it exists now. Either failure is the
    /// request's structured refusal — the previous snapshot is never served
    /// silently. `capabilities` reports service metadata, `targetMetadata`
    /// the static catalog, and `provider*` operations carry their own
    /// documents, so none of them consult or trigger the project load.
    pub fn handle(&mut self, request: &ToolRequest) -> ToolResponse {
        if Self::reads_program(request) {
            if let Err(error) = self.refresh() {
                return ToolResponse::Error { error };
            }
            if let Some(error) = self.stale_id(request) {
                return ToolResponse::Error { error };
            }
        }
        let response = hotpath::measure_block!(request_label(request), self.dispatch(request));
        self.note_id_space(request, &response);
        response
    }

    /// Whether the request consults the loaded program (#471).
    fn reads_program(request: &ToolRequest) -> bool {
        !matches!(
            request,
            ToolRequest::Capabilities
                | ToolRequest::TargetMetadata
                | ToolRequest::ProviderSemanticRename { .. }
                | ToolRequest::ProviderValidateEdit { .. }
        )
    }

    /// Ensure a valid current snapshot backs a program-reading request
    /// (#471, #512). With no snapshot the request performs the deferred
    /// initial load; with one, a changed disk fingerprint reloads it. A
    /// failed attempt surfaces the loader's structured diagnostic — the
    /// previous snapshot is never served — and leaves the session's cache
    /// empty so the next request retries the same resolution; a repaired
    /// project is adopted without restarting the service.
    fn refresh(&mut self) -> Result<(), ToolErrorInfo> {
        let Some(loaded) = &self.loaded else {
            // The deferred initial load (#512). A success establishes the
            // id space: no earlier program could have issued ids within
            // this service.
            let loaded = self.session.load().map_err(Self::loader_error)?;
            self.adopt(loaded);
            return Ok(());
        };
        let fingerprint = input::disk_fingerprint(&self.session.config, &loaded.input);
        if self.fingerprint.as_ref() == Some(&fingerprint) {
            return Ok(());
        }
        let loaded = self.session.reload().map_err(Self::loader_error)?;
        self.adopt(loaded);
        self.reloads += 1;
        // The reloaded program's numeric ids are a different space: ids
        // issued before the reload refuse `stale-id` until a `symbols` or
        // `rules` listing — or an `ambiguous-*` refusal, which already names
        // the current candidates — re-establishes them.
        self.symbol_ids_current = false;
        self.rule_ids_current = false;
        Ok(())
    }

    /// The loader's diagnostic carried through the request's structured
    /// refusal unchanged.
    fn loader_error(diagnostic: Diagnostic) -> ToolErrorInfo {
        ToolErrorInfo {
            code: diagnostic.code,
            message: diagnostic.message,
        }
    }

    /// Replace the loaded snapshot and rebuild the semantic services over it.
    fn adopt(&mut self, loaded: Loaded) {
        let semantic = self.session.shared_semantic(&loaded);
        self.lint_semantic = (!self.session.config.lint.rules.is_empty())
            .then(|| semantic.with_lint_config(self.session.config.lint.clone()));
        self.semantic = Some(semantic);
        self.fingerprint = Some(input::disk_fingerprint(&self.session.config, &loaded.input));
        self.loaded = Some(loaded);
    }

    /// The current snapshot — present whenever a program-reading request
    /// reaches `dispatch`, because `refresh` succeeded first.
    fn snapshot(&self) -> &Loaded {
        self.loaded
            .as_ref()
            .expect("the refresh gate guarantees a snapshot")
    }

    /// The semantic service over the current snapshot — built in `adopt`.
    fn semantic(&self) -> &SemanticService<'static> {
        self.semantic
            .as_deref()
            .expect("the refresh gate guarantees a snapshot")
    }

    /// Refuse a numeric id that was issued by an earlier program (#471):
    /// `stale-id` until the client observes the current space. Name
    /// addressing resolves against the loaded index and is always fresh.
    fn stale_id(&self, request: &ToolRequest) -> Option<ToolErrorInfo> {
        let stale = match request {
            ToolRequest::References { symbol } | ToolRequest::Usage { symbol } => {
                matches!(symbol, Address::Id(_)) && !self.symbol_ids_current
            }
            ToolRequest::Cfg { rule } => matches!(rule, Address::Id(_)) && !self.rule_ids_current,
            ToolRequest::SemanticRename { target, .. } => {
                matches!(target.symbol, Some(Address::Id(_))) && !self.symbol_ids_current
            }
            _ => false,
        };
        stale.then(|| ToolErrorInfo {
            code: "stale-id".to_string(),
            message: "the loaded project changed on disk; numeric symbol ids and rule indexes issued before the reload are no longer valid — list `symbols` or `rules` again, or address by name".to_string(),
        })
    }

    /// A request that names the current id space re-establishes it (#471): a
    /// successful `symbols`/`rules` listing, or an `ambiguous-*` refusal that
    /// already reports the current candidates. `semanticRename` reports its
    /// `ambiguous-symbol` as a result diagnostic rather than a refusal.
    fn note_id_space(&mut self, request: &ToolRequest, response: &ToolResponse) {
        match response {
            ToolResponse::Ok { result } => match request {
                ToolRequest::Symbols { .. } => self.symbol_ids_current = true,
                ToolRequest::Rules => self.rule_ids_current = true,
                ToolRequest::SemanticRename { .. }
                    if diagnostics_list_code(result, "ambiguous-symbol") =>
                {
                    self.symbol_ids_current = true;
                }
                _ => {}
            },
            ToolResponse::Error { error } => match error.code.as_str() {
                "ambiguous-symbol" => self.symbol_ids_current = true,
                "ambiguous-rule" => self.rule_ids_current = true,
                _ => {}
            },
        }
    }

    fn dispatch(&mut self, request: &ToolRequest) -> ToolResponse {
        match request {
            ToolRequest::Capabilities => ToolResponse::Ok {
                result: serde_json::to_value(self.capabilities()).expect("capabilities serialize"),
            },
            ToolRequest::Compile => {
                let result = serde_json::to_value(self.session.compile())
                    .expect("compile result serializes");
                ToolResponse::Ok { result }
            }
            ToolRequest::Check => {
                let result =
                    serde_json::to_value(self.session.check()).expect("check result serializes");
                ToolResponse::Ok { result }
            }
            ToolRequest::Analyze => {
                let loaded = self.snapshot().clone();
                let semantic = Arc::clone(
                    self.semantic
                        .as_ref()
                        .expect("the refresh gate guarantees a snapshot"),
                );
                let result = serde_json::to_value(self.session.analyze_loaded(loaded, &semantic))
                    .expect("analyze result serializes");
                ToolResponse::Ok { result }
            }
            ToolRequest::Inspect => {
                let loaded = self.snapshot().clone();
                let semantic = Arc::clone(
                    self.semantic
                        .as_ref()
                        .expect("the refresh gate guarantees a snapshot"),
                );
                let result = serde_json::to_value(self.session.inspect_loaded(loaded, &semantic))
                    .expect("inspect result serializes");
                ToolResponse::Ok { result }
            }
            ToolRequest::Project => self.ok(self.project()),
            ToolRequest::Rules => self.semantic_query(Request::ListRules),
            ToolRequest::Symbols { kind } => {
                self.semantic_query_with_resolved_span_paths(Request::ListSymbols {
                    kind: kind.clone(),
                })
            }
            ToolRequest::References { symbol } => match self.symbol_address(symbol) {
                Ok(symbol) => {
                    self.semantic_query_with_resolved_span_paths(Request::FindReferences { symbol })
                }
                Err(error) => ToolResponse::Error { error },
            },
            ToolRequest::Usage { symbol } => match self.symbol_address(symbol) {
                Ok(symbol) => self.semantic_query(Request::GetUsage { symbol }),
                Err(error) => ToolResponse::Error { error },
            },
            ToolRequest::Cfg { rule } => match self.rule_address(rule) {
                Ok(rule) => self.semantic_query(Request::GetCfg { rule }),
                Err(error) => ToolResponse::Error { error },
            },
            ToolRequest::Findings(selection) => self.findings(selection),
            ToolRequest::PersistentObjects => self.persistent_objects(),
            ToolRequest::Lint(selection) => self.lint(selection),
            ToolRequest::LintRules => self.configured_semantic().handle(&Request::LintRules),
            ToolRequest::CallGraph => self.ok(self.call_graph()),
            ToolRequest::CostEstimate(selection) => self.cost_estimate(selection),
            ToolRequest::TargetMetadata => self.ok(self.target_metadata()),
            ToolRequest::ValidateEdit {
                sources,
                transaction,
            } => self.ok(serde_json::to_value(
                self.session
                    .validate_edit_transaction(sources.as_ref(), transaction),
            )
            .expect("serializes")),
            ToolRequest::SemanticRename { sources, target } => {
                let rename = self.session.semantic_rename(sources.as_ref(), target);
                self.ok(serde_json::to_value(rename).expect("serializes"))
            }
            ToolRequest::ProviderSemanticRename {
                language_id,
                documents,
                position_document_uri,
                position,
                new_name,
                project_root,
                sources,
            } => {
                let req = crate::provider_edit::ProviderRenameRequest {
                    documents: documents.clone(),
                    position_document_uri: position_document_uri.clone(),
                    position: *position,
                    new_name: new_name.clone(),
                    project_root: project_root.clone(),
                    sources: sources.clone(),
                };
                self.ok(
                    serde_json::to_value(self.run_provider_flow(language_id, |p| {
                        crate::provider_edit::semantic_rename(p, &req)
                    }))
                    .expect("serializes"),
                )
            }
            ToolRequest::ProviderValidateEdit {
                language_id,
                documents,
                transaction,
                sources,
                project_root,
            } => {
                let req = crate::provider_edit::ProviderValidateRequest {
                    documents: documents.clone(),
                    transaction: transaction.clone(),
                    sources: sources.clone(),
                    project_root: project_root.clone(),
                };
                self.ok(
                    serde_json::to_value(self.run_provider_flow(language_id, |p| {
                        crate::provider_edit::validate_transaction(p, &req)
                    }))
                    .expect("serializes"),
                )
            }
        }
    }

    /// Compile through the shared session pipeline.
    ///
    /// A failed refresh emptied the session's cache, so `compile`'s own
    /// load attempt re-surfaces the reload diagnostic as this envelope's
    /// refusal; the stale `self.loaded` snapshot is never consulted (#471).
    pub fn compile(&mut self) -> Envelope<CompileResult> {
        let _ = self.refresh();
        self.session.compile()
    }

    /// Check through the shared session pipeline.
    ///
    /// The same refresh contract as [`Self::compile`] applies (#471).
    pub fn check(&mut self) -> Envelope<CheckResult> {
        let _ = self.refresh();
        self.session.check()
    }

    /// Analyze through the shared session pipeline.
    ///
    /// The same refresh contract as [`Self::inspect`] applies (#471): a
    /// failed refresh cannot delegate to the stale snapshot, so the
    /// session's own load surfaces the refusal (#513 reuses the held
    /// semantic service on the fresh path).
    pub fn analyze(&mut self) -> Envelope<AnalyzeResult> {
        if self.refresh().is_err() {
            return self.session.analyze();
        }
        let loaded = self.snapshot().clone();
        let semantic = Arc::clone(
            self.semantic
                .as_ref()
                .expect("the refresh gate guarantees a snapshot"),
        );
        self.session.analyze_loaded(loaded, &semantic)
    }

    /// Inspect through the shared session pipeline.
    ///
    /// `inspect` renders through `self.semantic`, so a failed refresh cannot
    /// delegate to the stale snapshot the way `compile` can — the session's
    /// own load surfaces the same failure as the envelope's refusal (#471).
    pub fn inspect(&mut self) -> Envelope<InspectResult> {
        if self.refresh().is_err() {
            return self.session.inspect();
        }
        let loaded = self.snapshot().clone();
        let semantic = Arc::clone(
            self.semantic
                .as_ref()
                .expect("the refresh gate guarantees a snapshot"),
        );
        self.session.inspect_loaded(loaded, &semantic)
    }

    /// Spawn the LPP provider client for `language_id` through the session's
    /// provider registry (#142).
    ///
    /// Tooling consumes provider capabilities through the transport-neutral
    /// `wright_lpp::LanguageProvider` seam; process and JSON-RPC details
    /// stay inside `wright-lpp`. Unconfigured languages and missing
    /// capabilities refuse explicitly.
    pub fn language_provider(
        &self,
        language_id: &str,
    ) -> Result<Box<dyn wright_lpp::LanguageProvider>, wright_lpp::ProviderError> {
        self.session.language_provider(language_id)
    }

    /// Run a provider-driven mutation flow (#139) through the session's
    /// shared provider lifecycle path.
    fn run_provider_flow(
        &self,
        language_id: &str,
        flow: impl FnOnce(
            &mut dyn wright_lpp::LanguageProvider,
        ) -> crate::provider_edit::ProviderMutation,
    ) -> crate::provider_edit::ProviderMutation {
        self.session.run_provider_flow(
            language_id,
            &wright_lpp::ClientInfo {
                name: SERVICE_NAME.to_string(),
                version: SERVICE_VERSION.to_string(),
            },
            flow,
        )
    }

    fn ok(&self, result: serde_json::Value) -> ToolResponse {
        ToolResponse::Ok { result }
    }

    /// `findings`: every static-analysis finding with resolved span paths.
    ///
    /// The tool/agent surface resolves `span.path` exactly like the CLI
    /// `analyze`/`lint` workflows, so one file identity holds per finding
    /// across every surface (#102). A selection narrows the reported set
    /// (#430): without selection fields the result stays the bare finding
    /// array; with them it becomes `{"findings": [...], "selection": {...}}`
    /// so truncation is always visible.
    fn findings(&self, selection: &crate::select::FindingSelection) -> ToolResponse {
        if let Some(error) = self.selection_error(selection) {
            return error;
        }
        match self.semantic_query_with_resolved_span_paths(Request::GetFindings) {
            ToolResponse::Ok { result } => {
                let findings = match result {
                    serde_json::Value::Array(findings) => findings,
                    _ => Vec::new(),
                };
                let (findings, outcome) = selection
                    .apply_findings(findings, &crate::select::file_bases(&self.snapshot().input));
                match outcome {
                    Some(outcome) => self.ok(json!({ "findings": findings, "selection": outcome })),
                    None => self.ok(serde_json::Value::Array(findings)),
                }
            }
            other => other,
        }
    }

    /// `persistentObjects`: persistent-object facts with resolved source paths.
    fn persistent_objects(&self) -> ToolResponse {
        self.semantic_query_with_resolved_span_paths(Request::GetPersistentObjects)
    }

    /// Resolve a symbol `Address` to its numeric id (#429). A name resolves
    /// through the shared semantic index; unmatched and ambiguous names are
    /// the analyzer's structured `unknown-symbol`/`ambiguous-symbol` errors.
    fn symbol_address(&self, address: &Address) -> Result<u32, ToolErrorInfo> {
        match address {
            Address::Id(id) => Ok(*id),
            Address::Name(name) => self
                .semantic()
                .resolve_symbol(name)
                .map(|symbol| symbol.id.index() as u32),
        }
    }

    /// Resolve a rule `Address` to its numeric index (#429).
    fn rule_address(&self, address: &Address) -> Result<u32, ToolErrorInfo> {
        match address {
            Address::Id(id) => Ok(*id),
            Address::Name(name) => self.semantic().resolve_rule(name).map(|rule| rule as u32),
        }
    }

    fn semantic_query_with_resolved_span_paths(&self, request: Request) -> ToolResponse {
        match self.semantic_query(request) {
            ToolResponse::Ok { mut result } => {
                crate::session::resolve_span_paths(&mut result, self.snapshot());
                ToolResponse::Ok { result }
            }
            other => other,
        }
    }

    /// Run one semantic query over the loaded program.
    fn semantic_query(&self, request: Request) -> ToolResponse {
        self.semantic().handle(&request)
    }

    fn configured_semantic(&self) -> &SemanticService<'static> {
        self.lint_semantic
            .as_ref()
            .unwrap_or_else(|| self.semantic())
    }

    /// A selection naming an unknown rule id is a usage error, never a
    /// silent empty result (#430).
    fn selection_error(&self, selection: &crate::select::FindingSelection) -> Option<ToolResponse> {
        selection
            .validate(self.session.lint_registry())
            .err()
            .map(|message| ToolResponse::Error {
                error: ToolErrorInfo {
                    code: "invalid-selection".to_string(),
                    message,
                },
            })
    }

    /// `lint`: per-rule id and effective severity, effective configuration,
    /// and findings over the loaded program through the same semantic-service
    /// path as the CLI `lint` workflow (no duplicated rule execution, #98).
    /// Full rule metadata is served once by `lintRules` rather than inlined
    /// into every `lint` response (#431). A `selection` member records the
    /// true total and withheld count when the request selected a subset (#430).
    fn lint(&self, selection: &crate::select::FindingSelection) -> ToolResponse {
        if let Some(error) = self.selection_error(selection) {
            return error;
        }
        let service = self.configured_semantic();
        let lint_rules = match service.handle(&Request::LintRules) {
            Response::Ok { result } => result,
            Response::Error { .. } => serde_json::json!({}),
        };
        let mut findings = match service.handle(&Request::GetFindings) {
            Response::Ok { result } => result,
            Response::Error { .. } => serde_json::json!([]),
        };
        crate::session::resolve_span_paths(&mut findings, self.snapshot());
        let findings = match findings {
            serde_json::Value::Array(findings) => findings,
            _ => Vec::new(),
        };
        let (findings, outcome) =
            selection.apply_findings(findings, &crate::select::file_bases(&self.snapshot().input));
        let mut result = json!({
            "inputIdentity": self.snapshot().input.identity,
            "rules": crate::result::compact_lint_rules(lint_rules.get("rules")),
            "config": lint_rules.get("config").cloned().unwrap_or_else(|| json!({})),
            "findings": findings,
            "skipped": lint_rules.get("skipped").cloned().unwrap_or_else(|| json!([])),
        });
        if let Some(outcome) = outcome {
            result["selection"] = serde_json::to_value(outcome).expect("selection serializes");
        }
        self.ok(result)
    }

    /// Program summary with origin and source identity.
    fn project(&self) -> serde_json::Value {
        let loaded = self.snapshot();
        let index = SemanticIndex::build(&loaded.program);
        let findings = wright_analyzer::canonical::analyze(
            &loaded.program,
            &wright_analyzer::registry::LintConfig::default(),
        );
        json!({
            "origin": { "kind": loaded.origin.kind, "locale": loaded.origin.locale },
            "inputIdentity": loaded.input.identity,
            "files": loaded.source_files.len().max(1),
            "globalVariables": loaded.program.global_variables.len(),
            "playerVariables": loaded.program.player_variables.len(),
            "subroutines": loaded.program.subroutines.len(),
            "rules": loaded.program.rules.len(),
            "symbols": index.symbols().count(),
            "findings": findings.len(),
        })
    }

    fn call_graph(&self) -> serde_json::Value {
        let edges = self
            .snapshot()
            .program
            .rules
            .iter()
            .flat_map(|rule| {
                rule.actions.iter().filter_map(move |action| match action {
                    workshop_rs::Action::CallSubroutine { subroutine } => {
                        Some(json!({ "caller": rule.name, "callee": subroutine }))
                    }
                    _ => None,
                })
            })
            .collect::<Vec<_>>();
        serde_json::Value::Array(edges)
    }

    /// `costEstimate`: exact generated-resource counts plus static findings.
    /// The findings subset honors the shared selection (#430); findings carry
    /// no span, so a `file` selection matches nothing here.
    fn cost_estimate(&self, selection: &crate::select::FindingSelection) -> ToolResponse {
        if let Some(error) = self.selection_error(selection) {
            return error;
        }
        let loaded = self.snapshot();
        let locale = CompilerSession::locale_for(loaded);
        let text = workshop_rs::emitter::emit(&loaded.program, self.session.catalog(), &locale)
            .unwrap_or_default();
        let waits = loaded
            .program
            .rules
            .iter()
            .flat_map(|r| &r.actions)
            .filter(|a| matches!(a, workshop_rs::Action::Call { name, .. } if name == "wait"))
            .count();
        let findings = wright_analyzer::canonical::analyze(
            &loaded.program,
            &wright_analyzer::registry::LintConfig::default(),
        );
        let findings = findings
            .iter()
            .map(|f| {
                json!({
                    "code": f.code,
                    "severity": f.severity.as_str(),
                    "message": f.message,
                })
            })
            .collect::<Vec<_>>();
        let (findings, outcome) =
            selection.apply_findings(findings, &crate::select::file_bases(&loaded.input));
        let mut result = json!({
            "exact": {
                "emittedBytes": text.len(),
                "programActions": loaded.program.rules.iter().map(|r| r.actions.len()).sum::<usize>(),
                "programRules": loaded.program.rules.len(),
                "waitActions": waits,
            },
            "findings": findings,
            "kind": {
                "exact": "exact target-resource counts",
                "findings": "static/heuristic execution indicators",
                "performance": "compiler-host performance is measured by wright-bench, not in-process",
            },
        });
        if let Some(outcome) = outcome {
            result["selection"] = serde_json::to_value(outcome).expect("selection serializes");
        }
        self.ok(result)
    }

    fn target_metadata(&self) -> serde_json::Value {
        let catalog = self.session.catalog();
        json!({
            "catalogVersion": catalog.catalog_version(),
            "locales": catalog.locales().iter().map(|l| l.to_string()).collect::<Vec<_>>(),
            "actions": catalog.entries_of(workshop_rs::catalog::Kind::Action).count(),
            "values": catalog.entries_of(workshop_rs::catalog::Kind::Value).count(),
            "events": catalog.entries_of(workshop_rs::catalog::Kind::Event).count(),
            "operators": catalog.entries_of(workshop_rs::catalog::Kind::Operator).count(),
            "enumDomains": catalog.enum_domains().map(|domain| json!({
                "domain": domain.domain,
                "members": domain.members.iter().map(|m| m.member.clone()).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }
}

/// The hotpath measurement label for one request's dispatch.
#[cfg_attr(not(feature = "hotpath"), allow(dead_code))]
fn request_label(request: &ToolRequest) -> &'static str {
    match request {
        ToolRequest::Capabilities => "req::capabilities",
        ToolRequest::Compile => "req::compile",
        ToolRequest::Check => "req::check",
        ToolRequest::Analyze => "req::analyze",
        ToolRequest::Inspect => "req::inspect",
        ToolRequest::Project => "req::project",
        ToolRequest::Rules => "req::rules",
        ToolRequest::Symbols { .. } => "req::symbols",
        ToolRequest::References { .. } => "req::references",
        ToolRequest::Usage { .. } => "req::usage",
        ToolRequest::Cfg { .. } => "req::cfg",
        ToolRequest::Findings(_) => "req::findings",
        ToolRequest::PersistentObjects => "req::persistentObjects",
        ToolRequest::Lint(_) => "req::lint",
        ToolRequest::LintRules => "req::lintRules",
        ToolRequest::CallGraph => "req::callGraph",
        ToolRequest::CostEstimate(_) => "req::costEstimate",
        ToolRequest::TargetMetadata => "req::targetMetadata",
        ToolRequest::ValidateEdit { .. } => "req::validateEdit",
        ToolRequest::SemanticRename { .. } => "req::semanticRename",
        ToolRequest::ProviderSemanticRename { .. } => "req::providerSemanticRename",
        ToolRequest::ProviderValidateEdit { .. } => "req::providerValidateEdit",
    }
}

/// Whether a result payload's `diagnostics` include `code` (#471): the
/// `semanticRename` refusal surface embeds `ambiguous-symbol` this way.
fn diagnostics_list_code(result: &serde_json::Value, code: &str) -> bool {
    result
        .get("diagnostics")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|diagnostics| {
            diagnostics
                .iter()
                .any(|d| d.get("code").and_then(serde_json::Value::as_str) == Some(code))
        })
}
