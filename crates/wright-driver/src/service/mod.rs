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

use serde_json::json;

use crate::diag::Diagnostic;
use crate::input;
use crate::result::{AnalyzeResult, CheckResult, CompileResult, Envelope, InspectResult};
use crate::session::Provenance;
use crate::source_provider::document_sources;
use crate::{CompilerSession, Loaded};
use wright_analyzer::canonical::{
    BLOCK_KINDS, ReferenceKind, SemanticIndex, SemanticService, SymbolKind,
};
/// A structured tool error.
pub use wright_analyzer::service::ErrorInfo as ToolErrorInfo;
/// A tool response: a structured owned result or a structured error.
pub use wright_analyzer::service::Response as ToolResponse;
use wright_analyzer::service::{Request, Response};

mod capabilities;
mod cost;
mod edit_statements;
mod simulate;

/// The tool-service name and version.
pub const SERVICE_NAME: &str = "wright-tool-service";
pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const AGENT_CONTRACT: &str = "wright-agent/v1";

/// Rules returned by a `content` request that sets no `max` (ADR-0024).
const DEFAULT_CONTENT_RULES: usize = 20;

pub use capabilities::{Capabilities, OPERATIONS};

pub use crate::request::{
    Address, StatementEditChange, StatementEditRequest, StatementList, ToolRequest,
};

struct Snapshot {
    loaded: Loaded,
    semantic: Arc<SemanticService<'static>>,
    lint_semantic: Option<SemanticService<'static>>,
    /// The disk state `loaded` was read from, recomputed before every
    /// program-reading request so a long-lived session observes edits, file
    /// additions, and removals (#471).
    fingerprint: input::DiskFingerprint,
}

