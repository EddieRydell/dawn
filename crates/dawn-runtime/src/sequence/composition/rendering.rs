use crate::sequence::color::{
    add_color, black, color_param, compose_max, intensity, invert_color, max_color, multiply_color,
    scale_color,
};
use crate::sequence::composition::graph::PreparedSignalKind;
use crate::sequence::composition::graph::{float_param, int_param};
use crate::sequence::effects::preparation::apply_automation_params;
use crate::sequence::effects::sampling::sample_effect_pixel;
use crate::*;
use dawn_language::operator::{BuiltinOperator, OperatorImplementation};
use dawn_language::values::{
    SampleDuration, SampleTime, SampleTimeError, sample_time_from_seconds_f32,
    sample_time_seconds_f32,
};

pub(crate) fn sample_effect_into(
    effect: &PreparedEffect,
    element_cell_offsets: &[usize],
    rendered: &mut [Color],
    sample_time: SampleTime,
    scratch: &mut DslVmScratch,
    bind_cache: &mut DslBindCache,
) -> Result<(), RenderError> {
    let local_seconds = effect.local_seconds(sample_time);
    let progress = effect.progress(sample_time);
    let automated =
        effect_implementation_at(effect, sample_time_seconds_f32(sample_time), bind_cache)?;
    let implementation = automated.as_ref().unwrap_or(&effect.implementation);

    if let Some(groups) = &effect.sample_groups {
        for group in groups.iter() {
            let color = sample_effect_group(
                effect,
                implementation,
                group.context,
                progress,
                local_seconds,
                scratch,
            )?;
            for target_index in &group.target_indexes {
                let Some(pixel) = effect.target.get(*target_index) else {
                    continue;
                };
                let flat_index =
                    element_cell_offsets[pixel.element_index] + pixel.element_cell_index;
                compose_max(&mut rendered[flat_index], color);
            }
        }
        return Ok(());
    }

    for pixel in effect.target.iter() {
        let color = sample_effect_pixel(
            effect,
            implementation,
            pixel,
            progress,
            local_seconds,
            scratch,
        )?;
        let flat_index = element_cell_offsets[pixel.element_index] + pixel.element_cell_index;
        compose_max(&mut rendered[flat_index], color);
    }
    Ok(())
}

pub(crate) fn sample_signal_graph(
    renderer: &PreparedSequenceRenderer,
    sample_time: SampleTime,
    output: &mut [Color],
    scratch: &mut SequenceRenderScratch,
) -> Result<(), RenderError> {
    prune_layer_cache(renderer, sample_time, scratch);
    let mut cache = std::mem::take(&mut scratch.signal_cache);
    if scratch.signal_cache_time != Some(sample_time) {
        recycle_graph_color_buffers(&mut cache, scratch);
        scratch.signal_cache_time = Some(sample_time);
    }
    let output_key = match sample_signal_node(
        renderer,
        renderer.signal_graph.output_index,
        sample_time,
        &mut cache,
        scratch,
    ) {
        Ok(output_key) => output_key,
        Err(error) => {
            recycle_graph_color_buffers(&mut cache, scratch);
            scratch.signal_cache_time = None;
            scratch.signal_cache = cache;
            return Err(error);
        }
    };
    output.copy_from_slice(
        cache
            .get(&output_key)
            .ok_or_else(|| RenderError::BadGraph {
                message: "sampled output signal is missing from the cache".to_string(),
            })?,
    );
    scratch.signal_cache = cache;
    Ok(())
}

fn prune_layer_cache(
    renderer: &PreparedSequenceRenderer,
    sample_time: SampleTime,
    scratch: &mut SequenceRenderScratch,
) {
    let oldest_sample_ticks = sample_time
        .checked_sub_duration(renderer.layer_cache_history)
        .map_or(0, |time| time.ticks());
    let expired = scratch
        .layer_cache
        .keys()
        .filter(|key| {
            renderer.layer_cache_history.ticks() == 0
                || key.sample_time.ticks() < oldest_sample_ticks
                || key.sample_time > sample_time
        })
        .copied()
        .collect::<Vec<_>>();
    for key in expired {
        let Some(mut colors) = scratch.layer_cache.remove(&key) else {
            continue;
        };
        colors.clear();
        scratch.color_buffers.push(colors);
    }
}

