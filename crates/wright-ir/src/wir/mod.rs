//! The Workshop IR model.
//!
//! Workshop IR models the lower-level workshop program structure produced by
//! [`crate::lower`]: variables with indexes, subroutines with indexes, and
//! rules with events, conditions, actions, and values. It is Wright-owned and
//! protocol-agnostic (ADR-0006).
//!
//! Name policy: call/value `name` fields keep Wright's source-level function
//! names (`len`, `wait`, `createBeam`, `range`); mapping those to Workshop
//! presentation names is an emission concern. `debug` and `print` lower to
//! dedicated [`Action::Debug`]/[`Action::Print`] nodes, and `.append` lowers
//! to a modify action with [`ModifyOp::AppendToArray`].

mod dump;
mod validate;

use crate::arena::Arena;
use crate::ids::Id;
use crate::source::{SourceFile, Span};

/// A typed ID referencing a [`WorkshopVariable`] in the global table.
pub type GlobalVarId = Id<WorkshopVariable>;
/// A typed ID referencing a [`WorkshopVariable`] in the player table.
pub type PlayerVarId = Id<WorkshopVariable>;
/// A typed ID referencing a [`WorkshopSubroutine`].
pub type SubroutineId = Id<WorkshopSubroutine>;
/// A typed ID referencing a [`Rule`].
pub type RuleId = Id<Rule>;
/// A typed ID referencing an [`Action`] in the action arena.
pub type ActionId = Id<Action>;
/// A typed ID referencing a [`ValueNode`] in the value arena.
pub type ValueId = Id<ValueNode>;

/// The Workshop IR program: tables and arenas produced by lowering.
#[derive(Debug, Clone)]
pub struct Program {
    /// The source-file registry, copied from the source HIR so spans remain
    /// resolvable for diagnostics.
    pub files: Arena<SourceFile>,
    pub global_variables: Arena<WorkshopVariable>,
    pub player_variables: Arena<WorkshopVariable>,
    pub subroutines: Arena<WorkshopSubroutine>,
    pub rules: Arena<Rule>,
    pub values: Arena<ValueNode>,
    pub actions: Arena<Action>,
}

impl Default for Program {
    fn default() -> Self {
        Program {
            files: Arena::new(),
            global_variables: Arena::new(),
            player_variables: Arena::new(),
            subroutines: Arena::new(),
            rules: Arena::new(),
            values: Arena::new(),
            actions: Arena::new(),
        }
    }
}

impl Program {
    /// Validate structural invariants: every ID resolves and every span is
    /// valid. Returns the first violation as a structured [`IrError`].
    ///
    /// [`IrError`]: crate::error::IrError
    pub fn validate(&self) -> Result<(), crate::error::IrError> {
        validate::validate(self)
    }

    /// Render a deterministic debug dump of the workshop program.
    pub fn dump(&self) -> String {
        dump::dump(self)
    }
}

/// A workshop variable (global or player) with its assigned index.
#[derive(Debug, Clone)]
pub struct WorkshopVariable {
    pub name: String,
    /// The workshop variable index assigned during lowering.
    pub index: u32,
    pub span: Option<Span>,
    /// The exact span of the declared identifier token.
    pub name_span: Option<Span>,
    /// The non-trivial source initializer, if the declaration had one.
    pub initializer: Option<ValueId>,
}

/// A workshop subroutine with its assigned index.
#[derive(Debug, Clone)]
pub struct WorkshopSubroutine {
    pub name: String,
    pub index: u32,
    pub span: Option<Span>,
    /// The exact span of the declared identifier token.
    pub name_span: Option<Span>,
}

/// A workshop rule.
#[derive(Debug, Clone)]
pub struct Rule {
    pub name: String,
    pub span: Option<Span>,
    /// The exact span of the rule name inside its string literal.
    pub name_span: Option<Span>,
    pub disabled: bool,
    pub event: Event,
    pub conditions: Vec<ValueId>,
    pub actions: Vec<ActionId>,
}

/// A workshop event.
#[derive(Debug, Clone)]
pub enum Event {
    /// `Ongoing - Global` (from `@Event global`).
    Global,
    /// `Ongoing - Each Player` (from `@Event eachPlayer`).
    EachPlayer,
    /// A subroutine body (`def name():`), referencing the subroutine.
    Subroutine(SubroutineId),
}

/// A workshop value (expression) node with its source span.
#[derive(Debug, Clone)]
pub struct ValueNode {
    pub value: Value,
    pub span: Option<Span>,
}

