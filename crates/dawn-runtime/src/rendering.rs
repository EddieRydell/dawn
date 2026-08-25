use super::color::{
    add_color, black, color_param, compose_max, intensity, invert_color, max_color, multiply_color,
    scale_color,
};
use super::effect_preparation::apply_automation_params;
use super::graph::{float_param, int_param};
use super::sampling::sample_effect_pixel;
use super::*;
use dawn_language::operator::{BuiltinOperator, OperatorImplementation};

fn unflatten_rendered_elements(
    elements: &[PreparedElement],
    colors: &[Color],
) -> Vec<RenderedElement> {
    let mut offset = 0usize;
    elements
        .iter()
        .map(|element| {
            let end = offset.saturating_add(element.pixel_count).min(colors.len());
            let mut pixels = colors[offset..end].to_vec();
            if pixels.len() < element.pixel_count {
                pixels.resize(element.pixel_count, black());
            }
            offset = offset.saturating_add(element.pixel_count);
            RenderedElement {
                element_id: element.id,
                pixels,
            }
        })
        .collect()
}

pub(crate) fn render_effect(
    effect: &PreparedEffect,
    element_cell_offsets: &[usize],
    rendered: &mut [Color],
    sample_seconds: f64,
    scratch: &mut DslVmScratch,
    bind_cache: &mut DslBindCache,
) -> Result<(), RenderError> {
    let local_seconds = sample_seconds - effect.start_seconds;
    let progress = (local_seconds / effect.duration_seconds).clamp(0.0, 1.0);
    let automated = effect_implementation_at(effect, sample_seconds, bind_cache)?;
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

pub(crate) fn render_composition_graph(
    renderer: &PreparedSequenceRenderer,
    sample_seconds: f64,
    scratch: &mut SequenceRenderScratch,
) -> Result<Vec<RenderedElement>, RenderError> {
    prune_layer_cache(renderer, sample_seconds, scratch);
    let frame_key = (sample_seconds * 1_000_000.0).round() as i64;
    let mut cache = std::mem::take(&mut scratch.graph_cache);
    if scratch.graph_cache_frame_key != Some(frame_key) {
        recycle_graph_color_buffers(&mut cache, scratch);
        scratch.graph_cache_frame_key = Some(frame_key);
    }
    let output = match render_graph_node_with_scratch(
        renderer,
        renderer.composition_graph.output_index,
        sample_seconds,
        &mut cache,
        scratch,
    ) {
        Ok(output) => output,
        Err(error) => {
            recycle_graph_color_buffers(&mut cache, scratch);
            scratch.graph_cache_frame_key = None;
            scratch.graph_cache = cache;
            return Err(error);
        }
    };
    let rendered = unflatten_rendered_elements(&renderer.elements, output.as_ref());
    drop(output);
    scratch.graph_cache = cache;
    Ok(rendered)
}

fn prune_layer_cache(
    renderer: &PreparedSequenceRenderer,
    sample_seconds: f64,
    scratch: &mut SequenceRenderScratch,
) {
    let current_frame_key = (sample_seconds * 1_000_000.0).round() as i64;
    let oldest_frame_key = current_frame_key - renderer.layer_cache_history_micros;
    let expired = scratch
        .layer_cache
        .keys()
        .filter(|key| {
            renderer.layer_cache_history_micros == 0
                || key.frame_key < oldest_frame_key
                || key.frame_key > current_frame_key
        })
        .copied()
        .collect::<Vec<_>>();
    for key in expired {
        let Some(colors) = scratch.layer_cache.remove(&key) else {
            continue;
        };
        if let Ok(mut colors) = Arc::try_unwrap(colors) {
            colors.clear();
            scratch.color_buffers.push(colors);
        }
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
    cache: &mut HashMap<GraphRenderCacheKey, Arc<Vec<Color>>>,
    scratch: &mut SequenceRenderScratch,
) {
    for (_, colors) in cache.drain() {
        if let Ok(mut colors) = Arc::try_unwrap(colors) {
            colors.clear();
            scratch.color_buffers.push(colors);
        }
    }
}

#[allow(dead_code)]
#[cfg(test)]
fn render_graph_node(
    renderer: &PreparedSequenceRenderer,
    node_index: usize,
    sample_seconds: f64,
    cache: &mut HashMap<GraphRenderCacheKey, Arc<Vec<Color>>>,
) -> Result<Arc<Vec<Color>>, RenderError> {
    let mut scratch = SequenceRenderScratch::default();
    scratch
        .effect_vm
        .resize_with(renderer.effects.len(), DslVmScratch::default);
    scratch.operator_vm.resize_with(
        renderer.composition_graph.nodes.len(),
        DslVmScratch::default,
    );
    render_graph_node_with_scratch(renderer, node_index, sample_seconds, cache, &mut scratch)
}

fn render_graph_node_with_scratch(
    renderer: &PreparedSequenceRenderer,
    node_index: usize,
    sample_seconds: f64,
    cache: &mut HashMap<GraphRenderCacheKey, Arc<Vec<Color>>>,
    scratch: &mut SequenceRenderScratch,
) -> Result<Arc<Vec<Color>>, RenderError> {
    let frame_key = (sample_seconds * 1_000_000.0).round() as i64;
    let key = GraphRenderCacheKey {
        node_index,
        frame_key,
    };
    if let Some(colors) = cache.get(&key) {
        return Ok(Arc::clone(colors));
    }
    let graph = &renderer.composition_graph;
    let node = graph
        .nodes
        .get(node_index)
        .ok_or_else(|| RenderError::BadGraph {
            message: "graph node index is out of bounds".to_string(),
        })?;
    let colors = match &node.kind {
        PreparedGraphNodeKind::Layer { layer_index } => {
            if renderer.layer_cache_history_micros > 0
                && let Some(colors) = scratch.layer_cache.get(&key)
            {
                Arc::clone(colors)
            } else {
                let colors =
                    Arc::new(renderer.render_layer_at(*layer_index, sample_seconds, scratch)?);
                if renderer.layer_cache_history_micros > 0 {
                    scratch.layer_cache.insert(key, Arc::clone(&colors));
                }
                colors
            }
        }
        PreparedGraphNodeKind::Operator {
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
                    sample_seconds,
                )?)
            };
            let params = automated_params.as_ref().unwrap_or(params);
            let mut operator_vm_scratch = std::mem::take(&mut scratch.operator_vm[node_index]);
            let rendered = render_graph_operator(GraphOperatorRenderContext {
                renderer,
                definition,
                inputs,
                params,
                prepared_bound_params: bound_params.as_ref(),
                target: node.target.as_ref(),
                sample_seconds,
                cache,
                scratch,
                operator_vm_scratch: &mut operator_vm_scratch,
            });
            scratch.operator_vm[node_index] = operator_vm_scratch;
            rendered?
        }
        PreparedGraphNodeKind::Output { inputs } => {
            let mut output = take_black_color_buffer(scratch, node.target.len());
            for input in inputs {
                let source = render_graph_node_with_scratch(
                    renderer,
                    *input,
                    sample_seconds,
                    cache,
                    scratch,
                )?;
                for (target, source) in output.iter_mut().zip(source.iter().copied()) {
                    compose_max(target, source);
                }
            }
            Arc::new(output)
        }
    };
    cache.insert(key, Arc::clone(&colors));
    Ok(colors)
}

