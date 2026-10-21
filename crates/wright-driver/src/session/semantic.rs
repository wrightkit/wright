use std::collections::BTreeSet;
use std::path::Path;

use super::{CompilerSession, Loaded, Provenance, ProviderOperation, root_relative};
use workshop_rs::actions::ElementNodeKind;
use workshop_rs::catalog::Catalog;
use wright_analyzer::canonical::SemanticService;
use wright_analyzer::service::Request;

use crate::diag::{Diagnostic, Stage};
use crate::progress::{ProgressEvent, ProgressPhase, ProgressUnit};
use crate::result::{
    AnalyzeResult, CallGraphResult, CfgResult, CostResult, Envelope, InspectResult, LintResult,
    RefsResult, RulesResult, SymbolsResult,
};
use crate::service::{Address, ToolErrorInfo, ToolRequest, ToolResponse, ToolService};

fn service_response(service: &SemanticService<'_>, request: &Request) -> serde_json::Value {
    match service.handle(request) {
        wright_analyzer::service::Response::Ok { result } => result,
        wright_analyzer::service::Response::Error { .. } => serde_json::Value::Null,
    }
}

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
                        "address": r["address"],
                        "span": r.get("span").cloned().unwrap_or(serde_json::Value::Null),
                        "controlFlow": {
                            "blocks": blocks.len(),
                            "edges": edge_count,
                            "loopBlocks": loop_blocks,
                            "waitBlocks": wait_blocks,
                        },
                    });
                    if let Ok(report) = &element_count
                        && let Some(node) = report.rules.get(index)
                    {
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
                                "span": wright_analyzer::canonical::span_json(child.span),
                            }));
                        }
                        entry["elements"] = serde_json::json!(node.count);
                        entry["conditionElements"] = serde_json::json!(condition_elements);
                        entry["conditions"] = serde_json::Value::Array(conditions);
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
        "persistentObjects": service_response(service, &Request::GetPersistentObjects),
    })
}

