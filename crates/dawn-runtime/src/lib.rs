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
mod show;
pub use show::*;

use dawn_language::dsl::{
    BoundParams, CompiledEffect, DslBindCache, DslVmScratch, EffectKind, GeneratedEffect,
    GeneratedEffectRef, GeneratorContext, Identifier, OperatorRunContext, ParamDecl, RunContext,
    RuntimeError, SignalSampler, TargetItemValue, TargetPixelValue, TargetValue, Type, Value,
};
use dawn_language::effect::{
    CurveSource, EffectDefinitionId, EffectImplementation, EffectInstId, EffectParamValue,
    EffectRef, EffectScope, GradientSource,
};
use dawn_language::element::{ElementCellRange, ElementNodeId, ElementSelection};
use dawn_language::identity::SourceIdentity;
use dawn_language::model::DawnProject;
use dawn_language::native_effect::{self, BoundNativeEffect, NativeGeneratedEffect, NativeSample};
use dawn_language::operator::{
    BuiltinOperator, OperatorDefinition, OperatorImplementation, validate_composition_graph,
};
use dawn_language::sequence::{
    AutomationBinding, AutomationClip, AutomationMapping, AutomationTarget, AutomationValue,
    CompositionGraphNodeId, CompositionGraphNodeKind, GraphPortId, MarkCollectionKey, Sequence,
    SequenceCompositionGraph, SequenceId, SequenceLayerId, automation_value_at,
};
use dawn_language::setup::SetupId;
use dawn_language::validation::{MAX_SEQUENCE_FRAME_COUNT, validate_sequence};
use dawn_language::values::{Color, DawnTime, Marks};
use indexmap::{IndexMap, IndexSet};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use color::{
    add_color, black, color_param, compose_max, intensity, invert_color, max_color, multiply_color,
    scale_color,
};
use elements::{PreparedElement, element_cell_offsets, prepare_elements};

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
        context.target_cache,
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

struct PrepareGraphContext<'a> {
    project: &'a DawnProject,
    sequence: &'a Sequence,
    elements: &'a [PreparedElement],
    layers: &'a [PreparedLayer],
}

