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
    /// One rule's canonical content in the `workshop-rs/rule-content-v1`
    /// format, with statement spans (ADR-0024).
    GetRuleContent { rule: u32 },
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

/// Neutral markers an [`ErrorInfo`] message can carry where one shared
/// fact must be spelled per surface (#888): the analyzer/driver emit the
/// marker once, and each surface renders it — `wright serve` the
/// operation name, the CLI its `wright inspect` command.
pub mod marker {
    /// `{file:N}` — the Nth source-file spelling, rendered against the
    /// loaded program's file table (the analyzer knows only file ids,
    /// never the spellings).
    pub const FILE_PREFIX: &str = "file:";
    /// `{list:OP}` — the operation/command that lists the declared names.
    pub const LIST_PREFIX: &str = "list:";
    /// `{list-all:OP}` — the same listing without its default bound.
    pub const LIST_ALL_PREFIX: &str = "list-all:";

    /// The `{file:N}` marker for a file-table index.
    pub fn file(index: usize) -> String {
        format!("{{file:{index}}}")
    }

    /// The `{list:OP}`/`{list-all:OP}` marker naming a name-listing
    /// operation; `unbounded` selects the unbounded form.
    pub fn list(operation: &str, unbounded: bool) -> String {
        let prefix = if unbounded {
            LIST_ALL_PREFIX
        } else {
            LIST_PREFIX
        };
        format!("{{{prefix}{operation}}}")
    }

    /// Escape a user-controlled substring before it is interpolated into
    /// a message markers can appear in (#888): `{` becomes `{{`, which
    /// every expansion pass copies through untouched and the surface's
    /// final [`unescape`] collapses back to a literal `{`. Without it a
    /// value like `{file:0}` or `{list:rules}` would render as a marker.
    pub fn escape(text: &str) -> String {
        text.replace('{', "{{")
    }

    /// The matching final rendering step: each `{{` collapses to a
    /// literal `{`. It runs once per message, after every expansion
    /// pass — never inside one, or an escaped `{list:…}` would surface
    /// unescaped and re-expand on the next pass.
    pub fn unescape(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(character) = chars.next() {
            if character == '{' && chars.peek() == Some(&'{') {
                chars.next();
            }
            out.push(character);
        }
        out
    }
}

/// The origin of a compiled program, carried in tool responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    /// `workshop` (native localized Workshop text) or `protocol`
    /// (`wright/opy-hir` bridge JSON).
    pub kind: String,
    /// The Workshop client locale, for Workshop-origin programs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}
