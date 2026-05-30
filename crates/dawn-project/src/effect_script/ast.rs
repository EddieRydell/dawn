use crate::model::Color;

use super::{EffectParamSchema, ScriptType};

#[derive(Debug, Clone, PartialEq)]
pub struct EffectAst {
    pub name: String,
    pub params: Vec<EffectParamSchema>,
    pub(super) sample: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Stmt {
    Let {
        name: String,
        value_type: ScriptType,
        expr: Expr,
    },
    Assign {
        name: String,
        expr: Expr,
    },
    Expr(Expr),
    For {
        name: String,
        value_type: ScriptType,
        initializer: Expr,
        condition: Expr,
        update: Box<Stmt>,
        body: Vec<Stmt>,
    },
    If {
        condition: Expr,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    Return(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Expr {
    Float(f64),
    Int(i64),
    Bool(bool),
    Color(Color),
    Ident(String),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UnaryOp {
    Negate,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,
    LogicalAnd,
    LogicalOr,
}
