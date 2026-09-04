use crate::RenderError;
use crate::sequence::color::{
    add_color, black, compose_max, intensity, invert_color, max_color, multiply_color, scale_color,
};
use crate::sequence::composition::graph::{
    PreparedOperator, PreparedOperatorNode, PreparedSignalKind,
};
use crate::sequence::effects::sampling::{
    apply_bound_automation, prepare_effect_params, prepare_native_sample, sample_effect_pixel,
};
use crate::sequence::renderer::{
    CachedEffectSample, CachedSignal, EffectAutomationScratch, MAX_SIGNAL_CACHE_ENTRIES_PER_PIXEL,
    PreparedSequenceRenderer, SequenceRenderScratch, SignalCacheKey,
};
use dawn_language::dsl::{
    BoundParams, DslVmScratch, OperatorRunContext, RuntimeError, SignalSampler,
};
use dawn_language::operator::BuiltinOperator;
use dawn_language::values::{
    Color, SampleDuration, SampleTime, SampleTimeError, sample_duration_from_seconds_f32,
    sample_time_from_seconds_f32,
};

pub(crate) fn sample_signal_graph(
    renderer: &PreparedSequenceRenderer,
    sample_time: SampleTime,
    output: &mut [Color],
    scratch: &mut SequenceRenderScratch,
) -> Result<(), RenderError> {
    let mut frames = std::mem::take(&mut scratch.signal_frames);
    for mut colors in frames.drain(..).flatten() {
        colors.clear();
        scratch.color_buffers.push(colors);
    }
    frames.resize_with(renderer.signal_graph.nodes.len(), || None);
    scratch.signal_consumers.clear();
    scratch
        .signal_consumers
        .extend_from_slice(&renderer.signal_graph.frame_consumers);
    let sampled = sample_signal_frame(
        renderer,
        renderer.signal_graph.output_index,
        sample_time,
        &mut frames,
        scratch,
    );
    match sampled {
        Ok(output_index) => {
            let colors = frames[output_index]
                .as_ref()
                .ok_or_else(|| RenderError::BadGraph {
                    message: "sampled output signal is missing".to_string(),
                })?;
            output.copy_from_slice(colors);
            scratch.signal_frames = frames;
            Ok(())
        }
        Err(error) => {
            scratch.signal_frames = frames;
            Err(error)
        }
    }
}

fn sample_signal_frame(
    renderer: &PreparedSequenceRenderer,
    node_index: usize,
    sample_time: SampleTime,
    frames: &mut [Option<Vec<Color>>],
    scratch: &mut SequenceRenderScratch,
) -> Result<usize, RenderError> {
    if frames.get(node_index).is_some_and(Option::is_some) {
        return Ok(node_index);
    }
    let graph = &renderer.signal_graph;
    let node = graph
        .nodes
        .get(node_index)
        .ok_or_else(|| RenderError::BadGraph {
            message: "graph node index is out of bounds".to_string(),
        })?;
    let colors = match &node.kind {
        PreparedSignalKind::Layer { layer_index } => {
            sample_layer_frame(renderer, *layer_index, sample_time, scratch)?
        }
        PreparedSignalKind::Operator {
            operator,
            inputs,
            automation,
            vm_slot,
        } => {
            let mut automation_scratch = scratch.operator_automation[node_index].take();
            if !automation.is_empty() {
                let state = automation_scratch.get_or_insert_with(|| operator.params.clone());
                if let Err(error) = apply_bound_automation(state, automation, sample_time) {
                    scratch.operator_automation[node_index] = automation_scratch;
                    return Err(error);
                }
            }
            let vm_slot = usize::from(*vm_slot);
            let mut vm_scratch = std::mem::take(&mut scratch.operator_vm[vm_slot]);
            let sampled = sample_operator_frame(
                renderer,
                operator,
                inputs,
                automation_scratch.as_ref(),
                sample_time,
                frames,
                scratch,
                &mut vm_scratch,
            );
            scratch.operator_vm[vm_slot] = vm_scratch;
            scratch.operator_automation[node_index] = automation_scratch;
            sampled?
        }
        PreparedSignalKind::Output { inputs } => {
            let mut output = take_black_color_buffer(scratch, graph.target.len());
            for input in inputs {
                let input = sample_signal_frame(renderer, *input, sample_time, frames, scratch)?;
                let source = frames[input]
                    .as_ref()
                    .ok_or_else(|| RenderError::BadGraph {
                        message: "sampled input signal is missing".to_string(),
                    })?;
                for (target, source) in output.iter_mut().zip(source.iter().copied()) {
                    compose_max(target, source);
                }
            }
            output
        }
    };
    let inputs = match &node.kind {
        PreparedSignalKind::Layer { .. } => &[][..],
        PreparedSignalKind::Operator { inputs, .. } | PreparedSignalKind::Output { inputs } => {
            inputs
        }
    };
    for input in inputs {
        let remaining = &mut scratch.signal_consumers[*input];
        *remaining = remaining.saturating_sub(1);
        if *remaining == 0
            && let Some(mut colors) = frames[*input].take()
        {
            colors.clear();
            scratch.color_buffers.push(colors);
        }
    }
    frames[node_index] = Some(colors);
    Ok(node_index)
}

