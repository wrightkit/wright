use std::collections::BTreeSet;
use std::path::Path;

use super::{CompilerSession, Loaded, Provenance, ProviderOperation, root_relative};
use workshop_rs::actions::ElementNodeKind;
use workshop_rs::catalog::Catalog;
use workshop_rs::source::Span;
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

/// Build the `analyze` report from existing semantic query surfaces plus the
/// canonical Workshop element count (#445).
///
/// Keeping this composition here makes the product boundary explicit: the
/// report layers Workshop cost/size, structural complexity, and
/// performance/stability risk indicators over symbol usage and CFG
/// measurements. The element count is the `workshop-rs`-owned canonical
/// computation; the risk layer reuses registry findings narrowed to rules
/// that declare a `performance` or `stability` tag, so correctness findings
/// stay out of the risk frame. Lint rules remain owned by `LintRegistry` and
/// are only exposed in full by `lint`/`findings` queries.
#[hotpath::measure]
fn semantic_facts(
    service: &SemanticService<'_>,
    loaded: &Loaded,
    catalog: &Catalog,
) -> serde_json::Value {
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

    // `Ok` carries the per-rule tree used below; `Err` only changes the cost
    // section — an unavailable count must not erase the rest of the report.
    let element_count = loaded.program.element_count(catalog);

    let rules = service_response(service, &Request::ListRules)
        .as_array()
        .map(|rules| {
            rules
                .iter()
                .enumerate()
                .map(|(index, r)| {
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
                    let mut entry = serde_json::json!({
                        "id": r["id"],
                        "name": r["name"],
                        "span": r.get("span").cloned().unwrap_or(serde_json::Value::Null),
                        "controlFlow": {
                            "blocks": blocks.len(),
                            "edges": edge_count,
                            "loopBlocks": loop_blocks,
                            "waitBlocks": wait_blocks,
                        },
                    });
                    // Report rules keep canonical source/WIR order, so the
                    // element node at `index` belongs to this rule.
                    if let Ok(report) = &element_count {
                        if let Some(node) = report.rules.get(index) {
                            let mut condition_elements = 0usize;
                            let mut conditions = Vec::new();
                            for child in &node.children {
                                if child.kind != ElementNodeKind::Condition {
                                    continue;
                                }
                                condition_elements += child.count;
                                conditions.push(serde_json::json!({
                                    "index": conditions.len(),
                                    "elements": child.count,
                                    "span": fact_span_json(child.span),
                                }));
                            }
                            entry["elements"] = serde_json::json!(node.count);
                            entry["conditionElements"] = serde_json::json!(condition_elements);
                            entry["conditions"] = serde_json::Value::Array(conditions);
                        }
                    }
                    entry
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let cost = cost_facts(&loaded.program, &element_count);
    let risks = risk_facts(service);

    serde_json::json!({
        "symbols": symbols,
        "rules": rules,
        "cost": cost,
        "risks": risks,
        // Persistent Workshop object facts join the semantic report rather
        // than living behind a query-only operation (#429).
        "persistentObjects": service_response(service, &Request::GetPersistentObjects),
    })
}

/// The `{file,start,end}` span shape every other `facts` entry carries, so
/// `resolve_span_paths` maps element-count locations the same way.
fn fact_span_json(span: Option<Span>) -> serde_json::Value {
    span.map_or(serde_json::Value::Null, |span| {
        serde_json::json!({
            "file": span.file.index(),
            "start": {"line": span.start.line, "col": span.start.col},
            "end": {"line": span.end.line, "col": span.end.col},
        })
    })
}

/// Workshop cost/size facts (#445): the canonical element count where the
/// program supports it, plus structural counts that are always computable.
/// The element count is an exact structural measurement — program size —
/// never a claim about runtime cost.
fn cost_facts(
    program: &workshop_rs::Program,
    element_count: &Result<
        workshop_rs::actions::ElementCountReport,
        workshop_rs::actions::ElementCountError,
    >,
) -> serde_json::Value {
    let counts = serde_json::json!({
        "rules": program.rules.len(),
        "conditions": program.rules.iter().map(|rule| rule.conditions.len()).sum::<usize>(),
        "actions": program.rules.iter().map(|rule| rule.actions.len()).sum::<usize>(),
        "waits": program
            .rules
            .iter()
            .flat_map(|rule| &rule.actions)
            .filter(|action| matches!(action, workshop_rs::Action::Call { name, .. } if name == "wait"))
            .count(),
    });
    let mut cost = serde_json::json!({ "counts": counts });
    match element_count {
        Ok(report) => {
            cost["elementCount"] = serde_json::json!(report.total);
        }
        Err(error) => {
            cost["elementCount"] = serde_json::Value::Null;
            cost["unavailableReason"] = serde_json::json!(error.to_string());
        }
    }
    cost
}

/// Performance/stability risk indicators (#445): registry findings narrowed
/// to rules that declare a `performance` or `stability` tag. Each finding
/// keeps its evidence class so consumers can distinguish exact facts,
/// static indicators, and heuristics; nothing here claims measured runtime
/// behavior.
fn risk_facts(service: &SemanticService<'_>) -> serde_json::Value {
    let lint_rules = service_response(service, &Request::LintRules);
    let risk_rule_ids: BTreeSet<&str> = lint_rules
        .get("rules")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|rule| {
            rule.get("tags")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|tags| {
                    tags.iter()
                        .any(|tag| matches!(tag.as_str(), Some("performance" | "stability")))
                })
        })
        .filter_map(|rule| rule.get("id").and_then(serde_json::Value::as_str))
        .collect();
    let risks = service_response(service, &Request::GetFindings)
        .as_array()
        .map(|findings| {
            findings
                .iter()
                .filter(|finding| {
                    finding
                        .get("code")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|code| risk_rule_ids.contains(code))
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    serde_json::Value::Array(risks)
}

/// The `Request::Program` summary for `loaded`. The analyzer counts retained
/// source documents, but a mapped provider program's file table entries carry
/// no text — `files` reports the loaded source-file table like `project` does.
fn program_summary(service: &SemanticService<'_>, loaded: &Loaded) -> serde_json::Value {
    let mut program = service_response(service, &Request::Program);
    if let serde_json::Value::Object(object) = &mut program {
        object["files"] = serde_json::json!(loaded.source_files.len().max(1));
    }
    program
}

#[hotpath::measure]
fn inspect_result(service: &SemanticService<'_>, loaded: &Loaded) -> InspectResult {
    InspectResult {
        program: program_summary(service, loaded),
        rules: service_response(service, &Request::ListRules),
        symbols: service_response(service, &Request::ListSymbols { kind: None }),
        references: service.references_for_all_symbols(),
    }
}

/// The `analyze` report over one semantic service: the `program` summary
/// without the lint-finding count, then the fact layers with every span
/// resolved against the loaded input like `lint` does — mapped or
/// source-parsed locations become authored paths, while unmapped provider
/// output resolves to `<provider-artifact>` instead of a fabricated
/// location (#445).
fn analyze_result(
    service: &SemanticService<'_>,
    loaded: &Loaded,
    catalog: &Catalog,
) -> AnalyzeResult {
    let mut program = program_summary(service, loaded);
    if let serde_json::Value::Object(object) = &mut program {
        object.remove("findings");
    }
    let mut facts = semantic_facts(service, loaded, catalog);
    hotpath::measure_block!("analyze::resolve_span_paths", {
        resolve_span_paths(&mut facts, loaded)
    });
    AnalyzeResult { program, facts }
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

pub(crate) fn span_path(file: Option<u64>, loaded: &Loaded) -> String {
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
    #[hotpath::measure]
    pub fn analyze(&mut self) -> Envelope<AnalyzeResult> {
        self.with_loaded(
            "analyze",
            |session| session.load_with_operation(ProviderOperation::Compile),
            |session, loaded| {
                let service = session.shared_semantic(&loaded);
                session.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
                analyze_result(&service, &loaded, session.catalog())
            },
        )
    }

    /// `analyze` over an already-loaded snapshot and its held semantic
    /// service — the ToolService reuse path (#513): the report renders
    /// through the service built once per loaded program rather than
    /// rebuilding the index per request.
    pub(crate) fn analyze_loaded(
        &mut self,
        loaded: Loaded,
        service: &SemanticService<'_>,
    ) -> Envelope<AnalyzeResult> {
        self.with_loaded(
            "analyze",
            |_| Ok(loaded),
            |session, loaded| {
                session.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
                analyze_result(service, &loaded, session.catalog())
            },
        )
    }

    /// `inspect`: load and produce the structural/semantic program model.
    #[hotpath::measure]
    pub fn inspect(&mut self) -> Envelope<InspectResult> {
        self.with_loaded(
            "inspect",
            |session| session.load(),
            |session, loaded| {
                let service = session.shared_semantic(&loaded);
                session.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
                inspect_result(&service, &loaded)
            },
        )
    }

    /// `symbols` (#429): the `symbols` operation's payload; `kind` narrows to
    /// one symbol kind and `file`/`max` bound the reported set (#531).
    pub fn symbols(
        &mut self,
        kind: Option<String>,
        file: Option<String>,
        max: Option<usize>,
    ) -> Envelope<SymbolsResult> {
        self.service_query("symbols", move |service| {
            tool_result(service.handle(&ToolRequest::Symbols { kind, file, max }))
                .map(SymbolsResult)
        })
    }

    /// `refs` (#429): the `usage` operation's payload — the header counts —
    /// plus the `references` operation's payload under `references`, for one
    /// symbol addressed by name. The reference selection fields (`kind`,
    /// `rule`, `file`, `max`; #531) narrow the `references` member and merge
    /// its `selection` summary at the top level.
    pub fn refs(
        &mut self,
        name: &str,
        kind: Option<String>,
        rule: Option<String>,
        file: Option<String>,
        max: Option<usize>,
    ) -> Envelope<RefsResult> {
        let symbol = Address::Name(name.to_string());
        let rule = rule.map(Address::Name);
        self.service_query("refs", move |service| {
            let references = tool_result(service.handle(&ToolRequest::References {
                symbol: symbol.clone(),
                kind,
                rule,
                file,
                max,
            }))?;
            let usage = tool_result(service.handle(&ToolRequest::Usage { symbol }))?;
            let mut result = usage.as_object().cloned().unwrap_or_default();
            match references {
                serde_json::Value::Object(mut selected) => {
                    result.insert(
                        "references".to_string(),
                        selected.remove("references").unwrap_or_default(),
                    );
                    if let Some(selection) = selected.remove("selection") {
                        result.insert("selection".to_string(), selection);
                    }
                }
                other => {
                    result.insert("references".to_string(), other);
                }
            }
            Ok(RefsResult(serde_json::Value::Object(result)))
        })
    }

    /// `cfg` (#429): the `cfg` operation's payload for one rule, addressed by
    /// name; `kind`/`max` bound the reported blocks (#531).
    pub fn cfg(
        &mut self,
        rule: &str,
        kind: Option<String>,
        max: Option<usize>,
    ) -> Envelope<CfgResult> {
        let rule = Address::Name(rule.to_string());
        self.service_query("cfg", move |service| {
            tool_result(service.handle(&ToolRequest::Cfg { rule, kind, max })).map(CfgResult)
        })
    }

    /// `callgraph` (#429): the `callGraph` operation's payload; `caller`/
    /// `callee`/`max` bound the reported edges (#531).
    pub fn callgraph(
        &mut self,
        caller: Option<String>,
        callee: Option<String>,
        max: Option<usize>,
    ) -> Envelope<CallGraphResult> {
        self.service_query("callgraph", move |service| {
            tool_result(service.handle(&ToolRequest::CallGraph {
                caller,
                callee,
                max,
            }))
            .map(CallGraphResult)
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
            |session, loaded| {
                session.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
                inspect_result(service, &loaded)
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
    #[hotpath::measure]
    pub fn lint(&mut self) -> Envelope<LintResult> {
        self.with_loaded(
            "lint",
            |session| session.load_with_operation(ProviderOperation::Compile),
            |session, loaded| {
                session.attach_workshop_completeness(&loaded);
                let shared = session.shared_semantic(&loaded);
                // An explicitly configured lint run shares the snapshot's
                // index and only reruns the registry under the effective
                // configuration (#513); the default configuration is the
                // shared service itself.
                let configured;
                let service = if session.config.lint.rules.is_empty() {
                    &*shared
                } else {
                    configured = shared.with_lint_config(session.config.lint.clone());
                    &configured
                };
                session.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
                let program = program_summary(service, &loaded);
                let lint_rules = service_response(service, &Request::LintRules);
                let lint_rule_count = lint_rules
                    .pointer("/rules")
                    .and_then(serde_json::Value::as_array)
                    .map_or(0, Vec::len);
                session.progress(ProgressEvent::with_count(
                    ProgressPhase::Linting,
                    lint_rule_count,
                    ProgressUnit::Rules,
                ));
                let mut findings = service_response(service, &Request::GetFindings);
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
