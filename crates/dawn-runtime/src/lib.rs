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

use dawn_language::effect::{
    CurveSource, EffectDefinitionId, EffectInstId, EffectParamValue, EffectScope, EffectTarget,
};
use dawn_language::effect_dsl::{
    BoundEffectParams, EffectBindCache, EffectKind, EffectVmScratch, GeneratedEffect,
    GeneratorContext, Identifier, RunContext, RuntimeError, TargetItemValue, TargetPixelValue,
    TargetValue, Type, Value,
};
use dawn_language::model::DawnProject;
use dawn_language::sequence::{
    AutomationBinding, AutomationClip, AutomationMapping, AutomationTarget, CompositionGraphNodeId,
    CompositionGraphNodeKind, EffectClip, GraphOperator, GraphOperatorRef, GraphPortId,
    MarkCollectionKey, Sequence, SequenceClip, SequenceClipId, SequenceClipKind,
    SequenceCompositionGraph, SequenceId, SequenceLayerId, validate_graph_interface,
};
use dawn_language::setup::{
    FixtureGroupId, FixtureInst, FixtureInstanceId, Geometry, Layout, SetupId,
};
use dawn_language::values::{Color, Curve, CurvePoint, CurveValue, DawnTime, Marks};
use indexmap::{IndexMap, IndexSet};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedTargetPixelAddress {
    pub fixture_id: FixtureInstanceId,
    pub fixture_pixel_index: usize,
}

#[derive(Clone, Debug)]
pub struct PreparedSequenceRenderer {
    frame_rate: u32,
    frame_count: u64,
    fixtures: Vec<PreparedFixture>,
    effects: Vec<PreparedEffect>,
    layers: Vec<PreparedLayer>,
    composition_graph: PreparedCompositionGraph,
    effects_by_frame: Vec<Vec<usize>>,
}

#[derive(Clone, Debug)]
pub struct PreparedEffectRasterRenderer {
    frame_rate: u32,
    frame_count: u64,
    index_start_frame: u64,
    start_seconds: f64,
    duration_seconds: f64,
    target: Arc<Vec<PreparedTargetPixel>>,
    target_lookup: HashMap<TargetColorAddress, Vec<usize>>,
    effects: Vec<PreparedEffect>,
    effects_by_frame: Vec<Vec<usize>>,
}

#[derive(Clone, Debug)]
pub struct PreparedEffectRasterSample {
    row_count: usize,
    effect_pixels: Vec<PreparedSampledEffectPixels>,
}

pub struct EffectRasterPrepareBatch<'a> {
    project: &'a DawnProject,
    sequence: &'a Sequence,
    fixtures: Vec<PreparedFixture>,
    fixture_ids: IndexSet<FixtureInstanceId>,
    groups: IndexMap<FixtureGroupId, Vec<FixtureInstanceId>>,
    frame_rate: u32,
    frame_count: u64,
    bind_cache: EffectBindCache,
    target_cache: PrepareTargetCache,
}

pub fn resolve_effect_target_pixel_addresses(
    project: &DawnProject,
    setup_id: &SetupId,
    target: &EffectTarget,
    scope: &EffectScope,
) -> Result<Vec<RenderedTargetPixelAddress>, RenderError> {
    let setup = project
        .setups
        .get(setup_id)
        .ok_or_else(|| RenderError::MissingSetup {
            setup_id: setup_id.clone(),
        })?;
    let layout = project
        .layouts
        .get(&setup.layout)
        .ok_or(RenderError::MissingLayout)?;
    let fixtures = prepare_fixtures(project, layout)?;
    let fixture_ids = fixtures
        .iter()
        .map(|fixture| fixture.id.clone())
        .collect::<IndexSet<_>>();
    let groups = layout
        .groups
        .iter()
        .map(|group| {
            let members = layout
                .fixtures
                .iter()
                .filter(|fixture| group.fixtures.iter().any(|member| member == &fixture.id))
                .map(|fixture| fixture.id.clone())
                .collect::<Vec<_>>();
            (group.id.clone(), members)
        })
        .collect::<IndexMap<_, _>>();
    let target_ids = prepare_target(target, &fixture_ids, &groups)?;
    let pixels = prepare_target_pixels(&target_ids, &fixtures, scope)?;
    Ok(pixels
        .into_iter()
        .map(|pixel| RenderedTargetPixelAddress {
            fixture_id: fixtures[pixel.fixture_index].id.clone(),
            fixture_pixel_index: pixel.fixture_pixel_index,
        })
        .collect())
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderedFrame {
    pub frame_index: u64,
    pub frame_rate: u32,
    pub clock_seconds: f64,
    pub sample_seconds: f64,
    pub fixtures: Vec<RenderedFixture>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderedFixture {
    pub fixture_id: FixtureInstanceId,
    pub pixels: Vec<Color>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RenderError {
    InvalidTiming { reason: String },
    MissingSetup { setup_id: SetupId },
    MissingLayout,
    MissingSequence { sequence_id: SequenceId },
    MissingFixture { fixture_id: FixtureInstanceId },
    MissingFixtureDefinition,
    MissingEffect { effect_id: EffectDefinitionId },
    MissingCurve,
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
        let layout = project
            .layouts
            .get(&setup.layout)
            .ok_or(RenderError::MissingLayout)?;
        let sequence =
            project
                .sequences
                .get(sequence_id)
                .ok_or_else(|| RenderError::MissingSequence {
                    sequence_id: sequence_id.clone(),
                })?;
        prepare_timing(sequence)?;

        let fixtures = prepare_fixtures(project, layout)?;
        let fixture_ids = fixtures
            .iter()
            .map(|fixture| fixture.id.clone())
            .collect::<IndexSet<_>>();
        let groups = layout
            .groups
            .iter()
            .map(|group| {
                let members = layout
                    .fixtures
                    .iter()
                    .filter(|fixture| group.fixtures.iter().any(|member| member == &fixture.id))
                    .map(|fixture| fixture.id.clone())
                    .collect::<Vec<_>>();
                (group.id.clone(), members)
            })
            .collect::<IndexMap<_, _>>();

        let mut effects = Vec::with_capacity(sequence.effects.len());
        let mut generated_child_count = 0usize;
        let mut bind_cache = EffectBindCache::default();
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
                    fixtures: &fixtures,
                    fixture_ids: &fixture_ids,
                    groups: &groups,
                    effects: &mut effects,
                    generated_child_count: &mut generated_child_count,
                    bind_cache: &mut bind_cache,
                    target_cache: &mut target_cache,
                },
                effect,
            )?;
        }

        let frame_rate = sequence.frame_rate;
        let duration_seconds = sequence.duration.as_seconds_f64();
        let frame_count = frame_count(duration_seconds, frame_rate);
        let effects_by_frame = build_effect_frame_index(&effects, frame_count, frame_rate);
        let layers = sequence
            .layers
            .iter()
            .map(|layer| PreparedLayer {
                id: layer.id.clone(),
                enabled: layer.enabled,
                effects: effects
                    .iter()
                    .enumerate()
                    .filter_map(|(index, effect)| (effect.layer_id == layer.id).then_some(index))
                    .collect(),
            })
            .collect::<Vec<_>>();
        let composition_graph = prepare_composition_graph(
            PrepareGraphContext {
                project,
                sequence,
                fixtures: &fixtures,
            },
            &sequence.composition_graph,
        )?;
        Ok(Self {
            frame_rate,
            frame_count,
            fixtures,
            effects,
            layers,
            composition_graph,
            effects_by_frame,
        })
    }

    pub fn render_seconds(&self, audio_seconds: f64) -> Result<RenderedFrame, RenderError> {
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
        self.render_at(frame_index, audio_seconds)
    }

    pub fn render_frame(&self, frame_index: u64) -> Result<RenderedFrame, RenderError> {
        let frame_index = frame_index.min(self.frame_count.saturating_sub(1));
        self.render_at(frame_index, frame_index as f64 / f64::from(self.frame_rate))
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
        active_effects
            .iter()
            .filter_map(|effect_index| self.effects.get(*effect_index))
            .map(|effect| effect.definition.name().as_str())
            .collect()
    }

    fn render_at(
        &self,
        frame_index: u64,
        clock_seconds: f64,
    ) -> Result<RenderedFrame, RenderError> {
        let sample_seconds = frame_index as f64 / f64::from(self.frame_rate);
        let mut layer_buffers = IndexMap::<SequenceLayerId, Vec<RenderedFixture>>::new();
        for layer in &self.layers {
            let mut rendered = blank_rendered_fixtures(&self.fixtures);
            if layer.enabled
                && let Some(active_effects) = self.effects_by_frame.get(frame_index as usize)
            {
                for effect_index in active_effects {
                    if !layer.effects.contains(effect_index) {
                        continue;
                    }
                    let Some(effect) = self.effects.get(*effect_index) else {
                        continue;
                    };
                    if sample_seconds < effect.start_seconds
                        || sample_seconds >= effect.start_seconds + effect.duration_seconds
                    {
                        continue;
                    }
                    render_effect(effect, &mut rendered, sample_seconds)?;
                }
            }
            layer_buffers.insert(layer.id.clone(), rendered);
        }
        let rendered = render_composition_graph(
            &self.composition_graph,
            &self.fixtures,
            &layer_buffers,
            sample_seconds,
        )?;

        Ok(RenderedFrame {
            frame_index,
            frame_rate: self.frame_rate,
            clock_seconds,
            sample_seconds,
            fixtures: rendered,
        })
    }
}

