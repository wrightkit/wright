//! Strongly typed IDs.
//!
//! Cutover shim (wright#143): the generic IR infrastructure is owned by
//! `workshop-rs`; this module re-exports it so the Opy HIR, the HIR → WIR
//! lowering, and the Workshop IR share one type identity. No independent
//! implementation lives here. Removal path: when the HIR and lowering extract
//! to `opy-rs`, this shim disappears and the HIR imports
//! `workshop_rs::ids` directly.

pub use workshop_rs::ids::*;
