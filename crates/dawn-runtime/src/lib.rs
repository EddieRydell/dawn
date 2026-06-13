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
use dawn_language::sequence::{MarkCollectionKey, Sequence, SequenceId};
use dawn_language::setup::{
    FixtureGroupId, FixtureInst, FixtureInstanceId, Geometry, Layout, SetupId,
};
use dawn_language::values::{Color, DawnTime, Marks};
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
    effects_by_frame: Vec<Vec<usize>>,
}

#[derive(Clone, Debug)]
pub struct PreparedEffectRasterRenderer {
    frame_rate: u32,
    frame_count: u64,
    start_seconds: f64,
    duration_seconds: f64,
    target: Arc<Vec<PreparedTargetPixel>>,
    target_lookup: HashMap<TargetColorAddress, Vec<usize>>,
    effects: Vec<PreparedEffect>,
    effects_by_frame: Vec<Vec<usize>>,
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
    UnsupportedAutomation,
    MissingSetup { setup_id: SetupId },
    MissingLayout,
    MissingSequence { sequence_id: SequenceId },
    MissingFixture { fixture_id: FixtureInstanceId },
    MissingFixtureDefinition,
    MissingEffect { effect_id: EffectDefinitionId },
    MissingCurve,
    MissingMarkCollection { key: MarkCollectionKey },
    BadTarget,
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
        if !sequence.automation_clips.is_empty() {
            return Err(RenderError::UnsupportedAutomation);
        }

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
        for effect in &sequence.effects {
            prepare_effect(
                PrepareEffectContext {
                    project,
                    sequence,
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
        Ok(Self {
            frame_rate,
            frame_count,
            fixtures,
            effects,
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
        let mut rendered = self
            .fixtures
            .iter()
            .map(|fixture| RenderedFixture {
                fixture_id: fixture.id.clone(),
                pixels: vec![black(); fixture.pixel_count],
            })
            .collect::<Vec<_>>();

        if let Some(active_effects) = self.effects_by_frame.get(frame_index as usize) {
            for effect_index in active_effects {
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

    fn render_target_colors_at(&self, frame_index: u64) -> Result<Vec<Color>, RenderError> {
        let sample_seconds = frame_index as f64 / f64::from(self.frame_rate);
        let mut rendered = vec![black(); self.target.len()];

        if let Some(active_effects) = self.effects_by_frame.get(frame_index as usize) {
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
        if !sequence.automation_clips.is_empty() {
            return Err(RenderError::UnsupportedAutomation);
        }

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

    pub fn prepare_effect(
        &mut self,
        effect_id: &EffectInstId,
    ) -> Result<PreparedEffectRasterRenderer, RenderError> {
        let effect = self
            .sequence
            .effects
            .iter()
            .find(|effect| &effect.id == effect_id)
            .ok_or_else(|| RenderError::MissingEffect {
                effect_id: EffectDefinitionId(effect_id.0.to_string()),
            })?;
        let mut effects = Vec::new();
        let mut generated_child_count = 0usize;
        let target = prepare_effect(
            PrepareEffectContext {
                project: self.project,
                sequence: self.sequence,
                fixtures: &self.fixtures,
                fixture_ids: &self.fixture_ids,
                groups: &self.groups,
                effects: &mut effects,
                generated_child_count: &mut generated_child_count,
                bind_cache: &mut self.bind_cache,
                target_cache: &mut self.target_cache,
            },
            effect,
        )?;

        let effects_by_frame =
            build_effect_frame_index(&effects, self.frame_count, self.frame_rate);
        Ok(PreparedEffectRasterRenderer {
            frame_rate: self.frame_rate,
            frame_count: self.frame_count,
            start_seconds: effect.start.as_seconds_f64(),
            duration_seconds: effect.duration.as_seconds_f64(),
            target_lookup: target_color_lookup(&target),
            target,
            effects,
            effects_by_frame,
        })
    }
}

struct PrepareEffectContext<'a> {
    project: &'a DawnProject,
    sequence: &'a Sequence,
    fixtures: &'a [PreparedFixture],
    fixture_ids: &'a IndexSet<FixtureInstanceId>,
    groups: &'a IndexMap<FixtureGroupId, Vec<FixtureInstanceId>>,
    effects: &'a mut Vec<PreparedEffect>,
    generated_child_count: &'a mut usize,
    bind_cache: &'a mut EffectBindCache,
    target_cache: &'a mut PrepareTargetCache,
}

fn prepare_effect(
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
        .get(&effect.definition)
        .ok_or_else(|| RenderError::MissingEffect {
            effect_id: effect.definition.clone(),
        })?;
    let target_ids = prepare_target(&effect.target, context.fixture_ids, context.groups)?;
    let target = prepare_target_pixels_cached(
        context.target_cache,
        &target_ids,
        context.fixtures,
        &effect.scope,
    )?;
    let start_seconds = effect.start.as_seconds_f64();
    let param_timing = EffectParamTiming {
        start_seconds,
        duration_seconds: effect_duration_seconds,
    };
    let params = prepare_params(
        context.project,
        context.sequence,
        &effect.param_overrides,
        param_timing,
    )?;
    let bound_params = definition
        .compiled
        .bind_params_cached(&params, context.bind_cache);
    match definition.compiled.kind() {
        EffectKind::Sample => context.effects.push(PreparedEffect {
            start_seconds,
            duration_seconds: effect_duration_seconds,
            target: Arc::clone(&target),
            definition: definition.compiled.clone(),
            params: bound_params,
        }),
        EffectKind::Generator => {
            let mut generator_context = GeneratorPrepareContext {
                project: context.project,
                fixtures: context.fixtures,
                effects: context.effects,
                generated_child_count: context.generated_child_count,
                bind_cache: context.bind_cache,
                target_cache: context.target_cache,
            };
            for expansion_target in generator_expansion_targets(&target, &effect.scope) {
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
                start_seconds,
                duration_seconds,
                target,
                definition: definition.compiled.clone(),
                params: bound_params,
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
    if let Some(entry) = cache.generator_context_targets.get(&key) {
        if Arc::ptr_eq(&entry.source, prepared_target) {
            return Arc::clone(&entry.target);
        }
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
    if let Some(entry) = cache.generated_targets.get(&key) {
        if Arc::ptr_eq(&entry.source, &target) {
            return Ok(Arc::clone(&entry.pixels));
        }
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
    let mut index = vec![Vec::new(); frame_count as usize];
    for (effect_index, effect) in effects.iter().enumerate() {
        let start_frame = (effect.start_seconds * f64::from(frame_rate))
            .floor()
            .max(0.0) as u64;
        let end_frame = ((effect.start_seconds + effect.duration_seconds) * f64::from(frame_rate))
            .ceil() as u64;
        for frame in start_frame..end_frame.min(frame_count) {
            if let Some(bucket) = index.get_mut(frame as usize) {
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

    for pixel in effect.target.iter() {
        let context = RunContext {
            progress,
            seconds: local_seconds,
            duration: effect.duration_seconds,
            pixel_index: pixel.pixel_index as i64,
            pixel_count: pixel.pixel_count as i64,
            pixel_fraction: pixel.pixel_fraction,
            global_marks: Marks { marks: Vec::new() },
        };
        let color = effect
            .definition
            .sample_bound(&effect.params, &context, &mut scratch)?;
        compose_max(
            &mut rendered[pixel.fixture_index].pixels[pixel.fixture_pixel_index],
            color,
        );
    }
    Ok(())
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

    for pixel in effect.target.iter() {
        let Some(target_indexes) = target_lookup.get(&TargetColorAddress {
            fixture_index: pixel.fixture_index,
            fixture_pixel_index: pixel.fixture_pixel_index,
        }) else {
            continue;
        };
        let context = RunContext {
            progress,
            seconds: local_seconds,
            duration: effect.duration_seconds,
            pixel_index: pixel.pixel_index as i64,
            pixel_count: pixel.pixel_count as i64,
            pixel_fraction: pixel.pixel_fraction,
            global_marks: Marks { marks: Vec::new() },
        };
        let color = effect
            .definition
            .sample_bound(&effect.params, &context, &mut scratch)?;
        for target_index in target_indexes {
            if let Some(target) = rendered.get_mut(*target_index) {
                compose_max(target, color);
            }
        }
    }
    Ok(())
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

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct TargetColorAddress {
    fixture_index: usize,
    fixture_pixel_index: usize,
}

#[derive(Clone, Debug)]
struct PreparedEffect {
    start_seconds: f64,
    duration_seconds: f64,
    target: Arc<Vec<PreparedTargetPixel>>,
    definition: dawn_language::effect_dsl::CompiledEffect,
    params: BoundEffectParams,
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
