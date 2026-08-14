//! Wright's typed intermediate-representation core.
//!
//! This crate owns the durable compiler data model described by
//! [`docs/adr/0006-rust-ir-core.md`](../../docs/adr/0006-rust-ir-core.md):
//!
//! * [`ids`] — strongly typed IDs for every stable identity;
//! * [`arena`] — bounds-checked arena storage for nodes;
//! * [`source`] — the source-file model and spans;
//! * [`settings`] — the neutral custom-game-settings carrier and table;
//! * [`hir`] — the internal Opy HIR model (frontend semantics);
//! * [`wir`] — the Workshop IR model (workshop program structure);
//! * [`lower`] — the HIR → Workshop IR lowering boundary;
//! * [`error`] — structured IR errors.
//!
//! The crate is protocol-agnostic: it does not depend on the
//! `wright/opy-hir` bridge types in `wright-core`, on OverPy, or on any other
//! crate.

pub mod arena;
pub mod error;
pub mod format;
pub mod hir;
pub mod ids;
pub mod lower;
pub mod settings;
pub mod source;
pub mod wir;
