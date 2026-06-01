use crate::model::Color;

use super::{EffectParamSchema, ScriptType};

#[derive(Debug, Clone, PartialEq)]
pub struct EffectAst {
    pub name: String,
    pub visibility: EffectVisibility,
    pub imports: Vec<EffectImport>,
    pub params: Vec<EffectParamSchema>,
    pub entrypoint: EffectEntrypoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectVisibility {
    Addable,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectImport {
    pub path: String,
    pub alias: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectModuleAst {
    pub imports: Vec<EffectImport>,
    pub effects: Vec<EffectAst>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EffectEntrypoint {
    Sample(Vec<Stmt>),
    Generator(Vec<Stmt>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
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
    Emit(EmitStmt),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
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
    Member {
        object: Box<Expr>,
        member: String,
    },
    Qualified {
        alias: String,
        name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
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

#[derive(Debug, Clone, PartialEq)]
pub struct EmitStmt {
    pub effect: EmitEffectRef,
    pub target: Expr,
    pub start: Expr,
    pub duration: Expr,
    pub params: Vec<EmitParam>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitEffectRef {
    Local { name: String },
    Imported { alias: String, name: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmitParam {
    pub name: String,
    pub expr: Expr,
}
