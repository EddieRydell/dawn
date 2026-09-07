use super::preparation::prepare_sample_program;
use crate::native_effect::{self, BoundNativeEffect, NativeGeneratedEffect};
use dawn_language::dsl::{
    BoundParams, BytecodeProgram, DslBindCache, EffectKind as RootEffectKind, GeneratedEffect,
    GeneratorContext, Identifier, ParamDecl, Type, Value, VmWorkspace,
};
use dawn_language::effect::{
    EffectDefinitionId, EffectImplementation, EffectRef, builtin_effect_definition,
};
use dawn_language::identity::SourceIdentity;
use dawn_language::model::DawnProject;
use dawn_language::values::{SampleDuration, SampleTime};
use indexmap::IndexMap;
use std::sync::Arc;

use crate::MAX_GENERATED_EFFECTS;
use crate::sequence::targets::{
    PreparedTargetCache, PreparedTargetPixel, generator_context_target,
    prepared_pixels_from_generated_target_cached, sorted_sample_target,
};
use crate::{PreparedEffect, PreparedEffectImplementation, PreparedElement, RenderError};

const MAX_GENERATOR_DEPTH: usize = 4;
const MAX_GENERATED_CHILDREN: usize = MAX_GENERATED_EFFECTS;

#[derive(Clone, Debug)]
pub(crate) struct GeneratorExpansion {
    pub(crate) start_time: SampleTime,
    pub(crate) duration: SampleDuration,
    pub(crate) target: Arc<[PreparedTargetPixel]>,
    pub(crate) depth: usize,
    pub(crate) definition_source: SourceIdentity,
}

pub(crate) struct GeneratorPrepareContext<'a> {
    pub(crate) project: &'a DawnProject,
    pub(crate) elements: &'a [PreparedElement],
    pub(crate) effects: &'a mut Vec<PreparedEffect>,
    pub(crate) generated_child_count: &'a mut usize,
    pub(crate) bind_cache: &'a mut DslBindCache,
    pub(crate) sample_programs: &'a mut IndexMap<EffectDefinitionId, Arc<BytecodeProgram>>,
    pub(crate) target_cache: &'a mut PreparedTargetCache,
}

pub(crate) fn expand_generator(
    context: &mut GeneratorPrepareContext<'_>,
    definition: &dawn_language::dsl::CompiledEffect,
    params: &BoundParams,
    expansion: GeneratorExpansion,
) -> Result<(), RenderError> {
    if expansion.depth >= MAX_GENERATOR_DEPTH {
        return Err(RenderError::GeneratorPrepare {
            message: "generator depth limit exceeded".to_string(),
        });
    }
    let mut workspace = VmWorkspace::default();
    let target = generator_context_target(context.target_cache, &expansion.target);
    let generated = definition.generate_bound(
        params,
        &GeneratorContext {
            start_time: expansion.start_time,
            duration: expansion.duration,
            target,
        },
        &mut workspace,
    )?;
    for child in generated {
        if *context.generated_child_count >= MAX_GENERATED_CHILDREN {
            return Err(RenderError::GeneratorPrepare {
                message: format!("generated child limit exceeded ({MAX_GENERATED_CHILDREN})"),
            });
        }
        *context.generated_child_count += 1;
        prepare_generated_child(
            context,
            expansion.depth,
            &expansion.definition_source,
            child,
        )?;
    }
    Ok(())
}

pub(crate) fn expand_native_generator(
    context: &mut GeneratorPrepareContext<'_>,
    definition: &BoundNativeEffect,
    start_time: SampleTime,
    duration: SampleDuration,
    target: Arc<[PreparedTargetPixel]>,
    depth: usize,
) -> Result<(), RenderError> {
    if depth >= MAX_GENERATOR_DEPTH {
        return Err(RenderError::GeneratorPrepare {
            message: "generator depth limit exceeded".to_string(),
        });
    }
    let generated = definition.generate(&GeneratorContext {
        start_time,
        duration,
        target: generator_context_target(context.target_cache, &target),
    })?;
    for child in generated {
        if *context.generated_child_count >= MAX_GENERATED_CHILDREN {
            return Err(RenderError::GeneratorPrepare {
                message: format!("generated child limit exceeded ({MAX_GENERATED_CHILDREN})"),
            });
        }
        *context.generated_child_count += 1;
        prepare_native_child(context, child)?;
    }
    Ok(())
}

fn prepare_native_child(
    context: &mut GeneratorPrepareContext<'_>,
    child: NativeGeneratedEffect,
) -> Result<(), RenderError> {
    let target = prepared_pixels_from_generated_target_cached(
        context.target_cache,
        context.elements,
        child.target,
    )?;
    let target = sorted_sample_target(&target);
    context.effects.push(PreparedEffect {
        start_time: child.start_time,
        duration: child.duration,
        target: context.target_cache.sample_target(target)?,
        implementation: PreparedEffectImplementation::Native {
            sample: child.sample,
            params: None,
        },
        automation: None,
    });
    Ok(())
}