fn cost_facts(
    program: &workshop_rs::Program,
    element_count: &Result<
        workshop_rs::actions::ElementCountReport,
        workshop_rs::actions::ElementCountError,
    >,
) -> serde_json::Value {
    let mut actions = 0usize;
    let mut waits = 0usize;
    let mut conditions = 0usize;
    for rule in program.rules.iter().filter(|rule| !rule.disabled) {
        conditions += rule
            .conditions
            .iter()
            .filter(|condition| !condition.disabled)
            .count();
        for (_, action) in wright_analyzer::canonical::live_actions(&rule.actions) {
            actions += 1;
            if matches!(action, workshop_rs::Action::Call { name, .. } if name == "wait") {
                waits += 1;
            }
        }
    }
    let counts = serde_json::json!({
        "rules": program.rules.len(),
        "conditions": conditions,
        "actions": actions,
        "waits": waits,
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

fn program_summary(service: &SemanticService<'_>, loaded: &Loaded) -> serde_json::Value {
    let mut program = service_response(service, &Request::Program);
    if let serde_json::Value::Object(object) = &mut program {
        object.insert(
            "files".to_string(),
            serde_json::json!(loaded.source_files.len().max(1)),
        );
    }
    program
}

pub(crate) fn check_program_summary(loaded: &Loaded) -> serde_json::Value {
    serde_json::json!({
        "origin": super::loaded_origin(loaded),
        "files": loaded.source_files.len().max(1),
        "globalVariables": loaded.program.global_variables.len(),
        "playerVariables": loaded.program.player_variables.len(),
        "subroutines": loaded.program.subroutines.len(),
        "rules": loaded.program.rules.len(),
    })
}

#[hotpath::measure]
fn inspect_result(service: &SemanticService<'_>, loaded: &Loaded) -> InspectResult {
    let mut rules = service_response(service, &Request::ListRules);
    let mut symbols = service_response(service, &Request::ListSymbols { kind: None });
    let mut references = service.references_for_all_symbols();
    for value in [&mut rules, &mut symbols, &mut references] {
        resolve_span_paths(value, loaded);
    }
    InspectResult {
        program: program_summary(service, loaded),
        rules,
        symbols,
        references,
        next: crate::result::next::list(crate::result::next::INSPECT),
    }
}

#[allow(unsafe_code)] // hotpath 0.28 emits `#[unsafe(export_name)]` statics
fn analyze_result(
    service: &SemanticService<'_>,
    loaded: &Loaded,
    catalog: &Catalog,
    selection: &crate::select::FindingSelection,
) -> AnalyzeResult {
    let mut program = program_summary(service, loaded);
    if let serde_json::Value::Object(object) = &mut program {
        object.remove("findings");
    }
    let mut facts = semantic_facts(service, loaded, catalog);
    hotpath::measure_block!("analyze::resolve_span_paths", {
        resolve_span_paths(&mut facts, loaded)
    });
    let risks = match facts["risks"].take() {
        serde_json::Value::Array(risks) => risks,
        _ => Vec::new(),
    };
    let (risks, outcome) =
        selection.apply_findings(risks, &crate::select::file_bases(&loaded.input));
    facts["risks"] = serde_json::Value::Array(risks);
    AnalyzeResult {
        program,
        facts,
        selection: outcome,
        next: crate::result::next::list(crate::result::next::ANALYZE),
    }
}

fn tool_result(response: ToolResponse) -> Result<serde_json::Value, ToolErrorInfo> {
    match response {
        ToolResponse::Ok { result } => Ok(result),
        ToolResponse::Error { error } => Err(error),
    }
}

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

pub(crate) fn prepare_findings(
    mut findings: serde_json::Value,
    service: &SemanticService<'_>,
    loaded: &Loaded,
    catalog: &Catalog,
) -> Vec<serde_json::Value> {
    resolve_span_paths(&mut findings, loaded);
    let mut findings = match findings {
        serde_json::Value::Array(findings) => findings,
        _ => Vec::new(),
    };
    crate::fix::attach_fixes(
        &mut findings,
        service.findings(),
        loaded,
        catalog,
        &CompilerSession::locale_for(loaded),
    );
    findings
}

pub(crate) fn span_path(file: Option<u64>, loaded: &Loaded) -> String {
    match loaded.provenance {
        Provenance::Unmapped => "<provider-artifact>".to_string(),
        Provenance::Mapped => mapped_span_path(file, loaded),
        Provenance::Source => source_span_path(file, loaded),
    }
}

fn mapped_span_path(file: Option<u64>, loaded: &Loaded) -> String {
    match file.and_then(|file| loaded.source_files.get(file as usize)) {
        Some(source) => root_relative(Some(Path::new(source)), &loaded.input.root)
            .unwrap_or_else(|| source.clone()),
        None => "<provider-artifact>".to_string(),
    }
}

fn source_span_path(file: Option<u64>, loaded: &Loaded) -> String {
    let Some(file) = file else {
        return loaded.input.display.clone();
    };
    let Some(source) = loaded.source_files.get(file as usize) else {
        return format!("<file {file}>");
    };
    let path = if file == 0 {
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
    path.and_then(|path| root_relative(Some(path), &loaded.input.root))
        .unwrap_or_else(|| source.clone())
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
                analyze_result(
                    &service,
                    &loaded,
                    session.catalog(),
                    &session.config.selection,
                )
            },
        )
    }

    pub(crate) fn analyze_loaded(
        &mut self,
        loaded: Loaded,
        service: &SemanticService<'_>,
        selection: &crate::select::FindingSelection,
    ) -> Envelope<AnalyzeResult> {
        self.with_loaded(
            "analyze",
            |_| Ok(loaded),
            |session, loaded| {
                session.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
                analyze_result(service, &loaded, session.catalog(), selection)
            },
        )
    }

    /// `inspect`: load and produce the structural/semantic program model.
    ///
    /// Provider-backed input loads through the same compile operation as
    /// `lint`/`analyze` (#655): a provider that cannot compile declines with
    /// its own diagnostic rather than a command-level refusal.
    #[hotpath::measure]
    pub fn inspect(&mut self) -> Envelope<InspectResult> {
        self.with_loaded(
            "inspect",
            |session| session.load_with_operation(ProviderOperation::Compile),
            |session, loaded| {
                let service = session.shared_semantic(&loaded);
                session.progress(ProgressEvent::new(ProgressPhase::SemanticAnalysis));
                inspect_result(&service, &loaded)
            },
        )
    }

    /// `rules` (ADR-0024): the `rules` operation's payload; `content` adds
    /// each selected rule's canonical content, `rule` selects one rule by
    /// name, and `file`/`max` bound the reported set.
    pub fn rules(
        &mut self,
        rule: Option<String>,
        file: Option<String>,
        max: Option<usize>,
        content: bool,
    ) -> Envelope<RulesResult> {
        self.service_query("rules", move |service| {
            tool_result(service.handle_full(&ToolRequest::Rules {
                name: None,
                rule: rule.map(Address::Name),
                snapshot: None,
                file,
                max,
                content,
                all: false,
            }))
            .map(RulesResult)
        })
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
            tool_result(service.handle_full(&ToolRequest::Symbols {
                kind,
                file,
                max,
                all: true,
            }))
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
            let references = tool_result(service.handle_full(&ToolRequest::References {
                symbol: symbol.clone(),
                snapshot: None,
                kind,
                rule,
                file,
                max,
                all: true,
            }))?;
            let usage = tool_result(service.handle_full(&ToolRequest::Usage {
                symbol,
                snapshot: None,
            }))?;
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
            tool_result(service.handle_full(&ToolRequest::Cfg {
                rule,
                snapshot: None,
                kind,
                max,
                all: true,
            }))
            .map(CfgResult)
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
            tool_result(service.handle_full(&ToolRequest::CallGraph {
                caller,
                callee,
                max,
                all: true,
            }))
            .map(CallGraphResult)
        })
    }

    /// `cost` (#429): the `costEstimate` operation's payload; the session's
    /// finding selection applies exactly as on the agent surface, and its
    /// outcome is kept in-process so the verdict reflects the full finding
    /// set (#635). `rule` selects the per-rule drill-down (#724); the
    /// run-cache comparison and `--max-total` gate apply inside the load,
    /// before the envelope's verdict (ADR-0027, #723).
    pub fn cost(&mut self, rule: Option<String>) -> Envelope<CostResult> {
        let selection = self.config.selection.clone();
        let mut envelope = self.service_query("cost", move |service| {
            let (mut result, outcome) =
                service.cost_estimate_result(&selection, rule.as_deref())?;
            let comparison = service.apply_cost_run(&result)?;
            if let Some(comparison) = comparison {
                result["__comparison"] = comparison.to_json();
            }
            Ok(CostResult {
                result,
                selection: outcome,
            })
        });
        if let Some(comparison) = envelope
            .result
            .result
            .as_object_mut()
            .and_then(|result| result.remove("__comparison"))
        {
            envelope.comparison = Some(comparison);
        }
        envelope
    }

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
            |session| session.load_with_operation(ProviderOperation::Compile),
            |session, loaded| {
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
                    Err(mut error) => {
                        crate::service::resolve_candidates(
                            &mut error.message,
                            &mut error.fields,
                            Some(&loaded),
                        );
                        let mut diagnostic =
                            Diagnostic::carried_error(error.code, Stage::Analysis, error.message);
                        diagnostic.fields = error.fields;
                        session.diagnostics.push(diagnostic);
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
                let findings = prepare_findings(
                    service_response(service, &Request::GetFindings),
                    service,
                    &loaded,
                    session.catalog(),
                );
                let (findings, selection) = session
                    .config
                    .selection
                    .apply_findings(findings, &crate::select::file_bases(&loaded.input));
                let (rules, config, skipped) = crate::result::split_lint_rules(&lint_rules);
                LintResult {
                    input_identity: loaded.input.identity.clone(),
                    program,
                    rules,
                    config,
                    findings: serde_json::Value::Array(findings),
                    skipped,
                    selection,
                    fixes: None,
                    next: crate::result::next::list(crate::result::next::LINT),
                }
            },
        )
    }
}
