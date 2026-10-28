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
/// other characters (`docs/cli/machine-contract.md`). Codes Wright coins
/// must satisfy it; codes carried verbatim from another contract only
/// need [`carried_code_ok`].
pub fn valid_code(code: &str) -> bool {
    !code.is_empty()
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// The one guarantee every emitted code keeps — including a code carried
/// verbatim from another contract (an LPP `refusalCode` or a provider
/// failure code): nonempty and whitespace-free (#789). A foreign code's
/// spelling is fixed by its own contract, so the kebab grammar does not
/// apply to it.
pub fn carried_code_ok(code: &str) -> bool {
    !code.is_empty() && !code.chars().any(char::is_whitespace)
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
    /// The exit class the producer assigns the failure (#1003): surfaces
    /// read it instead of re-classifying from the code. In-process only —
    /// the wire refuses carry `code`/`message`; the exit verdict already
    /// rides in the envelope.
    #[serde(skip)]
    pub class: ExitClass,
    #[serde(default, flatten)]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

/// The exit class a diagnostic or tool error is created with (#1003).
/// Producers — not renderers — decide whether a failure is a usage
/// error, a source error, an unsupported capability, or an internal
/// failure, so no downstream layer matches on the diagnostic code to
/// pick an exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExitClass {
    /// A source/user error (exit 1).
    #[default]
    SourceError,
    /// A usage error (exit 2).
    Usage,
    /// A recognized but unsupported request (exit 3).
    Unsupported,
    /// An internal/environment failure (exit 4).
    Internal,
}

impl ExitClass {
    /// The `wright-result/v1` exit code for the class.
    pub fn exit_code(self) -> u8 {
        match self {
            Self::SourceError => 1,
            Self::Usage => 2,
            Self::Unsupported => 3,
            Self::Internal => 4,
        }
    }
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
            class: ExitClass::SourceError,
            fields: Default::default(),
        }
    }

    /// An error payload whose `code` is issued by another contract — an
    /// LPP `refusalCode` such as `lookup.unknownWithin`, or a provider
    /// failure code — and carried verbatim so consumers match it. Only
    /// [`carried_code_ok`] is asserted: the wire contract fixes the
    /// spelling.
    pub fn carried(code: impl Into<String>, message: impl Into<String>) -> Self {
        let code = code.into();
        debug_assert!(
            carried_code_ok(&code),
            "carried diagnostic code {code:?} is empty or contains whitespace"
        );
        ErrorInfo {
            code,
            message: message.into(),
            class: ExitClass::SourceError,
            fields: Default::default(),
        }
    }

    /// This payload with a different exit class (#1003).
    pub fn with_class(mut self, class: ExitClass) -> Self {
        self.class = class;
        self
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
    fn error_info_carried_keeps_foreign_vocabulary() {
        for code in [
            "lookup.unknownWithin",
            "provider-refusal-rename.nameCollision",
        ] {
            let error = ErrorInfo::carried(code, "message");
            assert_eq!(error.code, code);
        }
    }

    #[test]
    #[should_panic(expected = "empty or contains whitespace")]
    fn error_info_carried_rejects_whitespace() {
        let _ = ErrorInfo::carried("bad code", "message");
    }
}
