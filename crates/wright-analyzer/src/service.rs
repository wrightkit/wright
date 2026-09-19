use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use workshop_rs::source::Span;
use workshop_rs::{Event, Program};

use crate::analysis::{Boundedness, Finding, analyze, persistent_objects};
use crate::cfg::cfg_response;
use crate::registry::{LintConfig, LintRegistry};
use crate::symbols::{Id, RuleId, SemanticIndex, Symbol};

pub const SERVICE_NAME: &str = "wright-tool";
pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Origin {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Request {
    Version,
    Program,
    ListRules,
    GetRule {
        rule: u32,
    },
    ListSymbols {
        #[serde(default)]
        kind: Option<String>,
    },
    GetSymbol {
        symbol: u32,
    },
    FindReferences {
        symbol: u32,
    },
    GetUsage {
        symbol: u32,
    },
    GetCfg {
        rule: u32,
    },
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

pub struct SemanticService<'a> {
    program: &'a Program,
    index: SemanticIndex,
    findings: Vec<Finding>,
    origin: Origin,
    config: LintConfig,
    registry: Arc<LintRegistry>,
}

impl<'a> SemanticService<'a> {
    pub fn new(program: &'a Program) -> Self {
        Self::with_origin(
            program,
            Origin {
                kind: "unknown".into(),
                locale: None,
            },
        )
    }

    pub fn from_workshop(program: &'a Program, locale: &str) -> Self {
        Self::with_origin(
            program,
            Origin {
                kind: "workshop".into(),
                locale: Some(locale.to_ascii_lowercase()),
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
        let mut findings = analyze(program, &config);
        findings.extend(registry.run_canonical_custom(program, &config));
        Self {
            program,
            index,
            findings,
            origin,
            config,
            registry,
        }
    }

    pub fn index(&self) -> &SemanticIndex {
        &self.index
    }

    pub fn program(&self) -> &'a Program {
        self.program
    }

    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    pub fn handle_json(&self, request_json: &str) -> String {
        let request: Request = match serde_json::from_str(request_json) {
            Ok(req) => req,
            Err(err) => {
                return serde_json::to_string(&self.error("malformed-request", err.to_string()))
                    .unwrap_or_else(|_| "{}".to_string());
            }
        };
        serde_json::to_string(&self.handle(&request)).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn handle(&self, request: &Request) -> Response {
        match request {
            Request::Version => Response::Ok {
                result: json!({
                    "name": SERVICE_NAME,
                    "version": SERVICE_VERSION,
                    "capabilities": [
                        "program", "rules", "symbols", "references", "usage",
                        "cfg", "findings", "persistentObjects", "lintRules"
                    ]
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
                result: json!(
                    self.program
                        .rules
                        .iter()
                        .enumerate()
                        .map(|(id, rule)| {
                            json!({
                                "id": id,
                                "name": rule.name,
                                "span": span_json(self.program.rule_span(id)),
                            })
                        })
                        .collect::<Vec<_>>()
                ),
            },
            Request::GetRule { rule } => self.rule(*rule as usize),
            Request::ListSymbols { kind } => Response::Ok {
                result: json!(
                    self.index
                        .symbols()
                        .filter(|s| kind.as_deref().is_none_or(|k| s.kind.as_str() == k))
                        .map(symbol_json)
                        .collect::<Vec<_>>()
                ),
            },
            Request::GetSymbol { symbol } => self
                .index
                .symbol(Id::from_index(*symbol as usize))
                .map_or_else(
                    || self.error("invalid-id", format!("unknown symbol {symbol}")),
                    |s| Response::Ok {
                        result: symbol_json(s),
                    },
                ),
            Request::FindReferences { symbol } => {
                let id = Id::from_index(*symbol as usize);
                if self.index.symbol(id).is_none() {
                    self.error("invalid-id", format!("unknown symbol {symbol}"))
                } else {
                    Response::Ok {
                        result: json!(
                            self.index
                                .references(id)
                                .into_iter()
                                .map(|r| {
                                    json!({
                                        "kind": r.kind.as_str(),
                                        "span": span_json(r.span),
                                        "rule": r.rule,
                                        "action": r.action,
                                        "value": r.value,
                                    })
                                })
                                .collect::<Vec<_>>()
                        ),
                    }
                }
            }
            Request::GetUsage { symbol } => {
                let id = Id::from_index(*symbol as usize);
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
            Request::GetCfg { rule } => cfg_response(self.program, *rule as usize),
            Request::GetFindings => Response::Ok {
                result: json!(self.findings.iter().map(finding_json).collect::<Vec<_>>()),
            },
            Request::GetPersistentObjects => Response::Ok {
                result: json!(persistent_objects(self.program)),
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

fn span_json(span: Option<Span>) -> JsonValue {
    span.map_or(JsonValue::Null, |s| {
        json!({
            "file": s.file.index(),
            "start": {"line": s.start.line, "col": s.start.col},
            "end": {"line": s.end.line, "col": s.end.col},
        })
    })
}

fn symbol_json(symbol: &Symbol) -> JsonValue {
    json!({
        "id": symbol.id.index(),
        "kind": symbol.kind.as_str(),
        "name": symbol.name,
        "span": span_json(symbol.span),
    })
}

fn finding_json(finding: &Finding) -> JsonValue {
    json!({
        "code": finding.code,
        "severity": finding.severity.as_str(),
        "message": finding.message,
        "span": span_json(finding.span),
        "rule": finding.rule,
        "action": finding.action,
        "value": finding.value,
        "evidence": finding.evidence.as_str(),
        "boundedness": finding.boundedness.map(Boundedness::as_str),
    })
}

fn event_name(event: &Event) -> String {
    match event {
        Event::Global => "global".into(),
        Event::EachPlayer | Event::EachPlayerWithFilters { .. } => "eachPlayer".into(),
        Event::Player { kind, .. } => format!("{kind:?}"),
        Event::Subroutine(name) => format!("subroutine:{name}"),
    }
}

fn lint_rules(registry: &LintRegistry, config: &LintConfig) -> JsonValue {
    let descriptors = registry.descriptors(config);
    let rules = descriptors
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "defaultSeverity": r.default_severity,
                "effectiveSeverity": r.effective_severity,
                "enabled": r.enabled,
                "summary": r.summary,
                "rationale": r.rationale,
                "documentation": r.documentation,
                "knownLimits": r.known_limits,
                "evidence": r.evidence,
                "tags": r.tags,
                "kind": r.kind,
            })
        })
        .collect::<Vec<_>>();
    let config_rules = descriptors
        .iter()
        .map(|r| {
            (
                r.id.clone(),
                json!({
                    "enabled": r.enabled,
                    "severity": r.effective_severity.as_str(),
                    "options": config.options(&r.id),
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    json!({
        "rules": rules,
        "config": {"rules": config_rules},
        "skipped": [],
    })
}
