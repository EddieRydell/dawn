#![deny(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

mod color;
mod elements;
mod generators;
mod graph;
mod params;
mod rendering;
mod sampling;
mod show;
mod target;
pub use show::*;

use dawn_language::dsl::{
    BoundParams, CompiledEffect, DslBindCache, DslVmScratch, EffectKind, Identifier,
    OperatorRunContext, RuntimeError, SignalSampler, TargetItemValue, TargetValue, Value,
};
use dawn_language::effect::{
    EffectDefinitionId, EffectImplementation, EffectInstId, EffectRef, EffectScope,
};
use dawn_language::element::{ElementNodeId, ElementSelection};
use dawn_language::model::DawnProject;
use dawn_language::native_effect::{self, BoundNativeEffect, NativeSample};
use dawn_language::operator::{BuiltinOperator, OperatorDefinition, OperatorImplementation};
use dawn_language::sequence::{
    AutomationBinding, AutomationClip, AutomationMapping, AutomationTarget, AutomationValue,
    CompositionGraphNodeId, MarkCollectionKey, Sequence, SequenceId, SequenceLayerId,
    automation_value_at,
};
use dawn_language::setup::SetupId;
use dawn_language::validation::{MAX_SEQUENCE_FRAME_COUNT, validate_sequence};
use dawn_language::values::{Color, Marks};
use indexmap::{IndexMap, IndexSet};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use color::black;
use elements::{PreparedElement, element_cell_offsets, prepare_elements};
use generators::{
    GeneratorExpansion, GeneratorPrepareContext, expand_generator, expand_native_generator,
};
use graph::{
    PrepareGraphContext, PreparedCompositionGraph, PreparedGraphNodeKind, prepare_composition_graph,
};
use params::{EffectParamTiming, prepare_operator_params, prepare_params};
use rendering::{render_composition_graph, render_effect, take_black_color_buffer};
use sampling::{
    PreparedSampleContextGroup, PreparedSampleGroupCacheEntry, PreparedSampledEffectPixel,
    PreparedSampledEffectPixels, TargetColorAddress, effect_implementation_at,
    evenly_sample_indices, prepare_sample_groups_for_implementation,
    prepare_sampled_effect_pixel_groups, render_sampled_effect_target_colors, sample_effect_group,
};
use target::{
    PreparedTargetCache, PreparedTargetPixel, generator_expansion_targets, prepare_target,
    prepare_target_pixels, prepare_target_pixels_cached,
};

static NEXT_RENDER_CACHE_ID: AtomicU64 = AtomicU64::new(1);

pub const MAX_GENERATED_EFFECTS: usize = 100_000;
pub const MAX_SIGNAL_SAMPLES_PER_OPERATOR_RENDER: usize = 4_096;

fn next_render_cache_id() -> u64 {
    NEXT_RENDER_CACHE_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedTargetPixelAddress {
    pub element_id: ElementNodeId,
    pub element_cell_index: usize,
}

#[derive(Clone, Debug)]
pub struct PreparedSequenceRenderer {
    render_cache_id: u64,
    frame_rate: u32,
    frame_count: u64,
    duration_seconds: f64,
    elements: Vec<PreparedElement>,
    element_cell_offsets: Vec<usize>,
    pixel_count: usize,
    effects: Vec<PreparedEffect>,
    effect_layer_indexes: Vec<usize>,
    layers: Vec<PreparedLayer>,
    composition_graph: PreparedCompositionGraph,
    effects_by_frame: Vec<Vec<usize>>,
    layer_cache_history_micros: i64,
}

#[derive(Debug, Default)]
pub struct SequenceRenderScratch {
    effect_vm: Vec<DslVmScratch>,
    operator_vm: Vec<DslVmScratch>,
    bind_cache: DslBindCache,
    graph_cache: HashMap<GraphRenderCacheKey, Arc<Vec<Color>>>,
    graph_cache_frame_key: Option<i64>,
    layer_cache: HashMap<GraphRenderCacheKey, Arc<Vec<Color>>>,
    render_cache_id: Option<u64>,
    color_buffers: Vec<Vec<Color>>,
}

#[derive(Clone, Debug)]
pub struct PreparedEffectRasterRenderer {
    frame_rate: u32,
    frame_count: u64,
    index_start_frame: u64,
    start_seconds: f64,
    duration_seconds: f64,
    target: Arc<Vec<PreparedTargetPixel>>,
    effects: Vec<PreparedEffect>,
    effects_by_frame: Vec<Vec<usize>>,
}

#[derive(Clone, Debug)]
pub struct PreparedEffectRasterSample {
    row_count: usize,
    effect_pixels: Vec<PreparedSampledEffectPixels>,
}

#[derive(Debug, Default)]
pub struct EffectRasterRenderScratch {
    effect_vm: Vec<DslVmScratch>,
    bind_cache: DslBindCache,
}

pub struct EffectRasterPrepareBatch<'a> {
    project: &'a DawnProject,
    sequence: &'a Sequence,
    elements: Vec<PreparedElement>,
    element_ids: IndexSet<ElementNodeId>,
    groups: IndexMap<ElementNodeId, Vec<ElementNodeId>>,
    frame_rate: u32,
    frame_count: u64,
    bind_cache: DslBindCache,
    compiled_effects: HashMap<EffectDefinitionId, Arc<CompiledEffect>>,
    target_cache: PrepareTargetCache,
}

