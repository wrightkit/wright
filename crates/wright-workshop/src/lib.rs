//! Wright's native Workshop language model (milestone M5).
//!
//! This crate owns the Workshop-language foundation so localized vanilla
//! Workshop text can be parsed, analyzed, transformed, and emitted without an
//! `.opy` round-trip:
//!
//! * [`catalog`] — the Wright-owned canonical Workshop catalog: stable
//!   semantic identities, kinds, parameters, and locale aliases;
//! * `lexer`/`parser` — the native localized Workshop frontend producing
//!   validated Workshop IR;
//! * `emitter` — deterministic localized Workshop emission from WIR;
//! * `detect` — Workshop client-language detection and explicit override;
//! * `roundtrip` — cross-language round-trip validation.
//!
//! The catalog is locale-independent at the identity layer: analyzer and WIR
//! APIs never need locale-specific strings to identify a builtin.

pub mod catalog;
mod error;
pub mod lexer;
pub mod parser;
pub mod validate;

pub use error::WorkshopError;
