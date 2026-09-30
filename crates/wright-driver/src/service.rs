//! [`ToolService`] exposes Wright's compile/check/analyze/query workflows and
//! agent-oriented semantic queries over stable public contracts, reusing the
//! driver session. It is transport-neutral: the stdio/JSON-RPC adapters
//! (#60) and the embedding API are thin mappings over the same operations,
//! so behavior is testable in-process without a transport.
//!
//! Capability/version negotiation is provided by [`Capabilities`]; cost and
//! resource inspection ([`ToolRequest::CostEstimate`]) consumes the
//! Wright-owned generated-resource semantics established by the `wright-bench`
//! harness (emitted bytes, canonical program counts, action/rule counts) and
//! distinguishes exact counts from static findings.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::diag::Diagnostic;
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
        /// by the same source identities the edits carry.
        sources: std::collections::BTreeMap<String, String>,
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
        /// the same source identities the target names.
        sources: std::collections::BTreeMap<String, String>,
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
    loaded: Loaded,
    semantic: SemanticService<'static>,
    lint_semantic: Option<SemanticService<'static>>,
}

impl<'a> ToolService<'a> {
    /// Build the service over a session, loading the program eagerly.
    pub fn new(session: &'a mut CompilerSession) -> Result<ToolService<'a>, Diagnostic> {
        let loaded = session.load()?;
        let semantic =
            session.shared_service_with(&loaded, wright_analyzer::registry::LintConfig::default());
        let lint_semantic = (!session.config.lint.rules.is_empty())
            .then(|| semantic.with_lint_config(session.config.lint.clone()));
        Ok(ToolService {
            session,
            loaded,
            semantic,
            lint_semantic,
        })
    }

    /// The loaded program snapshot (origin, input identity, canonical program).
    pub fn loaded(&self) -> &Loaded {
        &self.loaded
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
    pub fn handle(&mut self, request: &ToolRequest) -> ToolResponse {
        match request {
            ToolRequest::Capabilities => ToolResponse::Ok {
                result: serde_json::to_value(self.capabilities()).expect("capabilities serialize"),
            },
            ToolRequest::Compile => {
                let result =
                    serde_json::to_value(self.compile()).expect("compile result serializes");
                ToolResponse::Ok { result }
            }
            ToolRequest::Check => {
                let result = serde_json::to_value(self.check()).expect("check result serializes");
                ToolResponse::Ok { result }
            }
            ToolRequest::Analyze => {
                let result =
                    serde_json::to_value(self.analyze()).expect("analyze result serializes");
                ToolResponse::Ok { result }
            }
            ToolRequest::Inspect => {
                let result =
                    serde_json::to_value(self.inspect()).expect("inspect result serializes");
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
                self.session.validate_edit_transaction(sources, transaction),
            )
            .expect("serializes")),
            ToolRequest::SemanticRename { sources, target } => {
                let rename = self.session.semantic_rename(sources, target);
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
    pub fn compile(&mut self) -> Envelope<CompileResult> {
        self.session.compile()
    }

    /// Check through the shared session pipeline.
    pub fn check(&mut self) -> Envelope<CheckResult> {
        self.session.check()
    }

    /// Analyze through the shared session pipeline.
    pub fn analyze(&mut self) -> Envelope<AnalyzeResult> {
        self.session.analyze()
    }

    /// Inspect through the shared session pipeline.
    pub fn inspect(&mut self) -> Envelope<InspectResult> {
        self.session
            .inspect_loaded(self.loaded.clone(), &self.semantic)
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

    /// Run a provider-driven mutation flow (#139) over a fresh provider
    /// session: spawn by opaque language id, initialize, run the flow, and
    /// terminate gracefully.
    ///
    /// Any failure before the flow — an unconfigured language id, a spawn
    /// failure, a failed handshake — is the same structured
    /// [`crate::provider_edit::ProviderMutation`] refusal surface the flow
    /// itself uses, so callers handle one refusal contract. The provider
    /// process never outlives the request: graceful shutdown when possible,
    /// and the session's drop guard terminates it otherwise.
    fn run_provider_flow(
        &self,
        language_id: &str,
        flow: impl FnOnce(
            &mut dyn wright_lpp::LanguageProvider,
        ) -> crate::provider_edit::ProviderMutation,
    ) -> crate::provider_edit::ProviderMutation {
        let mut provider = match self.session.language_provider(language_id) {
            Ok(provider) => provider,
            Err(error) => return crate::provider_edit::provider_failure(&error),
        };
        if let Err(error) = provider.initialize(Some(&wright_lpp::ClientInfo {
            name: SERVICE_NAME.to_string(),
            version: SERVICE_VERSION.to_string(),
        })) {
            return crate::provider_edit::provider_failure(&error);
        }
        let mutation = flow(provider.as_mut());
        let _ = provider.shutdown();
        mutation
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
                    .apply_findings(findings, &crate::select::file_bases(&self.loaded.input));
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
                .semantic
                .resolve_symbol(name)
                .map(|symbol| symbol.id.index() as u32),
        }
    }

    /// Resolve a rule `Address` to its numeric index (#429).
    fn rule_address(&self, address: &Address) -> Result<u32, ToolErrorInfo> {
        match address {
            Address::Id(id) => Ok(*id),
            Address::Name(name) => self.semantic.resolve_rule(name).map(|rule| rule as u32),
        }
    }

    fn semantic_query_with_resolved_span_paths(&self, request: Request) -> ToolResponse {
        match self.semantic_query(request) {
            ToolResponse::Ok { mut result } => {
                crate::session::resolve_span_paths(&mut result, &self.loaded);
                ToolResponse::Ok { result }
            }
            other => other,
        }
    }

    /// Run one semantic query over the loaded program.
    fn semantic_query(&self, request: Request) -> ToolResponse {
        self.semantic.handle(&request)
    }

    fn configured_semantic(&self) -> &SemanticService<'static> {
        self.lint_semantic.as_ref().unwrap_or(&self.semantic)
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
        crate::session::resolve_span_paths(&mut findings, &self.loaded);
        let findings = match findings {
            serde_json::Value::Array(findings) => findings,
            _ => Vec::new(),
        };
        let (findings, outcome) =
            selection.apply_findings(findings, &crate::select::file_bases(&self.loaded.input));
        let mut result = json!({
            "inputIdentity": self.loaded.input.identity,
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
        let index = SemanticIndex::build(&self.loaded.program);
        let findings = wright_analyzer::canonical::analyze(
            &self.loaded.program,
            &wright_analyzer::registry::LintConfig::default(),
        );
        json!({
            "origin": { "kind": self.loaded.origin.kind, "locale": self.loaded.origin.locale },
            "inputIdentity": self.loaded.input.identity,
            "files": self.loaded.source_files.len().max(1),
            "globalVariables": self.loaded.program.global_variables.len(),
            "playerVariables": self.loaded.program.player_variables.len(),
            "subroutines": self.loaded.program.subroutines.len(),
            "rules": self.loaded.program.rules.len(),
            "symbols": index.symbols().count(),
            "findings": findings.len(),
        })
    }

    fn call_graph(&self) -> serde_json::Value {
        let edges = self
            .loaded
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
        let locale = CompilerSession::locale_for(&self.loaded);
        let text =
            workshop_rs::emitter::emit(&self.loaded.program, self.session.catalog(), &locale)
                .unwrap_or_default();
        let waits = self
            .loaded
            .program
            .rules
            .iter()
            .flat_map(|r| &r.actions)
            .filter(|a| matches!(a, workshop_rs::Action::Call { name, .. } if name == "wait"))
            .count();
        let findings = wright_analyzer::canonical::analyze(
            &self.loaded.program,
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
            selection.apply_findings(findings, &crate::select::file_bases(&self.loaded.input));
        let mut result = json!({
            "exact": {
                "emittedBytes": text.len(),
                "programActions": self.loaded.program.rules.iter().map(|r| r.actions.len()).sum::<usize>(),
                "programRules": self.loaded.program.rules.len(),
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