/// A workshop value (expression).
#[derive(Debug, Clone)]
pub enum Value {
    Number(f64),
    String(String),
    Bool(bool),
    Null,
    Array(Vec<ValueId>),
    Vector {
        x: ValueId,
        y: ValueId,
        z: ValueId,
    },
    /// A built-in enumerated value, e.g. `Team.ALL`.
    Enum {
        value_type: String,
        value: String,
    },
    GlobalVariable(GlobalVarId),
    PlayerVariable {
        player: ValueId,
        variable: PlayerVarId,
    },
    EventPlayer,
    /// A function call over workshop values.
    Call {
        name: String,
        args: Vec<ValueId>,
    },
}

impl ValueNode {
    /// Build a value node with a source span.
    pub fn new(value: Value, span: Option<Span>) -> Self {
        ValueNode { value, span }
    }
}

/// A workshop action.
#[derive(Debug, Clone)]
pub enum Action {
    SetGlobalVariable {
        variable: GlobalVarId,
        value: ValueId,
        span: Option<Span>,
        /// The exact span of the assigned variable identifier.
        target_span: Option<Span>,
    },
    ModifyGlobalVariable {
        variable: GlobalVarId,
        op: ModifyOp,
        value: ValueId,
        span: Option<Span>,
        /// The exact span of the modified variable identifier.
        target_span: Option<Span>,
    },
    SetPlayerVariable {
        player: ValueId,
        variable: PlayerVarId,
        value: ValueId,
        span: Option<Span>,
        /// The exact span of the assigned variable identifier.
        target_span: Option<Span>,
    },
    ModifyPlayerVariable {
        player: ValueId,
        variable: PlayerVarId,
        op: ModifyOp,
        value: ValueId,
        span: Option<Span>,
        /// The exact span of the modified variable identifier.
        target_span: Option<Span>,
    },
    CallSubroutine {
        subroutine: SubroutineId,
        span: Option<Span>,
        /// The exact span of the callee identifier occurrence.
        callee_span: Option<Span>,
    },
    If {
        branches: Vec<IfBranch>,
        else_body: Option<Vec<ActionId>>,
        span: Option<Span>,
    },
    While {
        condition: ValueId,
        body: Vec<ActionId>,
        span: Option<Span>,
    },
    ForGlobalVariable {
        variable: GlobalVarId,
        start: ValueId,
        stop: ValueId,
        step: ValueId,
        body: Vec<ActionId>,
        span: Option<Span>,
        /// The exact span of the loop variable identifier.
        target_span: Option<Span>,
    },
    /// The `debug(value)` HUD debug effect.
    Debug { value: ValueId, span: Option<Span> },
    /// The `print(message)` HUD message effect.
    Print {
        message: ValueId,
        span: Option<Span>,
    },
    /// Any other action call with side effects.
    Call {
        name: String,
        args: Vec<ValueId>,
        span: Option<Span>,
    },
}

impl Action {
    /// The source span of this action, if any.
    pub fn span(&self) -> Option<Span> {
        match self {
            Action::SetGlobalVariable { span, .. }
            | Action::ModifyGlobalVariable { span, .. }
            | Action::SetPlayerVariable { span, .. }
            | Action::ModifyPlayerVariable { span, .. }
            | Action::CallSubroutine { span, .. }
            | Action::If { span, .. }
            | Action::While { span, .. }
            | Action::ForGlobalVariable { span, .. }
            | Action::Debug { span, .. }
            | Action::Print { span, .. }
            | Action::Call { span, .. } => *span,
        }
    }
}

/// One condition/body pair of an `If` action.
#[derive(Debug, Clone)]
pub struct IfBranch {
    pub condition: ValueId,
    pub body: Vec<ActionId>,
}

/// The modify operators of the v0.1 surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModifyOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    RaiseToPower,
    AppendToArray,
    RemoveFromArray,
}

impl ModifyOp {
    /// A short canonical name for dumps and diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            ModifyOp::Add => "Add",
            ModifyOp::Subtract => "Subtract",
            ModifyOp::Multiply => "Multiply",
            ModifyOp::Divide => "Divide",
            ModifyOp::Modulo => "Modulo",
            ModifyOp::RaiseToPower => "RaiseToPower",
            ModifyOp::AppendToArray => "AppendToArray",
            ModifyOp::RemoveFromArray => "RemoveFromArray",
        }
    }
}
