use serde_json::json;

use crate::diag::Diagnostic;
use crate::result::{
    AnalyzeResult, CheckResult, CompileResult, Envelope, FindingMeta, InspectResult,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::session::Provenance;
use crate::session::{findings_with_meta, select_findings};
#[cfg(not(target_arch = "wasm32"))]
use crate::source_provider::document_sources;
use crate::{CompilerSession, Loaded};
use wright_analyzer::canonical::{BLOCK_KINDS, ReferenceKind, SemanticService, SymbolKind};
/// A structured tool error.
pub use wright_analyzer::service::ErrorInfo as ToolErrorInfo;
/// A tool response: a structured owned result or a structured error.
pub use wright_analyzer::service::Response as ToolResponse;
use wright_analyzer::service::{Request, Response};

mod callgraph;
mod capabilities;
mod cost;
mod edit_statements;
mod refusal;
pub(crate) mod settings;
mod simulate;
mod stale_ids;
mod state;

/// The tool-service name and version.
pub const SERVICE_NAME: &str = "wright-tool-service";
pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const AGENT_CONTRACT: &str = "wright-agent/v1";

const DEFAULT_CONTENT_RULES: usize = 20;

pub use capabilities::{Capabilities, OPERATIONS};
/// Compose a refusal's structured fields into the surface's own
/// vocabulary (#888).
pub use refusal::compose_fields;
pub(crate) use refusal::{
    carried_refusal, invalid_selection, invalid_selection_error, invalid_selection_fields, refusal,
    resolve_candidates, usage_refusal,
};

pub use crate::request::{
    Address, StatementEditChange, StatementEditRequest, StatementList, ToolRequest,
};

pub use state::ServiceState;
use state::Snapshot;

/// The session-aware tool service.
pub struct ToolService<'a> {
    session: &'a mut CompilerSession,
    snapshot: Option<Snapshot>,
    generation: u64,
    reloads: usize,
    invalidated: bool,
    selected_meta: Vec<FindingMeta>,
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
            selected_meta: Vec::new(),
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

    pub(crate) fn handle_full(&mut self, request: &ToolRequest) -> ToolResponse {
        if let ToolRequest::Rules { name: Some(_), .. } = request {
            return refusal::refusal(
                "malformed-request",
                "`rules` no longer takes `name`; address one rule with `rule` or list them all",
            );
        }
        match request {
            ToolRequest::Check { .. } | ToolRequest::Compile { .. } => self.refresh_workflow(),
            _ if Self::reads_program(request) => {
                if let ToolRequest::EditStatements { .. } = request
                    && let Some(response) = self.edit_statements_kind_gate()
                {
                    return response;
                }
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

    #[cfg(not(target_arch = "wasm32"))]
    fn provider_documents(
        &self,
        language_id: &str,
        documents: &Option<wright_lpp::DocumentSet>,
    ) -> Result<wright_lpp::DocumentSet, ToolErrorInfo> {
        if let Some(documents) = documents {
            return Ok(documents.clone());
        }
        let loaded = &self.snapshot().loaded;
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

    fn stamp_snapshot(&self, request: &ToolRequest, mut response: ToolResponse) -> ToolResponse {
        let ToolResponse::Ok { result } = &mut response else {
            return response;
        };
        let list_key = match request {
            ToolRequest::Rules { .. } => Some("rules"),
            ToolRequest::Symbols { .. } => Some("symbols"),
            ToolRequest::Findings { .. } => Some("findings"),
            ToolRequest::PersistentObjects { .. } => Some("persistentObjects"),
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
            // `compile` writes the emitted program to an output file —
            // the WebAssembly build has no filesystem to write one
            // (#959); the refusal names the absent capability.
            #[cfg(target_arch = "wasm32")]
            ToolRequest::Compile { .. } => refusal(
                "output-unavailable",
                "the WebAssembly build cannot write compile output to a file — compile runs on the native surfaces",
            ),
            #[cfg(not(target_arch = "wasm32"))]
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
                if let Err(error) = self.selection_check(selection) {
                    return ToolResponse::Error { error };
                }
                let (loaded, semantic) = self.snapshot_and_semantic();
                let envelope = self.session.analyze_loaded(loaded, &semantic, selection);
                self.selected_meta = envelope.result.risks_meta.clone();
                let result = serde_json::to_value(envelope).expect("analyze result serializes");
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
                rule,
                file,
                max,
                content,
                all,
                ..
            } => self.rules(
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
            ToolRequest::Lint { selection, all: _ } => {
                let (response, meta) = self.lint(selection);
                self.selected_meta = meta;
                response
            }
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
            // `providerValidateEdit` needs an LPP provider transport —
            // native-only (ADR-0037 decision 5); the wasm build refuses
            // with the absent capability named.
            #[cfg(target_arch = "wasm32")]
            ToolRequest::ProviderValidateEdit { .. } => refusal(
                "source-provider-unavailable",
                "'providerValidateEdit' requires the LPP provider transport — the WebAssembly build carries none",
            ),
            #[cfg(not(target_arch = "wasm32"))]
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
                self.ok(serde_json::to_value(self.session.run_provider_flow(
                    language_id,
                    &wright_lpp::ClientInfo {
                        name: SERVICE_NAME.to_string(),
                        version: SERVICE_VERSION.to_string(),
                    },
                    |provider| crate::provider_edit::validate_transaction(provider, &req),
                ))
                .expect("serializes"))
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
    #[cfg(not(target_arch = "wasm32"))]
    pub fn language_provider(
        &self,
        language_id: &str,
    ) -> Result<Box<dyn wright_lpp::LanguageProvider>, wright_lpp::ProviderError> {
        self.session.language_provider(language_id)
    }

    fn ok(&self, result: serde_json::Value) -> ToolResponse {
        ToolResponse::Ok { result }
    }

    fn findings(&self, selection: &crate::select::FindingSelection) -> ToolResponse {
        if let Err(error) = self.selection_check(selection) {
            return ToolResponse::Error { error };
        }
        let (findings, _meta, outcome) = select_findings(
            self.semantic(),
            &self.snapshot().loaded,
            self.session.catalog(),
            selection,
            findings_with_meta(self.semantic()),
        );
        match outcome {
            Some(outcome) => self.ok(json!({ "findings": findings, "selection": outcome })),
            None => self.ok(serde_json::Value::Array(findings)),
        }
    }

    fn persistent_objects(&self) -> ToolResponse {
        self.semantic_query_with_resolved_span_paths(Request::GetPersistentObjects)
    }

    fn symbol_address(&self, address: &Address) -> Result<u32, ToolErrorInfo> {
        match address {
            Address::Id(id) => Ok(*id),
            Address::Name(name) => self
                .semantic()
                .resolve_symbol(name)
                .map(|symbol| symbol.id.index() as u32),
        }
    }

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

    fn selection_check(
        &self,
        selection: &crate::select::FindingSelection,
    ) -> Result<(), ToolErrorInfo> {
        selection
            .validate(self.session.lint_registry())
            .map_err(|message| invalid_selection_error(message, Default::default()))?;
        let loaded = &self.snapshot().loaded;
        if let Some(file) = selection.file.as_deref()
            && let Some((message, fields)) = crate::select::file_error(
                file,
                &loaded.source_files,
                &crate::select::file_bases(&loaded.input),
            )
        {
            return Err(invalid_selection_error(message, fields));
        }
        Ok(())
    }

    fn select_result(
        &self,
        key: &str,
        result: serde_json::Value,
        file: Option<&str>,
        keep: impl Fn(&serde_json::Value) -> bool,
        max: Option<usize>,
        selected: bool,
    ) -> ToolResponse {
        let loaded = &self.snapshot().loaded;
        let file_bases = crate::select::file_bases(&loaded.input);
        if let Some(file) = file
            && let Some((message, fields)) =
                crate::select::file_error(file, &loaded.source_files, &file_bases)
        {
            return invalid_selection_fields(message, fields);
        }
        let items = match result {
            serde_json::Value::Array(items) => items,
            _ => Vec::new(),
        };
        let (kept, outcome) =
            crate::select::select_list(items, file, &file_bases, json_span_path, keep, max);
        if !selected {
            return self.ok(serde_json::Value::Array(kept));
        }
        self.ok(json!({ key: kept, "selection": outcome }))
    }

    fn rules(
        &self,
        rule: Option<&Address>,
        file: &Option<String>,
        max: &Option<usize>,
        content: bool,
    ) -> ToolResponse {
        let rule_id = match rule {
            Some(address) => match self.rule_address(address) {
                Ok(id) if (id as usize) < self.snapshot().loaded.program.rules.len() => {
                    Some(id as u64)
                }
                Ok(id) => return invalid_selection(format!("unknown rule {id}")),
                Err(error) => return ToolResponse::Error { error },
            },
            None => None,
        };
        let max = if content {
            max.or(Some(DEFAULT_CONTENT_RULES))
        } else {
            *max
        };
        let selected = content || rule_id.is_some() || file.is_some() || max.is_some();
        let response = self.semantic_query_with_resolved_span_paths(Request::ListRules);
        let response = match response {
            ToolResponse::Ok { result } => self.select_result(
                "rules",
                result,
                file.as_deref(),
                |item| rule_id.is_none_or(|id| item["id"].as_u64() == Some(id)),
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

    fn symbols(&self, kind: Option<&str>, file: Option<&str>, max: Option<usize>) -> ToolResponse {
        if let Some(kind) = kind
            && !SymbolKind::ALL.iter().any(|known| known.as_str() == kind)
        {
            return invalid_selection(format!(
                "unknown symbol kind '{}' (expected one of: {})",
                kind,
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
                "unknown reference kind '{}' (expected one of: {})",
                kind,
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
                Err(error) => {
                    return invalid_selection_fields(error.message, error.fields);
                }
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

    fn cfg(&self, rule: u32, kind: Option<&str>, max: Option<usize>) -> ToolResponse {
        if let Some(kind) = kind
            && !BLOCK_KINDS.contains(&kind)
        {
            return invalid_selection(format!(
                "unknown cfg block kind '{}' (expected one of: {})",
                kind,
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

    /// Lint through the shared session pipeline (#1002): the session's
    /// `lint` envelope — verdict, diagnostics, and findings over the
    /// served snapshot — serialized as the response `result`, so the
    /// `lint` operation and `wright lint` report the same `ok`/`exit`
    /// for one input and configuration. `selection` is the request's
    /// inline finding selection (the session's configured selection does
    /// not apply to a `lint` request).
    fn lint(
        &mut self,
        selection: &crate::select::FindingSelection,
    ) -> (ToolResponse, Vec<FindingMeta>) {
        if let Err(error) = self.selection_check(selection) {
            return (ToolResponse::Error { error }, Vec::new());
        }
        let (loaded, semantic) = self.snapshot_and_semantic();
        let envelope = self.session.lint_loaded(loaded, &semantic, selection);
        let meta = envelope.result.findings_meta.clone();
        let result = serde_json::to_value(envelope).expect("lint result serializes");
        (self.ok(result), meta)
    }

    fn project(&self) -> serde_json::Value {
        let loaded = &self.snapshot().loaded;
        let findings = wright_analyzer::canonical::analyze(
            &loaded.program,
            &wright_analyzer::registry::LintConfig::default(),
        );
        json!({
            "origin": { "kind": loaded.origin.kind, "locale": loaded.origin.locale },
            "inputIdentity": loaded.input.identity, "files": loaded.source_files.len().max(1),
            "sourceFiles": loaded.source_files.as_slice(), "globalVariables": loaded.program.global_variables.len(),
            "playerVariables": loaded.program.player_variables.len(), "subroutines": loaded.program.subroutines.len(),
            "rules": loaded.program.rules.len(), "symbols": self.semantic().symbol_count(), "findings": findings.len(),
            "settings": loaded.program.settings.as_ref().map(settings::settings_json),
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

/// The `lintRules` payload split into its findings-surface members; a
/// failed fetch splits the empty object.
fn lint_rules_split(
    service: &SemanticService<'_>,
) -> (serde_json::Value, serde_json::Value, serde_json::Value) {
    let lint_rules = match service.handle(&Request::LintRules) {
        Response::Ok { result } => result,
        Response::Error { .. } => serde_json::json!({}),
    };
    crate::result::split_lint_rules(&lint_rules)
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