fn sample_layer_frame(
    renderer: &PreparedSequenceRenderer,
    layer_index: usize,
    sample_time: SampleTime,
    scratch: &mut SequenceRenderScratch,
) -> Result<Vec<Color>, RenderError> {
    let Some(layer) = renderer.layers.get(layer_index) else {
        return Err(RenderError::BadGraph {
            message: "layer index is out of bounds".to_string(),
        });
    };
    let mut rendered = take_black_color_buffer(scratch, renderer.pixel_count);
    if !layer.enabled {
        return Ok(rendered);
    }
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
        let progress = effect.progress(sample_time);
        let local_time = effect.local_time(sample_time);
        let mut automation_scratch = scratch.effect_automation[*effect_index].take();
        let mut samples = std::mem::take(&mut scratch.effect_samples);
        samples.clear();
        let sampled = (|| {
            let (params, native_sample) = if effect.automation.is_none() {
                (None, None)
            } else {
                let state = automation_scratch.get_or_insert_with(EffectAutomationScratch::default);
                match &effect.implementation {
                    crate::PreparedEffectImplementation::Native { .. } => (
                        None,
                        Some(prepare_native_sample(effect, sample_time, state)?),
                    ),
                    crate::PreparedEffectImplementation::Dsl { .. } => (
                        Some(prepare_effect_params(effect, sample_time, state)?),
                        None,
                    ),
                }
            };
            for pixel in effect.target.iter() {
                let cached = effect.reuse_samples.then(|| {
                    samples.iter().find(|sample| {
                        sample.pixel_index == pixel.pixel_index()
                            && sample.pixel_count == pixel.pixel_count()
                    })
                });
                let color = match cached.flatten() {
                    Some(sample) => sample.color,
                    None => {
                        let color = sample_effect_pixel(
                            effect,
                            params,
                            native_sample,
                            pixel,
                            progress,
                            local_time,
                            &mut scratch.effect_vm,
                        )?;
                        if effect.reuse_samples {
                            samples.push(CachedEffectSample {
                                pixel_index: pixel.pixel_index(),
                                pixel_count: pixel.pixel_count(),
                                color,
                            });
                        }
                        color
                    }
                };
                let flat_index = renderer.element_cell_offsets[pixel.element_index()]
                    + pixel.element_cell_index();
                compose_max(&mut rendered[flat_index], color);
            }
            Ok::<(), RenderError>(())
        })();
        scratch.effect_samples = samples;
        scratch.effect_automation[*effect_index] = automation_scratch;
        sampled?;
    }
    Ok(rendered)
}

