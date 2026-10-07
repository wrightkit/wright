use std::{
    ops::Deref,
    sync::{Arc, OnceLock},
};

use serde_json::{Value as JsonValue, json};
use workshop_rs::source::Span;
use workshop_rs::{Event, Program};

use super::analysis::Finding;
use super::cfg::cfg_response;
use super::facts::persistent_objects;
use super::symbols::{Reference, RuleId, SemanticIndex, Symbol, SymbolId, SymbolKind};
use crate::analysis::Boundedness;
use crate::registry::{LintConfig, SkippedRule};
use crate::service::{ErrorInfo, Origin, Request, Response};

#[derive(Clone)]
enum ProgramSource<'a> {
    Borrowed(&'a Program),
    Shared(Arc<Program>),
}

impl ProgramSource<'_> {
    fn as_ref(&self) -> &Program {
        match self {
            Self::Borrowed(program) => program,
            Self::Shared(program) => program,
        }
    }
}

impl Deref for ProgramSource<'_> {
    type Target = Program;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

pub struct SemanticService<'a> {
    program: ProgramSource<'a>,
    index: Arc<SemanticIndex>,
    findings: Vec<Finding>,
    skipped: Vec<SkippedRule>,
    origin: Origin,
    config: LintConfig,
    registry: Arc<crate::registry::LintRegistry>,
    symbols_json: OnceLock<Vec<JsonValue>>,
    references_json: OnceLock<Vec<JsonValue>>,
    findings_json: OnceLock<JsonValue>,
    lint_rules_json: OnceLock<JsonValue>,
}

impl<'a> SemanticService<'a> {
    pub fn new(program: &'a Program) -> Self {
        Self::with_origin(
            program,
            Origin {
                kind: "unknown".to_string(),
                locale: None,
            },
        )
    }
    pub fn from_workshop(program: &'a Program, locale: &str) -> Self {
        Self::with_origin(
            program,
            Origin {
                kind: "workshop".to_string(),
                locale: Some(workshop_rs::catalog::Locale::new(locale).to_string()),
            },
        )
    }
    pub fn with_origin(program: &'a Program, origin: Origin) -> Self {
        Self::with_origin_and_config(program, origin, LintConfig::default())
    }
    pub fn with_origin_and_config(
        program: &'a Program,
        origin: Origin,
        config: LintConfig,
    ) -> Self {
        Self::with_origin_and_config_and_registry(
            program,
            origin,
            config,
            Arc::new(crate::registry::LintRegistry::default()),
        )
    }
    pub fn with_origin_and_config_and_registry(
        program: &'a Program,
        origin: Origin,
        config: LintConfig,
        registry: Arc<crate::registry::LintRegistry>,
    ) -> Self {
        Self::with_program_and_origin_and_config_and_registry(
            ProgramSource::Borrowed(program),
            origin,
            config,
            registry,
        )
    }

