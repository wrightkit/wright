//! Wright's Workshop-language adapter: re-exports the canonical `workshop-rs`
//! core.
//!
//! This crate is a **cutover adapter** (wright#143, ADR-0009): the canonical
//! Workshop catalog, parser, emitter, detection, round-trip, validation, and
//! Workshop IR are owned by `workshop-rs`; this crate only re-exports that
//! surface so existing `wright_workshop::…` call sites keep resolving during
//! the v0.2 cutover. It contains **no independent semantic implementation**:
//! every item below is the `workshop-rs` item.
//!
//! Removal path: migrate call sites to `workshop_rs::…` directly (the
//! re-exported paths are the `workshop-rs` paths), then delete this crate.
//! Wright tooling (CLI, services, analyzer, driver) consumes
//! `workshop-rs` for Workshop semantics; do not reintroduce Workshop
//! implementation here.

pub use workshop_rs::*;
