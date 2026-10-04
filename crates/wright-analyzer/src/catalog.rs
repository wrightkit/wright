//! Process-shared access to the built-in Workshop catalog.
//!
//! `workshop_rs::catalog::Catalog::builtin()` re-parses the embedded catalog
//! data and rebuilds every lookup index on each call. Sessions, providers,
//! and lint-rule loading all consume the same immutable dataset, so it is
//! built at most once per process and shared through `Arc`.

use std::sync::{Arc, OnceLock};

use workshop_rs::WorkshopError;
use workshop_rs::catalog::Catalog;

static BUILTIN: OnceLock<Result<Arc<Catalog>, WorkshopError>> = OnceLock::new();

/// The process-shared built-in Workshop catalog.
///
/// The catalog is parsed and indexed on the first call and the same instance
/// is returned by every later call. It is immutable for the process lifetime;
/// callers must not mutate the returned catalog.
pub fn builtin() -> Result<Arc<Catalog>, WorkshopError> {
    BUILTIN
        .get_or_init(|| {
            hotpath::measure_block!("session::catalog_builtin", Catalog::builtin()).map(Arc::new)
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_returns_one_shared_instance() {
        let first = builtin().expect("builtin catalog");
        let second = builtin().expect("builtin catalog");
        assert!(Arc::ptr_eq(&first, &second));
    }
}
