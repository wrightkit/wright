use std::path::Path;

use super::{CompilerSession, Loaded, Provenance, ProviderOperation, root_relative};
use wright_analyzer::canonical::SemanticService;
use wright_analyzer::service::Request;

use crate::diag::{Diagnostic, Stage};
use crate::progress::{ProgressEvent, ProgressPhase, ProgressUnit};
use crate::result::{
    AnalyzeResult, CallGraphResult, CfgResult, CostResult, Envelope, InspectResult, LintResult,
    RefsResult, SymbolsResult,
};
use crate::service::{Address, ToolErrorInfo, ToolRequest, ToolResponse, ToolService};

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
        // Persistent Workshop object facts join the semantic report rather
        // than living behind a query-only operation (#429).
        "persistentObjects": service_response(service, &Request::GetPersistentObjects),
    })
}

fn inspect_result(service: &SemanticService<'_>) -> InspectResult {
    InspectResult {
        program: service_response(service, &Request::Program),
        rules: service_response(service, &Request::ListRules),
        symbols: service_response(service, &Request::ListSymbols { kind: None }),
        references: service.references_for_all_symbols(),
    }
}

/// Unwrap a [`ToolResponse`] into its result or its structured error.
fn tool_result(response: ToolResponse) -> Result<serde_json::Value, ToolErrorInfo> {
    match response {
        ToolResponse::Ok { result } => Ok(result),
        ToolResponse::Error { error } => Err(error),
    }
}

/// Add the resolved `path` to every semantic `span` in a JSON result.
///
/// File 0 is the main input and resolves root-relative to the include root
/// (`--root`, defaulting to the input's directory); other files resolve from
/// the retained frontend file registry. `<file N>` is the fallback when no
/// registry entry resolves, and stdin inputs fall back to `<stdin>`.
pub(crate) fn resolve_span_paths(value: &mut serde_json::Value, loaded: &Loaded) {
    match value {
        serde_json::Value::Object(object) => {
            if let Some(span) = object
                .get_mut("span")
                .and_then(serde_json::Value::as_object_mut)
            {
                let file = span.get("file").and_then(serde_json::Value::as_u64);
                span.insert(
                    "path".to_string(),
                    serde_json::Value::String(span_path(file, loaded)),
                );
            }
            object
                .values_mut()
                .for_each(|value| resolve_span_paths(value, loaded));
        }
        serde_json::Value::Array(items) => items
            .iter_mut()
            .for_each(|value| resolve_span_paths(value, loaded)),
        _ => {}
    }
}

fn span_path(file: Option<u64>, loaded: &Loaded) -> String {
    if loaded.provenance == Provenance::Unmapped {
        "<provider-artifact>".to_string()
    } else if loaded.provenance == Provenance::Mapped {
        // Every mapped file is an authored source; file 0 is not the input.
        match file.and_then(|file| loaded.source_files.get(file as usize)) {
            Some(source) => root_relative(Some(Path::new(source)), &loaded.input.root)
                .unwrap_or_else(|| source.clone()),
            None => "<provider-artifact>".to_string(),
        }
    } else if let Some(file) = file {
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
    }
}

impl CompilerSession {
    pub fn analyze(&mut self) -> Envelope<AnalyzeResult> {
        self.with_loaded(
            "analyze",
            |session| session.load_with_operation(ProviderOperation::Compile),
            |session, loaded| {
                let service = session.service(&loaded);
                session.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
                let mut program = service_response(&service, &Request::Program);
                if let serde_json::Value::Object(object) = &mut program {
                    object.remove("findings");
                }
                let mut facts = semantic_facts(&service);
                if loaded.provenance == Provenance::Mapped {
                    resolve_span_paths(&mut facts, &loaded);
                }
                AnalyzeResult { program, facts }
            },
        )
    }

    /// `inspect`: load and produce the structural/semantic program model.
    pub fn inspect(&mut self) -> Envelope<InspectResult> {
        self.with_loaded(
            "inspect",
            |session| session.load(),
            |session, loaded| {
                let service = session.service(&loaded);
                session.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
                inspect_result(&service)
            },
        )
    }

    /// `symbols` (#429): the `symbols` operation's payload; `kind` narrows to
    /// one symbol kind.
    pub fn symbols(&mut self, kind: Option<String>) -> Envelope<SymbolsResult> {
        self.service_query("symbols", move |service| {
            tool_result(service.handle(&ToolRequest::Symbols { kind })).map(SymbolsResult)
        })
    }

    /// `refs` (#429): the `usage` operation's payload — the header counts —
    /// plus the `references` operation's payload under `references`, for one
    /// symbol addressed by name.
    pub fn refs(&mut self, name: &str) -> Envelope<RefsResult> {
        let symbol = Address::Name(name.to_string());
        self.service_query("refs", move |service| {
            let references = tool_result(service.handle(&ToolRequest::References {
                symbol: symbol.clone(),
            }))?;
            let usage = tool_result(service.handle(&ToolRequest::Usage { symbol }))?;
            let mut result = usage.as_object().cloned().unwrap_or_default();
            result.insert("references".to_string(), references);
            Ok(RefsResult(serde_json::Value::Object(result)))
        })
    }