/// The session-aware tool service.
pub struct ToolService<'a> {
    session: &'a mut CompilerSession,
    /// The current program snapshot, when one exists. The service may exist
    /// without one (#512): a configured project that cannot load at
    /// construction is deferred to the first program-reading request rather
    /// than failing the service, so `None` is the unloaded state — never an
    /// empty placeholder program.
    snapshot: Option<Snapshot>,
    /// The served program's generation (#595): `adopt` increments it, so
    /// every adopted snapshot — the deferred initial load, a
    /// content-changing reload, a recovery after a failed load — is a
    /// distinct generation. Responses that issue numeric `symbol`/`rule`
    /// addresses report it as `snapshot`, and a request carrying a numeric
    /// address must send that value back or be refused `stale-id`.
    generation: u64,
    /// How many times an observed disk change reloaded the program — the
    /// observable hook for "unchanged inputs are not reloaded" (#471).
    reloads: usize,
    /// `true` while the service sits in the invalidated state after a
    /// failed refresh (#512): the next successful load is a reload — an
    /// observed change brought the project back — rather than the deferred
    /// initial load.
    invalidated: bool,
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
    /// `targetMetadata`, the `provider*` mutations carrying their own
    /// document set) stay available, and each program-reading request
    /// retries the load — surfacing the loader's structured diagnostic
    /// until the project heals.
    #[hotpath::measure]
    pub fn new(session: &'a mut CompilerSession) -> Result<ToolService<'a>, Diagnostic> {
        session.verify_fixed_config()?;
        let mut service = ToolService {
            session,
            snapshot: None,
            generation: 0,
            reloads: 0,
            invalidated: false,
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
        self.snapshot.as_ref().map(|snapshot| &snapshot.loaded)
    }

    /// The served program's generation (#595) — the value responses report
    /// as `snapshot` and the value a numeric `symbol`/`rule` request must
    /// carry. `None` while no snapshot is adopted (deferred initial load or
    /// the invalidated state after a failed load).
    pub fn snapshot_generation(&self) -> Option<u64> {
        self.snapshot.as_ref().map(|_| self.generation)
    }

    /// Full semantic-service builds behind this service's session (#513) —
    /// the observable hook proving repeated requests share the loaded
    /// snapshot's index rather than rebuilding it.
    pub fn semantic_build_count(&self) -> usize {
        self.session.semantic_build_count()
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
    /// the static catalog, and `provider*` operations carrying their own
    /// document set consult neither — a `provider*` request that omits its
    /// document set derives it from the loaded project (#548) and so
    /// triggers the load like any other program-reading operation.
    ///
    /// `check` and `compile` are workflows, not snapshot queries (#591):
    /// they run the session's operation-aware load instead of requiring a
    /// canonical snapshot, so a compile-capable provider's `check` compiles
    /// for canonical validation and supplies the snapshot later queries
    /// read (#617); a provider that declines its compile keeps the
    /// check-only path.
    pub fn handle(&mut self, request: &ToolRequest) -> ToolResponse {
        let response = self.handle_full(request);
        let ToolResponse::Ok { mut result } = response else {
            return response;
        };
        match request {
            ToolRequest::Check { all } => crate::essential::envelope(&mut result, *all),
            ToolRequest::Analyze { all, .. } => {
                result["result"] = crate::essential::analyze(&result["result"], *all);
                crate::essential::envelope(&mut result, *all);
            }
            ToolRequest::Inspect { all } => {
                result["result"] = crate::essential::inspect(&result["result"], *all);
                crate::essential::envelope(&mut result, *all);
            }
            ToolRequest::Lint { all, .. } => result = crate::essential::lint(&result, *all),
            ToolRequest::References { symbol, all, .. } => {
                let id = match self.symbol_address(symbol) {
                    Ok(id) => id,
                    Err(error) => return ToolResponse::Error { error },
                };
                let usage = match self.semantic_query(Request::GetUsage { symbol: id }) {
                    ToolResponse::Ok { result } => result,
                    other => return other,
                };
                let mut payload = usage;
                payload["snapshot"] = json!(self.generation);
                if let Some(selected) = result.as_object_mut() {
                    payload["references"] = selected.remove("references").unwrap_or_default();
                    if let Some(selection) = selected.remove("selection") {
                        payload["selection"] = selection;
                    }
                } else {
                    payload["references"] = result;
                }
                result = crate::essential::query(&payload, *all);
            }
            ToolRequest::CostEstimate { all, .. } => result = crate::essential::cost(&result, *all),
            ToolRequest::Rules { all, .. }
            | ToolRequest::Symbols { all, .. }
            | ToolRequest::Cfg { all, .. }
            | ToolRequest::Findings { all, .. }
            | ToolRequest::PersistentObjects { all }
            | ToolRequest::CallGraph { all, .. } => result = crate::essential::query(&result, *all),
            _ => {}
        }
        ToolResponse::Ok { result }
    }

    pub(crate) fn handle_full(&mut self, request: &ToolRequest) -> ToolResponse {
        match request {
            ToolRequest::Check { .. } | ToolRequest::Compile { .. } => self.refresh_workflow(),
            _ if Self::reads_program(request) => {
                // ADR-0029: `editStatements` answers on the resolved input
                // kind, not on whether the project can load — an OPY/OSTW
                // input fails `refresh` before the operation's own
                // provider-boundary refusal could run, so the raw-Workshop
                // gate evaluates first.
                if let ToolRequest::EditStatements { .. } = request
                    && let Some(response) = self.edit_statements_kind_gate()
                {
                    return response;
                }
                // ADR-0033: `simulate` has no input-kind gate — `athena`
                // runs the loaded program's canonical emission, so a
                // provider-loaded source-language project simulates its
                // compiled Workshop artifact like every other
                // program-reading operation.
                if let Err(error) = self.refresh() {
                    return ToolResponse::Error { error };
                }
                if let Some(error) = self.stale_id(request) {
                    return ToolResponse::Error { error };
                }
            }
            _ => {}
        }
        let response = hotpath::measure_block!(request_label(request), self.dispatch(request));
        self.adopt_session_snapshot();
        self.stamp_snapshot(request, response)
    }

    /// Whether the request requires a canonical snapshot to answer (#471,
    /// #591). `check`/`compile` do not: they run their own operation-aware
    /// load, so a provider without canonical compile capability still
    /// answers `check`. A provider operation carrying its own document set
    /// consults no snapshot either; one that omits it derives the set from
    /// the loaded project (#548) and so requires it like every other
    /// program-reading operation.
    fn reads_program(request: &ToolRequest) -> bool {
        !matches!(
            request,
            ToolRequest::Check { .. }
                | ToolRequest::Compile { .. }
                | ToolRequest::Capabilities
                | ToolRequest::TargetMetadata
                | ToolRequest::ProviderValidateEdit {
                    documents: Some(_),
                    ..
                }
                | ToolRequest::Lookup { .. }
                | ToolRequest::ListEntries { .. }
        )
    }

    /// Ensure a valid current snapshot backs a program-reading request
    /// (#471, #512). With no snapshot the request performs the deferred
    /// initial load; with one, a changed disk fingerprint reloads it. A
    /// failed attempt surfaces the loader's structured diagnostic and
    /// invalidates the served snapshot — the previous program is never
    /// served, even if the input is restored to identical bytes — so the
    /// next request retries the same resolution and a repaired project is
    /// adopted without restarting the service.
    fn refresh(&mut self) -> Result<(), ToolErrorInfo> {
        let Some(snapshot) = &self.snapshot else {
            // The deferred initial load, or recovery after an invalidated
            // snapshot (#512). Recovery counts as a reload — an observed
            // disk change brought the project back — and `adopt` gives it a
            // fresh generation, so ids issued by the dropped program stay
            // refused (#595).
            let loaded = self.session.load().map_err(Self::loader_error)?;
            self.adopt(loaded);
            if self.invalidated {
                self.reloads += 1;
                self.invalidated = false;
            }
            return Ok(());
        };
        let fingerprint = input::disk_fingerprint(&self.session.config, &snapshot.loaded.input);
        if snapshot.fingerprint == fingerprint {
            return Ok(());
        }
        let loaded = match self.session.reload() {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.invalidate();
                return Err(Self::loader_error(diagnostic));
            }
        };
        self.adopt(loaded);
        self.reloads += 1;
        Ok(())
    }

    /// Drop the served snapshot after a failed refresh (#512). The next
    /// program-reading request must load again — restoring the input's exact
    /// previous bytes must not fingerprint-match the pre-failure snapshot
    /// back to life — and ids it issued keep their old generation, so they
    /// refuse `stale-id` against the recovered program (#595).
    fn invalidate(&mut self) {
        self.clear_snapshot();
        self.invalidated = true;
    }

    fn clear_snapshot(&mut self) {
        self.snapshot = None;
    }

    /// The freshness gate for `check`/`compile` (#591): like [`Self::refresh`]
    /// an observed disk change invalidates the served snapshot, but the
    /// workflow itself performs the operation-aware load — the session never
    /// demands a canonical compile just to run `check`, and the load's
    /// outcome rides in the workflow envelope rather than a `ToolResponse`
    /// refusal. No snapshot held means there is nothing to invalidate: the
    /// workflow resolves the input itself.
    fn refresh_workflow(&mut self) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let fingerprint = input::disk_fingerprint(&self.session.config, &snapshot.loaded.input);
        if snapshot.fingerprint == fingerprint {
            return;
        }
        self.session.drop_loaded();
        self.clear_snapshot();
        self.reloads += 1;
    }

    /// Adopt the canonical snapshot a `check`/`compile` workflow established
    /// when the service holds none (#591): a successful `compile` makes its
    /// program the snapshot later queries serve — the same guarantee as a
    /// session that had already compiled before service construction. A
    /// check-only provider's `check` caches no program (`cached_loaded` is
    /// `None`), so snapshot queries keep refusing `load()`'s explicit
    /// unsupported state rather than inheriting a placeholder program.
    fn adopt_session_snapshot(&mut self) {
        if self.snapshot.is_none()
            && let Some(loaded) = self.session.cached_loaded()
        {
            self.adopt(loaded);
        }
    }

    fn loader_error(diagnostic: Diagnostic) -> ToolErrorInfo {
        ToolErrorInfo {
            code: diagnostic.code,
            message: diagnostic.message,
        }
    }

    /// Adopt a successfully loaded program: every adoption is a new
    /// generation (#595), so numeric addresses a previous generation issued
    /// refuse `stale-id` — including the byte-identical recovery case, which
    /// is still a new program even though `inputIdentity` is unchanged.
    fn adopt(&mut self, loaded: Loaded) {
        let semantic = self.session.shared_semantic(&loaded);
        let lint_semantic = (!self.session.config.lint.rules.is_empty())
            .then(|| semantic.with_lint_config(self.session.config.lint.clone()));
        let fingerprint = input::disk_fingerprint(&self.session.config, &loaded.input);
        self.generation += 1;
        self.snapshot = Some(Snapshot {
            loaded,
            semantic,
            lint_semantic,
            fingerprint,
        });
    }

    fn snapshot(&self) -> &Snapshot {
        self.snapshot
            .as_ref()
            .expect("the refresh gate guarantees a snapshot")
    }

    fn snapshot_and_semantic(&self) -> (Loaded, Arc<SemanticService<'static>>) {
        let snapshot = self.snapshot();
        (snapshot.loaded.clone(), Arc::clone(&snapshot.semantic))
    }

    /// The document set a provider mutation runs against (#548): the
    /// caller's set when supplied, otherwise the loaded project's members —
    /// each member read from disk at `version` 0 and keyed by `file://` URI,
    /// so the provider sees the same project the session serves. The
    /// omitted-set path only runs after the refresh gate, so `snapshot` is
    /// guaranteed; a member that is not a readable disk file is a structured
    /// refusal, never a silently partial set.
    fn provider_documents(
        &self,
        language_id: &str,
        documents: &Option<wright_lpp::DocumentSet>,
    ) -> Result<wright_lpp::DocumentSet, ToolErrorInfo> {
        if let Some(documents) = documents {
            return Ok(documents.clone());
        }
        let loaded = &self.snapshot().loaded;
        // Mapped members are provider-file-table spellings relative to the
        // project root the provider was given; unmapped members are the
        // input's own display spellings, relative to the invocation cwd.
        let base = if loaded.provenance == Provenance::Mapped {
            &loaded.input.root
        } else {
            &loaded.input.cwd
        };
        crate::source_provider::provider_document_set(&loaded.source_files, base, language_id)
    }

    fn semantic(&self) -> &SemanticService<'static> {
        &self.snapshot().semantic
    }

    /// The snapshot gate for numeric addresses (#595): every numeric
    /// symbol/rule address a request carries must be scoped to the served
    /// program's generation — the request's `snapshot` member must match
    /// `self.generation`. A missing token or a different generation is
    /// refused `stale-id`, so a numeric id captured from an earlier program
    /// can never silently resolve against a reloaded or recovered one. A
    /// `references` request's numeric `rule` filter is checked like its
    /// `symbol` (#592). A request whose addresses are all declared names is
    /// generation-agnostic: it ignores `snapshot` and resolves against the
    /// current program.
    fn stale_id(&self, request: &ToolRequest) -> Option<ToolErrorInfo> {
        let (numeric, snapshot) = match request {
            ToolRequest::References {
                symbol,
                rule,
                snapshot,
                ..
            } => (
                matches!(symbol, Address::Id(_)) || matches!(rule, Some(Address::Id(_))),
                *snapshot,
            ),
            ToolRequest::Usage { symbol, snapshot } => {
                (matches!(symbol, Address::Id(_)), *snapshot)
            }
            ToolRequest::Cfg { rule, snapshot, .. } => (matches!(rule, Address::Id(_)), *snapshot),
            ToolRequest::Rules { rule, snapshot, .. } => {
                (matches!(rule, Some(Address::Id(_))), *snapshot)
            }
            ToolRequest::EditStatements {
                edits, snapshot, ..
            } => (
                edits.iter().any(|edit| matches!(edit.rule, Address::Id(_))),
                *snapshot,
            ),
            _ => (false, None),
        };
        if !numeric {
            return None;
        }
        match snapshot {
            Some(issued) if issued == self.generation => None,
            Some(issued) => Some(ToolErrorInfo {
                code: "stale-id".to_string(),
                message: format!(
                    "numeric symbol ids and rule indexes are scoped to the program generation that issued them: the served program is snapshot {} but this request's addresses were issued under snapshot {issued} — re-issue the ids from a current `symbols`/`rules` (or other id-reporting) response, or address by name",
                    self.generation
                ),
            }),
            None => Some(ToolErrorInfo {
                code: "stale-id".to_string(),
                message: format!(
                    "a numeric `symbol`/`rule` address is scoped to the program generation that issued it: send the `snapshot` value an id-reporting response (`symbols`, `rules`, `inspect`, `project`, …) returned — the current generation is {}",
                    self.generation
                ),
            }),
        }
    }

    /// Report the serving generation on every response that issues numeric
    /// `symbol`/`rule` addresses (#595): `snapshot` joins the result object
    /// — or wraps a bare-array listing result under its key — so a client
    /// always reads the generation the ids it holds belong to. It lands on
    /// the `wright-result/v1` envelope for `analyze`/`inspect` and in the
    /// payload for the query operations.
    fn stamp_snapshot(&self, request: &ToolRequest, mut response: ToolResponse) -> ToolResponse {
        let ToolResponse::Ok { result } = &mut response else {
            return response;
        };
        let list_key = match request {
            ToolRequest::Rules { .. } => Some("rules"),
            ToolRequest::Symbols { .. } => Some("symbols"),
            ToolRequest::Findings { .. } => Some("findings"),
            ToolRequest::PersistentObjects { .. } => Some("persistentObjects"),
            // `references` without selection fields answers a bare array —
            // the `handle` merge then moves `references`/`selection` onto the
            // usage payload and stamps `snapshot` there.
            ToolRequest::References { .. } => Some("references"),
            ToolRequest::Project
            | ToolRequest::Usage { .. }
            | ToolRequest::Lint { .. }
            | ToolRequest::LintRules
            | ToolRequest::CostEstimate { .. }
            | ToolRequest::Analyze { .. }
            | ToolRequest::Inspect { .. } => None,
            _ => return response,
        };
        let generation = json!(self.generation);
        if result.is_array() {
            let items = std::mem::take(result);
            *result = json!({ list_key.unwrap(): items, "snapshot": generation });
        } else {
            result["snapshot"] = generation;
        }
        response
    }

    fn dispatch(&mut self, request: &ToolRequest) -> ToolResponse {
        match request {
            ToolRequest::Capabilities => ToolResponse::Ok {
                result: serde_json::to_value(self.capabilities()).expect("capabilities serialize"),
            },
            ToolRequest::Compile { output, force, to } => {
                let envelope = self.session.compile_to(crate::CompileWrite {
                    dest: output
                        .clone()
                        .map_or(crate::OutputDest::Default, crate::OutputDest::Path),
                    overwrite: if *force {
                        crate::Overwrite::Force
                    } else {
                        crate::Overwrite::Refuse
                    },
                    to: to.clone(),
                });
                let result = serde_json::to_value(envelope).expect("compile result serializes");
                ToolResponse::Ok { result }
            }
            ToolRequest::Check { .. } => {
                let result =
                    serde_json::to_value(self.session.check()).expect("check result serializes");
                ToolResponse::Ok { result }
            }
            ToolRequest::Analyze { selection, all: _ } => {
                if let Some(error) = self.selection_error(selection) {
                    return error;
                }
                let (loaded, semantic) = self.snapshot_and_semantic();
                let result =
                    serde_json::to_value(self.session.analyze_loaded(loaded, &semantic, selection))
                        .expect("analyze result serializes");
                ToolResponse::Ok { result }
            }
            ToolRequest::Inspect { all: _ } => {
                let (loaded, semantic) = self.snapshot_and_semantic();
                let result = serde_json::to_value(self.session.inspect_loaded(loaded, &semantic))
                    .expect("inspect result serializes");
                ToolResponse::Ok { result }
            }
            ToolRequest::Project => self.ok(self.project()),
            ToolRequest::Rules {
                name,
                rule,
                file,
                max,
                content,
                all,
                ..
            } => self.rules(
                name,
                rule.as_ref(),
                file,
                &max.or_else(|| (*all && *content).then_some(usize::MAX)),
                *content,
            ),
            ToolRequest::Symbols {
                kind, file, max, ..
            } => self.symbols(kind.as_deref(), file.as_deref(), *max),
            ToolRequest::References {
                symbol,
                kind,
                rule,
                file,
                max,
                ..
            } => self.references(
                symbol,
                kind.as_deref(),
                rule.as_ref(),
                file.as_deref(),
                *max,
            ),
            ToolRequest::Usage { symbol, .. } => match self.symbol_address(symbol) {
                Ok(symbol) => self.semantic_query(Request::GetUsage { symbol }),
                Err(error) => ToolResponse::Error { error },
            },
            ToolRequest::Cfg {
                rule, kind, max, ..
            } => match self.rule_address(rule) {
                Ok(rule) => self.cfg(rule, kind.as_deref(), *max),
                Err(error) => ToolResponse::Error { error },
            },
            ToolRequest::Findings { selection, .. } => self.findings(selection),
            ToolRequest::PersistentObjects { .. } => self.persistent_objects(),
            ToolRequest::Lint { selection, all: _ } => self.lint(selection),
            ToolRequest::LintRules => self.configured_semantic().handle(&Request::LintRules),
            ToolRequest::CallGraph {
                caller,
                callee,
                max,
                ..
            } => self.call_graph(caller.as_deref(), callee.as_deref(), *max),
            ToolRequest::CostEstimate {
                selection, rule, ..
            } => self.cost_estimate(selection, rule.as_deref()),
            ToolRequest::TargetMetadata => self.ok(self.target_metadata()),
            ToolRequest::ValidateEdit {
                sources,
                transaction,
            } => self.ok(serde_json::to_value(
                self.session
                    .validate_edit_transaction(sources.as_ref(), transaction),
            )
            .expect("serializes")),
            ToolRequest::ProviderValidateEdit {
                language_id,
                documents,
                transaction,
                sources,
                project_root,
            } => {
                let documents = match self.provider_documents(language_id, documents) {
                    Ok(documents) => documents,
                    Err(error) => return ToolResponse::Error { error },
                };
                let req = crate::provider_edit::ProviderValidateRequest {
                    transaction: transaction.clone(),
                    sources: sources
                        .clone()
                        .unwrap_or_else(|| document_sources(&documents)),
                    project_root: project_root.clone(),
                    documents,
                };
                self.ok(
                    serde_json::to_value(self.run_provider_flow(language_id, |p| {
                        crate::provider_edit::validate_transaction(p, &req)
                    }))
                    .expect("serializes"),
                )
            }
            ToolRequest::EditStatements {
                input_identity,
                sources,
                edits,
                ..
            } => self.edit_statements(input_identity, sources, edits),
            ToolRequest::Simulate {
                input_identity,
                sources,
                seconds,
                scenario,
            } => self.simulate(input_identity, sources, *seconds, scenario),
            ToolRequest::Lookup {
                language,
                query,
                kind,
                within,
                locale,
                limit,
            } => crate::lookup::lookup(
                self.session,
                language,
                query.as_deref(),
                kind.as_deref(),
                within.as_deref(),
                locale.as_deref(),
                *limit,
            ),
            ToolRequest::ListEntries {
                language,
                kind,
                within,
                locale,
                page_size,
                cursor,
            } => crate::list_entries::list_entries(
                self.session,
                language,
                kind,
                within.as_deref(),
                locale.as_deref(),
                *page_size,
                cursor.as_deref(),
            ),
        }
    }

    /// Compile through the shared session pipeline.
    ///
    /// A changed input drops the stale snapshot so `compile`'s own load
    /// re-resolves it (#471); a successful load supplies the snapshot later
    /// queries read (#591). A failed load surfaces as this envelope's
    /// diagnostics — the stale snapshot is never consulted (#512).
    pub fn compile(&mut self) -> Envelope<CompileResult> {
        self.refresh_workflow();
        let envelope = self.session.compile();
        self.adopt_session_snapshot();
        envelope
    }

    /// Check through the shared session pipeline.
    ///
    /// The same freshness contract as [`Self::compile`] applies (#471),
    /// minus the canonical-snapshot requirement: `check` runs the
    /// operation-aware load, which on a compile-capable provider compiles
    /// for canonical validation (#617) and on a provider that declines its
    /// compile answers the check operation alone (#591).
    pub fn check(&mut self) -> Envelope<CheckResult> {
        self.refresh_workflow();
        let envelope = self.session.check();
        self.adopt_session_snapshot();
        envelope
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
        let (loaded, semantic) = self.snapshot_and_semantic();
        let selection = self.session.config.selection.clone();
        self.session.analyze_loaded(loaded, &semantic, &selection)
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
        let (loaded, semantic) = self.snapshot_and_semantic();
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
        match self.semantic_query(Request::GetFindings) {
            ToolResponse::Ok { result } => {
                let findings = crate::session::prepare_findings(
                    result,
                    self.semantic(),
                    &self.snapshot().loaded,
                    self.session.catalog(),
                );
                let (findings, outcome) = selection.apply_findings(
                    findings,
                    &crate::select::file_bases(&self.snapshot().loaded.input),
                );
                match outcome {
                    Some(outcome) => self.ok(json!({ "findings": findings, "selection": outcome })),
                    None => self.ok(serde_json::Value::Array(findings)),
                }
            }
            other => other,
        }
    }

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
                crate::session::resolve_span_paths(&mut result, &self.snapshot().loaded);
                ToolResponse::Ok { result }
            }
            other => other,
        }
    }

    fn semantic_query(&self, request: Request) -> ToolResponse {
        self.semantic().handle(&request)
    }

    fn configured_semantic(&self) -> &SemanticService<'static> {
        let snapshot = self.snapshot();
        snapshot
            .lint_semantic
            .as_ref()
            .unwrap_or(&snapshot.semantic)
    }

    /// A selection naming an unknown rule id is a usage error, never a
    /// silent empty result (#430).
    fn selection_error(&self, selection: &crate::select::FindingSelection) -> Option<ToolResponse> {
        self.selection_check(selection)
            .err()
            .map(|error| ToolResponse::Error { error })
    }

    /// The selection's validity as a structured error: an unknown `rule` id
    /// would silently select nothing and report a clean result (#430).
    fn selection_check(
        &self,
        selection: &crate::select::FindingSelection,
    ) -> Result<(), ToolErrorInfo> {
        selection
            .validate(self.session.lint_registry())
            .map_err(|message| ToolErrorInfo {
                code: "invalid-selection".to_string(),
                message,
            })
    }

    /// Bound one list-shaped semantic operation's result (#531): without
    /// selection fields the previous bare array is returned unchanged;
    /// with them the result becomes `{<key>: [...], "selection": {...}}`
    /// reporting the pre-selection `total` and the count `max` withheld.
    fn select_result(
        &self,
        key: &str,
        result: serde_json::Value,
        file: Option<&str>,
        keep: impl Fn(&serde_json::Value) -> bool,
        max: Option<usize>,
        selected: bool,
    ) -> ToolResponse {
        let items = match result {
            serde_json::Value::Array(items) => items,
            _ => Vec::new(),
        };
        let (kept, outcome) = crate::select::select_list(
            items,
            file,
            &crate::select::file_bases(&self.snapshot().loaded.input),
            json_span_path,
            keep,
            max,
        );
        if !selected {
            return self.ok(serde_json::Value::Array(kept));
        }
        self.ok(json!({ key: kept, "selection": outcome }))
    }

    /// `rules`: every rule with resolved span paths, like `symbols` — a
    /// `file` id without its path is unusable to a caller. The selection
    /// fields are `name` (a declared rule name), `rule` (one rule address),
    /// `file`, and `max` (#531). With `content`, each selected rule carries
    /// its canonical content, `max` defaults to [`DEFAULT_CONTENT_RULES`],
    /// and the selection shape is always returned (ADR-0024).
    fn rules(
        &self,
        name: &Option<String>,
        rule: Option<&Address>,
        file: &Option<String>,
        max: &Option<usize>,
        content: bool,
    ) -> ToolResponse {
        if let Some(name) = name
            && !self
                .snapshot()
                .loaded
                .program
                .rules
                .iter()
                .any(|rule| rule.name == *name)
        {
            return invalid_selection(format!("unknown rule '{name}'"));
        }
        let rule_id = match rule {
            Some(address) => match self.rule_address(address) {
                Ok(id) if (id as usize) < self.snapshot().loaded.program.rules.len() => {
                    Some(id as u64)
                }
                Ok(id) => return invalid_selection(format!("unknown rule {id}")),
                Err(error) if error.code == "unknown-rule" => {
                    return invalid_selection(error.message);
                }
                Err(error) => return ToolResponse::Error { error },
            },
            None => None,
        };
        let max = if content {
            max.or(Some(DEFAULT_CONTENT_RULES))
        } else {
            *max
        };
        let selected =
            content || name.is_some() || rule_id.is_some() || file.is_some() || max.is_some();
        let response = self.semantic_query_with_resolved_span_paths(Request::ListRules);
        let response = match response {
            ToolResponse::Ok { result } => self.select_result(
                "rules",
                result,
                file.as_deref(),
                |item| {
                    name.as_deref()
                        .is_none_or(|name| item["name"].as_str() == Some(name))
                        && rule_id.is_none_or(|id| item["id"].as_u64() == Some(id))
                },
                max,
                selected,
            ),
            other => other,
        };
        if content {
            self.attach_rule_content(response)
        } else {
            response
        }
    }

    fn attach_rule_content(&self, response: ToolResponse) -> ToolResponse {
        let ToolResponse::Ok { mut result } = response else {
            return response;
        };
        // OverPy spellings for the canonical ids need a provider capability
        // that no provider advertises yet (ADR-0024 decision 4).
        if self.snapshot().loaded.input.kind == crate::config::SourceKind::Opy {
            result["unavailable"] = json!({"names": "opy-rs"});
        }
        let Some(rules) = result["rules"].as_array_mut() else {
            return ToolResponse::Ok { result };
        };
        for rule in rules {
            let id = rule["id"].as_u64().unwrap_or_default() as u32;
            match self.semantic_query_with_resolved_span_paths(Request::GetRuleContent { rule: id })
            {
                ToolResponse::Ok { result: content } => rule["content"] = content,
                other => return other,
            }
        }
        ToolResponse::Ok { result }
    }

    /// `symbols`: every symbol with resolved span paths. `kind` predates
    /// #531 — a request carrying only `kind` keeps the previous bare-array
    /// shape, still filtered; the `file`/`max` selection fields wrap the
    /// result in `{"symbols": [...], "selection": {...}}` with the
    /// pre-selection total.
    fn symbols(&self, kind: Option<&str>, file: Option<&str>, max: Option<usize>) -> ToolResponse {
        if let Some(kind) = kind
            && !SymbolKind::ALL.iter().any(|known| known.as_str() == kind)
        {
            return invalid_selection(format!(
                "unknown symbol kind '{kind}' (expected one of: {})",
                SymbolKind::ALL
                    .iter()
                    .map(|known| known.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        match self.semantic_query_with_resolved_span_paths(Request::ListSymbols { kind: None }) {
            ToolResponse::Ok { result } => self.select_result(
                "symbols",
                result,
                file,
                |symbol| kind.is_none_or(|kind| symbol["kind"].as_str() == Some(kind)),
                max,
                file.is_some() || max.is_some(),
            ),
            other => other,
        }
    }

    /// `references`: every reference to one symbol. The selection fields
    /// are `kind` (a reference kind), `rule` (a rule address narrowing to
    /// references inside that rule), `file`, and `max` (#531).
    fn references(
        &self,
        symbol: &Address,
        kind: Option<&str>,
        rule: Option<&Address>,
        file: Option<&str>,
        max: Option<usize>,
    ) -> ToolResponse {
        if let Some(kind) = kind
            && !ReferenceKind::ALL
                .iter()
                .any(|known| known.as_str() == kind)
        {
            return invalid_selection(format!(
                "unknown reference kind '{kind}' (expected one of: {})",
                ReferenceKind::ALL
                    .iter()
                    .map(|known| known.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let rule_id = match rule {
            Some(address) => match self.rule_address(address) {
                Ok(id) if (id as usize) < self.snapshot().loaded.program.rules.len() => Some(id),
                Ok(id) => return invalid_selection(format!("unknown rule {id}")),
                Err(error) => return invalid_selection(error.message),
            },
            None => None,
        };
        match self.symbol_address(symbol) {
            Ok(symbol) => {
                match self
                    .semantic_query_with_resolved_span_paths(Request::FindReferences { symbol })
                {
                    ToolResponse::Ok { result } => self.select_result(
                        "references",
                        result,
                        file,
                        |reference| {
                            kind.is_none_or(|kind| reference["kind"].as_str() == Some(kind))
                                && rule_id.is_none_or(|id| {
                                    reference["rule"].as_u64() == Some(u64::from(id))
                                })
                        },
                        max,
                        kind.is_some() || rule.is_some() || file.is_some() || max.is_some(),
                    ),
                    other => other,
                }
            }
            Err(error) => ToolResponse::Error { error },
        }
    }

    /// `cfg`: one rule's control-flow graph. The selection fields are
    /// `kind` (a block kind) and `max` (#531): they filter and bound
    /// `blocks` — which keep their original `id`s, so `successors` still
    /// address the full graph — and add a `selection` member with the
    /// pre-selection total.
    fn cfg(&self, rule: u32, kind: Option<&str>, max: Option<usize>) -> ToolResponse {
        if let Some(kind) = kind
            && !BLOCK_KINDS.contains(&kind)
        {
            return invalid_selection(format!(
                "unknown cfg block kind '{kind}' (expected one of: {})",
                BLOCK_KINDS.join(", ")
            ));
        }
        match self.semantic_query(Request::GetCfg { rule }) {
            ToolResponse::Ok { mut result } => {
                if kind.is_none() && max.is_none() {
                    return self.ok(result);
                }
                let blocks = result["blocks"].as_array().cloned().unwrap_or_default();
                let (kept, outcome) = crate::select::select_list(
                    blocks,
                    None,
                    &[],
                    |_| None,
                    |block| kind.is_none_or(|kind| block["kind"].as_str() == Some(kind)),
                    max,
                );
                result["blocks"] = serde_json::Value::Array(kept);
                result["selection"] = serde_json::to_value(outcome).expect("selection serializes");
                self.ok(result)
            }
            other => other,
        }
    }

    /// `callGraph`: caller rules → callee subroutines. The selection fields
    /// are `caller` (a declared rule name), `callee` (a declared subroutine
    /// name), and `max` (#531).
    fn call_graph(
        &self,
        caller: Option<&str>,
        callee: Option<&str>,
        max: Option<usize>,
    ) -> ToolResponse {
        let program = &self.snapshot().loaded.program;
        if let Some(caller) = caller
            && !program.rules.iter().any(|rule| rule.name == caller)
        {
            return invalid_selection(format!("unknown caller rule '{caller}'"));
        }
        if let Some(callee) = callee
            && !program
                .subroutines
                .iter()
                .any(|subroutine| subroutine.name == callee)
        {
            return invalid_selection(format!("unknown callee subroutine '{callee}'"));
        }
        let edges = program
            .rules
            .iter()
            .filter(|rule| !rule.disabled)
            .flat_map(|rule| {
                wright_analyzer::canonical::live_actions(&rule.actions).filter_map(
                    move |(_, action)| {
                        wright_analyzer::canonical::subroutine_callee(action)
                            .map(|callee| json!({ "caller": rule.name, "callee": callee }))
                    },
                )
            })
            .collect::<Vec<_>>();
        self.select_result(
            "edges",
            serde_json::Value::Array(edges),
            None,
            |edge| {
                caller.is_none_or(|caller| edge["caller"].as_str() == Some(caller))
                    && callee.is_none_or(|callee| edge["callee"].as_str() == Some(callee))
            },
            max,
            caller.is_some() || callee.is_some() || max.is_some(),
        )
    }

    /// `lint`: per-rule id and effective severity, effective configuration,
    /// and findings over the loaded program through the same semantic-service
    /// path as the CLI `lint` workflow (no duplicated rule execution, #98).
    /// Full rule metadata is served once by `lintRules` rather than inlined
    /// into every `lint` response (#431). A `selection` member records the
    /// true total and withheld count when the request selected a subset
    /// (#430). Wire presentation omits configuration and caps findings.
    fn lint(&self, selection: &crate::select::FindingSelection) -> ToolResponse {
        if let Some(error) = self.selection_error(selection) {
            return error;
        }
        let service = self.configured_semantic();
        let lint_rules = match service.handle(&Request::LintRules) {
            Response::Ok { result } => result,
            Response::Error { .. } => serde_json::json!({}),
        };
        let findings = match service.handle(&Request::GetFindings) {
            Response::Ok { result } => result,
            Response::Error { .. } => serde_json::json!([]),
        };
        let findings = crate::session::prepare_findings(
            findings,
            service,
            &self.snapshot().loaded,
            self.session.catalog(),
        );
        let ok = !findings
            .iter()
            .any(|finding| finding["severity"] == "error");
        let (findings, outcome) = selection.apply_findings(
            findings,
            &crate::select::file_bases(&self.snapshot().loaded.input),
        );
        let (rules, config, skipped) = crate::result::split_lint_rules(&lint_rules);
        let mut result = json!({
            "inputIdentity": self.snapshot().loaded.input.identity,
            "ok": ok,
            "exit": if ok { 0 } else { crate::result::exit::SOURCE_ERROR },
            "rules": rules,
            "config": config,
            "findings": findings,
            "skipped": skipped,
        });
        if let Some(outcome) = outcome {
            result["selection"] = serde_json::to_value(outcome).expect("selection serializes");
        }
        self.ok(result)
    }

    fn project(&self) -> serde_json::Value {
        let loaded = &self.snapshot().loaded;
        let index = SemanticIndex::build(&loaded.program);
        let findings = wright_analyzer::canonical::analyze(
            &loaded.program,
            &wright_analyzer::registry::LintConfig::default(),
        );
        json!({
            "origin": { "kind": loaded.origin.kind, "locale": loaded.origin.locale },
            "inputIdentity": loaded.input.identity, "files": loaded.source_files.len().max(1),
            "sourceFiles": loaded.source_files.as_slice(), "globalVariables": loaded.program.global_variables.len(),
            "playerVariables": loaded.program.player_variables.len(), "subroutines": loaded.program.subroutines.len(),
            "rules": loaded.program.rules.len(), "symbols": index.symbols().count(), "findings": findings.len(),
        })
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

/// An unknown selection/filter value is a structured usage error, never a
/// silent empty result (#430, #531).
fn invalid_selection(message: String) -> ToolResponse {
    ToolResponse::Error {
        error: ToolErrorInfo {
            code: "invalid-selection".to_string(),
            message,
        },
    }
}

pub(crate) fn json_span_path(item: &serde_json::Value) -> Option<&str> {
    item.get("span")
        .and_then(|span| span.get("path"))
        .and_then(serde_json::Value::as_str)
}

#[cfg_attr(not(feature = "hotpath"), allow(dead_code))]
fn request_label(request: &ToolRequest) -> &'static str {
    match request {
        ToolRequest::Capabilities => "req::capabilities",
        ToolRequest::Compile { .. } => "req::compile",
        ToolRequest::Check { .. } => "req::check",
        ToolRequest::Analyze { .. } => "req::analyze",
        ToolRequest::Inspect { .. } => "req::inspect",
        ToolRequest::Project => "req::project",
        ToolRequest::Rules { .. } => "req::rules",
        ToolRequest::Symbols { .. } => "req::symbols",
        ToolRequest::References { .. } => "req::references",
        ToolRequest::Usage { .. } => "req::usage",
        ToolRequest::Cfg { .. } => "req::cfg",
        ToolRequest::Findings { .. } => "req::findings",
        ToolRequest::PersistentObjects { .. } => "req::persistentObjects",
        ToolRequest::Lint { .. } => "req::lint",
        ToolRequest::LintRules => "req::lintRules",
        ToolRequest::CallGraph { .. } => "req::callGraph",
        ToolRequest::CostEstimate { .. } => "req::costEstimate",
        ToolRequest::TargetMetadata => "req::targetMetadata",
        ToolRequest::ValidateEdit { .. } => "req::validateEdit",
        ToolRequest::ProviderValidateEdit { .. } => "req::providerValidateEdit",
        ToolRequest::EditStatements { .. } => "req::editStatements",
        ToolRequest::Lookup { .. } => "req::lookup",
        ToolRequest::ListEntries { .. } => "req::listEntries",
        ToolRequest::Simulate { .. } => "req::simulate",
    }
}
