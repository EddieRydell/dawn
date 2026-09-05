use crate::BuiltinOperator;
use crate::dsl::{
    BoundParams, OperatorRunContext, RunContext, RuntimeError, SignalSampler, VmWorkspace,
};
use crate::native_effect::{self, NativeSample};
use crate::sequence::{
    CachedSignal, EffectAutomationWorkspace, EvaluationError, EvaluationWorkspace,
    MAX_SIGNAL_CACHE_ENTRIES_PER_PIXEL, PreparedEffect, PreparedEffectImplementation,
    PreparedOperator, PreparedOperatorNode, PreparedPixel, PreparedSequence, PreparedSignalKind,
    SignalCacheKey,
};
use crate::values::{Color, SampleDuration, SampleTime, sample_duration_from_seconds_f32};
use alloc::format;
use alloc::string::ToString;
use alloc::vec::Vec;

pub(crate) fn sample_signal_graph(
    renderer: &PreparedSequence,
    sample_time: SampleTime,
    output: &mut [Color],
    workspace: &mut EvaluationWorkspace,
) -> Result<(), EvaluationError> {
    let graph = &renderer.signal_graph;
    // Recursive signal sampling uses the VM and caches, not these frame buffers.
    // Keep the fixed storage separate while evaluating, and restore it on errors too.
    let mut buffers = core::mem::take(&mut workspace.signal_buffers);
    let result = (|| {
        for node_index in graph.frame_nodes.iter().copied() {
            let node =
                graph
                    .nodes
                    .get(node_index)
                    .ok_or_else(|| EvaluationError::InvalidGraph {
                        message: "graph node index is out of bounds".to_string(),
                    })?;
            let destination = frame_range(renderer, node_index)?;
            match &node.kind {
                PreparedSignalKind::Layer { layer_index } => {
                    sample_layer_frame(
                        renderer,
                        *layer_index,
                        sample_time,
                        &mut buffers[destination],
                        workspace,
                    )?;
                }
                PreparedSignalKind::Operator {
                    operator,
                    inputs,
                    automation,
                    vm_slot,
                } => {
                    let mut automation_workspace = workspace.operator_automation[node_index].take();
                    if !automation.is_empty() {
                        let state =
                            automation_workspace.get_or_insert_with(|| operator.params.clone());
                        if let Err(error) = apply_bound_automation(state, automation, sample_time) {
                            workspace.operator_automation[node_index] = automation_workspace;
                            return Err(error);
                        }
                    }
                    let vm_slot = usize::from(*vm_slot);
                    let mut vm_workspace = core::mem::take(&mut workspace.operator_vm[vm_slot]);
                    let sampled = sample_operator_frame(
                        renderer,
                        operator,
                        inputs,
                        automation_workspace.as_ref(),
                        sample_time,
                        destination,
                        &mut buffers,
                        workspace,
                        &mut vm_workspace,
                    );
                    workspace.operator_vm[vm_slot] = vm_workspace;
                    workspace.operator_automation[node_index] = automation_workspace;
                    sampled?;
                }
                PreparedSignalKind::Output { inputs } => {
                    buffers[destination.clone()].fill(black());
                    for input in inputs {
                        let source = frame_range(renderer, *input)?;
                        for (target, source) in destination.clone().zip(source) {
                            let color = buffers[source];
                            compose_max(&mut buffers[target], color);
                        }
                    }
                }
            }
        }
        output.copy_from_slice(&buffers[frame_range(renderer, graph.output_index)?]);
        Ok(())
    })();
    workspace.signal_buffers = buffers;
    result
}

fn frame_range(
    renderer: &PreparedSequence,
    node_index: usize,
) -> Result<core::ops::Range<usize>, EvaluationError> {
    let graph = &renderer.signal_graph;
    let slot = graph
        .frame_slots
        .get(node_index)
        .copied()
        .unwrap_or(u16::MAX);
    if slot >= graph.frame_buffer_count {
        return Err(EvaluationError::InvalidGraph {
            message: "signal has no prepared frame buffer".to_string(),
        });
    }
    let start = usize::from(slot) * renderer.pixel_count;
    Ok(start..start + renderer.pixel_count)
}