    /// `cfg` (#429): the `cfg` operation's payload for one rule, addressed by
    /// name.
    pub fn cfg(&mut self, rule: &str) -> Envelope<CfgResult> {
        let rule = Address::Name(rule.to_string());
        self.service_query("cfg", move |service| {
            tool_result(service.handle(&ToolRequest::Cfg { rule })).map(CfgResult)
        })
    }

    /// `callgraph` (#429): the `callGraph` operation's payload.
    pub fn callgraph(&mut self) -> Envelope<CallGraphResult> {
        self.service_query("callgraph", |service| {
            tool_result(service.handle(&ToolRequest::CallGraph)).map(CallGraphResult)
        })
    }

    /// `cost` (#429): the `costEstimate` operation's payload; the session's
    /// finding selection applies exactly as on the agent surface.
    pub fn cost(&mut self) -> Envelope<CostResult> {
        let selection = self.config.selection.clone();
        self.service_query("cost", move |service| {
            tool_result(service.handle(&ToolRequest::CostEstimate(selection))).map(CostResult)
        })
    }

    /// Run one query operation through the session's [`ToolService`] — the
    /// same dispatch the agent surface uses, so CLI payloads equal the
    /// operation payloads by construction, including shared name resolution
    /// (#429). A structured service error becomes an analysis-stage
    /// diagnostic; a failed envelope carries a `null` result.
    fn service_query<T>(
        &mut self,
        command: &str,
        run: impl FnOnce(&mut ToolService<'_>) -> Result<T, ToolErrorInfo>,
    ) -> Envelope<T>
    where
        T: Default + serde::Serialize,
    {
        self.with_loaded(
            command,
            |session| session.load(),
            |session, _loaded| {
                session.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
                let mut service = match ToolService::new(session) {
                    Ok(service) => service,
                    Err(diagnostic) => {
                        session.diagnostics.push(diagnostic);
                        return T::default();
                    }
                };
                match run(&mut service) {
                    Ok(result) => result,
                    Err(error) => {
                        session.diagnostics.push(Diagnostic::error(
                            error.code,
                            Stage::Analysis,
                            error.message,
                        ));
                        T::default()
                    }
                }
            },
        )
    }

    pub(crate) fn inspect_loaded(
        &mut self,
        loaded: Loaded,
        service: &SemanticService<'_>,
    ) -> Envelope<InspectResult> {
        self.with_loaded(
            "inspect",
            |_| Ok(loaded),
            |session, _loaded| {
                session.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
                inspect_result(service)
            },
        )
    }

    /// `lint`: load and produce the source identity, program summary, per-rule
    /// id and effective severity, effective configuration, and findings (#98).
    /// Full rule metadata is served by `lintRules` rather than inlined into
    /// every `lint` result (#431).
    ///
    /// Lint rule findings are reported in `result.findings`; frontend and
    /// Workshop semantic-completeness diagnostics remain in the envelope.
    /// Rule enable/disable and severity come from `self.config.lint`, the same
    /// configuration the CLI flags and programmatic consumers set.
    pub fn lint(&mut self) -> Envelope<LintResult> {
        self.with_loaded(
            "lint",
            |session| session.load_with_operation(ProviderOperation::Compile),
            |session, loaded| {
                session.attach_workshop_completeness(&loaded);
                let service = session.service_with(&loaded, session.config.lint.clone());
                session.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
                let program = service_response(&service, &Request::Program);
                let lint_rules = service_response(&service, &Request::LintRules);
                let lint_rule_count = lint_rules
                    .pointer("/rules")
                    .and_then(serde_json::Value::as_array)
                    .map_or(0, Vec::len);
                session.progress(ProgressEvent::with_count(
                    ProgressPhase::Linting,
                    lint_rule_count,
                    ProgressUnit::Rules,
                ));
                let mut findings = service_response(&service, &Request::GetFindings);
                resolve_span_paths(&mut findings, &loaded);
                let findings = match findings {
                    serde_json::Value::Array(findings) => findings,
                    _ => Vec::new(),
                };
                let (findings, selection) = session
                    .config
                    .selection
                    .apply_findings(findings, &crate::select::file_bases(&loaded.input));
                let (rules, config, skipped) =
                    if let serde_json::Value::Object(mut object) = lint_rules {
                        (
                            crate::result::compact_lint_rules(object.remove("rules").as_ref()),
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
                LintResult {
                    input_identity: loaded.input.identity.clone(),
                    program,
                    rules,
                    config,
                    findings: serde_json::Value::Array(findings),
                    skipped,
                    selection,
                }
            },
        )
    }
}