impl PreparedEffectRasterRenderer {
    pub fn prepare(
        project: &DawnProject,
        setup_id: &SetupId,
        sequence_id: &SequenceId,
        clip_id: &SequenceClipId,
    ) -> Result<Self, RenderError> {
        let mut batch = EffectRasterPrepareBatch::prepare(project, setup_id, sequence_id)?;
        batch.prepare_clip(clip_id)
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

    pub fn render_target_colors(&self, audio_seconds: f64) -> Result<Vec<Color>, RenderError> {
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
        self.render_target_colors_at(frame_index)
    }

    pub fn prepare_sampled_raster(&self, row_count: usize) -> PreparedEffectRasterSample {
        let sample_indices = evenly_sample_indices(self.target.len(), row_count);
        let row_count = sample_indices.len();
        let mut sample_lookup = HashMap::<TargetColorAddress, Vec<usize>>::new();
        for (row_index, target_index) in sample_indices.into_iter().enumerate() {
            if let Some(pixel) = self.target.get(target_index) {
                sample_lookup
                    .entry(TargetColorAddress {
                        fixture_index: pixel.fixture_index,
                        fixture_pixel_index: pixel.fixture_pixel_index,
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
                                fixture_index: pixel.fixture_index,
                                fixture_pixel_index: pixel.fixture_pixel_index,
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

    pub fn render_sampled_raster_column(
        &self,
        sample: &PreparedEffectRasterSample,
        audio_seconds: f64,
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
        self.render_sampled_raster_column_at(sample, frame_index)
    }

    fn render_target_colors_at(&self, frame_index: u64) -> Result<Vec<Color>, RenderError> {
        let sample_seconds = frame_index as f64 / f64::from(self.frame_rate);
        let mut rendered = vec![black(); self.target.len()];

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
                render_effect_target_colors(
                    effect,
                    &self.target_lookup,
                    &mut rendered,
                    sample_seconds,
                )?;
            }
        }

        Ok(rendered)
    }

    fn render_sampled_raster_column_at(
        &self,
        sample: &PreparedEffectRasterSample,
        frame_index: u64,
    ) -> Result<Vec<Color>, RenderError> {
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
        let layout = project
            .layouts
            .get(&setup.layout)
            .ok_or(RenderError::MissingLayout)?;
        let sequence =
            project
                .sequences
                .get(sequence_id)
                .ok_or_else(|| RenderError::MissingSequence {
                    sequence_id: sequence_id.clone(),
                })?;
        prepare_timing(sequence)?;

        let fixtures = prepare_fixtures(project, layout)?;
        let fixture_ids = fixtures
            .iter()
            .map(|fixture| fixture.id.clone())
            .collect::<IndexSet<_>>();
        let groups = layout
            .groups
            .iter()
            .map(|group| {
                let members = layout
                    .fixtures
                    .iter()
                    .filter(|fixture| group.fixtures.iter().any(|member| member == &fixture.id))
                    .map(|fixture| fixture.id.clone())
                    .collect::<Vec<_>>();
                (group.id.clone(), members)
            })
            .collect::<IndexMap<_, _>>();
        let frame_rate = sequence.frame_rate;
        let duration_seconds = sequence.duration.as_seconds_f64();
        let frame_count = frame_count(duration_seconds, frame_rate);

        Ok(Self {
            project,
            sequence,
            fixtures,
            fixture_ids,
            groups,
            frame_rate,
            frame_count,
            bind_cache: EffectBindCache::default(),
            target_cache: PrepareTargetCache::default(),
        })
    }

    pub fn prepare_clip(
        &mut self,
        clip_id: &SequenceClipId,
    ) -> Result<PreparedEffectRasterRenderer, RenderError> {
        let clip = self
            .sequence
            .clips
            .iter()
            .find(|clip| &clip.id == clip_id)
            .ok_or_else(|| RenderError::MissingEffect {
                effect_id: EffectDefinitionId(clip_id.0.to_string()),
            })?;
        let SequenceClipKind::Effect(effect) = &clip.kind;
        let mut effects = Vec::new();
        let mut generated_child_count = 0usize;
        let target = prepare_effect_clip(
            PrepareEffectContext {
                project: self.project,
                sequence: self.sequence,
                layer_id: SequenceLayerId(0),
                fixtures: &self.fixtures,
                fixture_ids: &self.fixture_ids,
                groups: &self.groups,
                effects: &mut effects,
                generated_child_count: &mut generated_child_count,
                bind_cache: &mut self.bind_cache,
                target_cache: &mut self.target_cache,
            },
            clip,
            effect,
        )?;

        let start_seconds = clip.start.as_seconds_f64();
        let duration_seconds = clip.duration.as_seconds_f64();
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
            target_lookup: target_color_lookup(&target),
            target,
            effects,
            effects_by_frame,
        })
    }

    pub fn prepare_effect(
        &mut self,
        effect_id: &EffectInstId,
    ) -> Result<PreparedEffectRasterRenderer, RenderError> {
        self.prepare_clip(&SequenceClipId(effect_id.0))
    }
}

struct PrepareEffectContext<'a> {
    project: &'a DawnProject,
    sequence: &'a Sequence,
    layer_id: SequenceLayerId,
    fixtures: &'a [PreparedFixture],
    fixture_ids: &'a IndexSet<FixtureInstanceId>,
    groups: &'a IndexMap<FixtureGroupId, Vec<FixtureInstanceId>>,
    effects: &'a mut Vec<PreparedEffect>,
    generated_child_count: &'a mut usize,
    bind_cache: &'a mut EffectBindCache,
    target_cache: &'a mut PrepareTargetCache,
}

fn prepare_effect_inst(
    context: PrepareEffectContext<'_>,
    effect: &dawn_language::effect::EffectInst,
) -> Result<Arc<Vec<PreparedTargetPixel>>, RenderError> {
    let clip = SequenceClip {
        id: SequenceClipId(effect.id.0),
        start: effect.start.clone(),
        duration: effect.duration.clone(),
        target: effect.target.clone(),
        scope: effect.scope.clone(),
        kind: SequenceClipKind::Effect(EffectClip {
            definition: effect.definition.clone(),
            param_overrides: effect.param_overrides.clone(),
        }),
    };
    let SequenceClipKind::Effect(effect_clip) = &clip.kind;
    prepare_effect_clip(context, &clip, effect_clip)
}

fn prepare_effect_clip(
    context: PrepareEffectContext<'_>,
    clip: &SequenceClip,
    effect: &EffectClip,
) -> Result<Arc<Vec<PreparedTargetPixel>>, RenderError> {
    let effect_duration_seconds = clip.duration.as_seconds_f64();
    if !effect_duration_seconds.is_finite() || effect_duration_seconds <= 0.0 {
        return Err(RenderError::InvalidTiming {
            reason: "effect duration must be positive and finite".to_string(),
        });
    }
    let definition = context
        .project
        .definitions
        .effects
        .get(&effect.definition)
        .ok_or_else(|| RenderError::MissingEffect {
            effect_id: effect.definition.clone(),
        })?;
    let target_ids = prepare_target(&clip.target, context.fixture_ids, context.groups)?;
    let target = prepare_target_pixels_cached(
        context.target_cache,
        &target_ids,
        context.fixtures,
        &clip.scope,
    )?;
    let start_seconds = clip.start.as_seconds_f64();
    let param_timing = EffectParamTiming {
        start_seconds,
        duration_seconds: effect_duration_seconds,
    };
    let automation = automation_for_effect_clip(context.sequence, &clip.id);
    let params = prepare_params(
        context.project,
        context.sequence,
        &effect.param_overrides,
        param_timing,
    )?;
    match definition.compiled.kind() {
        EffectKind::Sample => {
            let bound_params = definition
                .compiled
                .bind_params_cached(&params, context.bind_cache);
            context.effects.push(PreparedEffect {
                layer_id: context.layer_id.clone(),
                start_seconds,
                duration_seconds: effect_duration_seconds,
                target: Arc::clone(&target),
                sample_groups: definition
                    .compiled
                    .sample_reads_only_written_slots()
                    .then(|| prepare_sample_context_groups(&target))
                    .flatten(),
                definition: definition.compiled.clone(),
                params,
                bound_params,
                automation,
            });
        }
        EffectKind::Generator => {
            let params = apply_automation_params(params, &automation, start_seconds);
            let bound_params = definition
                .compiled
                .bind_params_cached(&params, context.bind_cache);
            let mut generator_context = GeneratorPrepareContext {
                project: context.project,
                layer_id: context.layer_id.clone(),
                fixtures: context.fixtures,
                effects: context.effects,
                generated_child_count: context.generated_child_count,
                bind_cache: context.bind_cache,
                target_cache: context.target_cache,
            };
            for expansion_target in generator_expansion_targets(&target, &clip.scope) {
                expand_generator(
                    &mut generator_context,
                    &definition.compiled,
                    &bound_params,
                    GeneratorExpansion {
                        start_seconds,
                        duration_seconds: effect_duration_seconds,
                        target: expansion_target,
                        depth: 0,
                    },
                )?;
            }
        }
    }
    Ok(target)
}

struct PrepareGraphContext<'a> {
    project: &'a DawnProject,
    sequence: &'a Sequence,
    fixtures: &'a [PreparedFixture],
}

