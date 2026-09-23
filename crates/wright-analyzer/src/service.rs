//! [`SemanticService`] answers transport-neutral JSON requests about a
//! Workshop program: program summary, rule/action/value lookup,
//! symbol/reference inspection, usage, CFG inspection, and static-analysis
//! findings. The request/response models ([`Request`], [`Response`]) are
//! plain serde data with no transport or UI dependency.

use serde::{Deserialize, Serialize};

pub use crate::canonical::SemanticService;

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
    /// Persistent Workshop object facts, separate from lint diagnostics.
    GetPersistentObjects,
    /// The registered lint rules and the effective lint configuration.
    LintRules,
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
pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");

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
