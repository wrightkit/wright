//! Canonical semantic analysis and query interface over [`workshop_rs::Program`].

pub use crate::analysis::{
    Boundedness, EvidenceClass, Finding, Severity, analyze, persistent_objects,
};
pub use crate::service::{Origin, Request, Response, SemanticService};
pub use crate::symbols::{
    ActionId, Reference, ReferenceKind, RuleId, SemanticIndex, Symbol, SymbolId, SymbolKind,
    UsageSummary, ValueId,
};
