//! The read-only agent/tool interface over Wright's semantic services.
//!
//! [`SemanticService`] answers transport-neutral JSON requests about a
//! compiled Workshop IR program: program summary, rule/action/value lookup,
//! symbol/reference inspection, usage, CFG inspection, and static-analysis
//! findings. The request/response models ([`Request`], [`Response`]) are
//! plain serde data with no transport or UI dependency, and there is no
//! mutation or AST-editing contract in v0.2.
//!
//! The `wright-tool` binary wires the pipeline (protocol JSON → internal HIR
//! → Workshop IR) into this service and serves requests over stdin/stdout.

use serde::{Deserialize, Serialize};
use serde_json::json;

use wright_ir::error::IrError;
use wright_ir::source::Span;
use wright_ir::wir;

use crate::analysis::{self, Finding};
use crate::cfg::Cfg;
use crate::symbols::{ReferenceKind, SemanticIndex, SymbolId, SymbolKind};

/// A semantic query request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Request {
    /// Service identity and supported capabilities.
    Version,
    /// Program-level summary.
    Program,
    /// List every rule.
    ListRules,
    /// Look up one rule by id.
    GetRule { rule: u32 },
    /// List symbols, optionally filtered by kind.
    ListSymbols {
        #[serde(default)]
        kind: Option<String>,
    },
    /// Look up one symbol by id.
    GetSymbol { symbol: u32 },
    /// Find all references to a symbol.
    FindReferences { symbol: u32 },
    /// Aggregate usage counts for a symbol.
    GetUsage { symbol: u32 },
    /// The control-flow graph of one rule.
    GetCfg { rule: u32 },
    /// Every static-analysis finding.
    GetFindings,
}

/// A semantic query response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Response {
    Ok { result: serde_json::Value },
    Error { error: ErrorInfo },
}

/// A structured error payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub code: String,
    pub message: String,
}

/// The tool/service version and capabilities.
pub const SERVICE_NAME: &str = "wright-tool";
pub const SERVICE_VERSION: &str = "0.1.0";

/// The origin of a compiled program, carried in tool responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Origin {
    /// `workshop` (native localized Workshop text) or `protocol`
    /// (`wright/opy-hir` bridge JSON).
    pub kind: String,
    /// The Workshop client locale, for Workshop-origin programs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

/// The read-only semantic service over one compiled program.
pub struct SemanticService<'a> {
    program: &'a wir::Program,
    index: SemanticIndex,
    findings: Vec<Finding>,
    origin: Origin,
}

impl<'a> SemanticService<'a> {
    /// Build the service over a compiled program of unknown origin.
    pub fn new(program: &'a wir::Program) -> Result<SemanticService<'a>, IrError> {
        Self::with_origin(
            program,
            Origin {
                kind: "unknown".to_string(),
                locale: None,
            },
        )
    }

    /// Build the service over a program compiled from localized Workshop
    /// text in the given locale.
    pub fn from_workshop(
        program: &'a wir::Program,
        locale: &str,
    ) -> Result<SemanticService<'a>, IrError> {
        Self::with_origin(
            program,
            Origin {
                kind: "workshop".to_string(),
                locale: Some(wright_workshop::catalog::Locale::new(locale).to_string()),
            },
        )
    }

    /// Build the service over a program compiled from a bridge protocol
    /// payload.
    pub fn from_protocol(program: &'a wir::Program) -> Result<SemanticService<'a>, IrError> {
        Self::with_origin(
            program,
            Origin {
                kind: "protocol".to_string(),
                locale: None,
            },
        )
    }

    /// Build the service over a compiled program with explicit origin
    /// metadata.
    pub fn with_origin(
        program: &'a wir::Program,
        origin: Origin,
    ) -> Result<SemanticService<'a>, IrError> {
        let index = SemanticIndex::build(program)?;
        let findings = analysis::analyze(program);
        Ok(SemanticService {
            program,
            index,
            findings,
            origin,
        })
    }

    /// Handle one request and return its JSON response.
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

