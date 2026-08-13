//! The neutral settings carrier (#86).
//!
//! A typed, non-serde tree for custom-game-settings blocks shared by
//! wright-core (validation) and wright-workshop (emission). wright-ir is the
//! only neutral layer both crates see; the tree is a carrier — settings are
//! carried and emitted, never interpreted by lowering/analysis. The
//! fixture-evidenced name table lives in [`table`].

pub mod table;

use crate::source::Span;

/// A settings block: `settings { ... }` with its typed children.
#[derive(Debug, Clone)]
pub struct Settings {
    pub span: Option<Span>,
    pub children: Vec<SettingsNode>,
}

/// One member of a settings group.
#[derive(Debug, Clone)]
pub enum SettingsNode {
    Group {
        name: String,
        children: Vec<SettingsNode>,
        span: Option<Span>,
    },
    Number {
        name: String,
        value: f64,
        span: Option<Span>,
    },
    Bool {
        name: String,
        value: bool,
        span: Option<Span>,
    },
    String {
        name: String,
        value: String,
        span: Option<Span>,
    },
    List {
        name: String,
        elements: Vec<SettingsListElement>,
        span: Option<Span>,
    },
}

/// One element of a settings list (corpus lists are all strings).
#[derive(Debug, Clone)]
pub struct SettingsListElement {
    pub value: String,
    pub span: Option<Span>,
}

impl SettingsNode {
    /// The source span of this node, if any.
    pub fn span(&self) -> Option<Span> {
        match self {
            SettingsNode::Group { span, .. }
            | SettingsNode::Number { span, .. }
            | SettingsNode::Bool { span, .. }
            | SettingsNode::String { span, .. }
            | SettingsNode::List { span, .. } => *span,
        }
    }

    /// The key name of this node.
    pub fn name(&self) -> &str {
        match self {
            SettingsNode::Group { name, .. }
            | SettingsNode::Number { name, .. }
            | SettingsNode::Bool { name, .. }
            | SettingsNode::String { name, .. }
            | SettingsNode::List { name, .. } => name,
        }
    }
}