#[allow(clippy::too_many_arguments)]
fn sample_operator_frame(
    renderer: &PreparedSequenceRenderer,
    operator: &PreparedOperatorNode,
    inputs: &[usize],
    automation: Option<&BoundParams>,
    sample_time: SampleTime,
    frames: &mut [Option<Vec<Color>>],
    scratch: &mut SequenceRenderScratch,
    vm_scratch: &mut DslVmScratch,
) -> Result<Vec<Color>, RenderError> {
    let params = automation.unwrap_or(&operator.params);
    let PreparedOperator::Native(builtin) = &operator.implementation else {
        let PreparedOperator::Dsl(compiled) = &operator.implementation else {
            unreachable!("operator implementation is native or DSL")
        };
        let duration = renderer.duration;
        let progress = if duration.ticks() == 0 {
            0.0
        } else {
            (sample_time.ticks() as f32 / duration.ticks() as f32).clamp(0.0, 1.0)
        };
        let mut output = take_empty_color_buffer(scratch, renderer.signal_graph.target.len());
        let mut cache = std::mem::take(&mut scratch.signal_cache);
        for (flat_pixel_index, pixel) in renderer.signal_graph.target.iter().enumerate() {
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
                scratch,
            };
            match compiled.sample_operator(params, &context, &mut sampler, vm_scratch) {
                Ok(color) => output.push(color),
                Err(error) => {
                    scratch.signal_cache = cache;
                    return Err(error.into());
                }
            }
        }
        scratch.signal_cache = cache;
        return Ok(output);
    };

    match builtin {
        BuiltinOperator::Max => {
            binary_graph_op(renderer, inputs, sample_time, frames, scratch, max_color)
        }
        BuiltinOperator::Add => {
            binary_graph_op(renderer, inputs, sample_time, frames, scratch, add_color)
        }
        BuiltinOperator::Multiply => binary_graph_op(
            renderer,
            inputs,
            sample_time,
            frames,
            scratch,
            multiply_color,
        ),
        BuiltinOperator::IntensityModulate => {
            let source = sample_signal_frame(renderer, inputs[0], sample_time, frames, scratch)?;
            let mask = sample_signal_frame(renderer, inputs[1], sample_time, frames, scratch)?;
            let source = frames[source].as_ref().ok_or_else(missing_signal)?;
            let mask = frames[mask].as_ref().ok_or_else(missing_signal)?;
            let mut output = take_empty_color_buffer(scratch, source.len().min(mask.len()));
            output.extend(
                source
                    .iter()
                    .copied()
                    .zip(mask.iter().copied())
                    .map(|(source, mask)| scale_color(source, intensity(mask))),
            );
            Ok(output)
        }
        BuiltinOperator::Dim => {
            let amount = params.float(0)?.clamp(0.0, 1.0);
            map_graph_op(renderer, inputs[0], sample_time, frames, scratch, |color| {
                scale_color(color, amount)
            })
        }
        BuiltinOperator::Invert => map_graph_op(
            renderer,
            inputs[0],
            sample_time,
            frames,
            scratch,
            invert_color,
        ),
        BuiltinOperator::Colorize => {
            let tint = params.color(0)?;
            map_graph_op(renderer, inputs[0], sample_time, frames, scratch, |color| {
                scale_color(tint, intensity(color))
            })
        }
        BuiltinOperator::Delay => {
            sample_delay_frame(renderer, inputs[0], params, sample_time, scratch)
        }
        BuiltinOperator::Echo => {
            sample_echo_frame(renderer, inputs[0], params, sample_time, scratch)
        }
    }
}

fn missing_signal() -> RenderError {
    RenderError::BadGraph {
        message: "sampled input signal is missing".to_string(),
    }
}

fn take_black_color_buffer(scratch: &mut SequenceRenderScratch, len: usize) -> Vec<Color> {
    let mut colors = scratch.color_buffers.pop().unwrap_or_default();
    colors.clear();
    colors.resize(len, black());
    colors
}

fn take_empty_color_buffer(scratch: &mut SequenceRenderScratch, capacity: usize) -> Vec<Color> {
    let mut colors = scratch.color_buffers.pop().unwrap_or_default();
    colors.clear();
    if colors.capacity() < capacity {
        colors.reserve(capacity - colors.capacity());
    }
    colors
}