fn prepare_composition_graph(
    context: PrepareGraphContext<'_>,
    graph: &SequenceCompositionGraph,
) -> Result<PreparedCompositionGraph, RenderError> {
    let full_target = Arc::new(full_rig_target_pixels(context.fixtures)?);
    validate_graph_interface(graph).map_err(|error| RenderError::BadGraph {
        message: error.message,
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
        validate_composition_edge_ports(graph, &node_ids[from], &node_ids[to], edge)?;
        incoming[to].push((edge.to_port.clone(), from));
    }

    let mut prepared_nodes = Vec::<PreparedGraphNode>::new();
    let mut prepared_index_by_node = vec![usize::MAX; node_ids.len()];
    for node_index in &node_order {
        let node_id = &node_ids[*node_index];
        let node = graph_node(graph, node_id)?;
        let prepared = match &node.kind {
            CompositionGraphNodeKind::Layer { layer_id } => PreparedGraphNode {
                target: Arc::clone(&full_target),
                kind: PreparedGraphNodeKind::Layer {
                    layer_id: layer_id.clone(),
                },
            },
            CompositionGraphNodeKind::Operator(operator_node) => {
                let operator = builtin_graph_operator(&operator_node.operator)?;
                let inputs = composition_node_inputs(&node.kind, &incoming[*node_index])?
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
                validate_operator_inputs(operator, &inputs)?;
                PreparedGraphNode {
                    target: Arc::clone(&full_target),
                    kind: PreparedGraphNodeKind::Operator {
                        operator: operator.clone(),
                        inputs,
                        params: prepare_operator_params(
                            context.project,
                            context.sequence,
                            operator,
                            &operator_node.params,
                            EffectParamTiming {
                                start_seconds: 0.0,
                                duration_seconds: context.sequence.duration.as_seconds_f64(),
                            },
                        )?,
                        automation: automation_for_composition_node(context.sequence, &node.id),
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

fn builtin_graph_operator(operator: &GraphOperatorRef) -> Result<&GraphOperator, RenderError> {
    match operator {
        GraphOperatorRef::Builtin(operator) => Ok(operator),
    }
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

fn validate_operator_inputs(operator: &GraphOperator, inputs: &[usize]) -> Result<(), RenderError> {
    let valid = inputs.len() == operator.definition().inputs.len();
    if valid {
        Ok(())
    } else {
        Err(RenderError::BadGraph {
            message: format!("operator {:?} has wrong input count", operator),
        })
    }
}

fn validate_composition_edge_ports(
    graph: &SequenceCompositionGraph,
    from: &CompositionGraphNodeId,
    to: &CompositionGraphNodeId,
    edge: &dawn_language::sequence::EffectGraphEdge,
) -> Result<(), RenderError> {
    if !composition_output_ports(graph, from)?.contains(&edge.from_port.0.as_str()) {
        return Err(RenderError::BadGraph {
            message: format!(
                "unknown composition graph output port `{}`",
                edge.from_port.0
            ),
        });
    }
    if !composition_input_ports(graph, to)?.contains(&edge.to_port.0.as_str()) {
        return Err(RenderError::BadGraph {
            message: format!("unknown composition graph input port `{}`", edge.to_port.0),
        });
    }
    Ok(())
}

fn composition_node_inputs(
    kind: &CompositionGraphNodeKind,
    incoming: &[(GraphPortId, usize)],
) -> Result<Vec<usize>, RenderError> {
    composition_node_input_ports(kind)
        .iter()
        .map(|port| {
            incoming
                .iter()
                .find_map(|(input_port, node)| (input_port.0 == *port).then_some(*node))
                .ok_or_else(|| RenderError::BadGraph {
                    message: format!("composition graph input port `{port}` is not connected"),
                })
        })
        .collect()
}

fn composition_input_ports(
    graph: &SequenceCompositionGraph,
    node_id: &CompositionGraphNodeId,
) -> Result<Vec<&'static str>, RenderError> {
    Ok(composition_node_input_ports(
        &graph_node(graph, node_id)?.kind,
    ))
}

fn composition_output_ports(
    graph: &SequenceCompositionGraph,
    node_id: &CompositionGraphNodeId,
) -> Result<&'static [&'static str], RenderError> {
    Ok(composition_node_output_ports(
        &graph_node(graph, node_id)?.kind,
    ))
}

fn composition_node_input_ports(kind: &CompositionGraphNodeKind) -> Vec<&'static str> {
    match kind {
        CompositionGraphNodeKind::Layer { .. } => vec![],
        CompositionGraphNodeKind::Operator(operator) => {
            match builtin_graph_operator(&operator.operator) {
                Ok(operator) => operator
                    .definition()
                    .inputs
                    .iter()
                    .map(|port| port.source_name)
                    .collect(),
                Err(_) => vec![],
            }
        }
        CompositionGraphNodeKind::Output => vec!["input"],
    }
}

fn composition_node_output_ports(kind: &CompositionGraphNodeKind) -> &'static [&'static str] {
    match kind {
        CompositionGraphNodeKind::Layer { .. } => &["output"],
        CompositionGraphNodeKind::Operator(_) => &["output"],
        CompositionGraphNodeKind::Output => &[],
    }
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
    Ok(())
}

fn frame_count(duration_seconds: f64, frame_rate: u32) -> u64 {
    (duration_seconds * f64::from(frame_rate)).ceil() as u64
}

fn prepare_fixtures(
    project: &DawnProject,
    layout: &Layout,
) -> Result<Vec<PreparedFixture>, RenderError> {
    layout
        .fixtures
        .iter()
        .map(|fixture| prepare_fixture(project, fixture))
        .collect()
}

fn prepare_fixture(
    project: &DawnProject,
    fixture: &FixtureInst,
) -> Result<PreparedFixture, RenderError> {
    let definition = project
        .definitions
        .fixtures
        .get(&fixture.definition)
        .ok_or(RenderError::MissingFixtureDefinition)?;
    Ok(PreparedFixture {
        id: fixture.id.clone(),
        pixel_count: pixel_count(&definition.geometry),
    })
}

fn pixel_count(geometry: &Geometry) -> usize {
    match geometry {
        Geometry::Points { points } => points.len(),
        Geometry::Lines { pixels, .. } | Geometry::Arc { pixels, .. } => *pixels as usize,
    }
}

fn blank_rendered_fixtures(fixtures: &[PreparedFixture]) -> Vec<RenderedFixture> {
    fixtures
        .iter()
        .map(|fixture| RenderedFixture {
            fixture_id: fixture.id.clone(),
            pixels: vec![black(); fixture.pixel_count],
        })
        .collect()
}

fn flatten_rendered_fixtures(fixtures: &[RenderedFixture]) -> Vec<Color> {
    fixtures
        .iter()
        .flat_map(|fixture| fixture.pixels.iter().copied())
        .collect()
}

fn unflatten_rendered_fixtures(
    fixtures: &[PreparedFixture],
    colors: &[Color],
) -> Vec<RenderedFixture> {
    let mut offset = 0usize;
    fixtures
        .iter()
        .map(|fixture| {
            let end = offset.saturating_add(fixture.pixel_count).min(colors.len());
            let mut pixels = colors[offset..end].to_vec();
            if pixels.len() < fixture.pixel_count {
                pixels.resize(fixture.pixel_count, black());
            }
            offset = offset.saturating_add(fixture.pixel_count);
            RenderedFixture {
                fixture_id: fixture.id.clone(),
                pixels,
            }
        })
        .collect()
}

fn full_rig_target_pixels(
    fixtures: &[PreparedFixture],
) -> Result<Vec<PreparedTargetPixel>, RenderError> {
    let mut pixels = Vec::new();
    for (fixture_index, fixture) in fixtures.iter().enumerate() {
        let pixel_count = fixture.pixel_count;
        for fixture_pixel_index in 0..fixture.pixel_count {
            let pixel_index = fixture_pixel_index;
            let pixel_fraction = if fixture.pixel_count <= 1 {
                0.0
            } else {
                fixture_pixel_index as f64 / (fixture.pixel_count - 1) as f64
            };
            pixels.push(PreparedTargetPixel {
                fixture_index,
                fixture_pixel_index,
                pixel_index,
                pixel_count,
                pixel_fraction,
            });
        }
    }
    Ok(pixels)
}

fn prepare_target(
    target: &EffectTarget,
    fixture_ids: &IndexSet<FixtureInstanceId>,
    groups: &IndexMap<FixtureGroupId, Vec<FixtureInstanceId>>,
) -> Result<Vec<FixtureInstanceId>, RenderError> {
    match target {
        EffectTarget::Fixture(id) if fixture_ids.contains(id) => Ok(vec![id.clone()]),
        EffectTarget::Fixture(id) => Err(RenderError::MissingFixture {
            fixture_id: id.clone(),
        }),
        EffectTarget::Group(id) => groups.get(id).cloned().ok_or(RenderError::BadTarget),
    }
}

fn prepare_target_indexes(
    target: &[FixtureInstanceId],
    fixtures: &[PreparedFixture],
) -> Result<Vec<usize>, RenderError> {
    target
        .iter()
        .map(|id| {
            fixtures
                .iter()
                .position(|fixture| &fixture.id == id)
                .ok_or_else(|| RenderError::MissingFixture {
                    fixture_id: id.clone(),
                })
        })
        .collect()
}

fn prepare_target_pixels(
    target: &[FixtureInstanceId],
    fixtures: &[PreparedFixture],
    scope: &EffectScope,
) -> Result<Vec<PreparedTargetPixel>, RenderError> {
    let indexes = prepare_target_indexes(target, fixtures)?;
    let total_target_pixels = indexes
        .iter()
        .map(|index| fixtures[*index].pixel_count)
        .sum::<usize>();
    let mut pixels = Vec::with_capacity(total_target_pixels);
    let mut whole_index = 0usize;
    for fixture_index in indexes {
        let fixture_pixel_count = fixtures[fixture_index].pixel_count;
        for fixture_pixel_index in 0..fixture_pixel_count {
            let (pixel_index, pixel_count) = match scope {
                EffectScope::PerFixture => (fixture_pixel_index, fixture_pixel_count),
                EffectScope::WholeTarget => (whole_index, total_target_pixels),
            };
            pixels.push(PreparedTargetPixel {
                fixture_index,
                fixture_pixel_index,
                pixel_index,
                pixel_count,
                pixel_fraction: pixel_fraction(pixel_index, pixel_count),
            });
            whole_index += 1;
        }
    }
    Ok(pixels)
}

fn prepare_target_pixels_cached(
    cache: &mut PrepareTargetCache,
    target: &[FixtureInstanceId],
    fixtures: &[PreparedFixture],
    scope: &EffectScope,
) -> Result<Arc<Vec<PreparedTargetPixel>>, RenderError> {
    let key = PreparedTargetCacheKey {
        target: target.to_vec(),
        scope: PreparedTargetScopeKey::from(scope),
    };
    if let Some(pixels) = cache.prepared_targets.get(&key) {
        return Ok(Arc::clone(pixels));
    }
    let pixels = Arc::new(prepare_target_pixels(target, fixtures, scope)?);
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
            let mut fixture_pixels = Vec::new();
            let mut current_fixture_index = None;

            for pixel in target.iter() {
                if current_fixture_index.is_some_and(|index| index != pixel.fixture_index) {
                    targets.push(Arc::new(fixture_pixels));
                    fixture_pixels = Vec::new();
                }
                current_fixture_index = Some(pixel.fixture_index);
                fixture_pixels.push(pixel.clone());
            }

            if !fixture_pixels.is_empty() {
                targets.push(Arc::new(fixture_pixels));
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
    operator: &GraphOperator,
    overrides: &IndexMap<Identifier, EffectParamValue>,
    timing: EffectParamTiming,
) -> Result<IndexMap<Identifier, Value>, RenderError> {
    let mut params = operator
        .definition()
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
                        (seconds >= timing.start_seconds && seconds < end_seconds).then(|| {
                            DawnTime(Duration::from_secs_f64(seconds - timing.start_seconds))
                        })
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
        EffectParamValue::Array(values) => values
            .iter()
            .map(|value| prepare_param_value(project, sequence, value, timing))
            .collect::<Result<Vec<_>, _>>()
            .map(Arc::new)
            .map(Value::Array),
    }
}

fn automation_for_effect_clip(
    sequence: &Sequence,
    clip_id: &SequenceClipId,
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
                            if effect_id.0 == clip_id.0
                    )
                })
                .map(move |binding| PreparedAutomation {
                    clip: clip.clone(),
                    binding: binding.clone(),
                })
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
                .map(move |binding| PreparedAutomation {
                    clip: clip.clone(),
                    binding: binding.clone(),
                })
        })
        .collect()
}

