use std::sync::Arc;

use serde_json::{Value as JsonValue, json};
use workshop_rs::source::{FileId, Span};
use workshop_rs::{Event, Program};

use super::analysis::Finding;
use super::cfg::cfg_response;
use super::facts::persistent_objects;
use super::symbols::{ReferenceKind, RuleId, SemanticIndex, Symbol, SymbolId, SymbolKind};
use crate::analysis::{Boundedness, Severity};
use crate::registry::{LintConfig, SkippedRule};
use crate::service::{ErrorInfo, Origin, Request, Response};

pub struct SemanticService<'a> {
    program: &'a Program,
    index: SemanticIndex,
    findings: Vec<Finding>,
    skipped: Vec<SkippedRule>,
    origin: Origin,
    config: LintConfig,
    registry: Arc<crate::registry::LintRegistry>,
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
        let index = SemanticIndex::build(program);
        let report = registry.run_report(program, &config);
        Self {
            program,
            index,
            findings: report.findings,
            skipped: report.skipped,
            origin,
            config,
            registry,
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
        match request {
            Request::Version => Response::Ok { result: json!({"name": "wright-tool", "version": env!("CARGO_PKG_VERSION"), "capabilities": ["program", "rules", "symbols", "references", "usage", "cfg", "findings", "persistentObjects", "lintRules"]}) },
            Request::Program => Response::Ok { result: json!({"origin": self.origin, "files": file_count(self.program), "globalVariables": self.program.global_variables.len(), "playerVariables": self.program.player_variables.len(), "subroutines": self.program.subroutines.len(), "rules": self.program.rules.len(), "findings": self.findings.len()}) },
            Request::ListRules => Response::Ok { result: json!(self.program.rules.iter().enumerate().map(|(id, rule)| json!({"id": id, "name": rule.name, "span": span_json(self.program.rule_span(id))})).collect::<Vec<_>>()) },
            Request::GetRule { rule } => self.rule(*rule as usize),
            Request::ListSymbols { kind } => Response::Ok { result: json!(self.index.symbols().filter(|symbol| kind.as_deref().is_none_or(|kind| symbol_kind_name(symbol.kind) == kind)).map(symbol_json).collect::<Vec<_>>()) },
            Request::GetSymbol { symbol } => self.index.symbol(SymbolId::from_index(*symbol as usize)).map_or_else(|| self.error("invalid-id", format!("unknown symbol {symbol}")), |symbol| Response::Ok { result: symbol_json(symbol) }),
            Request::FindReferences { symbol } => { let id = SymbolId::from_index(*symbol as usize); if self.index.symbol(id).is_none() { self.error("invalid-id", format!("unknown symbol {symbol}")) } else { Response::Ok { result: json!(self.index.references(id).into_iter().map(|reference| json!({"kind": reference_kind_name(reference.kind), "span": span_json(reference.span), "rule": reference.rule, "action": reference.action, "value": reference.value})).collect::<Vec<_>>()) } } }
            Request::GetUsage { symbol } => { let id = SymbolId::from_index(*symbol as usize); self.index.symbol(id).map_or_else(|| self.error("invalid-id", format!("unknown symbol {symbol}")), |data| { let usage = self.index.usage(id); Response::Ok { result: json!({"symbol": data.name, "reads": usage.reads, "writes": usage.writes, "calls": usage.calls, "rules": usage.rules}) } }) }
            Request::GetCfg { rule } => cfg_response(self.program, *rule as usize),
            Request::GetFindings => Response::Ok { result: json!(self.findings.iter().map(finding_json).collect::<Vec<_>>()) },
            Request::GetPersistentObjects => Response::Ok { result: json!(persistent_objects(self.program)) },
            Request::LintRules => Response::Ok {
                result: lint_rules(&self.registry, &self.config, &self.skipped),
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

fn lint_rules(
    registry: &crate::registry::LintRegistry,
    config: &LintConfig,
    skipped: &[SkippedRule],
) -> JsonValue {
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
                    "severity": severity_name(rule.effective_severity),
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
fn file_count(program: &Program) -> usize {
    let mut count = 0;
    while program.source(FileId::from_index(count)).is_some() {
        count += 1;
    }
    count
}
pub(super) fn span_json(span: Option<Span>) -> JsonValue {
    span.map_or(JsonValue::Null, |span| json!({"file": span.file.index(), "start": {"line": span.start.line, "col": span.start.col}, "end": {"line": span.end.line, "col": span.end.col}}))
}
fn symbol_json(symbol: &Symbol) -> JsonValue {
    json!({"id": symbol.id.index(), "kind": symbol_kind_name(symbol.kind), "name": symbol.name, "span": span_json(symbol.span)})
}
fn finding_json(finding: &Finding) -> JsonValue {
    json!({"code": finding.code, "severity": severity_name(finding.severity), "message": finding.message, "span": span_json(finding.span), "rule": finding.rule, "action": finding.action, "value": finding.value, "evidence": finding.evidence.as_str(), "boundedness": finding.boundedness.map(Boundedness::as_str)})
}
fn symbol_kind_name(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::GlobalVariable => "globalVariable",
        SymbolKind::PlayerVariable => "playerVariable",
        SymbolKind::Subroutine => "subroutine",
        SymbolKind::Rule => "rule",
    }
}
fn reference_kind_name(kind: ReferenceKind) -> &'static str {
    match kind {
        ReferenceKind::Declaration => "declaration",
        ReferenceKind::Definition => "definition",
        ReferenceKind::Read => "read",
        ReferenceKind::Write => "write",
        ReferenceKind::Call => "call",
    }
}
fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
}
fn event_name(event: &Event) -> String {
    match event {
        Event::Subroutine(name) => format!("subroutine:{name}"),
        _ => crate::declarative::public_event_id(event).to_string(),
    }
}
