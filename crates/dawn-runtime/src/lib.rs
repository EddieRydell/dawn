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
    CurveSource, EffectDefinitionId, EffectParamValue, EffectScope, EffectTarget,
};
use dawn_language::effect_dsl::{
    BoundEffectParams, EffectVmScratch, Identifier, RunContext, RuntimeError, Value,
};
use dawn_language::model::DawnProject;
use dawn_language::sequence::{MarkCollectionKey, Sequence, SequenceId};
use dawn_language::setup::{
    FixtureGroupId, FixtureInst, FixtureInstanceId, Geometry, Layout, SetupId,
};
use dawn_language::values::{Color, Marks};
use indexmap::{IndexMap, IndexSet};

#[derive(Clone, Debug)]
pub struct PreparedSequenceRenderer {
    frame_rate: u32,
    frame_count: u64,
    fixtures: Vec<PreparedFixture>,
    effects: Vec<PreparedEffect>,
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
        for effect in &sequence.effects {
            let effect_duration_seconds = effect.duration.as_seconds_f64();
            if !effect_duration_seconds.is_finite() || effect_duration_seconds <= 0.0 {
                return Err(RenderError::InvalidTiming {
                    reason: "effect duration must be positive and finite".to_string(),
                });
            }
            let definition = project
                .definitions
                .effects
                .get(&effect.definition)
                .ok_or_else(|| RenderError::MissingEffect {
                    effect_id: effect.definition.clone(),
                })?;
            let target_ids = prepare_target(&effect.target, &fixture_ids, &groups)?;
            let target = prepare_target_indexes(&target_ids, &fixtures)?;
            let total_target_pixels = target
                .iter()
                .map(|index| fixtures[*index].pixel_count)
                .sum::<usize>();
            let params = prepare_params(project, sequence, &effect.param_overrides)?;
            let bound_params = definition.compiled.bind_params(&params);
            effects.push(PreparedEffect {
                start_seconds: effect.start.as_seconds_f64(),
                duration_seconds: effect_duration_seconds,
                target,
                total_target_pixels,
                scope: effect.scope.clone(),
                definition: definition.compiled.clone(),
                params: bound_params,
            });
        }

        let frame_rate = sequence.frame_rate;
        let duration_seconds = sequence.duration.as_seconds_f64();
        Ok(Self {
            frame_rate,
            frame_count: frame_count(duration_seconds, frame_rate),
            fixtures,
            effects,
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

        for effect in &self.effects {
            if sample_seconds < effect.start_seconds
                || sample_seconds >= effect.start_seconds + effect.duration_seconds
            {
                continue;
            }
            render_effect(effect, &self.fixtures, &mut rendered, sample_seconds)?;
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

fn prepare_params(
    project: &DawnProject,
    sequence: &Sequence,
    overrides: &IndexMap<Identifier, EffectParamValue>,
) -> Result<IndexMap<Identifier, Value>, RenderError> {
    overrides
        .iter()
        .map(|(key, value)| Ok((key.clone(), prepare_param_value(project, sequence, value)?)))
        .collect()
}

fn prepare_param_value(
    project: &DawnProject,
    sequence: &Sequence,
    value: &EffectParamValue,
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
            Ok(Value::Marks(Marks {
                marks: collection.marks.clone(),
            }))
        }
        EffectParamValue::Curve(source) => Ok(Value::Curve(match source {
            CurveSource::Inline(curve) => curve.clone(),
            CurveSource::Reference(id) => project
                .definitions
                .curves
                .get(id)
                .ok_or(RenderError::MissingCurve)?
                .curve
                .clone(),
        })),
        EffectParamValue::Array(values) => values
            .iter()
            .map(|value| prepare_param_value(project, sequence, value))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
    }
}

fn render_effect(
    effect: &PreparedEffect,
    fixtures: &[PreparedFixture],
    rendered: &mut [RenderedFixture],
    sample_seconds: f64,
) -> Result<(), RenderError> {
    let local_seconds = sample_seconds - effect.start_seconds;
    let progress = (local_seconds / effect.duration_seconds).clamp(0.0, 1.0);
    let mut whole_index = 0usize;
    let mut scratch = EffectVmScratch::default();

    for fixture_index in effect.target.iter().copied() {
        let fixture_pixel_count = fixtures[fixture_index].pixel_count;
        for fixture_pixel_index in 0..fixture_pixel_count {
            let (pixel_index, pixel_count) = match effect.scope {
                EffectScope::PerFixture => (fixture_pixel_index, fixture_pixel_count),
                EffectScope::WholeTarget => (whole_index, effect.total_target_pixels),
            };
            let context = RunContext {
                progress,
                seconds: local_seconds,
                duration: effect.duration_seconds,
                pixel_index: pixel_index as i64,
                pixel_count: pixel_count as i64,
                pixel_fraction: pixel_fraction(pixel_index, pixel_count),
                global_marks: Marks { marks: Vec::new() },
            };
            let color = effect
                .definition
                .sample_bound(&effect.params, &context, &mut scratch)?;
            compose_max(
                &mut rendered[fixture_index].pixels[fixture_pixel_index],
                color,
            );
            whole_index += 1;
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
struct PreparedEffect {
    start_seconds: f64,
    duration_seconds: f64,
    target: Vec<usize>,
    total_target_pixels: usize,
    scope: EffectScope,
    definition: dawn_language::effect_dsl::CompiledEffect,
    params: BoundEffectParams,
}

#[cfg(test)]
mod tests;