fn apply_automation_params(
    mut params: IndexMap<Identifier, Value>,
    automation: &[PreparedAutomation],
    sample_seconds: f64,
) -> IndexMap<Identifier, Value> {
    for automation in automation {
        let normalized = sample_automation_clip(&automation.clip, sample_seconds);
        params.insert(
            automation_param(&automation.binding).clone(),
            automation_value(
                &automation.clip,
                &automation.binding,
                normalized,
                sample_seconds,
            ),
        );
    }
    params
}

fn automation_param(binding: &AutomationBinding) -> &Identifier {
    match &binding.target {
        AutomationTarget::EffectParam { param, .. }
        | AutomationTarget::CompositionNodeParam { param, .. } => param,
    }
}

fn automation_value(
    clip: &AutomationClip,
    binding: &AutomationBinding,
    normalized: f64,
    sample_seconds: f64,
) -> Value {
    match &binding.mapping {
        AutomationMapping::Float { min, max } => Value::Float(lerp(*min, *max, normalized)),
        AutomationMapping::Int { min, max } => {
            Value::Int(lerp(*min as f64, *max as f64, normalized).round() as i64)
        }
        AutomationMapping::Bool => Value::Bool(normalized >= 0.5),
        AutomationMapping::Enum { values } => {
            if values.is_empty() {
                return Value::Void;
            }
            let index = if values.is_empty() {
                0
            } else {
                ((normalized.clamp(0.0, 1.0) * values.len() as f64).floor() as usize)
                    .min(values.len().saturating_sub(1))
            };
            Value::Enum(values[index].clone())
        }
        AutomationMapping::FloatCurve { min, max } => Value::Curve(Arc::new(float_curve_window(
            clip,
            *min,
            *max,
            sample_seconds,
        ))),
    }
}