pub(crate) fn take_black_color_buffer(
    scratch: &mut SequenceRenderScratch,
    len: usize,
) -> Vec<Color> {
    let mut colors = scratch.color_buffers.pop().unwrap_or_default();
    colors.clear();
    colors.resize(len, black());
    colors
}

fn take_empty_color_buffer(scratch: &mut SequenceRenderScratch, capacity: usize) -> Vec<Color> {
    let mut colors = scratch.color_buffers.pop().unwrap_or_default();
    colors.clear();
    if colors.capacity() < capacity {
        colors.reserve(capacity);
    }
    colors
}

fn recycle_graph_color_buffers(
    cache: &mut HashMap<SignalCacheKey, Vec<Color>>,
    scratch: &mut SequenceRenderScratch,
) {
    for (_, mut colors) in cache.drain() {
        colors.clear();
        scratch.color_buffers.push(colors);
    }
}

#[allow(dead_code)]
#[cfg(test)]
fn sample_signal_node_for_test(
    renderer: &PreparedSequenceRenderer,
    node_index: usize,
    sample_time: SampleTime,
    cache: &mut HashMap<SignalCacheKey, Vec<Color>>,
) -> Result<Vec<Color>, RenderError> {
    let mut scratch = SequenceRenderScratch::default();
    scratch
        .effect_vm
        .resize_with(renderer.effects.len(), DslVmScratch::default);
    scratch
        .operator_vm
        .resize_with(renderer.signal_graph.nodes.len(), DslVmScratch::default);
    let key = sample_signal_node(renderer, node_index, sample_time, cache, &mut scratch)?;
    Ok(cache.get(&key).cloned().unwrap_or_default())
}

fn sample_signal_node(
    renderer: &PreparedSequenceRenderer,
    node_index: usize,
    sample_time: SampleTime,
    cache: &mut HashMap<SignalCacheKey, Vec<Color>>,
    scratch: &mut SequenceRenderScratch,
) -> Result<SignalCacheKey, RenderError> {
    let key = SignalCacheKey {
        node_index,
        sample_time,
    };
    if cache.contains_key(&key) {
        return Ok(key);
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
            if renderer.layer_cache_history.ticks() > 0
                && let Some(colors) = scratch.layer_cache.get(&key)
            {
                colors.clone()
            } else {
                let colors = renderer.sample_layer(*layer_index, sample_time, scratch)?;
                if renderer.layer_cache_history.ticks() > 0 {
                    scratch.layer_cache.insert(key, colors.clone());
                }
                colors
            }
        }
        PreparedSignalKind::Operator {
            definition,
            inputs,
            params,
            automation,
            bound_params,
        } => {
            let automated_params = if automation.is_empty() {
                None
            } else {
                Some(apply_automation_params(
                    params.clone(),
                    automation,
                    sample_time_seconds_f32(sample_time),
                )?)
            };
            let params = automated_params.as_ref().unwrap_or(params);
            let mut operator_vm_scratch = std::mem::take(&mut scratch.operator_vm[node_index]);
            let rendered = sample_operator(OperatorSampleContext {
                renderer,
                definition,
                inputs,
                params,
                prepared_bound_params: bound_params.as_ref(),
                target: graph.target.as_ref(),
                sample_time,
                cache,
                scratch,
                operator_vm_scratch: &mut operator_vm_scratch,
            });
            scratch.operator_vm[node_index] = operator_vm_scratch;
            rendered?
        }
        PreparedSignalKind::Output { inputs } => {
            let mut output = take_black_color_buffer(scratch, graph.target.len());
            for input in inputs {
                let source_key = sample_signal_node(renderer, *input, sample_time, cache, scratch)?;
                let source = &cache[&source_key];
                for (target, source) in output.iter_mut().zip(source.iter().copied()) {
                    compose_max(target, source);
                }
            }
            output
        }
    };
    cache.insert(key, colors);
    Ok(key)
}