struct GraphOperatorRenderContext<'a> {
    renderer: &'a PreparedSequenceRenderer,
    definition: &'a OperatorDefinition,
    inputs: &'a [usize],
    params: &'a IndexMap<Identifier, Value>,
    prepared_bound_params: Option<&'a BoundParams>,
    target: &'a [PreparedTargetPixel],
    sample_seconds: f64,
    cache: &'a mut HashMap<GraphRenderCacheKey, Arc<Vec<Color>>>,
    scratch: &'a mut SequenceRenderScratch,
    operator_vm_scratch: &'a mut DslVmScratch,
}

fn render_graph_operator(
    context: GraphOperatorRenderContext<'_>,
) -> Result<Arc<Vec<Color>>, RenderError> {
    let GraphOperatorRenderContext {
        renderer,
        definition,
        inputs,
        params,
        prepared_bound_params,
        target,
        sample_seconds,
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
        let duration = renderer.duration_seconds;
        let mut output = take_empty_color_buffer(scratch, target.len());
        let mut sampler = GraphSignalSampler {
            renderer,
            inputs,
            cache,
            flat_pixel_index: 0,
            duration,
            samples: HashMap::new(),
            scratch,
        };
        for (flat_pixel_index, pixel) in target.iter().enumerate() {
            let context = OperatorRunContext {
                progress: (sample_seconds / duration).clamp(0.0, 1.0),
                seconds: sample_seconds,
                duration,
                pixel_index: pixel.pixel_index as i64,
                pixel_count: pixel.pixel_count as i64,
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
        return Ok(Arc::new(output));
    };
    match operator {
        BuiltinOperator::Max => {
            binary_graph_op(renderer, inputs, sample_seconds, cache, scratch, max_color)
        }
        BuiltinOperator::Add => {
            binary_graph_op(renderer, inputs, sample_seconds, cache, scratch, add_color)
        }
        BuiltinOperator::Multiply => binary_graph_op(
            renderer,
            inputs,
            sample_seconds,
            cache,
            scratch,
            multiply_color,
        ),
        BuiltinOperator::IntensityModulate => {
            let source = render_graph_node_with_scratch(
                renderer,
                inputs[0],
                sample_seconds,
                cache,
                scratch,
            )?;
            let mask = render_graph_node_with_scratch(
                renderer,
                inputs[1],
                sample_seconds,
                cache,
                scratch,
            )?;
            let mut output = take_empty_color_buffer(scratch, source.len().min(mask.len()));
            output.extend(
                source
                    .iter()
                    .copied()
                    .zip(mask.iter().copied())
                    .map(|(source, mask)| scale_color(source, intensity(mask))),
            );
            Ok(Arc::new(output))
        }
        BuiltinOperator::Dim => {
            let amount = float_param(params, "amount")?.clamp(0.0, 1.0);
            let source = render_graph_node_with_scratch(
                renderer,
                inputs[0],
                sample_seconds,
                cache,
                scratch,
            )?;
            let mut output = take_empty_color_buffer(scratch, source.len());
            output.extend(
                source
                    .iter()
                    .copied()
                    .map(|color| scale_color(color, amount)),
            );
            Ok(Arc::new(output))
        }
        BuiltinOperator::Invert => {
            let source = render_graph_node_with_scratch(
                renderer,
                inputs[0],
                sample_seconds,
                cache,
                scratch,
            )?;
            let mut output = take_empty_color_buffer(scratch, source.len());
            output.extend(source.iter().copied().map(invert_color));
            Ok(Arc::new(output))
        }
        BuiltinOperator::Colorize => {
            let tint = color_param(params, "color")?;
            let source = render_graph_node_with_scratch(
                renderer,
                inputs[0],
                sample_seconds,
                cache,
                scratch,
            )?;
            let mut output = take_empty_color_buffer(scratch, source.len());
            output.extend(
                source
                    .iter()
                    .copied()
                    .map(|color| scale_color(tint, intensity(color))),
            );
            Ok(Arc::new(output))
        }
        BuiltinOperator::Delay => Err(RenderError::BadGraph {
            message: "Delay must use its DSL implementation".to_string(),
        }),
        BuiltinOperator::Echo => {
            let delay = float_param(params, "seconds")?.max(0.0);
            let repeats = int_param(params, "repeats")?.clamp(1, 32);
            let decay = float_param(params, "decay")?.clamp(0.0, 1.0);
            let mut output = take_black_color_buffer(
                scratch,
                renderer.composition_graph.nodes[inputs[0]].target.len(),
            );
            for repeat in 0..=repeats {
                let source = render_graph_node_with_scratch(
                    renderer,
                    inputs[0],
                    sample_seconds - delay * repeat as f64,
                    cache,
                    scratch,
                )?;
                let amount = decay.powi(repeat as i32);
                for (target, source) in output.iter_mut().zip(source.iter().copied()) {
                    compose_max(target, scale_color(source, amount));
                }
            }
            Ok(Arc::new(output))
        }
    }
}

fn binary_graph_op(
    renderer: &PreparedSequenceRenderer,
    inputs: &[usize],
    sample_seconds: f64,
    cache: &mut HashMap<GraphRenderCacheKey, Arc<Vec<Color>>>,
    scratch: &mut SequenceRenderScratch,
    op: fn(Color, Color) -> Color,
) -> Result<Arc<Vec<Color>>, RenderError> {
    let left = render_graph_node_with_scratch(renderer, inputs[0], sample_seconds, cache, scratch)?;
    let right =
        render_graph_node_with_scratch(renderer, inputs[1], sample_seconds, cache, scratch)?;
    let mut output = take_empty_color_buffer(scratch, left.len().min(right.len()));
    output.extend(
        left.iter()
            .copied()
            .zip(right.iter().copied())
            .map(|(left, right)| op(left, right)),
    );
    Ok(Arc::new(output))
}

struct GraphSignalSampler<'a> {
    renderer: &'a PreparedSequenceRenderer,
    inputs: &'a [usize],
    cache: &'a mut HashMap<GraphRenderCacheKey, Arc<Vec<Color>>>,
    flat_pixel_index: usize,
    duration: f64,
    samples: HashMap<GraphRenderCacheKey, Arc<Vec<Color>>>,
    scratch: &'a mut SequenceRenderScratch,
}

impl SignalSampler for GraphSignalSampler<'_> {
    fn sample_signal(
        &mut self,
        input: usize,
        seconds: f64,
        _pixel_index: usize,
    ) -> Result<Color, RuntimeError> {
        if !seconds.is_finite() || seconds < 0.0 || seconds >= self.duration {
            return Ok(black());
        }
        let node = self
            .inputs
            .get(input)
            .copied()
            .ok_or_else(|| RuntimeError {
                message: "Signal input index is out of bounds".to_string(),
            })?;
        let key = GraphRenderCacheKey {
            node_index: node,
            frame_key: (seconds * 1_000_000.0).round() as i64,
        };
        if let Some(colors) = self.samples.get(&key) {
            return Ok(colors
                .get(self.flat_pixel_index)
                .copied()
                .unwrap_or_else(black));
        }
        if self.samples.len() >= MAX_SIGNAL_SAMPLES_PER_OPERATOR_RENDER {
            return Err(RuntimeError {
                message: format!(
                    "Signal sampling exceeded the per-operator budget of {MAX_SIGNAL_SAMPLES_PER_OPERATOR_RENDER} unique times"
                ),
            });
        }
        let colors =
            render_graph_node_with_scratch(self.renderer, node, seconds, self.cache, self.scratch)
                .map_err(|error| RuntimeError {
                    message: format!("failed to sample Signal: {error:?}"),
                })?;
        let color = colors
            .get(self.flat_pixel_index)
            .copied()
            .unwrap_or_else(black);
        self.samples.insert(key, colors);
        Ok(color)
    }
}