fn sample_automation_clip(clip: &AutomationClip, sample_seconds: f64) -> f64 {
    let start_seconds = clip.start.as_seconds_f64();
    let duration_seconds = clip.duration.as_seconds_f64();
    let position = if duration_seconds <= 0.0 {
        0.0
    } else {
        ((sample_seconds - start_seconds) / duration_seconds).clamp(0.0, 1.0)
    };
    sample_float_curve(&clip.curve, position).clamp(0.0, 1.0)
}

fn float_curve_window(clip: &AutomationClip, min: f64, max: f64, sample_seconds: f64) -> Curve {
    let start_seconds = clip.start.as_seconds_f64();
    let duration_seconds = clip.duration.as_seconds_f64().max(0.000000001);
    let sample_position = ((sample_seconds - start_seconds) / duration_seconds).clamp(0.0, 1.0);
    let points = clip
        .curve
        .points
        .iter()
        .filter_map(|point| {
            let position = point.position - sample_position;
            (0.0..=1.0).contains(&position).then(|| CurvePoint {
                position,
                value: CurveValue::Float(lerp(min, max, curve_point_float(point))),
            })
        })
        .collect::<Vec<_>>();
    if points.is_empty() {
        return Curve {
            points: vec![CurvePoint {
                position: 0.0,
                value: CurveValue::Float(lerp(
                    min,
                    max,
                    sample_automation_clip(clip, sample_seconds),
                )),
            }],
        };
    }
    Curve { points }
}

fn sample_float_curve(curve: &Curve, position: f64) -> f64 {
    let mut points = curve.points.iter().collect::<Vec<_>>();
    points.sort_by(|left, right| left.position.total_cmp(&right.position));
    let Some(first) = points.first() else {
        return 0.0;
    };
    if position <= first.position {
        return curve_point_float(first);
    }
    for pair in points.windows(2) {
        let left = pair[0];
        let right = pair[1];
        if position <= right.position {
            let span = right.position - left.position;
            let amount = if span <= 0.0 {
                0.0
            } else {
                (position - left.position) / span
            };
            return lerp(curve_point_float(left), curve_point_float(right), amount);
        }
    }
    points
        .last()
        .map(|point| curve_point_float(point))
        .unwrap_or(0.0)
}

fn curve_point_float(point: &CurvePoint) -> f64 {
    match point.value {
        CurveValue::Float(value) => value,
        CurveValue::Color(_) => 0.0,
    }
}

fn lerp(min: f64, max: f64, amount: f64) -> f64 {
    min + (max - min) * amount.clamp(0.0, 1.0)
}

const MAX_GENERATOR_DEPTH: usize = 4;
const MAX_GENERATED_CHILDREN: usize = 1_000_000;

