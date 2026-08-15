//! Canonical signature context for cross-frontend enum resolution.
//!
//! The OPY semantic compatibility manifest (#109) is the single source of
//! canonical action/value signatures and their parameter enum domains. The
//! Workshop parse path needs those expected domains to resolve a bare enum
//! member spelling that is ambiguous across domains (e.g. the shared `None`
//! member of `ChaseTimeReeval` / `ChaseRateReeval` / `Invis`, #111).
//!
//! This module defines the minimal parse-context *contract* between the
//! manifest owner and the Workshop frontend. It deliberately carries no
//! signature data: the manifest data file remains the only domain table, and
//! the Workshop crate never holds its own copy.

/// Supplies the expected enum domain for a call argument during parsing.
///
/// The parser asks for the expected domain of argument `arg_index` (0-based)
/// of the call whose Workshop catalog id is `catalog_id`. Implementations
/// must return the domain only when the canonical signature pins exactly one;
/// returning `None` keeps an ambiguous bare member rejected.
pub trait ExpectedDomain {
    /// The expected enum domain for `arg_index` of the call with catalog id
    /// `catalog_id`, or `None` when the signature does not pin one.
    fn expected_domain(&self, catalog_id: &str, arg_index: usize) -> Option<&str>;
}

/// The default context: no signature pins any argument domain, so ambiguous
/// bare enum members stay rejected (the pre-#111 boundary). Used by the
/// plain [`crate::parser::parse`] entry point and callers without signature
/// metadata.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoExpectedDomain;

impl ExpectedDomain for NoExpectedDomain {
    fn expected_domain(&self, _catalog_id: &str, _arg_index: usize) -> Option<&str> {
        None
    }
}
