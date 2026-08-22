//! The session-aware tool service (issues #57/#58).
//!
//! [`ToolService`] exposes Wright's compile/check/analyze/query workflows and
//! agent-oriented semantic queries over stable public contracts, reusing the
//! driver session. It is transport-neutral: the stdio/JSON-RPC adapters
//! (#60) and the embedding API are thin mappings over the same operations,
//! so behavior is testable in-process without a transport.
//!
//! Capability/version negotiation is provided by [`Capabilities`]; cost and
//! resource inspection ([`ToolRequest::CostEstimate`]) consumes the
//! Wright-owned generated-resource semantics established by the `wright-bench`
//! harness (emitted bytes, WIR node counts, action/rule counts) and
//! distinguishes exact counts from static findings.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::diag::Diagnostic;
use crate::result::{AnalyzeResult, CheckResult, CompileResult, Envelope, InspectResult};
use crate::{CompilerSession, Loaded, RESULT_CONTRACT};

/// The tool-service name and version.
pub const SERVICE_NAME: &str = "wright-tool-service";
pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A tool request: the owned query surface plus agent-oriented operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum ToolRequest {
    /// Service identity, contract, and supported operations.
    Capabilities,
    /// The loaded program summary (origin, files, counts, findings).
    Project,
    /// Every rule.
    Rules,
    /// Symbols, optionally filtered by kind.
    Symbols {
        #[serde(default)]
        kind: Option<String>,
    },
    /// References to a symbol.
    References { symbol: u32 },
    /// Usage counts for a symbol.
    Usage { symbol: u32 },
    /// The control-flow graph of one rule.
    Cfg { rule: u32 },
    /// Every static-analysis finding.
    Findings,
    /// Lint findings plus rule metadata and effective configuration (#98).
    Lint,
    /// The registered lint rules and the effective lint configuration.
    LintRules,
    /// The subroutine call graph (caller rules → callee subroutines).
    CallGraph,
    /// Generated-resource cost estimates (exact counts + findings).
    CostEstimate,
    /// Target/catalog metadata (actions, values, events, enum domains).
    TargetMetadata,
    /// Validate and preview a caller-supplied source-edit transaction
    /// against the session's project (#130): atomic all-or-nothing
    /// semantics, structured refusal diagnostics, no filesystem writes.
    #[serde(rename = "validateEditTransaction")]
    ValidateEdit {
        /// The current text of every source the transaction touches, keyed
        /// by the same source identities the edits carry.
        sources: std::collections::BTreeMap<String, String>,
        transaction: crate::edit::EditTransaction,
    },
    /// Request a semantic rename through the shared refactoring contract
    /// (#129/#130): returns the validated exact-range transaction or
    /// structured refusal diagnostics. Wright proposes/validates; applying
    /// edits to disk is an explicit consumer responsibility.
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

/// A tool response: a structured owned result or a structured error.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResponse {
    Ok { result: serde_json::Value },
    Error { error: ToolErrorInfo },
}

/// A structured tool error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolErrorInfo {
    pub code: String,
    pub message: String,
}

/// The capability/version contract of the service.
#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub name: String,
    pub version: String,
    pub contract: String,
    pub operations: Vec<String>,
    pub languages: Vec<String>,
    pub profiles: Vec<String>,
}

/// The session-aware tool service.
pub struct ToolService<'a> {
    session: &'a mut CompilerSession,
    loaded: Loaded,
}

impl<'a> ToolService<'a> {
    /// Build the service over a session, loading the program eagerly.
    pub fn new(session: &'a mut CompilerSession) -> Result<ToolService<'a>, Diagnostic> {
        let loaded = session.load()?;
        Ok(ToolService { session, loaded })
    }

    /// The loaded program snapshot (origin, input identity, WIR program).
    pub fn loaded(&self) -> &Loaded {
        &self.loaded
    }

    /// The capability/version contract.
    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            name: SERVICE_NAME.to_string(),
            version: SERVICE_VERSION.to_string(),
            contract: RESULT_CONTRACT.to_string(),
            operations: vec![
                "capabilities",
                "project",
                "rules",
                "symbols",
                "references",
                "usage",
                "cfg",
                "findings",
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
            languages: vec![
                "opy".to_string(),
                "workshop".to_string(),
                "ostw".to_string(),
            ],
            profiles: vec![
                crate::Profile::Off.as_str().to_string(),
                crate::Profile::Compat.as_str().to_string(),
                crate::Profile::Aggressive.as_str().to_string(),
            ],
        }
    }