fn sample_layer_frame(
    renderer: &PreparedSequence,
    layer_index: usize,
    sample_time: SampleTime,
    rendered: &mut [Color],
    workspace: &mut EvaluationWorkspace,
) -> Result<(), EvaluationError> {
    let Some(layer) = renderer.layers.get(layer_index) else {
        return Err(EvaluationError::InvalidGraph {
            message: "layer index is out of bounds".to_string(),
        });
    };
    rendered.fill(black());
    if !layer.enabled {
        return Ok(());
    }
    let Some(layer_effects) = renderer.effects_by_layer.get(layer_index) else {
        return Ok(());
    };
    for effect_index in layer_effects {
        let Some(effect) = renderer.effects.get(*effect_index) else {
            continue;
        };
        if effect.start_time > sample_time {
            break;
        }
        if !effect.is_active(sample_time) {
            continue;
        }
        let progress = effect.progress(sample_time);
        let local_time = effect.local_time(sample_time);
        let mut automation_workspace = workspace.effect_automation[*effect_index].take();
        let sample_count = renderer.targets[effect.target as usize].sample_count as usize;
        for sample in &mut workspace.effect_samples[..sample_count] {
            sample.pixel_count = 0;
        }
        let sampled = (|| {
            let (params, native_sample) = if effect.automation.is_none() {
                (None, None)
            } else {
                let state =
                    automation_workspace.get_or_insert_with(EffectAutomationWorkspace::default);
                match &effect.implementation {
                    PreparedEffectImplementation::Native { .. } => (
                        None,
                        Some(prepare_native_sample(effect, sample_time, state)?),
                    ),
                    PreparedEffectImplementation::Dsl { .. } => (
                        Some(prepare_effect_params(effect, sample_time, state)?),
                        None,
                    ),
                }
            };
            for pixel in renderer.target(effect.target) {
                // Pixel indices are already dense. The count distinguishes
                // fixtures of different sizes sharing the same index.
                let cached =
                    (sample_count != 0).then(|| &mut workspace.effect_samples[pixel.pixel_index()]);
                let color = match cached {
                    Some(sample) if sample.pixel_count == pixel.pixel_count => sample.color,
                    cached => {
                        let color = sample_effect_pixel(
                            &renderer.programs,
                            effect,
                            params,
                            native_sample,
                            pixel,
                            progress,
                            local_time,
                            &mut workspace.effect_vm,
                        )?;
                        if let Some(sample) = cached {
                            sample.pixel_count = pixel.pixel_count;
                            sample.color = color;
                        }
                        color
                    }
                };
                let flat_index = renderer.element_cell_offsets[pixel.element_index()]
                    + pixel.element_cell_index();
                compose_max(&mut rendered[flat_index], color);
            }
            Ok::<(), EvaluationError>(())
        })();
        workspace.effect_automation[*effect_index] = automation_workspace;
        sampled?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn sample_operator_frame(
    renderer: &PreparedSequence,
    operator: &PreparedOperatorNode,
    inputs: &[usize],
    automation: Option<&BoundParams>,
    sample_time: SampleTime,
    destination: core::ops::Range<usize>,
    buffers: &mut [Color],
    workspace: &mut EvaluationWorkspace,
    vm_workspace: &mut VmWorkspace,
) -> Result<(), EvaluationError> {
    let params = automation.unwrap_or(&operator.params);
    let PreparedOperator::Native(builtin) = &operator.implementation else {
        let PreparedOperator::Dsl(program) = &operator.implementation else {
            unreachable!("operator implementation is native or DSL")
        };
        let compiled = &renderer.programs[*program as usize];
        let duration = renderer.duration;
        let progress = if duration.ticks() == 0 {
            0.0
        } else {
            (sample_time.ticks() as f32 / duration.ticks() as f32).clamp(0.0, 1.0)
        };
        let output = &mut buffers[destination];
        let mut cache = core::mem::take(&mut workspace.signal_cache);
        for (flat_pixel_index, pixel) in renderer
            .target(renderer.signal_graph.target)
            .iter()
            .enumerate()
        {
            cache.clear();
            let context = OperatorRunContext {
                progress,
                time: SampleDuration::from_ticks(sample_time.ticks()),
                duration,
                pixel_index: pixel.pixel_index() as i32,
                pixel_count: pixel.pixel_count() as i32,
                pixel_fraction: pixel.pixel_fraction,
            };
            let mut sampler = GraphSignalSampler {
                renderer,
                inputs,
                cache: &mut cache,
                flat_pixel_index,
                duration: renderer.duration,
                workspace,
            };
            match compiled.sample_operator(params, &context, &mut sampler, vm_workspace) {
                Ok(color) => output[flat_pixel_index] = color,
                Err(error) => {
                    workspace.signal_cache = cache;
                    return Err(error.into());
                }
            }
        }
        workspace.signal_cache = cache;
        return Ok(());
    };

    match builtin {
        BuiltinOperator::Max => binary_graph_op(renderer, inputs, destination, buffers, max_color),
        BuiltinOperator::Add => binary_graph_op(renderer, inputs, destination, buffers, add_color),
        BuiltinOperator::Multiply => {
            binary_graph_op(renderer, inputs, destination, buffers, multiply_color)
        }
        BuiltinOperator::IntensityModulate => {
            binary_graph_op(renderer, inputs, destination, buffers, |source, mask| {
                scale_color(source, intensity(mask))
            })
        }
        BuiltinOperator::Dim => {
            let amount = params.float(0)?.clamp(0.0, 1.0);
            map_graph_op(renderer, inputs[0], destination, buffers, |color| {
                scale_color(color, amount)
            })
        }
        BuiltinOperator::Invert => {
            map_graph_op(renderer, inputs[0], destination, buffers, invert_color)
        }
        BuiltinOperator::Colorize => {
            let tint = params.color(0)?;
            map_graph_op(renderer, inputs[0], destination, buffers, |color| {
                scale_color(tint, intensity(color))
            })
        }
        BuiltinOperator::Delay => sample_delay_frame(
            renderer,
            inputs[0],
            params,
            sample_time,
            &mut buffers[destination],
            workspace,
        ),
        BuiltinOperator::Echo => sample_echo_frame(
            renderer,
            inputs[0],
            params,
            sample_time,
            &mut buffers[destination],
            workspace,
        ),
    }
}

fn binary_graph_op(
    renderer: &PreparedSequence,
    inputs: &[usize],
    destination: core::ops::Range<usize>,
    buffers: &mut [Color],
    op: impl Fn(Color, Color) -> Color,
) -> Result<(), EvaluationError> {
    let left = frame_range(renderer, inputs[0])?;
    let right = frame_range(renderer, inputs[1])?;
    for ((target, left), right) in destination.zip(left).zip(right) {
        buffers[target] = op(buffers[left], buffers[right]);
    }
    Ok(())
}

fn map_graph_op(
    renderer: &PreparedSequence,
    input: usize,
    destination: core::ops::Range<usize>,
    buffers: &mut [Color],
    op: impl Fn(Color) -> Color,
) -> Result<(), EvaluationError> {
    let input = frame_range(renderer, input)?;
    for (target, source) in destination.zip(input) {
        buffers[target] = op(buffers[source]);
    }
    Ok(())
}

fn sample_echo_frame(
    renderer: &PreparedSequence,
    input: usize,
    params: &BoundParams,
    sample_time: SampleTime,
    output: &mut [Color],
    workspace: &mut EvaluationWorkspace,
) -> Result<(), EvaluationError> {
    let delay = operator_delay(params, "echo")?;
    let repeats = params.int(1)?.clamp(1, 32);
    let decay = params.float(2)?.clamp(0.0, 1.0);
    output.fill(black());
    let mut cache = core::mem::take(&mut workspace.signal_cache);
    for (flat_pixel_index, output_pixel) in output.iter_mut().enumerate() {
        cache.clear();
        for repeat in 0..=repeats {
            let Some(delayed_time) = sample_time.checked_sub_duration(SampleDuration::from_ticks(
                delay.ticks().saturating_mul(repeat as u32),
            )) else {
                continue;
            };
            let sampled = sample_signal_pixel(
                renderer,
                input,
                delayed_time,
                flat_pixel_index,
                &mut cache,
                workspace,
            );
            match sampled {
                Ok(source) => compose_max(
                    output_pixel,
                    scale_color(source, powi_nonnegative(decay, repeat as u32)),
                ),
                Err(error) => {
                    workspace.signal_cache = cache;
                    return Err(error);
                }
            }
        }
    }
    workspace.signal_cache = cache;
    Ok(())
}

fn sample_delay_frame(
    renderer: &PreparedSequence,
    input: usize,
    params: &BoundParams,
    sample_time: SampleTime,
    output: &mut [Color],
    workspace: &mut EvaluationWorkspace,
) -> Result<(), EvaluationError> {
    output.fill(black());
    let Some(delayed_time) = sample_time.checked_sub_duration(operator_delay(params, "delay")?)
    else {
        return Ok(());
    };
    let mut cache = core::mem::take(&mut workspace.signal_cache);
    for (flat_pixel_index, output_pixel) in output.iter_mut().enumerate() {
        cache.clear();
        match sample_signal_pixel(
            renderer,
            input,
            delayed_time,
            flat_pixel_index,
            &mut cache,
            workspace,
        ) {
            Ok(color) => *output_pixel = color,
            Err(error) => {
                workspace.signal_cache = cache;
                return Err(error);
            }
        }
    }
    workspace.signal_cache = cache;
    Ok(())
}

fn operator_delay(params: &BoundParams, operator: &str) -> Result<SampleDuration, EvaluationError> {
    let seconds = params.float(0)?.max(0.0);
    sample_duration_from_seconds_f32(seconds).map_err(|_| EvaluationError::InvalidTiming {
        reason: format!("{operator} delay exceeds the runtime clock range"),
    })
}

fn sample_signal_pixel(
    renderer: &PreparedSequence,
    node_index: usize,
    sample_time: SampleTime,
    flat_pixel_index: usize,
    cache: &mut Vec<CachedSignal>,
    workspace: &mut EvaluationWorkspace,
) -> Result<Color, EvaluationError> {
    let key = SignalCacheKey {
        node_index,
        sample_time,
    };
    if let Some(cached) = cache.iter().find(|cached| cached.key == key) {
        return Ok(cached.color);
    }
    if cache.len() >= MAX_SIGNAL_CACHE_ENTRIES_PER_PIXEL {
        return Err(EvaluationError::InvalidGraph {
            message: format!(
                "signal sampling exceeded the per-pixel cache budget of {MAX_SIGNAL_CACHE_ENTRIES_PER_PIXEL} entries"
            ),
        });
    }
    let node = renderer.signal_graph.nodes.get(node_index).ok_or_else(|| {
        EvaluationError::InvalidGraph {
            message: "graph node index is out of bounds".to_string(),
        }
    })?;
    let color = match &node.kind {
        PreparedSignalKind::Layer { layer_index } => sample_layer_pixel(
            renderer,
            *layer_index,
            sample_time,
            flat_pixel_index,
            workspace,
        )?,
        PreparedSignalKind::Operator {
            operator,
            inputs,
            automation,
            vm_slot,
        } => {
            let mut automation_workspace = workspace.operator_automation[node_index].take();
            if !automation.is_empty() {
                let state = automation_workspace.get_or_insert_with(|| operator.params.clone());
                if let Err(error) = apply_bound_automation(state, automation, sample_time) {
                    workspace.operator_automation[node_index] = automation_workspace;
                    return Err(error);
                }
            }
            let vm_slot = usize::from(*vm_slot);
            let mut vm_workspace = core::mem::take(&mut workspace.operator_vm[vm_slot]);
            let sampled = sample_operator_pixel(
                renderer,
                operator,
                inputs,
                automation_workspace.as_ref(),
                sample_time,
                flat_pixel_index,
                cache,
                workspace,
                &mut vm_workspace,
            );
            workspace.operator_vm[vm_slot] = vm_workspace;
            workspace.operator_automation[node_index] = automation_workspace;
            sampled?
        }
        PreparedSignalKind::Output { inputs } => {
            let mut output = black();
            for input in inputs {
                compose_max(
                    &mut output,
                    sample_signal_pixel(
                        renderer,
                        *input,
                        sample_time,
                        flat_pixel_index,
                        cache,
                        workspace,
                    )?,
                );
            }
            output
        }
    };
    cache.push(CachedSignal { key, color });
    Ok(color)
}

fn sample_layer_pixel(
    renderer: &PreparedSequence,
    layer_index: usize,
    sample_time: SampleTime,
    flat_pixel_index: usize,
    workspace: &mut EvaluationWorkspace,
) -> Result<Color, EvaluationError> {
    let Some(layer) = renderer.layers.get(layer_index) else {
        return Err(EvaluationError::InvalidGraph {
            message: "layer index is out of bounds".to_string(),
        });
    };
    if !layer.enabled {
        return Ok(black());
    }
    let Some(pixel) = renderer
        .target(renderer.signal_graph.target)
        .get(flat_pixel_index)
    else {
        return Ok(black());
    };
    let mut rendered = black();
    let Some(layer_effects) = renderer.effects_by_layer.get(layer_index) else {
        return Ok(rendered);
    };
    for effect_index in layer_effects {
        let Some(effect) = renderer.effects.get(*effect_index) else {
            continue;
        };
        if effect.start_time > sample_time {
            break;
        }
        if !effect.is_active(sample_time) {
            continue;
        }
        let target = renderer.target(effect.target);
        if let Ok(target_index) = target.binary_search_by_key(
            &(pixel.element_index(), pixel.element_cell_index()),
            |effect_pixel| {
                (
                    effect_pixel.element_index(),
                    effect_pixel.element_cell_index(),
                )
            },
        ) {
            let effect_pixel = &target[target_index];
            let mut automation_workspace = workspace.effect_automation[*effect_index].take();
            let sampled = (|| {
                let (params, native_sample) = if effect.automation.is_none() {
                    (None, None)
                } else {
                    let state =
                        automation_workspace.get_or_insert_with(EffectAutomationWorkspace::default);
                    match &effect.implementation {
                        PreparedEffectImplementation::Native { .. } => (
                            None,
                            Some(prepare_native_sample(effect, sample_time, state)?),
                        ),
                        PreparedEffectImplementation::Dsl { .. } => (
                            Some(prepare_effect_params(effect, sample_time, state)?),
                            None,
                        ),
                    }
                };
                Ok::<_, EvaluationError>(sample_effect_pixel(
                    &renderer.programs,
                    effect,
                    params,
                    native_sample,
                    effect_pixel,
                    effect.progress(sample_time),
                    effect.local_time(sample_time),
                    &mut workspace.effect_vm,
                )?)
            })();
            workspace.effect_automation[*effect_index] = automation_workspace;
            let color = sampled?;
            compose_max(&mut rendered, color);
        }
    }
    Ok(rendered)
}

#[allow(clippy::too_many_arguments)]
fn sample_operator_pixel(
    renderer: &PreparedSequence,
    operator: &PreparedOperatorNode,
    inputs: &[usize],
    automation: Option<&BoundParams>,
    sample_time: SampleTime,
    flat_pixel_index: usize,
    cache: &mut Vec<CachedSignal>,
    workspace: &mut EvaluationWorkspace,
    vm_workspace: &mut VmWorkspace,
) -> Result<Color, EvaluationError> {
    let params = automation.unwrap_or(&operator.params);
    let PreparedOperator::Native(builtin) = &operator.implementation else {
        let PreparedOperator::Dsl(program) = &operator.implementation else {
            unreachable!("operator implementation is native or DSL")
        };
        let compiled = &renderer.programs[*program as usize];
        let pixel = &renderer.target(renderer.signal_graph.target)[flat_pixel_index];
        let duration = renderer.duration;
        let progress = if duration.ticks() == 0 {
            0.0
        } else {
            (sample_time.ticks() as f32 / duration.ticks() as f32).clamp(0.0, 1.0)
        };
        let context = OperatorRunContext {
            progress,
            time: SampleDuration::from_ticks(sample_time.ticks()),
            duration,
            pixel_index: pixel.pixel_index() as i32,
            pixel_count: pixel.pixel_count() as i32,
            pixel_fraction: pixel.pixel_fraction,
        };
        let mut sampler = GraphSignalSampler {
            renderer,
            inputs,
            cache,
            flat_pixel_index,
            duration: renderer.duration,
            workspace,
        };
        return Ok(compiled.sample_operator(params, &context, &mut sampler, vm_workspace)?);
    };
    let sample = |input: usize,
                  time: SampleTime,
                  cache: &mut Vec<CachedSignal>,
                  workspace: &mut EvaluationWorkspace| {
        sample_signal_pixel(
            renderer,
            inputs[input],
            time,
            flat_pixel_index,
            cache,
            workspace,
        )
    };
    Ok(match builtin {
        BuiltinOperator::Max => max_color(
            sample(0, sample_time, cache, workspace)?,
            sample(1, sample_time, cache, workspace)?,
        ),
        BuiltinOperator::Add => add_color(
            sample(0, sample_time, cache, workspace)?,
            sample(1, sample_time, cache, workspace)?,
        ),
        BuiltinOperator::Multiply => multiply_color(
            sample(0, sample_time, cache, workspace)?,
            sample(1, sample_time, cache, workspace)?,
        ),
        BuiltinOperator::IntensityModulate => {
            let source = sample(0, sample_time, cache, workspace)?;
            scale_color(source, intensity(sample(1, sample_time, cache, workspace)?))
        }
        BuiltinOperator::Dim => scale_color(
            sample(0, sample_time, cache, workspace)?,
            params.float(0)?.clamp(0.0, 1.0),
        ),
        BuiltinOperator::Invert => invert_color(sample(0, sample_time, cache, workspace)?),
        BuiltinOperator::Colorize => {
            let source = sample(0, sample_time, cache, workspace)?;
            scale_color(params.color(0)?, intensity(source))
        }
        BuiltinOperator::Delay => {
            let Some(delayed_time) =
                sample_time.checked_sub_duration(operator_delay(params, "delay")?)
            else {
                return Ok(black());
            };
            sample(0, delayed_time, cache, workspace)?
        }
        BuiltinOperator::Echo => {
            let delay = operator_delay(params, "echo")?;
            let repeats = params.int(1)?.clamp(1, 32);
            let decay = params.float(2)?.clamp(0.0, 1.0);
            let mut output = black();
            for repeat in 0..=repeats {
                let Some(delayed_time) = sample_time.checked_sub_duration(
                    SampleDuration::from_ticks(delay.ticks().saturating_mul(repeat as u32)),
                ) else {
                    continue;
                };
                compose_max(
                    &mut output,
                    scale_color(
                        sample(0, delayed_time, cache, workspace)?,
                        powi_nonnegative(decay, repeat as u32),
                    ),
                );
            }
            output
        }
    })
}

struct GraphSignalSampler<'a> {
    renderer: &'a PreparedSequence,
    inputs: &'a [usize],
    cache: &'a mut Vec<CachedSignal>,
    flat_pixel_index: usize,
    duration: SampleDuration,
    workspace: &'a mut EvaluationWorkspace,
}

impl SignalSampler for GraphSignalSampler<'_> {
    fn sample_signal(
        &mut self,
        input: usize,
        sample_time: SampleTime,
    ) -> Result<Color, RuntimeError> {
        if sample_time.ticks() >= self.duration.ticks() {
            return Ok(black());
        }
        let node = self
            .inputs
            .get(input)
            .copied()
            .ok_or_else(|| RuntimeError {
                message: "Signal input index is out of bounds".to_string(),
            })?;
        sample_signal_pixel(
            self.renderer,
            node,
            sample_time,
            self.flat_pixel_index,
            self.cache,
            self.workspace,
        )
        .map_err(|error| RuntimeError {
            message: format!("failed to sample Signal: {error:?}"),
        })
    }
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn sample_effect_pixel(
    programs: &[crate::dsl::bytecode::BytecodeProgram],
    effect: &PreparedEffect,
    params: Option<&BoundParams>,
    native_sample: Option<&NativeSample>,
    pixel: &PreparedPixel,
    progress: f32,
    local_time: SampleDuration,
    workspace: &mut VmWorkspace,
) -> Result<Color, RuntimeError> {
    let context = RunContext {
        progress,
        time: local_time,
        duration: effect.duration,
        pixel_index: pixel.pixel_index() as i32,
        pixel_count: pixel.pixel_count() as i32,
        pixel_fraction: pixel.pixel_fraction,
    };
    match &effect.implementation {
        PreparedEffectImplementation::Dsl {
            program,
            bound_params,
        } => programs[*program as usize].sample_effect(
            params.unwrap_or(bound_params),
            &context,
            workspace,
        ),
        PreparedEffectImplementation::Native { sample, .. } => {
            let sample_time = effect
                .start_time
                .checked_add_duration(local_time)
                .unwrap_or(effect.start_time);
            native_sample
                .unwrap_or(sample)
                .sample(&context, sample_time)
        }
    }
}

fn prepare_effect_params<'a>(
    effect: &PreparedEffect,
    sample_time: SampleTime,
    workspace: &'a mut EffectAutomationWorkspace,
) -> Result<&'a BoundParams, EvaluationError> {
    let automation = effect
        .automation
        .as_ref()
        .ok_or_else(|| EvaluationError::InvalidGraph {
            message: "automated effect is missing its prepared automation".to_string(),
        })?;
    if workspace.params.is_none() {
        workspace.params = Some(implementation_params(&effect.implementation)?.clone());
    }
    if workspace.sample_time != Some(sample_time) {
        let params = workspace
            .params
            .as_mut()
            .ok_or_else(|| EvaluationError::InvalidGraph {
                message: "automated effect parameters are missing".to_string(),
            })?;
        apply_bound_automation(params, &automation.bindings, sample_time)?;
        workspace.native_sample = None;
        workspace.sample_time = Some(sample_time);
    }
    workspace
        .params
        .as_ref()
        .ok_or_else(|| EvaluationError::InvalidGraph {
            message: "automated effect parameters are missing".to_string(),
        })
}

