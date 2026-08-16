//! Formatting helpers shared by the IR layers.
//!
//! Cutover shim (wright#143): owned by `workshop-rs`; re-exported so the Opy
//! frontend and the Workshop IR share one implementation. No independent
//! implementation lives here. Removal path: when the HIR and lowering extract
//! to `opy-rs`, this shim disappears and importers use
//! `workshop_rs::format` directly.

pub use workshop_rs::format::*;
