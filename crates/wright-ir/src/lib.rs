//! Wright's typed intermediate-representation core.
//!
//! This crate owns the durable compiler data model described by
//! [`docs/adr/0006-rust-ir-core.md`](../../docs/adr/0006-rust-ir-core.md):
//!
//! * [`ids`] — strongly typed IDs for every stable identity;
//! * [`arena`] — bounds-checked arena storage for nodes;
//! * [`source`] — the source-file model and spans;
//! * [`error`] — structured IR errors.
//!
//! The Opy HIR and Workshop IR models (`hir`, `wir`) and the lowering
//! boundary (`lower`) live in this crate as well; see the module
//! documentation there.
//!
//! The crate is protocol-agnostic: it does not depend on the
//! `wright/opy-hir` bridge types in `wright-core`, on OverPy, or on any other
//! crate.

pub mod arena;
pub mod error;
pub mod ids;
pub mod source;
