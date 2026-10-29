#![allow(clippy::result_large_err)]

pub mod client;
pub mod error;
pub mod process;
pub mod provider;
pub mod registry;
pub mod types;

/// The LPP 1.0 protocol version used by document-supplied requests.
pub const LPP_PROTOCOL_VERSION: &str = "1.0";

/// The additive LPP 1.1 version that enables provider-owned project loading.
pub const LPP_PROJECT_LOADING_PROTOCOL_VERSION: &str = "1.1";

/// The LPP 1.2 version that adds provider-owned directory targets.
pub const LPP_DIRECTORY_TARGET_PROTOCOL_VERSION: &str = "1.2";

/// The additive LPP 1.3 version that adds the `sourceIdentity` compile
/// capability.
pub const LPP_SOURCE_IDENTITY_PROTOCOL_VERSION: &str = "1.3";

/// The LPP 1.4 version that adds artifact format negotiation on `lpp/compile`.
pub const LPP_ARTIFACT_NEGOTIATION_PROTOCOL_VERSION: &str = "1.4";

/// The LPP 1.5 version that adds name and signature lookup on `lpp/lookup`.
pub const LPP_LOOKUP_PROTOCOL_VERSION: &str = "1.5";

/// The LPP 1.7 version that adds the `compileInputs` compile-input
/// declaration on `lpp/compile`.
pub const LPP_COMPILE_INPUTS_PROTOCOL_VERSION: &str = "1.7";

/// Every protocol version this client implements, in ascending order —
/// the client's side of the highest-mutual-version computation LPP §19.2
/// requires after a `protocolVersionMismatch` (#638). 1.6 is excluded:
/// the client parses `resolveIds` but has no `lpp/resolveIds` method.
pub const LPP_SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &[
    LPP_PROTOCOL_VERSION,
    LPP_PROJECT_LOADING_PROTOCOL_VERSION,
    LPP_DIRECTORY_TARGET_PROTOCOL_VERSION,
    LPP_SOURCE_IDENTITY_PROTOCOL_VERSION,
    LPP_ARTIFACT_NEGOTIATION_PROTOCOL_VERSION,
    LPP_LOOKUP_PROTOCOL_VERSION,
    LPP_COMPILE_INPUTS_PROTOCOL_VERSION,
];

/// Whether `version` is at least `minimum` in `major.minor` order. LPP
/// protocol versions are cumulative: a contract element defined in 1.4 is
/// valid in every later negotiated session.
pub fn protocol_version_at_least(version: &str, minimum: &str) -> bool {
    fn tuple(version: &str) -> Option<(u32, u32)> {
        let mut parts = version.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        Some((major, minor))
    }
    match (tuple(version), tuple(minimum)) {
        (Some(version), Some(minimum)) => version >= minimum,
        _ => false,
    }
}

/// The client name reported in `lpp/initialize` `clientInfo`.
pub const LPP_CLIENT_NAME: &str = "wright";

pub use client::{ClientConfig, ClientPhase, JsonRpcClient, MAX_PROVIDER_LINE_BYTES};
pub use error::{LocalProviderErrorKind, LppError, LppErrorKind, ProviderError};
pub use process::ChildProcess;
pub use provider::{LanguageProvider, NegotiatedCapabilities, StdioLanguageProvider};
pub use registry::{ProviderConfig, ProviderRegistry, RegistryError};
pub use types::{
    Capabilities, Capability, CheckResult, ClientInfo, CompileResult, Diagnostic,
    DiagnosticSeverity, Document, DocumentDiagnostics, DocumentEdits, DocumentSet, DocumentSymbols,
    InitializeResult, LanguageInfo, Location, LocationsResult, LookupParams, LookupResult,
    LookupWithin, Position, ProjectEntry, ProjectTargetKind, Range, ReconstructResult,
    RenameResult, ServerInfo, Symbol, SymbolsResult, TextEdit, ValidateEditsResult,
    WorkshopArtifact,
};