fn prepare_generated_child(
    context: &mut GeneratorPrepareContext<'_>,
    parent_depth: usize,
    definition_source: &SourceIdentity,
    child: GeneratedEffect,
) -> Result<(), RenderError> {
    let parent = context
        .project
        .definitions
        .effects
        .get(&EffectDefinitionId(definition_source.clone()))
        .ok_or_else(|| RenderError::GeneratorPrepare {
            message: format!("generator definition is not linked in {definition_source:?}"),
        })?;
    let slot = child.definition.0 as usize;
    let effect_ref = parent
        .generated_effect_targets
        .get(slot)
        .cloned()
        .ok_or_else(|| RenderError::GeneratorPrepare {
            message: format!("invalid generated effect slot {slot}"),
        })?;
    let definition = context
        .project
        .definitions
        .effects
        .resolve(&effect_ref)
        .ok_or_else(|| RenderError::GeneratorPrepare {
            message: match &effect_ref {
                EffectRef::Custom(effect_id) => format!(
                    "generated child effect `{}` does not exist in {}",
                    effect_id.0.object(),
                    effect_id.0.document()
                ),
                EffectRef::Builtin(builtin) => format!(
                    "generated built-in effect `{}` does not exist",
                    builtin_effect_definition(*builtin).source_name
                ),
            },
        })?;
    validate_generated_params(&definition.params, &child.params)?;
    let target = prepared_pixels_from_generated_target_cached(
        context.target_cache,
        context.elements,
        child.target,
    )?;
    let start_time = child.start_time;
    let duration = child.duration;
    match &definition.implementation {
        EffectImplementation::Dsl(definition_compiled) => {
            let EffectRef::Custom(effect_id) = &effect_ref else {
                unreachable!("DSL effects are custom")
            };
            let bound_params =
                definition_compiled.bind_params_pairs_cached(&child.params, context.bind_cache)?;
            match definition.kind {
                RootEffectKind::Sample => {
                    let program = prepare_sample_program(
                        context.sample_programs,
                        effect_id,
                        &definition_compiled.bytecode,
                    )?;
                    let target = sorted_sample_target(&target);
                    context.effects.push(PreparedEffect {
                        start_time,
                        duration,
                        target: context.target_cache.sample_target(target)?,
                        implementation: PreparedEffectImplementation::Dsl {
                            program,
                            bound_params,
                        },
                        automation: None,
                    });
                    Ok(())
                }
                RootEffectKind::Generator => expand_generator(
                    context,
                    definition_compiled,
                    &bound_params,
                    GeneratorExpansion {
                        start_time,
                        duration,
                        target,
                        depth: parent_depth + 1,
                        definition_source: effect_id.0.clone(),
                    },
                ),
            }
        }
        EffectImplementation::Native(builtin) => {
            let params = BoundParams::bind_pairs_cached(
                &definition.params,
                &child.params,
                context.bind_cache,
            )?;
            let bound = native_effect::bind_prepared(*builtin, params)?;
            match definition.kind {
                RootEffectKind::Sample => {
                    let BoundNativeEffect::Sample { sample, .. } = bound else {
                        return Err(RenderError::GeneratorPrepare {
                            message: "native sample effect bound as generator".to_string(),
                        });
                    };
                    let implementation = PreparedEffectImplementation::Native {
                        sample,
                        params: None,
                    };
                    let target = sorted_sample_target(&target);
                    context.effects.push(PreparedEffect {
                        start_time,
                        duration,
                        target: context.target_cache.sample_target(target)?,
                        implementation,
                        automation: None,
                    });
                    Ok(())
                }
                RootEffectKind::Generator => expand_native_generator(
                    context,
                    &bound,
                    start_time,
                    duration,
                    target,
                    parent_depth + 1,
                ),
            }
        }
    }
}

fn validate_generated_params(
    declarations: &[ParamDecl],
    params: &[(Identifier, Value)],
) -> Result<(), RenderError> {
    for (key, _) in params {
        if !declarations.iter().any(|param| param.name == *key) {
            return Err(RenderError::GeneratorPrepare {
                message: format!("unknown generated param `{}`", key.as_str()),
            });
        }
    }
    for param in declarations {
        let Some((_, value)) = params.iter().find(|(name, _)| *name == param.name) else {
            if param.default.is_none() {
                return Err(RenderError::GeneratorPrepare {
                    message: format!("missing generated param `{}`", param.name.as_str()),
                });
            }
            continue;
        };
        if !value_matches_type(value, &param.ty) {
            return Err(RenderError::GeneratorPrepare {
                message: format!("generated param `{}` has wrong type", param.name.as_str()),
            });
        }
    }
    Ok(())
}

fn value_matches_type(value: &Value, ty: &Type) -> bool {
    matches!(
        (value, ty),
        (Value::Void, Type::Void)
            | (Value::Int(_), Type::Int)
            | (Value::Float(_), Type::Float)
            | (Value::Int(_), Type::Float)
            | (Value::Bool(_), Type::Bool)
            | (Value::Color(_), Type::Color)
            | (Value::Marks(_), Type::Marks)
            | (Value::Curve(_), Type::Curve)
            | (Value::Gradient(_), Type::Gradient)
            | (Value::Target(_), Type::Target)
            | (Value::TargetItems(_), Type::TargetItems)
            | (Value::TargetItem(_), Type::TargetItem)
    ) || match (value, ty) {
        (Value::Array(items), Type::Array(item_type)) => {
            items.iter().all(|item| value_matches_type(item, item_type))
        }
        (Value::Enum(value), Type::Enum(options)) => options.iter().any(|option| option == value),
        _ => false,
    }
}
