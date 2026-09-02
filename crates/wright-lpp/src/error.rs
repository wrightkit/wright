//! Structured client failures for LPP interactions.
//!
//! Every provider interaction fails deterministically into one of the
//! [`ProviderError`] variants below. Each variant exposes a stable machine
//! `code()` and a human-readable `Display`, so tooling layers can surface
//! refusals without parsing protocol internals. Refusals are a normal,
//! well-formed outcome and never imply a broken session.

use std::fmt;

use serde_json::Value;

/// A structured LPP error: the wire `kind` plus its machine-readable
/// `details` and human `message`. The `message` is for display; `details` is
/// the machine contract (clients MUST NOT parse `message`).
#[derive(Debug, Clone, PartialEq)]
pub struct LppError {
    pub kind: LppErrorKind,
    /// The wire `data.lpp.details` object, preserved verbatim.
    pub details: Value,
    pub message: String,
}

impl fmt::Display for LppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl LppError {
    /// The refusal code when this error is a refusal, else `None`.
    pub fn refusal_code(&self) -> Option<&str> {
        if self.kind == LppErrorKind::Refusal {
            self.details.get("refusalCode").and_then(Value::as_str)
        } else {
            None
        }
    }

    /// The supported protocol versions when this error is a protocol version
    /// mismatch, else empty.
    pub fn supported_protocol_versions(&self) -> Vec<String> {
        if self.kind == LppErrorKind::ProtocolVersionMismatch {
            self.details
                .get("supportedProtocolVersions")
                .and_then(Value::as_array)
                .map(|versions| {
                    versions
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    /// The capability id when this error is a capability refusal, else `None`.
    pub fn capability(&self) -> Option<&str> {
        if self.kind == LppErrorKind::CapabilityUnavailable {
            self.details.get("capability").and_then(Value::as_str)
        } else {
            None
        }
    }

    /// The method when this error is a capability refusal, else `None`.
    pub fn method(&self) -> Option<&str> {
        if self.kind == LppErrorKind::CapabilityUnavailable {
            self.details.get("method").and_then(Value::as_str)
        } else {
            None
        }
    }
}

/// The LPP error kinds defined by LPP v1 (spec section 18.3). Unknown kinds
/// are preserved opaquely: the client displays the message and never
/// branches on them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LppErrorKind {
    ProtocolVersionMismatch,
    InvalidRequest,
    InvalidLanguage,
    InvalidDocument,
    InvalidEntry,
    ProjectLoadFailed,
    InvalidPosition,
    InvalidArtifact,
    CapabilityUnavailable,
    /// A deliberate, documented decline; the refusal code lives in the
    /// details.
    Refusal,
    /// An unknown kind, preserved verbatim.
    Unknown(String),
}

impl LppErrorKind {
    /// Parse a wire kind name; unknown names stay opaque.
    pub fn from_wire(name: &str) -> LppErrorKind {
        match name {
            "protocolVersionMismatch" => LppErrorKind::ProtocolVersionMismatch,
            "invalidRequest" => LppErrorKind::InvalidRequest,
            "invalidLanguage" => LppErrorKind::InvalidLanguage,
            "invalidDocument" => LppErrorKind::InvalidDocument,
            "invalidEntry" => LppErrorKind::InvalidEntry,
            "projectLoadFailed" => LppErrorKind::ProjectLoadFailed,
            "invalidPosition" => LppErrorKind::InvalidPosition,
            "invalidArtifact" => LppErrorKind::InvalidArtifact,
            "capabilityUnavailable" => LppErrorKind::CapabilityUnavailable,
            "refusal" => LppErrorKind::Refusal,
            other => LppErrorKind::Unknown(other.to_string()),
        }
    }

    /// A stable machine code for this kind.
    pub fn code(&self) -> &'static str {
        match self {
            LppErrorKind::ProtocolVersionMismatch => "protocol-version-mismatch",
            LppErrorKind::InvalidRequest => "invalid-request",
            LppErrorKind::InvalidLanguage => "invalid-language",
            LppErrorKind::InvalidDocument => "invalid-document",
            LppErrorKind::InvalidEntry => "invalid-entry",
            LppErrorKind::ProjectLoadFailed => "project-load-failed",
            LppErrorKind::InvalidPosition => "invalid-position",
            LppErrorKind::InvalidArtifact => "invalid-artifact",
            LppErrorKind::CapabilityUnavailable => "capability-unavailable",
            LppErrorKind::Refusal => "refusal",
            LppErrorKind::Unknown(_) => "lpp-error",
        }
    }
}

impl fmt::Display for LppErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LppErrorKind::Unknown(name) => write!(f, "lpp error '{name}'"),
            known => write!(f, "{}", known.code()),
        }
    }
}

