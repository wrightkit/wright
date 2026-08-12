//! Wright's typed intermediate-representation core.
//!
//! This crate owns the durable compiler data model described by
//! [`docs/adr/0006-rust-ir-core.md`](../../docs/adr/0006-rust-ir-core.md):
//!
//! * [`ids`] — strongly typed IDs for every stable identity;
//! * [`arena`] — bounds-checked arena storage for nodes;
//! * [`source`] — the source-file model and spans;
//! * [`hir`] — the internal Opy HIR model (frontend semantics);
//! * [`wir`] — the Workshop IR model (workshop program structure);
//! * [`error`] — structured IR errors.
//!
//! The lowering boundary from [`hir`] to [`wir`] lives in [`crate::lower`]
//! (added with the first lowering milestone). The crate is protocol-agnostic:
//! it does not depend on the `wright/opy-hir` bridge types in `wright-core`,
//! on OverPy, or on any other crate.

pub mod arena;
pub mod error;
pub mod hir;
pub mod ids;
pub mod source;
pub mod wir;