fn prepare_native_sample<'a>(
    effect: &PreparedEffect,
    sample_time: SampleTime,
    workspace: &'a mut EffectAutomationWorkspace,
) -> Result<&'a NativeSample, EvaluationError> {
    let builtin = match &effect.implementation {
        PreparedEffectImplementation::Native {
            params: Some((builtin, _)),
            ..
        } => *builtin,
        _ => {
            return Err(EvaluationError::InvalidGraph {
                message: "automated native effect has no bound parameters".to_string(),
            });
        }
    };
    prepare_effect_params(effect, sample_time, workspace)?;
    if workspace.native_sample.is_none() {
        let params = workspace
            .params
            .as_ref()
            .ok_or_else(|| EvaluationError::InvalidGraph {
                message: "automated effect parameters are missing".to_string(),
            })?;
        workspace.native_sample = Some(native_effect::prepare_sample(builtin, params)?);
    }
    workspace
        .native_sample
        .as_ref()
        .ok_or_else(|| EvaluationError::InvalidGraph {
            message: "automated native effect sample is missing".to_string(),
        })
}

fn apply_bound_automation(
    params: &mut BoundParams,
    automation: &[crate::sequence::PreparedAutomation],
    sample_time: SampleTime,
) -> Result<(), EvaluationError> {
    for binding in automation {
        params.apply_automation(
            usize::from(binding.param_index),
            &binding.curve,
            &binding.mapping,
            binding.position(sample_time),
        )?;
    }
    Ok(())
}