fn prepare_composition_graph(
    context: PrepareGraphContext<'_>,
    graph: &SequenceCompositionGraph,
) -> Result<PreparedCompositionGraph, RenderError> {
    let full_target = Arc::new(full_rig_target_pixels(context.elements)?);
    validate_composition_graph(graph, &context.project.definitions.operators).map_err(|error| {
        RenderError::BadGraph {
            message: error.message,
        }
    })?;
    validate_composition_graph_layers(context.sequence, graph)?;
    let node_ids = composition_graph_node_ids(graph)?;
    let node_indexes = node_ids
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, node_id)| (node_id, index))
        .collect::<IndexMap<_, _>>();
    let node_order = topological_composition_graph_order(&node_ids, &node_indexes, graph)?;
    let mut incoming = vec![Vec::<(GraphPortId, usize)>::new(); node_ids.len()];
    for edge in &graph.edges {
        let from = node_index(&node_indexes, &edge.from)?;
        let to = node_index(&node_indexes, &edge.to)?;
        incoming[to].push((edge.to_port.clone(), from));
    }

    let mut prepared_nodes = Vec::<PreparedGraphNode>::new();
    let mut prepared_index_by_node = vec![usize::MAX; node_ids.len()];
    for node_index in &node_order {
        let node_id = &node_ids[*node_index];
        let node = graph_node(graph, node_id)?;
        let prepared = match &node.kind {
            CompositionGraphNodeKind::Layer { layer_id } => {
                let layer_index = context
                    .layers
                    .iter()
                    .position(|layer| layer.id == *layer_id)
                    .ok_or_else(|| RenderError::BadGraph {
                        message: format!(
                            "composition graph references missing layer {}",
                            layer_id.0
                        ),
                    })?;
                PreparedGraphNode {
                    target: Arc::clone(&full_target),
                    kind: PreparedGraphNodeKind::Layer { layer_index },
                }
            }
            CompositionGraphNodeKind::Operator(operator_node) => {
                let definition = context
                    .project
                    .definitions
                    .operators
                    .resolve(&operator_node.operator)
                    .ok_or_else(|| RenderError::BadGraph {
                        message: "missing operator definition".to_string(),
                    })?
                    .clone();
                let inputs = definition
                    .inputs
                    .iter()
                    .map(|port| {
                        incoming[*node_index]
                            .iter()
                            .find_map(|(input_port, node)| {
                                (input_port.0 == port.source_name).then_some(*node)
                            })
                            .ok_or_else(|| RenderError::BadGraph {
                                message: format!(
                                    "composition graph input port `{}` is not connected",
                                    port.source_name
                                ),
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .map(|input| {
                        let prepared_index = prepared_index_by_node[input];
                        (prepared_index != usize::MAX)
                            .then_some(prepared_index)
                            .ok_or_else(|| RenderError::BadGraph {
                                message: "composition graph order did not prepare an input first"
                                    .to_string(),
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let params = prepare_operator_params(
                    context.project,
                    context.sequence,
                    &definition,
                    &operator_node.params,
                    EffectParamTiming {
                        start_seconds: 0.0,
                        duration_seconds: context.sequence.duration.as_seconds_f64(),
                    },
                )?;
                let automation = automation_for_composition_node(context.sequence, &node.id);
                let bound_params = match (&definition.implementation, automation.is_empty()) {
                    (OperatorImplementation::Dsl(compiled), true) => {
                        Some(compiled.bind_params(&params)?)
                    }
                    _ => None,
                };
                PreparedGraphNode {
                    target: Arc::clone(&full_target),
                    kind: PreparedGraphNodeKind::Operator {
                        definition: Box::new(definition.clone()),
                        inputs,
                        params,
                        automation,
                        bound_params,
                    },
                }
            }
            CompositionGraphNodeKind::Output => {
                let inputs = incoming[*node_index]
                    .iter()
                    .map(|(_, input)| {
                        let prepared_index = prepared_index_by_node[*input];
                        (prepared_index != usize::MAX)
                            .then_some(prepared_index)
                            .ok_or_else(|| RenderError::BadGraph {
                                message:
                                    "composition graph order did not prepare output input first"
                                        .to_string(),
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                PreparedGraphNode {
                    target: Arc::clone(&full_target),
                    kind: PreparedGraphNodeKind::Output { inputs },
                }
            }
        };
        prepared_index_by_node[*node_index] = prepared_nodes.len();
        prepared_nodes.push(prepared);
    }

    let output_candidates = node_order
        .iter()
        .filter(|index| {
            matches!(
                graph_node(graph, &node_ids[**index]).map(|node| &node.kind),
                Ok(CompositionGraphNodeKind::Output)
            )
        })
        .copied()
        .collect::<Vec<_>>();
    let [output_source_index] = output_candidates.as_slice() else {
        return Err(RenderError::BadGraph {
            message: "composition graph must have exactly one output node".to_string(),
        });
    };
    let output_index = prepared_index_by_node[*output_source_index];
    if output_index == usize::MAX {
        return Err(RenderError::BadGraph {
            message: "composition graph output node is not in render order".to_string(),
        });
    }

    Ok(PreparedCompositionGraph {
        output_index,
        nodes: prepared_nodes,
    })
}

fn composition_graph_node_ids(
    graph: &SequenceCompositionGraph,
) -> Result<Vec<CompositionGraphNodeId>, RenderError> {
    let mut ids = IndexSet::new();
    for node in &graph.nodes {
        if !ids.insert(node.id.clone()) {
            return Err(RenderError::BadGraph {
                message: format!("duplicate composition graph node {}", node.id.0),
            });
        }
    }
    Ok(ids.into_iter().collect())
}

fn validate_composition_graph_layers(
    sequence: &Sequence,
    graph: &SequenceCompositionGraph,
) -> Result<(), RenderError> {
    let mut graph_layer_ids = IndexSet::new();
    for node in &graph.nodes {
        let CompositionGraphNodeKind::Layer { layer_id } = &node.kind else {
            continue;
        };
        if !sequence.layers.iter().any(|layer| layer.id == *layer_id) {
            return Err(RenderError::BadGraph {
                message: format!(
                    "composition graph layer node references missing layer {}",
                    layer_id.0
                ),
            });
        }
        if !graph_layer_ids.insert(layer_id.clone()) {
            return Err(RenderError::BadGraph {
                message: format!(
                    "composition graph has duplicate layer node for layer {}",
                    layer_id.0
                ),
            });
        }
    }
    for layer in &sequence.layers {
        if !graph_layer_ids.contains(&layer.id) {
            return Err(RenderError::BadGraph {
                message: format!(
                    "composition graph is missing layer node for layer {}",
                    layer.id.0
                ),
            });
        }
    }
    Ok(())
}

fn node_index(
    indexes: &IndexMap<CompositionGraphNodeId, usize>,
    node_id: &CompositionGraphNodeId,
) -> Result<usize, RenderError> {
    indexes
        .get(node_id)
        .copied()
        .ok_or_else(|| RenderError::BadGraph {
            message: format!(
                "edge references missing composition graph node {}",
                node_id.0
            ),
        })
}

fn graph_node<'a>(
    graph: &'a SequenceCompositionGraph,
    id: &CompositionGraphNodeId,
) -> Result<&'a dawn_language::sequence::CompositionGraphNode, RenderError> {
    graph
        .nodes
        .iter()
        .find(|node| node.id == *id)
        .ok_or_else(|| RenderError::BadGraph {
            message: format!("missing composition graph node {}", id.0),
        })
}

fn topological_composition_graph_order(
    node_ids: &[CompositionGraphNodeId],
    node_indexes: &IndexMap<CompositionGraphNodeId, usize>,
    graph: &SequenceCompositionGraph,
) -> Result<Vec<usize>, RenderError> {
    let mut indegree = vec![0usize; node_ids.len()];
    let mut outgoing = vec![Vec::<usize>::new(); node_ids.len()];
    for edge in &graph.edges {
        let from = node_index(node_indexes, &edge.from)?;
        let to = node_index(node_indexes, &edge.to)?;
        outgoing[from].push(to);
        indegree[to] += 1;
    }
    let mut ready = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count == 0).then_some(index))
        .collect::<Vec<_>>();
    let mut order = Vec::with_capacity(node_ids.len());
    while let Some(index) = ready.pop() {
        order.push(index);
        for next in &outgoing[index] {
            indegree[*next] = indegree[*next].saturating_sub(1);
            if indegree[*next] == 0 {
                ready.push(*next);
            }
        }
    }
    if order.len() != node_ids.len() {
        return Err(RenderError::BadGraph {
            message: "composition graph contains a cycle".to_string(),
        });
    }
    Ok(order)
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

fn full_rig_target_pixels(
    elements: &[PreparedElement],
) -> Result<Vec<PreparedTargetPixel>, RenderError> {
    let mut pixels = Vec::new();
    for (element_index, element) in elements.iter().enumerate() {
        let pixel_count = element.pixel_count;
        for element_cell_index in 0..element.pixel_count {
            let pixel_index = element_cell_index;
            let pixel_fraction = if element.pixel_count <= 1 {
                0.0
            } else {
                element_cell_index as f64 / (element.pixel_count - 1) as f64
            };
            pixels.push(PreparedTargetPixel {
                element_index,
                element_cell_index,
                pixel_index,
                pixel_count,
                pixel_fraction,
            });
        }
    }
    Ok(pixels)
}

fn prepare_target(
    target: &ElementSelection,
    element_ids: &IndexSet<ElementNodeId>,
    groups: &IndexMap<ElementNodeId, Vec<ElementNodeId>>,
) -> Result<PreparedTargetSelection, RenderError> {
    if let Some(members) = groups.get(&target.node) {
        if target.cells.is_some() {
            return Err(RenderError::BadTarget);
        }
        return Ok(PreparedTargetSelection {
            elements: members.clone(),
            cells: None,
        });
    }
    if !element_ids.contains(&target.node) {
        return Err(RenderError::MissingElement {
            element_id: target.node,
        });
    }
    Ok(PreparedTargetSelection {
        elements: vec![target.node],
        cells: target.cells,
    })
}

fn prepare_target_indexes(
    target: &[ElementNodeId],
    elements: &[PreparedElement],
) -> Result<Vec<usize>, RenderError> {
    target
        .iter()
        .map(|id| {
            elements
                .iter()
                .position(|element| &element.id == id)
                .ok_or(RenderError::MissingElement { element_id: *id })
        })
        .collect()
}

fn prepare_target_pixels(
    target: &PreparedTargetSelection,
    elements: &[PreparedElement],
    scope: &EffectScope,
) -> Result<Vec<PreparedTargetPixel>, RenderError> {
    let indexes = prepare_target_indexes(&target.elements, elements)?;
    if indexes.iter().any(|index| !elements[*index].color_enabled) {
        return Err(RenderError::BadTarget);
    }
    let total_target_pixels = indexes
        .iter()
        .map(|index| {
            selected_cell_range(elements[*index].pixel_count, target.cells).map(|range| range.len())
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .sum::<usize>();
    let mut pixels = Vec::with_capacity(total_target_pixels);
    let mut whole_index = 0usize;
    for element_index in indexes {
        let element_cell_count = elements[element_index].pixel_count;
        let cells = selected_cell_range(element_cell_count, target.cells)?;
        for (selected_index, element_cell_index) in cells.enumerate() {
            let (pixel_index, pixel_count) = match scope {
                EffectScope::PerFixture => (
                    selected_index,
                    target
                        .cells
                        .map_or(element_cell_count, |range| range.count as usize),
                ),
                EffectScope::WholeTarget => (whole_index, total_target_pixels),
            };
            pixels.push(PreparedTargetPixel {
                element_index,
                element_cell_index,
                pixel_index,
                pixel_count,
                pixel_fraction: pixel_fraction(pixel_index, pixel_count),
            });
            whole_index += 1;
        }
    }
    Ok(pixels)
}

fn selected_cell_range(
    pixel_count: usize,
    range: Option<ElementCellRange>,
) -> Result<std::ops::Range<usize>, RenderError> {
    let Some(range) = range else {
        return Ok(0..pixel_count);
    };
    let start = range.start as usize;
    let end = start
        .checked_add(range.count as usize)
        .ok_or(RenderError::BadTarget)?;
    if range.count == 0 || end > pixel_count {
        return Err(RenderError::BadTarget);
    }
    Ok(start..end)
}

fn prepare_target_pixels_cached(
    cache: &mut PrepareTargetCache,
    target: &PreparedTargetSelection,
    elements: &[PreparedElement],
    scope: &EffectScope,
) -> Result<Arc<Vec<PreparedTargetPixel>>, RenderError> {
    let key = PreparedTargetCacheKey {
        target: target.clone(),
        scope: PreparedTargetScopeKey::from(scope),
    };
    if let Some(pixels) = cache.prepared_targets.get(&key) {
        return Ok(Arc::clone(pixels));
    }
    let pixels = Arc::new(prepare_target_pixels(target, elements, scope)?);
    cache.prepared_targets.insert(key, Arc::clone(&pixels));
    Ok(pixels)
}

fn generator_expansion_targets(
    target: &Arc<Vec<PreparedTargetPixel>>,
    scope: &EffectScope,
) -> Vec<Arc<Vec<PreparedTargetPixel>>> {
    match scope {
        EffectScope::WholeTarget => vec![Arc::clone(target)],
        EffectScope::PerFixture => {
            let mut targets = Vec::new();
            let mut element_pixels = Vec::new();
            let mut current_element_index = None;

            for pixel in target.iter() {
                if current_element_index.is_some_and(|index| index != pixel.element_index) {
                    targets.push(Arc::new(element_pixels));
                    element_pixels = Vec::new();
                }
                current_element_index = Some(pixel.element_index);
                element_pixels.push(pixel.clone());
            }

            if !element_pixels.is_empty() {
                targets.push(Arc::new(element_pixels));
            }

            targets
        }
    }
}

fn prepare_params(
    project: &DawnProject,
    sequence: &Sequence,
    overrides: &IndexMap<Identifier, EffectParamValue>,
    timing: EffectParamTiming,
) -> Result<IndexMap<Identifier, Value>, RenderError> {
    overrides
        .iter()
        .map(|(key, value)| {
            Ok((
                key.clone(),
                prepare_param_value(project, sequence, value, timing)?,
            ))
        })
        .collect()
}

fn prepare_operator_params(
    project: &DawnProject,
    sequence: &Sequence,
    definition: &OperatorDefinition,
    overrides: &IndexMap<Identifier, EffectParamValue>,
    timing: EffectParamTiming,
) -> Result<IndexMap<Identifier, Value>, RenderError> {
    let mut params = definition
        .params
        .iter()
        .filter_map(|param| {
            param
                .default
                .as_ref()
                .map(|default| (param.name.clone(), default.clone()))
        })
        .collect::<IndexMap<_, _>>();
    for (name, value) in prepare_params(project, sequence, overrides, timing)? {
        params.insert(name, value);
    }
    Ok(params)
}

#[derive(Clone, Copy)]
struct EffectParamTiming {
    start_seconds: f64,
    duration_seconds: f64,
}

fn prepare_param_value(
    project: &DawnProject,
    sequence: &Sequence,
    value: &EffectParamValue,
    timing: EffectParamTiming,
) -> Result<Value, RenderError> {
    match value {
        EffectParamValue::Int(value) => Ok(Value::Int(*value)),
        EffectParamValue::Float(value) => Ok(Value::Float(*value)),
        EffectParamValue::Bool(value) => Ok(Value::Bool(*value)),
        EffectParamValue::Color(value) => Ok(Value::Color(*value)),
        EffectParamValue::Enum(value) => Ok(Value::Enum(value.clone())),
        EffectParamValue::Marks(key) => {
            let collection = sequence
                .mark_collections
                .iter()
                .find(|collection| collection.key == *key)
                .ok_or_else(|| RenderError::MissingMarkCollection { key: key.clone() })?;
            let end_seconds = timing.start_seconds + timing.duration_seconds;
            Ok(Value::Marks(Arc::new(Marks {
                marks: collection
                    .marks
                    .iter()
                    .filter_map(|mark| {
                        let seconds = mark.as_seconds_f64();
                        (seconds >= timing.start_seconds && seconds < end_seconds)
                            .then(|| DawnTime::from_seconds_f64(seconds - timing.start_seconds))
                    })
                    .collect(),
            })))
        }
        EffectParamValue::Curve(source) => Ok(Value::Curve(Arc::new(match source {
            CurveSource::Inline(curve) => curve.clone(),
            CurveSource::Reference(id) => project
                .definitions
                .curves
                .get(id)
                .ok_or(RenderError::MissingCurve)?
                .curve
                .clone(),
        }))),
        EffectParamValue::Gradient(source) => Ok(Value::Gradient(Arc::new(match source {
            GradientSource::Inline(gradient) => gradient.clone(),
            GradientSource::Reference(id) => project
                .definitions
                .gradients
                .get(id)
                .ok_or(RenderError::MissingGradient)?
                .gradient
                .clone(),
        }))),
        EffectParamValue::Array(values) => values
            .iter()
            .map(|value| prepare_param_value(project, sequence, value, timing))
            .collect::<Result<Vec<_>, _>>()
            .map(Arc::new)
            .map(Value::Array),
    }
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

const MAX_GENERATOR_DEPTH: usize = 4;
const MAX_GENERATED_CHILDREN: usize = MAX_GENERATED_EFFECTS;

#[derive(Clone, Debug)]
struct GeneratorExpansion {
    start_seconds: f64,
    duration_seconds: f64,
    target: Arc<Vec<PreparedTargetPixel>>,
    depth: usize,
    definition_source: SourceIdentity,
}

struct GeneratorPrepareContext<'a> {
    project: &'a DawnProject,
    layer_id: SequenceLayerId,
    elements: &'a [PreparedElement],
    effects: &'a mut Vec<PreparedEffect>,
    generated_child_count: &'a mut usize,
    bind_cache: &'a mut DslBindCache,
    compiled_effects: &'a mut HashMap<EffectDefinitionId, Arc<CompiledEffect>>,
    target_cache: &'a mut PrepareTargetCache,
}

fn expand_generator(
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
    let mut scratch = DslVmScratch::default();
    let target = generator_context_target(context.target_cache, &expansion.target);
    let generated = definition.generate_bound(
        params,
        &GeneratorContext {
            duration: expansion.duration_seconds,
            target,
        },
        &mut scratch,
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
            expansion.start_seconds,
            expansion.depth,
            &expansion.definition_source,
            child,
        )?;
    }
    Ok(())
}

fn expand_native_generator(
    context: &mut GeneratorPrepareContext<'_>,
    definition: &BoundNativeEffect,
    start_seconds: f64,
    duration_seconds: f64,
    target: Arc<Vec<PreparedTargetPixel>>,
    depth: usize,
) -> Result<(), RenderError> {
    if depth >= MAX_GENERATOR_DEPTH {
        return Err(RenderError::GeneratorPrepare {
            message: "generator depth limit exceeded".to_string(),
        });
    }
    let generated = definition.generate(&GeneratorContext {
        duration: duration_seconds,
        target: generator_context_target(context.target_cache, &target),
    })?;
    for child in generated {
        if *context.generated_child_count >= MAX_GENERATED_CHILDREN {
            return Err(RenderError::GeneratorPrepare {
                message: format!("generated child limit exceeded ({MAX_GENERATED_CHILDREN})"),
            });
        }
        *context.generated_child_count += 1;
        prepare_native_child(context, start_seconds, child)?;
    }
    Ok(())
}

fn prepare_native_child(
    context: &mut GeneratorPrepareContext<'_>,
    parent_start_seconds: f64,
    child: NativeGeneratedEffect,
) -> Result<(), RenderError> {
    if !child.duration_seconds.is_finite() || child.duration_seconds <= 0.0 {
        return Err(RenderError::InvalidTiming {
            reason: "generated effect duration must be positive and finite".to_string(),
        });
    }
    let target = prepared_pixels_from_generated_target_cached(
        context.target_cache,
        context.elements,
        child.target,
    )?;
    let name = child.sample.display_name().to_string();
    context.effects.push(PreparedEffect {
        layer_id: context.layer_id.clone(),
        start_seconds: parent_start_seconds + child.start_seconds,
        duration_seconds: child.duration_seconds,
        sample_groups: prepare_sample_context_groups_cached(context.target_cache, &target),
        target,
        name,
        implementation: PreparedEffectImplementation::Native {
            builtin: None,
            sample: child.sample,
        },
        params: IndexMap::new(),
        automation: Vec::new(),
    });
    Ok(())
}

fn prepare_generated_child(
    context: &mut GeneratorPrepareContext<'_>,
    parent_start_seconds: f64,
    parent_depth: usize,
    definition_source: &SourceIdentity,
    child: GeneratedEffect,
) -> Result<(), RenderError> {
    let effect_ref = match &child.definition {
        GeneratedEffectRef::Local(name) => {
            EffectRef::Custom(EffectDefinitionId(SourceIdentity::from_document(
                definition_source.document_id().clone(),
                name.as_str().to_string(),
            )))
        }
        GeneratedEffectRef::Builtin(builtin) => EffectRef::Builtin(*builtin),
    };
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
                    builtin.definition().source_name
                ),
            },
        })?;
    validate_generated_params(&definition.params, &child.params)?;
    let duration_seconds = child.duration_seconds;
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        return Err(RenderError::InvalidTiming {
            reason: "generated effect duration must be positive and finite".to_string(),
        });
    }
    let target = prepared_pixels_from_generated_target_cached(
        context.target_cache,
        context.elements,
        child.target,
    )?;
    let start_seconds = parent_start_seconds + child.start_seconds;
    match &definition.implementation {
        EffectImplementation::Dsl(definition_compiled) => {
            let EffectRef::Custom(effect_id) = &effect_ref else {
                unreachable!("DSL effects are custom")
            };
            let compiled = context
                .compiled_effects
                .entry(effect_id.clone())
                .or_insert_with(|| Arc::new(definition_compiled.clone()))
                .clone();
            let bound_params = compiled.bind_params_cached(&child.params, context.bind_cache)?;
            match definition.kind {
                EffectKind::Sample => {
                    context.effects.push(PreparedEffect {
                        layer_id: context.layer_id.clone(),
                        start_seconds,
                        duration_seconds,
                        sample_groups: prepare_sample_groups_for_effect(
                            context.target_cache,
                            &compiled,
                            &target,
                        ),
                        target,
                        name: definition.display_name.clone(),
                        implementation: PreparedEffectImplementation::Dsl {
                            definition: compiled,
                            bound_params,
                        },
                        params: child.params,
                        automation: Vec::new(),
                    });
                    Ok(())
                }
                EffectKind::Generator => expand_generator(
                    context,
                    &compiled,
                    &bound_params,
                    GeneratorExpansion {
                        start_seconds,
                        duration_seconds,
                        target,
                        depth: parent_depth + 1,
                        definition_source: effect_id.0.clone(),
                    },
                ),
            }
        }
        EffectImplementation::Native(builtin) => {
            let bound = native_effect::bind(*builtin, &child.params)?;
            match definition.kind {
                EffectKind::Sample => {
                    let BoundNativeEffect::Sample(sample) = bound else {
                        return Err(RenderError::GeneratorPrepare {
                            message: "native sample effect bound as generator".to_string(),
                        });
                    };
                    let implementation = PreparedEffectImplementation::Native {
                        builtin: Some(*builtin),
                        sample,
                    };
                    context.effects.push(PreparedEffect {
                        layer_id: context.layer_id.clone(),
                        start_seconds,
                        duration_seconds,
                        sample_groups: prepare_sample_groups_for_implementation(
                            context.target_cache,
                            &implementation,
                            &target,
                        ),
                        target,
                        name: definition.display_name.clone(),
                        implementation,
                        params: child.params,
                        automation: Vec::new(),
                    });
                    Ok(())
                }
                EffectKind::Generator => expand_native_generator(
                    context,
                    &bound,
                    start_seconds,
                    duration_seconds,
                    target,
                    parent_depth + 1,
                ),
            }
        }
    }
}

fn validate_generated_params(
    declarations: &[ParamDecl],
    params: &IndexMap<Identifier, Value>,
) -> Result<(), RenderError> {
    for key in params.keys() {
        if !declarations.iter().any(|param| &param.name == key) {
            return Err(RenderError::GeneratorPrepare {
                message: format!("unknown generated param `{}`", key.as_str()),
            });
        }
    }
    for param in declarations {
        let Some(value) = params.get(&param.name) else {
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

fn target_groups_from_pixels(pixels: &[PreparedTargetPixel]) -> Vec<Arc<TargetItemValue>> {
    vec![Arc::new(TargetItemValue {
        pixels: Arc::new(pixels.iter().map(target_pixel_value).collect()),
    })]
}

fn generator_context_target(
    cache: &mut PrepareTargetCache,
    prepared_target: &Arc<Vec<PreparedTargetPixel>>,
) -> Arc<TargetValue> {
    let key = arc_key(prepared_target);
    if let Some(entry) = cache.generator_context_targets.get(&key)
        && Arc::ptr_eq(&entry.source, prepared_target)
    {
        return Arc::clone(&entry.target);
    }
    let target = Arc::new(TargetValue {
        groups: target_groups_from_pixels(prepared_target),
    });
    cache.generator_context_targets.insert(
        key,
        GeneratorContextTargetCacheEntry {
            source: Arc::clone(prepared_target),
            target: Arc::clone(&target),
        },
    );
    target
}

fn target_pixel_value(pixel: &PreparedTargetPixel) -> TargetPixelValue {
    TargetPixelValue {
        element_index: pixel.element_index as i64,
        element_cell_index: pixel.element_cell_index as i64,
        pixel_index: pixel.pixel_index as i64,
        pixel_count: pixel.pixel_count as i64,
        pixel_fraction: pixel.pixel_fraction,
    }
}

fn prepared_pixels_from_generated_target(
    elements: &[PreparedElement],
    target: Arc<TargetItemValue>,
) -> Result<Vec<PreparedTargetPixel>, RenderError> {
    target
        .pixels
        .iter()
        .copied()
        .map(|pixel| {
            let element_index = usize::try_from(pixel.element_index).map_err(|_| {
                RenderError::GeneratorPrepare {
                    message: "generated target element index cannot be negative".to_string(),
                }
            })?;
            let element_cell_index = usize::try_from(pixel.element_cell_index).map_err(|_| {
                RenderError::GeneratorPrepare {
                    message: "generated target pixel index cannot be negative".to_string(),
                }
            })?;
            let element =
                elements
                    .get(element_index)
                    .ok_or_else(|| RenderError::GeneratorPrepare {
                        message: "generated target element index is out of bounds".to_string(),
                    })?;
            if element_cell_index >= element.pixel_count {
                return Err(RenderError::GeneratorPrepare {
                    message: "generated target pixel index is out of bounds".to_string(),
                });
            }
            let pixel_index =
                usize::try_from(pixel.pixel_index).map_err(|_| RenderError::GeneratorPrepare {
                    message: "generated target pixel context index cannot be negative".to_string(),
                })?;
            let pixel_count =
                usize::try_from(pixel.pixel_count).map_err(|_| RenderError::GeneratorPrepare {
                    message: "generated target pixel context count cannot be negative".to_string(),
                })?;
            Ok(PreparedTargetPixel {
                element_index,
                element_cell_index,
                pixel_index,
                pixel_count,
                pixel_fraction: pixel.pixel_fraction,
            })
        })
        .collect()
}

fn prepared_pixels_from_generated_target_cached(
    cache: &mut PrepareTargetCache,
    elements: &[PreparedElement],
    target: Arc<TargetItemValue>,
) -> Result<Arc<Vec<PreparedTargetPixel>>, RenderError> {
    let key = arc_key(&target);
    if let Some(entry) = cache.generated_targets.get(&key)
        && Arc::ptr_eq(&entry.source, &target)
    {
        return Ok(Arc::clone(&entry.pixels));
    }
    let pixels = Arc::new(prepared_pixels_from_generated_target(
        elements,
        Arc::clone(&target),
    )?);
    cache.generated_targets.insert(
        key,
        GeneratedTargetCacheEntry {
            source: target,
            pixels: Arc::clone(&pixels),
        },
    );
    Ok(pixels)
}

fn arc_key<T>(value: &Arc<T>) -> usize {
    Arc::as_ptr(value).cast::<()>() as usize
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

fn render_effect(
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

fn render_composition_graph(
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

fn render_sampled_effect_target_colors(
    effect: &PreparedEffect,
    effect_pixels: &PreparedSampledEffectPixels,
    rendered: &mut [Color],
    sample_seconds: f64,
    scratch: &mut DslVmScratch,
    bind_cache: &mut DslBindCache,
) -> Result<(), RenderError> {
    let local_seconds = sample_seconds - effect.start_seconds;
    let progress = (local_seconds / effect.duration_seconds).clamp(0.0, 1.0);
    let automated = effect_implementation_at(effect, sample_seconds, bind_cache)?;
    let implementation = automated.as_ref().unwrap_or(&effect.implementation);
    match implementation {
        PreparedEffectImplementation::Dsl {
            definition,
            bound_params,
        } => render_sampled_effect_pixels(effect_pixels, rendered, |sample_context| {
            definition.sample_bound(
                bound_params,
                &run_context(effect, sample_context, progress, local_seconds),
                scratch,
            )
        }),
        PreparedEffectImplementation::Native { sample, .. } => {
            render_sampled_effect_pixels(effect_pixels, rendered, |sample_context| {
                sample.sample(&run_context(
                    effect,
                    sample_context,
                    progress,
                    local_seconds,
                ))
            })
        }
    }
}

#[inline(always)]
fn render_sampled_effect_pixels(
    effect_pixels: &PreparedSampledEffectPixels,
    rendered: &mut [Color],
    mut sample: impl FnMut(PreparedSampleContext) -> Result<Color, RuntimeError>,
) -> Result<(), RenderError> {
    if let Some(groups) = &effect_pixels.groups {
        for group in groups {
            let color = sample(group.context)?;
            for row in &group.rows {
                if let Some(target) = rendered.get_mut(*row) {
                    compose_max(target, color);
                }
            }
        }
        return Ok(());
    }
    for sampled in &effect_pixels.pixels {
        let pixel = &sampled.pixel;
        let color = sample(PreparedSampleContext {
            pixel_index: pixel.pixel_index,
            pixel_count: pixel.pixel_count,
            pixel_fraction: pixel.pixel_fraction,
        })?;
        for row in &sampled.rows {
            if let Some(target) = rendered.get_mut(*row) {
                compose_max(target, color);
            }
        }
    }
    Ok(())
}

#[inline(always)]
fn run_context(
    effect: &PreparedEffect,
    sample_context: PreparedSampleContext,
    progress: f64,
    local_seconds: f64,
) -> RunContext {
    RunContext {
        progress,
        seconds: local_seconds,
        duration: effect.duration_seconds,
        pixel_index: sample_context.pixel_index as i64,
        pixel_count: sample_context.pixel_count as i64,
        pixel_fraction: sample_context.pixel_fraction,
        global_marks: Marks { marks: Vec::new() },
    }
}

#[inline(always)]
fn sample_effect_pixel(
    effect: &PreparedEffect,
    implementation: &PreparedEffectImplementation,
    pixel: &PreparedTargetPixel,
    progress: f64,
    local_seconds: f64,
    scratch: &mut DslVmScratch,
) -> Result<Color, RuntimeError> {
    sample_effect_group(
        effect,
        implementation,
        PreparedSampleContext {
            pixel_index: pixel.pixel_index,
            pixel_count: pixel.pixel_count,
            pixel_fraction: pixel.pixel_fraction,
        },
        progress,
        local_seconds,
        scratch,
    )
}

#[inline(always)]
fn sample_effect_group(
    effect: &PreparedEffect,
    implementation: &PreparedEffectImplementation,
    sample_context: PreparedSampleContext,
    progress: f64,
    local_seconds: f64,
    scratch: &mut DslVmScratch,
) -> Result<Color, RuntimeError> {
    let context = run_context(effect, sample_context, progress, local_seconds);
    match implementation {
        PreparedEffectImplementation::Dsl {
            definition,
            bound_params,
        } => definition.sample_bound(bound_params, &context, scratch),
        PreparedEffectImplementation::Native { sample, .. } => sample.sample(&context),
    }
}

fn effect_implementation_at(
    effect: &PreparedEffect,
    sample_seconds: f64,
    bind_cache: &mut DslBindCache,
) -> Result<Option<PreparedEffectImplementation>, RenderError> {
    if effect.automation.is_empty() {
        return Ok(None);
    }
    let params =
        apply_automation_params(effect.params.clone(), &effect.automation, sample_seconds)?;
    Ok(Some(match &effect.implementation {
        PreparedEffectImplementation::Dsl { definition, .. } => PreparedEffectImplementation::Dsl {
            bound_params: definition.bind_params_cached(&params, bind_cache)?,
            definition: Arc::clone(definition),
        },
        PreparedEffectImplementation::Native { builtin, .. } => match builtin {
            Some(builtin) => match native_effect::bind(*builtin, &params)? {
                BoundNativeEffect::Sample(sample) => PreparedEffectImplementation::Native {
                    builtin: Some(*builtin),
                    sample,
                },
                _ => {
                    return Err(RenderError::GeneratorPrepare {
                        message: "automated native sample bound as generator".to_string(),
                    });
                }
            },
            None => {
                return Err(RenderError::GeneratorPrepare {
                    message: "missing native effect identity".to_string(),
                });
            }
        },
    }))
}

fn prepare_sample_context_groups(
    target: &[PreparedTargetPixel],
) -> Option<Vec<PreparedSampleContextGroup>> {
    let mut group_indexes = HashMap::<PreparedSampleContextKey, usize>::new();
    let mut groups = Vec::<PreparedSampleContextGroup>::new();
    let mut has_repeated_context = false;

    for (target_index, pixel) in target.iter().enumerate() {
        let context = PreparedSampleContext {
            pixel_index: pixel.pixel_index,
            pixel_count: pixel.pixel_count,
            pixel_fraction: pixel.pixel_fraction,
        };
        let key = PreparedSampleContextKey::from(context);
        if let Some(group_index) = group_indexes.get(&key) {
            has_repeated_context = true;
            groups[*group_index].target_indexes.push(target_index);
        } else {
            group_indexes.insert(key, groups.len());
            groups.push(PreparedSampleContextGroup {
                context,
                target_indexes: vec![target_index],
            });
        }
    }

    has_repeated_context.then_some(groups)
}

fn prepare_sample_context_groups_cached(
    cache: &mut PrepareTargetCache,
    target: &Arc<Vec<PreparedTargetPixel>>,
) -> Option<Arc<Vec<PreparedSampleContextGroup>>> {
    let key = arc_key(target);
    if let Some(entry) = cache.sample_groups.get(&key)
        && Arc::ptr_eq(&entry.source, target)
    {
        return entry.groups.clone();
    }
    let groups = prepare_sample_context_groups(target).map(Arc::new);
    cache.sample_groups.insert(
        key,
        PreparedSampleGroupCacheEntry {
            source: Arc::clone(target),
            groups: groups.clone(),
        },
    );
    groups
}

fn prepare_sample_groups_for_effect(
    cache: &mut PrepareTargetCache,
    compiled: &Arc<CompiledEffect>,
    target: &Arc<Vec<PreparedTargetPixel>>,
) -> Option<Arc<Vec<PreparedSampleContextGroup>>> {
    let compiled_key = arc_key(compiled);
    let eligible = *cache
        .sample_group_eligibility
        .entry(compiled_key)
        .or_insert_with(|| compiled.sample_reads_only_written_slots());
    eligible
        .then(|| prepare_sample_context_groups_cached(cache, target))
        .flatten()
}

fn prepare_sample_groups_for_implementation(
    cache: &mut PrepareTargetCache,
    implementation: &PreparedEffectImplementation,
    target: &Arc<Vec<PreparedTargetPixel>>,
) -> Option<Arc<Vec<PreparedSampleContextGroup>>> {
    match implementation {
        PreparedEffectImplementation::Dsl { definition, .. } => {
            prepare_sample_groups_for_effect(cache, definition, target)
        }
        PreparedEffectImplementation::Native { .. } => {
            prepare_sample_context_groups_cached(cache, target)
        }
    }
}

fn prepare_sampled_effect_pixel_groups(
    pixels: &[PreparedSampledEffectPixel],
) -> Option<Vec<PreparedSampledEffectPixelGroup>> {
    let mut group_indexes = HashMap::<PreparedSampleContextKey, usize>::new();
    let mut groups = Vec::<PreparedSampledEffectPixelGroup>::new();
    let mut has_repeated_context = false;

    for sampled in pixels {
        let context = PreparedSampleContext {
            pixel_index: sampled.pixel.pixel_index,
            pixel_count: sampled.pixel.pixel_count,
            pixel_fraction: sampled.pixel.pixel_fraction,
        };
        let key = PreparedSampleContextKey::from(context);
        if let Some(group_index) = group_indexes.get(&key) {
            has_repeated_context = true;
            groups[*group_index]
                .rows
                .extend(sampled.rows.iter().copied());
        } else {
            group_indexes.insert(key, groups.len());
            groups.push(PreparedSampledEffectPixelGroup {
                context,
                rows: sampled.rows.clone(),
            });
        }
    }

    has_repeated_context.then_some(groups)
}

fn evenly_sample_indices(source_count: usize, sample_count: usize) -> Vec<usize> {
    if source_count == 0 || sample_count == 0 {
        return Vec::new();
    }
    let sample_count = source_count.min(sample_count);
    if sample_count == 1 {
        return vec![0];
    }
    let last_source_index = source_count - 1;
    let last_sample_index = sample_count - 1;
    (0..sample_count)
        .map(|sample_index| {
            (sample_index * last_source_index + last_sample_index / 2) / last_sample_index
        })
        .collect()
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

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct PreparedTargetSelection {
    elements: Vec<ElementNodeId>,
    cells: Option<ElementCellRange>,
}

#[derive(Clone, Debug)]
struct PreparedTargetPixel {
    element_index: usize,
    element_cell_index: usize,
    pixel_index: usize,
    pixel_count: usize,
    pixel_fraction: f64,
}

#[derive(Clone, Copy, Debug)]
struct PreparedSampleContext {
    pixel_index: usize,
    pixel_count: usize,
    pixel_fraction: f64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct PreparedSampleContextKey {
    pixel_index: usize,
    pixel_count: usize,
    pixel_fraction_bits: u64,
}

impl From<PreparedSampleContext> for PreparedSampleContextKey {
    fn from(context: PreparedSampleContext) -> Self {
        Self {
            pixel_index: context.pixel_index,
            pixel_count: context.pixel_count,
            pixel_fraction_bits: context.pixel_fraction.to_bits(),
        }
    }
}

#[derive(Clone, Debug)]
struct PreparedSampleContextGroup {
    context: PreparedSampleContext,
    target_indexes: Vec<usize>,
}

#[derive(Clone, Debug)]
struct PreparedSampledEffectPixels {
    pixels: Vec<PreparedSampledEffectPixel>,
    groups: Option<Vec<PreparedSampledEffectPixelGroup>>,
}

#[derive(Clone, Debug)]
struct PreparedSampledEffectPixel {
    pixel: PreparedTargetPixel,
    rows: Vec<usize>,
}

#[derive(Clone, Debug)]
struct PreparedSampledEffectPixelGroup {
    context: PreparedSampleContext,
    rows: Vec<usize>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct TargetColorAddress {
    element_index: usize,
    element_cell_index: usize,
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

#[derive(Clone, Debug)]
struct PreparedCompositionGraph {
    output_index: usize,
    nodes: Vec<PreparedGraphNode>,
}

#[derive(Clone, Debug)]
struct PreparedGraphNode {
    target: Arc<Vec<PreparedTargetPixel>>,
    kind: PreparedGraphNodeKind,
}

#[derive(Clone, Debug)]
enum PreparedGraphNodeKind {
    Layer {
        layer_index: usize,
    },
    Operator {
        definition: Box<OperatorDefinition>,
        inputs: Vec<usize>,
        params: IndexMap<Identifier, Value>,
        automation: Vec<PreparedAutomation>,
        bound_params: Option<BoundParams>,
    },
    Output {
        inputs: Vec<usize>,
    },
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
    prepared_targets: HashMap<PreparedTargetCacheKey, Arc<Vec<PreparedTargetPixel>>>,
    generated_targets: HashMap<usize, GeneratedTargetCacheEntry>,
    generator_context_targets: HashMap<usize, GeneratorContextTargetCacheEntry>,
    sample_groups: HashMap<usize, PreparedSampleGroupCacheEntry>,
    sample_group_eligibility: HashMap<usize, bool>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct PreparedTargetCacheKey {
    target: PreparedTargetSelection,
    scope: PreparedTargetScopeKey,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum PreparedTargetScopeKey {
    PerFixture,
    WholeTarget,
}

impl From<&EffectScope> for PreparedTargetScopeKey {
    fn from(scope: &EffectScope) -> Self {
        match scope {
            EffectScope::PerFixture => Self::PerFixture,
            EffectScope::WholeTarget => Self::WholeTarget,
        }
    }
}

struct GeneratedTargetCacheEntry {
    source: Arc<TargetItemValue>,
    pixels: Arc<Vec<PreparedTargetPixel>>,
}

struct GeneratorContextTargetCacheEntry {
    source: Arc<Vec<PreparedTargetPixel>>,
    target: Arc<TargetValue>,
}

struct PreparedSampleGroupCacheEntry {
    source: Arc<Vec<PreparedTargetPixel>>,
    groups: Option<Arc<Vec<PreparedSampleContextGroup>>>,
}

#[cfg(test)]
mod tests;