    pub fn with_shared_program(
        program: Arc<Program>,
        origin: Origin,
        config: LintConfig,
        registry: Arc<crate::registry::LintRegistry>,
    ) -> SemanticService<'static> {
        SemanticService::<'static>::with_program_and_origin_and_config_and_registry(
            ProgramSource::Shared(program),
            origin,
            config,
            registry,
        )
    }

    fn with_program_and_origin_and_config_and_registry(
        program: ProgramSource<'a>,
        origin: Origin,
        config: LintConfig,
        registry: Arc<crate::registry::LintRegistry>,
    ) -> Self {
        let index = hotpath::measure_block!("analyzer::index_build", {
            Arc::new(SemanticIndex::build(program.as_ref()))
        });
        let report = hotpath::measure_block!("analyzer::lint_run", {
            registry.run_report(program.as_ref(), &config)
        });
        Self {
            program,
            index,
            findings: report.findings,
            skipped: report.skipped,
            origin,
            config,
            registry,
            symbols_json: OnceLock::new(),
            references_json: OnceLock::new(),
            findings_json: OnceLock::new(),
            lint_rules_json: OnceLock::new(),
        }
    }

    pub fn with_lint_config(&self, config: LintConfig) -> Self {
        let report = hotpath::measure_block!("analyzer::lint_run_reconfigured", {
            self.registry.run_report(self.program.as_ref(), &config)
        });
        Self {
            program: self.program.clone(),
            index: Arc::clone(&self.index),
            findings: report.findings,
            skipped: report.skipped,
            origin: self.origin.clone(),
            config,
            registry: Arc::clone(&self.registry),
            symbols_json: OnceLock::new(),
            references_json: OnceLock::new(),
            findings_json: OnceLock::new(),
            lint_rules_json: OnceLock::new(),
        }
    }

    #[hotpath::measure]
    pub fn references_for_all_symbols(&self) -> JsonValue {
        JsonValue::Array(self.references_json().clone())
    }

    fn symbols_json(&self) -> &Vec<JsonValue> {
        self.symbols_json
            .get_or_init(|| self.index.symbols().map(symbol_json).collect())
    }

    fn references_json(&self) -> &Vec<JsonValue> {
        self.references_json.get_or_init(|| {
            self.index
                .references_for_all_symbols()
                .into_iter()
                .map(|references| {
                    JsonValue::Array(references.into_iter().map(reference_json).collect())
                })
                .collect()
        })
    }

    /// Resolve a symbol by its exact declared name (#429). A name matching no
    /// symbol is `unknown-symbol`; a name shared by more than one symbol is
    /// `ambiguous-symbol` with each candidate's kind and numeric id — callers
    /// may fall back to the numeric form. No match is ever guessed.
    pub fn resolve_symbol(&self, name: &str) -> Result<&Symbol, ErrorInfo> {
        let matches: Vec<&Symbol> = self
            .index
            .symbols()
            .filter(|symbol| symbol.name == name)
            .collect();
        match matches.as_slice() {
            [symbol] => Ok(symbol),
            [] => Err(ErrorInfo {
                code: "unknown-symbol".to_string(),
                message: format!("unknown symbol '{name}'"),
            }),
            _ => Err(ErrorInfo {
                code: "ambiguous-symbol".to_string(),
                message: format!(
                    "ambiguous symbol '{name}': {}",
                    matches
                        .iter()
                        .map(|symbol| format!("{} {}", symbol.kind.as_str(), symbol.id.index()))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }),
        }
    }

    /// Resolve a rule by its exact declared name to its rule index (#429).
    /// Only rule symbols participate: a rule named `x` stays reachable even
    /// when a variable or subroutine is also named `x`. Unmatched and
    /// duplicate rule names are `unknown-rule`/`ambiguous-rule`.
    pub fn resolve_rule(&self, name: &str) -> Result<RuleId, ErrorInfo> {
        let matches: Vec<&Symbol> = self
            .index
            .symbols()
            .filter(|symbol| symbol.kind == SymbolKind::Rule && symbol.name == name)
            .collect();
        match matches.as_slice() {
            [symbol] => Ok(symbol.rule.expect("rule symbols carry their rule index")),
            [] => Err(ErrorInfo {
                code: "unknown-rule".to_string(),
                message: format!("unknown rule '{name}'"),
            }),
            _ => Err(ErrorInfo {
                code: "ambiguous-rule".to_string(),
                message: format!(
                    "ambiguous rule '{name}': {}",
                    matches
                        .iter()
                        .map(|symbol| format!("rule {}", symbol.rule.expect("rule symbol")))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }),
        }
    }
    pub fn handle_json(&self, request_json: &str) -> String {
        let request: Request = match serde_json::from_str(request_json) {
            Ok(req) => req,
            Err(err) => {
                return serde_json::to_string(&Response::Error {
                    error: ErrorInfo {
                        code: "invalid-json".to_string(),
                        message: format!("could not parse request JSON: {err}"),
                    },
                })
                .expect("error response serializes");
            }
        };
        let response = self.handle(&request);
        serde_json::to_string(&response).expect("response serializes")
    }
    pub fn handle(&self, request: &Request) -> Response {
        hotpath::measure_block!(request_label(request), self.dispatch(request))
    }
    fn dispatch(&self, request: &Request) -> Response {
        match request {
            Request::Version => Response::Ok { result: json!({"name": "wright-tool", "version": env!("CARGO_PKG_VERSION"), "capabilities": ["program", "rules", "symbols", "references", "usage", "cfg", "findings", "persistentObjects", "lintRules"]}) },
            Request::Program => Response::Ok { result: json!({"origin": self.origin, "globalVariables": self.program.global_variables.len(), "playerVariables": self.program.player_variables.len(), "subroutines": self.program.subroutines.len(), "rules": self.program.rules.len(), "findings": self.findings.len()}) },
            Request::ListRules => Response::Ok { result: json!(self.program.rules.iter().enumerate().map(|(id, rule)| json!({"id": id, "name": rule.name, "span": span_json(self.program.rule_span(id))})).collect::<Vec<_>>()) },
            Request::GetRule { rule } => self.rule(*rule as usize),
            Request::ListSymbols { kind } => Response::Ok { result: JsonValue::Array(self.symbols_json().iter().filter(|symbol| kind.as_deref().is_none_or(|kind| symbol["kind"].as_str() == Some(kind))).cloned().collect()) },
            Request::GetSymbol { symbol } => self.index.symbol(SymbolId::from_index(*symbol as usize)).map_or_else(|| self.error("invalid-id", format!("unknown symbol {symbol}")), |symbol| Response::Ok { result: self.symbols_json()[symbol.id.index()].clone() }),
            Request::FindReferences { symbol } => { let id = SymbolId::from_index(*symbol as usize); if self.index.symbol(id).is_none() { self.error("invalid-id", format!("unknown symbol {symbol}")) } else { Response::Ok { result: self.references_json()[id.index()].clone() } } }
            Request::GetUsage { symbol } => { let id = SymbolId::from_index(*symbol as usize); self.index.symbol(id).map_or_else(|| self.error("invalid-id", format!("unknown symbol {symbol}")), |data| { let usage = self.index.usage(id); Response::Ok { result: json!({"id": id.index(), "kind": data.kind.as_str(), "symbol": data.name, "reads": usage.reads, "writes": usage.writes, "calls": usage.calls, "rules": usage.rules}) } }) }
            Request::GetCfg { rule } => cfg_response(self.program.as_ref(), *rule as usize),
            Request::GetFindings => Response::Ok { result: self.findings_json.get_or_init(|| json!(self.findings.iter().map(finding_json).collect::<Vec<_>>())).clone() },
            Request::GetPersistentObjects => Response::Ok { result: json!(persistent_objects(self.program.as_ref())) },
            Request::LintRules => Response::Ok {
                result: self.lint_rules_json.get_or_init(|| lint_rules(&self.registry, &self.config, &self.skipped)).clone(),
            }
        }
    }
    fn rule(&self, id: RuleId) -> Response {
        let Some(rule) = self.program.rules.get(id) else {
            return self.error("invalid-id", format!("unknown rule {id}"));
        };
        Response::Ok {
            result: json!({"id": id, "name": rule.name, "span": span_json(self.program.rule_span(id)), "disabled": rule.disabled, "event": event_name(&rule.event), "conditions": rule.conditions.len(), "actions": rule.actions.len()}),
        }
    }
    fn error(&self, code: &str, message: String) -> Response {
        Response::Error {
            error: ErrorInfo {
                code: code.to_string(),
                message,
            },
        }
    }
}

/// The hotpath measurement label for one semantic request's dispatch.
#[cfg_attr(not(feature = "hotpath"), allow(dead_code))]
fn request_label(request: &Request) -> &'static str {
    match request {
        Request::Version => "svc::version",
        Request::Program => "svc::program",
        Request::ListRules => "svc::listRules",
        Request::GetRule { .. } => "svc::getRule",
        Request::ListSymbols { .. } => "svc::listSymbols",
        Request::GetSymbol { .. } => "svc::getSymbol",
        Request::FindReferences { .. } => "svc::findReferences",
        Request::GetUsage { .. } => "svc::getUsage",
        Request::GetCfg { .. } => "svc::getCfg",
        Request::GetFindings => "svc::getFindings",
        Request::GetPersistentObjects => "svc::getPersistentObjects",
        Request::LintRules => "svc::lintRules",
    }
}
fn lint_rules(
    registry: &crate::registry::LintRegistry,
    config: &LintConfig,
    skipped: &[SkippedRule],
) -> JsonValue {
    #[cfg(test)]
    tests::BUILDS.with(|count| count.set(count.get() + 1));
    let descriptors = registry.descriptors(config);
    let rules = descriptors
        .iter()
        .map(|rule| {
            json!({
                "id": rule.id,
                "defaultSeverity": rule.default_severity,
                "effectiveSeverity": rule.effective_severity,
                "enabled": rule.enabled,
                "summary": rule.summary,
                "rationale": rule.rationale,
                "documentation": rule.documentation,
                "knownLimits": rule.known_limits,
                "evidence": rule.evidence,
                "tags": rule.tags,
                "kind": rule.kind,
            })
        })
        .collect::<Vec<_>>();
    let config_rules = descriptors
        .iter()
        .map(|rule| {
            (
                rule.id.clone(),
                json!({
                    "enabled": rule.enabled,
                    "severity": rule.effective_severity.as_str(),
                    "options": config.options(&rule.id),
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    json!({
        "rules": rules,
        "config": {"rules": config_rules},
        "skipped": skipped,
    })
}
pub(super) fn span_json(span: Option<Span>) -> JsonValue {
    span.map_or(JsonValue::Null, |span| json!({"file": span.file.index(), "start": {"line": span.start.line, "col": span.start.col}, "end": {"line": span.end.line, "col": span.end.col}}))
}
fn symbol_json(symbol: &Symbol) -> JsonValue {
    #[cfg(test)]
    tests::BUILDS.with(|count| count.set(count.get() + 1));
    json!({"id": symbol.id.index(), "kind": symbol.kind.as_str(), "name": symbol.name, "span": span_json(symbol.span)})
}
fn finding_json(finding: &Finding) -> JsonValue {
    #[cfg(test)]
    tests::BUILDS.with(|count| count.set(count.get() + 1));
    json!({"code": finding.code, "severity": finding.severity.as_str(), "message": finding.message, "span": span_json(finding.span), "rule": finding.rule, "action": finding.action, "value": finding.value, "evidence": finding.evidence.as_str(), "boundedness": finding.boundedness.map(Boundedness::as_str)})
}
fn reference_json(reference: &Reference) -> JsonValue {
    #[cfg(test)]
    tests::BUILDS.with(|count| count.set(count.get() + 1));
    json!({
        "kind": reference.kind.as_str(),
        "span": span_json(reference.span),
        "rule": reference.rule,
        "action": reference.action,
        "value": reference.value,
    })
}
fn event_name(event: &Event) -> String {
    match event {
        Event::Subroutine(name) => format!("subroutine:{name}"),
        _ => crate::declarative::public_event_id(event).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        pub(super) static BUILDS: Cell<usize> = const { Cell::new(0) };
    }

    #[test]
    fn unchanged_queries_reuse_serialized_surfaces_and_reconfiguration_is_fresh() {
        let catalog = crate::catalog::builtin().unwrap();
        let text = include_str!("../../../../tests/fixtures/workshop/synthetic/control-flow.ws");
        let program = workshop_rs::parser::parse_with_context(
            text,
            &catalog,
            &workshop_rs::catalog::Locale::new("en-US"),
            &*catalog,
        )
        .unwrap();
        let service = SemanticService::new(&program);
        let requests = [
            Request::ListSymbols { kind: None },
            Request::ListSymbols {
                kind: Some("globalVariable".into()),
            },
            Request::ListSymbols {
                kind: Some("unknown".into()),
            },
            Request::GetSymbol { symbol: 0 },
            Request::FindReferences { symbol: 0 },
            Request::FindReferences { symbol: u32::MAX },
            Request::GetFindings,
            Request::LintRules,
        ];
        let warm = requests
            .iter()
            .map(|r| serde_json::to_string(&service.handle(r)).unwrap())
            .collect::<Vec<_>>();
        let references = service.references_for_all_symbols();
        let builds = BUILDS.with(Cell::get);
        assert!(builds > 0);
        for _ in 0..3 {
            for (request, expected) in requests.iter().zip(&warm) {
                assert_eq!(
                    serde_json::to_string(&service.handle(request)).unwrap(),
                    *expected
                );
            }
            assert_eq!(service.references_for_all_symbols(), references);
            assert_eq!(
                BUILDS.with(Cell::get),
                builds,
                "warm queries must not rebuild metadata or serialize index data"
            );
        }
        let mut config = LintConfig::default();
        config.disable("min-wait-loop");
        config
            .rules
            .entry("while-without-wait".into())
            .or_default()
            .severity_override = Some(crate::registry::SeverityLabel::Error);
        config
            .rules
            .entry("repeated-value".into())
            .or_default()
            .options
            .min_matches = Some(7);
        let configured = service.with_lint_config(config.clone());
        let fresh =
            SemanticService::with_origin_and_config(&program, service.origin.clone(), config);
        for request in &requests {
            assert_eq!(
                serde_json::to_string(&configured.handle(request)).unwrap(),
                serde_json::to_string(&fresh.handle(request)).unwrap()
            );
        }
        assert_ne!(
            serde_json::to_string(&configured.handle(&Request::LintRules)).unwrap(),
            warm.last().unwrap().as_str()
        );
        let builds = BUILDS.with(Cell::get);
        for request in &requests {
            configured.handle(request);
        }
        assert_eq!(BUILDS.with(Cell::get), builds);
    }
}
