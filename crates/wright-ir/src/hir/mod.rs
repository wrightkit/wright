//! The internal Opy HIR model.
//!
//! This is the compiler-side form of the semantics carried by the
//! `wright/opy-hir` bridge protocol (ADR-0005), with three differences that
//! matter for a durable data model (ADR-0006):
//!
//! * symbol references are strongly typed IDs (`GlobalVarId`,
//!   `PlayerVarId`, `SubroutineId`, `ConstantId`, `MacroId`) instead of name
//!   strings;
//! * binary/unary operators are typed enums instead of operator strings;
//! * statements and expressions live in append-only arenas addressed by
//!   `StmtId`/`ExprId`.
//!
//! The model is built from a validated protocol payload by
//! `wright_core::hir::convert`, so it is valid by construction; it still
//! offers [`Program::validate`] to reject dangling references and invalid
//! spans, and [`Program::dump`] for deterministic debug output.

mod dump;
mod validate;

use crate::arena::Arena;
use crate::ids::Id;
use workshop_rs::source::{FileId, SourceFile, Span};

/// A typed ID referencing a [`GlobalVar`].
pub type GlobalVarId = Id<GlobalVar>;
/// A typed ID referencing a [`PlayerVar`].
pub type PlayerVarId = Id<PlayerVar>;
/// A typed ID referencing a [`Subroutine`].
pub type SubroutineId = Id<Subroutine>;
/// A typed ID referencing a [`Constant`].
pub type ConstantId = Id<Constant>;
/// A typed ID referencing a [`Macro`].
pub type MacroId = Id<Macro>;
/// A typed ID referencing a [`Rule`].
pub type RuleId = Id<Rule>;
/// A typed ID referencing a [`Stmt`] in the statement arena.
pub type StmtId = Id<Stmt>;
/// A typed ID referencing an [`Expr`] in the expression arena.
pub type ExprId = Id<Expr>;
/// A typed ID referencing a user [`EnumDecl`].
pub type EnumId = Id<EnumDecl>;
/// A typed ID referencing a user [`Function`].
pub type FunctionId = Id<Function>;

/// The internal Opy HIR program: an arena-backed forest of symbols, rules,
/// statements, and expressions over one file registry.
#[derive(Debug, Clone)]
pub struct Program {
    pub files: Arena<SourceFile>,
    pub globals: Arena<GlobalVar>,
    pub players: Arena<PlayerVar>,
    pub subroutines: Arena<Subroutine>,
    pub constants: Arena<Constant>,
    pub macros: Arena<Macro>,
    /// User-defined enum declarations (frontend-neutral; distinct from the
    /// builtin Workshop catalog enums).
    pub enums: Arena<EnumDecl>,
    /// User-defined value/void functions with parameters (frontend-neutral;
    /// distinct from Workshop subroutines).
    pub functions: Arena<Function>,
    pub rules: Arena<Rule>,
    pub stmts: Arena<Stmt>,
    pub exprs: Arena<Expr>,
    /// The custom-game-settings carrier, copied through from the protocol
    /// (inert in the internal model, #86). The settings model is owned by
    /// `workshop-rs` (wright#143).
    pub settings: Option<workshop_rs::settings::Settings>,
}

impl Default for Program {
    fn default() -> Self {
        Program {
            files: Arena::new(),
            globals: Arena::new(),
            players: Arena::new(),
            subroutines: Arena::new(),
            constants: Arena::new(),
            macros: Arena::new(),
            enums: Arena::new(),
            functions: Arena::new(),
            rules: Arena::new(),
            stmts: Arena::new(),
            exprs: Arena::new(),
            settings: None,
        }
    }
}

impl Program {
    /// The file for a file ID, if in range.
    pub fn file(&self, file: FileId) -> Option<&SourceFile> {
        self.files.get(file)
    }

    /// Validate structural invariants: every ID resolves and every span is
    /// valid. Returns the first violation as a structured [`IrError`].
    ///
    /// [`IrError`]: crate::error::IrError
    pub fn validate(&self) -> Result<(), crate::error::IrError> {
        validate::validate(self)
    }

    /// Render a deterministic debug dump of the program.
    pub fn dump(&self) -> String {
        dump::dump(self)
    }
}

