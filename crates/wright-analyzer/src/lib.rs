pub mod analysis;
pub mod cfg;
pub mod declarative;
pub mod registry;
pub mod service;
pub mod symbols;

pub mod canonical {
    pub use crate::analysis::{
        Boundedness, EvidenceClass, Finding, Severity, analyze, persistent_objects,
    };
    pub use crate::cfg::cfg_response;
    pub use crate::service::{
        ErrorInfo, Origin, Request, Response, SERVICE_NAME, SERVICE_VERSION, SemanticService,
    };
    pub use crate::symbols::{
        Id, Reference, ReferenceKind, RuleId, SemanticIndex, Symbol, SymbolId, SymbolKind,
        UsageSummary,
    };
}
