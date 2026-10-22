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

/// Whether `code` obeys the diagnostic-code grammar: a single kebab-case
/// token of lowercase ASCII letters, digits, and hyphens — no spaces or
/// other characters (`docs/cli/machine-contract.md`).
pub fn valid_code(code: &str) -> bool {
    !code.is_empty()
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// A structured error payload.
///
/// `fields` carries the refusal metadata surfaces reword or consume
/// programmatically (#888): a `listOp`/`listAll` name-listing operation,
/// `candidates` entries (`address`, `file`/`path`, `line`, `column`),
/// `declared`/`declaredTotal` bounded name lists, and the queried
/// `name`/`file`/`kind`. The keys serialize as siblings of `code` and
/// `message`, which the wire schema already permits.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub code: String,
    pub message: String,
    #[serde(default, flatten)]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

impl ErrorInfo {
    /// An error payload carrying `code` and `message`. `code` must obey the
    /// diagnostic-code grammar (`valid_code`); a violation is a bug, so it
    /// fails a debug assertion.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        let code = code.into();
        debug_assert!(
            valid_code(&code),
            "diagnostic code {code:?} violates the code grammar"
        );
        ErrorInfo {
            code,
            message: message.into(),
            fields: Default::default(),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_code_accepts_kebab_tokens() {
        for code in [
            "parse-error",
            "unknown-enum-member",
            "invalid-selection",
            "provider-refusal-x",
            "w1",
        ] {
            assert!(valid_code(code), "{code}");
        }
    }

    #[test]
    fn valid_code_rejects_spaces_uppercase_empty_and_other_characters() {
        for code in [
            "unknown-enum member",
            "Unknown-Value",
            "",
            "workshop.raw-setting.x",
            "a_b",
        ] {
            assert!(!valid_code(code), "{code}");
        }
    }

    #[test]
    #[should_panic(expected = "violates the code grammar")]
    fn error_info_new_rejects_a_spaced_code() {
        let _ = ErrorInfo::new("unknown-enum member", "message");
    }

    #[test]
    #[should_panic(expected = "violates the code grammar")]
    fn error_info_new_rejects_an_uppercase_code() {
        let _ = ErrorInfo::new("Unknown-Value", "message");
    }
}
