//! First-party language-provider management commands.

use wright_driver::{OpyProviderConfig, OpyProviderError, ResolvedOpyProvider};

/// Explicitly install/update the first-party OPY provider.
pub(crate) fn update(version: Option<&str>) -> Result<ResolvedOpyProvider, OpyProviderError> {
    OpyProviderConfig::default().update(version)
}
