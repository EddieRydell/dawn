use super::lexer::TextSpan;
use super::types::{Identifier, Type, Value};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Module {
    pub effects: Vec<EffectDecl>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EffectDecl {
    pub name: Identifier,
    pub params: Vec<ParamDecl>,
    pub sample: FunctionDecl,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParamDecl {
    pub name: Identifier,
    pub ty: Type,
    pub default: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FunctionDecl {
    pub return_type: Type,
    pub name: Identifier,
    pub params: Vec<FunctionParam>,
    pub body: Block,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FunctionParam {
    pub name: Identifier,
    pub ty: Type,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Block {
    pub statements: Vec<Stmt>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Stmt {
    Local {
        ty: Type,
        name: Identifier,
        initializer: Option<Expr>,
    },
    Assign {
        name: Identifier,
        value: Expr,
    },
    Expr(Expr),
    If {
        condition: Expr,
        then_block: Block,
        else_block: Option<Block>,
    },
    For {
        initializer: Box<Stmt>,
        condition: Expr,
        update: Box<Stmt>,
        body: Block,
    },
    Return(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Expr {
    pub kind: ExprKind,
    pub span: TextSpan,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ExprKind {
    Literal(Value),
    Variable(Identifier),
    Array(Vec<Expr>),
    Index {
        target: Box<Expr>,
        index: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UnaryOp {
    Negate,
    Not,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,
    And,
    Or,
}