    /// Handle one request.
    pub fn handle(&self, request: &Request) -> Response {
        match request {
            Request::Version => Response::Ok {
                result: json!({
                    "name": SERVICE_NAME,
                    "version": SERVICE_VERSION,
                    "capabilities": ["program", "rules", "symbols", "references", "usage", "cfg", "findings"],
                }),
            },
            Request::Program => Response::Ok {
                result: json!({
                    "origin": self.origin,
                    "files": self.program.files.len(),
                    "globalVariables": self.program.global_variables.len(),
                    "playerVariables": self.program.player_variables.len(),
                    "subroutines": self.program.subroutines.len(),
                    "rules": self.program.rules.len(),
                    "findings": self.findings.len(),
                }),
            },
            Request::ListRules => {
                let rules: Vec<serde_json::Value> = self
                    .program
                    .rules
                    .iter()
                    .enumerate()
                    .map(|(id, rule)| json!({ "id": id, "name": rule.name, "span": span_value(rule.span) }))
                    .collect();
                Response::Ok {
                    result: json!(rules),
                }
            }
            Request::GetRule { rule } => {
                let id = wir::RuleId::from_index(*rule as usize);
                let Some(rule_data) = self.program.rules.get(id) else {
                    return self.error("invalid-id", format!("unknown rule {rule}"));
                };
                let event = match &rule_data.event {
                    wir::Event::Global => "global".to_string(),
                    wir::Event::EachPlayer => "eachPlayer".to_string(),
                    wir::Event::Subroutine(subroutine) => {
                        let name = self
                            .program
                            .subroutines
                            .get(*subroutine)
                            .map_or_else(|| "<dangling>".to_string(), |s| s.name.clone());
                        format!("subroutine:{name}")
                    }
                };
                Response::Ok {
                    result: json!({
                        "id": *rule,
                        "name": rule_data.name,
                        "span": span_value(rule_data.span),
                        "disabled": rule_data.disabled,
                        "event": event,
                        "conditions": rule_data.conditions.len(),
                        "actions": rule_data.actions.len(),
                    }),
                }
            }
            Request::ListSymbols { kind } => {
                let filter = kind.as_deref().and_then(symbol_kind);
                let symbols: Vec<serde_json::Value> = self
                    .index
                    .symbols()
                    .filter(|symbol| filter.is_none_or(|k| symbol.kind == k))
                    .map(|symbol| {
                        json!({
                            "id": symbol.id.index(),
                            "kind": symbol_kind_name(symbol.kind),
                            "name": symbol.name,
                            "span": span_value(symbol.span),
                        })
                    })
                    .collect();
                Response::Ok {
                    result: json!(symbols),
                }
            }
            Request::GetSymbol { symbol } => {
                let Some(symbol_data) = self.index.symbol(SymbolId::from_index(*symbol as usize))
                else {
                    return self.error("invalid-id", format!("unknown symbol {symbol}"));
                };
                Response::Ok {
                    result: json!({
                        "id": symbol_data.id.index(),
                        "kind": symbol_kind_name(symbol_data.kind),
                        "name": symbol_data.name,
                        "span": span_value(symbol_data.span),
                    }),
                }
            }
            Request::FindReferences { symbol } => {
                let symbol_id = SymbolId::from_index(*symbol as usize);
                if self.index.symbol(symbol_id).is_none() {
                    return self.error("invalid-id", format!("unknown symbol {symbol}"));
                }
                let references: Vec<serde_json::Value> = self
                    .index
                    .references(symbol_id)
                    .into_iter()
                    .map(|reference| {
                        json!({
                            "kind": reference_kind_name(reference.kind),
                            "span": span_value(reference.span),
                            "rule": reference.rule.map(|rule| rule.index()),
                            "action": reference.action.map(|action| action.index()),
                            "value": reference.value.map(|value| value.index()),
                        })
                    })
                    .collect();
                Response::Ok {
                    result: json!(references),
                }
            }
            Request::GetUsage { symbol } => {
                let symbol_id = SymbolId::from_index(*symbol as usize);
                let Some(symbol_data) = self.index.symbol(symbol_id) else {
                    return self.error("invalid-id", format!("unknown symbol {symbol}"));
                };
                let usage = self.index.usage(symbol_id);
                Response::Ok {
                    result: json!({
                        "symbol": symbol_data.name,
                        "reads": usage.reads,
                        "writes": usage.writes,
                        "calls": usage.calls,
                        "rules": usage.rules,
                    }),
                }
            }
            Request::GetCfg { rule } => {
                let id = wir::RuleId::from_index(*rule as usize);
                if self.program.rules.get(id).is_none() {
                    return self.error("invalid-id", format!("unknown rule {rule}"));
                }
                let Ok(cfg) = Cfg::build(self.program, id) else {
                    return self.error("invalid-cfg", format!("rule {rule} has no CFG"));
                };
                let blocks: Vec<serde_json::Value> = cfg
                    .blocks()
                    .map(|block| {
                        let data = cfg.block(block).expect("in range");
                        json!({
                            "id": block.index(),
                            "kind": cfg_kind_name(&data.kind),
                            "waits": data.waits,
                            "calls": data.calls.iter().map(|s| s.index()).collect::<Vec<_>>(),
                            "actions": data.actions.iter().map(|a| a.index()).collect::<Vec<_>>(),
                            "successors": data.successors.iter().map(|(to, kind)| json!({
                                "to": to.index(),
                                "kind": cfg_edge_name(*kind),
                            })).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                Response::Ok {
                    result: json!({
                        "entry": cfg.entry().index(),
                        "exit": cfg.exit().index(),
                        "blocks": blocks,
                    }),
                }
            }
            Request::GetFindings => {
                let findings: Vec<serde_json::Value> = self
                    .findings
                    .iter()
                    .map(|finding| {
                        json!({
                            "code": finding.code,
                            "severity": severity_name(finding.severity),
                            "message": finding.message,
                            "span": span_value(finding.span),
                            "rule": finding.rule.index(),
                            "action": finding.action.map(|action| action.index()),
                            "value": finding.value.map(|value| value.index()),
                        })
                    })
                    .collect();
                Response::Ok {
                    result: json!(findings),
                }
            }
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

fn symbol_kind(name: &str) -> Option<SymbolKind> {
    Some(match name {
        "globalVariable" => SymbolKind::GlobalVariable,
        "playerVariable" => SymbolKind::PlayerVariable,
        "subroutine" => SymbolKind::Subroutine,
        "rule" => SymbolKind::Rule,
        _ => return None,
    })
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

fn severity_name(severity: analysis::Severity) -> &'static str {
    match severity {
        analysis::Severity::Warning => "warning",
        analysis::Severity::Info => "info",
    }
}

fn cfg_kind_name(kind: &crate::cfg::BlockKind) -> &'static str {
    match kind {
        crate::cfg::BlockKind::Entry => "entry",
        crate::cfg::BlockKind::StraightLine => "block",
        crate::cfg::BlockKind::If { .. } => "if",
        crate::cfg::BlockKind::While { .. } => "while",
        crate::cfg::BlockKind::ForHeader { .. } => "for",
        crate::cfg::BlockKind::Exit => "exit",
    }
}

fn cfg_edge_name(kind: crate::cfg::EdgeKind) -> &'static str {
    match kind {
        crate::cfg::EdgeKind::Fallthrough => "fallthrough",
        crate::cfg::EdgeKind::BranchTrue => "true",
        crate::cfg::EdgeKind::BranchFalse => "false",
        crate::cfg::EdgeKind::BackEdge => "back",
        crate::cfg::EdgeKind::LoopExit => "loop-exit",
    }
}

/// Render an optional span as JSON (`null` when absent).
fn span_value(span: Option<Span>) -> serde_json::Value {
    match span {
        Some(span) => json!({
            "file": span.file.index(),
            "start": { "line": span.start.line, "col": span.start.col },
            "end": { "line": span.end.line, "col": span.end.col },
        }),
        None => serde_json::Value::Null,
    }
}