/// A global variable declaration.
#[derive(Debug, Clone)]
pub struct GlobalVar {
    pub name: String,
    /// The explicit source index, when the source requested one.
    pub index: Option<u32>,
    pub span: Option<Span>,
    /// The exact span of the declared identifier token.
    pub name_span: Option<Span>,
    /// A non-trivial source initializer, when present.
    pub initializer: Option<ExprId>,
}

/// A player variable declaration.
#[derive(Debug, Clone)]
pub struct PlayerVar {
    pub name: String,
    pub index: Option<u32>,
    pub span: Option<Span>,
    /// The exact span of the declared identifier token.
    pub name_span: Option<Span>,
    pub initializer: Option<ExprId>,
}

/// A subroutine: either declared (`subroutine name`), defined
/// (`def name():`), or both. The body, when present, is the definition.
#[derive(Debug, Clone)]
pub struct Subroutine {
    pub name: String,
    pub index: Option<u32>,
    /// Span of the `subroutine name` declaration, when present.
    pub decl_span: Option<Span>,
    /// The exact span of the declared identifier token.
    pub decl_name_span: Option<Span>,
    /// The definition body, when the source defined one.
    pub body: Option<SubroutineBody>,
}

/// The body of a `def name():` definition.
#[derive(Debug, Clone)]
pub struct SubroutineBody {
    pub span: Option<Span>,
    /// The exact span of the defined identifier token in `def name():`.
    pub name_span: Option<Span>,
    pub statements: Vec<StmtId>,
}

/// A source-level constant (`macro name = value`).
#[derive(Debug, Clone)]
pub struct Constant {
    pub name: String,
    pub span: Option<Span>,
    pub value: ExprId,
}

/// A source-level function macro (`macro name(args): body`).
#[derive(Debug, Clone)]
pub struct Macro {
    pub name: String,
    pub args: Vec<String>,
    pub span: Option<Span>,
    pub body: Vec<StmtId>,
}

/// A user-defined enum declaration (frontend-neutral). Built-in Workshop
/// enums resolve through the catalog instead; this node carries user-defined
/// enum identity so member references stay type-distinct from numbers.
#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: String,
    pub span: Option<Span>,
    pub members: Vec<EnumMember>,
}

/// One member of a user enum declaration.
#[derive(Debug, Clone)]
pub struct EnumMember {
    pub name: String,
    pub span: Option<Span>,
}

/// A user-defined function with parameters and an optional return type.
///
/// Frontend-neutral: OSTW expression-bodied value functions and
/// non-subroutine `void` functions are inlined at their call sites by the
/// reference; this node records the declaration (parameters, defaults,
/// return type, body) so a call site can be expanded without re-opening
/// frontend ownership. Workshop subroutines (rule-named `void` functions)
/// use [`Subroutine`] instead.
#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    /// The declared return type (`None` for `void`).
    pub return_type: Option<TypeName>,
    pub body: FunctionBody,
    pub span: Option<Span>,
    /// The exact span of the declared identifier token.
    pub name_span: Option<Span>,
}

/// The body of a user function: a single expression (value function) or a
/// statement list (`void` function).
#[derive(Debug, Clone)]
pub enum FunctionBody {
    Expression(ExprId),
    Statements(Vec<StmtId>),
}

/// A typed function parameter.
#[derive(Debug, Clone)]
pub struct Param {
    pub type_name: Option<TypeName>,
    pub name: String,
    /// The default value expression, when the parameter has one.
    pub default: Option<ExprId>,
    pub span: Option<Span>,
}

/// A frontend-neutral type reference (name, array depth, pipe unions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeName {
    pub name: String,
    /// The number of trailing `[]` array markers.
    pub array_depth: u32,
    /// Additional union alternatives (`A | B`).
    pub unions: Vec<TypeName>,
    pub span: Option<Span>,
}

impl TypeName {
    /// The display form of this type (`Hero[]`, `Team | Number`, `Number`).
    pub fn display(&self) -> String {
        let mut out = self.name.to_string();
        for _ in 0..self.array_depth {
            out.push_str("[]");
        }
        for union in &self.unions {
            out.push_str(" | ");
            out.push_str(&union.display());
        }
        out
    }

