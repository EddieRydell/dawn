use dawn_language::dsl::{
    BoundParams, CompiledEffect, DslBindCache, DslVmScratch, Identifier, RuntimeError,
    TargetItemValue, TargetValue, Value,
};
use dawn_language::effect::{EffectInstId, EffectRef};
use dawn_language::element::ElementNodeId;
use dawn_language::model::DawnProject;
use dawn_language::native_effect::NativeSample;
use dawn_language::sequence::{
    AutomationBinding, AutomationClip, MarkCollectionKey, SequenceId, SequenceLayerId,
};
use dawn_language::setup::SetupId;
use dawn_language::validation::validate_sequence;
use dawn_language::values::Color;
use dawn_language::values::{
    SampleDuration, SampleTime, sample_duration_from_dawn_duration, sample_duration_seconds_f32,
    sample_time_from_seconds_f32,
};
use indexmap::{IndexMap, IndexSet};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::composition::{
    PrepareGraphContext, PreparedSignalGraph, layer_cache_history, prepare_signal_graph,
};
use super::composition::{sample_effect_into, sample_signal_graph, take_black_color_buffer};
use super::effects::preparation::{PrepareEffectContext, prepare_effect_inst};
use super::effects::sampling::{PreparedSampleContextGroup, PreparedSampleGroupCacheEntry};
use super::elements::{PreparedElement, element_cell_offsets, prepare_elements};
use super::targets::{PreparedTargetCache, PreparedTargetPixel};
use super::timeline::{frame_at_or_before, frame_count, prepare_timing, sample_time_for_frame};

static NEXT_RENDER_CACHE_ID: AtomicU32 = AtomicU32::new(1);

pub const MAX_GENERATED_EFFECTS: usize = 100_000;
pub const MAX_SIGNAL_SAMPLES_PER_OPERATOR_RENDER: usize = 4_096;

