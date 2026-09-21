use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::source::Span;
use workshop_rs::{Event, Program, parser, wir};

use crate::analysis::{self, Boundedness, Finding, Severity};
use crate::cfg::Cfg;
use crate::registry::{LintConfig, LintRegistry};
use crate::symbols::{ReferenceKind, RuleId, SemanticIndex, Symbol, SymbolId, SymbolKind};

pub const SERVICE_NAME: &str = "wright-tool";
pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Request {
    Version,
    Program,
    ListRules,
    GetRule { rule: u32 },
    ListSymbols {
        #[serde(default)]
        kind: Option<String>,
    },
    GetSymbol { symbol: u32 },
    FindReferences { symbol: u32 },
    GetUsage { symbol: u32 },
    GetCfg { rule: u32 },
    GetFindings,
    GetPersistentObjects,
    LintRules,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Response {
    Ok { result: JsonValue },
    Error { error: ErrorInfo },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Origin {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

pub struct SemanticService<'a> {
    program: &'a Program,
    owned_program: Option<Box<Program>>,
    index: SemanticIndex,
    findings: Vec<Finding>,
    origin: Origin,
    config: LintConfig,
    registry: Arc<LintRegistry>,
}

impl<'a> SemanticService<'a> {
    pub fn with_origin_and_config(
        program: &'a Program,
        origin: Origin,
        config: LintConfig,
    ) -> Self {
        Self::with_origin_and_config_and_registry(
            program,
            origin,
            config,
            Arc::new(LintRegistry::default()),
        )
    }

    pub fn with_origin_and_config_and_registry(
        program: &'a Program,
        origin: Origin,
        config: LintConfig,
        registry: Arc<LintRegistry>,
    ) -> Self {
        let index = SemanticIndex::build(program);
        let mut findings = analysis::analyze(program, &config);
        findings.extend(registry.run_canonical_custom(program, &config));
        Self {
            program,
            owned_program: None,
            index,
            findings,
            origin,
            config,
            registry,
        }
    }

    pub fn from_workshop(
        program: &'a wir::Program,
        locale: impl Into<String>,
    ) -> Result<SemanticService<'a>, wir::error::IrError> {
        let locale_str = locale.into().to_ascii_lowercase();
        let catalog = Catalog::builtin().unwrap();
        let text = program
            .source(workshop_rs::ids::Id::from_index(0))
            .map(|d| d.text().to_string())
            .unwrap_or_else(|| {
                workshop_rs::emitter::emit_wir(program, &catalog, &Locale::new(&locale_str))
                    .unwrap_or_default()
            });
        let parsed = parser::parse_with_context(
            &text,
            &catalog,
            &Locale::new(&locale_str),
            &catalog,
        ).unwrap_or_else(|_| Program::new());
        let boxed = Box::new(parsed);
        let program_ref: &'a Program = unsafe { &*(boxed.as_ref() as *const Program) };
        let config = LintConfig::default();
        let mut service = Self::with_origin_and_config(
            program_ref,
            Origin {
                kind: "workshop".to_string(),
                locale: Some(locale_str),
            },
            config,
        );
        service.owned_program = Some(boxed);
        Ok(service)
    }

    pub fn from_protocol(
        program: &'a wir::Program,
    ) -> Result<SemanticService<'a>, wir::error::IrError> {
        Self::from_workshop(program, "en-US")
    }

    pub fn new(program: &'a wir::Program) -> Result<SemanticService<'a>, wir::error::IrError> {
        Self::from_workshop(program, "en-US")
    }

    pub fn handle_json(&self, request_json: &str) -> String {
        let request = match serde_json::from_str::<Request>(request_json) {
            Ok(request) => request,
            Err(error) => {
                return serde_json::to_string(&self.error("malformed-request", error.to_string()))
                    .expect("error response serializes");
            }
        };
        serde_json::to_string(&self.handle(&request)).expect("response serializes")
    }

    pub fn handle(&self, request: &Request) -> Response {
        match request {
            Request::Version => Response::Ok {
                result: json!({
                    "name": SERVICE_NAME,
                    "version": SERVICE_VERSION,
                    "capabilities": ["program", "rules", "symbols", "references", "usage", "cfg", "findings", "persistentObjects", "lintRules"],
                }),
            },
            Request::Program => Response::Ok {
                result: json!({
                    "origin": self.origin,
                    "files": 1,
                    "globalVariables": self.program.global_variables.len(),
                    "playerVariables": self.program.player_variables.len(),
                    "subroutines": self.program.subroutines.len(),
                    "rules": self.program.rules.len(),
                    "findings": self.findings.len(),
                }),
            },
            Request::ListRules => Response::Ok {
                result: json!(self.program.rules.iter().enumerate().map(|(id, rule)| json!({
                    "id": id,
                    "name": rule.name,
                    "span": span_json(self.program.rule_span(id)),
                })).collect::<Vec<_>>()),
            },
            Request::GetRule { rule } => self.rule(*rule as usize),
            Request::ListSymbols { kind } => Response::Ok {
                result: json!(self.index.symbols().filter(|symbol| {
                    kind.as_deref().is_none_or(|k| symbol_kind_name(symbol.kind) == k)
                }).map(symbol_json).collect::<Vec<_>>()),
            },
            Request::GetSymbol { symbol } => {
                let id = SymbolId::from_index(*symbol as usize);
                self.index.symbol(id).map_or_else(
                    || self.error("invalid-id", format!("unknown symbol {symbol}")),
                    |s| Response::Ok { result: symbol_json(s) },
                )
            }
            Request::FindReferences { symbol } => {
                let id = SymbolId::from_index(*symbol as usize);
                if self.index.symbol(id).is_none() {
                    self.error("invalid-id", format!("unknown symbol {symbol}"))
                } else {
                    Response::Ok {
                        result: json!(self.index.references(id).into_iter().map(|reference| json!({
                            "kind": reference_kind_name(reference.kind),
                            "span": span_json(reference.span),
                            "rule": reference.rule,
                            "action": reference.action,
                            "value": reference.value,
                        })).collect::<Vec<_>>()),
                    }
                }
            }
            Request::GetUsage { symbol } => {
                let id = SymbolId::from_index(*symbol as usize);
                self.index.symbol(id).map_or_else(
                    || self.error("invalid-id", format!("unknown symbol {symbol}")),
                    |data| {
                        let usage = self.index.usage(id);
                        Response::Ok {
                            result: json!({
                                "symbol": data.name,
                                "reads": usage.reads,
                                "writes": usage.writes,
                                "calls": usage.calls,
                                "rules": usage.rules,
                            }),
                        }
                    },
                )
            }
            Request::GetCfg { rule } => {
                let rule_idx = *rule as usize;
                if self.program.rules.get(rule_idx).is_none() {
                    return self.error("invalid-id", format!("unknown rule {rule}"));
                }
                match Cfg::build(self.program, rule_idx) {
                    Ok(cfg) => Response::Ok { result: cfg.to_json() },
                    Err(_) => self.error("invalid-cfg", format!("rule {rule} has no CFG")),
                }
            }
            Request::GetFindings => Response::Ok {
                result: json!(self.findings.iter().map(finding_json).collect::<Vec<_>>()),
            },
            Request::GetPersistentObjects => Response::Ok {
                result: json!(analysis::persistent_objects(self.program)),
            },
            Request::LintRules => Response::Ok {
                result: lint_rules(&self.registry, &self.config),
            },
        }
    }

    fn rule(&self, id: RuleId) -> Response {
        let Some(rule) = self.program.rules.get(id) else {
            return self.error("invalid-id", format!("unknown rule {id}"));
        };
        Response::Ok {
            result: json!({
                "id": id,
                "name": rule.name,
                "span": span_json(self.program.rule_span(id)),
                "disabled": rule.disabled,
                "event": event_name(&rule.event),
                "conditions": rule.conditions.len(),
                "actions": rule.actions.len(),
            }),
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

fn symbol_json(symbol: &Symbol) -> JsonValue {
    json!({
        "id": symbol.id.index(),
        "kind": symbol_kind_name(symbol.kind),
        "name": symbol.name,
        "span": span_json(symbol.span),
        "rule": symbol.rule,
    })
}

fn finding_json(finding: &Finding) -> JsonValue {
    json!({
        "code": finding.code,
        "severity": severity_name(finding.severity),
        "message": finding.message,
        "span": span_json(finding.span),
        "rule": finding.rule,
        "action": finding.action,
        "value": finding.value,
        "evidence": match finding.evidence {
            analysis::EvidenceClass::Exact => "exact",
            analysis::EvidenceClass::StaticIndicator => "static-indicator",
            analysis::EvidenceClass::Heuristic => "heuristic",
            analysis::EvidenceClass::RuntimeValidated => "runtime-validated",
        },
        "boundedness": finding.boundedness.map(Boundedness::as_str),
    })
}

fn span_json(span: Option<Span>) -> JsonValue {
    span.map_or(JsonValue::Null, |s| {
        json!({
            "file": s.file.index(),
            "start": {"line": s.start.line, "col": s.start.col},
            "end": {"line": s.end.line, "col": s.end.col},
        })
    })
}

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
}

pub fn symbol_kind_name(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::GlobalVariable => "globalVariable",
        SymbolKind::PlayerVariable => "playerVariable",
        SymbolKind::Subroutine => "subroutine",
        SymbolKind::Rule => "rule",
    }
}

pub fn reference_kind_name(kind: ReferenceKind) -> &'static str {
    match kind {
        ReferenceKind::Declaration => "declaration",
        ReferenceKind::Definition => "definition",
        ReferenceKind::Read => "read",
        ReferenceKind::Write => "write",
        ReferenceKind::Call => "call",
    }
}

fn event_name(event: &Event) -> &'static str {
    match event {
        Event::Global => "global",
        Event::EachPlayer => "eachPlayer",
        Event::Subroutine(_) => "subroutine",
        _ => "player",
    }
}

