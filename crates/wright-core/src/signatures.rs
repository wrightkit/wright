//! Canonical signature context for ambiguous enum member resolution.
//!
//! Cutover shim (wright#143): the parse-context contract is owned by
//! `workshop-rs` (`workshop_rs::signatures`); this module re-exports it so
//! Wright's manifest owner (`wright-opy`) and the Workshop parse path keep
//! implementing/consuming one trait. No independent implementation lives
//! here. Removal path: when the OPY provider extracts to `opy-rs`, this shim
//! disappears and importers use `workshop_rs::signatures` directly.

pub use workshop_rs::signatures::*;
