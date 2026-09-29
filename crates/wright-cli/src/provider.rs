use wright_driver::{OpyProviderConfig, OpyProviderError, ResolvedOpyProvider};

/// The first-party OPY provider recorded in the provider store, if usable.
pub(crate) fn installed() -> Result<Option<ResolvedOpyProvider>, OpyProviderError> {
    OpyProviderConfig::default().installed()
}

/// The latest published OPY provider version (pointer fetch only).
pub(crate) fn latest_version() -> Result<String, OpyProviderError> {
    OpyProviderConfig::default().latest_version()
}

/// Explicitly install/update the first-party OPY provider.
pub(crate) fn update(version: Option<&str>) -> Result<ResolvedOpyProvider, OpyProviderError> {
    OpyProviderConfig::default().update(version)
}