struct OperatorSampleContext<'a> {
    renderer: &'a PreparedSequenceRenderer,
    definition: &'a OperatorDefinition,
    inputs: &'a [usize],
    params: &'a IndexMap<Identifier, Value>,
    prepared_bound_params: Option<&'a BoundParams>,
    target: &'a [PreparedTargetPixel],
    sample_time: SampleTime,
    cache: &'a mut HashMap<SignalCacheKey, Vec<Color>>,
    scratch: &'a mut SequenceRenderScratch,
    operator_vm_scratch: &'a mut DslVmScratch,
}

fn sample_operator(context: OperatorSampleContext<'_>) -> Result<Vec<Color>, RenderError> {
    let OperatorSampleContext {
        renderer,
        definition,
        inputs,
        params,
        prepared_bound_params,
        target,
        sample_time,
        cache,
        scratch,
        operator_vm_scratch,
    } = context;
    let OperatorImplementation::Native(operator) = &definition.implementation else {
        let OperatorImplementation::Dsl(compiled) = &definition.implementation else {
            unreachable!("operator implementation is native or DSL")
        };
        let dynamic_bound_params;
        let bound = if let Some(bound) = prepared_bound_params {
            bound
        } else {
            dynamic_bound_params = compiled.bind_params_cached(params, &mut scratch.bind_cache)?;
            &dynamic_bound_params
        };
        let duration = renderer.duration_seconds();
        let sample_seconds = sample_time_seconds_f32(sample_time);
        let mut output = take_empty_color_buffer(scratch, target.len());
        let mut sampler = GraphSignalSampler {
            renderer,
            inputs,
            cache,
            flat_pixel_index: 0,
            duration: renderer.duration,
            sampled_signals: 0,
            scratch,
        };
        for (flat_pixel_index, pixel) in target.iter().enumerate() {
            let context = OperatorRunContext {
                progress: (sample_seconds / duration).clamp(0.0, 1.0),
                seconds: sample_seconds,
                duration,
                pixel_index: pixel.pixel_index as i32,
                pixel_count: pixel.pixel_count as i32,
                pixel_fraction: pixel.pixel_fraction,
                global_marks: Marks { marks: Vec::new() },
            };
            sampler.flat_pixel_index = flat_pixel_index;
            output.push(compiled.sample_bound(
                bound,
                &context,
                &mut sampler,
                operator_vm_scratch,
            )?);
        }
        return Ok(output);
    };
    match operator {
        BuiltinOperator::Max => {
            binary_graph_op(renderer, inputs, sample_time, cache, scratch, max_color)
        }
        BuiltinOperator::Add => {
            binary_graph_op(renderer, inputs, sample_time, cache, scratch, add_color)
        }
        BuiltinOperator::Multiply => binary_graph_op(
            renderer,
            inputs,
            sample_time,
            cache,
            scratch,
            multiply_color,
        ),
        BuiltinOperator::IntensityModulate => {
            let source_key = sample_signal_node(renderer, inputs[0], sample_time, cache, scratch)?;
            let mask_key = sample_signal_node(renderer, inputs[1], sample_time, cache, scratch)?;
            let source = &cache[&source_key];
            let mask = &cache[&mask_key];
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
            let amount = float_param(params, "amount")?.clamp(0.0, 1.0);
            let source_key = sample_signal_node(renderer, inputs[0], sample_time, cache, scratch)?;
            let source = &cache[&source_key];
            let mut output = take_empty_color_buffer(scratch, source.len());
            output.extend(
                source
                    .iter()
                    .copied()
                    .map(|color| scale_color(color, amount)),
            );
            Ok(output)
        }
        BuiltinOperator::Invert => {
            let source_key = sample_signal_node(renderer, inputs[0], sample_time, cache, scratch)?;
            let source = &cache[&source_key];
            let mut output = take_empty_color_buffer(scratch, source.len());
            output.extend(source.iter().copied().map(invert_color));
            Ok(output)
        }
        BuiltinOperator::Colorize => {
            let tint = color_param(params, "color")?;
            let source_key = sample_signal_node(renderer, inputs[0], sample_time, cache, scratch)?;
            let source = &cache[&source_key];
            let mut output = take_empty_color_buffer(scratch, source.len());
            output.extend(
                source
                    .iter()
                    .copied()
                    .map(|color| scale_color(tint, intensity(color))),
            );
            Ok(output)
        }
        BuiltinOperator::Delay => Err(RenderError::BadGraph {
            message: "Delay must use its DSL implementation".to_string(),
        }),
        BuiltinOperator::Echo => {
            let delay = float_param(params, "seconds")?.max(0.0);
            let delay =
                dawn_language::values::DawnDuration::try_from_seconds_f32(delay).map_err(|_| {
                    RenderError::InvalidTiming {
                        reason: "echo delay exceeds the runtime clock range".to_string(),
                    }
                })?;
            let delay = dawn_language::values::sample_duration_from_dawn_duration(&delay).map_err(
                |_| RenderError::InvalidTiming {
                    reason: "echo delay exceeds the runtime clock range".to_string(),
                },
            )?;
            let repeats = int_param(params, "repeats")?.clamp(1, 32);
            let decay = float_param(params, "decay")?.clamp(0.0, 1.0);
            let mut output = take_black_color_buffer(scratch, renderer.signal_graph.target.len());
            for repeat in 0..=repeats {
                let Some(delayed_time) = sample_time.checked_sub_duration(
                    SampleDuration::from_ticks(delay.ticks().saturating_mul(repeat as u32)),
                ) else {
                    continue;
                };
                let source_key =
                    sample_signal_node(renderer, inputs[0], delayed_time, cache, scratch)?;
                let source = &cache[&source_key];
                let amount = decay.powi(repeat);
                for (target, source) in output.iter_mut().zip(source.iter().copied()) {
                    compose_max(target, scale_color(source, amount));
                }
            }
            Ok(output)
        }
    }
}

