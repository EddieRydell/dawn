use super::ast::Block;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BytecodeFunction {
    pub body: Block,
}