pub fn resolve_effect_target_pixel_addresses(
    project: &DawnProject,
    setup_id: &SetupId,
    target: &ElementSelection,
    scope: &EffectScope,
) -> Result<Vec<RenderedTargetPixelAddress>, RenderError> {
    let setup = project
        .setups
        .get(setup_id)
        .ok_or_else(|| RenderError::MissingSetup {
            setup_id: setup_id.clone(),
        })?;
    let tree = project
        .element_trees
        .get(&setup.elements)
        .ok_or(RenderError::MissingElementTree)?;
    let (elements, groups) = prepare_elements(project, tree)?;
    let element_ids = elements
        .iter()
        .map(|element| element.id)
        .collect::<IndexSet<_>>();
    let target = prepare_target(target, &element_ids, &groups)?;
    let pixels = prepare_target_pixels(&target, &elements, scope)?;
    Ok(pixels
        .into_iter()
        .map(|pixel| RenderedTargetPixelAddress {
            element_id: elements[pixel.element_index].id,
            element_cell_index: pixel.element_cell_index,
        })
        .collect())
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderedFrame {
    pub frame_index: u64,
    pub frame_rate: u32,
    pub clock_seconds: f64,
    pub sample_seconds: f64,
    pub elements: Vec<RenderedElement>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderedElement {
    pub element_id: ElementNodeId,
    pub pixels: Vec<Color>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RenderError {
    InvalidTiming { reason: String },
    MissingSetup { setup_id: SetupId },
    MissingElementTree,
    MissingSequence { sequence_id: SequenceId },
    MissingElement { element_id: ElementNodeId },
    MissingEffect { effect_id: EffectRef },
    MissingEffectInstance { effect_id: EffectInstId },
    MissingCurve,
    MissingGradient,
    MissingMarkCollection { key: MarkCollectionKey },
    BadTarget,
    BadGraph { message: String },
    EffectVm { message: String },
    GeneratorPrepare { message: String },
}

impl From<RuntimeError> for RenderError {
    fn from(error: RuntimeError) -> Self {
        Self::EffectVm {
            message: error.message,
        }
    }
}

impl PreparedSequenceRenderer {
    pub fn prepare(
        project: &DawnProject,
        setup_id: &SetupId,
        sequence_id: &SequenceId,
    ) -> Result<Self, RenderError> {
        let setup = project
            .setups
            .get(setup_id)
            .ok_or_else(|| RenderError::MissingSetup {
                setup_id: setup_id.clone(),
            })?;
        let tree = project
            .element_trees
            .get(&setup.elements)
            .ok_or(RenderError::MissingElementTree)?;
        let sequence =
            project
                .sequences
                .get(sequence_id)
                .ok_or_else(|| RenderError::MissingSequence {
                    sequence_id: sequence_id.clone(),
                })?;
        validate_sequence(project, sequence).map_err(|error| RenderError::BadGraph {
            message: error.message,
        })?;
        prepare_timing(sequence)?;

        let (elements, groups) = prepare_elements(project, tree)?;
        let (element_cell_offsets, pixel_count) = element_cell_offsets(&elements);
        let element_ids = elements
            .iter()
            .map(|element| element.id)
            .collect::<IndexSet<_>>();
        let mut effects = Vec::with_capacity(sequence.effects.len());
        let mut generated_child_count = 0usize;
        let mut bind_cache = DslBindCache::default();
        let mut compiled_effects = HashMap::new();
        let mut target_cache = PrepareTargetCache::default();
        let layer_ids = sequence
            .layers
            .iter()
            .map(|layer| layer.id.clone())
            .collect::<IndexSet<_>>();
        for effect in &sequence.effects {
            if !layer_ids.contains(&effect.layer_id) {
                return Err(RenderError::BadGraph {
                    message: format!(
                        "effect {} references missing layer {}",
                        effect.id.0, effect.layer_id.0
                    ),
                });
            }
            prepare_effect_inst(
                PrepareEffectContext {
                    project,
                    sequence,
                    layer_id: effect.layer_id.clone(),
                    elements: &elements,
                    element_ids: &element_ids,
                    groups: &groups,
                    effects: &mut effects,
                    generated_child_count: &mut generated_child_count,
                    bind_cache: &mut bind_cache,
                    compiled_effects: &mut compiled_effects,
                    target_cache: &mut target_cache,
                },
                effect,
            )?;
        }

        let layers = sequence
            .layers
            .iter()
            .map(|layer| PreparedLayer {
                id: layer.id.clone(),
                enabled: layer.enabled,
            })
            .collect::<Vec<_>>();
        let effect_layer_indexes = effects
            .iter()
            .map(|effect| {
                layers
                    .iter()
                    .position(|layer| layer.id == effect.layer_id)
                    .ok_or_else(|| RenderError::BadGraph {
                        message: format!(
                            "prepared effect references missing layer {}",
                            effect.layer_id.0
                        ),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let frame_rate = sequence.frame_rate;
        let duration_seconds = sequence.duration.as_seconds_f64();
        let frame_count = frame_count(duration_seconds, frame_rate);
        let mut effects_by_frame = build_effect_frame_index(&effects, frame_count, frame_rate);
        for active_effects in &mut effects_by_frame {
            active_effects.sort_unstable_by_key(|effect_index| {
                (effect_layer_indexes[*effect_index], *effect_index)
            });
        }
        let composition_graph = prepare_composition_graph(
            PrepareGraphContext {
                project,
                sequence,
                elements: &elements,
                layers: &layers,
            },
            &sequence.composition_graph,
        )?;
        let layer_cache_history_micros = layer_cache_history_micros(&composition_graph)?;
        Ok(Self {
            render_cache_id: next_render_cache_id(),
            frame_rate,
            frame_count,
            duration_seconds,
            elements,
            element_cell_offsets,
            pixel_count,
            effects,
            effect_layer_indexes,
            layers,
            composition_graph,
            effects_by_frame,
            layer_cache_history_micros,
        })
    }

    pub fn render_seconds(&self, audio_seconds: f64) -> Result<RenderedFrame, RenderError> {
        self.render_seconds_with_scratch(audio_seconds, &mut SequenceRenderScratch::default())
    }

    pub fn render_seconds_with_scratch(
        &self,
        audio_seconds: f64,
        scratch: &mut SequenceRenderScratch,
    ) -> Result<RenderedFrame, RenderError> {
        if !audio_seconds.is_finite() {
            return Err(RenderError::InvalidTiming {
                reason: "audio seconds must be finite".to_string(),
            });
        }
        let max_frame = self.frame_count.saturating_sub(1);
        let frame_index = (audio_seconds * f64::from(self.frame_rate)).floor();
        let frame_index = if frame_index < 0.0 {
            0
        } else if frame_index > max_frame as f64 {
            max_frame
        } else {
            frame_index as u64
        };
        self.render_at(frame_index, audio_seconds, scratch)
    }

    pub fn render_frame(&self, frame_index: u64) -> Result<RenderedFrame, RenderError> {
        self.render_frame_with_scratch(frame_index, &mut SequenceRenderScratch::default())
    }

    pub fn render_frame_with_scratch(
        &self,
        frame_index: u64,
        scratch: &mut SequenceRenderScratch,
    ) -> Result<RenderedFrame, RenderError> {
        let frame_index = frame_index.min(self.frame_count.saturating_sub(1));
        self.render_at(
            frame_index,
            frame_index as f64 / f64::from(self.frame_rate),
            scratch,
        )
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub fn frame_rate(&self) -> u32 {
        self.frame_rate
    }

    pub fn active_effect_names(&self, frame_index: u64) -> Vec<&str> {
        let Some(active_effects) = self.effects_by_frame.get(frame_index as usize) else {
            return Vec::new();
        };
        let mut active_effects = active_effects.clone();
        active_effects.sort_unstable();
        active_effects
            .into_iter()
            .filter_map(|effect_index| self.effects.get(effect_index))
            .map(|effect| effect.name.as_str())
            .collect()
    }

    fn render_at(
        &self,
        frame_index: u64,
        clock_seconds: f64,
        scratch: &mut SequenceRenderScratch,
    ) -> Result<RenderedFrame, RenderError> {
        if scratch.render_cache_id != Some(self.render_cache_id) {
            scratch.graph_cache.clear();
            scratch.graph_cache_frame_key = None;
            scratch.layer_cache.clear();
            scratch.render_cache_id = Some(self.render_cache_id);
        }
        scratch
            .effect_vm
            .resize_with(self.effects.len(), DslVmScratch::default);
        scratch
            .operator_vm
            .resize_with(self.composition_graph.nodes.len(), DslVmScratch::default);
        let sample_seconds = frame_index as f64 / f64::from(self.frame_rate);
        let rendered = render_composition_graph(self, sample_seconds, scratch)?;

        Ok(RenderedFrame {
            frame_index,
            frame_rate: self.frame_rate,
            clock_seconds,
            sample_seconds,
            elements: rendered,
        })
    }

    fn render_layer_at(
        &self,
        layer_index: usize,
        sample_seconds: f64,
        scratch: &mut SequenceRenderScratch,
    ) -> Result<Vec<Color>, RenderError> {
        let layer = &self.layers[layer_index];
        let mut rendered = take_black_color_buffer(scratch, self.pixel_count);
        if !layer.enabled || !sample_seconds.is_finite() || sample_seconds < 0.0 {
            return Ok(rendered);
        }

        let frame_index = (sample_seconds * f64::from(self.frame_rate)).floor() as usize;
        if let Some(active_effects) = self.effects_by_frame.get(frame_index) {
            let first = active_effects.partition_point(|effect_index| {
                self.effect_layer_indexes[*effect_index] < layer_index
            });
            let last = active_effects.partition_point(|effect_index| {
                self.effect_layer_indexes[*effect_index] <= layer_index
            });
            for effect_index in &active_effects[first..last] {
                let Some(effect) = self.effects.get(*effect_index) else {
                    continue;
                };
                if sample_seconds < effect.start_seconds
                    || sample_seconds >= effect.start_seconds + effect.duration_seconds
                {
                    continue;
                }
                render_effect(
                    effect,
                    &self.element_cell_offsets,
                    &mut rendered,
                    sample_seconds,
                    &mut scratch.effect_vm[*effect_index],
                    &mut scratch.bind_cache,
                )?;
            }
        }
        Ok(rendered)
    }
}

impl PreparedEffectRasterRenderer {
    pub fn prepare(
        project: &DawnProject,
        setup_id: &SetupId,
        sequence_id: &SequenceId,
        effect_id: &EffectInstId,
    ) -> Result<Self, RenderError> {
        let mut batch = EffectRasterPrepareBatch::prepare(project, setup_id, sequence_id)?;
        batch.prepare_effect(effect_id)
    }

    pub fn start_seconds(&self) -> f64 {
        self.start_seconds
    }

    pub fn duration_seconds(&self) -> f64 {
        self.duration_seconds
    }

    pub fn frame_rate(&self) -> u32 {
        self.frame_rate
    }

    pub fn target_pixel_count(&self) -> usize {
        self.target.len()
    }

    pub fn sampled_raster_column_seconds(
        &self,
        column_index: usize,
        column_count: usize,
    ) -> Result<f64, RenderError> {
        if column_count == 0 || column_index >= column_count {
            return Err(RenderError::InvalidTiming {
                reason: "raster column index must be within a non-empty raster".to_string(),
            });
        }
        let end_frame = ((self.start_seconds + self.duration_seconds) * f64::from(self.frame_rate))
            .ceil()
            .max(self.index_start_frame as f64) as u64;
        let active_frame_count = end_frame.saturating_sub(self.index_start_frame).max(1);
        let sample_offset = (((column_index as f64 + 0.5) * active_frame_count as f64
            / column_count as f64)
            .floor() as u64)
            .min(active_frame_count.saturating_sub(1));
        Ok((self.index_start_frame + sample_offset) as f64 / f64::from(self.frame_rate))
    }

    pub fn prepare_sampled_raster(&self, row_count: usize) -> PreparedEffectRasterSample {
        let sample_indices = evenly_sample_indices(self.target.len(), row_count);
        let row_count = sample_indices.len();
        let mut sample_lookup = HashMap::<TargetColorAddress, Vec<usize>>::new();
        for (row_index, target_index) in sample_indices.into_iter().enumerate() {
            if let Some(pixel) = self.target.get(target_index) {
                sample_lookup
                    .entry(TargetColorAddress {
                        element_index: pixel.element_index,
                        element_cell_index: pixel.element_cell_index,
                    })
                    .or_default()
                    .push(row_index);
            }
        }
        let effect_pixels = self
            .effects
            .iter()
            .map(|effect| {
                let pixels = effect
                    .target
                    .iter()
                    .filter_map(|pixel| {
                        sample_lookup
                            .get(&TargetColorAddress {
                                element_index: pixel.element_index,
                                element_cell_index: pixel.element_cell_index,
                            })
                            .map(|rows| PreparedSampledEffectPixel {
                                pixel: pixel.clone(),
                                rows: rows.clone(),
                            })
                    })
                    .collect::<Vec<_>>();
                let groups = prepare_sampled_effect_pixel_groups(&pixels);
                PreparedSampledEffectPixels { pixels, groups }
            })
            .collect();
        PreparedEffectRasterSample {
            row_count,
            effect_pixels,
        }
    }

    pub fn render_sampled_raster_column_with_scratch(
        &self,
        sample: &PreparedEffectRasterSample,
        audio_seconds: f64,
        scratch: &mut EffectRasterRenderScratch,
    ) -> Result<Vec<Color>, RenderError> {
        if !audio_seconds.is_finite() {
            return Err(RenderError::InvalidTiming {
                reason: "audio seconds must be finite".to_string(),
            });
        }
        let max_frame = self.frame_count.saturating_sub(1);
        let frame_index = (audio_seconds * f64::from(self.frame_rate)).floor();
        let frame_index = if frame_index < 0.0 {
            0
        } else if frame_index > max_frame as f64 {
            max_frame
        } else {
            frame_index as u64
        };
        self.render_sampled_raster_column_at_with_scratch(sample, frame_index, scratch)
    }

    fn render_sampled_raster_column_at_with_scratch(
        &self,
        sample: &PreparedEffectRasterSample,
        frame_index: u64,
        scratch: &mut EffectRasterRenderScratch,
    ) -> Result<Vec<Color>, RenderError> {
        scratch
            .effect_vm
            .resize_with(self.effects.len(), DslVmScratch::default);
        let sample_seconds = frame_index as f64 / f64::from(self.frame_rate);
        let mut rendered = vec![black(); sample.row_count];

        let local_frame_index = frame_index.saturating_sub(self.index_start_frame);
        if let Some(active_effects) = self.effects_by_frame.get(local_frame_index as usize) {
            for effect_index in active_effects {
                let Some(effect) = self.effects.get(*effect_index) else {
                    continue;
                };
                if sample_seconds < effect.start_seconds
                    || sample_seconds >= effect.start_seconds + effect.duration_seconds
                {
                    continue;
                }
                let Some(effect_pixels) = sample.effect_pixels.get(*effect_index) else {
                    continue;
                };
                render_sampled_effect_target_colors(
                    effect,
                    effect_pixels,
                    &mut rendered,
                    sample_seconds,
                    &mut scratch.effect_vm[*effect_index],
                    &mut scratch.bind_cache,
                )?;
            }
        }

        Ok(rendered)
    }
}

impl<'a> EffectRasterPrepareBatch<'a> {
    pub fn prepare(
        project: &'a DawnProject,
        setup_id: &SetupId,
        sequence_id: &SequenceId,
    ) -> Result<Self, RenderError> {
        let setup = project
            .setups
            .get(setup_id)
            .ok_or_else(|| RenderError::MissingSetup {
                setup_id: setup_id.clone(),
            })?;
        let tree = project
            .element_trees
            .get(&setup.elements)
            .ok_or(RenderError::MissingElementTree)?;
        let sequence =
            project
                .sequences
                .get(sequence_id)
                .ok_or_else(|| RenderError::MissingSequence {
                    sequence_id: sequence_id.clone(),
                })?;
        validate_sequence(project, sequence).map_err(|error| RenderError::BadGraph {
            message: error.message,
        })?;
        prepare_timing(sequence)?;

        let (elements, groups) = prepare_elements(project, tree)?;
        let element_ids = elements
            .iter()
            .map(|element| element.id)
            .collect::<IndexSet<_>>();
        let frame_rate = sequence.frame_rate;
        let duration_seconds = sequence.duration.as_seconds_f64();
        let frame_count = frame_count(duration_seconds, frame_rate);

        Ok(Self {
            project,
            sequence,
            elements,
            element_ids,
            groups,
            frame_rate,
            frame_count,
            bind_cache: DslBindCache::default(),
            compiled_effects: HashMap::new(),
            target_cache: PrepareTargetCache::default(),
        })
    }

    pub fn prepare_effect(
        &mut self,
        effect_id: &EffectInstId,
    ) -> Result<PreparedEffectRasterRenderer, RenderError> {
        let effect = self
            .sequence
            .effects
            .iter()
            .find(|effect| effect.id == *effect_id)
            .ok_or(RenderError::MissingEffectInstance {
                effect_id: effect_id.clone(),
            })?;
        let mut effects = Vec::new();
        let mut generated_child_count = 0usize;
        let target = prepare_effect_inst(
            PrepareEffectContext {
                project: self.project,
                sequence: self.sequence,
                layer_id: SequenceLayerId(0),
                elements: &self.elements,
                element_ids: &self.element_ids,
                groups: &self.groups,
                effects: &mut effects,
                generated_child_count: &mut generated_child_count,
                bind_cache: &mut self.bind_cache,
                compiled_effects: &mut self.compiled_effects,
                target_cache: &mut self.target_cache,
            },
            effect,
        )?;

        let start_seconds = effect.start.as_seconds_f64();
        let duration_seconds = effect.duration.as_seconds_f64();
        let index_start_frame = (start_seconds * f64::from(self.frame_rate)).ceil().max(0.0) as u64;
        let index_end_frame = ((start_seconds + duration_seconds) * f64::from(self.frame_rate))
            .ceil()
            .max(index_start_frame as f64) as u64;
        let index_frame_count = index_end_frame.saturating_sub(index_start_frame).max(1);
        let effects_by_frame = build_effect_frame_index_for_window(
            &effects,
            index_start_frame,
            index_frame_count,
            self.frame_rate,
        );
        Ok(PreparedEffectRasterRenderer {
            frame_rate: self.frame_rate,
            frame_count: self.frame_count,
            index_start_frame,
            start_seconds,
            duration_seconds,
            target,
            effects,
            effects_by_frame,
        })
    }
}

struct PrepareEffectContext<'a> {
    project: &'a DawnProject,
    sequence: &'a Sequence,
    layer_id: SequenceLayerId,
    elements: &'a [PreparedElement],
    element_ids: &'a IndexSet<ElementNodeId>,
    groups: &'a IndexMap<ElementNodeId, Vec<ElementNodeId>>,
    effects: &'a mut Vec<PreparedEffect>,
    generated_child_count: &'a mut usize,
    bind_cache: &'a mut DslBindCache,
    compiled_effects: &'a mut HashMap<EffectDefinitionId, Arc<CompiledEffect>>,
    target_cache: &'a mut PrepareTargetCache,
}

fn prepare_effect_inst(
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

fn prepare_timing(sequence: &Sequence) -> Result<(), RenderError> {
    if sequence.frame_rate == 0 {
        return Err(RenderError::InvalidTiming {
            reason: "frame rate must be greater than zero".to_string(),
        });
    }
    let duration_seconds = sequence.duration.as_seconds_f64();
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        return Err(RenderError::InvalidTiming {
            reason: "duration must be positive and finite".to_string(),
        });
    }
    let prepared_frames = duration_seconds * f64::from(sequence.frame_rate);
    if !prepared_frames.is_finite() || prepared_frames.ceil() > MAX_SEQUENCE_FRAME_COUNT as f64 {
        return Err(RenderError::InvalidTiming {
            reason: format!(
                "sequence exceeds the prepared-frame budget of {MAX_SEQUENCE_FRAME_COUNT} frames"
            ),
        });
    }
    Ok(())
}

fn frame_count(duration_seconds: f64, frame_rate: u32) -> u64 {
    (duration_seconds * f64::from(frame_rate)).ceil() as u64
}

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

fn arc_key<T>(value: &Arc<T>) -> usize {
    Arc::as_ptr(value).cast::<()>() as usize
}

fn automation_for_effect(
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

fn automation_for_composition_node(
    sequence: &Sequence,
    node_id: &CompositionGraphNodeId,
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
                        AutomationTarget::CompositionNodeParam {
                            node_id: target_node_id,
                            ..
                        } if target_node_id == node_id
                    )
                })
                .map(move |binding| prepare_automation(clip, binding))
        })
        .collect()
}

fn prepare_automation(clip: &AutomationClip, binding: &AutomationBinding) -> PreparedAutomation {
    let mut clip = clip.clone();
    clip.curve
        .points
        .sort_by(|left, right| left.position.total_cmp(&right.position));
    PreparedAutomation {
        clip,
        binding: binding.clone(),
    }
}

fn apply_automation_params(
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

fn automation_param(binding: &AutomationBinding) -> &Identifier {
    match &binding.target {
        AutomationTarget::EffectParam { param, .. }
        | AutomationTarget::CompositionNodeParam { param, .. } => param,
    }
}

fn build_effect_frame_index(
    effects: &[PreparedEffect],
    frame_count: u64,
    frame_rate: u32,
) -> Vec<Vec<usize>> {
    build_effect_frame_index_for_window(effects, 0, frame_count, frame_rate)
}

fn build_effect_frame_index_for_window(
    effects: &[PreparedEffect],
    start_frame: u64,
    frame_count: u64,
    frame_rate: u32,
) -> Vec<Vec<usize>> {
    let end_frame_limit = start_frame.saturating_add(frame_count);
    let effect_frame_range = |effect: &PreparedEffect| {
        let effect_start_frame = (effect.start_seconds * f64::from(frame_rate))
            .floor()
            .max(0.0) as u64;
        let effect_end_frame = ((effect.start_seconds + effect.duration_seconds)
            * f64::from(frame_rate))
        .ceil() as u64;
        effect_start_frame.max(start_frame)..effect_end_frame.min(end_frame_limit)
    };
    let mut active_counts = vec![0usize; frame_count as usize];
    for effect in effects {
        for frame in effect_frame_range(effect) {
            active_counts[frame.saturating_sub(start_frame) as usize] += 1;
        }
    }
    let mut index = active_counts
        .into_iter()
        .map(Vec::with_capacity)
        .collect::<Vec<_>>();
    for (effect_index, effect) in effects.iter().enumerate() {
        for frame in effect_frame_range(effect) {
            index[frame.saturating_sub(start_frame) as usize].push(effect_index);
        }
    }
    index
}

fn pixel_fraction(index: usize, count: usize) -> f64 {
    if count <= 1 {
        0.0
    } else {
        index as f64 / (count - 1) as f64
    }
}

fn float_param(params: &IndexMap<Identifier, Value>, name: &str) -> Result<f64, RenderError> {
    let name = Identifier::new(name.to_string()).map_err(|_| RenderError::BadGraph {
        message: format!("invalid operator parameter name `{name}`"),
    })?;
    params
        .get(&name)
        .and_then(|value| match value {
            Value::Float(value) => Some(*value),
            _ => None,
        })
        .ok_or_else(|| RenderError::BadGraph {
            message: format!("missing or invalid operator parameter `{}`", name.as_str()),
        })
}

fn int_param(params: &IndexMap<Identifier, Value>, name: &str) -> Result<i64, RenderError> {
    let name = Identifier::new(name.to_string()).map_err(|_| RenderError::BadGraph {
        message: format!("invalid operator parameter name `{name}`"),
    })?;
    params
        .get(&name)
        .and_then(|value| match value {
            Value::Int(value) => Some(*value),
            _ => None,
        })
        .ok_or_else(|| RenderError::BadGraph {
            message: format!("missing or invalid operator parameter `{}`", name.as_str()),
        })
}

fn layer_cache_history_micros(graph: &PreparedCompositionGraph) -> Result<i64, RenderError> {
    let mut history_seconds = 0.0_f64;
    for node in &graph.nodes {
        let PreparedGraphNodeKind::Operator {
            definition,
            params,
            automation,
            ..
        } = &node.kind
        else {
            continue;
        };
        if !matches!(
            definition.implementation,
            OperatorImplementation::Native(BuiltinOperator::Echo)
        ) {
            continue;
        }
        let mut delay = float_param(params, "seconds")?.max(0.0);
        let mut repeats = int_param(params, "repeats")?.clamp(1, 32);
        for automation in automation {
            match (
                automation_param(&automation.binding).as_str(),
                &automation.binding.mapping,
            ) {
                ("seconds", AutomationMapping::Float { min, max }) => {
                    delay = delay.max(*min).max(*max).max(0.0);
                }
                ("repeats", AutomationMapping::Int { min, max }) => {
                    repeats = repeats.max(*min).max(*max).clamp(1, 32);
                }
                _ => {}
            }
        }
        history_seconds = history_seconds.max(delay * repeats as f64);
    }
    Ok(if !history_seconds.is_finite() || history_seconds <= 0.0 {
        0
    } else {
        (history_seconds * 1_000_000.0).ceil() as i64
    })
}

#[derive(Clone, Debug)]
struct PreparedEffect {
    layer_id: SequenceLayerId,
    start_seconds: f64,
    duration_seconds: f64,
    target: Arc<Vec<PreparedTargetPixel>>,
    sample_groups: Option<Arc<Vec<PreparedSampleContextGroup>>>,
    name: String,
    implementation: PreparedEffectImplementation,
    params: IndexMap<Identifier, Value>,
    automation: Vec<PreparedAutomation>,
}

#[derive(Clone, Debug)]
enum PreparedEffectImplementation {
    Dsl {
        definition: Arc<CompiledEffect>,
        bound_params: BoundParams,
    },
    Native {
        builtin: Option<dawn_language::effect::BuiltinEffect>,
        sample: NativeSample,
    },
}

#[derive(Clone, Debug)]
struct PreparedLayer {
    id: SequenceLayerId,
    enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct GraphRenderCacheKey {
    node_index: usize,
    frame_key: i64,
}

#[derive(Clone, Debug)]
struct PreparedAutomation {
    clip: AutomationClip,
    binding: AutomationBinding,
}

#[derive(Default)]
struct PrepareTargetCache {
    target: PreparedTargetCache,
    generated_targets: HashMap<usize, GeneratedTargetCacheEntry>,
    generator_context_targets: HashMap<usize, GeneratorContextTargetCacheEntry>,
    sample_groups: HashMap<usize, PreparedSampleGroupCacheEntry>,
    sample_group_eligibility: HashMap<usize, bool>,
}

pub(crate) struct GeneratedTargetCacheEntry {
    pub(crate) source: Arc<TargetItemValue>,
    pub(crate) pixels: Arc<Vec<PreparedTargetPixel>>,
}

pub(crate) struct GeneratorContextTargetCacheEntry {
    pub(crate) source: Arc<Vec<PreparedTargetPixel>>,
    pub(crate) target: Arc<TargetValue>,
}

#[cfg(test)]
mod tests;
