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
mod effect_preparation;
mod elements;
mod generators;
mod graph;
mod params;
mod raster;
mod rendering;
mod sampling;
mod sequence;
mod show;
pub use raster::{
    EffectRasterPrepareBatch, EffectRasterRenderScratch, PreparedEffectRasterRenderer,
    PreparedEffectRasterSample,
};
pub use sequence::resolve_effect_target_pixel_addresses;
pub use show::*;

use dawn_language::dsl::{
    BoundParams, CompiledEffect, DslBindCache, DslVmScratch, EffectKind, Identifier,
    OperatorRunContext, RuntimeError, SignalSampler, TargetItemValue, TargetValue, Value,
};
use dawn_language::effect::{EffectDefinitionId, EffectImplementation, EffectInstId, EffectRef};
use dawn_language::element::ElementNodeId;
use dawn_language::model::DawnProject;
use dawn_language::native_effect::{self, BoundNativeEffect, NativeSample};
use dawn_language::operator::OperatorDefinition;
use dawn_language::sequence::{
    AutomationBinding, AutomationClip, MarkCollectionKey, Sequence, SequenceId, SequenceLayerId,
};
use dawn_language::setup::SetupId;
use dawn_language::validation::validate_sequence;
use dawn_language::values::{Color, Marks};
use indexmap::{IndexMap, IndexSet};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use color::black;
use effect_preparation::{PrepareEffectContext, prepare_effect_inst};
use elements::{PreparedElement, element_cell_offsets, prepare_elements};
use generators::{
    GeneratorExpansion, GeneratorPrepareContext, expand_generator, expand_native_generator,
};
use graph::{
    PrepareGraphContext, PreparedCompositionGraph, PreparedGraphNodeKind,
    layer_cache_history_micros, prepare_composition_graph,
};
use params::{EffectParamTiming, prepare_operator_params, prepare_params};
use rendering::{render_composition_graph, render_effect, take_black_color_buffer};
use sampling::{
    PreparedSampleContextGroup, PreparedSampleGroupCacheEntry, PreparedSampledEffectPixel,
    PreparedSampledEffectPixels, TargetColorAddress, effect_implementation_at,
    evenly_sample_indices, prepare_sample_groups_for_implementation,
    prepare_sampled_effect_pixel_groups, render_sampled_effect_target_colors, sample_effect_group,
};
use sequence::{
    PreparedTargetCache, PreparedTargetPixel, generator_expansion_targets, prepare_target,
    prepare_target_pixels_cached,
};
use sequence::{
    build_effect_frame_index, build_effect_frame_index_for_window, frame_count, prepare_timing,
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

fn arc_key<T>(value: &Arc<T>) -> usize {
    Arc::as_ptr(value).cast::<()>() as usize
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