fn binary_graph_op(
    renderer: &PreparedSequenceRenderer,
    inputs: &[usize],
    sample_time: SampleTime,
    frames: &mut [Option<Vec<Color>>],
    scratch: &mut SequenceRenderScratch,
    op: fn(Color, Color) -> Color,
) -> Result<Vec<Color>, RenderError> {
    let left = sample_signal_frame(renderer, inputs[0], sample_time, frames, scratch)?;
    let right = sample_signal_frame(renderer, inputs[1], sample_time, frames, scratch)?;
    let left = frames[left].as_ref().ok_or_else(missing_signal)?;
    let right = frames[right].as_ref().ok_or_else(missing_signal)?;
    let mut output = take_empty_color_buffer(scratch, left.len().min(right.len()));
    output.extend(
        left.iter()
            .copied()
            .zip(right.iter().copied())
            .map(|(left, right)| op(left, right)),
    );
    Ok(output)
}

fn map_graph_op(
    renderer: &PreparedSequenceRenderer,
    input: usize,
    sample_time: SampleTime,
    frames: &mut [Option<Vec<Color>>],
    scratch: &mut SequenceRenderScratch,
    op: impl Fn(Color) -> Color,
) -> Result<Vec<Color>, RenderError> {
    let input = sample_signal_frame(renderer, input, sample_time, frames, scratch)?;
    let input = frames[input].as_ref().ok_or_else(missing_signal)?;
    let mut output = take_empty_color_buffer(scratch, input.len());
    output.extend(input.iter().copied().map(op));
    Ok(output)
}

fn sample_echo_frame(
    renderer: &PreparedSequenceRenderer,
    input: usize,
    params: &BoundParams,
    sample_time: SampleTime,
    scratch: &mut SequenceRenderScratch,
) -> Result<Vec<Color>, RenderError> {
    let delay = operator_delay(params, "echo")?;
    let repeats = params.int(1)?.clamp(1, 32);
    let decay = params.float(2)?.clamp(0.0, 1.0);
    let mut output = take_black_color_buffer(scratch, renderer.signal_graph.target.len());
    let mut cache = std::mem::take(&mut scratch.signal_cache);
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
                scratch,
            );
            match sampled {
                Ok(source) => compose_max(output_pixel, scale_color(source, decay.powi(repeat))),
                Err(error) => {
                    scratch.signal_cache = cache;
                    return Err(error);
                }
            }
        }
    }
    scratch.signal_cache = cache;
    Ok(output)
}

fn sample_delay_frame(
    renderer: &PreparedSequenceRenderer,
    input: usize,
    params: &BoundParams,
    sample_time: SampleTime,
    scratch: &mut SequenceRenderScratch,
) -> Result<Vec<Color>, RenderError> {
    let mut output = take_black_color_buffer(scratch, renderer.signal_graph.target.len());
    let Some(delayed_time) = sample_time.checked_sub_duration(operator_delay(params, "delay")?)
    else {
        return Ok(output);
    };
    let mut cache = std::mem::take(&mut scratch.signal_cache);
    for (flat_pixel_index, output_pixel) in output.iter_mut().enumerate() {
        cache.clear();
        match sample_signal_pixel(
            renderer,
            input,
            delayed_time,
            flat_pixel_index,
            &mut cache,
            scratch,
        ) {
            Ok(color) => *output_pixel = color,
            Err(error) => {
                scratch.signal_cache = cache;
                return Err(error);
            }
        }
    }
    scratch.signal_cache = cache;
    Ok(output)
}

fn operator_delay(params: &BoundParams, operator: &str) -> Result<SampleDuration, RenderError> {
    let seconds = params.float(0)?.max(0.0);
    sample_duration_from_seconds_f32(seconds).map_err(|_| RenderError::InvalidTiming {
        reason: format!("{operator} delay exceeds the runtime clock range"),
    })
}