#[derive(Clone, Debug)]
struct GeneratorExpansion {
    start_seconds: f64,
    duration_seconds: f64,
    target: Arc<Vec<PreparedTargetPixel>>,
    depth: usize,
}

struct GeneratorPrepareContext<'a> {
    project: &'a DawnProject,
    layer_id: SequenceLayerId,
    fixtures: &'a [PreparedFixture],
    effects: &'a mut Vec<PreparedEffect>,
    generated_child_count: &'a mut usize,
    bind_cache: &'a mut EffectBindCache,
    target_cache: &'a mut PrepareTargetCache,
}

fn expand_generator(
    context: &mut GeneratorPrepareContext<'_>,
    definition: &dawn_language::effect_dsl::CompiledEffect,
    params: &BoundEffectParams,
    expansion: GeneratorExpansion,
) -> Result<(), RenderError> {
    if expansion.depth >= MAX_GENERATOR_DEPTH {
        return Err(RenderError::GeneratorPrepare {
            message: "generator depth limit exceeded".to_string(),
        });
    }
    let mut scratch = EffectVmScratch::default();
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
        prepare_generated_child(context, expansion.start_seconds, expansion.depth, child)?;
    }
    Ok(())
}

fn prepare_generated_child(
    context: &mut GeneratorPrepareContext<'_>,
    parent_start_seconds: f64,
    parent_depth: usize,
    child: GeneratedEffect,
) -> Result<(), RenderError> {
    let effect_id = EffectDefinitionId(child.definition.as_str().to_string());
    let definition = context
        .project
        .definitions
        .effects
        .get(&effect_id)
        .ok_or_else(|| RenderError::GeneratorPrepare {
            message: format!("generated child effect `{}` does not exist", effect_id.0),
        })?;
    validate_generated_params(&definition.compiled, &child.params)?;
    let duration_seconds = child.duration_seconds;
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        return Err(RenderError::InvalidTiming {
            reason: "generated effect duration must be positive and finite".to_string(),
        });
    }
    let target = prepared_pixels_from_generated_target_cached(
        context.target_cache,
        context.fixtures,
        child.target,
    )?;
    let bound_params = definition
        .compiled
        .bind_params_cached(&child.params, context.bind_cache);
    let start_seconds = parent_start_seconds + child.start_seconds;
    match definition.compiled.kind() {
        EffectKind::Sample => {
            context.effects.push(PreparedEffect {
                layer_id: context.layer_id.clone(),
                start_seconds,
                duration_seconds,
                sample_groups: definition
                    .compiled
                    .sample_reads_only_written_slots()
                    .then(|| prepare_sample_context_groups(&target))
                    .flatten(),
                target,
                definition: definition.compiled.clone(),
                params: child.params,
                bound_params,
                automation: Vec::new(),
            });
            Ok(())
        }
        EffectKind::Generator => expand_generator(
            context,
            &definition.compiled,
            &bound_params,
            GeneratorExpansion {
                start_seconds,
                duration_seconds,
                target,
                depth: parent_depth + 1,
            },
        ),
    }
}

fn validate_generated_params(
    definition: &dawn_language::effect_dsl::CompiledEffect,
    params: &IndexMap<Identifier, Value>,
) -> Result<(), RenderError> {
    for key in params.keys() {
        if !definition.params().iter().any(|param| &param.name == key) {
            return Err(RenderError::GeneratorPrepare {
                message: format!("unknown generated param `{}`", key.as_str()),
            });
        }
    }
    for param in definition.params() {
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
            | (Value::Curve(_), Type::Curve(_))
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
        fixture_index: pixel.fixture_index as i64,
        fixture_pixel_index: pixel.fixture_pixel_index as i64,
        pixel_index: pixel.pixel_index as i64,
        pixel_count: pixel.pixel_count as i64,
        pixel_fraction: pixel.pixel_fraction,
    }
}

fn prepared_pixels_from_generated_target(
    fixtures: &[PreparedFixture],
    target: Arc<TargetItemValue>,
) -> Result<Vec<PreparedTargetPixel>, RenderError> {
    target
        .pixels
        .iter()
        .copied()
        .map(|pixel| {
            let fixture_index = usize::try_from(pixel.fixture_index).map_err(|_| {
                RenderError::GeneratorPrepare {
                    message: "generated target fixture index cannot be negative".to_string(),
                }
            })?;
            let fixture_pixel_index = usize::try_from(pixel.fixture_pixel_index).map_err(|_| {
                RenderError::GeneratorPrepare {
                    message: "generated target pixel index cannot be negative".to_string(),
                }
            })?;
            let fixture =
                fixtures
                    .get(fixture_index)
                    .ok_or_else(|| RenderError::GeneratorPrepare {
                        message: "generated target fixture index is out of bounds".to_string(),
                    })?;
            if fixture_pixel_index >= fixture.pixel_count {
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
                fixture_index,
                fixture_pixel_index,
                pixel_index,
                pixel_count,
                pixel_fraction: pixel.pixel_fraction,
            })
        })
        .collect()
}