    /// Whether this type is the plain `void` type (no arrays/unions).
    pub fn is_void(&self) -> bool {
        self.name == "void" && self.array_depth == 0 && self.unions.is_empty()
    }
}

/// A rule with its event, conditions, and actions.
#[derive(Debug, Clone)]
pub struct Rule {
    pub name: String,
    pub span: Option<Span>,
    /// The exact span of the rule name inside its string literal.
    pub name_span: Option<Span>,
    pub disabled: bool,
    pub event: Event,
    /// The rule priority (frontend-neutral; the reference orders rules by
    /// priority, lower first).
    pub priority: Option<i32>,
    pub conditions: Vec<ExprId>,
    pub actions: Vec<StmtId>,
}

/// A rule event.
#[derive(Debug, Clone)]
pub struct Event {
    pub name: String,
    pub args: Vec<ExprId>,
    pub span: Option<Span>,
}

/// One condition/body pair of an `if` statement.
#[derive(Debug, Clone)]
pub struct IfBranch {
    pub condition: ExprId,
    pub body: Vec<StmtId>,
}

/// A statement.
#[derive(Debug, Clone)]
pub enum Stmt {
    /// An expression statement, typically a call with side effects.
    Expr {
        expr: ExprId,
        span: Option<Span>,
    },
    /// Assignment. Compound assignments were desugared by the frontend.
    Assign {
        target: ExprId,
        value: ExprId,
        span: Option<Span>,
    },
    /// Conditional with any number of branches and an optional else body.
    If {
        branches: Vec<IfBranch>,
        else_body: Option<Vec<StmtId>>,
        span: Option<Span>,
    },
    /// Iteration over a range, binding a global variable.
    For {
        variable: GlobalVarId,
        iterable: ExprId,
        body: Vec<StmtId>,
        span: Option<Span>,
        /// The exact span of the loop variable identifier.
        variable_span: Option<Span>,
    },
    /// A C-style `for (init; condition; step)` loop binding a global
    /// variable. Frontend-neutral: the reference emits
    /// `For Global Variable(variable, start, condition, step)`.
    CFor {
        variable: GlobalVarId,
        start: Option<ExprId>,
        condition: Option<ExprId>,
        step: Option<ExprId>,
        body: Vec<StmtId>,
        span: Option<Span>,
    },
    /// Iteration over an array, binding a global counter variable; body
    /// references to the loop element lower to `Index(iterable, counter)`.
    /// (The reference emits `For Global Variable(counter, 0, Count Of(arr), 1)`
    /// with the element rewritten to `Value In Array(arr, counter)`.)
    Foreach {
        variable: GlobalVarId,
        iterable: ExprId,
        body: Vec<StmtId>,
        span: Option<Span>,
    },
    /// A loop.
    While {
        condition: ExprId,
        body: Vec<StmtId>,
        span: Option<Span>,
    },
    /// A `switch` over a value with case/default arms and explicit
    /// fallthrough (frontend-neutral; the reference lowers to a jump table).
    Switch {
        value: ExprId,
        cases: Vec<SwitchCase>,
        span: Option<Span>,
    },
    /// Return from a function (`None` value for `return;`).
    Return {
        value: Option<ExprId>,
        span: Option<Span>,
    },
    Break {
        span: Option<Span>,
    },
    Continue {
        span: Option<Span>,
    },
    /// Call a subroutine.
    CallSubroutine {
        subroutine: SubroutineId,
        span: Option<Span>,
        /// The exact span of the callee identifier occurrence.
        callee_span: Option<Span>,
    },
    /// A no-op emitted by the frontend.
    Pass {
        span: Option<Span>,
    },
}

/// One `case`/`default` arm of a switch (`None` value for `default`).
#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub value: Option<ExprId>,
    pub body: Vec<StmtId>,
    pub span: Option<Span>,
}

