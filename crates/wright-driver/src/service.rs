//! The session-aware tool service (M9, issues #57/#58).
//!
//! [`ToolService`] exposes Wright's compile/check/analyze/query workflows and
//! agent-oriented semantic queries over stable public contracts, reusing the
//! M6 driver session. It is transport-neutral: the stdio/JSON-RPC adapters
//! (M9 #60) and the embedding API are thin mappings over the same operations,
//! so behavior is testable in-process without a transport.
//!
//! Capability/version negotiation is provided by [`Capabilities`]; cost and
//! resource inspection ([`ToolRequest::CostEstimate`]) consumes the
//! Wright-owned generated-resource semantics established by the M8 benchmark
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
    /// Lint findings plus rule metadata and effective configuration (M12, #98).
    Lint,
    /// The registered lint rules and the effective lint configuration.
    LintRules,
    /// The subroutine call graph (caller rules → callee subroutines).
    CallGraph,
    /// Generated-resource cost estimates (exact counts + findings).
    CostEstimate,
    /// Target/catalog metadata (actions, values, events, enum domains).
    TargetMetadata,
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
            ToolRequest::Findings => {
                self.semantic_query(wright_analyzer::service::Request::GetFindings)
            }
            ToolRequest::Lint => self.lint(),
            ToolRequest::LintRules => self.semantic_query_with_config(
                wright_analyzer::service::Request::LintRules,
                self.session.config.lint.clone(),
            ),
            ToolRequest::CallGraph => self.ok(self.call_graph()),
            ToolRequest::CostEstimate => self.ok(self.cost_estimate()),
            ToolRequest::TargetMetadata => self.ok(self.target_metadata()),
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

    /// Run one M4 semantic query over the loaded program.
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
    /// `lint` workflow (no duplicated rule execution, M12 #98).
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
                let findings = match service.handle(&wright_analyzer::service::Request::GetFindings)
                {
                    wright_analyzer::service::Response::Ok { result } => result,
                    wright_analyzer::service::Response::Error { .. } => serde_json::json!([]),
                };
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
                if let Some(wright_ir::wir::Action::CallSubroutine { subroutine, .. }) =
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
    /// Compiler-host performance is measured by the M8 benchmark harness, not
    /// in-process.
    fn cost_estimate(&self) -> serde_json::Value {
        let catalog = wright_workshop::catalog::Catalog::builtin().expect("built-in catalog loads");
        let locale = self
            .loaded
            .origin
            .locale
            .clone()
            .map(|locale| wright_workshop::catalog::Locale::new(&locale))
            .unwrap_or_else(|| wright_workshop::catalog::Locale::new("en-US"));
        let text = wright_workshop::emitter::emit(&self.loaded.program, &catalog, &locale)
            .unwrap_or_default();
        let waits = self
            .loaded
            .program
            .actions
            .iter()
            .filter(|action| {
                matches!(action, wright_ir::wir::Action::Call { name, .. } if name == "wait")
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
        let catalog = match wright_workshop::catalog::Catalog::builtin() {
            Ok(catalog) => catalog,
            Err(error) => {
                return json!({ "error": error.to_string() });
            }
        };
        json!({
            "catalogVersion": catalog.schema_version,
            "locales": catalog.locales().iter().map(|l| l.to_string()).collect::<Vec<_>>(),
            "actions": catalog.entries_of(wright_workshop::catalog::Kind::Action).count(),
            "values": catalog.entries_of(wright_workshop::catalog::Kind::Value).count(),
            "events": catalog.entries_of(wright_workshop::catalog::Kind::Event).count(),
            "operators": catalog.entries_of(wright_workshop::catalog::Kind::Operator).count(),
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