    /// Handle one tool request, returning a structured owned response.
    pub fn handle(&self, request: &ToolRequest) -> ToolResponse {
        match request {
            ToolRequest::Capabilities => ToolResponse::Ok {
                result: serde_json::to_value(self.capabilities()).expect("capabilities serialize"),
            },
            ToolRequest::Project => self.ok(self.project()),
            ToolRequest::Rules => self.semantic_query(wright_analyzer::service::Request::ListRules),
            ToolRequest::Symbols { kind } => {
                self.semantic_query(wright_analyzer::service::Request::ListSymbols {
                    kind: kind.clone(),
                })
            }
            ToolRequest::References { symbol } => {
                self.semantic_query(wright_analyzer::service::Request::FindReferences {
                    symbol: *symbol,
                })
            }
            ToolRequest::Usage { symbol } => {
                self.semantic_query(wright_analyzer::service::Request::GetUsage { symbol: *symbol })
            }
            ToolRequest::Cfg { rule } => {
                self.semantic_query(wright_analyzer::service::Request::GetCfg { rule: *rule })
            }
            ToolRequest::Findings => self.findings(),
            ToolRequest::Lint => self.lint(),
            ToolRequest::LintRules => self.semantic_query_with_config(
                wright_analyzer::service::Request::LintRules,
                self.session.config.lint.clone(),
            ),
            ToolRequest::CallGraph => self.ok(self.call_graph()),
            ToolRequest::CostEstimate => self.ok(self.cost_estimate()),
            ToolRequest::TargetMetadata => self.ok(self.target_metadata()),
            ToolRequest::ValidateEdit {
                sources,
                transaction,
            } => self.ok(serde_json::to_value(crate::edit::validate_transaction(
                &self.session.config,
                sources,
                transaction,
            ))
            .expect("edit validation serializes")),
            ToolRequest::SemanticRename { sources, target } => self.ok(serde_json::to_value(
                crate::edit::semantic_rename(&self.session.config, sources, target),
            )
            .expect("semantic rename serializes")),
            ToolRequest::ProviderSemanticRename {
                language_id,
                documents,
                position_document_uri,
                position,
                new_name,
                project_root,
                sources,
            } => {
                let request = crate::provider_edit::ProviderRenameRequest {
                    documents: documents.clone(),
                    position_document_uri: position_document_uri.clone(),
                    position: *position,
                    new_name: new_name.clone(),
                    project_root: project_root.clone(),
                    sources: sources.clone(),
                };
                self.ok(
                    serde_json::to_value(self.run_provider_flow(language_id, |provider| {
                        crate::provider_edit::semantic_rename(provider, &request)
                    }))
                    .expect("provider semantic rename serializes"),
                )
            }
            ToolRequest::ProviderValidateEdit {
                language_id,
                documents,
                transaction,
                sources,
                project_root,
            } => {
                let request = crate::provider_edit::ProviderValidateRequest {
                    documents: documents.clone(),
                    transaction: transaction.clone(),
                    sources: sources.clone(),
                    project_root: project_root.clone(),
                };
                self.ok(
                    serde_json::to_value(self.run_provider_flow(language_id, |provider| {
                        crate::provider_edit::validate_transaction(provider, &request)
                    }))
                    .expect("provider edit validation serializes"),
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
        self.session.inspect()
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

    fn error(&self, code: &str, message: String) -> ToolResponse {
        ToolResponse::Error {
            error: ToolErrorInfo {
                code: code.to_string(),
                message,
            },
        }
    }

    /// `findings`: every static-analysis finding with resolved span paths.
    ///
    /// The tool/agent surface resolves `span.path` exactly like the CLI
    /// `analyze`/`lint` workflows, so one file identity holds per finding
    /// across every surface (#102).
    fn findings(&self) -> ToolResponse {
        let response = self.semantic_query(wright_analyzer::service::Request::GetFindings);
        match response {
            ToolResponse::Ok { mut result } => {
                crate::session::resolve_finding_span_paths(&mut result, &self.loaded);
                ToolResponse::Ok { result }
            }
            other => other,
        }
    }

    /// Run one semantic query over the loaded program.
    fn semantic_query(&self, request: wright_analyzer::service::Request) -> ToolResponse {
        self.semantic_query_with_config(request, wright_analyzer::registry::LintConfig::default())
    }

    /// Run one semantic query over the loaded program with an explicit lint
    /// configuration.
    fn semantic_query_with_config(
        &self,
        request: wright_analyzer::service::Request,
        config: wright_analyzer::registry::LintConfig,
    ) -> ToolResponse {
        let origin = wright_analyzer::service::Origin {
            kind: self.loaded.origin.kind.clone(),
            locale: self.loaded.origin.locale.clone(),
        };
        match wright_analyzer::service::SemanticService::with_origin_and_config(
            &self.loaded.program,
            origin,
            config,
        ) {
            Ok(service) => match service.handle(&request) {
                wright_analyzer::service::Response::Ok { result } => ToolResponse::Ok { result },
                wright_analyzer::service::Response::Error { error } => ToolResponse::Error {
                    error: ToolErrorInfo {
                        code: error.code,
                        message: error.message,
                    },
                },
            },
            Err(error) => self.error("analysis-error", error.to_string()),
        }
    }

    /// `lint`: rule metadata, effective configuration, and findings over the
    /// loaded program through the same semantic-service path as the CLI
    /// `lint` workflow (no duplicated rule execution, #98).
    fn lint(&self) -> ToolResponse {
        let config = self.session.config.lint.clone();
        let origin = wright_analyzer::service::Origin {
            kind: self.loaded.origin.kind.clone(),
            locale: self.loaded.origin.locale.clone(),
        };
        match wright_analyzer::service::SemanticService::with_origin_and_config(
            &self.loaded.program,
            origin,
            config,
        ) {
            Ok(service) => {
                let lint_rules = match service.handle(&wright_analyzer::service::Request::LintRules)
                {
                    wright_analyzer::service::Response::Ok { result } => result,
                    wright_analyzer::service::Response::Error { .. } => serde_json::json!({}),
                };
                let mut findings =
                    match service.handle(&wright_analyzer::service::Request::GetFindings) {
                        wright_analyzer::service::Response::Ok { result } => result,
                        wright_analyzer::service::Response::Error { .. } => serde_json::json!([]),
                    };
                crate::session::resolve_finding_span_paths(&mut findings, &self.loaded);
                self.ok(json!({
                    "inputIdentity": self.loaded.input.identity,
                    "rules": lint_rules.get("rules").cloned().unwrap_or_else(|| json!([])),
                    "config": lint_rules.get("config").cloned().unwrap_or_else(|| json!({})),
                    "findings": findings,
                }))
            }
            Err(error) => self.error("analysis-error", error.to_string()),
        }
    }

    /// Program summary with origin and source identity.
    fn project(&self) -> serde_json::Value {
        let index = wright_analyzer::symbols::SemanticIndex::build(&self.loaded.program);
        let findings = wright_analyzer::analysis::analyze(&self.loaded.program);
        json!({
            "origin": {
                "kind": self.loaded.origin.kind,
                "locale": self.loaded.origin.locale,
            },
            "inputIdentity": self.loaded.input.identity,
            "files": self.loaded.program.files.len(),
            "globalVariables": self.loaded.program.global_variables.len(),
            "playerVariables": self.loaded.program.player_variables.len(),
            "subroutines": self.loaded.program.subroutines.len(),
            "rules": self.loaded.program.rules.len(),
            "symbols": index.map(|i| i.symbols().count()).unwrap_or(0),
            "findings": findings.len(),
        })
    }

    /// The subroutine call graph: every caller rule → callee subroutines.
    fn call_graph(&self) -> serde_json::Value {
        let mut edges: Vec<serde_json::Value> = Vec::new();
        for rule in self.loaded.program.rules.iter() {
            for action in &rule.actions {
                if let Some(workshop_rs::wir::Action::CallSubroutine { subroutine, .. }) =
                    self.loaded.program.actions.get(*action)
                {
                    let callee = self
                        .loaded
                        .program
                        .subroutines
                        .get(*subroutine)
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| "<dangling>".to_string());
                    edges.push(json!({
                        "caller": rule.name,
                        "callee": callee,
                    }));
                }
            }
        }
        serde_json::Value::Array(edges)
    }

    /// Generated-resource cost estimates.
    ///
    /// Exact counts: emitted bytes, WIR value/action/rule counts, and wait
    /// actions. Static indicators: analysis findings (e.g. `min-wait-loop`).
    /// Compiler-host performance is measured by the `wright-bench` harness, not
    /// in-process.
    fn cost_estimate(&self) -> serde_json::Value {
        let catalog = workshop_rs::catalog::Catalog::builtin().expect("built-in catalog loads");
        let locale = self
            .loaded
            .origin
            .locale
            .clone()
            .map(|locale| workshop_rs::catalog::Locale::new(&locale))
            .unwrap_or_else(|| workshop_rs::catalog::Locale::new("en-US"));
        let text =
            workshop_rs::emitter::emit(&self.loaded.program, &catalog, &locale).unwrap_or_default();
        let waits = self
            .loaded
            .program
            .actions
            .iter()
            .filter(|action| {
                matches!(action, workshop_rs::wir::Action::Call { name, .. } if name == "wait")
            })
            .count();
        let findings = wright_analyzer::analysis::analyze(&self.loaded.program);
        json!({
            "exact": {
                "emittedBytes": text.len(),
                "wirValues": self.loaded.program.values.len(),
                "wirActions": self.loaded.program.actions.len(),
                "wirRules": self.loaded.program.rules.len(),
                "waitActions": waits,
            },
            "findings": findings.iter().map(|finding| json!({
                "code": finding.code,
                "severity": severity_name(finding.severity),
                "message": finding.message,
            })).collect::<Vec<_>>(),
            "kind": {
                "exact": "exact target-resource counts",
                "findings": "static/heuristic execution indicators",
                "performance": "compiler-host performance is measured by wright-bench, not in-process",
            },
        })
    }

    /// Target/catalog metadata for reasoning about Workshop operations.
    fn target_metadata(&self) -> serde_json::Value {
        let catalog = match workshop_rs::catalog::Catalog::builtin() {
            Ok(catalog) => catalog,
            Err(error) => {
                return json!({ "error": error.to_string() });
            }
        };
        json!({
            "catalogVersion": catalog.schema_version,
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

/// The canonical severity name of a finding.
fn severity_name(severity: wright_analyzer::analysis::Severity) -> &'static str {
    match severity {
        wright_analyzer::analysis::Severity::Warning => "warning",
        wright_analyzer::analysis::Severity::Info => "info",
    }
}