fn implementation_params(
    implementation: &PreparedEffectImplementation,
) -> Result<&BoundParams, EvaluationError> {
    match implementation {
        PreparedEffectImplementation::Dsl { bound_params, .. } => Ok(bound_params),
        PreparedEffectImplementation::Native {
            params: Some((_, params)),
            ..
        } => Ok(params),
        PreparedEffectImplementation::Native { params: None, .. } => {
            Err(EvaluationError::InvalidGraph {
                message: "automation requires a parameterized sample effect".to_string(),
            })
        }
    }
}

#[inline(always)]
fn compose_max(target: &mut Color, source: Color) {
    target.red = target.red.max(source.red);
    target.green = target.green.max(source.green);
    target.blue = target.blue.max(source.blue);
}

#[inline(always)]
fn max_color(left: Color, right: Color) -> Color {
    Color {
        red: left.red.max(right.red),
        green: left.green.max(right.green),
        blue: left.blue.max(right.blue),
    }
}

#[inline(always)]
fn add_color(left: Color, right: Color) -> Color {
    Color {
        red: left.red.saturating_add(right.red),
        green: left.green.saturating_add(right.green),
        blue: left.blue.saturating_add(right.blue),
    }
}

#[inline(always)]
fn multiply_color(left: Color, right: Color) -> Color {
    Color {
        red: ((u16::from(left.red) * u16::from(right.red)) / 255) as u8,
        green: ((u16::from(left.green) * u16::from(right.green)) / 255) as u8,
        blue: ((u16::from(left.blue) * u16::from(right.blue)) / 255) as u8,
    }
}

#[inline(always)]
fn invert_color(color: Color) -> Color {
    Color {
        red: 255 - color.red,
        green: 255 - color.green,
        blue: 255 - color.blue,
    }
}

#[inline(always)]
fn scale_color(color: Color, amount: f32) -> Color {
    let scale = |value: u8| (f32::from(value) * amount.clamp(0.0, 1.0) + 0.5) as u8;
    Color {
        red: scale(color.red),
        green: scale(color.green),
        blue: scale(color.blue),
    }
}

#[inline(always)]
fn intensity(color: Color) -> f32 {
    f32::from(color.red.max(color.green).max(color.blue)) / 255.0
}

#[inline]
fn powi_nonnegative(mut base: f32, mut exponent: u32) -> f32 {
    let mut result = 1.0;
    while exponent != 0 {
        if exponent & 1 != 0 {
            result *= base;
        }
        base *= base;
        exponent >>= 1;
    }
    result
}

const fn black() -> Color {
    Color {
        red: 0,
        green: 0,
        blue: 0,
    }
}
