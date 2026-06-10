use super::ast::Block;

#[derive(Clone, Debug)]
pub(crate) struct BytecodeFunction {
    pub body: Block,
}
