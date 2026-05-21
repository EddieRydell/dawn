//! AST node types for the VibeLights DSL.

/// Source span for error reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// A complete DSL script.
#[derive(Debug, Clone)]
pub struct Script {
    pub metadata: Vec<Metadata>,
    pub type_defs: Vec<TypeDef>,
    pub params: Vec<ParamDef>,
    pub functions: Vec<FnDef>,
    pub body: Vec<Stmt>,
}

/// `@name "Fire Flicker"` or `@spatial false`
#[derive(Debug, Clone)]
pub struct Metadata {
    pub key: String,
    pub value: MetaValue,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum MetaValue {
    Str(String),
    Bool(bool),
}

/// `enum Foo { A, B, C }` or `flags Bar { X, Y, Z }`
#[derive(Debug, Clone)]
pub struct TypeDef {
    pub kind: TypeDefKind,
    pub name: String,
    pub variants: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeDefKind {
    Enum,
    Flags,
}

/// `param speed: float(0.1, 10.0) = 2.0`
#[derive(Debug, Clone)]
pub struct ParamDef {
    pub name: String,
    pub ty: ParamType,
    /// Default value expression. `None` for param types that have no default (e.g. `Path`).
    pub default: Option<Expr>,
    pub span: Span,
}

/// The type annotation on a param declaration.
#[derive(Debug, Clone)]
pub enum ParamType {
    Float(Option<(f64, f64)>),
    Int(Option<(i32, i32)>),
    Bool,
    Color,
    Gradient,
    Curve,
    /// Motion path (callable or bare ident).
    Path,
    /// Named user type (enum or flags).
    Named(String),
}

/// `fn foo(x: float, y: float) -> float { ... }`
#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: String,
    pub params: Vec<FnParam>,
    pub return_type: TypeName,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FnParam {
    pub name: String,
    pub ty: TypeName,
}

/// Type names used in function signatures and type annotations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeName {
    Float,
    Int,
    Bool,
    Color,
    Vec2,
    Gradient,
    Curve,
}

/// Statements in the body.
#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        name: String,
        value: Expr,
        span: Span,
    },
    Expr(Expr),
}

/// Expressions.
#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    /// Float literal: `1.0`, `3.14`
    FloatLit(f64),
    /// Integer literal: `42`
    IntLit(i32),
    /// Boolean literal: `true`, `false`
    BoolLit(bool),
    /// Color literal: `#ff0000`
    ColorLit { r: u8, g: u8, b: u8 },
    /// Variable reference: `t`, `pixel`, `speed`
    Ident(String),
    /// Binary operation: `a + b`
    BinOp {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// Unary operation: `-x`, `!b`
    UnaryOp {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    /// Function call: `sin(x)`, `rgb(r, g, b)`
    Call {
        name: String,
        args: Vec<Expr>,
    },
    /// Method call: `color.scale(f)`, `gradient(t)`
    MethodCall {
        object: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },
    /// Field access: `pos2d.x`, `c.r`
    Field {
        object: Box<Expr>,
        field: String,
    },
    /// If expression: `if cond { a } else { b }`
    If {
        condition: Box<Expr>,
        then_body: Vec<Stmt>,
        else_body: Option<Vec<Stmt>>,
    },
    /// Switch expression: `switch expr { case Pat => body, default => body }`
    Switch {
        scrutinee: Box<Expr>,
        cases: Vec<(Expr, Vec<Stmt>)>,
        default: Option<Vec<Stmt>>,
    },
    /// Enum variant access: `ColorMode.Static`
    EnumAccess {
        enum_name: String,
        variant: String,
    },
    /// Flag combination: `Mirror | Wrap` (only in param defaults)
    FlagCombine(Vec<String>),
    /// Gradient literal in param defaults: `#000, #ff4400@0.4, #fff`
    GradientLit(Vec<GradientStop>),
    /// Curve literal in param defaults: `x1:y1, x2:y2`
    CurveLit(Vec<(f64, f64)>),
}

#[derive(Debug, Clone)]
pub struct GradientStop {
    pub color: (u8, u8, u8),
    pub position: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
    And,
    Or,
    BitOr,
    BitAnd,
    BitXor,
    Shl,
    Shr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}