fn binary_graph_op(
    renderer: &PreparedSequenceRenderer,
    inputs: &[usize],
    sample_time: SampleTime,
    cache: &mut HashMap<SignalCacheKey, Vec<Color>>,
    scratch: &mut SequenceRenderScratch,
    op: fn(Color, Color) -> Color,
) -> Result<Vec<Color>, RenderError> {
    let left_key = sample_signal_node(renderer, inputs[0], sample_time, cache, scratch)?;
    let right_key = sample_signal_node(renderer, inputs[1], sample_time, cache, scratch)?;
    let left = &cache[&left_key];
    let right = &cache[&right_key];
    let mut output = take_empty_color_buffer(scratch, left.len().min(right.len()));
    output.extend(
        left.iter()
            .copied()
            .zip(right.iter().copied())
            .map(|(left, right)| op(left, right)),
    );
    Ok(output)
}

struct GraphSignalSampler<'a> {
    renderer: &'a PreparedSequenceRenderer,
    inputs: &'a [usize],
    cache: &'a mut HashMap<SignalCacheKey, Vec<Color>>,
    flat_pixel_index: usize,
    duration: SampleDuration,
    sampled_signals: usize,
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
        let key = SignalCacheKey {
            node_index: node,
            sample_time,
        };
        if !self.cache.contains_key(&key)
            && self.sampled_signals >= MAX_SIGNAL_SAMPLES_PER_OPERATOR_RENDER
        {
            return Err(RuntimeError {
                message: format!(
                    "Signal sampling exceeded the per-operator budget of {MAX_SIGNAL_SAMPLES_PER_OPERATOR_RENDER} unique times"
                ),
            });
        }
        if !self.cache.contains_key(&key) {
            self.sampled_signals += 1;
        }
        let key = sample_signal_node(self.renderer, node, sample_time, self.cache, self.scratch)
            .map_err(|error| RuntimeError {
                message: format!("failed to sample Signal: {error:?}"),
            })?;
        let color = self.cache[&key]
            .get(self.flat_pixel_index)
            .copied()
            .unwrap_or_else(black);
        Ok(color)
    }
}