impl Stmt {
    /// The source span of this statement, if any.
    pub fn span(&self) -> Option<Span> {
        match self {
            Stmt::Expr { span, .. }
            | Stmt::Assign { span, .. }
            | Stmt::If { span, .. }
            | Stmt::For { span, .. }
            | Stmt::CFor { span, .. }
            | Stmt::Foreach { span, .. }
            | Stmt::While { span, .. }
            | Stmt::Switch { span, .. }
            | Stmt::Return { span, .. }
            | Stmt::Break { span }
            | Stmt::Continue { span }
            | Stmt::CallSubroutine { span, .. }
            | Stmt::Pass { span } => *span,
        }
    }
}

/// An expression.
#[derive(Debug, Clone)]
pub enum Expr {
    /// A numeric literal with its source spelling.
    Number {
        value: f64,
        text: String,
        span: Option<Span>,
    },
    /// A string literal without format placeholders.
    String { value: String, span: Option<Span> },
    /// A boolean literal.
    Bool { value: bool, span: Option<Span> },
    /// The null literal.
    Null { span: Option<Span> },
    /// An array literal, possibly empty.
    Array {
        elements: Vec<ExprId>,
        span: Option<Span>,
    },
    /// A vector literal `vect(x, y, z)`.
    Vector {
        x: ExprId,
        y: ExprId,
        z: ExprId,
        span: Option<Span>,
    },
    /// A built-in enumerated value, e.g. `Team.ALL`.
    Enum {
        value_type: String,
        value: String,
        span: Option<Span>,
    },
    /// A member of a user-defined enum (frontend-neutral; builtin enums use
    /// [`Expr::Enum`] through the catalog).
    UserEnum {
        enum_id: EnumId,
        member: String,
        span: Option<Span>,
    },
    /// A reference to a global variable.
    GlobalVar {
        variable: GlobalVarId,
        span: Option<Span>,
    },
    /// A reference to a player variable on a player expression.
    PlayerVar {
        player: ExprId,
        variable: PlayerVarId,
        span: Option<Span>,
    },
    /// The `eventPlayer` pseudo-symbol.
    EventPlayer { span: Option<Span> },
    /// A reference to a source-level constant.
    Constant {
        constant: ConstantId,
        span: Option<Span>,
    },
    /// A call to a user function (inlined by the reference at call sites).
    UserCall {
        function: FunctionId,
        args: Vec<ExprId>,
        span: Option<Span>,
    },
    /// A reference to a named parameter of the enclosing user function
    /// (substituted by name during #119 inlining).
    Param { name: String, span: Option<Span> },
    /// A function call.
    Call {
        name: String,
        args: Vec<ExprId>,
        span: Option<Span>,
    },
    /// A member/extension call `receiver.name(args)`.
    ReceiverCall {
        receiver: ExprId,
        name: String,
        args: Vec<ExprId>,
        span: Option<Span>,
    },
    /// A source-level macro invocation kept explicit.
    MacroCall {
        macro_: MacroId,
        args: Vec<ExprId>,
        span: Option<Span>,
    },
    /// A reference to a macro parameter inside a macro body.
    MacroParam { name: String, span: Option<Span> },
    /// A binary operation over typed operators.
    Binary {
        op: BinaryOp,
        left: ExprId,
        right: ExprId,
        span: Option<Span>,
    },
    /// A unary operation over typed operators.
    Unary {
        op: UnaryOp,
        operand: ExprId,
        span: Option<Span>,
    },
    /// Indexing `array[index]`.
    Index {
        array: ExprId,
        index: ExprId,
        span: Option<Span>,
    },
    /// A string with `{0}`, `{1}`-style placeholders and their arguments.
    Format {
        text: String,
        args: Vec<ExprId>,
        span: Option<Span>,
    },
    /// A conditional expression `condition ? then_value : else_value`
    /// (frontend-neutral; the reference emits `If-Then-Else`).
    Ternary {
        condition: ExprId,
        then_value: ExprId,
        else_value: ExprId,
        span: Option<Span>,
    },
    /// A type cast `<TypeName>value` (frontend-neutral type coercion).
    Cast {
        type_name: TypeName,
        value: ExprId,
        span: Option<Span>,
    },
}

