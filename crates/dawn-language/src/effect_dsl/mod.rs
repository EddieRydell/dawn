mod ast;
mod bytecode;
mod checked;
mod compiler;
mod diagnostic;
mod parser;
mod typecheck;
mod vm;

use bytecode::RegisterFunction;
use compiler::compile_checked_effects;
pub use diagnostic::Diagnostic;
use indexmap::IndexMap;
use parser::parse_module;
use typecheck::check_module;
pub use vm::{
    BoundEffectParams, EffectBindCache, EffectVmScratch, GeneratedEffect, GeneratorContext,
    RunContext, RuntimeError,
};

pub(crate) mod lexer;
pub mod types;

pub use crate::values::{Color, Curve, CurvePoint, CurveValue, Marks};
pub use ast::ParamDecl;
pub use types::{
    Identifier, TargetItemValue, TargetItemsValue, TargetPixelValue, TargetValue, Type, Value,
};

pub fn compile_effects(source: &str) -> Result<Vec<CompiledEffect>, Vec<Diagnostic>> {
    let module = parse_module(source)?;
    let module = check_module(module)?;
    Ok(compile_checked_effects(module))
}

#[derive(Clone, Debug)]
pub struct CompiledEffect {
    name: Identifier,
    params: Vec<ParamDecl>,
    kind: EffectKind,
    function: RegisterFunction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectKind {
    Sample,
    Generator,
}

impl PartialEq for CompiledEffect {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.params == other.params
            && self.kind == other.kind
            && self.function == other.function
    }
}

impl CompiledEffect {
    pub fn name(&self) -> &Identifier {
        &self.name
    }

    pub fn params(&self) -> &[ParamDecl] {
        &self.params
    }

    pub fn kind(&self) -> EffectKind {
        self.kind
    }

    pub fn sample(
        &self,
        params: &IndexMap<Identifier, Value>,
        context: &RunContext,
    ) -> Result<Color, RuntimeError> {
        let bound = self.bind_params(params);
        let mut scratch = EffectVmScratch::default();
        self.sample_bound(&bound, context, &mut scratch)
    }

    pub fn bind_params(&self, params: &IndexMap<Identifier, Value>) -> BoundEffectParams {
        vm::bind_effect_params(self, params)
    }

    pub fn bind_params_cached(
        &self,
        params: &IndexMap<Identifier, Value>,
        cache: &mut EffectBindCache,
    ) -> BoundEffectParams {
        vm::bind_effect_params_cached(self, params, cache)
    }

    pub fn sample_bound(
        &self,
        params: &BoundEffectParams,
        context: &RunContext,
        scratch: &mut EffectVmScratch,
    ) -> Result<Color, RuntimeError> {
        vm::run_sample_effect(self, params, context, scratch)
    }

    pub fn generate_bound(
        &self,
        params: &BoundEffectParams,
        context: &GeneratorContext,
        scratch: &mut EffectVmScratch,
    ) -> Result<Vec<GeneratedEffect>, RuntimeError> {
        vm::run_generator_effect(self, params, context, scratch)
    }
}
