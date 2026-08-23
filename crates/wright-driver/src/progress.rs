//! Transport-neutral workflow progress events.
//!
//! The driver reports semantic workflow boundaries without terminal strings,
//! ANSI, timing, or presentation policy. CLI and embedding consumers may
//! observe these events independently.

use std::sync::Arc;

/// A real orchestration phase exposed to interested consumers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgressPhase {
    /// Resolve the input path/stdin and detect its source kind.
    InputResolution,
    /// Load a multi-file project boundary.
    ProjectLoading,
    /// Parse source or protocol input.
    Parsing,
    /// Validate a parsed model.
    Validation,
    /// Lower a validated model into the shared representation.
    Lowering,
    /// Run semantic queries or analysis.
    SemanticAnalysis,
    /// Execute configured lint rules.
    Linting,
    /// Emit compiled Workshop text.
    Emission,
    /// Reconstruct a source-language project.
    Conversion,
}

/// The unit associated with optional bounded phase metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgressUnit {
    Files,
    Rules,
}

/// A phase transition emitted by [`crate::CompilerSession`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgressEvent {
    pub phase: ProgressPhase,
    pub count: Option<usize>,
    pub unit: Option<ProgressUnit>,
}

impl ProgressEvent {
    pub const fn new(phase: ProgressPhase) -> Self {
        Self {
            phase,
            count: None,
            unit: None,
        }
    }

    pub const fn with_count(phase: ProgressPhase, count: usize, unit: ProgressUnit) -> Self {
        Self {
            phase,
            count: Some(count),
            unit: Some(unit),
        }
    }
}

/// Receives semantic workflow phase transitions from a compiler session.
pub trait ProgressObserver: Send + Sync {
    fn on_progress(&self, event: ProgressEvent);
}

/// Convenience alias for observers shared with a running session.
pub type SharedProgressObserver = Arc<dyn ProgressObserver>;
