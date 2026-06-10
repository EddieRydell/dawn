use super::ast::{EffectDecl, Module, ParamDecl};
use super::bytecode::BytecodeFunction;
use super::CompiledEffect;

pub(crate) fn compile_checked_effects(module: Module) -> Vec<CompiledEffect> {
    module.effects.into_iter().map(compile_effect).collect()
}

fn compile_effect(effect: EffectDecl) -> CompiledEffect {
    CompiledEffect {
        name: effect.name,
        params: effect.params.into_iter().map(compile_param).collect(),
        sample: BytecodeFunction {
            body: effect.sample.body,
        },
    }
}

fn compile_param(param: ParamDecl) -> ParamDecl {
    param
}
