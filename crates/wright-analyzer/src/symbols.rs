//! [`SemanticIndex`] is the read-only semantic query surface for tooling and
//! agents: it enumerates every symbol (global/player variables, subroutines,
//! rules), records every reference site (declarations, reads, writes, calls,
//! definitions) with its source span and rule/action/value context, and
//! answers usage and find-references queries without scraping source text.

pub use crate::canonical::{
    Reference, ReferenceKind, SemanticIndex, Symbol, SymbolId, SymbolKind, UsageSummary,
};
