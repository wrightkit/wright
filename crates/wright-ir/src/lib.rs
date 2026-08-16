//! Wright's typed intermediate-representation core.
//!
//! This crate owns the Opy-frontend IR content described by
//! [`docs/adr/0006-rust-ir-core.md`](../../docs/adr/0006-rust-ir-core.md):
//!
//! * [`hir`] — the internal Opy HIR model (frontend semantics);
//! * [`lower`] — the HIR → Workshop IR lowering boundary.
//!
//! Canonical Workshop IR ownership moved to `workshop-rs` (wright#143,
//! ADR-0009): the `wir`, `settings`, and `source` models are no longer
//! implemented here. The generic IR infrastructure they share (`ids`,
//! `arena`, `format`, `error`) is re-exported from `workshop-rs` so the HIR
//! and the Workshop IR it lowers into share one type identity. These
//! re-export shims contain no independent implementation and disappear when
//! the HIR and lowering extract to `opy-rs`; until then, canonical
//! Workshop-type changes route to `workshop-rs`.
//!
//! The crate remains protocol-agnostic: it does not depend on the
//! `wright/opy-hir` bridge types in `wright-core`, on OverPy, or on any other
//! Wright tooling crate.

pub mod arena;
pub mod error;
pub mod format;
pub mod hir;
pub mod ids;
pub mod lower;
