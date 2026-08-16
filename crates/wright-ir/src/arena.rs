//! Bounds-checked arena storage for nodes.
//!
//! Cutover shim (wright#143): the generic IR infrastructure is owned by
//! `workshop-rs`; this module re-exports it so the Opy HIR and the Workshop IR
//! share one arena/ID identity. No independent implementation lives here.
//! Removal path: when the HIR and lowering extract to `opy-rs`, this shim
//! disappears and the HIR imports `workshop_rs::arena` directly.

pub use workshop_rs::arena::*;
