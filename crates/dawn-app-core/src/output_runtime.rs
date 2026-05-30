use std::collections::BTreeMap;

use dawn_project::analysis::ProjectAnalysis;
use dawn_project::document::{
    SequenceDocument, SequenceEffectParamDocument, SequenceMarkCollectionDocument,
};
use dawn_project::effect_script::{FixtureContext, PixelContext, RuntimeValue};
use dawn_project::model::{Color, EffectParam, FixtureId, Resolved, SequenceEffectScope};
use dawn_project::render::{layout_render_plan, GeometryRenderBounds, GeometryRenderPoint};

#[derive(Debug, Clone)]
pub struct OutputFrame {
    pub source: OutputSourceMetadata,
    pub time_ms: u64,
    pub generation: u64,
    pub status: OutputFrameStatus,
    pub bounds: GeometryRenderBounds,
    pub fixtures: Vec<OutputFixtureFrame>,
}

#[derive(Debug, Clone)]
pub struct OutputSourceMetadata {
    pub label: String,
    pub kind: OutputSourceKind,
    pub duration_ms: u64,
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
    pub bulb_radius: f64,
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

pub fn evaluate_sequence_frame(
    analysis: &ProjectAnalysis,
    document: &SequenceDocument,
    time_ms: u64,
    generation: u64,
) -> OutputFrame {
    let Some(project) = analysis.resolved.as_ref() else {
        return empty_frame(
            generation,
            "Project must resolve before preview is available",
        );
    };
    let render_plan = layout_render_plan(&project.display.layout.fixtures);
    let mut fixtures = render_plan
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

    let mut status = OutputFrameStatus::Live;
    for effect in &document.effects {
        let Some(render) = &effect.render else {
            continue;
        };
        let local_ms = if time_ms < effect.start_ms
            || time_ms >= effect.start_ms.saturating_add(effect.duration_ms)
        {
            continue;
        } else {
            time_ms.saturating_sub(effect.start_ms)
        };
        let progress = if effect.duration_ms == 0 {
            0.0
        } else {
            (local_ms as f64 / effect.duration_ms as f64).clamp(0.0, 1.0)
        };
        let params = runtime_params_from_document(
            &render.params,
            &document.mark_collections,
            effect.start_ms,
        );
        let target_pixel_count = render.target_pixels.len();
        for (target_pixel_index, pixel) in render.target_pixels.iter().enumerate() {
            let Some(fixture) = fixtures.get_mut(pixel.fixture_index) else {
                continue;
            };
            let Some(output_pixel) = fixture.pixels.get_mut(pixel.pixel_index) else {
                continue;
            };
            let pixel_context = pixel_context_for_effect(
                effect.scope,
                target_pixel_index,
                target_pixel_count,
                pixel.pixel_index,
                pixel.pixel_count,
            );
            match analysis.sample_effect_script_key(
                &render.script_key,
                progress,
                local_ms as f64 / 1_000.0,
                FixtureContext {
                    index: pixel.fixture_index,
                },
                pixel_context,
                params.clone(),
            ) {
                Ok(color) => add_clamped(&mut output_pixel.color, color),
                Err(error) => status = OutputFrameStatus::Error(error),
            }
        }
    }

    OutputFrame {
        source: OutputSourceMetadata {
            label: format!("Sequence {}", document.object_key),
            kind: OutputSourceKind::Sequence,
            duration_ms: document.duration_ms,
            fps: document.frame_rate,
        },
        time_ms,
        generation,
        status,
        bounds: render_plan.bounds,
        fixtures,
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
    effect_start_ms: u64,
) -> BTreeMap<String, RuntimeValue> {
    params
        .iter()
        .filter_map(|param| {
            runtime_value_from_param(&param.value, mark_collections, effect_start_ms)
                .map(|value| (param.name.clone(), value))
        })
        .collect()
}

pub fn runtime_value_from_param(
    param: &EffectParam<Resolved>,
    mark_collections: &[SequenceMarkCollectionDocument],
    effect_start_ms: u64,
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
                .marks_ms
                .iter()
                .map(|mark_ms| (*mark_ms as i128 - effect_start_ms as i128) as f64 / 1_000.0)
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
            duration_ms: 0,
            fps: 0,
        },
        time_ms: 0,
        generation,
        status: OutputFrameStatus::Idle(message.into()),
        bounds: GeometryRenderBounds {
            min_x: -5.0,
            min_y: -4.0,
            max_x: 5.0,
            max_y: 4.0,
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
