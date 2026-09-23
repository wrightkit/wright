//! Each analysis produces [`Finding`]s with a stable code, a severity, a
//! human-readable message, and the offending rule/action/value and span.

use serde::{Deserialize, Serialize};

/// The severity of a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Warning,
    Info,
    Error,
}

/// How strongly a finding is supported by the available evidence.
///
/// Classifies the *kind* of evidence behind a rule's findings, not the
/// severity or the certainty of an individual finding:
///
/// * `Exact` — a structural fact of the program that holds regardless of runtime values.
/// * `StaticIndicator` — the trigger is statically known but the impact is an indicator.
/// * `Heuristic` — a documented fixed heuristic list.
/// * `RuntimeValidated` — reserved for rules whose findings are confirmed by runtime evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceClass {
    Exact,
    StaticIndicator,
    Heuristic,
    RuntimeValidated,
}

impl EvidenceClass {
    /// The stable serialized spelling of this class.
    pub fn as_str(self) -> &'static str {
        match self {
            EvidenceClass::Exact => "exact",
            EvidenceClass::StaticIndicator => "static-indicator",
            EvidenceClass::Heuristic => "heuristic",
            EvidenceClass::RuntimeValidated => "runtime-validated",
        }
    }
}

/// The boundedness evidence of a no-yield `While` loop (issue #103).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Boundedness {
    ObviouslyUnbounded,
    StaticallyBounded,
    Unknown,
}

impl Boundedness {
    pub fn as_str(self) -> &'static str {
        match self {
            Boundedness::ObviouslyUnbounded => "obviously-unbounded",
            Boundedness::StaticallyBounded => "statically-bounded",
            Boundedness::Unknown => "unknown",
        }
    }
}

pub use crate::canonical::{Finding, analyze};
