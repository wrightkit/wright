//! The frontend's concrete syntax tree (CST).
//!
//! Source-preserving syntax structure with spans on every node, produced by
//! [`crate::parser`] and consumed by [`crate::lower`] (and, in later
//! milestones, language services). Nodes are deliberately close to the Opy
//! HIR contract so lowering stays a small, reviewable mapping; unresolved
//! names and member accesses remain explicit until semantic resolution.

use crate::diag::Span;

/// A parsed program: declarations and rule/subroutine entries.
#[derive(Debug, Clone)]
pub struct Program {
    pub declarations: Vec<Decl>,
    pub rules: Vec<RuleEntry>,
}

/// A program-scope declaration.
#[derive(Debug, Clone)]
pub enum Decl {
    GlobalVariable {
        name: String,
        /// An explicit Workshop index (`globalvar x 100`), when given.
        index: Option<u32>,
        span: Span,
        initializer: Option<Expr>,
    },
    PlayerVariable {
        name: String,
        index: Option<u32>,
        span: Span,
        initializer: Option<Expr>,
    },
    Subroutine {
        name: String,
        span: Span,
    },
    /// A user-defined `enum`; members fold to numeric constants.
    Enum {
        name: String,
        members: Vec<(String, Span)>,
        span: Span,
    },
    /// A `macro` declaration with parameterized statement body.
    Macro {
        name: String,
        args: Vec<String>,
        body: Vec<Stmt>,
        span: Span,
    },
}

/// A rule or a subroutine definition.
#[derive(Debug, Clone)]
pub enum RuleEntry {
    Rule(Rule),
    SubroutineDef {
        name: String,
        span: Span,
        body: Vec<Stmt>,
    },
}

/// A rule with its event, conditions, and actions.
#[derive(Debug, Clone)]
pub struct Rule {
    pub name: String,
    pub span: Span,
    pub disabled: bool,
    pub event: Event,
    pub conditions: Vec<Expr>,
    pub actions: Vec<Stmt>,
}

/// A rule event or an `@Event` directive.
#[derive(Debug, Clone)]
pub struct Event {
    pub name: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

/// A statement.
#[derive(Debug, Clone)]
pub enum Stmt {
    Expr {
        expr: Expr,
        span: Span,
    },
    Assign {
        target: Expr,
        value: Expr,
        span: Span,
    },
    If {
        branches: Vec<IfBranch>,
        r#else: Option<Vec<Stmt>>,
        span: Span,
    },
    For {
        variable: Expr,
        iterable: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    Pass {
        span: Span,
    },
}

/// One condition/body pair of an `if`.
#[derive(Debug, Clone)]
pub struct IfBranch {
    pub condition: Expr,
    pub body: Vec<Stmt>,
}

/// An expression.
#[derive(Debug, Clone)]
pub enum Expr {
    Number {
        value: f64,
        text: String,
        span: Span,
    },
    String {
        value: String,
        span: Span,
    },
    Bool {
        value: bool,
        span: Span,
    },
    Null {
        span: Span,
    },
    Array {
        elements: Vec<Expr>,
        span: Span,
    },
    /// A plain function call.
    Call {
        name: String,
        args: Vec<Expr>,
        span: Span,
    },
    /// A call on a receiver (`x.f(...)`).
    ReceiverCall {
        receiver: Box<Expr>,
        name: String,
        args: Vec<Expr>,
        span: Span,
    },
    /// An unresolved identifier (resolved during lowering).
    Name {
        name: String,
        span: Span,
    },
    /// A member access `x.y` (resolved during lowering).
    Member {
        receiver: Box<Expr>,
        member: String,
        span: Span,
    },
    Index {
        array: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    Binary {
        op: String,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    Unary {
        op: String,
        operand: Box<Expr>,
        span: Span,
    },
}
