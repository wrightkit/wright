mod analysis;
mod cfg;
mod facts;
mod resolve;
mod service;
mod symbols;
mod traversal;

pub use analysis::{Finding, LintFix, RepeatedValue, RepeatedValueSpan, analyze};
pub use cfg::BLOCK_KINDS;
pub(crate) use cfg::matching_end;
pub use service::{FindingMeta, SemanticService, span_json};
pub use symbols::*;
pub(crate) use traversal::{dead_actions, unwrapped};
pub use traversal::{live_actions, subroutine_callee};