fn next_render_cache_id() -> u32 {
    NEXT_RENDER_CACHE_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedTargetPixelAddress {
    pub element_id: ElementNodeId,
    pub element_cell_index: usize,
}

#[derive(Clone, Debug)]
pub struct PreparedSequenceRenderer {
    pub(crate) render_cache_id: u32,
    pub(crate) frame_rate: u32,
    pub(crate) frame_count: u32,
    pub(crate) duration: SampleDuration,
    pub(crate) elements: Vec<PreparedElement>,
    pub(crate) element_cell_offsets: Vec<usize>,
    pub(crate) pixel_count: usize,
    pub(crate) effects: Vec<PreparedEffect>,
    pub(crate) effects_by_layer: Vec<Vec<usize>>,
    pub(crate) layers: Vec<PreparedLayer>,
    pub(crate) signal_graph: PreparedSignalGraph,
    pub(crate) layer_cache_history: SampleDuration,
}

#[derive(Debug, Default)]
pub struct SequenceRenderScratch {
    pub(crate) effect_vm: Vec<DslVmScratch>,
    pub(crate) operator_vm: Vec<DslVmScratch>,
    pub(crate) bind_cache: DslBindCache,
    pub(crate) signal_cache: HashMap<SignalCacheKey, Vec<Color>>,
    pub(crate) signal_cache_time: Option<SampleTime>,
    pub(crate) layer_cache: HashMap<SignalCacheKey, Vec<Color>>,
    pub(crate) render_cache_id: Option<u32>,
    pub(crate) color_buffers: Vec<Vec<Color>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderedFrame {
    pub frame_index: u32,
    pub frame_rate: u32,
    pub sample_time: SampleTime,
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
    OutputSize { expected: usize, actual: usize },
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
        let mut effects_by_layer = vec![Vec::new(); layers.len()];
        for (effect_index, effect) in effects.iter().enumerate() {
            let layer_index = layers
                .iter()
                .position(|layer| layer.id == effect.layer_id)
                .ok_or_else(|| RenderError::BadGraph {
                    message: format!(
                        "prepared effect references missing layer {}",
                        effect.layer_id.0
                    ),
                })?;
            effects_by_layer[layer_index].push(effect_index);
        }
        for layer_effects in &mut effects_by_layer {
            layer_effects.sort_unstable_by(|left, right| {
                effects[*left]
                    .start_time
                    .cmp(&effects[*right].start_time)
                    .then(left.cmp(right))
            });
        }
        let frame_rate = sequence.frame_rate;
        let duration = sample_duration_from_dawn_duration(&sequence.duration).map_err(|_| {
            RenderError::InvalidTiming {
                reason: "sequence duration exceeds the runtime clock range".to_string(),
            }
        })?;
        let frame_count = frame_count(&sequence.duration, frame_rate)?;
        let signal_graph = prepare_signal_graph(
            PrepareGraphContext {
                project,
                sequence,
                elements: &elements,
                layers: &layers,
            },
            &sequence.composition_graph,
        )?;
        let layer_cache_history = layer_cache_history(&signal_graph)?;
        Ok(Self {
            render_cache_id: next_render_cache_id(),
            frame_rate,
            frame_count,
            duration,
            elements,
            element_cell_offsets,
            pixel_count,
            effects,
            effects_by_layer,
            layers,
            signal_graph,
            layer_cache_history,
        })
    }

    pub fn render_seconds(&self, audio_seconds: f32) -> Result<RenderedFrame, RenderError> {
        self.render_seconds_with_scratch(audio_seconds, &mut SequenceRenderScratch::default())
    }

    pub fn render_seconds_with_scratch(
        &self,
        audio_seconds: f32,
        scratch: &mut SequenceRenderScratch,
    ) -> Result<RenderedFrame, RenderError> {
        if !audio_seconds.is_finite() {
            return Err(RenderError::InvalidTiming {
                reason: "audio seconds must be finite".to_string(),
            });
        }
        let max_frame = self.frame_count.saturating_sub(1);
        let sample_time = sample_time_from_seconds_f32(audio_seconds).map_err(|_| {
            RenderError::InvalidTiming {
                reason: "audio seconds exceed the runtime clock range".to_string(),
            }
        })?;
        let frame_index = frame_at_or_before(sample_time, self.frame_rate).min(max_frame);
        self.render_at(frame_index, scratch)
    }

    pub fn render_frame(&self, frame_index: u32) -> Result<RenderedFrame, RenderError> {
        self.render_frame_with_scratch(frame_index, &mut SequenceRenderScratch::default())
    }

    pub fn render_frame_with_scratch(
        &self,
        frame_index: u32,
        scratch: &mut SequenceRenderScratch,
    ) -> Result<RenderedFrame, RenderError> {
        let frame_index = frame_index.min(self.frame_count.saturating_sub(1));
        self.render_at(frame_index, scratch)
    }

    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }

    pub fn frame_rate(&self) -> u32 {
        self.frame_rate
    }

    /// Samples the prepared show signal at an exact microsecond clock value.
    /// This is the controller-facing path: it performs no floating-point time
    /// conversion and reuses all caller-owned VM/render scratch memory.
    pub fn sample_at(
        &self,
        sample_time: SampleTime,
        scratch: &mut SequenceRenderScratch,
    ) -> Result<Vec<RenderedElement>, RenderError> {
        let mut colors = take_black_color_buffer(scratch, self.pixel_count);
        self.sample_colors_into(sample_time, &mut colors, scratch)?;
        let mut offset = 0;
        let rendered = self
            .elements
            .iter()
            .map(|element| {
                let end = offset + element.pixel_count;
                let pixels = colors[offset..end].to_vec();
                offset = end;
                RenderedElement {
                    element_id: element.id,
                    pixels,
                }
            })
            .collect();
        colors.clear();
        scratch.color_buffers.push(colors);
        Ok(rendered)
    }

    /// Samples the show into a caller-owned, full-rig RGB buffer. After the
    /// scratch buffers have warmed up, callers do not allocate the final frame.
    pub fn sample_into(
        &self,
        sample_time: SampleTime,
        output: &mut [Color],
        scratch: &mut SequenceRenderScratch,
    ) -> Result<(), RenderError> {
        if output.len() != self.pixel_count {
            return Err(RenderError::OutputSize {
                expected: self.pixel_count,
                actual: output.len(),
            });
        }
        self.sample_colors_into(sample_time, output, scratch)
    }

    fn sample_colors_into(
        &self,
        sample_time: SampleTime,
        output: &mut [Color],
        scratch: &mut SequenceRenderScratch,
    ) -> Result<(), RenderError> {
        if scratch.render_cache_id != Some(self.render_cache_id) {
            scratch.signal_cache.clear();
            scratch.signal_cache_time = None;
            scratch.layer_cache.clear();
            scratch.render_cache_id = Some(self.render_cache_id);
        }
        scratch
            .effect_vm
            .resize_with(self.effects.len(), DslVmScratch::default);
        scratch
            .operator_vm
            .resize_with(self.signal_graph.nodes.len(), DslVmScratch::default);
        if sample_time.ticks() >= self.duration.ticks() {
            output.fill(Color {
                red: 0,
                green: 0,
                blue: 0,
            });
            return Ok(());
        }
        sample_signal_graph(self, sample_time, output, scratch)
    }

    pub fn duration(&self) -> SampleDuration {
        self.duration
    }

    pub fn active_effect_names(&self, frame_index: u32) -> Vec<&str> {
        let Ok(sample_time) = sample_time_for_frame(frame_index, self.frame_rate) else {
            return Vec::new();
        };
        self.effects
            .iter()
            .filter(|effect| effect.is_active(sample_time))
            .map(|effect| effect.name.as_str())
            .collect()
    }

    fn render_at(
        &self,
        frame_index: u32,
        scratch: &mut SequenceRenderScratch,
    ) -> Result<RenderedFrame, RenderError> {
        let sample_time = sample_time_for_frame(frame_index, self.frame_rate)?;
        let rendered = self.sample_at(sample_time, scratch)?;

        Ok(RenderedFrame {
            frame_index,
            frame_rate: self.frame_rate,
            sample_time,
            elements: rendered,
        })
    }

    pub(crate) fn sample_layer(
        &self,
        layer_index: usize,
        sample_time: dawn_language::values::SampleTime,
        scratch: &mut SequenceRenderScratch,
    ) -> Result<Vec<Color>, RenderError> {
        let layer = &self.layers[layer_index];
        let mut rendered = take_black_color_buffer(scratch, self.pixel_count);
        if !layer.enabled {
            return Ok(rendered);
        }

        if let Some(layer_effects) = self.effects_by_layer.get(layer_index) {
            for effect_index in layer_effects {
                let Some(effect) = self.effects.get(*effect_index) else {
                    continue;
                };
                if effect.start_time > sample_time {
                    break;
                }
                if !effect.is_active(sample_time) {
                    continue;
                }
                sample_effect_into(
                    effect,
                    &self.element_cell_offsets,
                    &mut rendered,
                    sample_time,
                    &mut scratch.effect_vm[*effect_index],
                    &mut scratch.bind_cache,
                )?;
            }
        }
        Ok(rendered)
    }

    pub(crate) fn duration_seconds(&self) -> f32 {
        sample_duration_seconds_f32(self.duration)
    }
}

