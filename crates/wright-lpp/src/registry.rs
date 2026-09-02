//! Provider discovery and configuration by opaque language id.
//!
//! A [`ProviderRegistry`] maps opaque language id strings to provider
//! configurations. Nothing in this module (or elsewhere in the crate)
//! branches on a particular source language: `x-demo-lang` or any other id
//! is just a key. When no provider is configured for a language id,
//! [`ProviderRegistry::spawn`] refuses explicitly with
//! `ProviderError::NotConfigured` — there is no fallback.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use crate::error::ProviderError;
use crate::provider::StdioLanguageProvider;

/// One configured provider, keyed by its opaque language id.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    /// The opaque language id this provider serves (the registry key).
    pub language_id: String,
    /// The provider binary to spawn.
    pub command: PathBuf,
    /// Extra arguments passed to the binary.
    pub args: Vec<String>,
    /// Per-request timeout for this provider.
    pub request_timeout: Duration,
}

impl ProviderConfig {
    /// Build a configuration for `language_id` running `command` with `args`
    /// and the default request timeout.
    pub fn new(
        language_id: impl Into<String>,
        command: impl Into<PathBuf>,
        args: Vec<String>,
    ) -> ProviderConfig {
        ProviderConfig {
            language_id: language_id.into(),
            command: command.into(),
            args,
            request_timeout: Duration::from_secs(30),
        }
    }
}

/// A registry error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// A provider is already registered for the language id.
    DuplicateLanguage { language_id: String },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryError::DuplicateLanguage { language_id } => write!(
                f,
                "a provider is already registered for language id '{language_id}'"
            ),
        }
    }
}

impl std::error::Error for RegistryError {}

/// Provider discovery: a registry of provider configurations keyed by
/// opaque language id.
#[derive(Debug, Clone, Default)]
pub struct ProviderRegistry {
    providers: BTreeMap<String, ProviderConfig>,
}

impl ProviderRegistry {
    /// An empty registry.
    pub fn new() -> ProviderRegistry {
        ProviderRegistry::default()
    }

    /// Register a provider configuration. Refuses duplicates.
    pub fn register(&mut self, config: ProviderConfig) -> Result<(), RegistryError> {
        if self.providers.contains_key(&config.language_id) {
            return Err(RegistryError::DuplicateLanguage {
                language_id: config.language_id.clone(),
            });
        }
        self.providers.insert(config.language_id.clone(), config);
        Ok(())
    }

    /// Whether a provider has been explicitly registered for `language_id`.
    pub fn contains(&self, language_id: &str) -> bool {
        self.providers.contains_key(language_id)
    }

    /// Spawn a fresh provider session for `language_id`.
    ///
    /// Refuses explicitly when no provider is configured for the id
    /// (`ProviderError::NotConfigured`); there is no fallback to in-process
    /// semantics.
    pub fn spawn(&self, language_id: &str) -> Result<StdioLanguageProvider, ProviderError> {
        let config =
            self.providers
                .get(language_id)
                .ok_or_else(|| ProviderError::NotConfigured {
                    language_id: language_id.to_string(),
                })?;
        StdioLanguageProvider::spawn(&config.command, &config.args, config.request_timeout)
    }
}
