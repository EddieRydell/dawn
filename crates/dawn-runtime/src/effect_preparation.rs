use super::*;

pub(crate) struct PrepareEffectContext<'a> {
    pub(crate) project: &'a DawnProject,
    pub(crate) sequence: &'a Sequence,
    pub(crate) layer_id: SequenceLayerId,
    pub(crate) elements: &'a [PreparedElement],
    pub(crate) element_ids: &'a IndexSet<ElementNodeId>,
    pub(crate) groups: &'a IndexMap<ElementNodeId, Vec<ElementNodeId>>,
    pub(crate) effects: &'a mut Vec<PreparedEffect>,
    pub(crate) generated_child_count: &'a mut usize,
    pub(crate) bind_cache: &'a mut DslBindCache,
    pub(crate) compiled_effects: &'a mut HashMap<EffectDefinitionId, Arc<CompiledEffect>>,
    pub(crate) target_cache: &'a mut PrepareTargetCache,
}

pub(crate) fn prepare_effect_inst(
    context: PrepareEffectContext<'_>,
    effect: &dawn_language::effect::EffectInst,
) -> Result<Arc<Vec<PreparedTargetPixel>>, RenderError> {
    let effect_duration_seconds = effect.duration.as_seconds_f64();
    if !effect_duration_seconds.is_finite() || effect_duration_seconds <= 0.0 {
        return Err(RenderError::InvalidTiming {
            reason: "effect duration must be positive and finite".to_string(),
        });
    }
    let definition = context
        .project
        .definitions
        .effects
        .resolve(&effect.definition)
        .ok_or_else(|| RenderError::MissingEffect {
            effect_id: effect.definition.clone(),
        })?;
    let target_selection = prepare_target(&effect.target, context.element_ids, context.groups)?;
    let target = prepare_target_pixels_cached(
        &mut context.target_cache.target,
        &target_selection,
        context.elements,
        &effect.scope,
    )?;
    let start_seconds = effect.start.as_seconds_f64();
    let param_timing = EffectParamTiming {
        start_seconds,
        duration_seconds: effect_duration_seconds,
    };
    let automation = automation_for_effect(context.sequence, &effect.id);
    let params = prepare_params(
        context.project,
        context.sequence,
        &effect.param_overrides,
        param_timing,
    )?;
    match definition.kind {
        EffectKind::Sample => {
            let implementation = match &definition.implementation {
                EffectImplementation::Dsl(compiled) => {
                    let EffectRef::Custom(id) = &effect.definition else {
                        unreachable!("DSL effects are custom")
                    };
                    let compiled = context
                        .compiled_effects
                        .entry(id.clone())
                        .or_insert_with(|| Arc::new(compiled.clone()))
                        .clone();
                    PreparedEffectImplementation::Dsl {
                        bound_params: compiled.bind_params_cached(&params, context.bind_cache)?,
                        definition: compiled,
                    }
                }
                EffectImplementation::Native(builtin) => {
                    match native_effect::bind(*builtin, &params)? {
                        BoundNativeEffect::Sample(sample) => PreparedEffectImplementation::Native {
                            builtin: Some(*builtin),
                            sample,
                        },
                        _ => {
                            return Err(RenderError::GeneratorPrepare {
                                message: "native sample effect bound as generator".to_string(),
                            });
                        }
                    }
                }
            };
            context.effects.push(PreparedEffect {
                layer_id: context.layer_id.clone(),
                start_seconds,
                duration_seconds: effect_duration_seconds,
                target: Arc::clone(&target),
                sample_groups: prepare_sample_groups_for_implementation(
                    context.target_cache,
                    &implementation,
                    &target,
                ),
                name: definition.display_name.clone(),
                implementation,
                params,
                automation,
            });
        }
        EffectKind::Generator => {
            let params = apply_automation_params(params, &automation, start_seconds)?;
            let mut generator_context = GeneratorPrepareContext {
                project: context.project,
                layer_id: context.layer_id.clone(),
                elements: context.elements,
                effects: context.effects,
                generated_child_count: context.generated_child_count,
                bind_cache: context.bind_cache,
                compiled_effects: context.compiled_effects,
                target_cache: context.target_cache,
            };
            for expansion_target in generator_expansion_targets(&target, &effect.scope) {
                match &definition.implementation {
                    EffectImplementation::Dsl(compiled) => {
                        let EffectRef::Custom(id) = &effect.definition else {
                            unreachable!("DSL effects are custom")
                        };
                        let compiled = generator_context
                            .compiled_effects
                            .entry(id.clone())
                            .or_insert_with(|| Arc::new(compiled.clone()))
                            .clone();
                        let bound =
                            compiled.bind_params_cached(&params, generator_context.bind_cache)?;
                        expand_generator(
                            &mut generator_context,
                            &compiled,
                            &bound,
                            GeneratorExpansion {
                                start_seconds,
                                duration_seconds: effect_duration_seconds,
                                target: expansion_target,
                                depth: 0,
                                definition_source: id.0.clone(),
                            },
                        )?;
                    }
                    EffectImplementation::Native(builtin) => {
                        let bound = native_effect::bind(*builtin, &params)?;
                        expand_native_generator(
                            &mut generator_context,
                            &bound,
                            start_seconds,
                            effect_duration_seconds,
                            expansion_target,
                            0,
                        )?;
                    }
                }
            }
        }
    }
    Ok(target)
}

pub(crate) fn automation_for_effect(
    sequence: &Sequence,
    target_effect_id: &EffectInstId,
) -> Vec<PreparedAutomation> {
    sequence
        .automation_clips
        .iter()
        .flat_map(|clip| {
            clip.bindings
                .iter()
                .filter(move |binding| {
                    matches!(
                        &binding.target,
                        AutomationTarget::EffectParam { effect_id, .. }
                            if effect_id == target_effect_id
                    )
                })
                .map(move |binding| prepare_automation(clip, binding))
        })
        .collect()
}

pub(crate) fn prepare_automation(
    clip: &AutomationClip,
    binding: &AutomationBinding,
) -> PreparedAutomation {
    let mut clip = clip.clone();
    clip.curve
        .points
        .sort_by(|left, right| left.position.total_cmp(&right.position));
    PreparedAutomation {
        clip,
        binding: binding.clone(),
    }
}

pub(crate) fn apply_automation_params(
    mut params: IndexMap<Identifier, Value>,
    automation: &[PreparedAutomation],
    sample_seconds: f64,
) -> Result<IndexMap<Identifier, Value>, RenderError> {
    for automation in automation {
        let value = automation_value_at(&automation.clip, &automation.binding, sample_seconds)
            .map(|value| match value {
                AutomationValue::Int(value) => Value::Int(value),
                AutomationValue::Float(value) => Value::Float(value),
                AutomationValue::Bool(value) => Value::Bool(value),
                AutomationValue::Enum(value) => Value::Enum(value),
                AutomationValue::Curve(value) => Value::Curve(Arc::new(value)),
            })
            .ok_or_else(|| RenderError::BadGraph {
                message: "enum automation mapping has no values".to_string(),
            })?;
        params.insert(automation_param(&automation.binding).clone(), value);
    }
    Ok(params)
}

pub(crate) fn automation_param(binding: &AutomationBinding) -> &Identifier {
    match &binding.target {
        AutomationTarget::EffectParam { param, .. }
        | AutomationTarget::CompositionNodeParam { param, .. } => param,
    }
}