fn sample_signal_pixel(
    renderer: &PreparedSequenceRenderer,
    node_index: usize,
    sample_time: SampleTime,
    flat_pixel_index: usize,
    cache: &mut Vec<CachedSignal>,
    scratch: &mut SequenceRenderScratch,
) -> Result<Color, RenderError> {
    let key = SignalCacheKey {
        node_index,
        sample_time,
    };
    if let Some(cached) = cache.iter().find(|cached| cached.key == key) {
        return Ok(cached.color);
    }
    if cache.len() >= MAX_SIGNAL_CACHE_ENTRIES_PER_PIXEL {
        return Err(RenderError::BadGraph {
            message: format!(
                "signal sampling exceeded the per-pixel cache budget of {MAX_SIGNAL_CACHE_ENTRIES_PER_PIXEL} entries"
            ),
        });
    }
    let node =
        renderer
            .signal_graph
            .nodes
            .get(node_index)
            .ok_or_else(|| RenderError::BadGraph {
                message: "graph node index is out of bounds".to_string(),
            })?;
    let color = match &node.kind {
        PreparedSignalKind::Layer { layer_index } => sample_layer_pixel(
            renderer,
            *layer_index,
            sample_time,
            flat_pixel_index,
            scratch,
        )?,
        PreparedSignalKind::Operator {
            operator,
            inputs,
            automation,
            vm_slot,
        } => {
            let mut automation_scratch = scratch.operator_automation[node_index].take();
            if !automation.is_empty() {
                let state = automation_scratch.get_or_insert_with(|| operator.params.clone());
                if let Err(error) = apply_bound_automation(state, automation, sample_time) {
                    scratch.operator_automation[node_index] = automation_scratch;
                    return Err(error);
                }
            }
            let vm_slot = usize::from(*vm_slot);
            let mut vm_scratch = std::mem::take(&mut scratch.operator_vm[vm_slot]);
            let sampled = sample_operator_pixel(
                renderer,
                operator,
                inputs,
                automation_scratch.as_ref(),
                sample_time,
                flat_pixel_index,
                cache,
                scratch,
                &mut vm_scratch,
            );
            scratch.operator_vm[vm_slot] = vm_scratch;
            scratch.operator_automation[node_index] = automation_scratch;
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
                        scratch,
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
    renderer: &PreparedSequenceRenderer,
    layer_index: usize,
    sample_time: SampleTime,
    flat_pixel_index: usize,
    scratch: &mut SequenceRenderScratch,
) -> Result<Color, RenderError> {
    let Some(layer) = renderer.layers.get(layer_index) else {
        return Err(RenderError::BadGraph {
            message: "layer index is out of bounds".to_string(),
        });
    };
    if !layer.enabled {
        return Ok(black());
    }
    let Some(pixel) = renderer.signal_graph.target.get(flat_pixel_index) else {
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
        if let Ok(target_index) = effect.target.binary_search_by_key(
            &(pixel.element_index(), pixel.element_cell_index()),
            |effect_pixel| {
                (
                    effect_pixel.element_index(),
                    effect_pixel.element_cell_index(),
                )
            },
        ) {
            let effect_pixel = &effect.target[target_index];
            let mut automation_scratch = scratch.effect_automation[*effect_index].take();
            let sampled = (|| {
                let (params, native_sample) = if effect.automation.is_none() {
                    (None, None)
                } else {
                    let state =
                        automation_scratch.get_or_insert_with(EffectAutomationScratch::default);
                    match &effect.implementation {
                        crate::PreparedEffectImplementation::Native { .. } => (
                            None,
                            Some(prepare_native_sample(effect, sample_time, state)?),
                        ),
                        crate::PreparedEffectImplementation::Dsl { .. } => (
                            Some(prepare_effect_params(effect, sample_time, state)?),
                            None,
                        ),
                    }
                };
                Ok::<_, RenderError>(sample_effect_pixel(
                    effect,
                    params,
                    native_sample,
                    effect_pixel,
                    effect.progress(sample_time),
                    effect.local_time(sample_time),
                    &mut scratch.effect_vm,
                )?)
            })();
            scratch.effect_automation[*effect_index] = automation_scratch;
            let color = sampled?;
            compose_max(&mut rendered, color);
        }
    }
    Ok(rendered)
}

#[allow(clippy::too_many_arguments)]
fn sample_operator_pixel(
    renderer: &PreparedSequenceRenderer,
    operator: &PreparedOperatorNode,
    inputs: &[usize],
    automation: Option<&BoundParams>,
    sample_time: SampleTime,
    flat_pixel_index: usize,
    cache: &mut Vec<CachedSignal>,
    scratch: &mut SequenceRenderScratch,
    vm_scratch: &mut DslVmScratch,
) -> Result<Color, RenderError> {
    let params = automation.unwrap_or(&operator.params);
    let PreparedOperator::Native(builtin) = &operator.implementation else {
        let PreparedOperator::Dsl(compiled) = &operator.implementation else {
            unreachable!("operator implementation is native or DSL")
        };
        let pixel = &renderer.signal_graph.target[flat_pixel_index];
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
            scratch,
        };
        return Ok(compiled.sample_operator(params, &context, &mut sampler, vm_scratch)?);
    };
    let sample = |input: usize,
                  time: SampleTime,
                  cache: &mut Vec<CachedSignal>,
                  scratch: &mut SequenceRenderScratch| {
        sample_signal_pixel(
            renderer,
            inputs[input],
            time,
            flat_pixel_index,
            cache,
            scratch,
        )
    };
    Ok(match builtin {
        BuiltinOperator::Max => max_color(
            sample(0, sample_time, cache, scratch)?,
            sample(1, sample_time, cache, scratch)?,
        ),
        BuiltinOperator::Add => add_color(
            sample(0, sample_time, cache, scratch)?,
            sample(1, sample_time, cache, scratch)?,
        ),
        BuiltinOperator::Multiply => multiply_color(
            sample(0, sample_time, cache, scratch)?,
            sample(1, sample_time, cache, scratch)?,
        ),
        BuiltinOperator::IntensityModulate => {
            let source = sample(0, sample_time, cache, scratch)?;
            scale_color(source, intensity(sample(1, sample_time, cache, scratch)?))
        }
        BuiltinOperator::Dim => scale_color(
            sample(0, sample_time, cache, scratch)?,
            params.float(0)?.clamp(0.0, 1.0),
        ),
        BuiltinOperator::Invert => invert_color(sample(0, sample_time, cache, scratch)?),
        BuiltinOperator::Colorize => {
            let source = sample(0, sample_time, cache, scratch)?;
            scale_color(params.color(0)?, intensity(source))
        }
        BuiltinOperator::Delay => {
            let Some(delayed_time) =
                sample_time.checked_sub_duration(operator_delay(params, "delay")?)
            else {
                return Ok(black());
            };
            sample(0, delayed_time, cache, scratch)?
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
                    scale_color(sample(0, delayed_time, cache, scratch)?, decay.powi(repeat)),
                );
            }
            output
        }
    })
}

struct GraphSignalSampler<'a> {
    renderer: &'a PreparedSequenceRenderer,
    inputs: &'a [usize],
    cache: &'a mut Vec<CachedSignal>,
    flat_pixel_index: usize,
    duration: SampleDuration,
    scratch: &'a mut SequenceRenderScratch,
}

impl SignalSampler for GraphSignalSampler<'_> {
    fn sample_signal(
        &mut self,
        input: usize,
        seconds: f32,
        _pixel_index: usize,
    ) -> Result<Color, RuntimeError> {
        let sample_time = match sample_time_from_seconds_f32(seconds) {
            Ok(time) => time,
            Err(SampleTimeError::Negative) => return Ok(black()),
            Err(_) => {
                return Err(RuntimeError {
                    message: "Signal sample time exceeds the runtime clock range".to_string(),
                });
            }
        };
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
            self.scratch,
        )
        .map_err(|error| RuntimeError {
            message: format!("failed to sample Signal: {error:?}"),
        })
    }
}
