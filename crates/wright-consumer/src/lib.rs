//! The external consumer's public API surface (M9, issue #61).
//!
//! [`run_consumer`] drives every public embedding/tool workflow over one
//! input, proving that a consumer depending only on `wright-driver` can
//! compile/check/analyze/query and validate edits without internal IR
//! imports or CLI scraping. The `wright-consumer` binary is a thin wrapper
//! over it.

pub mod workflow;

pub use workflow::run_consumer;