/// One structured client failure. All variants are deterministic outcomes of
/// the client contract; none implies a silent fallback.
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderError {
    /// No provider is configured for the requested opaque language id.
    NotConfigured { language_id: String },
    /// The provider binary could not be spawned.
    Spawn { message: String },
    /// A transport I/O failure (write/read of the process pipes).
    Io { message: String },
    /// The provider process exited without completing the request. `status`
    /// carries the exit code when it could be observed.
    Exited {
        status: Option<i32>,
        message: String,
    },
    /// No response arrived within the configured timeout.
    Timeout {
        method: String,
        duration: std::time::Duration,
    },
    /// The provider's output is not a valid LPP v1 message.
    Malformed { detail: String },
    /// A standard JSON-RPC error from the provider (spec section 4.1).
    JsonRpc { code: i64, message: String },
    /// A structured LPP error (code -32000 with `data.lpp`).
    Lpp(LppError),
    /// A method was invoked before `lpp/initialize` succeeded.
    NotInitialized { method: String },
    /// `lpp/initialize` was already performed for this session.
    AlreadyInitialized,
    /// A method was invoked after `lpp/shutdown`.
    ShutDown { method: String },
    /// The provider echoed a protocol version other than the one requested.
    ProtocolVersionMismatch {
        supported: Vec<String>,
        message: String,
    },
    /// A local provider could not be resolved or installed.
    Local {
        kind: LocalProviderErrorKind,
        message: String,
    },
}

/// Machine-readable local provider resolution failure classes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalProviderErrorKind {
    Missing,
    UnsupportedPlatform,
    Offline,
    Download,
    Integrity,
    Install,
}

impl LocalProviderErrorKind {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Missing => "provider-missing",
            Self::UnsupportedPlatform => "provider-unsupported-platform",
            Self::Offline => "provider-offline",
            Self::Download => "provider-download",
            Self::Integrity => "provider-integrity",
            Self::Install => "provider-install",
        }
    }
}

impl ProviderError {
    /// A stable machine-readable code for this failure.
    pub fn code(&self) -> &'static str {
        match self {
            ProviderError::NotConfigured { .. } => "provider-not-configured",
            ProviderError::Spawn { .. } => "provider-spawn",
            ProviderError::Io { .. } => "provider-io",
            ProviderError::Exited { .. } => "provider-exited",
            ProviderError::Timeout { .. } => "provider-timeout",
            ProviderError::Malformed { .. } => "provider-malformed",
            ProviderError::JsonRpc { .. } => "jsonrpc-error",
            ProviderError::Lpp(error) => error.kind.code(),
            ProviderError::NotInitialized { .. } => "provider-not-initialized",
            ProviderError::AlreadyInitialized => "provider-already-initialized",
            ProviderError::ShutDown { .. } => "provider-shutdown",
            ProviderError::ProtocolVersionMismatch { .. } => "protocol-version-mismatch",
            ProviderError::Local { kind, .. } => kind.code(),
        }
    }

    /// Build a structured LPP error.
    pub fn lpp(kind: LppErrorKind, details: Value, message: impl Into<String>) -> ProviderError {
        ProviderError::Lpp(LppError {
            kind,
            details,
            message: message.into(),
        })
    }

    /// The refusal code when this failure is a refusal, else `None`.
    pub fn refusal_code(&self) -> Option<&str> {
        match self {
            ProviderError::Lpp(error) => error.refusal_code(),
            _ => None,
        }
    }

    /// The supported protocol versions when this failure is a version
    /// mismatch, else empty.
    pub fn supported_protocol_versions(&self) -> Vec<String> {
        match self {
            ProviderError::Lpp(error) => error.supported_protocol_versions(),
            ProviderError::ProtocolVersionMismatch { supported, .. } => supported.clone(),
            _ => Vec::new(),
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderError::NotConfigured { language_id } => {
                write!(
                    f,
                    "no LPP provider is configured for language id '{language_id}'"
                )
            }
            ProviderError::Spawn { message } => {
                write!(f, "cannot start the LPP provider: {message}")
            }
            ProviderError::Io { message } => write!(f, "LPP provider I/O failure: {message}"),
            ProviderError::Exited { status, message } => match status {
                Some(status) => write!(f, "{message} (exit status {status})"),
                None => write!(f, "{message}"),
            },
            ProviderError::Timeout { method, duration } => {
                write!(
                    f,
                    "the LPP provider did not respond to '{method}' within {}ms",
                    duration.as_millis()
                )
            }
            ProviderError::Malformed { detail } => {
                write!(f, "malformed LPP provider message: {detail}")
            }
            ProviderError::JsonRpc { code, message } => {
                write!(f, "JSON-RPC error {code}: {message}")
            }
            ProviderError::Lpp(error) => write!(f, "LPP error: {error}"),
            ProviderError::NotInitialized { method } => {
                write!(
                    f,
                    "the LPP session is not initialized; cannot send '{method}'"
                )
            }
            ProviderError::AlreadyInitialized => {
                write!(f, "the LPP session is already initialized")
            }
            ProviderError::ShutDown { method } => {
                write!(f, "the LPP session is shut down; cannot send '{method}'")
            }
            ProviderError::ProtocolVersionMismatch { supported, message } => {
                if supported.is_empty() {
                    write!(f, "{message}")
                } else {
                    write!(
                        f,
                        "{message} (supported protocol versions: {})",
                        supported.join(", ")
                    )
                }
            }
            ProviderError::Local { message, .. } => write!(f, "local provider failure: {message}"),
        }
    }
}

impl std::error::Error for ProviderError {}
