//! WIR transformation pipeline (milestone M8, issues #51/#52).
//!
//! Transformations live in an explicit, validated pass pipeline separate from
//! read-only analysis and backend emission. Profiles select the pass set:
//!
//! * [`Profile::Off`] — no transformation at all (the clean reference path);
//! * [`Profile::Compat`] — evidence-backed, compatibility-safe passes, each
//!   gated on N/E regression fixtures with before/after metrics;
//! * [`Profile::Aggressive`] — experimental profile marker; in v1 it selects
//!   the same evidence-backed compat passes (no speculative passes ship
//!   without evidence).
//!
//! Source-semantic behavior is never owned by this pipeline: declaration
//! initializers lower into synthetic Initialize rules in the
//! profile-independent HIR → WIR lowering, so profiles may only change
//! semantics-preserving representation/resource behavior (#112).
//!
//! [`run`] validates the WIR before and after the pipeline, so a pass can
//! never leave the program in an invalid state.

pub mod fold_constants;
pub mod pipeline;
pub mod profile;

pub use pipeline::{PassResult, PassStats, run};
pub use profile::Profile;
