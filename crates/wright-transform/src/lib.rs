//! Transformations live in an explicit, validated pass pipeline separate from
//! read-only analysis and backend emission. Profiles select the pass set:
//!
//! * [`Profile::Off`] — no transformation at all (the clean reference path);
//! * [`Profile::Compat`] — evidence-backed, compatibility-safe passes;
//! * [`Profile::Aggressive`] — experimental profile marker; selects compat passes in v1.
//!
//! [`run`] validates the program before and after the pipeline.

pub mod pipeline;
pub mod profile;

pub use pipeline::{PassResult, PassStats, run, run_canonical};
pub use profile::Profile;
