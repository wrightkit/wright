//! Wright's Language Provider Protocol (LPP) v1 client (#142).
//!
//! This crate is the Wright-owned client/runtime side of the Language
//! Provider Protocol. The wire contract itself — message shapes, methods,
//! error kinds, and conformance fixtures — is owned by the
//! `language-provider-protocol` repository (spec/lpp-v1.md); this crate
//! consumes that contract and never redefines it.
//!
//! # Layers
//!
//! ```text
//! ToolService / language services
//!         |
//!         |  LanguageProvider (transport-neutral, language-neutral trait)
//!         v
//!  StdioLanguageProvider    -- capability guards, typed LPP data mapping
//!         |
//!         |  JsonRpcClient   -- framing, correlation, timeouts, session phase
//!         v
//!  ChildProcess             -- spawn/kill/wait a long-running stdio provider
//!         |
//!         v
//!  a provider binary (any source language; the conformance reference
//!  provider serves the deliberately foreign `x-demo-lang` language)
//! ```
//!
//! `StdioLanguageProvider` implements the [`LanguageProvider`] trait, which
//! is the stable seam ToolService and language services consume. The trait
//! exposes provider capabilities and source-oriented operations only; JSON-
//! RPC framing, correlation ids, process handles, and timeouts stay below
//! it.
//!
//! # Language neutrality
//!
//! Nothing in this crate branches on a particular source language. Providers
//! are discovered by opaque language id strings through
//! [`registry::ProviderRegistry`]; a language id such as `x-demo-lang` is
//! just a key. When no provider is configured for a language id, or when a
//! required capability was not negotiated, the client refuses explicitly
//! with a structured [`error::ProviderError`] — there is no silent fallback
//! to in-process compiler semantics.
//!
//! # Failure classes
//!
//! Every provider interaction fails deterministically into one of the
//! structured [`error::ProviderError`] variants: spawn failure, process
//! exit, request timeout, malformed response, protocol version mismatch,
//! JSON-RPC errors, typed LPP errors (including `capabilityUnavailable`
//! refusals), and client-side session-phase violations. Each variant carries
//! a stable machine `code()`.

// LPP errors legitimately carry full structured detail values, so error
// returns are large; boxing would add an allocation per error without a
// measured benefit (the same policy as wright-driver).
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

/// The client name reported in `lpp/initialize` `clientInfo`.
pub const LPP_CLIENT_NAME: &str = "wright";

pub use client::{ClientConfig, ClientPhase, JsonRpcClient};
pub use error::{LocalProviderErrorKind, LppError, LppErrorKind, ProviderError};
pub use process::ChildProcess;
pub use provider::{LanguageProvider, NegotiatedCapabilities, StdioLanguageProvider};
pub use registry::{ProviderConfig, ProviderRegistry, RegistryError};
pub use types::{
    Capabilities, Capability, CheckResult, ClientInfo, CompileResult, Diagnostic,
    DiagnosticSeverity, Document, DocumentDiagnostics, DocumentEdits, DocumentSet, DocumentSymbols,
    InitializeResult, LanguageInfo, Location, LocationsResult, Position, ProjectEntry, Range,
    ReconstructResult, RenameResult, ServerInfo, Symbol, SymbolsResult, TextEdit,
    ValidateEditsResult, WorkshopArtifact,
};
