use std::collections::BTreeMap;

use dawn_project::analysis::ProjectAnalysis;
use dawn_project::document::{
    SequenceDocument, SequenceEffectParamDocument, SequenceMarkCollectionDocument,
};
use dawn_project::effect_script::{
    BytecodeStats, CompiledEffect, EffectSampleScratch, FixtureContext, PixelContext,
    PreparedEffectParams, RuntimeError, RuntimeValue,
};
use dawn_project::model::{
    Color, Distance, DistanceSpan, EffectParam, FixtureId, Resolved, SequenceEffectScope,
};
use dawn_project::render::{layout_render_plan, GeometryRenderBounds, GeometryRenderPoint};

#[derive(Debug, Clone)]
pub struct OutputFrame {
    pub source: OutputSourceMetadata,
    pub time_seconds: f64,
    pub generation: u64,
    pub status: OutputFrameStatus,
    pub bounds: GeometryRenderBounds,
    pub fixtures: Vec<OutputFixtureFrame>,
}

#[derive(Debug, Clone)]
pub struct OutputSourceMetadata {
    pub label: String,
    pub kind: OutputSourceKind,
    pub duration_seconds: f64,
    pub fps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputSourceKind {
    Sequence,
    Empty,
}

#[derive(Debug, Clone)]
pub enum OutputFrameStatus {
    Live,
    Idle(String),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct OutputFixtureFrame {
    pub id: FixtureId,
    pub name: String,
    pub bulb_radius: DistanceSpan,
    pub pixels: Vec<OutputPixelFrame>,
}

#[derive(Debug, Clone)]
pub struct OutputPixelFrame {
    pub position: GeometryRenderPoint,
    pub color: Color,
}

pub trait OutputSink {
    fn write_frame(&self, frame: OutputFrame);
}

pub struct SequenceFrameEvaluator<'a> {
    source: OutputSourceMetadata,
    bounds: GeometryRenderBounds,
    fixture_templates: Vec<OutputFixtureFrame>,
    effects: Vec<PreparedSequenceEffect<'a>>,
}

impl<'a> SequenceFrameEvaluator<'a> {
    pub fn new(
        analysis: &'a ProjectAnalysis,
        document: &'a SequenceDocument,
    ) -> Result<Self, String> {
        let Some(project) = analysis.resolved.as_ref() else {
            return Err("Project must resolve before preview is available".to_string());
        };
        let render_plan = layout_render_plan(&project.display.layout.fixtures);
        let fixture_templates = render_plan
            .fixtures
            .iter()
            .zip(project.display.layout.fixtures.iter())
            .map(|(plan, fixture)| OutputFixtureFrame {
                id: fixture.id,
                name: fixture.name.clone(),
                bulb_radius: plan.bulb_radius,
                pixels: plan
                    .emitters
                    .iter()
                    .map(|position| OutputPixelFrame {
                        position: *position,
                        color: Color::new(0, 0, 0),
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();

        let effects = document
            .effects
            .iter()
            .filter_map(|effect| {
                let render = effect.render.as_ref()?;
                let render_plan = match analysis.compiled_script_for_key(&render.script_key) {
                    Some(script) => match prepare_params_from_document(
                        script,
                        &render.params,
                        &document.mark_collections,
                        effect.start_seconds,
                    ) {
                        Ok(prepared_params) => PreparedEffectRender::Ready {
                            script,
                            target_pixels: prepare_effect_pixels(
                                effect.scope,
                                &render.target_pixels,
                                &fixture_templates,
                            ),
                            prepared_params,
                            scratch: EffectSampleScratch::new(script.bytecode_stats()),
                            _bytecode_stats: script.bytecode_stats(),
                        },
                        Err(error) => PreparedEffectRender::BadParams(error),
                    },
                    None => PreparedEffectRender::MissingScript(render.script_key.clone()),
                };
                Some(PreparedSequenceEffect {
                    start_seconds: effect.start_seconds,
                    duration_seconds: effect.duration_seconds,
                    render: render_plan,
                })
            })
            .collect();

        Ok(Self {
            source: OutputSourceMetadata {
                label: format!("Sequence {}", document.object_key),
                kind: OutputSourceKind::Sequence,
                duration_seconds: document.duration_seconds,
                fps: document.frame_rate,
            },
            bounds: render_plan.bounds,
            fixture_templates,
            effects,
        })
    }

    pub fn evaluate(&mut self, time_seconds: f64, generation: u64) -> OutputFrame {
        let mut fixtures = self.fixture_templates.clone();
        let mut status = OutputFrameStatus::Live;

        for effect in &mut self.effects {
            let local_seconds = if time_seconds < effect.start_seconds
                || time_seconds >= effect.start_seconds + effect.duration_seconds
            {
                continue;
            } else {
                time_seconds - effect.start_seconds
            };
            let progress = if effect.duration_seconds == 0.0 {
                0.0
            } else {
                (local_seconds / effect.duration_seconds).clamp(0.0, 1.0)
            };

            let PreparedEffectRender::Ready {
                script,
                target_pixels,
                prepared_params,
                scratch,
                ..
            } = &mut effect.render
            else {
                status = effect.render.error_status();
                continue;
            };

            for pixel in target_pixels {
                let output_pixel = &mut fixtures[pixel.fixture_index].pixels[pixel.pixel_index];
                match script.sample_prepared_with_scratch(
                    progress,
                    local_seconds,
                    pixel.fixture_context,
                    pixel.pixel_context,
                    prepared_params,
                    scratch,
                ) {
                    Ok(color) => add_clamped(&mut output_pixel.color, color),
                    Err(error) => status = OutputFrameStatus::Error(error.to_string()),
                }
            }
        }

        OutputFrame {
            source: self.source.clone(),
            time_seconds,
            generation,
            status,
            bounds: self.bounds,
            fixtures,
        }
    }
}

struct PreparedSequenceEffect<'a> {
    start_seconds: f64,
    duration_seconds: f64,
    render: PreparedEffectRender<'a>,
}

enum PreparedEffectRender<'a> {
    Ready {
        script: &'a CompiledEffect,
        target_pixels: Vec<PreparedEffectPixel>,
        prepared_params: PreparedEffectParams,
        scratch: EffectSampleScratch,
        _bytecode_stats: BytecodeStats,
    },
    MissingScript(String),
    BadParams(RuntimeError),
}

struct PreparedEffectPixel {
    fixture_index: usize,
    pixel_index: usize,
    fixture_context: FixtureContext,
    pixel_context: PixelContext,
}

impl PreparedEffectRender<'_> {
    fn error_status(&self) -> OutputFrameStatus {
        match self {
            Self::Ready { .. } => OutputFrameStatus::Live,
            Self::MissingScript(script_key) => {
                OutputFrameStatus::Error(format!("compiled script `{script_key}` was not found"))
            }
            Self::BadParams(error) => OutputFrameStatus::Error(error.to_string()),
        }
    }
}

fn prepare_effect_pixels(
    scope: SequenceEffectScope,
    target_pixels: &[dawn_project::document::SequenceEffectPixelDocument],
    fixture_templates: &[OutputFixtureFrame],
) -> Vec<PreparedEffectPixel> {
    let target_pixel_count = target_pixels.len();
    target_pixels
        .iter()
        .enumerate()
        .filter_map(|(target_pixel_index, pixel)| {
            fixture_templates
                .get(pixel.fixture_index)?
                .pixels
                .get(pixel.pixel_index)?;
            Some(PreparedEffectPixel {
                fixture_index: pixel.fixture_index,
                pixel_index: pixel.pixel_index,
                fixture_context: FixtureContext {
                    index: pixel.fixture_index,
                },
                pixel_context: pixel_context_for_effect(
                    scope,
                    target_pixel_index,
                    target_pixel_count,
                    pixel.pixel_index,
                    pixel.pixel_count,
                ),
            })
        })
        .collect()
}

pub fn evaluate_sequence_frame(
    analysis: &ProjectAnalysis,
    document: &SequenceDocument,
    time_seconds: f64,
    generation: u64,
) -> OutputFrame {
    match SequenceFrameEvaluator::new(analysis, document) {
        Ok(mut evaluator) => evaluator.evaluate(time_seconds, generation),
        Err(message) => empty_frame(generation, message),
    }
}

pub fn pixel_context_for_effect(
    scope: SequenceEffectScope,
    target_pixel_index: usize,
    target_pixel_count: usize,
    fixture_pixel_index: usize,
    fixture_pixel_count: usize,
) -> PixelContext {
    match scope {
        SequenceEffectScope::PerFixture => PixelContext {
            index: fixture_pixel_index,
            count: fixture_pixel_count,
        },
        SequenceEffectScope::WholeTarget => PixelContext {
            index: target_pixel_index,
            count: target_pixel_count,
        },
    }
}

fn add_clamped(target: &mut Color, color: Color) {
    target.red = target.red.saturating_add(color.red);
    target.green = target.green.saturating_add(color.green);
    target.blue = target.blue.saturating_add(color.blue);
}

pub fn runtime_params_from_document(
    params: &[SequenceEffectParamDocument],
    mark_collections: &[SequenceMarkCollectionDocument],
    effect_start_seconds: f64,
) -> BTreeMap<String, RuntimeValue> {
    params
        .iter()
        .filter_map(|param| {
            runtime_value_from_param(&param.value, mark_collections, effect_start_seconds)
                .map(|value| (param.name.clone(), value))
        })
        .collect()
}

pub fn prepare_params_from_document(
    script: &CompiledEffect,
    params: &[SequenceEffectParamDocument],
    mark_collections: &[SequenceMarkCollectionDocument],
    effect_start_seconds: f64,
) -> Result<PreparedEffectParams, RuntimeError> {
    script.prepare_params_with(|name| {
        params
            .iter()
            .find(|param| param.name == name)
            .and_then(|param| {
                runtime_value_from_param(&param.value, mark_collections, effect_start_seconds)
            })
    })
}

pub fn runtime_value_from_param(
    param: &EffectParam<Resolved>,
    mark_collections: &[SequenceMarkCollectionDocument],
    effect_start_seconds: f64,
) -> Option<RuntimeValue> {
    match param {
        EffectParam::Integer { value } => Some(RuntimeValue::Int(*value as i64)),
        EffectParam::Float { value } => Some(RuntimeValue::Float(*value)),
        EffectParam::Boolean { value } => Some(RuntimeValue::Bool(*value)),
        EffectParam::Enum { value } => Some(RuntimeValue::Enum(value.clone())),
        EffectParam::Flags { value } => Some(RuntimeValue::Flags(value.clone())),
        EffectParam::Color { value } => Some(RuntimeValue::Color(*value)),
        EffectParam::Curve { curve } => Some(RuntimeValue::Curve(curve.clone())),
        EffectParam::Marks { key } => {
            let mut marks = mark_collections
                .iter()
                .find(|collection| collection.key == *key)?
                .marks_seconds
                .iter()
                .map(|mark_seconds| *mark_seconds - effect_start_seconds)
                .collect::<Vec<_>>();
            marks.sort_by(f64::total_cmp);
            Some(RuntimeValue::Marks(marks))
        }
    }
}

pub fn empty_frame(generation: u64, message: impl Into<String>) -> OutputFrame {
    OutputFrame {
        source: OutputSourceMetadata {
            label: "No preview source".to_string(),
            kind: OutputSourceKind::Empty,
            duration_seconds: 0.0,
            fps: 0,
        },
        time_seconds: 0.0,
        generation,
        status: OutputFrameStatus::Idle(message.into()),
        bounds: GeometryRenderBounds {
            min_x: Distance::from_micrometers(-5_000_000),
            min_y: Distance::from_micrometers(-4_000_000),
            max_x: Distance::from_micrometers(5_000_000),
            max_y: Distance::from_micrometers(4_000_000),
        },
        fixtures: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use dawn_project::model::SequenceEffectScope;

    use super::pixel_context_for_effect;

    #[test]
    fn per_fixture_scope_repeats_pixel_context_for_group_members() {
        let contexts = [
            pixel_context_for_effect(SequenceEffectScope::PerFixture, 0, 5, 0, 2),
            pixel_context_for_effect(SequenceEffectScope::PerFixture, 1, 5, 1, 2),
            pixel_context_for_effect(SequenceEffectScope::PerFixture, 2, 5, 0, 3),
            pixel_context_for_effect(SequenceEffectScope::PerFixture, 3, 5, 1, 3),
            pixel_context_for_effect(SequenceEffectScope::PerFixture, 4, 5, 2, 3),
        ];

        assert_eq!(
            contexts.map(|context| (context.index, context.count)),
            [(0, 2), (1, 2), (0, 3), (1, 3), (2, 3)]
        );
    }

    #[test]
    fn whole_target_scope_uses_continuous_group_pixel_context() {
        let contexts = [
            pixel_context_for_effect(SequenceEffectScope::WholeTarget, 0, 5, 0, 2),
            pixel_context_for_effect(SequenceEffectScope::WholeTarget, 1, 5, 1, 2),
            pixel_context_for_effect(SequenceEffectScope::WholeTarget, 2, 5, 0, 3),
            pixel_context_for_effect(SequenceEffectScope::WholeTarget, 3, 5, 1, 3),
            pixel_context_for_effect(SequenceEffectScope::WholeTarget, 4, 5, 2, 3),
        ];

        assert_eq!(
            contexts.map(|context| (context.index, context.count)),
            [(0, 5), (1, 5), (2, 5), (3, 5), (4, 5)]
        );
    }

    #[test]
    fn fixture_target_context_matches_for_both_scopes() {
        let per_fixture = pixel_context_for_effect(SequenceEffectScope::PerFixture, 1, 3, 1, 3);
        let whole_target = pixel_context_for_effect(SequenceEffectScope::WholeTarget, 1, 3, 1, 3);

        assert_eq!(
            (per_fixture.index, per_fixture.count),
            (whole_target.index, whole_target.count)
        );
    }
}
