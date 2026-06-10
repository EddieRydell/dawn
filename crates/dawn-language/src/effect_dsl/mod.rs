mod ast;
mod bytecode;
mod compiler;
mod diagnostic;
mod parser;
mod typecheck;
mod vm;

use bytecode::BytecodeFunction;
use compiler::compile_checked_effects;
pub use diagnostic::Diagnostic;
use indexmap::IndexMap;
use parser::parse_module;
use typecheck::check_module;
pub use vm::{RunContext, RuntimeError};

pub(crate) mod lexer;
pub mod types;

pub use ast::ParamDecl;
pub use types::{Color, Curve, CurvePoint, CurveValue, Identifier, Marks, Type, Value};

pub fn compile_effects(source: &str) -> Result<Vec<CompiledEffect>, Vec<Diagnostic>> {
    let module = parse_module(source)?;
    check_module(&module)?;
    Ok(compile_checked_effects(module))
}

#[derive(Clone, Debug)]
pub struct CompiledEffect {
    name: Identifier,
    params: Vec<ParamDecl>,
    sample: BytecodeFunction,
}

impl CompiledEffect {
    pub fn name(&self) -> &Identifier {
        &self.name
    }

    pub fn params(&self) -> &[ParamDecl] {
        &self.params
    }

    pub fn sample(
        &self,
        params: &IndexMap<Identifier, Value>,
        context: &RunContext,
    ) -> Result<Color, RuntimeError> {
        vm::run_effect(self, params, context)
    }
}