fn prepared_pixels_from_generated_target_cached(
    cache: &mut PrepareTargetCache,
    fixtures: &[PreparedFixture],
    target: Arc<TargetItemValue>,
) -> Result<Arc<Vec<PreparedTargetPixel>>, RenderError> {
    let key = arc_key(&target);
    if let Some(entry) = cache.generated_targets.get(&key)
        && Arc::ptr_eq(&entry.source, &target)
    {
        return Ok(Arc::clone(&entry.pixels));
    }
    let pixels = Arc::new(prepared_pixels_from_generated_target(
        fixtures,
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
    let mut index = vec![Vec::new(); frame_count as usize];
    let end_frame_limit = start_frame.saturating_add(frame_count);
    for (effect_index, effect) in effects.iter().enumerate() {
        let effect_start_frame = (effect.start_seconds * f64::from(frame_rate))
            .floor()
            .max(0.0) as u64;
        let effect_end_frame = ((effect.start_seconds + effect.duration_seconds)
            * f64::from(frame_rate))
        .ceil() as u64;
        for frame in effect_start_frame.max(start_frame)..effect_end_frame.min(end_frame_limit) {
            let local_frame = frame.saturating_sub(start_frame);
            if let Some(bucket) = index.get_mut(local_frame as usize) {
                bucket.push(effect_index);
            }
        }
    }
    index
}

fn render_effect(
    effect: &PreparedEffect,
    rendered: &mut [RenderedFixture],
    sample_seconds: f64,
) -> Result<(), RenderError> {
    let local_seconds = sample_seconds - effect.start_seconds;
    let progress = (local_seconds / effect.duration_seconds).clamp(0.0, 1.0);
    let mut scratch = EffectVmScratch::default();

    if let Some(groups) = &effect.sample_groups {
        for group in groups {
            let color =
                sample_effect_group(effect, group.context, progress, local_seconds, &mut scratch)?;
            for target_index in &group.target_indexes {
                let Some(pixel) = effect.target.get(*target_index) else {
                    continue;
                };
                compose_max(
                    &mut rendered[pixel.fixture_index].pixels[pixel.fixture_pixel_index],
                    color,
                );
            }
        }
        return Ok(());
    }

    for pixel in effect.target.iter() {
        let color = sample_effect_pixel(effect, pixel, progress, local_seconds, &mut scratch)?;
        compose_max(
            &mut rendered[pixel.fixture_index].pixels[pixel.fixture_pixel_index],
            color,
        );
    }
    Ok(())
}

fn render_composition_graph(
    graph: &PreparedCompositionGraph,
    fixtures: &[PreparedFixture],
    layer_buffers: &IndexMap<SequenceLayerId, Vec<RenderedFixture>>,
    sample_seconds: f64,
) -> Result<Vec<RenderedFixture>, RenderError> {
    let mut cache = HashMap::<GraphRenderCacheKey, Vec<Color>>::new();
    let output = render_graph_node(
        graph,
        graph.output_index,
        sample_seconds,
        layer_buffers,
        &mut cache,
    )?;
    Ok(unflatten_rendered_fixtures(fixtures, &output))
}

fn render_graph_node(
    graph: &PreparedCompositionGraph,
    node_index: usize,
    sample_seconds: f64,
    layer_buffers: &IndexMap<SequenceLayerId, Vec<RenderedFixture>>,
    cache: &mut HashMap<GraphRenderCacheKey, Vec<Color>>,
) -> Result<Vec<Color>, RenderError> {
    let frame_key = (sample_seconds * 1_000_000.0).round() as i64;
    let key = GraphRenderCacheKey {
        node_index,
        frame_key,
    };
    if let Some(colors) = cache.get(&key) {
        return Ok(colors.clone());
    }
    let node = graph
        .nodes
        .get(node_index)
        .ok_or_else(|| RenderError::BadGraph {
            message: "graph node index is out of bounds".to_string(),
        })?;
    let colors = match &node.kind {
        PreparedGraphNodeKind::Layer { layer_id } => layer_buffers
            .get(layer_id)
            .map(|fixtures| flatten_rendered_fixtures(fixtures))
            .unwrap_or_else(|| vec![black(); node.target.len()]),
        PreparedGraphNodeKind::Operator {
            operator,
            inputs,
            params,
            automation,
        } => {
            let params = apply_automation_params(params.clone(), automation, sample_seconds);
            render_graph_operator(
                graph,
                operator,
                inputs,
                &params,
                sample_seconds,
                layer_buffers,
                cache,
            )?
        }
        PreparedGraphNodeKind::Output { inputs } => {
            let mut output = vec![black(); node.target.len()];
            for input in inputs {
                let source =
                    render_graph_node(graph, *input, sample_seconds, layer_buffers, cache)?;
                for (target, source) in output.iter_mut().zip(source) {
                    compose_max(target, source);
                }
            }
            output
        }
    };
    cache.insert(key, colors.clone());
    Ok(colors)
}

fn render_graph_operator(
    graph: &PreparedCompositionGraph,
    operator: &GraphOperator,
    inputs: &[usize],
    params: &IndexMap<Identifier, Value>,
    sample_seconds: f64,
    layer_buffers: &IndexMap<SequenceLayerId, Vec<RenderedFixture>>,
    cache: &mut HashMap<GraphRenderCacheKey, Vec<Color>>,
) -> Result<Vec<Color>, RenderError> {
    match operator {
        GraphOperator::Max => binary_graph_op(
            graph,
            inputs,
            sample_seconds,
            layer_buffers,
            cache,
            max_color,
        ),
        GraphOperator::Add => binary_graph_op(
            graph,
            inputs,
            sample_seconds,
            layer_buffers,
            cache,
            add_color,
        ),
        GraphOperator::Multiply => binary_graph_op(
            graph,
            inputs,
            sample_seconds,
            layer_buffers,
            cache,
            multiply_color,
        ),
        GraphOperator::IntensityModulate => {
            let source = render_graph_node(graph, inputs[0], sample_seconds, layer_buffers, cache)?;
            let mask = render_graph_node(graph, inputs[1], sample_seconds, layer_buffers, cache)?;
            Ok(source
                .into_iter()
                .zip(mask)
                .map(|(source, mask)| scale_color(source, intensity(mask)))
                .collect())
        }
        GraphOperator::Dim => {
            let amount = float_param(params, "amount")?.clamp(0.0, 1.0);
            let source = render_graph_node(graph, inputs[0], sample_seconds, layer_buffers, cache)?;
            Ok(source
                .into_iter()
                .map(|color| scale_color(color, amount))
                .collect())
        }
        GraphOperator::Invert => {
            let source = render_graph_node(graph, inputs[0], sample_seconds, layer_buffers, cache)?;
            Ok(source.into_iter().map(invert_color).collect())
        }
        GraphOperator::Colorize => {
            let tint = color_param(params, "color")?;
            let source = render_graph_node(graph, inputs[0], sample_seconds, layer_buffers, cache)?;
            Ok(source
                .into_iter()
                .map(|color| scale_color(tint, intensity(color)))
                .collect())
        }
        GraphOperator::Delay => {
            let delay = float_param(params, "seconds")?.max(0.0);
            render_graph_node(
                graph,
                inputs[0],
                sample_seconds - delay,
                layer_buffers,
                cache,
            )
        }
        GraphOperator::Echo => {
            let delay = float_param(params, "seconds")?.max(0.0);
            let repeats = int_param(params, "repeats")?.clamp(1, 32);
            let decay = float_param(params, "decay")?.clamp(0.0, 1.0);
            let mut output = vec![black(); graph.nodes[inputs[0]].target.len()];
            for repeat in 0..=repeats {
                let source = render_graph_node(
                    graph,
                    inputs[0],
                    sample_seconds - delay * repeat as f64,
                    layer_buffers,
                    cache,
                )?;
                let amount = decay.powi(repeat as i32);
                for (target, source) in output.iter_mut().zip(source) {
                    compose_max(target, scale_color(source, amount));
                }
            }
            Ok(output)
        }
    }
}

fn binary_graph_op(
    graph: &PreparedCompositionGraph,
    inputs: &[usize],
    sample_seconds: f64,
    layer_buffers: &IndexMap<SequenceLayerId, Vec<RenderedFixture>>,
    cache: &mut HashMap<GraphRenderCacheKey, Vec<Color>>,
    op: fn(Color, Color) -> Color,
) -> Result<Vec<Color>, RenderError> {
    let left = render_graph_node(graph, inputs[0], sample_seconds, layer_buffers, cache)?;
    let right = render_graph_node(graph, inputs[1], sample_seconds, layer_buffers, cache)?;
    Ok(left.into_iter().zip(right).map(|(l, r)| op(l, r)).collect())
}

fn target_color_lookup(target: &[PreparedTargetPixel]) -> HashMap<TargetColorAddress, Vec<usize>> {
    let mut lookup = HashMap::<TargetColorAddress, Vec<usize>>::new();
    for (target_index, pixel) in target.iter().enumerate() {
        lookup
            .entry(TargetColorAddress {
                fixture_index: pixel.fixture_index,
                fixture_pixel_index: pixel.fixture_pixel_index,
            })
            .or_default()
            .push(target_index);
    }
    lookup
}

fn render_effect_target_colors(
    effect: &PreparedEffect,
    target_lookup: &HashMap<TargetColorAddress, Vec<usize>>,
    rendered: &mut [Color],
    sample_seconds: f64,
) -> Result<(), RenderError> {
    let local_seconds = sample_seconds - effect.start_seconds;
    let progress = (local_seconds / effect.duration_seconds).clamp(0.0, 1.0);
    let mut scratch = EffectVmScratch::default();

    if let Some(groups) = &effect.sample_groups {
        for group in groups {
            let color =
                sample_effect_group(effect, group.context, progress, local_seconds, &mut scratch)?;
            for effect_target_index in &group.target_indexes {
                let Some(pixel) = effect.target.get(*effect_target_index) else {
                    continue;
                };
                let Some(target_indexes) = target_lookup.get(&TargetColorAddress {
                    fixture_index: pixel.fixture_index,
                    fixture_pixel_index: pixel.fixture_pixel_index,
                }) else {
                    continue;
                };
                for target_index in target_indexes {
                    if let Some(target) = rendered.get_mut(*target_index) {
                        compose_max(target, color);
                    }
                }
            }
        }
        return Ok(());
    }

    for pixel in effect.target.iter() {
        let Some(target_indexes) = target_lookup.get(&TargetColorAddress {
            fixture_index: pixel.fixture_index,
            fixture_pixel_index: pixel.fixture_pixel_index,
        }) else {
            continue;
        };
        let color = sample_effect_pixel(effect, pixel, progress, local_seconds, &mut scratch)?;
        for target_index in target_indexes {
            if let Some(target) = rendered.get_mut(*target_index) {
                compose_max(target, color);
            }
        }
    }
    Ok(())
}

fn render_sampled_effect_target_colors(
    effect: &PreparedEffect,
    effect_pixels: &PreparedSampledEffectPixels,
    rendered: &mut [Color],
    sample_seconds: f64,
) -> Result<(), RenderError> {
    let local_seconds = sample_seconds - effect.start_seconds;
    let progress = (local_seconds / effect.duration_seconds).clamp(0.0, 1.0);
    let mut scratch = EffectVmScratch::default();

    if let Some(groups) = &effect_pixels.groups {
        for group in groups {
            let color =
                sample_effect_group(effect, group.context, progress, local_seconds, &mut scratch)?;
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
        let color = sample_effect_pixel(effect, pixel, progress, local_seconds, &mut scratch)?;
        for row in &sampled.rows {
            if let Some(target) = rendered.get_mut(*row) {
                compose_max(target, color);
            }
        }
    }
    Ok(())
}

fn sample_effect_pixel(
    effect: &PreparedEffect,
    pixel: &PreparedTargetPixel,
    progress: f64,
    local_seconds: f64,
    scratch: &mut EffectVmScratch,
) -> Result<Color, RuntimeError> {
    sample_effect_group(
        effect,
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

fn sample_effect_group(
    effect: &PreparedEffect,
    sample_context: PreparedSampleContext,
    progress: f64,
    local_seconds: f64,
    scratch: &mut EffectVmScratch,
) -> Result<Color, RuntimeError> {
    let context = RunContext {
        progress,
        seconds: local_seconds,
        duration: effect.duration_seconds,
        pixel_index: sample_context.pixel_index as i64,
        pixel_count: sample_context.pixel_count as i64,
        pixel_fraction: sample_context.pixel_fraction,
        global_marks: Marks { marks: Vec::new() },
    };
    if effect.automation.is_empty() {
        return effect
            .definition
            .sample_bound(&effect.bound_params, &context, scratch);
    }
    let params = apply_automation_params(
        effect.params.clone(),
        &effect.automation,
        effect.start_seconds + local_seconds,
    );
    let bound_params = effect.definition.bind_params(&params);
    effect
        .definition
        .sample_bound(&bound_params, &context, scratch)
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

fn compose_max(target: &mut Color, source: Color) {
    target.red = target.red.max(source.red);
    target.green = target.green.max(source.green);
    target.blue = target.blue.max(source.blue);
}

fn max_color(left: Color, right: Color) -> Color {
    Color {
        red: left.red.max(right.red),
        green: left.green.max(right.green),
        blue: left.blue.max(right.blue),
    }
}

fn add_color(left: Color, right: Color) -> Color {
    Color {
        red: left.red.saturating_add(right.red),
        green: left.green.saturating_add(right.green),
        blue: left.blue.saturating_add(right.blue),
    }
}

fn multiply_color(left: Color, right: Color) -> Color {
    Color {
        red: ((u16::from(left.red) * u16::from(right.red)) / 255) as u8,
        green: ((u16::from(left.green) * u16::from(right.green)) / 255) as u8,
        blue: ((u16::from(left.blue) * u16::from(right.blue)) / 255) as u8,
    }
}

fn invert_color(color: Color) -> Color {
    Color {
        red: 255 - color.red,
        green: 255 - color.green,
        blue: 255 - color.blue,
    }
}

fn scale_color(color: Color, amount: f64) -> Color {
    Color {
        red: scale_channel(color.red, amount),
        green: scale_channel(color.green, amount),
        blue: scale_channel(color.blue, amount),
    }
}

fn scale_channel(value: u8, amount: f64) -> u8 {
    (f64::from(value) * amount.clamp(0.0, 1.0)).round() as u8
}

fn intensity(color: Color) -> f64 {
    f64::from(color.red.max(color.green).max(color.blue)) / 255.0
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

fn color_param(params: &IndexMap<Identifier, Value>, name: &str) -> Result<Color, RenderError> {
    let name = Identifier::new(name.to_string()).map_err(|_| RenderError::BadGraph {
        message: format!("invalid operator parameter name `{name}`"),
    })?;
    params
        .get(&name)
        .and_then(|value| match value {
            Value::Color(value) => Some(*value),
            _ => None,
        })
        .ok_or_else(|| RenderError::BadGraph {
            message: format!("missing or invalid operator parameter `{}`", name.as_str()),
        })
}

fn black() -> Color {
    Color {
        red: 0,
        green: 0,
        blue: 0,
    }
}

#[derive(Clone, Debug)]
struct PreparedFixture {
    id: FixtureInstanceId,
    pixel_count: usize,
}

#[derive(Clone, Debug)]
struct PreparedTargetPixel {
    fixture_index: usize,
    fixture_pixel_index: usize,
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
    fixture_index: usize,
    fixture_pixel_index: usize,
}

#[derive(Clone, Debug)]
struct PreparedEffect {
    layer_id: SequenceLayerId,
    start_seconds: f64,
    duration_seconds: f64,
    target: Arc<Vec<PreparedTargetPixel>>,
    sample_groups: Option<Vec<PreparedSampleContextGroup>>,
    definition: dawn_language::effect_dsl::CompiledEffect,
    params: IndexMap<Identifier, Value>,
    bound_params: BoundEffectParams,
    automation: Vec<PreparedAutomation>,
}

#[derive(Clone, Debug)]
struct PreparedLayer {
    id: SequenceLayerId,
    enabled: bool,
    effects: Vec<usize>,
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
        layer_id: SequenceLayerId,
    },
    Operator {
        operator: GraphOperator,
        inputs: Vec<usize>,
        params: IndexMap<Identifier, Value>,
        automation: Vec<PreparedAutomation>,
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
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct PreparedTargetCacheKey {
    target: Vec<FixtureInstanceId>,
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

#[cfg(test)]
mod tests;