pub(crate) fn arc_key<T>(value: &Arc<T>) -> usize {
    Arc::as_ptr(value).cast::<()>() as usize
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedEffect {
    pub(crate) layer_id: SequenceLayerId,
    pub(crate) start_time: dawn_language::values::SampleTime,
    pub(crate) duration: SampleDuration,
    pub(crate) target: Arc<Vec<PreparedTargetPixel>>,
    pub(crate) sample_groups: Option<Arc<Vec<PreparedSampleContextGroup>>>,
    pub(crate) name: String,
    pub(crate) implementation: PreparedEffectImplementation,
    pub(crate) params: IndexMap<Identifier, Value>,
    pub(crate) automation: Vec<PreparedAutomation>,
}

impl PreparedEffect {
    pub(crate) fn is_active(&self, sample_time: dawn_language::values::SampleTime) -> bool {
        sample_time >= self.start_time
            && self
                .start_time
                .checked_add_duration(self.duration)
                .is_some_and(|end| sample_time < end)
    }

    pub(crate) fn duration_seconds(&self) -> f32 {
        sample_duration_seconds_f32(self.duration)
    }

    pub(crate) fn local_seconds(&self, sample_time: dawn_language::values::SampleTime) -> f32 {
        sample_duration_seconds_f32(
            sample_time
                .checked_duration_since(self.start_time)
                .unwrap_or(SampleDuration::from_ticks(0)),
        )
    }

    pub(crate) fn progress(&self, sample_time: dawn_language::values::SampleTime) -> f32 {
        let elapsed = sample_time
            .checked_duration_since(self.start_time)
            .map_or(0, |duration| duration.ticks());
        (elapsed as f32 / self.duration.ticks() as f32).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Debug)]
pub(crate) enum PreparedEffectImplementation {
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
pub(crate) struct PreparedLayer {
    pub(crate) id: SequenceLayerId,
    pub(crate) enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SignalCacheKey {
    pub(crate) node_index: usize,
    pub(crate) sample_time: SampleTime,
}

impl Hash for SignalCacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.node_index.hash(state);
        self.sample_time.ticks().hash(state);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedAutomation {
    pub(crate) clip: AutomationClip,
    pub(crate) binding: AutomationBinding,
}

#[derive(Default)]
pub(crate) struct PrepareTargetCache {
    pub(crate) target: PreparedTargetCache,
    pub(crate) generated_targets: HashMap<usize, GeneratedTargetCacheEntry>,
    pub(crate) generator_context_targets: HashMap<usize, GeneratorContextTargetCacheEntry>,
    pub(crate) sample_groups: HashMap<usize, PreparedSampleGroupCacheEntry>,
    pub(crate) sample_group_eligibility: HashMap<usize, bool>,
}

pub(crate) struct GeneratedTargetCacheEntry {
    pub(crate) source: Arc<TargetItemValue>,
    pub(crate) pixels: Arc<Vec<PreparedTargetPixel>>,
}

pub(crate) struct GeneratorContextTargetCacheEntry {
    pub(crate) source: Arc<Vec<PreparedTargetPixel>>,
    pub(crate) target: Arc<TargetValue>,
}