impl Expr {
    /// The source span of this expression, if any.
    pub fn span(&self) -> Option<Span> {
        match self {
            Expr::Number { span, .. }
            | Expr::String { span, .. }
            | Expr::Bool { span, .. }
            | Expr::Null { span }
            | Expr::Array { span, .. }
            | Expr::Vector { span, .. }
            | Expr::Enum { span, .. }
            | Expr::UserEnum { span, .. }
            | Expr::GlobalVar { span, .. }
            | Expr::PlayerVar { span, .. }
            | Expr::EventPlayer { span }
            | Expr::Constant { span, .. }
            | Expr::UserCall { span, .. }
            | Expr::Param { span, .. }
            | Expr::Call { span, .. }
            | Expr::ReceiverCall { span, .. }
            | Expr::MacroCall { span, .. }
            | Expr::MacroParam { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Unary { span, .. }
            | Expr::Index { span, .. }
            | Expr::Format { span, .. }
            | Expr::Ternary { span, .. }
            | Expr::Cast { span, .. } => *span,
        }
    }

    /// A short kind name for diagnostics.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Expr::Number { .. } => "number",
            Expr::String { .. } => "string",
            Expr::Bool { .. } => "bool",
            Expr::Null { .. } => "null",
            Expr::Array { .. } => "array",
            Expr::Vector { .. } => "vector",
            Expr::Enum { .. } => "enum",
            Expr::UserEnum { .. } => "userEnum",
            Expr::GlobalVar { .. } => "globalVar",
            Expr::PlayerVar { .. } => "playerVar",
            Expr::EventPlayer { .. } => "eventPlayer",
            Expr::Constant { .. } => "constant",
            Expr::UserCall { .. } => "userCall",
            Expr::Param { .. } => "param",
            Expr::Call { .. } => "call",
            Expr::ReceiverCall { .. } => "receiverCall",
            Expr::MacroCall { .. } => "macroCall",
            Expr::MacroParam { .. } => "macroParam",
            Expr::Binary { .. } => "binary",
            Expr::Unary { .. } => "unary",
            Expr::Index { .. } => "index",
            Expr::Format { .. } => "format",
            Expr::Ternary { .. } => "ternary",
            Expr::Cast { .. } => "cast",
        }
    }
}

/// The typed binary operators of the v0.1 surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Power,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
}

impl BinaryOp {
    /// The protocol spelling of this operator.
    pub fn as_str(self) -> &'static str {
        match self {
            BinaryOp::Add => "+",
            BinaryOp::Subtract => "-",
            BinaryOp::Multiply => "*",
            BinaryOp::Divide => "/",
            BinaryOp::Modulo => "%",
            BinaryOp::Power => "**",
            BinaryOp::Equal => "==",
            BinaryOp::NotEqual => "!=",
            BinaryOp::Less => "<",
            BinaryOp::LessEqual => "<=",
            BinaryOp::Greater => ">",
            BinaryOp::GreaterEqual => ">=",
            BinaryOp::And => "and",
            BinaryOp::Or => "or",
        }
    }

    /// Parse a protocol operator spelling.
    pub fn parse(op: &str) -> Option<BinaryOp> {
        Some(match op {
            "+" => BinaryOp::Add,
            "-" => BinaryOp::Subtract,
            "*" => BinaryOp::Multiply,
            "/" => BinaryOp::Divide,
            "%" => BinaryOp::Modulo,
            "**" => BinaryOp::Power,
            "==" => BinaryOp::Equal,
            "!=" => BinaryOp::NotEqual,
            "<" => BinaryOp::Less,
            "<=" => BinaryOp::LessEqual,
            ">" => BinaryOp::Greater,
            ">=" => BinaryOp::GreaterEqual,
            "and" => BinaryOp::And,
            "or" => BinaryOp::Or,
            _ => return None,
        })
    }
}

/// The typed unary operators of the v0.1 surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,
    Not,
}

impl UnaryOp {
    /// The protocol spelling of this operator.
    pub fn as_str(self) -> &'static str {
        match self {
            UnaryOp::Negate => "-",
            UnaryOp::Not => "not",
        }
    }

    /// Parse a protocol operator spelling.
    pub fn parse(op: &str) -> Option<UnaryOp> {
        Some(match op {
            "-" => UnaryOp::Negate,
            "not" => UnaryOp::Not,
            _ => return None,
        })
    }
}
