//! The frontend's concrete syntax tree (CST).
//!
//! Source-preserving syntax structure with a span on every node, produced by
//! [`crate::parser`]. This task ships syntax/project infrastructure only:
//! names, members, calls, and types remain unresolved syntax nodes until the
//! #118 semantic/type system exists. The tree covers the syntax forms the
//! committed protect-ban corpus exercises; other OSTW forms are rejected
//! explicitly by the parser rather than guessed.

use wright_ir::source::Span;

/// A parsed source file: imports and top-level items.
#[derive(Debug, Clone)]
pub struct File {
    pub imports: Vec<Import>,
    pub items: Vec<Item>,
    pub span: Span,
}

/// A quoted `import "path";` statement.
#[derive(Debug, Clone)]
pub struct Import {
    /// The quoted path exactly as written (forward-slash relative).
    pub path: String,
    pub span: Span,
}

/// One top-level item.
#[derive(Debug, Clone)]
pub enum Item {
    GlobalVar(VarDecl),
    PlayerVar(VarDecl),
    /// `define name: expr;` or `define name(params): expr;`.
    Define(DefineDecl),
    /// `Type name: expr;` or `Type name(params): expr;` (non-`define`).
    TypedDecl(TypedDecl),
    /// `[Type] name(params) [rule-name] { body }`.
    Function(FunctionDecl),
    Enum(EnumDecl),
    Rule(RuleDecl),
    Class(ClassDecl),
}

/// A `globalvar` / `playervar` declaration.
#[derive(Debug, Clone)]
pub struct VarDecl {
    /// The declared type, when present (`Number`, `Hero[]`, `Team | Number`,
    /// `define`, `Any`, ...).
    pub type_name: Option<TypeRef>,
    pub name: String,
    /// The explicit variable index form (`globalvar Number i 127;`).
    pub index: Option<Expr>,
    /// The initializer, when present (`= expr`).
    pub value: Option<Expr>,
    pub span: Span,
}

/// A `define` constant or define-macro.
#[derive(Debug, Clone)]
pub struct DefineDecl {
    pub name: String,
    /// The parameters, when the define is function-like.
    pub params: Vec<Param>,
    pub value: Expr,
    pub span: Span,
}

/// A typed non-`define` declaration (`Number x: 1;`,
/// `Number TeamIndex(Team team): expr;`).
#[derive(Debug, Clone)]
pub struct TypedDecl {
    pub type_name: TypeRef,
    pub name: String,
    /// Present when the declaration is function-like.
    pub params: Option<Vec<Param>>,
    pub value: Expr,
    pub span: Span,
}

/// A brace-bodied function declaration (`void f(params) { ... }`, with the
/// optional quoted rule-name between the parameter list and the body).
#[derive(Debug, Clone)]
pub struct FunctionDecl {
    pub return_type: Option<TypeRef>,
    pub name: String,
    pub params: Vec<Param>,
    /// The optional quoted rule name (`void f() "Rule name" { ... }`).
    pub rule_name: Option<String>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

/// An `enum Name { Member, ... }` declaration.
#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: String,
    pub members: Vec<String>,
    pub span: Span,
}

