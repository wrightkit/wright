use std::fmt;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct LppError {
    pub kind: LppErrorKind,
    pub details: Value,
    pub message: String,
}

impl fmt::Display for LppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl LppError {
    pub fn refusal_code(&self) -> Option<&str> {
        if self.kind == LppErrorKind::Refusal {
            self.details.get("refusalCode").and_then(Value::as_str)
        } else {
            None
        }
    }

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

    pub fn capability(&self) -> Option<&str> {
        if self.kind == LppErrorKind::CapabilityUnavailable {
            self.details.get("capability").and_then(Value::as_str)
        } else {
            None
        }
    }

    pub fn method(&self) -> Option<&str> {
        if self.kind == LppErrorKind::CapabilityUnavailable {
            self.details.get("method").and_then(Value::as_str)
        } else {
            None
        }
    }
}

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
    Refusal,
    Unknown(String),
}

impl LppErrorKind {
    pub fn from_wire(name: &str) -> LppErrorKind {
        match name {
            "protocolVersionMismatch" => Self::ProtocolVersionMismatch,
            "invalidRequest" => Self::InvalidRequest,
            "invalidLanguage" => Self::InvalidLanguage,
            "invalidDocument" => Self::InvalidDocument,
            "invalidEntry" => Self::InvalidEntry,
            "projectLoadFailed" => Self::ProjectLoadFailed,
            "invalidPosition" => Self::InvalidPosition,
            "invalidArtifact" => Self::InvalidArtifact,
            "capabilityUnavailable" => Self::CapabilityUnavailable,
            "refusal" => Self::Refusal,
            other => Self::Unknown(other.to_string()),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::ProtocolVersionMismatch => "protocol-version-mismatch",
            Self::InvalidRequest => "invalid-request",
            Self::InvalidLanguage => "invalid-language",
            Self::InvalidDocument => "invalid-document",
            Self::InvalidEntry => "invalid-entry",
            Self::ProjectLoadFailed => "project-load-failed",
            Self::InvalidPosition => "invalid-position",
            Self::InvalidArtifact => "invalid-artifact",
            Self::CapabilityUnavailable => "capability-unavailable",
            Self::Refusal => "refusal",
            Self::Unknown(_) => "lpp-error",
        }
    }
}

impl fmt::Display for LppErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(name) => write!(f, "lpp error '{name}'"),
            known => write!(f, "{}", known.code()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderError {
    NotConfigured {
        language_id: String,
    },
    Spawn {
        message: String,
    },
    Io {
        message: String,
    },
    Exited {
        status: Option<i32>,
        message: String,
    },
    Timeout {
        method: String,
        duration: std::time::Duration,
    },
    Malformed {
        detail: String,
    },
    JsonRpc {
        code: i64,
        message: String,
    },
    Lpp(LppError),
    NotInitialized {
        method: String,
    },
    AlreadyInitialized,
    ShutDown {
        method: String,
    },
    ProtocolVersionMismatch {
        supported: Vec<String>,
        message: String,
    },
    Local {
        kind: LocalProviderErrorKind,
        message: String,
    },
}

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
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotConfigured { .. } => "provider-not-configured",
            Self::Spawn { .. } => "provider-spawn",
            Self::Io { .. } => "provider-io",
            Self::Exited { .. } => "provider-exited",
            Self::Timeout { .. } => "provider-timeout",
            Self::Malformed { .. } => "provider-malformed",
            Self::JsonRpc { .. } => "jsonrpc-error",
            Self::Lpp(error) => error.kind.code(),
            Self::NotInitialized { .. } => "provider-not-initialized",
            Self::AlreadyInitialized => "provider-already-initialized",
            Self::ShutDown { .. } => "provider-shutdown",
            Self::ProtocolVersionMismatch { .. } => "protocol-version-mismatch",
            Self::Local { kind, .. } => kind.code(),
        }
    }

    pub fn lpp(kind: LppErrorKind, details: Value, message: impl Into<String>) -> ProviderError {
        ProviderError::Lpp(LppError {
            kind,
            details,
            message: message.into(),
        })
    }

    pub fn refusal_code(&self) -> Option<&str> {
        match self {
            Self::Lpp(error) => error.refusal_code(),
            _ => None,
        }
    }

    pub fn supported_protocol_versions(&self) -> Vec<String> {
        match self {
            Self::Lpp(error) => error.supported_protocol_versions(),
            Self::ProtocolVersionMismatch { supported, .. } => supported.clone(),
            _ => Vec::new(),
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConfigured { language_id } => write!(
                f,
                "no LPP provider is configured for language id '{language_id}'"
            ),
            Self::Spawn { message } => write!(f, "cannot start the LPP provider: {message}"),
            Self::Io { message } => write!(f, "LPP provider I/O failure: {message}"),
            Self::Exited {
                status: Some(status),
                message,
            } => write!(f, "{message} (exit status {status})"),
            Self::Exited {
                status: None,
                message,
            } => write!(f, "{message}"),
            Self::Timeout { method, duration } => write!(
                f,
                "the LPP provider did not respond to '{method}' within {}ms",
                duration.as_millis()
            ),
            Self::Malformed { detail } => write!(f, "malformed LPP provider message: {detail}"),
            Self::JsonRpc { code, message } => write!(f, "JSON-RPC error {code}: {message}"),
            Self::Lpp(error) => write!(f, "LPP error: {error}"),
            Self::NotInitialized { method } => write!(
                f,
                "the LPP session is not initialized; cannot send '{method}'"
            ),
            Self::AlreadyInitialized => write!(f, "the LPP session is already initialized"),
            Self::ShutDown { method } => {
                write!(f, "the LPP session is shut down; cannot send '{method}'")
            }
            Self::ProtocolVersionMismatch { supported, message } if supported.is_empty() => {
                write!(f, "{message}")
            }
            Self::ProtocolVersionMismatch { supported, message } => write!(
                f,
                "{message} (supported protocol versions: {})",
                supported.join(", ")
            ),
            Self::Local { message, .. } => write!(f, "local provider failure: {message}"),
        }
    }
}

impl std::error::Error for ProviderError {}
