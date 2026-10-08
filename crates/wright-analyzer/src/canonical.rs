mod analysis;
mod cfg;
mod facts;
mod resolve;
mod service;
mod symbols;
mod traversal;

pub use analysis::{Finding, LintFix, analyze};
pub use cfg::BLOCK_KINDS;
pub(crate) use cfg::matching_end;
pub use service::SemanticService;
pub use symbols::*;