/// A `rule: "name" ... { body }` declaration with its modifiers.
#[derive(Debug, Clone)]
pub struct RuleDecl {
    pub disabled: bool,
    /// The quoted rule name, when present.
    pub name: Option<String>,
    /// The rule priority (`rule: "name" -1`), when present.
    pub priority: Option<Expr>,
    /// The event expression (`Event.OngoingPlayer`), when present.
    pub event: Option<Expr>,
    /// The `if (expr)` conditions preceding the body.
    pub conditions: Vec<Expr>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

/// A `class Name { ... }` declaration (syntax only; no class semantics).
#[derive(Debug, Clone)]
pub struct ClassDecl {
    pub name: String,
    pub members: Vec<ClassMember>,
    pub span: Span,
}

/// One member of a class body.
#[derive(Debug, Clone)]
pub enum ClassMember {
    /// `public Type name;`.
    Field {
        type_name: TypeRef,
        name: String,
        span: Span,
    },
    /// `public constructor(params) { body }`.
    Constructor {
        params: Vec<Param>,
        body: Vec<Stmt>,
        span: Span,
    },
    /// `public Type name(params): expr;` or `public Type name(params) { body }`.
    Method {
        type_name: TypeRef,
        name: String,
        params: Vec<Param>,
        value: Option<Expr>,
        body: Option<Vec<Stmt>>,
        span: Span,
    },
}

/// A type reference: a name with optional array depth and pipe unions.
#[derive(Debug, Clone)]
pub struct TypeRef {
    pub name: String,
    /// The number of trailing `[]` array markers.
    pub array_depth: u32,
    /// Additional union alternatives (`A | B`).
    pub unions: Vec<TypeRef>,
    pub span: Span,
}

/// A function/define parameter.
#[derive(Debug, Clone)]
pub struct Param {
    /// The parameter type, when declared (`Number`, `Button[]`, `define`).
    pub type_name: Option<TypeRef>,
    pub name: String,
    /// The default value (`= expr`), when present.
    pub default: Option<Expr>,
    pub span: Span,
}

/// A statement inside a rule/function/class body.
#[derive(Debug, Clone)]
pub enum Stmt {
    /// An expression statement (`f();`, `Wait(1);`).
    Expr {
        expr: Expr,
        span: Span,
    },
    /// An assignment (`x = e;`, `x += e;`, `x -= e;`).
    Assign {
        target: Expr,
        op: AssignOp,
        value: Expr,
        span: Span,
    },
    If {
        branches: Vec<IfBranch>,
        else_body: Option<Vec<Stmt>>,
        span: Span,
    },
    /// `for (init; condition; increment) { body }`.
    For {
        init: Option<Expr>,
        condition: Option<Expr>,
        increment: Option<Expr>,
        body: Vec<Stmt>,
        span: Span,
    },
    /// `foreach (Type var in iterable) { body }`.
    Foreach {
        var_type: Option<TypeRef>,
        var: String,
        iterable: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    Switch {
        value: Expr,
        cases: Vec<SwitchCase>,
        span: Span,
    },
    /// A local `define name = expr;` inside a body.
    LocalDefine {
        name: String,
        value: Expr,
        span: Span,
    },
    /// A local typed declaration inside a body (`Any x = expr;`).
    LocalDecl {
        type_name: TypeRef,
        name: String,
        value: Expr,
        span: Span,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },
    /// A bare block `{ ... }`.
    Block {
        body: Vec<Stmt>,
        span: Span,
    },
}

/// One `if`/`else if` branch.
#[derive(Debug, Clone)]
pub struct IfBranch {
    pub condition: Expr,
    pub body: Vec<Stmt>,
    pub span: Span,
}

/// One `case`/`default` arm of a `switch`.
#[derive(Debug, Clone)]
pub struct SwitchCase {
    /// The case value expression; `None` for `default`.
    pub value: Option<Expr>,
    /// The arm body; `None` for a fallthrough marker with no statements yet.
    pub body: Vec<Stmt>,
    pub span: Span,
}

/// An expression.
#[derive(Debug, Clone)]
pub enum Expr {
    Number {
        value: f64,
        text: String,
        span: Span,
    },
    /// A `"..."` string with its unescaped value.
    String {
        value: String,
        span: Span,
    },
    /// An `@"..."` verbatim string with its unescaped value.
    VerbatimString {
        value: String,
        span: Span,
    },
    Ident {
        name: String,
        span: Span,
    },
    Null {
        span: Span,
    },
    Bool {
        value: bool,
        span: Span,
    },
    /// `receiver.name` (any depth).
    Member {
        receiver: Box<Expr>,
        name: String,
        span: Span,
    },
    /// `callee(args)` with positional and named arguments.
    Call {
        callee: Box<Expr>,
        args: Vec<CallArg>,
        span: Span,
    },
    /// `array[index]`.
    Index {
        array: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    Array {
        elements: Vec<Expr>,
        span: Span,
    },
    /// `<"format", args...>` string interpolation.
    FormatString {
        format: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    /// `<Type>value` type cast (syntax only).
    Cast {
        type_name: TypeRef,
        value: Box<Expr>,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    Ternary {
        condition: Box<Expr>,
        then_value: Box<Expr>,
        else_value: Box<Expr>,
        span: Span,
    },
    /// `new Type(args)`.
    New {
        type_name: String,
        args: Vec<CallArg>,
        span: Span,
    },
    /// An assignment in expression position (for-loop initializers).
    Assign {
        target: Box<Expr>,
        op: AssignOp,
        value: Box<Expr>,
        span: Span,
    },
    /// `i++` / `i--`.
    Postfix {
        op: PostfixOp,
        operand: Box<Expr>,
        span: Span,
    },
}

/// One call argument: positional or named (`Name: value`).
#[derive(Debug, Clone)]
pub enum CallArg {
    Positional {
        value: Expr,
        span: Span,
    },
    Named {
        name: String,
        value: Expr,
        span: Span,
    },
}

/// The unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,
    Not,
}

/// The binary operators.
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

/// The assignment operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    AddAssign,
    SubtractAssign,
    MultiplyAssign,
    DivideAssign,
    ModuloAssign,
}

/// The postfix operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostfixOp {
    Increment,
    Decrement,
}