fn lint_rules(registry: &LintRegistry, config: &LintConfig) -> JsonValue {
    let descriptors = registry.descriptors(config);
    let rules = descriptors
        .iter()
        .map(|rule| {
            json!({
                "id": rule.id,
                "defaultSeverity": severity_name(rule.default_severity),
                "effectiveSeverity": severity_name(rule.effective_severity),
                "enabled": rule.enabled,
                "summary": rule.summary,
                "rationale": rule.rationale,
                "documentation": rule.documentation,
                "knownLimits": rule.known_limits,
                "evidence": match rule.evidence {
                    analysis::EvidenceClass::Exact => "exact",
                    analysis::EvidenceClass::StaticIndicator => "static-indicator",
                    analysis::EvidenceClass::Heuristic => "heuristic",
                    analysis::EvidenceClass::RuntimeValidated => "runtime-validated",
                },
                "tags": rule.tags,
                "kind": rule.kind,
            })
        })
        .collect::<Vec<_>>();
    let mut config_rules = serde_json::Map::new();
    for rule in &descriptors {
        config_rules.insert(
            rule.id.clone(),
            json!({
                "enabled": rule.enabled,
                "severity": severity_name(rule.effective_severity),
                "options": config.options(&rule.id),
            }),
        );
    }
    json!({
        "rules": rules,
        "config": {
            "rules": config_rules,
        },
        "skipped": [],
    })
}
